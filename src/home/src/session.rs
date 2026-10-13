// The session: the one long-lived X client. It shows the splash while YARG starts, stays out of the way
// while YARG runs, and shows the dashboard when YARG exits ("Exit" in YARG's menu). PLAY YARG starts it again.

use crate::event::Event;
use crate::event::VolKey;
use crate::gfx::{rgb, Canvas, Fonts};
use crate::input;
use crate::logo::draw_splash;
use crate::model::*;
use crate::sys;
use crate::ui::{self, Hit};
use crate::x::Display;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use crate::songcopy::Stop;
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
        sys::ensure_sound();
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

/// Connect Wii guitars without the Bluetooth screen: while the dashboard is up, anything that calls (1+2 held) is
/// connected, and a failed attempt is simply tried again the next time it calls. Idle while YARG runs: scanning would
/// add latency to the controllers that are playing.
fn spawn_wii_auto(tx: Sender<Event>, active: Arc<AtomicBool>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        if !active.load(Ordering::Relaxed) || !sys::bt_present() {
            std::thread::sleep(Duration::from_secs(2));
            continue;
        }
        match sys::wii_auto_step() {
            sys::WiiAuto::Nothing => {}
            sys::WiiAuto::Connected(m) => {
                res(&tx, Res::Done(m));
                res(&tx, sys::bt_list());
            }
            sys::WiiAuto::Failed(e) => {
                res(&tx, Res::Done(e));
                res(&tx, sys::bt_list());
                std::thread::sleep(Duration::from_secs(3));
            }
        }
    });
}

/// Watch the Wii guitars' batteries (the bridge writes them): tell the main loop once when one runs low. Reading a few
/// small files is all this does: it never talks to the guitars.
fn spawn_battery_watch(tx: Sender<Event>) {
    std::thread::spawn(move || {
        let mut announced = std::collections::HashMap::new();
        loop {
            for line in sys::newly_low(&mut announced, &sys::wii_batteries()) {
                if tx.send(Event::LowBattery(line)).is_err() {
                    return;
                }
            }
            std::thread::sleep(Duration::from_secs(10));
        }
    });
}

/// A notice drawn over everything (also the game): a low battery, or the volume after a volume key.
struct Banner {
    text: String,
    until: Instant,
    accent: crate::gfx::Rgb,
    /// A level bar under the text (the volume), 0..=100.
    level: Option<u8>,
}

fn banner_canvas(fonts: &mut Fonts, screen_h: usize, screen_w: usize, b: &Banner) -> Canvas {
    let u = screen_h as f32 / 720.0;
    let w = ((fonts.width(&b.text, 28.0 * u, true) + 110.0 * u) as usize).clamp((360.0 * u) as usize, screen_w);
    let h = if b.level.is_some() { 92.0 } else { 72.0 };
    let mut cv = Canvas::new(w, (h * u) as usize);
    cv.rect(0, 0, w as i32, cv.h as i32, rgb(10, 16, 30), 1.0);
    cv.rect(0, 0, (10.0 * u) as i32, cv.h as i32, b.accent, 1.0);
    fonts.draw(&mut cv, &b.text, 40.0 * u, 16.0 * u, 28.0 * u, true, rgb(255, 255, 255), 1.0);
    if let Some(p) = b.level {
        let (bx, bw, by) = (40.0 * u, w as f32 - 80.0 * u, 62.0 * u);
        cv.rect(bx as i32, by as i32, bw as i32, (10.0 * u) as i32, rgb(60, 70, 90), 1.0);
        cv.rect(bx as i32, by as i32, (bw * p as f32 / 100.0) as i32, (10.0 * u) as i32, b.accent, 1.0);
    }
    cv
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

/// Install YARG for the first time and refresh the status (so PLAY YARG appears).
fn yarg_install_job(tx: &Sender<Event>, zip: Option<String>, upd: &UpdInfo) {
    let r = sys::yarg_install(zip.as_deref(), |t| res(tx, Res::Progress(t)));
    res(tx, Res::YargInstalled(r));
    res(tx, Res::Status(sys::status(&sys::sound_label(), upd)));
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
            Cmd::SoundSelectBt(mac) => bg(tx, move |tx| {
                done(tx, sys::sound_select_bt(&mac));
                res(tx, sys::sound_refresh());
            }),
            Cmd::SoundVolume { card, pct } => bg(tx, move |_| sys::sound_volume(card, pct)),
            Cmd::SoundTest => bg(tx, |tx| done(tx, sys::sound_test())),
            Cmd::Refresh(Screen::Bluetooth) => bg(tx, |tx| res(tx, sys::bt_list())),
            Cmd::BtScan => bg(tx, |tx| res(tx, sys::bt_scan())),
            Cmd::BtConnect(d) => bg(tx, move |tx| {
                // A failure is an alert that waits for OK, not a toast that is gone in a few seconds.
                match sys::bt_connect(&d) {
                    Ok(m) => res(tx, Res::Done(m)),
                    Err(e) => res(tx, Res::Alert(e)),
                }
                res(tx, sys::bt_list());
            }),
            Cmd::BtForget(mac) => bg(tx, move |tx| {
                done(tx, sys::bt_forget(&mac));
                res(tx, sys::bt_list());
            }),
            Cmd::Refresh(Screen::Updates) => bg(tx, move |tx| res(tx, Res::Upd(sys::upd_info(&upd)))),
            Cmd::UpdCheck => bg(tx, move |tx| res(tx, Res::Upd(sys::upd_check(&upd)))),
            Cmd::UpdApply => bg(tx, move |tx| {
                done(tx, sys::upd_apply(|t| res(tx, Res::Progress(t))));
                let mut u = sys::upd_info(&upd);
                u.available = false;
                res(tx, Res::Upd(u));
            }),
            Cmd::YargInstall => bg(tx, move |tx| yarg_install_job(tx, None, &upd)),
            Cmd::YargInstallZip(zip) => bg(tx, move |tx| yarg_install_job(tx, Some(zip), &upd)),
            Cmd::YargCancel => bg(tx, |_| sys::update_cancel()),
            Cmd::Refresh(Screen::OsUpdate) => bg(tx, |tx| res(tx, Res::Os(sys::os_info()))),
            Cmd::OsCheck => bg(tx, |tx| {
                let _ = sys::os_run("os-check", |_| {});
                res(tx, Res::Os(sys::os_info()));
            }),
            Cmd::OsInstall => bg(tx, move |tx| {
                let r = sys::os_run("os-update", |t| res(tx, Res::Progress(t)));
                res(tx, Res::OsDone(r.map(|_| "The update is installed. Restart to use it".to_string())));
                res(tx, Res::Os(sys::os_info()));
                res(tx, Res::Status(sys::status(&sys::sound_label(), &upd)));
            }),
            Cmd::OsInstallOffline => bg(tx, move |tx| {
                let r = sys::os_run("os-update-offline", |t| res(tx, Res::Progress(t.replace("Downloading the update", "Copying the update"))));
                res(tx, Res::OsDone(r.map(|_| "The update is installed. Restart to use it".to_string())));
                res(tx, Res::Os(sys::os_info()));
                res(tx, Res::Status(sys::status(&sys::sound_label(), &upd)));
            }),
            Cmd::OsCancel => bg(tx, |_| sys::os_cancel()),
            Cmd::OsGoBack => bg(tx, move |tx| {
                let r = sys::os_run("os-goback", |_| {});
                res(tx, Res::OsDone(r.map(|_| "Restart to go back to the earlier system".to_string())));
                res(tx, Res::Os(sys::os_info()));
            }),
            Cmd::OsAck => bg(tx, |_| {
                let _ = sys::os_run("os-ack", |_| {});
            }),
            Cmd::OsAuto(on) => bg(tx, move |tx| done(tx, sys::write_auto_os(on))),
            Cmd::Refresh(Screen::Yarg) => bg(tx, |tx| res(tx, Res::YargZips(sys::yarg_zips()))),
            Cmd::UpdRollback => bg(tx, move |tx| {
                done(tx, sys::upd_rollback());
                res(tx, Res::Upd(sys::upd_info(&upd)));
            }),
            Cmd::UpdAuto(on) => bg(tx, move |tx| done(tx, sys::write_auto_update(on))),
            Cmd::SongsAuto(on) => bg(tx, move |tx| done(tx, sys::write_auto_songs(on))),
            Cmd::Refresh(Screen::Devices) => bg(tx, |tx| res(tx, Res::Devices(sys::devices_list()))),
            Cmd::Refresh(Screen::Install) => bg(tx, |tx| res(tx, Res::Disks(sys::install_disks()))),
            Cmd::Install(disk) => bg(tx, move |tx| {
                let r = sys::install_run(&disk, |step| res(tx, Res::InstallStep(step)));
                res(tx, Res::InstallDone(r));
            }),
            Cmd::Refresh(Screen::Songs) | Cmd::SongsRefresh => bg(tx, |tx| res(tx, Res::Songs(sys::songs_refresh()))),
            Cmd::Refresh(Screen::System) => bg(tx, |tx| res(tx, Res::SysInfo(sys::sys_info()))),
            Cmd::Refresh(Screen::Storage) => bg(tx, |tx| res(tx, Res::Storage(sys::storage_info()))),
            Cmd::Refresh(Screen::ImportPick) => bg(tx, |tx| res(tx, Res::Drives(sys::import_drives()))),
            Cmd::Refresh(Screen::Manage) => {
                let path = m.browse.path.clone();
                bg(tx, move |tx| res(tx, sys::songs_browse(path)));
            }
            Cmd::SongsBrowse(path) => bg(tx, move |tx| res(tx, sys::songs_browse(path))),
            Cmd::ImportScan(drive) => bg(tx, move |tx| match sys::import_scan(&drive) {
                Ok(plan) => res(tx, Res::ImportPlan(Ok(plan))),
                Err(Stop::Cancelled) => res(tx, Res::Done("Cancelled".into())),
                Err(Stop::Other(e)) => res(tx, Res::ImportPlan(Err(e))),
                Err(_) => res(tx, Res::ImportPlan(Err("Could not read the drive".into()))),
            }),
            Cmd::ImportRun(plan) => bg(tx, move |tx| {
                let r = sys::import_run(&plan, |t| res(tx, Res::Progress(t)));
                res(tx, Res::ImportDone(r));
                res(tx, Res::Storage(sys::storage_info()));
            }),
            Cmd::ImportCancel => bg(tx, |_| sys::import_cancel()),
            Cmd::GrowData => bg(tx, |tx| {
                match sys::grow_data() {
                    Ok(m) => res(tx, Res::Done(m)),
                    Err(e) => res(tx, Res::Alert(e)),
                }
                res(tx, Res::Storage(sys::storage_info()));
            }),
            Cmd::SongsDelete(path) => bg(tx, move |tx| {
                done(tx, sys::songs_delete(&path));
                // Show the folder the deleted thing was in (or the nearest one that is left).
                let mut parent = path.clone();
                parent.pop();
                res(tx, sys::songs_browse(parent));
                res(tx, Res::Storage(sys::storage_info()));
            }),
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
    // Mirror the screens and honour the lid before the window exists, so the first picture is already right.
    crate::screens::configure_now();
    let mut disp = match Display::open() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("yargos-home: {e}");
            return 1;
        }
    };
    let mut cv = Canvas::new(disp.w, disp.h);
    let (tx, rx) = channel::<Event>();
    disp.spawn_events(tx.clone());
    crate::screens::spawn_watcher(tx.clone());
    let active = Arc::new(AtomicBool::new(false));
    input::spawn_pads(tx.clone(), active.clone());
    spawn_wii_auto(tx.clone(), active.clone());
    spawn_battery_watch(tx.clone());
    // The low-battery notice over the game: the text and when to take it down.
    let mut banner: Option<Banner> = None;
    // The mixer is stored a few seconds after the last volume key (a power cut would lose it otherwise).
    let mut volume_save_at: Option<Instant> = None;
    let mut banner_drawn = Instant::now();

    let mut model = Model::new();
    let mut hits: Vec<Hit> = Vec::new();
    let mut mode = Mode::Dash;
    let mut visible = true;
    let mut dirty = true;
    let mut had_toast = false;
    // The main menu's status card is read again every few seconds while it is up: things change after it first drew
    // (the Wi-Fi gets its name, a restarted system closes its update, a guitar connects).
    let mut status_read = Instant::now();
    let mut notice: Option<String> = None;
    let started = Instant::now();
    // YARGOS_DEBUG_FPS=1: log how fast the dashboard draws and any frame that took long (to the journal).
    let fps_debug = std::env::var_os("YARGOS_DEBUG_FPS").is_some();
    let (mut frames, mut fps_since) = (0u32, Instant::now());
    let mut last_iter = Instant::now();

    // The data partition should have grown to fill the drive at boot. If room is still unused (that step failed, or the
    // drive was not ready then), try again now, once, while nothing else is running.
    bg(&tx, |tx| {
        if sys::data_slack_kib() >= SLACK_WORTH_KIB {
            eprintln!("yargos-home: {} MB of the drive is unused after /data; asking for it to be used", sys::data_slack_kib() / 1024);
            if let Ok(m) = sys::grow_data() {
                res(tx, Res::Done(m));
            }
        }
    });

    // Boot to the dashboard: YARG starts when PLAY YARG is chosen. The cursor only shows once a mouse moves.
    active.store(true, Ordering::Relaxed);
    disp.show_cursor(false);
    disp.focus();
    if sys::install_requested() {
        let cmds = model.open(Screen::Install);
        exec(cmds, &model, &tx);
    } else {
        exec(vec![Cmd::Refresh(Screen::Home)], &model, &tx);
    }

    loop {
        if fps_debug && last_iter.elapsed() > Duration::from_millis(500) {
            eprintln!("yargos-home: the loop was away for {} ms", last_iter.elapsed().as_millis());
        }
        last_iter = Instant::now();
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Event::LowBattery(text)) => match mode {
                Mode::Dash => {
                    eprintln!("yargos-home: low battery: {text}");
                    model.toast(format!("Low battery: {text}"));
                    if model.screen == Screen::Home {
                        exec(vec![Cmd::Refresh(Screen::Home)], &model, &tx); // the card lists it too
                    }
                    dirty = true;
                }
                Mode::Game(_) => {
                    eprintln!("yargos-home: low battery, banner over the game: {text}");
                    banner = Some(Banner { text: format!("Low battery: {text}"), until: Instant::now() + Duration::from_secs(10), accent: rgb(245, 175, 50), level: None });
                    banner_drawn = Instant::now() - Duration::from_secs(1);
                }
            },
            Ok(Event::Volume(key)) => bg(&tx, move |tx| {
                let r = match key {
                    VolKey::Up => sys::volume_change(5),
                    VolKey::Down => sys::volume_change(-5),
                    VolKey::Mute => sys::volume_toggle_mute(),
                };
                let _ = tx.send(Event::VolumeShown(r));
            }),
            Ok(Event::VolumeShown(shown)) => {
                eprintln!("yargos-home: volume notice: {}", shown.map(|v| if v.muted { "muted".to_string() } else { format!("{}%", v.pct) }).unwrap_or_else(|| "no volume control".into()));
                banner = Some(match shown {
                    Some(v) => Banner {
                        text: if v.muted { "Muted".into() } else { format!("Volume {}%", v.pct) },
                        until: Instant::now() + Duration::from_secs(2),
                        accent: rgb(23, 230, 140),
                        level: Some(if v.muted { 0 } else { v.pct }),
                    },
                    None => Banner { text: "This output has no volume control here: use the TV or headphones".into(), until: Instant::now() + Duration::from_secs(4), accent: rgb(245, 175, 50), level: None },
                });
                banner_drawn = Instant::now() - Duration::from_secs(1);
                if shown.is_some() {
                    volume_save_at = Some(Instant::now() + Duration::from_secs(3));
                    if matches!(mode, Mode::Dash) && model.screen == Screen::Sound {
                        exec(vec![Cmd::Refresh(Screen::Sound)], &model, &tx);
                    }
                }
            }
            Ok(Event::YargExited { code, .. }) => {
                banner = None;
                disp.hide_banner();
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
            Ok(Event::ScreenChanged) => {
                disp.resize();
                cv = Canvas::new(disp.w, disp.h);
                hits.clear();
                dirty = true;
            }
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return 1,
        }

        if matches!(mode, Mode::Dash) && model.screen == Screen::Home && model.busy.is_none() && model.alert.is_none() && status_read.elapsed() >= Duration::from_secs(8) {
            status_read = Instant::now();
            exec(vec![Cmd::Refresh(Screen::Home)], &model, &tx);
        }

        // Animations run on the clock, not on idle moments: a steady stream of events (a moving mouse, a controller
        // with a noisy axis) would otherwise freeze the spinner while everything else carries on.
        let now_tick = started.elapsed().as_millis() as u64 / 100;
        if now_tick != model.tick {
            model.tick = now_tick;
            match mode {
                Mode::Dash => {
                    let toast = model.live_toast().is_some();
                    dirty |= model.busy.is_some() || toast || had_toast || model.screen == Screen::Password && model.tick % 5 == 0;
                    had_toast = toast;
                }
                Mode::Game(since) => dirty |= visible && model.tick % 3 == 0 && since.elapsed() < Duration::from_secs(60),
            }
        }

        // The notice over the game: drawn again a few times a second (the game repaints under it), then taken down.
        if volume_save_at.is_some_and(|t| Instant::now() >= t) {
            volume_save_at = None;
            bg(&tx, |_| sys::volume_save());
        }
        if let Some(b) = &banner {
            if Instant::now() >= b.until {
                banner = None;
                disp.hide_banner();
            } else if banner_drawn.elapsed() >= Duration::from_millis(250) {
                banner_drawn = Instant::now();
                let cv_b = banner_canvas(&mut fonts, disp.h, disp.w, b);
                disp.show_banner(&cv_b, ((disp.w - cv_b.w) / 2) as i32, (disp.h as f32 * 0.04) as i32);
            }
        }

        if dirty && visible {
            let frame_start = Instant::now();
            match mode {
                Mode::Game(since) => draw_splash(&mut cv, &mut fonts, "Starting YARG\u{2026}", Some(splash_progress(since))),
                Mode::Dash => ui::draw(&mut cv, &mut fonts, &model, &mut hits),
            }
            disp.present(&cv);
            dirty = false;
            if fps_debug {
                frames += 1;
                if frame_start.elapsed() > Duration::from_millis(150) {
                    eprintln!("yargos-home: slow frame, {} ms", frame_start.elapsed().as_millis());
                }
                if fps_since.elapsed() >= Duration::from_secs(1) {
                    eprintln!("yargos-home: {frames} frames in {} ms", fps_since.elapsed().as_millis());
                    frames = 0;
                    fps_since = Instant::now();
                }
            }
        }
    }
}
