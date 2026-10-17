# YARG OS

A small Linux system that turns a PC into a game console for [YARG](https://github.com/YARC-Official/YARG), the free
open-source rhythm game. Plug in a USB stick, power on, and it boots straight into a dashboard and then YARG: no desktop, no
terminal, controller-friendly menus. Wii guitars, Xbox 360 drums, USB microphones and MIDI pianos work.

> **Unofficial.** This is a community project, not made or endorsed by the YARG team (YARC). YARG is **not included**: the
> device downloads it from YARC's own releases. See [docs/LICENSING.md](docs/LICENSING.md).

It is built with [Buildroot](https://buildroot.org) for x86_64 UEFI PCs (AMD and Intel graphics), is about 5 GB on disk, and
updates itself like a console.

## What it does

- **Boots like a console:** UEFI -> splash -> dashboard (Wi-Fi, Sound, Bluetooth, Updates, Songs, System) -> YARG fullscreen.
  Everything is reachable with a game controller, keyboard or mouse. Nobody needs a terminal.
- **YARG on demand:** the first start offers INSTALL YARG (a ~120 MB download with a percentage, checked against the SHA-256
  GitHub publishes; or install from a zip on your song drive when offline). YARG updates itself over the air without touching
  the system, and rolls back if a new version does not stay up.
- **Instruments:**
  - Wii guitars over Bluetooth (hold 1+2) and the ION Drum Rocker (Xbox 360) are turned into standard PlayStation
    instruments that YARG already understands (`src/bridge`).
  - USB microphones and USB MIDI pianos/drums/keys work as in YARG on any Linux.
- **Wii guitar batteries:** the Bluetooth screen shows each connected guitar's level (`READY 82%`, an amber `LOW BATTERY`
  at 15% or when the Wiimote itself says so), the main menu card lists low ones, and a notice is shown once when a guitar
  runs low: on the dashboard, and drawn over YARG itself while you play.
- **Volume:** the Sound screen has a Volume row (the speakers or a USB sound card, or Bluetooth headphones), and the volume
  keys (or Ctrl+Alt+Up / Down / M) work everywhere, also inside YARG, with a level bar over the screen. HDMI volume is the TV's.
- **Bluetooth headphones:** earbuds and headphones pair on the Bluetooth screen and become a choice on the Sound screen
  (SBC audio through `bluez-alsa`; no sound server). If they are switched off when YARG starts, the speakers play. Tried with
  Anker Soundcore Liberty 4 earbuds and a Wii guitar at the same time.
- **HDMI sound:** the Sound screen says which HDMI output has a screen on it (the graphics chip offers three), and the
  digital output is switched on for you.
- **Diagnostics:** SYSTEM > Diagnostics shows what the system sees (Bluetooth, Sound, YARG, System, and a keyboard test) as text
  made to be photographed, so nobody needs a terminal to report a problem.
- **Song drive:** any drive labelled `YARGSONGS` (exFAT, NTFS, FAT, ext4) mounts itself; each top-level folder that holds songs
  becomes its own song source in YARG.
- **Internal songs:** SYSTEM > Storage copies a whole portable drive (folders exactly as they are) onto the computer's
  own disk, so the songs are there without the drive. Imported songs become YARG song sources like the song drive does,
  and a song, a folder or everything can be deleted again from the same screen, all with a game controller. No formatting:
  the songs go in a folder on the data partition, which already fills the disk; the last few GB stay free for the system.
- **Screens:** HDMI/TV mirroring, and a closed laptop lid keeps the external screen on.
- **System updates over the air:** two system slots (A/B), signed releases, automatic fallback if a new system does not come up.
  It never restarts by itself. See [Updates](#updates).
- **Install to the computer's disk** from the same dashboard, so it starts without the USB stick.
- **Closed to the network:** no ssh, no remote login, nothing listening. See [Security](#security).

## Try it

You need an x86_64 PC with UEFI, a USB stick of 8 GB or more (USB 3 is nicer), and internet for the first run.

1. Download `yargos-<version>.img.xz` from the [releases](https://github.com/siesta5787/yargos/releases) (or build it, see
   below) and write it to the stick. From a checkout of this repository on Linux:
   `IMG=yargos-<version>.img.xz sudo -E tools/flash.sh` (lists drives, asks you to retype the one to erase, verifies; it takes
   the compressed file as it is). Everything on the stick is erased.
2. Turn **Secure Boot off** in the PC's firmware settings (the boot loader is unsigned), and boot from the stick (usually
   F7, F11, F12 or Del at power-on). A small menu shows for 5 seconds: *YARG OS* (default), *Install YARG OS on this computer*,
   and a verbose troubleshooting entry.
3. Connect Wi-Fi (WI-FI), then choose **INSTALL YARG**. Name your song drive `YARGSONGS` and plug it in.
4. Choose PLAY YARG. To keep it on the computer's own disk: SYSTEM > *Install on this computer's disk*.

The first boot grows the data partition to fill the stick. Wired networks work without setup.

## Updates

- **YARG** is updated by `yargos-update` (daily, can be switched off on the Updates screen). The new version runs "on trial"
  and is rolled back if it does not stay up.
- **The system** is updated by `yargos-os-update` (SYSTEM > System update; a daily check can be switched off). A signed
  release is written to the other system slot and gets a boot entry that is tried twice; if the new system does not bring
  up the dashboard and keep it running, the boot loader falls back to the old one by itself and the update is not tried
  again. "Go back to the earlier system" is one press. Releases are signed (Ed25519); the public keys are in
  `board/yargos/rootfs-overlay/usr/lib/yargos/keys/`. A failed download says why (no DNS, a stalled connection, an HTTP
  error, a full disk) and is retried, resuming where it stopped.
- **No internet on the device?** Download `rootfs-<version>.img.xz`, `stable.json` and `stable.json.sig` from the release
  page (the `os-<version>` release) on another computer, put the three files on the song drive (top level or one folder
  down) and choose SYSTEM > System update > *Install from the song drive*. Same signature and checksum checks as a download.

Design and the reasoning: [docs/OTA-PLAN.md](docs/OTA-PLAN.md), [docs/DESIGN.md](docs/DESIGN.md).

## Security

The device is meant to be harmless on a home network: no ssh or other login service, no open ports (LLMNR and mDNS are
switched off; only the DHCP client and loopback sockets exist), YARG runs as an unprivileged `player` user, and the few
things that need root go through a helper that accepts a short list of fixed requests. Root never runs or follows files the
`player` can write (`/data` and its first-level folders belong to root). System updates are signed and cannot be downgraded.

Known limit: the **console** (Ctrl+Alt+F2) has a root login with an empty password. It needs physical access to the machine
(there is no network login), and a person at the boot menu can edit the kernel command line anyway, but if the machine is
used where that matters, treat it as unsecured against someone at the keyboard.

Report security problems by opening a GitHub issue (or contact the maintainer privately through their GitHub profile).

## Building from source

The first build takes about 10 hours on 4 cores (mostly LLVM/Clang for Mesa); later builds only redo what changed.

    git clone --branch 2026.02.3 https://github.com/buildroot/buildroot.git
    cd buildroot
    make BR2_EXTERNAL=/path/to/yargos yargos_x86_64_defconfig
    make                    # output/images/yargos.img (about 5.3 GB)

You need the usual Buildroot host tools plus `unzip`, and network access once (the Rust crates `x11rb` and `fontdue` are
fetched and vendored by a build hook). The image does not contain YARG; `YARGOS_WITH_YARG=1` (see `tools/br.sh`) builds a
**private** test image that does, which must never be published ([licensing](docs/LICENSING.md)).

The maintainer builds through `tools/br.sh`, which also keeps the build tree in `~/yargos-build` working (see the comments
in the script); `YARGOS_VERSION=2026.11.1 tools/br.sh` stamps a release version.

### Publishing a release (maintainers and forks)

    YARGOS_VERSION=2026.11.1 tools/br.sh
    tools/release.sh 2026.11.1 --github --notes "What is new"     # image, signed manifest, licence + source bundle
    tools/publish.sh 2026.11.1                                    # shows the plan; add --yes to publish with `gh`

Devices look for updates at the GitHub release `os-channel` of this repository. **If you fork**, replace the signing keys
(`board/yargos/rootfs-overlay/usr/lib/yargos/keys/`) and the address `DEFAULT_URL` in `usr/bin/yargos-os-update` with your
own, and keep your private key out of the repository. Details in [docs/LICENSING.md](docs/LICENSING.md).

## Repository layout

- `configs/yargos_x86_64_defconfig`, `board/yargos/` - Buildroot config, kernel options, image layout, the root filesystem
  overlay (systemd units, udev rules, update and install scripts).
- `src/home` - `yargos-home` (Rust): boot splash, session, dashboard, and the root helper. `src/bridge` - `yargos-bridge`
  (Rust): Wii guitar and Xbox drum bridges.
- `package/` - Buildroot packages for the two programs and the (optional) YARG seed.
- `tools/` - flashing, building, release and publishing, QEMU helpers and tests (`test-update.sh`, `test-os-update.sh`).
- `docs/` - [design notes](docs/DESIGN.md), [update design](docs/OTA-PLAN.md), [what is verified](docs/STATUS.md),
  [licensing](docs/LICENSING.md). `CLAUDE.md` holds working rules for AI-assisted development.

## Testing

`cargo test` in `src/home` and `src/bridge`; `tools/test-update.sh` (YARG updater against a fake release server);
`tools/test-os-update.sh` (system updates in QEMU: good update, bad update with automatic fallback, power cut mid-write, going
back, install to a second disk; about 35 minutes); `tools/test-songdrive.sh` (the song drive's hotplug mount, no waiting without it, the status card), `tools/test-battery.sh`, `tools/test-songs.sh` (importing portable drives and deleting internal songs in QEMU,
with key presses on the real dashboard; about 10 minutes). `tools/qemu-run.sh` boots the image in QEMU (`HEADLESS=1` for a serial
console; see its header) and `yargos-home screenshot <screen> out.png` renders any dashboard screen without a display.

## Status

Tested on a 2020 HP laptop (Ryzen 4000) from a USB stick and installed to its disk (boot, Wi-Fi, sound, YARG, NTFS song
drive, Wii guitar, ION drums, microphone, USB piano, HDMI mirroring), and on the BOSGAME E5 Plus mini PC the system is meant for
(HDMI sound, Bluetooth earbuds, importing songs to the internal disk, USB keyboards, and over-the-air system updates, 2026.10.14
to 2026.10.26). Still unproven: the offline update from a song drive, a low-battery notice drawn over a real YARG window. A cheap 2.4 GHz
wireless keyboard needed switching off and on after being replugged (a standard USB keyboard does not). The full, honest list of
what is and is not verified is in [docs/STATUS.md](docs/STATUS.md).

## License

MIT for this repository, see [LICENSE](LICENSE). The built system contains many other packages under their own licences
(Linux is GPL-2.0, and so on); every release comes with the licences and the source of the GPL/LGPL parts. YARG is not
included. Details: [docs/LICENSING.md](docs/LICENSING.md).

## Credits

[YARG](https://github.com/YARC-Official/YARG) and the YARC community; [Buildroot](https://buildroot.org);
[PlasticBand](https://github.com/TheNathannator/PlasticBand) for the instrument report layouts; the authors of x11rb, fontdue
and every package in the image.
