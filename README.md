# YARGOS

Buildroot external tree for a console-style [YARG](https://github.com/YARC-Official/YARG) appliance
(x86_64, AMD/Intel). Power on -> UEFI -> systemd-boot -> Xorg (no WM) -> YARG fullscreen.

Status: the image builds and its contents have been inspected, but it has not been confirmed to boot
all the way to YARG. The instrument bridges are partly tested on real hardware. See
[Status](#status) for what is and is not verified.

## Build

The first build takes about 10 hours on 4 cores (mostly LLVM/Clang for Mesa). Later builds only redo
what changed.

    git clone --branch 2026.02.3 https://github.com/buildroot/buildroot.git
    cd buildroot
    make BR2_EXTERNAL=/path/to/yargos-br2 yargos_x86_64_defconfig
    make                   # output/images/yargos.img (about 5.6 GB)

Needs a system `unzip` (Buildroot uses it for the YARG zip). Write the image to a USB stick or SSD:
`sudo dd if=output/images/yargos.img of=/dev/sdX bs=4M status=progress`

### Maintainer's setup (`tools/br.sh`)

The first build was done at `/tmp/brout` and `/tmp/buildroot`, then moved to `~/yargos-build/`
(`/tmp` is wiped at boot). About 480 files in the host tools still embed the old `/tmp` paths, so use
`tools/br.sh` for every build there: it recreates the `/tmp` symlinks first.

    tools/br.sh                        # rebuild the image -> ~/yargos-build/output/images/yargos.img
    tools/br.sh yargos-bridge-rebuild  # rebuild one package, then the image

ccache is enabled (`~/.buildroot-ccache`, 20G) so a from-scratch rebuild is much faster.

## Installing / trying it on a PC

`output/images/yargos.img` is a complete UEFI-bootable disk (128M boot, 1.25G system, 700M data; about 2.2 GB,
so any 4 GB or larger USB stick works, preferably USB 3). It runs from the drive you write it to. On first
boot `yargos-growdata.service` grows the data partition (and its filesystem) to fill the drive, before `/data`
is mounted; on a drive exactly the image's size nothing grows and the 700M holds YARG with some room to spare.

    sudo tools/flash.sh                 # lists USB drives, asks which, makes you retype it, writes, verifies

On the target PC: turn Secure Boot off in the firmware settings (the boot loader is unsigned) and boot from
the USB drive (usually F7, F11, F12 or Del at power-on). A small menu shows for 5 s:

- **Start YARG OS from this drive** (default): runs from the stick.
- **Install YARG OS on this computer**: opens the launcher on its Install screen (see below).
- **Start YARG OS (verbose, for troubleshooting)**: prints boot messages, and has a serial console.

### Installing to the computer's own disk

Choose the install entry (or SYSTEM > Install on this computer's disk in the launcher). It lists the other
disks (at least 8 GB, nothing mounted from them; never the drive it is running from), asks you to confirm which
one gets erased, then runs `yargos-install` through the root helper and shows its steps: GPT with a 128 MiB
EFI partition, a 4 GiB system and the rest as data; it copies the running system and the whole data
partition (YARG, its settings and updates, Bluetooth pairings, Wi-Fi passwords), writes boot entries and
registers with the firmware (the fallback `EFI/BOOT/BOOTX64.EFI` boots it anyway). The installed system has no
boot menu (hold the space bar during startup to get one). Every partition on the new disk gets a fresh random
GUID that the boot entry and `/etc/fstab` use, so the USB drive and the installed disk can be plugged in
together; the flashed image uses fixed GUIDs (`...0001` system, `...0002` data) for the same reason. Never
look a root or data partition up by label.

If a machine boots to a black screen or stops early, the boot log is kept on the drive: plug it into another
Linux PC, mount the `rootfs` partition and run `journalctl --directory <mount>/var/log/journal -b -1`
(or read it from a photo of the screen: the kernel now shows its messages from the first second).

For a diagnostic report on a machine with no network: press Ctrl+Alt+F2, log in as `root` (no password),
run `journalctl -b > /data/boot.log`, then read `boot.log` from the `YARGDATA` partition on another PC.

## Layout

- `configs/yargos_x86_64_defconfig` - top-level config
- `board/yargos/linux.fragment` - kernel options (broad driver set, UHID, ALSA sequencer, no hid-wiimote)
- `board/yargos/genimage.cfg` - GPT: ESP (128M) / rootfs (1.8G ext4) / YARGDATA (700M ext4, grows on first boot)
- `board/yargos/esp/loader/` - systemd-boot config; the verbose `yargos-debug` entry is the default
  for now (see "Boot entries")
- `board/yargos/rootfs-overlay/` - systemd units, udev rules, bluez config, launch scripts
- `package/yarg` - pinned YARG release, installed as a seed in `/usr/share/yarg-seed`
- `package/yargos-bridge` + `src/bridge` - Rust bridges: Wiimote guitar and wired Xbox 360 drums ->
  virtual PS3 instruments (uhid)
- `package/yargos-home` + `src/home` - Rust session: boot splash, YARG launcher, and the dashboard shown
  when YARG is closed (needs the network once at build time to fetch its two crates, x11rb and fontdue)
- `tools/` - `flash.sh` (write the image to a USB drive), `br.sh` (build), `dev-test.sh` (try the bridges on a desktop Linux PC), `test-update.sh`,
  and QEMU helpers: `qemu-run.sh`, `qemu-key.sh` (send keys), `qemu-shot.sh` (screenshot), `qemu-sh.sh` (run a command in the guest)

## Runtime design

- `yargos-firstboot.service` copies the seeded YARG into `/data/apps/yarg/<version>` and points
  `/data/apps/yarg/current` at it. YARG runs from there, so newer releases can be dropped in
  without rebuilding the OS (see "Updating YARG").
- YARG runs with `-persistent-data-path /data/yarg -offline`; settings, profiles and the song
  cache live on the data partition. It uses OpenGL (radeonsi); this Buildroot has no RADV Vulkan option.
- The song drive: label it `YARGSONGS` (ext4, exFAT, NTFS or FAT). It mounts read-only at
  `/media/songs` with `nofail`, so a missing drive never blocks boot. Point YARG's song folder at
  `/media/songs` once; the path never changes after that.
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
- Bluetooth pairing: run `yargos-pair [seconds]`. Wiimotes (1+2 held) are connected and trusted;
  HID gaming devices, keyboards and mice are paired and trusted. Pairings persist in `/data/bluetooth`.
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
hold-1+2 flow), Updates (check, install, automatic on/off, go back), Songs (is the drive mounted) and
System (restart, shut down). Instruments navigate like YARG's menus: strum or cymbals move up/down, green
fret/pad = OK, red = Back, the two outer frets/pads = left/right. Power actions go through a root helper
(`yargos-ctl.socket`) that accepts only `poweroff` and `reboot`.

`yargos-home screenshot <screen> out.png` renders any screen with demo data (home, wifi, password, sound,
bluetooth, updates, songs, system, busy, confirm, splash) without a display.

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
press Ctrl+Alt+F2, log in as root (empty password; there is no network login), and run
`iwctl station <wlan> connect "SSID"`. Press Ctrl+Alt+F1 to return to the game.

## Boot entries

On the flashed stick, `loader.conf` defaults to `yargos.conf` (quiet) with a 5 second menu that also offers
`yargos-install.conf` (the same kernel with `yargos.install=1`, which makes the launcher open on its Install
screen) and `yargos-debug.conf` (verbose output, also on the serial console). The installer writes its own
`loader.conf` with `timeout 0` and only the first and last entries, pointing at the new disk's GUID.

## Trying it without the appliance

- `tools/dev-test.sh` (see its header): runs the bridges on a desktop Linux PC. Needs root for
  `/dev/uhid`. `setup` blocks `hid_wiimote` and installs dev udev rules; `teardown` undoes it;
  `drums`, `wii`, `watch` (shows the virtual instrument's reports), `raw` (shows what the Ion kit
  itself sends) and `yarg` (runs a downloaded YARG as your user).
- `tools/qemu-run.sh`: boots the image in QEMU using a throwaway overlay (`HEADLESS=1` writes the
  console to `~/yargos-build/qemu/serial.log`, and `tools/qemu-shot.sh` saves a screenshot; needs
  `qemu-system-x86` and `ovmf`, and membership of the `kvm` group). Use `fresh` after rebuilding the
  image, because the overlay is tied to the old image.

## Status

Verified:

- The defconfig loads and every symbol takes effect (checked against Buildroot 2026.02.3). This
  Buildroot has no "all linux-firmware" option, so the image uses a broad per-chip firmware list.
- A full build completes. Image contents were inspected (partitions, bridge, YARG seed, units, rules,
  `player` user), not booted to YARG.
- Updater: in QEMU the daily check ran from the image, over HTTPS, and reported "up to date (0.15.0)".
  Staging, switching and rolling back are tested only against the local fake server.
- Dashboard (QEMU, real image): all screens open and show live data (network, sound card and volume,
  update check against GitHub, song-drive state), keyboard and mouse (hover and click) work, and Shut down
  powers the guest off through the root helper. Wi-Fi and Bluetooth screens only show their "no adapter"
  states (QEMU has neither); the output parsers are tested on sample text, not on real iwd/BlueZ output.
- Boot flow (QEMU, quiet kernel arguments): the logo splash is on screen at about 3 s, the dashboard takes
  over at about 5 s with PLAY YARG selected, and Enter starts YARG.
- Found by the boot tests and fixed: systemd was built without ACL support (so `uaccess` never gave the
  player user raw-HID access, which instruments need), and the kernel had no fbdev emulation (no `/dev/fb0`, so no splash and no on-screen console).
- Polish (checked in QEMU): `/media/songs` is an automount, so there is no boot wait without the drive;
  wired DHCP works; the WiFi regulatory-database warning is gone; YARG starts about 7 s after power on
  (first boot copies YARG to the data partition, later boots are shorter).
- Wiimote guitar (real hardware, desktop PC): connects over Bluetooth, the bridge lights LED 1 and the
  virtual PS3 guitar reports all five frets, strum up/down, whammy (full range), Start/Select and tilt
  correctly, with no noise at rest; a guitar without a touch bar reports an idle slider. YARG on the
  desktop auto-creates a guitar profile for it and the menus can be navigated with it. Needs the
  desktop workarounds described under Runtime design.
- Under QEMU (software rendering, `HEADLESS=1 tools/qemu-run.sh fresh`): the image boots, first boot seeds
  YARG onto the data partition, rootless Xorg starts on the generic `modesetting` driver, and YARG reaches
  its main menu about 10 s after kernel start. Real AMD hardware (amdgpu, audio, input) is untested.
- YARG keyboard and mouse input (QEMU, fresh data partition): the anti-piracy dialog is dismissed by a click
  on I UNDERSTAND (pointer moved and clicked through XTEST) or by Down then Enter (Enter alone does nothing
  until a button is selected), and the main menu then follows the arrow keys, Enter and Esc. The cause was
  Unity's built-in SDL: with no window manager it uses "legacy" fullscreen, reparents its window into an
  override-redirect container, loses the X focus that way and then drops all input; `run-yarg` sets
  `SDL_VIDEO_X11_LEGACY_FULLSCREEN=0`.
- On a 2020 HP Envy x360 (Ryzen 4000, booted from a USB stick; reported by the owner): the image boots to the
  launcher and into YARG, the touchpad and the built-in and a USB WiFi adapter work, and YARG takes keyboard
  and mouse input. Instruments were not tried on it. A USB stick that answers its first read with "Medium not
  present" can fail to boot (another stick or port worked); `usb-storage.delay_use=5` is set but did not fix that.
- Bridge: 19 unit tests pass. With a real ION Drum Rocker, pad, cymbal, d-pad and kick1 flags
  decode correctly in the virtual kit's reports. On a desktop with `libhidapi-hidraw0` installed,
  YARG recognises the virtual kit through its own PS3 drum layout, auto-creates a drums profile,
  and the menus can be navigated with the pads.

- Trimmed image (2.76 GB -> 2.2 GB): `post-build.sh` drops the Clang/libclc/SPIR-V tools Mesa's build leaves
  behind (nothing links them) and compresses the firmware to `.xz` (404 -> 160 MB). In QEMU the trimmed image
  boots, YARG reaches its menu, and the kernel loads an xz-compressed `regulatory.db` (a corrupt copy fails
  with the expected error, so the check can tell). Real firmware (amdgpu, Wi-Fi, Bluetooth) loading from `.xz`
  is not yet confirmed on hardware.
- Installing to a disk (QEMU, a blank 12 GB virtio disk beside the boot stick): the boot menu's install entry
  opens the Install screen, which lists only the other disk; the install ran to the end, and the installed
  disk by itself booted straight to the launcher (no menu) with YARG 0.15.0. Its partitions, boot entry,
  `fstab`, data copy and blank machine id were checked. An earlier version failed on a leftover install with
  the same `YARGDATA` label because the stick looked its data up by label; both now use GUIDs.
Not verified:
- The installer on real hardware: a real internal SATA/NVMe disk, firmware that ignores the fallback boot
  file or has many stale entries, Secure Boot machines, and disks with other systems on them (it erases all
  of the chosen disk).

- Dashboard navigation with real instruments (the button mapping assumes the kernel's standard HID
  gamepad mapping); the Wi-Fi password flow and Bluetooth pairing on real adapters; the dashboard's pad mapping on a
  real guitar (`tools/dev-test.sh pads` shows it).
- Playing a song on the virtual drums: whether each pad, cymbal and the kick land on the right lane
  in gameplay, and kick2.
- Drum velocity for yellow and green (red looks right; the Ion may not encode velocity as documented
  for Rock Band kits). YARG only uses velocity for the optional bonus on ghost/accent notes and the
  hit-sound volume; a flagged hit always registers (its layout floors the velocity at 1).
- Wiimote guitars: playing a song in YARG (menus work); the World Tour touch bar (only a guitar without
  one was tested); Bluetooth reconnect on a button press after the first connection; and the whole
  Bluetooth path on the appliance image (pairing flow, `hid-generic` binding, bridge autostart).
- Whether `hid-sony` claims the virtual `12BA:0100`/`12BA:0210` devices (uhid is declared as a
  Bluetooth device to avoid it). Fallback: impersonate a Santroller device (`1209:2882`).
- Anything on real hardware: amdgpu, HDMI audio, Bluetooth, WiFi, USB input, boot time.
- Bluetooth Polybar guitars (generic Bluetooth HID is enabled; nothing specific was checked).

Known limits: root filesystem is read-write ext4 for now (read-only root + overlay is a later phase);
the data partition is a fixed 2G.

Not started: a logo splash screen (boot is silent: black screen, no cursor, until YARG appears),
RAUC/A-B OS updates (the OS itself is updated by reflashing), ALSA default-device selection for HDMI/USB
audio, a WiFi setup screen that works without a keyboard.

Not verified: an actual update install on the image (only the up-to-date check ran), WiFi, the power
button (configured, not pressed), VT switching to a maintenance console.

## License

MIT, see `LICENSE`. It covers the files in this repository only. YARG and the software it bundles
(BASS, Discord SDK) keep their own licenses, and the built image must not be redistributed.
