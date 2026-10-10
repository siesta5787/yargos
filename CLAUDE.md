# YARGOS

Buildroot external tree that builds a console-style YARG appliance image (x86_64). Read `README.md`
first: it has the layout, runtime design and the verified / not verified lists. Keep that list honest;
do not move something to "Verified" unless it was actually run.

## Rules that are easy to get wrong

- Build with `tools/br.sh`, never `make` directly in the output folder. The build lives in
  `~/yargos-build/` but ~480 host-tool files embed `/tmp/brout` and `/tmp/buildroot`; `br.sh` recreates
  those symlinks (`/tmp` is wiped at boot). Do not move or delete `~/yargos-build` (a rebuild is ~10 h).
- Buildroot is 2026.02.3. Check symbol names in its `Config.in` files before using them: it has no
  `LINUX_FIRMWARE_ALL`, no Mesa RADV option, and no `host-unzip`. After editing the defconfig, re-run
  `make yargos_x86_64_defconfig` and confirm each symbol is set in `.config`.
- Config changes do not rebuild packages automatically: use `tools/br.sh <pkg>-rebuild` or
  `-reconfigure`. Buildroot does not rebuild dependents either.
- Do not publish the built image or YARG's release build. YARG's own code is LGPL-3.0, but its release also contains
  BASS (whose licence covers only the developer's own software, "not the publishing of other's software"), the Discord
  SDK and Unity's runtime. The repo only holds the package definition and checksum; devices download YARG from YARC.
  Details and sources: docs/OTA-PLAN.md. Secure Boot is not planned.
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
The song drive mounts through `/etc/fstab` (`LABEL=YARGSONGS`, automount) and NTFS needs `mount.ntfs` -> ntfs-3g
plus `CONFIG_FUSE_FS`; the kernel's ntfs3 refuses some Windows-made drives. To click inside YARG under QEMU,
build a static x11rb program that calls XTEST (`xtest_fake_input`) and fetch it into the guest with curl.

`screens.rs` `apply` must never switch off and re-enable an output that already shows the planned mode (an HDMI TV
loses sync and shows torn, corrupted pictures for seconds); it only disables what changes, and resizes the screen only
when the size differs.

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

## Disk identity and the installer

Never find a root or data partition by label: the flashed image uses fixed partition GUIDs (`...0001` system,
`...0002` data) and `yargos-install` gives every installed disk fresh random ones (it rewrites the boot entry
and `/etc/fstab`). Labels would clash as soon as the stick and an installed disk are plugged in together.
`usr/bin/yargos-install` is plain sh (busybox ash; this image's jq has no regex), run as root through the
`install /dev/X` request of `yargos-home ctl`, which only accepts a plain disk path. Test it in QEMU with
`DISK2=~/yargos-build/qemu/target.img` (blank second disk) and pick "Install" in the boot menu; boot the result
with `IMG=<that file> WORK=<other dir> tools/qemu-run.sh fresh`. The quiet boot entries have no serial console:
use the verbose entry (third in the menu) when you need `tools/qemu-sh.sh`.

## Who owns what on /data (security)

The device has no root login and no ssh; the remaining risk is code running as `player` (YARG with a bad song or mod)
reaching root. So: root never sources, runs, or `chown -R`s anything the player can write, and never follows a path
through a player-writable folder. `/data` and its first-level folders (`apps`, `home`, `bluetooth`, `iwd`, `alsa`) are
root-owned (the player cannot swap them for links); only `apps/yarg`, `yarg`, `home/player` and the file `yargos.conf`
belong to the player. `firstboot.sh` (root) chowns only those leaves themselves and replaces links with real folders;
the seeded YARG is installed by the player (`yargos-seed.service`); the daily `yargos-update.service` runs as `player`
and `yargos.conf` is read with `grep -x` for exact lines, never sourced (do the same in any new root code, e.g. the OS
updater). `asound.conf` is root-owned because `alsactl` (root) parses it and an ALSA config can load a program: the
Sound screen writes it through the helper (`sound CARD DEV`). Anything new that root reads from /data must be root-owned.
Test: as root plant links (`ln -s /etc /data/apps/yarg`), run `/usr/lib/yargos/firstboot.sh`, check /etc is untouched.

## Style

Match the surrounding code: terse comments that explain why, no TODO banners, small modules.
