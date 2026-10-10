# Over-the-air OS updates: plan

Status: planned, nothing built yet. Planned by an Opus agent from the code as of 2026-10-10; the owner's decisions are
below. Verify the items marked *verify* before relying on them.

## Decisions made by the owner
- **No Secure Boot.** Not planned at all (it would need signed loader/kernel/modules, key enrollment, and a rework of the
  per-disk kernel command line). The firmware's Secure Boot stays off; the README says so.
- **Automatic updates must be switchable off** from the launcher (the YARG updater already has `AUTO_UPDATE`; the OS
  updater gets its own switch on the Updates screen, same pattern as the Songs screen switch).
- Still open (agent's default first): 8 GB minimum stick with 2 GiB slots; custom shell updater rather than
  systemd-sysupdate; 2 boot tries; background download with install at next restart; publish `legal-info` sources.

## Licensing (checked 2026-10-10, not legal advice)
- YARG's own code is LGPL-3.0-or-later (repo LICENSE, README). Redistributing that is allowed with LGPL duties.
- The release build also contains BASS (+ add-ons libbass_fx, bassopus...), the Discord Game SDK and Unity's runtime.
  YARG's README lists BASS as "Proprietary (free for non-commercial use)" and the Discord SDK as "Licenseless".
- un4seen.com/bass.html: the free licence is for "your product"; "Reselling/sublicensing is not permitted. Your
  products must be end-user products, eg. not components used by other products. ... these licences only cover your own
  software, not the publishing of other's software." So we may not publish YARG's build, and a public OS payload must not
  contain it. The device downloads YARG from YARC's own releases (`yargos-update`), which is fine. Unity's runtime and
  the Discord SDK were not checked in detail: treat as not redistributable.
- Private use (your own stick) is not publishing. Handing the image to other people is.

## Recommended design
- **A/B root slots + systemd-boot boot counting + a small custom updater** (`usr/bin/yargos-os-update`, style of
  `yargos-update`). systemd 258 here already ships `systemd-bless-boot`, `boot-complete.target`
  and the boot-check units (verified in the target). Rejected: in-place single root (power cut = brick), an app layer on
  /data (root-run code from a player-writable partition), RAUC/SWUpdate/Mender/ostree (heavy, bootloader/initramfs
  assumptions), systemd-sysupdate (gpg, poor progress/resume).
- **Layout** (needs one final re-flash and re-install): ESP 256 MiB, root A 2 GiB, root B 2 GiB (both root-type GUID,
  names yargos-a/b), data last (growdata unchanged). Flashed image ~5.5 GB (sticks >= 8 GB). Fixed GUIDs on the flashed
  image: A `...0001`, data `...0002`, B `...0003`.
- **Disk identity moves into the boot entries** (`root=PARTUUID=<slot> systemd.mount-extra=PARTUUID=<data>:/data:ext4:...
  panic=10`), so one published rootfs fits every disk and fstab has no /data line (*verify* `x-systemd.device-timeout`
  inside mount-extra, systemd >= 254).
- **Kernels on the ESP** per version (`/yargos/<ver>/bzImage`), copied from the freshly written slot's /boot so kernel,
  modules and firmware always match. Entries `yargos-<ver>+2.conf`; an entry at `+0` sorts last so the old one boots.
  *Verify* default/sort behaviour in QEMU first.
- **Payload**: signed `manifest.json` + `rootfs-<ver>.zchunks` (1536 MiB ext4 in 32 MiB chunks, each `zstd -19`, per-chunk
  hashes, all-zero chunks skipped); expected download ~300-400 MB (*measure* with `zstd -19 --long`). Files land
  uncompressed in the slot (the compressed-firmware/modules problem does not return). Hosted as GitHub release assets
  with a permanent `channel` release holding `stable.json`/`beta.json`; fetch with `curl -r` ranges (*verify* the asset CDN
  honours Range, else one asset per chunk). Version `YYYY.MM.N` in os-release.
- **YARG kept out of the published rootfs**: the seed goes to the *flashed image's data partition* (`mke2fs -d` under
  fakeroot); firstboot no longer copies a seed; if `/data/apps/yarg/current` is missing the launcher offers "Install YARG"
  (download from YARC). `release.sh` refuses to publish if the rootfs contains yarg-seed, libbass* or discord_game_sdk*.
  GPL: publish Buildroot `make legal-info` output with the payload.
- **Security**: minisign (ed25519) signs the manifest (public keys in the image, plus an offline emergency key); no
  downgrades; rejected-version list; never source files from /data as root.
- **Procedure**: check (only when the booted entry is marked good) -> delete the inactive slot's entry and kernel ->
  write chunks (resume, read-back verify) -> install kernel + entry with a try counter -> reboot when the user wants ->
  `yargos-boot-ok.service` (RequiredBy boot-complete.target: launcher up, modules.dep not empty, YARG verify-dir, device
  classes present again) -> `systemd-bless-boot` or bad + reboot; `panic=10` for panics.
- **Launcher**: Updates screen gets a system block (check, update with progress, restart now/later, automatic on/off,
  go back to the previous system), an ATTENTION box that waits for OK after a rollback, "UPDATE READY" on the home
  screen. Updates never restart on their own and slow down while YARG runs. Root helper gains fixed argument-less
  requests (`os-update`, `os-cancel`, `os-goback`, `os-ack`).
- **Release**: `tools/release.sh <ver> [--channel beta] [--local DIR]`; `--local` serves from this PC (set
  `OS_UPDATE_URL` in /data/yargos.conf), which alone ends the re-flashing.

## Problems found in today's tree (fix first, "Phase 0")
1. **Root runs a file the player can write**: `yargos-update.service` runs as root and `yargos-update` does
   `. /data/yargos.conf`, which (like /data) belongs to `player`. Read the settings with sed/grep instead; never source
   anything from /data as root.
2. Wi-Fi passwords live on the rootfs (`/var/lib/iwd`, no override in /etc/iwd/main.conf): a new slot would lose them.
   Move iwd's state to /data (bind mount, migrated once).
3. The data partition's GUID is hard-coded in the rootfs fstab (the installer seds it): move it to the boot entry.
4. No `panic=` / watchdog: a kernel panic hangs instead of rebooting into the fallback.

## Phases
- Phase 0 (~2 days, useful alone): items 1-4 above, seed into data.ext4, os-release version, kernel in /boot.
- Phase 1 (~4-5 days, one re-flash): new genimage layout and installer, `yargos-os-update` with signing, boot-ok
  service, tests (`tools/test-os-update.sh`, QEMU with a 64G overlay, forced bad releases, `system_reset` mid-update).
- Phase 2 (~3 days): launcher screens, ctl requests, timer (`OnBootSec=5min`, daily), GitHub publishing, channels.
- Phase 3 (optional): read-only root, chunk deltas, watchdog drivers. (Secure Boot: not planned.)
