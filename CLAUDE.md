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
- Do not redistribute the built image or YARG itself: the release bundles proprietary BASS and the
  Discord SDK. The repo only holds the package definition and checksum.
- `genimage.cfg` boot-entry changes require `fresh` for `tools/qemu-run.sh` (stale qcow2 overlay).

## The bridge (`src/bridge`)

Dependency-free Rust (`cargo test`, `cargo build --release`). One binary, modes `wii` and `drums`,
each turning an instrument YARG cannot read on Linux into a virtual PS3 instrument through
`/dev/uhid` (root only). Report layouts come from PlasticBand's docs
(https://github.com/TheNathannator/PlasticBand), the Xbox side from the kernel's `xpad.c`.
New instrument = new module + mode in `main.rs` + udev rule line + systemd unit.
Hardware-dependent facts (button bits, ranges) must be tested on the real device with
`tools/dev-test.sh`; unit tests only cover decoding against the documented layout.

## Style

Match the surrounding code: terse comments that explain why, no TODO banners, small modules.
