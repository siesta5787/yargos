# Status: what has been verified

Honest list. "Reported" means the owner tried it on real hardware and said so; everything else was checked in QEMU or by
unit tests and scripts (`cargo test` in `src/home` and `src/bridge`, `tools/test-update.sh`, `tools/test-os-update.sh`).
Nothing is listed as working unless it was run.

## Confirmed on real hardware (reported)

Test machine: a 2020 HP Envy x360 (Ryzen 5 4000, Radeon graphics, NVMe), booted from a 256 GB USB stick and also
installed to its internal disk. A BOSGAME E5 Plus mini PC is the intended target and has not been tried yet.

- Boot to the launcher, Wi-Fi (built-in and a USB adapter), sound, touchpad, keyboard and mouse, an NTFS song drive on a USB
  flash drive, HDMI mirroring with the laptop lid closed (the TV can show glitches for a few seconds after a change; the newest
  image avoids re-programming an output that does not change, not yet re-checked).
- Installing YARG from the dashboard (download, unpack, run: under 30 s) and playing: several full songs with drums
  (ION Drum Rocker), Wii guitar and microphone vocals at the same time. A USB piano (Yamaha P-143) connects and its speaker
  works; no song has been played on piano yet.
- Installing the system to the laptop's internal disk.

## Not yet tried on real hardware

- The A/B system update (it has only run in QEMU), the automatic Wii connect, the sound-quality fix (sample-rate conversion),
  the newest dashboard screens, a Rii 2.4 GHz keyboard receiver (its keys did nothing; cause unknown), drums in every lane,
  the guitar's touch bar, pitch detection from a real microphone, eMMC / SD readers / Intel VMD disks, Secure Boot machines
  (not supported: turn it off), the mini PC.
- A laptop disk installed with the pre-A/B installer has no second system slot: system updates cannot work there until it is
  reinstalled from a current stick (SYSTEM > Install on this computer's disk). Running from the USB stick needs no install.
- Wii guitar battery (level on the Bluetooth screen, low notice on the dashboard and over the game): unit-tested and run in
  QEMU with fake guitars (`tools/test-battery.sh`). Not tried with a real Wiimote (how its level byte reads on your
  rechargeable packs, whether a status request while idle disturbs the real guitar) or over a real YARG window (the notice
  was only seen over a stand-in program).
- The song drive is mounted by udev (hotplug and at boot) instead of an automount, after YARG sat on "scanning songs" for
  hours with its old drive removed (each lookup under /media/songs waited 3 s). Measured and fixed in QEMU
  (`tools/test-songdrive.sh`); not yet seen on the real machine. The first scan of newly imported songs from a USB stick may
  still take a while: that is YARG reading thousands of files.
- SYSTEM > Storage (import a portable drive to the internal songs, browse, delete): unit-tested, and `tools/test-songs.sh`
  drives the real dashboard in QEMU with three fake ext4 USB drives (import, same-name drives, a drive already mounted as
  the song drive, YARG's folder list, deleting a folder, a song and everything: all pass). Not tried on real hardware: real
  exFAT/NTFS drives, an SD card, a big library (copy speed, how long reading a 1 TB drive takes before the question),
  cutting the power mid-copy, and the guitar as the only controller.
- System update on real hardware: the first try, 2026.10.13 -> 2026.10.14 from the USB stick, failed with "Download failed".
  The real cause (seen on the console): "No space left on device": the stick's data partition was still the image's 700 MB (full
  with YARG), so the boot step that grows it to fill the stick had not worked on that stick. Why it did not is NOT known: it
  grows fine in QEMU, on a virtual disk, an NVMe disk and a USB-attached disk. Now: the dashboard notices unused room after the
  data partition, asks the system to use it (once at start, and SYSTEM > Storage > Use the rest of this drive), and the
  updater checks free space first and says so. The updater also retries, resumes, says why it failed, and can install from the
  song drive. Tested in QEMU (including a boot with the boot-time grow switched off); not yet on the real stick.
- YARG's "Failed to initialize audio device" on the laptop: the suspected cause (ALSA's default is card 0 device 0, an
  HDMI-only card on AMD, or the saved card number moved when USB devices came and went) is a guess. Fix written (the
  output is saved by card id, and one is chosen before YARG starts if the saved one is gone) and unit-tested only.
- Whether `hid-sony` claims the virtual `12BA:0100` / `12BA:0210` devices on every kernel (fallback: impersonate a Santroller
  device, `1209:2882`).
- Playing songs with the virtual drums in every lane (pads, cymbals, kick, kick2) and drum velocity for yellow and green
  (YARG only uses velocity for bonus notes and hit-sound volume; a flagged hit always registers).
- The installer on a real machine whose firmware ignores the fallback boot file or has many stale entries, and disks that
  hold another system (it erases all of the chosen disk).
- The dashboard's navigation with a real guitar's buttons, the Wi-Fi password flow and Bluetooth pairing on real adapters,
  Bluetooth "Polybar" guitars (generic Bluetooth HID is enabled, nothing specific was checked).

## Known limits

- The root filesystem is read-write ext4 (a read-only root with an overlay is a possible later step).
- System updates download a whole image (about 230-400 MB), no deltas.
- The console has an empty-password root login on VT2 (physical access only; there is no network login of any kind).
- YARG's own `.sig` is not checked (its public key is not published); the download is checked against the SHA-256 GitHub
  publishes for the release.

## Detailed log (QEMU and tests)

- Published release (2026-10-12 UTC): `os-2026.10.13` (system image for updates, the flashable disk image, the legal bundle) and
  the `os-channel` release holding `stable.json` + signature. Checked from outside, anonymously: the channel files download and
  the signature verifies against the key in the repo; the updater shipped inside the image (extracted from it) reports "update
  available" for an older version and "up to date" for 2026.10.13; the update payload downloads through GitHub's redirect and
  matches the signed manifest; the flashable disk image decompresses to exactly the built image and boots to the dashboard
  (QEMU). Not yet run: a real device installing an update from GitHub (2026.10.13 is the first version there is to update
  from; the first real update will be the next release).

- First public release (found by checking the published files from outside): the system updater fetched the update channel
  without following redirects, and GitHub always redirects, so a device would have saved an empty file and refused it as
  "not signed by us". Fixed (`curl -L` and an empty-file check) in 2026.10.13; the test server now redirects like GitHub
  (`tools/redirect-server.py`). The first image (2026.10.12) was withdrawn the same hour.

- The defconfig loads and every symbol takes effect (checked against Buildroot 2026.02.3). This
  Buildroot has no "all linux-firmware" option, so the image uses a broad per-chip firmware list.
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
- Bridge: 19 unit tests pass. With a real ION Drum Rocker, pad, cymbal, d-pad and kick1 flags
  decode correctly in the virtual kit's reports. On a desktop with `libhidapi-hidraw0` installed,
  YARG recognises the virtual kit through its own PS3 drum layout, auto-creates a drums profile,
  and the menus can be navigated with the pads.
- Trimmed image (2.76 GB -> 2.4 GB): `post-build.sh` drops the Clang/libclc/SPIR-V tools Mesa's build leaves
  behind (nothing links them). Firmware compression to `.xz` was tried (404 -> 160 MB, and the kernel read a
  compressed `regulatory.db` in QEMU) but the first real-hardware run lost Wi-Fi and sound, so firmware is
  plain again until it can be tested on the real drivers.
- Song drive: an NTFS drive labelled YARGSONGS (QEMU, as a USB drive) mounts read-only at `/media/songs` through
  ntfs-3g and its files can be read; `run-yarg` gives YARG one song source for each top-level folder on the drive that has songs
  (`usr/lib/yargos/song-sources`; system folders skipped; a flat drive is one source), replacing its sources
  under `/media/songs` and keeping any others; the "Add song folders to YARG" switch on the Songs screen (`AUTO_SONG_SOURCES` in `/data/yargos.conf`) turns it off. The
  Songs screen also lists the connected drive with its free space and the sources found. The switch was toggled in QEMU
  and saved; the drive was an ext4 test drive, not a real NTFS one.
  Checked in QEMU with an ext4 drive of three song folders, an empty one and Windows system folders. The Linux kernel's own NTFS driver refused a real Windows-formatted drive
  ("Failed to load $BadClus", "volume is dirty"); ntfs-3g mounted that same drive read-only on the dev PC.
  The kernel now has FUSE and `mount.ntfs` points at ntfs-3g. exFAT, FAT and ext4 are built into the kernel.
- YARG's audio add-ons: with no `LD_LIBRARY_PATH`, YARG could not load Opus sound effects or songs (23 sound
  effects, 90 "Failed to load ... .opus: OK!" errors) and failed to initialise any microphone ("Failed to
  initialize microphone", "Failed to create effects handle for Freeverb"). `run-yarg` now puts YARG's Plugins
  folder on the library path: 0 failures, and connecting a (simulated QEMU) sound card as a microphone no
  longer errors. A real USB microphone has not been tried since. One `NullReferenceException` in
  `BassMicDevice.ProcessRecordData` still appears in YARG's log at startup in QEMU (never on the dev PC);
  nothing visibly breaks.
- First boot on a big drive (QEMU, a 256 GB overlay of the image): earlier images never ran the first-boot service
  there (so YARG was "not installed" and the Sound screen got "Permission denied"); fixed, and with the fix
  `/data/apps/yarg/current` exists, `/data` belongs to `player` and YARG starts. Also reproduced and fixed only
  in QEMU; the real laptop has not run this image yet.
- Wiimote readiness: connecting a Wiimote from the Bluetooth screen now waits for its guitar bridge to run (and stay
  up for two seconds) and otherwise disconnects it and shows an ATTENTION box that waits for OK; a connected Wiimote
  shows READY or NOT READY on its row. Checked in QEMU with the fake Wiimote (`yargos-home wiicheck`: bridge running,
  bridge stopped, no device). Not tried with real Bluetooth.
- First run without YARG (the default image has none; QEMU, 256 GB overlay, real GitHub): the main menu shows INSTALL YARG
  and "YARG: not installed"; the screen downloaded the 122 MB release with a live percentage, checked the SHA-256 from
  GitHub's metadata, unpacked, ran the library check and the menu became PLAY YARG. The same with a zip on a (fake) song
  drive, with no download. Offline, an already installed system, a zip without a version in its name and a corrupt zip
  all end in a clear message and leave nothing behind (tools/test-update.sh covers the first-install cases too). Not yet
  tried on the laptop; the private "with YARG" variant builds and carries the seed.
- Fixed in the same round: `firstboot.sh` used `stat`, which this image does not have, so it threw away the Sound choice
  at every boot (it now uses `[ -O file ]`; the choice survives a reboot in QEMU).
- System updates over the air (Phase 1 + 2 of docs/OTA-PLAN.md, all in QEMU with `tools/test-os-update.sh`, ~35 min, all
  passing): a signed release downloads (resumable), is written to the other slot, checked (size, SHA-256 read-back,
  fsck, version and kernel inside) and gets a boot entry that tries it twice; on restart the new system is judged by
  `yargos-boot-ok` and blessed, and `settle` closes the books. Also tested: a bad update (no launcher) falls back to the
  old system by itself after two tries, is recorded and not offered again; a power cut mid-write leaves the running system
  alone and the update installs on the next try; going back; the installer's new layout; an installed NVMe disk (random
  GUIDs) boots alone and updates itself; the dashboard path (SYSTEM > System update: check, install with a percentage,
  BACK cancels, "Restart now?"). The system needs ONE re-flash and re-install (new layout, 5.3 GB image). NOT done: nothing
  is published, so the default update address (GitHub release `os-channel` of this repo) answers "could not reach the
  update server" until the owner publishes a first release (see tools/release.sh); not tried on the laptop; chunked/delta
  downloads (a release is a ~230-400 MB download); automatic updates were only exercised by calling `auto` directly.
- Install YARG on a slow stick (owner's report: "stuck unpacking", wheel not moving, after a forced reboot every retry said
  "another update is running"). Fixed, checked in QEMU with throttled disks and a hard kill mid-install: (1) the lock
  belongs to a process of this boot, so one left by a reboot or power cut is cleared (with the half-unpacked folder and
  the partial download); (2) unpacking shows a percentage like the download does; (3) BACK cancels a download/install
  (stops curl/unzip, removes everything, says nothing was changed); (4) the spinner runs on the clock, not on idle
  moments: any steady stream of events (pointer, a controller) used to freeze it; (5) the gradient backdrop is made once
  instead of every frame and pixels go to X without a copy (frames were ~170 ms in software); (6) the kernel keeps at
  most 32 MB of unwritten data (`sysctl.d`), so a ~500 MB unpack cannot leave the stick busy for minutes. The real cause
  of the owner's freeze is not proven: in QEMU the dashboard never stalled, even with all disk I/O limited to 5 MB/s.
  Confirmed by the owner on the next image: download, unpack and install took under 30 seconds on the laptop; the first
  freeze was never explained.
  `YARGOS_DEBUG_FPS=1` (kernel command line `systemd.setenv=YARGOS_DEBUG_FPS=1`) logs the frame rate, slow frames and
  any gap in the dashboard loop to the journal.
- Security hardening (Phase 0 of docs/OTA-PLAN.md), checked in QEMU on a 256 GB drive: the player cannot rename,
  replace or add anything in /data's first level or write asound.conf; links planted where root works (apps/yarg, yarg,
  yargos.conf) are replaced and what they pointed at is untouched; the YARG seed is installed by the player; the daily
  update runs as the player (sandboxed) and still reaches GitHub; the Sound screen saves through the root helper; iwd
  (Wi-Fi passwords), ALSA state and Bluetooth live on /data. Not tried: the carry-over of an older system's iwd/ALSA
  state, and the installer with this layout (its copy code is unchanged).
- Wii guitars reconnecting (reported by the owner: a guitar that "syncs wrong" stays dead, after a reboot only one of
  two connects, a timed-out bridge never retries). Fixed in the newest image, checked in QEMU with the fake Wiimote
  (`tools/fake-wiimote.rs`, which now answers like a guitar, and can drop its first status requests): (1) the bridge asks
  for the guitar's status every 3 s until it answers, instead of once, and writes `/run/yargos-bridge/ready-hidrawN` when
  the guitar streams: the launcher's READY means that, not just "bridge running"; (2) the bridge unit has no start limit;
  (3) player-LED slots are tied to the bridge's pid, so ones left by killed bridges are reused (they used to run out);
  (4) the connect check looked for ANY Wiimote HID device, so with one guitar up the second was "connected" after a single
  try: it now looks for that Wiimote's address; (5) while the dashboard is up (not while YARG runs) the launcher listens
  for 5 s at a time and connects any Nintendo RVL-CNT controller that is calling (1+2 held), so no trip to the Bluetooth
  screen is needed; a failed attempt is retried the next time 1+2 is pressed. The BlueZ half (a real Wiimote, `bluetoothctl
  scan on` output parsed by `parse_scan_wiimotes`) is unit-tested on sample output only: not tried on real hardware.
- Devices screen: one row per thing YARG can use (keyboards and mice with their USB id, sound cards, MIDI ports, pads);
  a raw USB row only for a device the kernel made nothing of; a keyboard whose receiver has no input device says NO INPUT.
  The owner's Rii 2.4G keyboard receiver was listed by the old screen but typed nothing: not diagnosed yet (needs the
  vendor:product id and a check that the keyboard itself is switched on; a standard USB keyboard hot-plugs fine in QEMU).
- Wii bridge lifetime (QEMU, fake Wiimote): the bridge starts when a Wiimote's HID device appears, is restarted
  two seconds after it dies while the Wiimote is still there, and stops when the Wiimote disconnects
  (`BindsTo` its hidraw device, `Restart=on-failure`). The drum bridge was left as it was.
- NVMe and other internal storage (QEMU, a virtual NVMe disk): the kernel had no NVMe/MMC/VMD driver, so a laptop's
  internal SSD was invisible and an installed system could not have booted from it (no initramfs, so they are built
  in). Fixed: the installer lists the NVMe disk, installs, and the installed disk boots alone to the launcher.
  eMMC, SD readers and Intel VMD were added the same way but are not tested.
- Screens: every connected screen mirrors at the biggest resolution all of them share; with the lid shut and an
  external screen connected the laptop panel is turned off (`src/home/src/screens.rs`, polled
  every 2 s, lid read from /proc/acpi/button/lid). A lone screen uses its preferred mode. The planner has unit tests;
  in QEMU a forced resolution change is restored by the watcher and the launcher resizes. A real second screen and
  the lid work on the laptop (QEMU has one connected output). Outputs that already show the right mode are left
  alone: re-programming an HDMI TV's link makes it glitch for seconds. `yargos-home screens` prints what it sees and plans.
- SYSTEM > Connected devices lists USB devices, MIDI ports, sound cards and controllers (QEMU: keyboard, mouse, sound
  card). The "Midi Through" loopback port is no longer created (modprobe `install snd-seq-dummy /bin/true`) so YARG's
  device list only shows real devices. A USB piano (Yamaha P-143) works; it shows as two ports, MIDI 1 is the keys.
- Sound quality: when a program's sample rate is not one the sound card does (44.1 kHz music on a USB card that only
  does 48 kHz), ALSA converts it, and the converter this image had (the built-in one, no ALSA plugins) turned a
  clean 10 kHz test tone into one with a false tone only 23 dB below it: harsh, tinny. `alsa-plugins` + `libsamplerate`
  are now in the image and `etc/alsa/conf.d/50-yargos-rate.conf` makes `samplerate_best` the default: the same test
  now leaves nothing above -94 dB. Measured in the guest with ALSA writing to a file (`tools/audio-test/`); the QEMU
  USB sound card only does 48 kHz so it is a faithful stand-in for the conversion case. Not yet heard on the real
  piano: if the P-143 (or whatever the output is) accepts 44.1 kHz itself, no conversion happens and the harshness has
  another cause (check what rates it offers: it is on the to-do list for the Connected devices screen).
- Installing to a disk (QEMU, a blank 12 GB virtio disk beside the boot stick): the boot menu's install entry
  opens the Install screen, which lists only the other disk; the install ran to the end, and the installed
  disk by itself booted straight to the launcher (no menu) with YARG 0.15.0. Its partitions, boot entry,
  `fstab`, data copy and blank machine id were checked. An earlier version failed on a leftover install with
  the same `YARGDATA` label because the stick looked its data up by label; both now use GUIDs.
