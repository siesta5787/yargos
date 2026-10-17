# Over-the-air OS updates: plan

Status: Phases 0, 1 and 2 (dashboard screens, timer) are built and tested in QEMU (2026-10-11); publishing a first release
and trying it on hardware are next. Planned by an
Opus agent from the code as of 2026-10-10; the owner's decisions are below. "As built" notes in the text say where the
implementation differs from the first plan.

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
- **YARG kept out of the published rootfs** (DONE 2026-10-10: the default build has no YARG and the device installs it from
  the dashboard, from GitHub or from a zip on the song drive; the older plan below, seeding the data partition, is not used): the seed goes to the *flashed image's data partition* (`mke2fs -d` under
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
1. **Root runs a file the player can write** (DONE 2026-10-10): `yargos-update` sourced `/data/yargos.conf`. It now
   reads the settings with grep and the daily timer runs it as `player`. Same class, also fixed: `firstboot.sh` did
   `chown -R` on player-owned trees (a planted link hands over any file) and `alsactl` (root) parsed the
   player-writable `asound.conf`, which can load a program. Rule now: /data and its first level folders belong to
   root; only the leaves the player's programs write (apps/yarg, yarg, home/player) and `yargos.conf` belong to the
   player; root never recurses into or follows paths in those; the seed is installed by the player
   (`yargos-seed.service`); `asound.conf` is root-owned and written through the helper (`sound CARD DEV`).
2. Wi-Fi passwords lived on the rootfs (`/var/lib/iwd`) (DONE 2026-10-10: iwd, ALSA state and Bluetooth are bind
   mounted from /data/iwd, /data/alsa, /data/bluetooth, carried over once).
3. The data partition's GUID is hard-coded in the rootfs fstab (the installer seds it): move it to the boot entry.
4. No `panic=` / watchdog: a kernel panic hangs instead of rebooting into the fallback.

## Phases
- Phase 0 (~2 days, useful alone): items 1-4 above, seed into data.ext4, os-release version, kernel in /boot.
- Phase 1 (~4-5 days, one re-flash): new genimage layout and installer, `yargos-os-update` with signing, boot-ok
  service, tests (`tools/test-os-update.sh`, QEMU with a 64G overlay, forced bad releases, `system_reset` mid-update).
- Phase 2 (~3 days): launcher screens, ctl requests, timer (`OnBootSec=5min`, daily), GitHub publishing, channels.
- Phase 3 (optional): read-only root, chunk deltas, watchdog drivers. (Secure Boot: not planned.)

## As built (Phase 1)
- **Layout**: ESP 256 MiB | `yargos-a` 2 GiB (the shipped system, GUID `...0001`) | `yargos-b` 2 GiB (empty, `...0003`) | data
  (`...0002`). The GPT partition NAMES (`yargos-a`/`yargos-b`) tell the updater which partition is the other slot, so
  installed disks (random GUIDs) work the same. Flashed image 5.3 GB. `yargos-install` makes the same layout.
- **Disk identity is in the boot entry**: `root=PARTUUID=<slot> systemd.mount-extra=PARTUUID=<data>:/data:ext4:...
  panic=10`; fstab has no /data line, one rootfs fits every disk. `usr/lib/yargos/entries.sh` writes entries for the
  image build, the installer and the updater. Verified: `mount-extra` with `x-systemd.device-timeout`, growdata reading the
  GUID from the command line.
- **Kernel** is inside each rootfs (`/boot/bzImage`, `BR2_LINUX_KERNEL_INSTALL_TARGET`) and copied to
  `/efi/yargos/<version>/bzImage` by whoever writes the entry, so kernel and modules always match. The ESP is mounted at
  `/efi` by systemd (`/boot` only when there is no `/efi`). Entries: `yargos-<version>[+tries].conf` (the default
  `yargos-2*`: newest wins, an entry with 0 tries left is skipped), `debug.conf` and (on a stick) `install.conf` follow
  the system that booted last (`yargos-os-update settle`).
- **Verified boot-counting behaviour (systemd-boot 258.7 in QEMU)**: a `+2` entry is tried twice (`+1-1`, `+0-2`), then
  sorted last so the older one boots; a boot that reaches `boot-complete.target` with no failed unit is blessed by
  `systemd-bless-boot` (the counter is removed); `boot-complete.target` only starts on a trial boot.
- **Payload** (simpler than the zchunks idea): one `rootfs-<version>.img.xz` (xz of the raw 1.5 GB ext4), a signed
  `<channel>.json` manifest with the sizes and SHA-256 of both the xz and the raw image. ~230 MB at `xz -1`. `curl -C -`
  resumes, a server without ranges makes it start over. Written with `xz -dc | dd` straight onto the other slot, read back
  and checked against the raw SHA-256, `e2fsck -fn`, then mounted read-only to check the version inside equals the
  announced one and to take the kernel. No `zstd` on the device (xz is already there); `openssl` (Ed25519, `pkeyutl
  -rawin`) verifies the signature. Keys: `usr/lib/yargos/keys/*.pem` (release + an emergency key whose private half is
  kept offline); private keys live in `~/.yargos/` (outside the repo).
- **Order of operations** (power-cut safe): delete every entry (and kernel dir) that points at the other slot, then write
  and verify it, then copy the kernel, then write the `+2` entry LAST. Until that last step only the running system has an
  entry. The new system's `yargos-boot-ok.service` (RequiredBy `boot-complete.target`) needs the launcher up AND with no
  restarts over 8 s (a crash-looping service is "active" part of the time; an earlier check missed that and blessed a
  system with no launcher), and restarts at once on failure when on trial; after two tries the loader skips the entry.
  `yargos-os-settle.service` runs every boot after blessing: it records `updated=` or, if the system that ran is not the
  one that was staged, `rolled_back=` plus the version on `rejected` (never offered again).
- **go-back**: renames the running system's entry to `+0`, so the other system becomes the default at the next restart.
- **Tests**: `tools/test-os-update.sh` (QEMU, ~30 min): good update, bad update that falls back, power cut mid-write,
  going back. `tools/release.sh` makes and signs a release and refuses an image that holds YARG, BASS, Discord or Unity.
- Dashboard: SYSTEM > Updates > System updates (check, install with a percentage, BACK cancels, "Restart now?", automatic on/off,
  go back); a rolled-back update raises an ATTENTION box that is acknowledged once; the main menu card says when a system
  update is waiting. The root helper only accepts the fixed words `os-check`, `os-update`, `os-update-offline`, `os-find`, `os-cancel`, `os-goback`, `os-ack`.
  `yargos-os-update.timer` checks 5 minutes after boot and daily (never while YARG runs, never restarts).
- **Failures and offline** (added after the first real-hardware update failed with just "Download failed"): curl's exit
  status and HTTP code are put into words (`curl_why`: DNS, refused, HTTP error, disk full/read-only, timeout/stall,
  TLS/clock, cut off) and shown on the dashboard; the download retries up to 6 times keeping the partial file (a
  connection moving less than 2 KB/s for 45 s counts as stalled), and checks free space first. `yargos-os-update find` and
  `install-offline` (helper words `os-find`, `os-update-offline`) read `<channel>.json`, `.json.sig` and the image the
  manifest names from the song drive (top level or one folder down): the manifest must carry our signature (it is
  size-limited because /run is memory), then the same image checks run; the image is copied into /data/os-update first, so
  nothing on the drive is ever deleted. `tools/publish.sh` also attaches the signed manifest to each version's release
  page so the three files come from one page.
- Not built (yet): chunked/delta payloads, GitHub publishing is manual (`gh` commands in
  `tools/release.sh`), `legal-info` publishing.
