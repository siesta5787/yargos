# Licensing

Not legal advice: this is what the project does and why. If you redistribute images, check it yourself.

## This repository

The files in this repository (scripts, the Rust programs `yargos-home` and `yargos-bridge`, configs, documentation) are
under the MIT licence, see `LICENSE`. The Rust programs use only MIT / Apache-2.0 / Zlib crates (x11rb, fontdue and what
they pull in; see `src/home/Cargo.lock`). The fonts the dashboard draws with (DejaVu, free licence) come from the system
image, not from this repository, and the logo is drawn in code.

## What a system image contains

A built image is a Buildroot system. The MIT licence of this repository does not cover the rest of it:

| Part | Licence |
| --- | --- |
| Linux kernel | GPL-2.0 |
| systemd, glibc, iwd and most libraries (ALSA, udev, ...) | LGPL-2.1+ (some GPL) |
| busybox, BlueZ, util-linux, e2fsprogs, ntfs-3g, bash, ... | GPL-2.0 / GPL-3.0 |
| Mesa, X.Org, libinput, curl, OpenSSL, jq, ... | MIT / BSD / Apache-2.0 |
| `linux-firmware` and `intel-microcode` (binary blobs) | redistributable under the licence texts shipped with them |
| `yargos-home`, `yargos-bridge`, scripts | MIT |

**Each system release is published with a legal bundle** (`legal-<version>.tar`, a second asset of the GitHub release): the
licence text of every package, a `manifest.csv`, the source code of the GPL / LGPL packages, the licence files of the
compiled-in Rust crates, and the exact source tree of this project (`yargos-source.tar.gz`). `tools/release.sh` makes it
and `tools/publish.sh` refuses to publish without it. Inside the system, `/usr/share/doc/yargos/NOTICE` says where to find
it. Anyone can also rebuild the image from this repository with Buildroot (see the README).

## YARG itself is not part of the system

YARG's own code is LGPL-3.0, but its release build also contains BASS (proprietary; its licence allows free
non-commercial use by the developer of a product, and says it "only covers your own software, not the publishing of
other's software"), the Discord Game SDK and Unity's runtime. So:

- the default image and every published system release contain no YARG; the device downloads YARG from YARC's own GitHub
  release when you choose INSTALL YARG (or installs a zip you put on your song drive);
- `package/yarg` only holds the download address and checksum; `YARGOS_WITH_YARG=1 tools/br.sh` builds a private test
  image with YARG preloaded, which must never be published;
- `tools/release.sh` refuses an image that contains YARG, BASS, the Discord SDK or the Unity player.

## Names, trademarks, affiliation

YARG OS is an unofficial community project. It is not made, endorsed or supported by YARC or the YARG team; "YARG" is used
to say what the system is for. PlayStation, Wii, Xbox, Guitar Hero and Rock Band belong to their owners. The bridges make a
Wii guitar or an Xbox 360 drum kit look to YARG like a standard PlayStation instrument (USB ids and device names), because
that is the form YARG reads; the report layouts come from the [PlasticBand](https://github.com/TheNathannator/PlasticBand)
documentation (credit to its authors).

## Forks and your own releases

Devices trust the public keys in `board/yargos/rootfs-overlay/usr/lib/yargos/keys/` and look for updates at the GitHub
release `os-channel` of this repository (`DEFAULT_URL` in `usr/bin/yargos-os-update`). If you fork and publish your own
systems, replace the keys with your own and change that address, otherwise your devices would trust (and ask) someone else.
The private signing keys are never in the repository (`~/.yargos/` on the maintainer's machine).
