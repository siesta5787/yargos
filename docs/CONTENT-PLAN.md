# Plan: browse and install YARC Launcher content (songs, venues) from the dashboard

Status: PLAN ONLY, nothing is built. Written from a research pass of the YARC Launcher, YARG and YARC's repositories (the
sources named below). Checked again by hand: the live catalog `https://releases.yarg.in/profiles/` (302 to `index.json`,
`curl -L`) lists 54 items: 2 `application`, 41 `setlist`, 11 `venue`; setlist files have `fileType: "encrypted"` (7z), a
venue's file is a plain `7z`, all on `github.com/YARC-Official/...` releases. Everything else is reported, not run, and marked
UNVERIFIED where it was not confirmed.

## Verdict

Feasible and small in code: the launcher's catalog is one public JSON index plus one JSON file per item, downloads are 7z
files on YARC's GitHub releases (GitHub publishes a SHA-256 per file), and YARG already reads launcher content when started
with `-download-location <dir>`. About 7.4 GB in total today, the Official Setlist alone 2.45 GB.

**The biggest risk is permission, not code.** Setlists are password-protected 7z files; the launcher builds the password in
code its authors label "Idiot prevention"; the setlist README says the songs "must be used exclusively for YARG". Copying
the unlock step without YARC's agreement is the one thing not to do. Phase 0 is asking YARC.

## What the launcher does (from its sources)

- Tauri app (TypeScript + Rust), "YARG License 1.0" (MIT-like, no right to the YARG name or logos). The index is
  `${VITE_RELEASES_SERVER_URL}/profiles/` = `https://releases.yarg.in/profiles/` (`src/hooks/useMarketIndex.ts`);
  format `{banner, lastUpdated, profiles:[{uuid, type, category, url, release, name, iconUrl, ...}]}` (`src/profiles/marketplace.ts`).
- Each item is at `releases.yarg.in/profiles/<uuid>/`: `metadata` (name, description, credits; setlists have
  `songs:[{title, artist, length, releaseDate}]`, venues have `venueAuthor`, `venueType`, JPEG screenshots) and
  `version.content[].files[]{url, sigUrl?, fileType: zip|7z|encrypted}` with a `tag`. NO genre, instrument or difficulty in
  the song metadata: filters on those are impossible from this source.
- Download (`src-tauri/src/utils.rs`): HTTP Range resume, User-Agent `YARC-Launcher (contact@yarg.in)`, encrypted files with
  `sevenz-rust` (aes256). No setlist or venue has a `sigUrl`: the launcher verifies nothing on them. GitHub's release
  metadata gives `size` and `digest: sha256:...` for each file (one was hashed and matched).
- Install layout: `<download location>/Setlists/<uuid>/` and `/Venues/<uuid>/`, each with `installation/` and `tag.txt`;
  "installed" = `tag.txt` equals the profile's tag; an update wipes `installation/` and downloads everything again.
- YARG: `-download-location` sets `SetlistPath` and `VenuePath`; `SongContainer` scans `SetlistPath`; `VenueLoader` collects
  `*.yarground` recursively from `VenuePath` and picks one at random per song (no picker). Venues are Unity AssetBundles.
  New songs are only noticed when the song cache is missing/invalid or the player runs Refresh (our USB imports likely
  have the same gap: not checked on a device).
- Side find: the launcher contains YARG's minisign public key and every YARG zip has a `.sig`: `yargos-update` could
  verify YARG's signature (it says the key is unknown today).

## Legal and permission

- Setlist README: musicians gave permission for YARG specifically; "must be used exclusively for YARG". YARC "does not
  endorse mods or external programs" (`legal-articles/terms.md`). Venues: no stated licence (treat like the setlist).
  No published rules or rate limits for `releases.yarg.in` (UNVERIFIED). GitHub's unauthenticated API: about 60 requests/hour.
- YARG OS must never redistribute, mirror or cache this content in an image, release or repo; must not offer to export
  songs out of YARG's folders; must not use the launcher's User-Agent; must not ship the unlock step without written
  agreement. The device downloads from the original servers itself (like INSTALL YARG), the screens say "from YARC" and
  credit the charters.
- Ask: GitHub issue/discussion on YARC-Launcher, the YARG Discord, `yargofficialgame@gmail.com` (licensing.md) or
  `contact@yarg.in`. Draft: "I maintain YARG OS, an unofficial open-source Linux appliance that boots to a controller menu
  and runs your official YARG build downloaded from your GitHub. We'd like the device to list releases.yarg.in/profiles and
  install your setlists and venues exactly as the launcher does (from your GitHub releases, into Setlists/Venues passed to
  YARG with -download-location, used only by YARG, never redistributed). Is that OK, including unpacking the encrypted
  setlist archives the launcher's way, and do you want anything from us (User-Agent, request limits, attribution, a
  supported API or key)?"

## Design

- Menu: a main-menu row GET SONGS, second (PLAY YARG, GET SONGS, SOUND, BLUETOOTH, CONNECTED DEVICES, SYSTEM). `HOME_ITEMS`
  and the key sequences in `tools/test-songs.sh` and other key-driven tests change.
- Fetching like `yargos-update`: a player-run sh script `usr/bin/yargos-content` (curl + jq) with `refresh` (index + the 52 content
  profiles into `/data/yarc/.catalog/`, about 1 MB, at most daily), `list` (TSV), `info UUID`, `install UUID` (STEP and
  `PROGRESS pct MB MB KB/s` lines), `remove UUID`. The Rust side parses the TSV: no JSON/HTTP crate.
- Unpacking: `yargos-home unpack` in Rust with the vendored `sevenz-rust` crate (licence to confirm), writing only regular
  files and folders (a link can never be created). Alternative: Buildroot `p7zip` (`7za`; `7zr` cannot do encrypted), older C++
  parsing untrusted files.
- Layout mirrors the launcher: `/data/yarc/{Setlists,Venues}/<uuid>/{tag.txt,installation/}`; `firstboot.sh` creates
  `/data/yarc` as a player-owned leaf; `run-yarg` adds `-download-location /data/yarc`. Outside `/data/songs`, so `song-sources`
  and Storage do not take it for an imported drive.
- Install job: one lock (pid + boot id); `sys::ScanPause` held; the digest from `api.github.com/repos/YARC-Official/<repo>/releases/tags/<tag>`
  (only when installing); files one at a time with `curl -L -C - --speed-limit`, `.part` kept for resume, errors in words
  (`curl_why`); hash, unpack to `.stage-<uuid>`, swap, `tag.txt` last; BACK cancels (TERM trapped, stage removed). Space: download
  + unpacked size must fit and leave `reserve_kib` free, else refuse with the numbers. Afterwards touch
  `/data/yarg/.yargos-rescan`; `run-yarg` deletes `songcache.bin` so the next start scans everything (imports too).
- Browsing: LEFT/RIGHT between tabs Official / YARN / Venues / Installed; rows show name, song count, size and INSTALLED /
  UPDATE / QUEUED / %; OK opens details (description, credits, scrolling song list, Install / Update / Remove). 54 rows need
  no search in v1. No artwork in v1 (icons are launcher-bundled, screenshots are JPEG). Offline: the cached list with its date,
  installing greyed out.
- Security: everything runs as player. URLs only under `https://github.com/YARC-Official/` and `https://releases.yarg.in/`; UUID
  `[0-9a-f-]{36}`; the unpacker refuses absolute paths, `..`, links, device files, more than 20 000 entries or more than the free
  space. Venues are code-like content (AssetBundles; normally data and shaders, UNVERIFIED for YARG): the risk is loader or GPU
  driver bugs at the same trust level as YARG inside the player account; mitigations: official repo only, digest check, one-press Remove.

## Phases

- **Phase 0 (small):** ask YARC; spike on the image: `curl -fsSL https://releases.yarg.in/profiles/ | jq -r '.profiles[]|[.type,.category,.name]|@tsv'`;
  on a private test image put one venue under `/data/yarc/Venues/x/installation/`, start YARG with `-download-location`, confirm
  it appears. Accept: YARC answered in writing, 54 rows on the device, the venue shows in YARG.
- **Phase 1 (medium-large):** `yargos-content`, `firstboot.sh`, `run-yarg`, `model.rs`, `ui.rs`, `sys.rs`, `session.rs`, a new
  `content.rs` and the unpacker, crate vendoring and licence files; list, details, install/remove one item with progress, cancel,
  resume, rescan. Accept: `tools/test-content.sh` passes; on hardware a YARN pack and a venue install over Wi-Fi and play.
- **Phase 2 (medium):** search with the on-screen keyboard (titles and artists), a queue with background downloads and a line
  on the home card, update markers and a daily notify-only check; verify YARG's `.sig` in `yargos-update`.
- **Phase 3 (small each):** artwork (vendored decoder), "Remove all content" under Storage, community sources only with their
  owners' agreement (Chorus Encore has a search API, `POST https://api.enchor.us/search`, terms UNVERIFIED; charts of
  commercial songs and YARC does not endorse external charts: out of scope for now).

## Testing

Unit tests: TSV and profile parsers on saved real JSON, the free-space sum, URL and UUID rules, the unpacker against path
traversal, links, size bombs, a wrong password, a cut file. A fake server (extend `tools/redirect-server.py`: fake index, profiles,
test 7z files, a fake GitHub release-by-tag answer, all behind redirects; overrides `YARGOS_CONTENT_URL`, `YARGOS_GH_API`).
QEMU `tools/test-content.sh`, key-driven like `test-songs.sh`: install, layout and owner, `tag.txt`, rescan marker, `run-yarg`
arguments (with a `YARGOS_YARG_CMD` stand-in), cancel then resume, disk-full refusal on a small overlay, offline view, bad
digest, update, remove. Only on real hardware: the real 2.45 GB download with Wi-Fi and Bluetooth sharing a radio, YARG's full
rescan time, venues on the real GPU, unpack speed on the stick.

## Open questions (recommended default first)

1. Wait for YARC's yes before building any setlist install? Yes. Venues too.
2. Where does the entry go? Second row of the main menu.
3. How to unpack 7z? A Rust crate, not p7zip.
4. What is offered? Official and YARN setlists and venues; YARG builds stay with INSTALL YARG.
5. Downloads block the screen in v1? Yes (percentage, BACK cancels); background downloads in Phase 2.
6. Force a full rescan by deleting the song cache after a change (imports too)? Yes.
7. Not enough space? Refuse with the numbers in words, never touch the reserve.
8. Update content by itself? No: notify only.
