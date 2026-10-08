// The privileged helper. systemd runs `yargos-home ctl` as root for each connection to
// /run/yargos-ctl.sock (socket mode 0660, group player); it reads one line and does one of a few
// fixed things. Nothing else is accepted, so the dashboard cannot be used to run arbitrary commands.

use std::io::BufRead;
use std::process::Command;

pub fn parse(line: &str) -> Option<(&'static str, &'static [&'static str])> {
    match line.trim() {
        "poweroff" => Some(("systemctl", &["poweroff"])),
        "reboot" => Some(("systemctl", &["reboot"])),
        _ => None,
    }
}

pub fn run() -> i32 {
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    match parse(&line) {
        Some((cmd, args)) => match Command::new(cmd).args(args).status() {
            Ok(s) if s.success() => {
                println!("ok");
                0
            }
            _ => {
                println!("error: {cmd} failed");
                1
            }
        },
        None => {
            println!("error: unknown request");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_known_requests_are_accepted() {
        assert_eq!(parse("poweroff\n"), Some(("systemctl", &["poweroff"][..])));
        assert_eq!(parse(" reboot "), Some(("systemctl", &["reboot"][..])));
        assert_eq!(parse("poweroff; rm -rf /"), None);
        assert_eq!(parse("systemctl poweroff"), None);
        assert_eq!(parse(""), None);
    }
}
