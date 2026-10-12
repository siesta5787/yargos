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
    /// `sound CARD DEV`: write /data/asound.conf (root-owned: root's own audio tools read it).
    Sound(u32, u32),
    /// `os-check`, `os-update`, `os-update-offline`, `os-find`, `os-cancel`, `os-goback`, `os-ack`: the system updater, always without arguments (the
    /// word picks the command; progress lines stream back, then "ok" or "error").
    Os(&'static str),
    /// `songs-mount /dev/NAME`: mount a portable drive read-only for importing its songs (yargos-songs checks the device).
    SongsMount(String),
    /// `songs-umount`: unmount it again.
    SongsUmount,
}

/// A whole-disk device path: /dev/ and then letters and digits only (sda, vdb, nvme0n1, mmcblk0).
fn disk_path(s: &str) -> Option<&str> {
    let name = s.strip_prefix("/dev/")?;
    (!name.is_empty() && name.len() <= 20 && name.chars().all(|c| c.is_ascii_alphanumeric())).then_some(s)
}

/// A small number, digits only (a card or device index).
fn index(s: &str, max: u32) -> Option<u32> {
    (!s.is_empty() && s.len() <= 3 && s.chars().all(|c| c.is_ascii_digit())).then(|| s.parse().ok()).flatten().filter(|&n| n <= max)
}

pub fn parse(line: &str) -> Option<Request> {
    let line = line.trim();
    match line {
        "poweroff" => Some(Request::Fixed("systemctl", &["poweroff"])),
        "reboot" => Some(Request::Fixed("systemctl", &["reboot"])),
        "os-check" => Some(Request::Os("check")),
        "os-update" => Some(Request::Os("install")),
        "os-cancel" => Some(Request::Os("cancel")),
        "os-goback" => Some(Request::Os("go-back")),
        "os-ack" => Some(Request::Os("ack")),
        "os-find" => Some(Request::Os("find")),
        "grow-data" => Some(Request::Fixed("/usr/lib/yargos/growdata.sh", &[])),
        "os-update-offline" => Some(Request::Os("install-offline")),
        "songs-umount" => Some(Request::SongsUmount),
        _ => {
            if let Some(dev) = line.strip_prefix("songs-mount ") {
                return disk_path(dev).map(|d| Request::SongsMount(d.to_string()));
            }
            if let Some(rest) = line.strip_prefix("sound ") {
                let (c, d) = rest.split_once(' ')?;
                return Some(Request::Sound(index(c, 31)?, index(d, 255)?));
            }
            line.strip_prefix("install ").and_then(disk_path).map(|d| Request::Install(d.to_string()))
        }
    }
}

fn write_sound(card: u32, dev: u32) -> std::io::Result<()> {
    // Written beside the target and renamed over it: /data is root's, so nothing can be swapped under us.
    let tmp = format!("{}.new", crate::sys::ASOUND);
    // By the card's id, not its number: USB sound cards (the piano, a microphone) renumber the cards from boot to boot.
    let id = std::fs::read_to_string(format!("/proc/asound/card{card}/id")).ok().map(|s| s.trim().to_string());
    let id = id.filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    std::fs::write(&tmp, crate::sys::asound_text(id.as_deref(), card, dev))?;
    std::fs::rename(&tmp, crate::sys::ASOUND)
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
        // Its output (progress lines) goes straight down the socket; a final line says how it ended.
        Some(Request::Os(cmd)) => match Command::new("/usr/bin/yargos-os-update").arg(cmd).status() {
            Ok(s) if s.success() => {
                println!("ok");
                0
            }
            _ => {
                println!("error: the system update did not finish");
                1
            }
        },
        // The script prints "ok" or "error: why" itself.
        Some(Request::SongsMount(dev)) => match Command::new("/usr/bin/yargos-songs").args(["mount", &dev]).status() {
            Ok(s) if s.success() => 0,
            _ => 1,
        },
        Some(Request::SongsUmount) => match Command::new("/usr/bin/yargos-songs").arg("umount").status() {
            Ok(s) if s.success() => 0,
            _ => 1,
        },
        Some(Request::Sound(card, dev)) => match write_sound(card, dev) {
            Ok(()) => {
                println!("ok");
                0
            }
            Err(e) => {
                println!("error: {e}");
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
        assert_eq!(parse("poweroff\n"), Some(Request::Fixed("systemctl", &["poweroff"])));
        assert_eq!(parse(" reboot "), Some(Request::Fixed("systemctl", &["reboot"])));
        assert_eq!(parse("poweroff; rm -rf /"), None);
        assert_eq!(parse("systemctl poweroff"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn the_system_updater_is_reached_only_through_fixed_words() {
        assert_eq!(parse("os-check\n"), Some(Request::Os("check")));
        assert_eq!(parse("os-update"), Some(Request::Os("install")));
        assert_eq!(parse("os-cancel"), Some(Request::Os("cancel")));
        assert_eq!(parse("os-goback"), Some(Request::Os("go-back")));
        assert_eq!(parse("os-ack"), Some(Request::Os("ack")));
        assert_eq!(parse("os-find"), Some(Request::Os("find")));
        assert_eq!(parse("grow-data"), Some(Request::Fixed("/usr/lib/yargos/growdata.sh", &[])));
        assert_eq!(parse("grow-data /dev/sda"), None);
        assert_eq!(parse("os-update-offline"), Some(Request::Os("install-offline")));
        assert_eq!(parse("os-update-offline /tmp/x"), None);
        assert_eq!(parse("os-find; reboot"), None);
        assert_eq!(parse("os-update --url http://evil"), None);
        assert_eq!(parse("os-settle"), None);
        assert_eq!(parse("os-boot-ok"), None);
        assert_eq!(parse("os-check; reboot"), None);
    }

    #[test]
    fn sound_takes_two_small_numbers_and_nothing_else() {
        assert_eq!(parse("sound 1 0\n"), Some(Request::Sound(1, 0)));
        assert_eq!(parse("sound 31 255"), Some(Request::Sound(31, 255)));
        assert_eq!(parse("sound 32 0"), None);
        assert_eq!(parse("sound 1"), None);
        assert_eq!(parse("sound -1 0"), None);
        assert_eq!(parse("sound 1 0 2"), None);
        assert_eq!(parse("sound 1; reboot 0"), None);
        assert_eq!(parse("sound 0x1 0"), None);
        assert_eq!(parse("sound  1 0"), None);
    }

    #[test]
    fn mounting_a_song_drive_takes_only_a_plain_device_path() {
        assert_eq!(parse("songs-mount /dev/sdb1\n"), Some(Request::SongsMount("/dev/sdb1".into())));
        assert_eq!(parse("songs-mount /dev/nvme0n1p2"), Some(Request::SongsMount("/dev/nvme0n1p2".into())));
        assert_eq!(parse("songs-umount"), Some(Request::SongsUmount));
        assert_eq!(parse("songs-mount /dev/sdb1; reboot"), None);
        assert_eq!(parse("songs-mount /dev/../etc"), None);
        assert_eq!(parse("songs-mount /tmp/x"), None);
        assert_eq!(parse("songs-mount sdb1"), None);
        assert_eq!(parse("songs-mount"), None);
        assert_eq!(parse("songs-umount /run"), None);
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
