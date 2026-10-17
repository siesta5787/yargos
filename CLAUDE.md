# YARGOS

Buildroot external tree that builds a console-style YARG appliance image (x86_64). Read `README.md`
first (overview), then `docs/DESIGN.md` (how it works) and `docs/STATUS.md` (what is verified and what is not). Keep that
list honest: do not move something to "verified" unless it was actually run. `docs/LICENSING.md` has the licensing rules.
The repository is public: no secrets, no private keys, nothing that is not ours to publish (docs/LICENSING.md).

## Rules that are easy to get wrong

- Build with `tools/br.sh`, never `make` directly in the output folder. The build lives in
  `~/yargos-build/` but ~480 host-tool files embed `/tmp/brout` and `/tmp/buildroot`; `br.sh` recreates
  those symlinks (`/tmp` is wiped at boot). Do not move or delete `~/yargos-build` (a rebuild is ~10 h).
- Buildroot is 2026.02.3. Check symbol names in its `Config.in` files before using them: it has no
  `LINUX_FIRMWARE_ALL`, no Mesa RADV option, and no `host-unzip`. After editing the defconfig, re-run
  `make yargos_x86_64_defconfig` and confirm each symbol is set in `.config`.
- Config changes do not rebuild packages automatically: use `tools/br.sh <pkg>-rebuild` or
  `-reconfigure`. Buildroot does not rebuild dependents either.
- Never publish anything that contains YARG's release build (a `YARGOS_WITH_YARG=1` image): it bundles BASS (whose licence
  covers only the developer's own software, "not the publishing of other's software"), the Discord SDK and Unity's runtime.
  The default (clean) image has none of it and MAY be published, but only together with its legal bundle (licences + the
  source of the GPL/LGPL parts: `tools/release.sh` builds it, `tools/publish.sh` insists). `release.sh` refuses an image
  with YARG in it. The repo only holds YARG's download address and checksum. Details: docs/LICENSING.md. Secure Boot is
  not planned.
- `genimage.cfg` boot-entry changes require `fresh` for `tools/qemu-run.sh` (stale qcow2 overlay).

## The bridge (`src/bridge`)

Dependency-free Rust (`cargo test`, `cargo build --release`). One binary, modes `wii` and `drums`,
each turning an instrument YARG cannot read on Linux into a virtual PS3 instrument through
`/dev/uhid` (root only). Report layouts come from PlasticBand's docs
(https://github.com/TheNathannator/PlasticBand), the Xbox side from the kernel's `xpad.c`.
New instrument = new module + mode in `main.rs` + udev rule line + systemd unit.
The udev rules match HID devices with `KERNELS=="0005:057E:0306.*"` (bus:vendor:product with 4 hex digits each); the
8-digit `HID_ID` form never matches there. `tools/fake-wiimote.rs` checks the Wiimote rule in QEMU without hardware.
The launcher treats a Wiimote as usable only when `yargos-bridge-wii@hidrawN` is active (`yargos-home wiicheck [addr]`
shows what it sees); "connected" in BlueZ alone is not enough.
A Wiimote is ready only when `/run/yargos-bridge/ready-hidrawN` exists (the bridge streams) and its unit is active; the bridge
keeps asking the Wiimote for its status until it answers. `tools/fake-wiimote.rs [address] [ignore] [seconds]` imitates a
Wiimote with a guitar for QEMU (build with `rustc --edition 2021`, fetch with curl, run in the verbose boot).
Never test "is a Wiimote there" without its address: one connected guitar made the second look connected.
The bridge writes `/run/yargos-bridge/battery-hidrawN` from the status report's level byte (buf[6], 0xC8 = full) and flag (buf[3]
bit 0). Never ask a streaming guitar for its status while it is being played (it only asks after 10 s idle), and a 0x20 report
while streaming must only record the battery: the setup path recalibrates the touch bar. `fake-wiimote.rs` takes BATTERY and LOW.
Hardware-dependent facts (button bits, ranges) must be tested on the real device with
`tools/dev-test.sh`; unit tests only cover decoding against the documented layout.

## The session and dashboard (`src/home`)

Rust with two crates (x11rb, fontdue), software rendering. `model.rs` is a pure state machine (inputs ->
state + `Cmd`s, results -> `apply`) with unit tests; `ui.rs` draws it; `sys.rs` wraps iwctl/amixer/bluetoothctl/
yargos-update with parsers that are tested on sample output; `session.rs` is the X session loop. Preview any
screen without a display: `yargos-home screenshot <screen> out.png`. Buildroot builds it offline, so the
package vendors the crates in a pre-build hook (needs network once). To test on the image:
`CMDLINE="systemd.setenv=YARGOS_YARG_CMD=/bin/true" HEADLESS=1 tools/qemu-run.sh fresh` shows the dashboard at
once; `tools/qemu-key.sh`, `qemu-shot.sh` and `qemu-sh.sh` drive and inspect the guest.

## YARG's runtime

`usr/lib/yargos/run-yarg` must keep `LD_LIBRARY_PATH` pointing at `YARG_Data/Plugins`: BASS loads its add-ons (Opus,
FX) by bare name, and without it Opus audio and the microphone fail with misleading messages ("...: OK!").
The overlay must not contain `usr/sbin` (Buildroot checks for a merged /usr: put helpers in `usr/bin`).
The song drive mounts through `/etc/fstab` (`LABEL=YARGSONGS`, `noauto`) started by udev (`71-yargos-songs.rules`), NEVER as an automount (every lookup with no drive waited 3 s: YARG's song cache check took hours) and NTFS needs `mount.ntfs` -> ntfs-3g
plus `CONFIG_FUSE_FS`; the kernel's ntfs3 refuses some Windows-made drives. To click inside YARG under QEMU,
build a static x11rb program that calls XTEST (`xtest_fake_input`) and fetch it into the guest with curl.

`screens.rs` `apply` must never switch off and re-enable an output that already shows the planned mode (an HDMI TV
loses sync and shows torn, corrupted pictures for seconds); it only disables what changes, and resizes the screen only
when the size differs.

## Building with and without YARG

The default build has NO YARG: `BR2_PACKAGE_YARG` is off, `post-build.sh` deletes any `usr/share/yarg-seed` left in the target,
and the device installs YARG itself (INSTALL YARG on the main menu, `yargos-update install`). That image is the only kind that
may ever be published. `YARGOS_WITH_YARG=1 tools/br.sh` switches the package on (and redoes its install step) for a PRIVATE
test image; a plain `tools/br.sh` switches it back off. Each full build also leaves `images/yargos-clean.img` or
`yargos-with-yarg.img`; `images/yargos.img` is just the latest. The busybox image has no `stat`, `nc`, `pgrep` or `python`:
write image scripts without them (`[ -O f ]`, `wc -c < f`).

## Talking to GitHub

GitHub serves release downloads through redirects: every `curl` to it needs `-L` (without it curl saves the empty redirect body
and exits 0; `-s -f` do not catch that), and a download must be checked for a non-empty file. Test servers must redirect too:
`tools/redirect-server.py` (a plain `python3 -m http.server` hid exactly this bug until the first real release).

## Long jobs on the dashboard

Anything that can outlive a reboot or a power cut (the update lock, staging folders) must be recoverable on the next run:
`yargos-update`'s lock stores pid and boot id and is cleared when its owner is gone. A long job needs a percentage and,
if it can be stopped safely, BACK to cancel (`Model.cancellable`, `Cmd::YargCancel`; scripts must trap TERM so their
cleanup runs). Animations use `model.tick`, which follows the clock in `session.rs`: do not count idle timeouts. The
system disk is a USB stick: big writes need the small dirty limits in `etc/sysctl.d/yargos.conf`. If the dashboard seems
stuck on hardware, boot with `systemd.setenv=YARGOS_DEBUG_FPS=1` and read the journal (`yargos-home:` lines).

## Size

`board/yargos/post-build.sh` removes the Clang/SPIR-V leftovers of Mesa's build. Do NOT compress firmware to `.xz` or
the kernel modules: compressed firmware cost a real laptop its Wi-Fi and sound (the cause was never confirmed,
so it needs a hardware test before it comes back), and the build's host `depmod` has no zlib/xz/zstd, so
compressed modules silently give an empty `modules.dep` and no driver loads. If `.xz` firmware ever sits in the
target the post-build step stops on purpose: delete them and run `tools/br.sh linux-firmware-dirclean`.
Changing `BR2_TARGET_ROOTFS_EXT2_SIZE` needs `tools/br.sh yargos_x86_64_defconfig`, then deleting
`images/rootfs.ext2`, `rootfs.ext4` and `yargos.img`, then `tools/br.sh`; keep it well above `du -sm output/target`.

## First boot on a drive bigger than the image

Every real stick is bigger than the image, so growdata always rewrites the partition table at boot; the data device
then briefly disappears and systemd silently drops any unit that `RequiresMountsFor=/data` (it never started: YARG
"not installed", exit code 127, Sound "Permission denied"). `yargos-firstboot` therefore has no mount requirement and
its script waits for `/data` itself; /data is `nofail` and not fsck'd. QEMU's default 2.6 GB disk hides all of this:
test with a big overlay (`qemu-img create -q -f qcow2 -b ~/yargos-build/output/images/yargos.img -F raw
~/yargos-build/qemu/overlay.qcow2 256G`, then `tools/qemu-run.sh` without `fresh`) and check that `/data/apps/yarg/current`
exists and `/data` belongs to `player`.

## Audio

`alsa-plugins` is installed only for libsamplerate (`samplerate_best` is the system default via
`etc/alsa/conf.d/50-yargos-rate.conf`); post-build.sh deletes its other plugins and its `10-samplerate.conf` because each
shows up as an extra output device in YARG's list (BASS lists every ALSA PCM). Never fall back to ALSA's built-in
converter: it is audibly harsh. Method for testing audio quality: tools/audio-test/README.

## Boot-disk drivers must be built in

There is no initramfs, so the drivers for whatever the boot disk sits on (SATA/AHCI, NVMe, eMMC/SD, Intel VMD) are
`=y`, never modules. Test with QEMU: `NVME_IMG=~/yargos-build/qemu/nvme.img HEADLESS=1 tools/qemu-run.sh fresh` (blank
NVMe disk, pick Install), then `BOOT_NVME=1 IMG=<that file> WORK=<other dir> HEADLESS=1 tools/qemu-run.sh fresh`
to boot the installed disk alone. `MAX_OUTPUTS=2` adds a second virtual screen but headless QEMU does not connect it.

## System updates (A/B slots)

Read docs/OTA-PLAN.md ("As built"). The rules that are easy to break: (1) a boot entry names its root partition by GUID and
/data by `systemd.mount-extra`; nothing in the rootfs knows its disk (no /data line in fstab, growdata reads the GUID
from /proc/cmdline); (2) all entry writing goes through `usr/lib/yargos/entries.sh` (image build, installer, updater);
(3) the updater deletes every entry that points at the other slot BEFORE writing it and writes the new entry LAST (a power
cut must leave only the running system's entry); (4) the efivar `LoaderBootCountPath` holds an EFI path with backslashes
in UTF-16; (5) `yargos-boot-ok` must require the launcher to stay up with no restarts (a crash-looping service is
"active" part of the time); (6) the ESP is mounted at `/efi`; (7) the image has no zstd/stat/nc: xz and openssl are
there. Every build stamps `/usr/lib/yargos/version` (`YARGOS_VERSION`, default the build time); a release image must be
built with its own version. Test builds (`YARGOS_TEST_CONSOLE=1`) get a serial console on the normal entry; real builds
must not. `tools/test-os-update.sh` rebuilds the image several times and leaves a test image in `images/`: run
`tools/br.sh` afterwards.

## Disk identity and the installer

Never find a root or data partition by label: the flashed image uses fixed partition GUIDs (`...0001` system A,
`...0003` system B, `...0002` data) and `yargos-install` gives every installed disk fresh random ones (it writes the
boot entries; the slots are told apart by their GPT names `yargos-a`/`yargos-b`). Labels would clash as soon as the stick and an installed disk are plugged in together.
`usr/bin/yargos-install` is plain sh (busybox ash; this image's jq has no regex), run as root through the
`install /dev/X` request of `yargos-home ctl`, which only accepts a plain disk path. Test it in QEMU with
`DISK2=~/yargos-build/qemu/target.img` (blank second disk) and pick "Install" in the boot menu; boot the result
with `IMG=<that file> WORK=<other dir> tools/qemu-run.sh fresh`. The quiet boot entries have no serial console:
use the verbose entry (third in the menu) when you need `tools/qemu-sh.sh`.

## Who owns what on /data (security)

The device has no root login and no ssh; the remaining risk is code running as `player` (YARG with a bad song or mod)
reaching root. So: root never sources, runs, or `chown -R`s anything the player can write, and never follows a path
through a player-writable folder. `/data` and its first-level folders (`apps`, `home`, `bluetooth`, `iwd`, `alsa`, `bluealsa`) are
root-owned (the player cannot swap them for links); only `apps/yarg`, `yarg`, `songs`, `home/player` and the file `yargos.conf`
belong to the player. `firstboot.sh` (root) chowns only those leaves themselves and replaces links with real folders;
the seeded YARG is installed by the player (`yargos-seed.service`); the daily `yargos-update.service` runs as `player`
and `yargos.conf` is read with `grep -x` for exact lines, never sourced (do the same in any new root code, e.g. the OS
updater). `asound.conf` is root-owned because `alsactl` (root) parses it and an ALSA config can load a program: the
Sound screen writes it through the helper (`sound CARD DEV`). Anything new that root reads from /data must be root-owned.
Test: as root plant links (`ln -s /etc /data/apps/yarg`), run `/usr/lib/yargos/firstboot.sh`, check /etc is untouched.

## The data partition must fill the drive

A flashed stick has a 700 MB data partition that `growdata.sh` grows at boot. A real stick once stayed at 700 MB (YARG filled it,
updates and imports failed with "No space left on device"; the cause is unknown, QEMU cannot reproduce it). So the dashboard
retries (`sys::data_slack_kib`, helper word `grow-data`) and the updater checks free space. Never assume /data is big: check
`df` before writing something large. `BOOT_USB=1 tools/qemu-run.sh` boots the image as a USB stick; add
`CMDLINE="systemd.mask=yargos-growdata.service ..."` to test the dashboard's retry.

## Bluetooth headphones (sound output)

BlueZ has its audio plugin and `bluez-alsa` (A2DP source only, SBC) supplies `pcm.bluealsa`; there is no PipeWire/PulseAudio.
Pair a headset in one `bluetoothctl` session WITH an agent (see `bt_connect_audio`): a one-shot `bluetoothctl pair` has none and
earbuds fail halfway. asound.conf only ever names a headset by a validated address (`ctl::valid_mac`: an ALSA config can load
programs). `/data/asound.prefer` is the chosen headset; `plan_sound` (before every YARG start) uses it only while connected, else
a real sound card via `sound-temp`: YARG must always find an existing default output. Never scan for Wiimotes while YARG runs.
The headset's ALSA slave is pinned to 44.1 kHz / 2 ch / S16 (`asound_text_bt`) and bluealsa runs with `--a2dp-force-audio-cd`:
a program that opens the output at another rate or channel count makes the bluealsa plugin call SelectCodec, BlueZ closes and
reopens the live stream, the earbuds did not answer ("No reply to Open request", exactly 25 s later "Couldn't set A2DP
configuration: Timeout"), YARG's audio start hung 25 s and the link dropped. Never let a program renegotiate a live A2DP stream.

## Wi-Fi and Bluetooth share a radio

On many laptops (the test HP Envy: Realtek 8822CE) Wi-Fi and Bluetooth are one chip. The dashboard's automatic Wiimote scan runs
almost all the time, and an OS update crawled at 0% (no error: the updater only gives up below 2 KB/s). So `sys::ScanPause`
(held by the OS update, the YARG update/install jobs) stops the scan while a download runs, and the update screen shows MB done
and KB/s (`PROGRESS percent MB-done MB-total KB/s`). Never add a download without pausing the scan.

## Volume

`sys::volume_*`: a card's mixer (knob = Master else PCM/Speaker/...; the other controls are opened once) or a headset through
`bluealsa-cli volume` (software volume; the model marks it `BT_VOLUME`). The session grabs the volume keys on the root window
(`x::grab_volume_keys`) so YARG does not get them; the level shows in the banner window. HDMI has no mixer on purpose.

## Internal songs (SYSTEM > Storage)

Read docs/DESIGN.md ("The internal song library"). Rules: songs live in `/data/songs/<drive name>/` on the data partition (no
formatting, no extra partition: the data partition already fills the disk); the copying, listing and deleting is plain Rust
as the player (`songcopy.rs`), root only mounts the portable drive (`yargos-songs`, helper words `songs-mount /dev/X` and
`songs-umount`), read-only with `nosuid,nodev,noexec`, and only a USB/SD drive that is not the system's own. A copy goes to
`<file>.yargos-part` and is renamed when whole, same-size files are skipped, so every import can be repeated to resume. The
free-space reserve (`sys::reserve_kib`) protects YARG's data and the system update download: never copy past it. Deleting
never follows links and refuses names with `/` or `..`. Anything new that writes under /data/songs does it as the player.
`SONGS_IMG="a.img b.img" tools/qemu-run.sh` attaches several fake USB drives (`mkfs.ext4 -L NAME -d folder img 64M` makes
one without root).

## Style

Match the surrounding code: terse comments that explain why, no TODO banners, small modules.
