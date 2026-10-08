// The session: the one long-lived X client. It shows the splash while YARG starts, stays out of the way
// while YARG runs, and shows the dashboard when YARG exits ("Exit" in YARG's menu). PLAY YARG starts it again.

use crate::event::Event;
use crate::gfx::{Canvas, Fonts};
use crate::input;
use crate::logo::draw_splash;
use crate::model::*;
use crate::sys;
use crate::ui::{self, Hit};
use crate::x::Display;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long YARG must stay up before the version that is "on trial" is marked good.
const GOOD_AFTER_SECS: u64 = 60;

fn yarg_command() -> String {
    std::env::var("YARGOS_YARG_CMD").unwrap_or_else(|_| "/usr/lib/yargos/run-yarg".into())
}

fn updater(arg: &str) -> Option<String> {
    Command::new("yargos-update").arg(arg).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).to_string())
}

/// Start YARG. Before: let the updater count this launch (after repeated failed launches of a new version
/// it rolls back first). While running: after GOOD_AFTER_SECS the new version is marked good.
fn launch_yarg(tx: Sender<Event>) {
    let cmd = yarg_command();
    std::thread::spawn(move || {
        if let Some(out) = updater("boot-check") {
            if out.contains("rolling back") {
                let _ = tx.send(Event::Notice("The last YARG update did not work, so the previous version was restored".into()));
            }
        }
        let start = Instant::now();
        let mut child = match Command::new(&cmd).spawn() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("yargos-home: cannot start {cmd}: {e}");
                let _ = tx.send(Event::YargExited { code: Some(127), secs: 0 });
                return;
            }
        };
        let mut marked = false;
        let code = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status.code(),
                Ok(None) => {
                    if !marked && start.elapsed() >= Duration::from_secs(GOOD_AFTER_SECS) {
                        marked = true;
                        updater("mark-good");
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
                Err(_) => break None,
            }
        };
        let _ = tx.send(Event::YargExited { code, secs: start.elapsed().as_secs() });
    });
}

fn bg(tx: &Sender<Event>, job: impl FnOnce(&Sender<Event>) + Send + 'static) {
    let tx = tx.clone();
    std::thread::spawn(move || job(&tx));
}

fn res(tx: &Sender<Event>, r: Res) {
    let _ = tx.send(Event::Res(r));
}

fn done(tx: &Sender<Event>, r: Result<String, String>) {
    res(tx, Res::Done(match r { Ok(m) | Err(m) => m }));
}

/// Run the commands a screen asked for in the background; results come back as events.
fn exec(cmds: Vec<Cmd>, m: &Model, tx: &Sender<Event>) {
    for cmd in cmds {
        let upd = m.upd.clone();
        match cmd {
            Cmd::LaunchYarg => {} // handled by the loop
            Cmd::Refresh(Screen::Home) => bg(tx, move |tx| {
                let label = sys::sound_label();
                res(tx, Res::Status(sys::status(&label, &upd)));
            }),
            Cmd::Refresh(Screen::Wifi) => bg(tx, |tx| res(tx, sys::wifi_refresh(false))),
            Cmd::WifiScan => bg(tx, |tx| res(tx, sys::wifi_refresh(true))),
            Cmd::WifiConnect { ssid, pass } => bg(tx, move |tx| {
                done(tx, sys::wifi_connect(&ssid, pass.as_deref()));
                res(tx, sys::wifi_refresh(false));
            }),
            Cmd::Refresh(Screen::Sound) => bg(tx, |tx| res(tx, sys::sound_refresh())),
            Cmd::SoundSelect { card, dev } => bg(tx, move |tx| {
                done(tx, sys::sound_select(card, dev));
                res(tx, sys::sound_refresh());
            }),
            Cmd::SoundVolume { card, pct } => bg(tx, move |_| sys::sound_volume(card, pct)),
            Cmd::SoundTest => bg(tx, |tx| done(tx, sys::sound_test())),
            Cmd::Refresh(Screen::Bluetooth) => bg(tx, |tx| res(tx, sys::bt_list())),
            Cmd::BtScan => bg(tx, |tx| res(tx, sys::bt_scan())),
            Cmd::BtConnect(d) => bg(tx, move |tx| {
                done(tx, sys::bt_connect(&d));
                res(tx, sys::bt_list());
            }),
            Cmd::BtForget(mac) => bg(tx, move |tx| {
                done(tx, sys::bt_forget(&mac));
                res(tx, sys::bt_list());
            }),
            Cmd::Refresh(Screen::Updates) => bg(tx, move |tx| res(tx, Res::Upd(sys::upd_info(&upd)))),
            Cmd::UpdCheck => bg(tx, move |tx| res(tx, Res::Upd(sys::upd_check(&upd)))),
            Cmd::UpdApply => bg(tx, move |tx| {
                done(tx, sys::upd_apply());
                let mut u = sys::upd_info(&upd);
                u.available = false;
                res(tx, Res::Upd(u));
            }),
            Cmd::UpdRollback => bg(tx, move |tx| {
                done(tx, sys::upd_rollback());
                res(tx, Res::Upd(sys::upd_info(&upd)));
            }),
            Cmd::UpdAuto(on) => bg(tx, move |tx| done(tx, sys::write_auto_update(on))),
            Cmd::Refresh(Screen::Songs) | Cmd::SongsRefresh => bg(tx, |tx| res(tx, Res::Songs(sys::songs_refresh()))),
            Cmd::Refresh(Screen::System) => bg(tx, |tx| res(tx, Res::SysInfo(sys::sys_info()))),
            Cmd::Refresh(_) => {}
            Cmd::Power(p) => bg(tx, move |tx| {
                if let Err(e) = sys::power(p) {
                    done(tx, Err(format!("Could not do that: {e}")));
                }
            }),
        }
    }
}

enum Mode {
    Game(Instant),
    Dash,
}

fn splash_progress(since: Instant) -> f32 {
    1.0 - (-(since.elapsed().as_secs_f32()) / 10.0).exp() * 0.9 - 0.1
}

/// Without fonts there is no UI: just run YARG the old way.
fn run_plain() -> i32 {
    match Command::new(yarg_command()).status() {
        Ok(s) if s.success() => 0,
        _ => 1,
    }
}

pub fn run() -> i32 {
    let Some(mut fonts) = Fonts::load() else {
        eprintln!("yargos-home: fonts not found, starting YARG without the dashboard");
        return run_plain();
    };
    let disp = match Display::open() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("yargos-home: {e}");
            return 1;
        }
    };
    let mut cv = Canvas::new(disp.w, disp.h);
    let (tx, rx) = channel::<Event>();
    disp.spawn_events(tx.clone());
    let active = Arc::new(AtomicBool::new(false));
    input::spawn_pads(tx.clone(), active.clone());

    let mut model = Model::new();
    let mut hits: Vec<Hit> = Vec::new();
    let mut mode = Mode::Dash;
    let mut visible = true;
    let mut dirty = true;
    let mut had_toast = false;
    let mut notice: Option<String> = None;

    // Boot to the dashboard: YARG starts when PLAY YARG is chosen. The cursor only shows once a mouse moves.
    active.store(true, Ordering::Relaxed);
    disp.show_cursor(false);
    disp.focus();
    exec(vec![Cmd::Refresh(Screen::Home)], &model, &tx);

    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Event::YargExited { code, .. }) => {
                mode = Mode::Dash;
                active.store(true, Ordering::Relaxed);
                model = Model::new();
                if let Some(n) = notice.take() {
                    model.toast(n);
                } else if code != Some(0) {
                    model.toast(format!("YARG closed unexpectedly ({})", code.map(|c| format!("code {c}")).unwrap_or_else(|| "it was stopped".into())));
                }
                disp.show_cursor(false);
                disp.focus();
                visible = true;
                exec(vec![Cmd::Refresh(Screen::Home)], &model, &tx);
                dirty = true;
            }
            Ok(Event::Notice(n)) => notice = Some(n),
            Ok(Event::Nav(n)) if matches!(mode, Mode::Dash) => {
                disp.show_cursor(false);
                let cmds = model.handle(n);
                dirty = true;
                if cmds.contains(&Cmd::LaunchYarg) {
                    mode = Mode::Game(Instant::now());
                    active.store(false, Ordering::Relaxed);
                    disp.show_cursor(false);
                    launch_yarg(tx.clone());
                } else {
                    exec(cmds, &model, &tx);
                }
            }
            Ok(Event::Click(x, y)) if matches!(mode, Mode::Dash) => {
                if let Some(id) = ui::hit_test(&hits, x, y) {
                    let cmds = model.click(id);
                    dirty = true;
                    if cmds.contains(&Cmd::LaunchYarg) {
                        mode = Mode::Game(Instant::now());
                        active.store(false, Ordering::Relaxed);
                        disp.show_cursor(false);
                        launch_yarg(tx.clone());
                    } else {
                        exec(cmds, &model, &tx);
                    }
                }
            }
            Ok(Event::Hover(x, y)) if matches!(mode, Mode::Dash) => {
                disp.show_cursor(true);
                if let Some(id) = ui::hit_test(&hits, x, y) {
                    let before = (model.sel, model.confirm_sel, model.osk.row, model.osk.col);
                    model.hover(id);
                    dirty |= before != (model.sel, model.confirm_sel, model.osk.row, model.osk.col);
                }
            }
            Ok(Event::Res(r)) => {
                model.apply(r);
                dirty = true;
            }
            Ok(Event::Visible(v)) => {
                visible = v;
                dirty |= v;
            }
            Ok(Event::Redraw) => dirty = true,
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => {
                model.tick += 1;
                match mode {
                    Mode::Dash => {
                        let toast = model.live_toast().is_some();
                        dirty |= model.busy.is_some() || toast || had_toast || model.screen == Screen::Password && model.tick % 5 == 0;
                        had_toast = toast;
                    }
                    Mode::Game(since) => dirty |= visible && model.tick % 3 == 0 && since.elapsed() < Duration::from_secs(60),
                }
            }
            Err(RecvTimeoutError::Disconnected) => return 1,
        }

        if dirty && visible {
            match mode {
                Mode::Game(since) => draw_splash(&mut cv, &mut fonts, "Starting YARG\u{2026}", Some(splash_progress(since))),
                Mode::Dash => ui::draw(&mut cv, &mut fonts, &model, &mut hits),
            }
            disp.present(&cv);
            dirty = false;
        }
    }
}
