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

## Layout

- `configs/yargos_x86_64_defconfig` - top-level config
- `board/yargos/linux.fragment` - kernel options (broad driver set, UHID, ALSA sequencer, no hid-wiimote)
- `board/yargos/genimage.cfg` - GPT: ESP (256M) / rootfs (3G ext4) / YARGDATA (2G ext4)
- `board/yargos/esp/loader/` - systemd-boot config; the verbose `yargos-debug` entry is the default
  for now (see "Boot entries")
- `board/yargos/rootfs-overlay/` - systemd units, udev rules, bluez config, launch scripts
- `package/yarg` - pinned YARG release, installed as a seed in `/usr/share/yarg-seed`
- `package/yargos-bridge` + `src/bridge` - Rust bridges: Wiimote guitar and wired Xbox 360 drums ->
  virtual PS3 instruments (uhid)
- `tools/` - `br.sh` (build), `dev-test.sh` (try the bridges on a desktop Linux PC), `qemu-run.sh`

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

## Updating YARG

`yargos-update` (shell, uses curl, jq, unzip) updates YARG on the data partition without touching the OS:

- It asks GitHub for the latest release, downloads the Linux zip, and checks it against the SHA-256
  digest GitHub publishes in the release metadata (both over HTTPS). YARG's own `.sig` is not checked
  because its public key is unknown. A release with no digest is refused.
- It unpacks to a staging folder, checks that every library's dependencies load, then atomically
  switches `/data/apps/yarg/current` to the new folder and keeps the old one as `previous`.
- It never touches the running game; the new version starts the next time YARG launches.
- A new version is "on trial": `yarg.service` runs `yargos-update boot-check` before each start and
  `yarg-health.service` calls `mark-good` after 60 s of uptime. Three starts without that and the next start
  rolls back to `previous` and remembers the bad version so it is not offered again.
- `yargos-update.timer` runs `auto` 3 minutes after boot and daily. Set `AUTO_UPDATE=no` in
  `/data/yargos.conf` to turn that off. Other commands: `status`, `check`, `apply [--restart]`, `rollback`.
- `tools/test-update.sh [extracted YARG folder]` tests all of this against a local fake release server.

Networking: wired links use DHCP (systemd-networkd). WiFi is handled by `iwd` (which does its own DHCP):
press Ctrl+Alt+F2, log in as root (empty password; there is no network login), and run
`iwctl station <wlan> connect "SSID"`. Press Ctrl+Alt+F1 to return to the game.

## Boot entries

`loader.conf` defaults to `yargos-debug` (verbose kernel and systemd output, also on the serial
console) with a 3 second menu. For the finished console experience, change it to `default yargos.conf`
and `timeout 0` (silent boot).

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
- Bridge: 19 unit tests pass. With a real ION Drum Rocker, pad, cymbal, d-pad and kick1 flags
  decode correctly in the virtual kit's reports. On a desktop with `libhidapi-hidraw0` installed,
  YARG recognises the virtual kit through its own PS3 drum layout, auto-creates a drums profile,
  and the menus can be navigated with the pads.

Not verified:

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
