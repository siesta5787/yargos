// The privileged helper. systemd runs `yargos-home ctl` as root for each connection to
// /run/yargos-ctl.sock (socket mode 0660, group player); it reads one line and does one of a few
// fixed things. Nothing else is accepted, so the dashboard cannot be used to run arbitrary commands.
// The program's output goes straight back down the socket, which is how the installer reports progress.

use std::io::BufRead;
use std::process::Command;

#[derive(Debug, PartialEq)]
pub enum Request {
    Fixed(&'static str, &'static [&'static str]),
    /// `install /dev/NAME`: erase that disk and install the system on it.
    Install(String),
}

/// A whole-disk device path: /dev/ and then letters and digits only (sda, vdb, nvme0n1, mmcblk0).
fn disk_path(s: &str) -> Option<&str> {
    let name = s.strip_prefix("/dev/")?;
    (!name.is_empty() && name.len() <= 20 && name.chars().all(|c| c.is_ascii_alphanumeric())).then_some(s)
}

pub fn parse(line: &str) -> Option<Request> {
    let line = line.trim();
    match line {
        "poweroff" => Some(Request::Fixed("systemctl", &["poweroff"])),
        "reboot" => Some(Request::Fixed("systemctl", &["reboot"])),
        _ => line.strip_prefix("install ").and_then(disk_path).map(|d| Request::Install(d.to_string())),
    }
}

pub fn run() -> i32 {
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    match parse(&line) {
        Some(Request::Fixed(cmd, args)) => match Command::new(cmd).args(args).status() {
            Ok(s) if s.success() => {
                println!("ok");
                0
            }
            _ => {
                println!("error: {cmd} failed");
                1
            }
        },
        // The script prints STEP/DONE/ERROR lines itself; the script also re-checks that the disk is a valid target.
        Some(Request::Install(disk)) => match Command::new("/usr/bin/yargos-install").args(["run", &disk]).status() {
            Ok(s) if s.success() => 0,
            _ => 1,
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
        assert_eq!(parse("poweroff\n"), Some(Request::Fixed("systemctl", &["poweroff"])));
        assert_eq!(parse(" reboot "), Some(Request::Fixed("systemctl", &["reboot"])));
        assert_eq!(parse("poweroff; rm -rf /"), None);
        assert_eq!(parse("systemctl poweroff"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn install_takes_only_a_plain_disk_path() {
        assert_eq!(parse("install /dev/sda\n"), Some(Request::Install("/dev/sda".into())));
        assert_eq!(parse("install /dev/nvme0n1"), Some(Request::Install("/dev/nvme0n1".into())));
        assert_eq!(parse("install /dev/sda; reboot"), None);
        assert_eq!(parse("install /dev/../etc/passwd"), None);
        assert_eq!(parse("install sda"), None);
        assert_eq!(parse("install /dev/"), None);
        assert_eq!(parse("install /tmp/x"), None);
        assert_eq!(parse("install"), None);
    }
}
