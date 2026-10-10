// Multi-monitor policy for the session (no window manager and no desktop to do it for us):
//  - every connected screen shows the same picture (mirror), at the biggest resolution they all have;
//  - with the laptop lid shut and an external screen connected, the laptop's own panel is turned off and the
//    external screen carries on alone;
//  - with only the laptop's panel, or a lone external screen, it simply uses its preferred mode.
// `plan` is pure and unit tested; `apply`/`watch` talk to the X server through RandR.

use crate::event::Event;
use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::time::Duration;
use x11rb::connection::Connection;
use x11rb::protocol::randr::{self, ConnectionExt as _};
use x11rb::protocol::xproto::Window;
use x11rb::rust_connection::RustConnection;

#[derive(Clone, Debug, PartialEq)]
pub struct Mode {
    pub id: u32,
    pub w: u16,
    pub h: u16,
    pub hz: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Out {
    pub id: u32,
    pub name: String,
    pub connected: bool,
    pub internal: bool,
    pub modes: Vec<Mode>,
    /// The mode the screen says it prefers (its native one).
    pub preferred: Option<u32>,
    pub crtcs: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub w: u16,
    pub h: u16,
    /// (output, mode), all placed at 0,0.
    pub outs: Vec<(u32, u32)>,
    pub primary: u32,
}

/// Connectors that are the laptop's own panel.
pub fn is_internal(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("edp") || n.starts_with("lvds") || n.starts_with("dsi")
}

/// `/proc/acpi/button/lid/*/state` -> is the lid shut?
pub fn parse_lid(state: &str) -> bool {
    state.to_ascii_lowercase().contains("closed")
}

pub fn lid_closed() -> bool {
    let Ok(dir) = std::fs::read_dir("/proc/acpi/button/lid") else { return false };
    dir.filter_map(|e| e.ok()).any(|e| std::fs::read_to_string(e.path().join("state")).map(|s| parse_lid(&s)).unwrap_or(false))
}

fn best_mode<'a>(o: &'a Out, w: u16, h: u16) -> Option<&'a Mode> {
    let mut same: Vec<&Mode> = o.modes.iter().filter(|m| m.w == w && m.h == h).collect();
    // The preferred mode if it is this size, otherwise the highest refresh rate.
    same.sort_by(|a, b| (Some(b.id) == o.preferred).cmp(&(Some(a.id) == o.preferred)).then(b.hz.partial_cmp(&a.hz).unwrap_or(std::cmp::Ordering::Equal)));
    same.into_iter().next()
}

/// The screen's own best mode: its preferred one, else the biggest.
fn native_mode(o: &Out) -> Option<&Mode> {
    o.preferred.and_then(|p| o.modes.iter().find(|m| m.id == p)).or_else(|| o.modes.iter().max_by_key(|m| m.w as u32 * m.h as u32))
}

pub fn plan(outs: &[Out], lid_closed: bool) -> Option<Plan> {
    let connected: Vec<&Out> = outs.iter().filter(|o| o.connected && !o.modes.is_empty()).collect();
    if connected.is_empty() {
        return None;
    }
    let external: Vec<&Out> = connected.iter().copied().filter(|o| !o.internal).collect();
    // With the lid shut the panel is dark: only the external screens remain.
    let active: Vec<&Out> = if lid_closed && !external.is_empty() { external.clone() } else { connected.clone() };

    // A single screen just uses the mode it prefers (its native one), not the biggest it can be talked into.
    if active.len() == 1 {
        let o = active[0];
        let m = native_mode(o)?;
        return Some(Plan { w: m.w, h: m.h, outs: vec![(o.id, m.id)], primary: o.id });
    }

    // The biggest resolution every active screen has.
    let mut common: Vec<(u16, u16)> = active[0].modes.iter().map(|m| (m.w, m.h)).collect();
    common.sort();
    common.dedup();
    common.retain(|&(w, h)| active.iter().all(|o| best_mode(o, w, h).is_some()));
    let primary_of = |v: &[&Out]| v.iter().find(|o| !o.internal).or(v.first()).map(|o| o.id).unwrap();

    if let Some(&(w, h)) = common.iter().max_by_key(|&&(w, h)| w as u32 * h as u32) {
        let outs: Vec<(u32, u32)> = active.iter().map(|o| (o.id, best_mode(o, w, h).unwrap().id)).collect();
        return Some(Plan { w, h, outs, primary: primary_of(&active) });
    }
    // Nothing in common: the best single screen (an external one if there is one) at its own preferred mode.
    let one = *external.first().or(active.first())?;
    let m = native_mode(one)?;
    Some(Plan { w: m.w, h: m.h, outs: vec![(one.id, m.id)], primary: one.id })
}

// ---- talking to the X server ---------------------------------------------------------------------

fn read_outputs(conn: &RustConnection, root: Window) -> Result<(Vec<Out>, u32), String> {
    let res = conn.randr_get_screen_resources(root).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
    let modes: HashMap<u32, Mode> = res
        .modes
        .iter()
        .filter(|m| u32::from(m.mode_flags) & (u32::from(randr::ModeFlag::INTERLACE) | u32::from(randr::ModeFlag::DOUBLE_SCAN)) == 0 && m.htotal > 0 && m.vtotal > 0)
        .map(|m| (m.id, Mode { id: m.id, w: m.width, h: m.height, hz: m.dot_clock as f64 / (m.htotal as f64 * m.vtotal as f64) }))
        .collect();
    let mut outs = vec![];
    for &o in &res.outputs {
        let info = conn.randr_get_output_info(o, res.config_timestamp).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
        let name = String::from_utf8_lossy(&info.name).to_string();
        let list: Vec<Mode> = info.modes.iter().filter_map(|id| modes.get(id).cloned()).collect();
        outs.push(Out {
            id: o,
            internal: is_internal(&name),
            name,
            connected: info.connection == randr::Connection::CONNECTED,
            preferred: info.modes.iter().take(info.num_preferred as usize).find(|id| modes.contains_key(id)).copied(),
            modes: list,
            crtcs: info.crtcs.clone(),
        });
    }
    Ok((outs, res.config_timestamp))
}

/// What the CRTCs show now: (crtc, mode, x, y, outputs), only the enabled ones.
fn current(conn: &RustConnection, root: Window) -> Result<Vec<(u32, u32, i16, i16, Vec<u32>)>, String> {
    let res = conn.randr_get_screen_resources_current(root).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
    let mut v = vec![];
    for &c in &res.crtcs {
        let i = conn.randr_get_crtc_info(c, res.config_timestamp).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
        if i.mode != 0 {
            v.push((c, i.mode, i.x, i.y, i.outputs.clone()));
        }
    }
    Ok(v)
}

fn already(plan: &Plan, cur: &[(u32, u32, i16, i16, Vec<u32>)]) -> bool {
    cur.len() == plan.outs.len() && plan.outs.iter().all(|(o, m)| cur.iter().any(|(_, cm, x, y, os)| cm == m && *x == 0 && *y == 0 && os.contains(o)))
}

/// Make the X server match the plan. Returns the new screen size if something changed.
fn apply(conn: &RustConnection, root: Window, outs: &[Out], cfg_ts: u32, plan: &Plan) -> Result<Option<(u16, u16)>, String> {
    let cur = current(conn, root)?;
    if already(plan, &cur) {
        return Ok(None);
    }
    let res = conn.randr_get_screen_resources_current(root).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
    // A CRTC for each output: the one it already uses if possible, otherwise any free one it can drive.
    let mut used: Vec<u32> = vec![];
    let mut assign: Vec<(u32, u32, u32)> = vec![]; // (crtc, output, mode)
    for &(oid, mode) in &plan.outs {
        let out = outs.iter().find(|o| o.id == oid).ok_or("unknown output")?;
        let mine = cur.iter().find(|c| c.4.contains(&oid)).map(|c| c.0).filter(|c| !used.contains(c));
        let crtc = mine.or_else(|| out.crtcs.iter().copied().find(|c| !used.contains(c) && res.crtcs.contains(c))).ok_or("no free CRTC")?;
        used.push(crtc);
        assign.push((crtc, oid, mode));
    }
    // Everything off first (the screen cannot shrink under an active CRTC), then the new size, then the screens.
    for c in &res.crtcs {
        let _ = conn.randr_set_crtc_config(*c, x11rb::CURRENT_TIME, cfg_ts, 0, 0, 0, randr::Rotation::ROTATE0, &[]);
    }
    conn.randr_set_screen_size(root, plan.w, plan.h, plan.w as u32 * 254 / 960, plan.h as u32 * 254 / 960).map_err(|e| e.to_string())?;
    for (crtc, out, mode) in &assign {
        let r = conn
            .randr_set_crtc_config(*crtc, x11rb::CURRENT_TIME, cfg_ts, 0, 0, *mode, randr::Rotation::ROTATE0, &[*out])
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        if r.status != randr::SetConfig::SUCCESS {
            return Err(format!("could not set the mode on output {out}: {:?}", r.status));
        }
    }
    let _ = conn.randr_set_output_primary(root, plan.primary);
    conn.flush().map_err(|e| e.to_string())?;
    Ok(Some((plan.w, plan.h)))
}

/// Look at the screens and the lid and fix the layout if it is not right. `Ok(Some(size))` if it changed.
pub fn configure(conn: &RustConnection, root: Window) -> Result<Option<(u16, u16)>, String> {
    let (outs, ts) = read_outputs(conn, root)?;
    match plan(&outs, lid_closed()) {
        Some(p) => apply(conn, root, &outs, ts, &p),
        None => Ok(None),
    }
}

/// `yargos-home screens set WxH`: put the first connected screen in that mode (a test: the watcher then puts it back).
pub fn force(size: &str) -> i32 {
    let Some((w, h)) = size.split_once('x').and_then(|(a, b)| Some((a.parse::<u16>().ok()?, b.parse::<u16>().ok()?))) else {
        eprintln!("usage: yargos-home screens set WIDTHxHEIGHT");
        return 2;
    };
    let Ok((conn, num)) = RustConnection::connect(None) else { return 1 };
    let root = conn.setup().roots[num].root;
    let Ok((outs, ts)) = read_outputs(&conn, root) else { return 1 };
    let Some(o) = outs.iter().find(|o| o.connected) else { return 1 };
    let Some(m) = best_mode(o, w, h) else {
        eprintln!("{} has no {w}x{h} mode", o.name);
        return 1;
    };
    let p = Plan { w, h, outs: vec![(o.id, m.id)], primary: o.id };
    match apply(&conn, root, &outs, ts, &p) {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

/// `yargos-home screens`: what the launcher sees (outputs, modes, the lid) and what it would do (diagnostics).
pub fn report() -> i32 {
    let Ok((conn, num)) = RustConnection::connect(None) else { eprintln!("cannot connect to the X server"); return 1 };
    let root = conn.setup().roots[num].root;
    let (outs, _) = match read_outputs(&conn, root) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let lid = lid_closed();
    println!("lid closed: {lid}");
    for o in &outs {
        let pref = o.preferred.and_then(|p| o.modes.iter().find(|m| m.id == p)).map(|m| format!("{}x{}", m.w, m.h)).unwrap_or_default();
        println!("{}  connected={} internal={} modes={} preferred={pref}", o.name, o.connected, o.internal, o.modes.len());
    }
    match plan(&outs, lid) {
        Some(p) => println!("plan: {}x{}, outputs {:?}, primary {}", p.w, p.h, p.outs, p.primary),
        None => println!("plan: none"),
    }
    if let Ok(cur) = current(&conn, root) {
        println!("now: {cur:?}");
    }
    0
}

/// Once, before the window exists, so the first picture already has the right size.
pub fn configure_now() {
    let Ok((conn, num)) = RustConnection::connect(None) else { return };
    let root = conn.setup().roots[num].root;
    match configure(&conn, root) {
        Ok(Some((w, h))) => eprintln!("yargos-home: screens set up: {w}x{h}"),
        Ok(None) => {}
        Err(e) => eprintln!("yargos-home: could not set up the screens: {e}"),
    }
}

/// Re-check every couple of seconds (a plugged-in cable and the lid are noticed here) and tell the main loop when
/// the screen size changed.
pub fn spawn_watcher(tx: Sender<Event>) {
    std::thread::spawn(move || {
        let Ok((conn, num)) = RustConnection::connect(None) else { return };
        let root = conn.setup().roots[num].root;
        loop {
            std::thread::sleep(Duration::from_secs(2));
            match configure(&conn, root) {
                Ok(Some((w, h))) => {
                    eprintln!("yargos-home: screens changed: {w}x{h}");
                    let _ = tx.send(Event::ScreenChanged);
                }
                Ok(None) => {}
                Err(e) => eprintln!("yargos-home: screen check failed: {e}"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: u32, w: u16, h: u16, hz: f64) -> Mode {
        Mode { id, w, h, hz }
    }
    fn out(id: u32, name: &str, connected: bool, modes: Vec<Mode>, pref: Option<u32>) -> Out {
        Out { id, name: name.into(), connected, internal: is_internal(name), modes, preferred: pref, crtcs: vec![id + 100] }
    }
    fn laptop() -> Out {
        out(1, "eDP", true, vec![m(10, 1920, 1080, 60.0), m(11, 1280, 720, 60.0)], Some(10))
    }
    fn tv() -> Out {
        out(2, "HDMI-A-0", true, vec![m(20, 3840, 2160, 30.0), m(21, 1920, 1080, 60.0), m(22, 1920, 1080, 50.0), m(23, 1280, 720, 60.0)], Some(20))
    }

    #[test]
    fn a_lone_panel_uses_its_preferred_mode() {
        let p = plan(&[laptop(), out(2, "HDMI-A-0", false, vec![], None)], false).unwrap();
        assert_eq!(p, Plan { w: 1920, h: 1080, outs: vec![(1, 10)], primary: 1 });
    }

    #[test]
    fn a_lone_screen_uses_its_preferred_mode_not_its_biggest() {
        let odd = out(5, "Virtual-1", true, vec![m(50, 5120, 2160, 60.0), m(51, 1280, 800, 60.0)], Some(51));
        let p = plan(&[odd], false).unwrap();
        assert_eq!((p.w, p.h, p.outs), (1280, 800, vec![(5, 51)]));
    }

    #[test]
    fn hdmi_plugged_in_mirrors_at_the_biggest_size_both_have() {
        let p = plan(&[laptop(), tv()], false).unwrap();
        assert_eq!((p.w, p.h), (1920, 1080)); // not the TV's 4K: the laptop cannot show it
        assert_eq!(p.outs, vec![(1, 10), (2, 21)]); // the 60 Hz one on the TV
        assert_eq!(p.primary, 2); // the external screen is primary
    }

    #[test]
    fn lid_shut_with_hdmi_turns_the_panel_off_and_keeps_the_tv_on() {
        let p = plan(&[laptop(), tv()], true).unwrap();
        assert_eq!(p.outs.len(), 1);
        assert_eq!(p.outs[0].0, 2);
        assert_eq!((p.w, p.h), (3840, 2160)); // alone it uses its own preferred mode
    }

    #[test]
    fn lid_shut_without_an_external_screen_leaves_the_panel_on() {
        let p = plan(&[laptop(), out(2, "HDMI-A-0", false, vec![], None)], true).unwrap();
        assert_eq!(p.outs, vec![(1, 10)]);
    }

    #[test]
    fn no_common_size_falls_back_to_the_external_screen_alone() {
        let odd = out(3, "HDMI-A-1", true, vec![m(30, 1024, 768, 60.0)], Some(30));
        let p = plan(&[laptop(), odd], false).unwrap();
        assert_eq!(p.outs, vec![(3, 30)]);
        assert_eq!((p.w, p.h), (1024, 768));
    }

    #[test]
    fn several_screens_all_mirror() {
        let dp = out(4, "DP-1", true, vec![m(40, 1920, 1080, 60.0), m(41, 1280, 720, 60.0)], Some(40));
        let p = plan(&[tv(), dp], false).unwrap();
        assert_eq!(p.outs, vec![(2, 21), (4, 40)]);
        assert_eq!((p.w, p.h), (1920, 1080));
    }

    #[test]
    fn nothing_connected_means_no_plan() {
        assert_eq!(plan(&[out(1, "eDP", false, vec![], None)], false), None);
        assert_eq!(plan(&[], false), None);
    }

    #[test]
    fn internal_connectors_and_the_lid_file_are_recognised() {
        assert!(is_internal("eDP-1") && is_internal("LVDS-1") && is_internal("eDP"));
        assert!(!is_internal("HDMI-A-0") && !is_internal("DP-2"));
        assert!(parse_lid("state:      closed\n"));
        assert!(!parse_lid("state:      open\n"));
    }

    #[test]
    fn an_unchanged_layout_is_not_applied_again() {
        let p = Plan { w: 1920, h: 1080, outs: vec![(1, 10), (2, 21)], primary: 2 };
        assert!(already(&p, &[(101, 10, 0, 0, vec![1]), (102, 21, 0, 0, vec![2])]));
        assert!(!already(&p, &[(101, 10, 0, 0, vec![1])]));
        assert!(!already(&p, &[(101, 10, 0, 0, vec![1]), (102, 21, 1920, 0, vec![2])]));
    }
}
