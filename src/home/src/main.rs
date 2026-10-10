mod ctl;
mod demo;
mod event;
mod gfx;
mod input;
mod logo;
mod model;
mod session;
mod splash;
mod sys;
mod ui;
mod x;

use gfx::{Canvas, Fonts};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let code = match args.get(1).map(String::as_str) {
        Some("session") => session::run(),
        Some("splash") => splash::run(),
        Some("ctl") => ctl::run(),
        Some("padtest") => input::pad_test(),
        Some("xinfo") => x::xinfo(),
        Some("xfocus") => x::xfocus(),
        Some("screenshot") => {
            let name = args.get(2).map(String::as_str).unwrap_or("home");
            let out = args.get(3).map(String::as_str).unwrap_or("/tmp/yargos-home.png");
            let (w, h) = (
                args.get(4).and_then(|s| s.parse().ok()).unwrap_or(1280),
                args.get(5).and_then(|s| s.parse().ok()).unwrap_or(720),
            );
            let mut fonts = Fonts::load().expect("DejaVu fonts not found");
            let mut cv = Canvas::new(w, h);
            if name == "splash" {
                logo::draw_splash(&mut cv, &mut fonts, "Starting YARG\u{2026}", Some(0.4));
            } else {
                ui::draw(&mut cv, &mut fonts, &demo::model(name), &mut Vec::new());
            }
            cv.save_png(out).unwrap();
            println!("wrote {out}");
            0
        }
        _ => {
            eprintln!("usage: yargos-home session | splash | ctl | screenshot <screen> <out.png> [w h]");
            2
        }
    };
    std::process::exit(code);
}
