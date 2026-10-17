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
- The song drive: label it `YARGSONGS` (ext4, exFAT, NTFS or FAT). It mounts read-only at `/media/songs` (`nofail`, so a
  missing drive never blocks boot) by udev when it appears, at boot or when plugged in (`71-yargos-songs.rules` wants
  `media-songs.mount`, the fstab line; `x-systemd.device-bound` unmounts it when the drive goes). It is deliberately NOT an
  automount: with no drive every look at a path under /media/songs waited 3 s for it, and YARG checks several paths per song
  in its cache, so its start took hours once a drive had been removed (measured: 3-4 s per lookup). With no drive
  /media/songs is an empty folder and lookups fail at once. The path never changes.
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
- Bluetooth headphones as the sound output: BlueZ is built with its audio plugin and `bluez-alsa` provides the ALSA plugin
  (`pcm.bluealsa`, A2DP source only: `bluealsa.service` is overridden to `-p a2dp-source`; SBC, no AAC/aptX so no
  non-free codec libraries). The dashboard pairs a headset in ONE `bluetoothctl` session that registers an agent (a one-shot
  `bluetoothctl pair` has none, and earbuds' pairing is refused halfway), trusts and connects it, and then asks the helper
  for `sound-bt <address>`: asound.conf gets a `bluealsa` slave for that exact address (the address is validated: an ALSA
  config can load programs) and `/data/asound.prefer` records the choice. Before every YARG start `ensure_sound` runs
  `plan_sound`: a preferred headset that is connected plays; one that is switched off is replaced in asound.conf by a real
  sound card (`sound-temp`, which keeps the preference) so YARG always has a default output; picking a sound card forgets
  the preference. The Sound screen lists connected headsets as outputs (their volume is the Volume row, see below). Latency is that of SBC (roughly 150-250 ms): YARG's audio calibration compensates for a steady delay. A2DP
  traffic shares the radio with the Wii guitars; the Wii auto-connect scan only runs on the dashboard, never during YARG (and it
  sits out while an OS or YARG download runs: on many machines Wi-Fi and Bluetooth are one module, and a scan that never stops
  starved a download to "0%"). The headset's ALSA slave is PINNED (`rate 44100`, `channels 2`, `format S16_LE` in `asound_text_bt`)
  and bluealsa runs with `--a2dp-force-audio-cd` (44.1 kHz only): the bluealsa ALSA plugin calls SelectCodec when a program opens
  the PCM at a different rate or channel count, BlueZ then closes and reopens the live stream, and the earbuds did not answer
  ("No reply to Open request"; the 25 s D-Bus timeout is the 22-25 s hang of YARG's audio start, after which the link dropped).
  An older headset config on disk is rewritten at the next YARG start.
- Volume: one level for whatever the sound goes to. A sound card: its mixer (`Master`, else `PCM`/`Speaker`/`Headphone`/`Digital`);
  the first time a card is touched its other playback controls (Speaker, Headphone, PCM...) are opened to 100% so the one knob
  decides, because on many laptops each has its own level. A Bluetooth headset: bluealsa's volume for its A2DP PCM
  (`bluealsa-cli volume`, 0..127), which for A2DP is bluealsa's software scaling (works with every headset; native AVRCP
  absolute volume is a bluealsa option, `--a2dp-volume`, that "might not work with all headsets" and is not used yet). Mute for
  a headset is level zero with the old level remembered. HDMI has no mixer: the TV's. The session grabs the volume keys on
  the root window (XF86AudioRaise/Lower/Mute, and Ctrl+Alt+Up/Down/M; with every Caps/Num Lock combination) so they work in
  YARG too, applies a 5% step in the background, and shows the level in the banner window (the same one the low-battery
  notice uses). The mixer is stored (`alsactl store`, helper word `volume-save`) 3 s after the last key, and each headset's
  level lives in `/data/bluealsa`. `yargos-home volume get|up|down|mute` does the same from a shell.
- Diagnostics: SYSTEM > Diagnostics shows five text reports (Bluetooth, Sound, YARG, System, Keyboard test) read through the root
  helper words `diag-bluetooth|sound|yarg|system|keys`, which run `/usr/lib/yargos/diag NAME` (a fixed list, bounded; only the
  keyboard test touches anything: it resets a USB keyboard that heard nothing). A page is read once when it opens: OK reads it
  again. It exists so that a photo of the screen shows what the system saw, instead of typing commands on the console. The Sound
  page lists the HDMI ports the kernel sees a screen on (the ELD files) and the digital-output switches; the System page lists the
  keyboards, the kernel's and Xorg's view of the last hot-plug, and what `usb-kick` did.
- HDMI sound: the HDA card's `IEC958 Playback Switch` controls (one per HDMI port) start OFF and plain ALSA never turns them on
  (a desktop's sound server does): the helper word `sound-prepare` (and every `sound`/`sound-temp` write) switches them on, run by
  the dashboard when an output is chosen, before the test sound and before every YARG start. The Sound screen tags each HDMI
  output from `/proc/asound/cardN/eld#C.P` ("<monitor> connected" or "nothing connected").
- USB: `72-yargos-usb-power.rules` turns USB autosuspend off for every non-hub device (a mains-powered console), and starts
  `yargos-usb-kick@.service` for the 24ae receiver and for any USB device with a boot keyboard interface: after a hot-plug (never in
  the first 45 s, devices present at start work) it watches the receiver's input nodes and switches the device off and on
  (`authorized` 0/1) with growing waits until a key or movement arrives, logging the raw HID reports and a USB traffic summary for
  the Diagnostics page. It was written for a wireless-keyboard receiver that stayed silent after a replug; that keyboard turned
  out to need its own power switch cycled, so it is a harmless safety net.
- Bluetooth radio discipline: every `bluetoothctl` start registers an advertisement monitor with BlueZ (the journal's "Adv Monitor
  app disconnected" lines) which is work for the controller, so the dashboard keeps its Bluetooth calls few: one `info` per
  device, names remembered (`bt_name`), the adapter powered on once, nothing from the main card's refresh. When PLAY is pressed
  `bt_quiet` waits for the Wii guitar scan in progress to end before the output is chosen; `ensure_sound` then plays one second
  of silence to a chosen headset (`bt_probe`) and, if it does not answer, uses the speakers this time rather than let YARG
  block on a dead link. `bluealsa` runs with `--keep-alive=30` so the repeated opens during YARG's start do not stop and restart
  the stream. (The ~22 s hang that dropped the radio, guitars too, was NOT the dashboard's Bluetooth calls: it was the live codec
  renegotiation described above; the quiet radio and the probe stay as cheap safeguards.)
- Wii guitar batteries: the Wiimote's status report (0x20, answer to a 0x15 request) carries a level byte (0xC8 = full) and
  a "battery low" flag. The bridge writes `/run/yargos-bridge/battery-hidrawN` (`<percent> <low 0|1>`); at connect it asks
  anyway, and while the guitar streams it asks again only after 10 s without any change on it and at most once a minute (a
  status request mid-song is an interruption). A status answer while streaming only records the battery and re-sends the
  reporting mode; it must not restart the extension setup (that recalibrates the touch bar). The launcher shows the level
  on the Bluetooth screen and a LOW BATTERY line on the main card, and `sys::newly_low` announces each guitar once (again
  after it was back above 40%): a toast on the dashboard, and while YARG runs a notice window (override-redirect, made after
  YARG's so it stacks above it, no input, redrawn 4 times a second for 10 s) via `Display::show_banner`. NiMH packs read
  lower than alkalines: the percentage is a guide, the flag and 15% decide "low".
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
has these screens. The main menu: Play (or INSTALL YARG), Sound (pick the output, volume,
test tone; writes `/data/asound.conf`), Bluetooth (scan, pair/connect, forget; Wiimotes get their own
hold-1+2 flow), Connected devices and System. System: Wi-Fi (turn the radio off or on: `iwctl device X set-property Powered`, remembered as `WIFI=no` in `/data/yargos.conf` and applied when the dashboard starts; scan, join, an on-screen keyboard for passwords), Updates (a menu of
System updates: check, install, restart, automatic on/off, go back, from the song drive; and YARG updates: check, install,
automatic on/off, go back), Storage (import songs to the internal drive, browse them and delete a song, a folder or everything
with LEFT; Song drive: the drive, and one YARG song source per folder on it; Use the rest of this drive when there is unused room;
Install on this computer's disk, only while the system runs from a USB or removable drive: `yargos-install where` says
"internal" for a built-in disk) and Diagnostics; Restart and Shut down are two small square icons under the rows (DOWN from the
last row reaches them, LEFT/RIGHT choose, both ask first). BACK walks the same path up with the row you came from highlighted. Instruments navigate like YARG's menus: strum or cymbals move up/down, green
fret/pad = OK, red = Back, the two outer frets/pads = left/right. Everything that needs root goes through a helper
(`yargos-ctl.socket`, `yargos-home ctl`) that accepts only a few fixed requests: `poweroff`, `reboot`, `install /dev/<disk>`,
`sound <card> <device>`, `songs-mount /dev/<partition>`, `songs-umount`, `grow-data` and the system update words (`os-check`, `os-update`, `os-cancel`, `os-goback`, `os-ack`).

`yargos-home screenshot <screen> out.png` renders any screen with demo data (home, wifi, password, sound,
bluetooth, bt-battery, home-battery, sound-bt, bt-audio, diag, updates, songs, system, devices, yarg, os-update, storage, import, import-confirm, manage, manage-songs,
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

The system updates itself like a console: `yargos-os-update` (root; the dashboard's SYSTEM > Updates > System updates reaches it
through the root helper) downloads a signed release into the OTHER system slot, adds a boot entry that tries it twice and
never restarts by itself. The new system must bring up the launcher and keep it running (`yargos-boot-ok`), otherwise the
boot loader falls back to the old system after two tries and the update is remembered as rejected. SYSTEM > Updates > System updates
also has automatic updates on/off (`OS_AUTO_UPDATE=no`), a switch for test versions (`OS_CHANNEL=beta`: the device then reads `beta.json` instead of `stable.json`; see `tools/publish.sh` and `tools/promote.sh`), restart to use a waiting update, and going back to the earlier
system. Release files on the song drive: every folder (top level and one level down) is checked (signature, image present with the right size) and the highest version wins; `stable.json` and `beta.json` both count (`offline_pick`). The System updates screen looks at the song drive again every 4 s while it is open (`Cmd::OsFindOffline`, a result that only changes the "Install from the song drive" row), and OK on that row with nothing found looks again at once and says what it found: a drive plugged in after the screen opened is noticed. `tools/release.sh` makes and signs a release (`YARGOS_VERSION=2026.11.1 tools/br.sh` first); it refuses an image that
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
