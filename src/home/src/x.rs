// The session's X11 window: one full-screen window we draw into with PutImage. No window manager is
// running, so windows stack in the order they are mapped: YARG's window, mapped later, covers ours,
// and ours shows again as soon as YARG exits.

use crate::event::Event;
use crate::gfx::Canvas;
use crate::input::keysym_to_nav;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event as XEvent;
use x11rb::rust_connection::RustConnection;
use x11rb::COPY_DEPTH_FROM_PARENT;

pub struct Display {
    conn: Arc<RustConnection>,
    win: Window,
    gc: Gcontext,
    depth: u8,
    pub w: usize,
    pub h: usize,
    cursor_arrow: Cursor,
    cursor_blank: Cursor,
}

impl Display {
    pub fn open() -> Result<Self, String> {
        let (conn, num) = RustConnection::connect(None).map_err(|e| format!("cannot connect to the X server: {e}"))?;
        let setup = conn.setup();
        let screen = setup.roots[num].clone();
        let fmt = setup.pixmap_formats.iter().find(|f| f.depth == screen.root_depth).ok_or("no matching pixmap format")?;
        if fmt.bits_per_pixel != 32 {
            return Err(format!("unsupported pixel format ({} bpp)", fmt.bits_per_pixel));
        }
        let (w, h) = (screen.width_in_pixels, screen.height_in_pixels);
        let win = conn.generate_id().map_err(|e| e.to_string())?;
        let gc = conn.generate_id().map_err(|e| e.to_string())?;
        conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            win,
            screen.root,
            0,
            0,
            w,
            h,
            0,
            WindowClass::INPUT_OUTPUT,
            screen.root_visual,
            &CreateWindowAux::new().background_pixel(screen.black_pixel).override_redirect(1).event_mask(
                EventMask::EXPOSURE | EventMask::KEY_PRESS | EventMask::BUTTON_PRESS | EventMask::POINTER_MOTION | EventMask::VISIBILITY_CHANGE,
            ),
        )
        .map_err(|e| e.to_string())?;
        conn.create_gc(gc, win, &CreateGCAux::new()).map_err(|e| e.to_string())?;

        // Cursors: the normal arrow for the dashboard and an invisible one for the splash.
        let font = conn.generate_id().map_err(|e| e.to_string())?;
        conn.open_font(font, b"cursor").map_err(|e| e.to_string())?;
        let cursor_arrow = conn.generate_id().map_err(|e| e.to_string())?;
        conn.create_glyph_cursor(cursor_arrow, font, font, 68, 69, 0, 0, 0, 0xffff, 0xffff, 0xffff).map_err(|e| e.to_string())?;
        let blank_pm = conn.generate_id().map_err(|e| e.to_string())?;
        conn.create_pixmap(1, blank_pm, win, 1, 1).map_err(|e| e.to_string())?;
        let cursor_blank = conn.generate_id().map_err(|e| e.to_string())?;
        conn.create_cursor(cursor_blank, blank_pm, blank_pm, 0, 0, 0, 0, 0, 0, 0, 0).map_err(|e| e.to_string())?;

        // No window manager runs, so nothing hands keyboard focus to YARG's window. Watch for new top-level
        // windows (SubstructureNotify can be shared) and focus them ourselves.
        conn.change_window_attributes(screen.root, &ChangeWindowAttributesAux::new().event_mask(EventMask::SUBSTRUCTURE_NOTIFY))
            .map_err(|e| e.to_string())?;
        conn.map_window(win).map_err(|e| e.to_string())?;
        conn.flush().map_err(|e| e.to_string())?;
        Ok(Self { conn: Arc::new(conn), win, gc, depth: screen.root_depth, w: w as usize, h: h as usize, cursor_arrow, cursor_blank })
    }

    pub fn show_cursor(&self, visible: bool) {
        let c = if visible { self.cursor_arrow } else { self.cursor_blank };
        let _ = self.conn.change_window_attributes(self.win, &ChangeWindowAttributesAux::new().cursor(c));
        let _ = self.conn.flush();
    }

    pub fn focus(&self) {
        let _ = self.conn.set_input_focus(InputFocus::POINTER_ROOT, self.win, x11rb::CURRENT_TIME);
        let _ = self.conn.flush();
    }

    pub fn present(&self, cv: &Canvas) {
        let max = self.conn.maximum_request_bytes().saturating_sub(64);
        let rows = (max / (cv.w * 4)).clamp(1, 256);
        let mut y = 0;
        while y < cv.h {
            let n = rows.min(cv.h - y);
            let mut data = Vec::with_capacity(cv.w * n * 4);
            for p in &cv.px[y * cv.w..(y + n) * cv.w] {
                data.extend_from_slice(&p.to_le_bytes());
            }
            if self.conn.put_image(ImageFormat::Z_PIXMAP, self.win, self.gc, cv.w as u16, n as u16, 0, y as i16, 0, self.depth, &data).is_err() {
                return;
            }
            y += n;
        }
        let _ = self.conn.flush();
    }

    /// Forward keyboard, mouse and visibility events to the main loop on a background thread.
    pub fn spawn_events(&self, tx: Sender<Event>) {
        let conn = self.conn.clone();
        let own = self.win;
        let screen_area = self.w as u32 * self.h as u32;
        std::thread::spawn(move || {
            let setup = conn.setup();
            let (min, max) = (setup.min_keycode, setup.max_keycode);
            let map = conn.get_keyboard_mapping(min, max - min + 1).ok().and_then(|c| c.reply().ok());
            let sym = |code: u8, shift: bool| -> u32 {
                let Some(m) = &map else { return 0 };
                let per = m.keysyms_per_keycode as usize;
                let i = (code - min) as usize * per + usize::from(shift && per > 1);
                m.keysyms.get(i).copied().unwrap_or(0)
            };
            while let Ok(ev) = conn.wait_for_event() {
                let out = match ev {
                    XEvent::KeyPress(e) => keysym_to_nav(sym(e.detail, u16::from(e.state) & 1 != 0)).map(Event::Nav),
                    XEvent::ButtonPress(e) if e.detail == 1 => Some(Event::Click(e.event_x as i32, e.event_y as i32)),
                    XEvent::MotionNotify(e) => Some(Event::Hover(e.event_x as i32, e.event_y as i32)),
                    XEvent::Expose(_) => Some(Event::Redraw),
                    XEvent::VisibilityNotify(e) => Some(Event::Visible(e.state != Visibility::FULLY_OBSCURED)),
                    XEvent::MapNotify(e) if e.window != own && !e.override_redirect => {
                        // Only a window that fills most of the screen is the game; small helper windows
                        // that Unity maps afterwards must not take the focus away.
                        let big = conn
                            .get_geometry(e.window)
                            .ok()
                            .and_then(|c| c.reply().ok())
                            .map(|g| g.width as u32 * g.height as u32 * 2 >= screen_area)
                            .unwrap_or(false);
                        eprintln!("yargos-home: window 0x{:x} mapped, {}", e.window, if big { "focusing it" } else { "ignoring it (small)" });
                        if big {
                            let _ = conn.set_input_focus(InputFocus::POINTER_ROOT, e.window, x11rb::CURRENT_TIME);
                            let _ = conn.flush();
                        }
                        None
                    }
                    _ => None,
                };
                if let Some(o) = out {
                    if tx.send(o).is_err() {
                        break;
                    }
                }
            }
        });
    }
}

/// `yargos-home xinfo`: print who has the input focus and how every top-level window is set up (diagnostics).
pub fn xinfo() -> i32 {
    let Ok((conn, num)) = RustConnection::connect(None) else { eprintln!("cannot connect to the X server"); return 1 };
    let root = conn.setup().roots[num].root;
    let name = |w: Window| -> String {
        conn.get_property(false, w, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 64)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| String::from_utf8_lossy(&r.value).into_owned())
            .unwrap_or_default()
    };
    if let Some(f) = conn.get_input_focus().ok().and_then(|c| c.reply().ok()) {
        println!("input focus: 0x{:x} (revert {:?})", f.focus, f.revert_to);
    }
    if let Some(p) = conn.query_pointer(root).ok().and_then(|c| c.reply().ok()) {
        println!("pointer at {},{} over child 0x{:x}", p.root_x, p.root_y, p.child);
    }
    let mut stack: Vec<(Window, usize)> = conn
        .query_tree(root)
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|t| t.children.into_iter().rev().map(|w| (w, 0)).collect())
        .unwrap_or_default();
    while let Some((w, depth)) = stack.pop() {
        let (Some(a), Some(g)) = (
            conn.get_window_attributes(w).ok().and_then(|c| c.reply().ok()),
            conn.get_geometry(w).ok().and_then(|c| c.reply().ok()),
        ) else { continue };
        println!(
            "{:indent$}0x{w:x} \"{}\" {}x{}+{}+{} map={:?} override={} your_mask={:?} all_masks={:?}",
            "", name(w), g.width, g.height, g.x, g.y, a.map_state, a.override_redirect, a.your_event_mask, a.all_event_masks,
            indent = depth * 2
        );
        if let Some(t) = conn.query_tree(w).ok().and_then(|c| c.reply().ok()) {
            stack.extend(t.children.into_iter().rev().map(|c| (c, depth + 1)));
        }
    }
    0
}

/// `yargos-home xfocus`: lose and regain the input focus on the window called YARG (diagnostics).
pub fn xfocus() -> i32 {
    let Ok((conn, num)) = RustConnection::connect(None) else { return 1 };
    let root = conn.setup().roots[num].root;
    let mut stack = vec![root];
    while let Some(w) = stack.pop() {
        let Some(t) = conn.query_tree(w).ok().and_then(|c| c.reply().ok()) else { continue };
        for c in t.children {
            let n = conn
                .get_property(false, c, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 8)
                .ok()
                .and_then(|r| r.reply().ok())
                .map(|r| r.value)
                .unwrap_or_default();
            if n == b"YARG" {
                let _ = conn.set_input_focus(InputFocus::NONE, 0u32, x11rb::CURRENT_TIME);
                let _ = conn.flush();
                std::thread::sleep(std::time::Duration::from_millis(300));
                let _ = conn.set_input_focus(InputFocus::POINTER_ROOT, c, x11rb::CURRENT_TIME);
                let _ = conn.get_input_focus().ok().and_then(|c| c.reply().ok());
                println!("refocused 0x{c:x}");
                return 0;
            }
            stack.push(c);
        }
    }
    eprintln!("no YARG window");
    1
}
