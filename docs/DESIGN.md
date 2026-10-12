# Design notes

How the pieces work. The README has the overview; docs/OTA-PLAN.md has the reasoning behind system updates;
docs/STATUS.md says what has and has not been verified.

## Runtime design

- The default image has no YARG. On first boot the main menu says INSTALL YARG: that screen downloads the latest release
  from YARC's GitHub (checksum from the release metadata, ~120 MB, with a percentage), or installs a
  `YARG_v<version>-Linux-x86_64.zip` found on the song drive when there is no internet, or leads to Wi-Fi setup first.
  `yargos-update install` / `install-zip` do this; the daily timer never installs YARG on its own. An image built with the
  seed instead gets it copied by `yargos-seed.service` (as the player) into `/data/apps/yarg/<version>`. Either way YARG
  runs from `/data/apps/yarg/current`, so newer releases arrive without rebuilding the OS (see "Updating YARG").
- YARG runs with `-persistent-data-path /data/yarg -offline`; settings, profiles and the song
  cache live on the data partition. It uses OpenGL (radeonsi); this Buildroot has no RADV Vulkan option.
- The song drive: label it `YARGSONGS` (ext4, exFAT, NTFS or FAT). It mounts read-only at
  `/media/songs` with `nofail`, so a missing drive never blocks boot. Point YARG's song folder at
  `/media/songs` once; the path never changes after that.
- The data partition of a flashed stick starts at 700 MB and `growdata.sh` grows it (partition and file system, online) to
  fill the stick at boot. It is safe to repeat and takes a lock, so the dashboard also runs it (helper word `grow-data`) when
  `/sys` shows room left after the data partition (once at start, and from SYSTEM > Storage), in case the boot step missed it.
  The installer gives an installed disk a data partition that fills it from the start.
- The internal song library: `/data/songs/<name>/...` (SYSTEM > Storage). The data partition already fills the whole disk
  (the installer gives it everything after the two system slots, `growdata` does the same on a stick), so there is nothing to
  format: songs are a folder on it, owned by the player (a first-level exception next to `apps/yarg` and `yarg`, made by
  `firstboot.sh`). Importing copies a portable drive into `/data/songs/<its label>/` (folders exactly as they are, minus
  Windows housekeeping folders, links and special files). `songcopy.rs` does the copying as the player; root only mounts the
  drive: `yargos-songs mount /dev/X` (through the helper request `songs-mount /dev/X`) mounts it read-only with no
  exec/dev/suid at `/run/yargos-import/mnt`, after checking that the device is a USB/SD drive other than the system's own
  (`yargos-songs list`); a drive already mounted (the YARGSONGS drive) is bind-mounted instead, since NTFS cannot be mounted
  twice. Copies go to `<file>.yargos-part` and are renamed when complete; a file with the same size is skipped, so a stopped
  or cut copy simply carries on when the import is repeated. A folder is named after the drive's label; a different drive
  (another filesystem UUID, recorded in `.yargos-source`) with the same label gets `label (2)`. The last 10% of the disk
  (at least 1.5 GB, at most 4 GB) always stays free: YARG's data, YARG updates and the system update download share it. The
  installer does not carry `/data/songs` to a new disk (it can be huge): import again there. `song-sources` scans each
  imported folder like a drive of its own, and `run-yarg` puts the results into YARG's `SongFolders`.
- Wii guitars: the kernel config leaves `CONFIG_HID_WIIMOTE` off (it is only blacklisted in the dev setup). udev starts `yargos-bridge wii /dev/hidrawN` per Wiimote
  (`057E:0306` / `057E:0330`). It initialises the extension, reads the guitar, and creates a uhid
  device impersonating a PS3 Guitar Hero World Tour guitar (`12BA:0100`) using the PlasticBand
  descriptor and report layout.
- Xbox 360 drums (ION Drum Rocker, `1bad:0130`): YARG cannot read wired XInput devices on Linux. The
  kernel `xpad` driver exposes the kit as evdev; udev starts `yargos-bridge drums /dev/input/eventN`,
  which grabs the node and creates a uhid PS3 Rock Band 2 drum kit (`12BA:0210`). The Xbox pad/cymbal
  flags map 1:1 onto the PS3 ones, so YARG's own decoding is reused. xpad inverts the Y axes; the
  bridge undoes that. The kit's piezo sensors report velocity spikes with no hit, so velocity is only
  forwarded while a pad flag is set. A udev rule makes the raw kit root-only so YARG does not list it
  as a second device. Other 360 kits can be added by extending the udev rule.
- YARG reads instruments through HIDrogen (`libhidapi-hidraw.so.0`), which the image ships. A
  normal desktop needs `sudo apt install libhidapi-hidraw0`, or YARG silently falls back to a generic
  gamepad layer (its log, `~/.config/unity3d/YARC/YARG/Player.log`, says so).
- USB MIDI drums/keys/mic need no bridge (ALSA sequencer + libRtMidi).
- Bluetooth: the dashboard's Bluetooth screen pairs HID devices, keyboards and mice; hold 1+2 on a Wiimote and it connects by
  itself while the dashboard is up. `yargos-pair [seconds]` does the same from a console. Pairings persist in `/data/bluetooth`.
- On a desktop kernel (hid-wiimote built), a Wiimote connects and BlueZ creates its HID device, but
  hid-generic refuses to claim it (a "special driver" exists), so no hidraw node appears. The image's
  kernel does not have that driver, so hid-generic binds normally. `tools/dev-test.sh wii` works around it
  on desktops by briefly setting `hid.ignore_special_drivers` while binding.
- Future Santroller guitars (USB or Bluetooth HID) need no bridge; YARG reads them over hidraw.
- Buildroot's own `xorg.service` is masked in the overlay so it does not fight `yarg.service`, and
  `default.target` is `graphical.target` (otherwise `yarg.service` never starts).
- Buildroot's `modesetting_drv.so` is not linked against libgbm ("undefined symbol: gbm_bo_get_offset"),
  so `/usr/lib/yargos/xorg` preloads `libgbm` for the X server only. Xorg autodetects its driver
  (amdgpu for AMD via an OutputClass, modesetting otherwise) so the image is not tied to one GPU.
- `session.sh` writes Xorg's log to `/tmp/Xorg.0.log` and prints it to the journal if Xorg exits.

## Session and dashboard

`yarg.service` starts Xorg with `yargos-home session` as its only client (no window manager). The machine
boots to the **dashboard**; PLAY YARG (the first entry, so one OK press) runs YARG
(`/usr/lib/yargos/run-yarg`) and the session stays out of the way while it runs. When YARG exits (Exit in its
menu, or a crash, which also shows a message) the dashboard appears again.

The dashboard is software-rendered (no GPU needed), works with a keyboard, mouse or the instruments, and
has these screens: Wi-Fi (scan, join, an on-screen keyboard for passwords), Sound (pick the output, volume,
test tone; writes `/data/asound.conf`), Bluetooth (scan, pair/connect, forget; Wiimotes get their own
hold-1+2 flow), Updates (YARG: check, install, automatic on/off, go back), Songs (the drive, and one YARG song source
per folder on it) and System (restart, shut down, install to this computer's disk, connected devices, system update,
storage: import songs to the internal drive, browse them and delete a song, a folder or everything with LEFT). Instruments navigate like YARG's menus: strum or cymbals move up/down, green
fret/pad = OK, red = Back, the two outer frets/pads = left/right. Everything that needs root goes through a helper
(`yargos-ctl.socket`, `yargos-home ctl`) that accepts only a few fixed requests: `poweroff`, `reboot`, `install /dev/<disk>`,
`sound <card> <device>`, `songs-mount /dev/<partition>`, `songs-umount`, `grow-data` and the system update words (`os-check`, `os-update`, `os-cancel`, `os-goback`, `os-ack`).

`yargos-home screenshot <screen> out.png` renders any screen with demo data (home, wifi, password, sound,
bluetooth, updates, songs, system, devices, yarg, os-update, storage, import, import-confirm, manage, manage-songs,
manage-confirm, busy, confirm, splash) without a display.

The boot splash (`yargos-home splash`) draws the logo straight onto `/dev/fb0` early in boot; the kernel
needs `CONFIG_DRM_FBDEV_EMULATION` for that.

## Updating YARG

`yargos-update` (shell, uses curl, jq, unzip) updates YARG on the data partition without touching the OS:

- It asks GitHub for the latest release, downloads the Linux zip, and checks it against the SHA-256
  digest GitHub publishes in the release metadata (both over HTTPS). YARG's own `.sig` is not checked
  because its public key is unknown. A release with no digest is refused.
- It unpacks to a staging folder, checks that every library's dependencies load, then atomically
  switches `/data/apps/yarg/current` to the new folder and keeps the old one as `previous`.
- It never touches the running game; the new version starts the next time YARG launches.
- A new version is "on trial": the session runs `yargos-update boot-check` each time PLAY YARG launches YARG,
  and calls `mark-good` once YARG has stayed up for 60 s. After three launches that did not get that far, the
  next launch rolls back to `previous` (and tells you on the dashboard) and remembers the bad version so it
  is not offered again. Booting or using the dashboard does not count.
- `yargos-update.timer` runs `auto` 3 minutes after boot and daily. Set `AUTO_UPDATE=no` in
  `/data/yargos.conf` to turn that off. Other commands: `status`, `check`, `apply [--restart]`, `rollback`.
- `tools/test-update.sh [extracted YARG folder]` tests all of this against a local fake release server.

Networking: wired links use DHCP (systemd-networkd). WiFi is handled by `iwd` (which does its own DHCP):
press Ctrl+Alt+F2, log in as root (empty password: the console is the only way in, see "Security" in the README), and run
`iwctl station <wlan> connect "SSID"`. Press Ctrl+Alt+F1 to return to the game.

## Boot entries

The boot entries live on the ESP (mounted at `/efi`): `yargos-<version>.conf` (quiet; `default yargos-2*` picks the
newest one that has not failed), `install.conf` (`yargos.install=1`, which makes the launcher open on its Install
screen; on a stick only) and `debug.conf` (verbose output, also on the serial console); the last two follow the
system that started last. Each entry names its root partition by GUID and /data via `systemd.mount-extra`, and its
kernel is `/yargos/<version>/bzImage` on the ESP. The installer writes `loader.conf` with `timeout 0`.

## Updating the system over the air

The system updates itself like a console: `yargos-os-update` (root; the dashboard's SYSTEM > System update reaches it
through the root helper) downloads a signed release into the OTHER system slot, adds a boot entry that tries it twice and
never restarts by itself. The new system must bring up the launcher and keep it running (`yargos-boot-ok`), otherwise the
boot loader falls back to the old system after two tries and the update is remembered as rejected. SYSTEM > System update
also has automatic updates on/off (`OS_AUTO_UPDATE=no`), restart to use a waiting update, and going back to the earlier
system. `tools/release.sh` makes and signs a release (`YARGOS_VERSION=2026.11.1 tools/br.sh` first); it refuses an image that
holds YARG. `tools/test-os-update.sh` tests all of it in QEMU. Details and the design: docs/OTA-PLAN.md. Releases are signed
with the key in `~/.yargos/release-ed25519.pem` (not in the repo, back it up); the matching public keys are in
`board/yargos/rootfs-overlay/usr/lib/yargos/keys/`, plus an emergency key whose private half should be kept offline.

## Trying it without the appliance

- `tools/dev-test.sh` (see its header): runs the bridges on a desktop Linux PC. Needs root for
  `/dev/uhid`. `setup` blocks `hid_wiimote` and installs dev udev rules; `teardown` undoes it;
  `drums`, `wii`, `watch` (shows the virtual instrument's reports), `raw` (shows what the Ion kit
  itself sends) and `yarg` (runs a downloaded YARG as your user).
- `tools/qemu-run.sh`: boots the image in QEMU using a throwaway overlay (`HEADLESS=1` writes the
  console to `~/yargos-build/qemu/serial.log`, and `tools/qemu-shot.sh` saves a screenshot; needs
  `qemu-system-x86` and `ovmf`, and membership of the `kvm` group). Use `fresh` after rebuilding the
  image, because the overlay is tied to the old image.
