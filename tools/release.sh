#!/bin/bash
# Make a signed YARG OS release from the current build, ready to publish (or to serve from this PC for testing).
#
#   YARGOS_VERSION=2026.11.1 tools/br.sh              build the system with its version baked in (all of it: the image
#                                                     keeps no other record of its version)
#   tools/release.sh 2026.11.1 [options]
#       --channel stable|beta   which channel file to write (default stable)
#       --out DIR               where to put the files (default ~/yargos-build/release/<version>)
#       --base URL              where the image will be downloaded from (default: next to the channel file)
#       --notes "text"          a line the device shows with the update
#       --key FILE              Ed25519 private key (default ~/.yargos/release-ed25519.pem)
#       --github                put the image's address in the manifest as the GitHub release os-<version> of this repo
#       --fast                  quick, larger compression (for tests)
#       --no-legal              skip the licence/source bundle (tests only: a published release must have it)
#
# Writes DIR/<channel>.json, DIR/<channel>.json.sig, DIR/rootfs-<version>.img.xz (what devices download to update), DIR/yargos-<version>.img.xz
# (the whole disk image people flash: tools/flash.sh takes it as it is) and DIR/legal-<version>.tar. Test it without GitHub:
#   python3 -m http.server -d DIR 8000      and on the device put  OS_UPDATE_URL=http://<this PC>:8000  in /data/yargos.conf
# Publish (the owner does this by hand): the image is a release asset of tag os-<version>; the .json and .json.sig go
# into the standing release "os-channel" (replace the old ones):
#   gh release create os-<version> DIR/rootfs-<version>.img.xz --title "YARG OS <version>" --notes "..."
#   gh release upload os-channel DIR/<channel>.json DIR/<channel>.json.sig --clobber
# and pass --base https://github.com/<owner>/yargos/releases/download/os-<version> when making the files.
set -eu
HERE=$(cd "$(dirname "$0")/.." && pwd)
BUILD=${YARGOS_BUILD_DIR:-$HOME/yargos-build}
IMG=${YARGOS_IMAGES:-$BUILD/output/images}
VERSION=${1:-}
[ -n "$VERSION" ] || { sed -n '2,22p' "$0"; exit 2; }
shift
CHANNEL=stable; OUT=; BASE=; NOTES=; KEY=$HOME/.yargos/release-ed25519.pem; XZ=-6; LEGAL=1
while [ $# -gt 0 ]; do
    case $1 in
        --channel) CHANNEL=$2; shift 2 ;;
        --out) OUT=$2; shift 2 ;;
        --base) BASE=$2; shift 2 ;;
        --notes) NOTES=$2; shift 2 ;;
        --key) KEY=$2; shift 2 ;;
        --fast) XZ=-1; shift ;;
        --no-legal) LEGAL=0; shift ;;
        --github)
            repo=$(git -C "$HERE" remote get-url origin | sed -e 's|^git@github.com:||' -e 's|^https://github.com/||' -e 's|\.git$||')
            BASE=https://github.com/$repo/releases/download/os-$VERSION; shift ;;
        *) echo "unknown option $1"; exit 2 ;;
    esac
done
case $VERSION in *[!0-9.]* | "" | .*) echo "version must be dotted numbers, e.g. 2026.11.1"; exit 2 ;; esac
case $CHANNEL in stable | beta) ;; *) echo "channel is stable or beta"; exit 2 ;; esac
[ -f "$KEY" ] || { echo "signing key $KEY not found"; exit 1; }
[ -f "$IMG/rootfs.ext4" ] || { echo "no $IMG/rootfs.ext4: build first"; exit 1; }
OUT=${OUT:-$BUILD/release/$VERSION}
if [ "$LEGAL" = 1 ] && git -C "$HERE" status --porcelain | grep -q .; then
    echo "REFUSING: the git tree has uncommitted changes, so the source in the legal bundle would not match the binaries. Commit first (or --no-legal for a test)."
    exit 1
fi

# The image must be the one built for this version, and must not hold anything that may not be published.
built=$(debugfs -R "cat /usr/lib/yargos/version" "$IMG/rootfs.ext4" 2>/dev/null | tr -d '\n')
[ "$built" = "$VERSION" ] || { echo "rootfs.ext4 is version '$built', not $VERSION: build with YARGOS_VERSION=$VERSION tools/br.sh"; exit 1; }
if tar tf "$IMG/rootfs.tar" | grep -E -i 'yarg-seed|libbass|discord_game_sdk|UnityPlayer' | head -3 | grep .; then
    echo "REFUSING: the image contains YARG or its proprietary parts (see above). Build without YARGOS_WITH_YARG."
    exit 1
fi
# The version must also be newer than every earlier release this tree knows of, or devices will ignore it.
echo "version $VERSION, channel $CHANNEL, output $OUT"

mkdir -p "$OUT"
FILE=rootfs-$VERSION.img.xz
echo "compressing (xz $XZ)..."
xz -T0 "$XZ" -c "$IMG/rootfs.ext4" > "$OUT/$FILE.tmp"
mv "$OUT/$FILE.tmp" "$OUT/$FILE"
RAW_SIZE=$(stat -L -c %s "$IMG/rootfs.ext4")
RAW_SHA=$(sha256sum "$IMG/rootfs.ext4" | cut -d' ' -f1)
SIZE=$(stat -c %s "$OUT/$FILE")
SHA=$(sha256sum "$OUT/$FILE" | cut -d' ' -f1)
jq -n --arg v "$VERSION" --arg notes "$NOTES" --arg base "$BASE" --arg f "$FILE" \
    --argjson size "$SIZE" --arg sha "$SHA" --argjson rsize "$RAW_SIZE" --arg rsha "$RAW_SHA" \
    '{version: $v, notes: $notes, image: {file: $f, size: $size, sha256: $sha, raw_size: $rsize, raw_sha256: $rsha}}
     + (if $base == "" then {} else {base: $base} end)' > "$OUT/$CHANNEL.json"
openssl pkeyutl -sign -inkey "$KEY" -rawin -in "$OUT/$CHANNEL.json" -out "$OUT/$CHANNEL.json.sig"
# Check it the way a device will.
for pub in "$HERE"/board/yargos/rootfs-overlay/usr/lib/yargos/keys/*.pem; do
    openssl pkeyutl -verify -pubin -inkey "$pub" -rawin -in "$OUT/$CHANNEL.json" -sigfile "$OUT/$CHANNEL.json.sig" >/dev/null 2>&1 && { ok=1; break; }
done
[ "${ok:-0}" = 1 ] || { echo "the signature does not verify against any key in the image: wrong key?"; exit 1; }

# The whole disk image (for people who flash a stick). Only if its first system partition (at 257 MiB, after the 256 MiB
# boot partition) is byte for byte the image being released, i.e. it comes from the same build.
if [ -f "$IMG/yargos.img" ] && cmp -s -n "$RAW_SIZE" -i 0:$((257 * 1024 * 1024)) "$IMG/rootfs.ext4" "$IMG/yargos.img"; then
    echo "compressing the disk image (xz $XZ)..."
    xz -T0 "$XZ" -c "$IMG/yargos.img" > "$OUT/yargos-$VERSION.img.xz.tmp" && mv "$OUT/yargos-$VERSION.img.xz.tmp" "$OUT/yargos-$VERSION.img.xz"
else
    echo "NOTE: no disk image made ($IMG/yargos.img is missing or not from this build); the release can only update existing devices"
fi

# Licences and sources: everything in the image is redistributed with the release, so the licence texts of all of it and
# the source of the GPL / LGPL parts go with it (plus the exact source tree of this project, and the licence files of
# the Rust crates compiled in).
if [ "$LEGAL" = 1 ]; then
    echo "collecting licences and sources (make legal-info)..."
    (cd "$HERE" && tools/br.sh legal-info >/dev/null 2>&1) || { echo "make legal-info failed"; exit 1; }
    LI=$BUILD/output/legal-info
    L=$(mktemp -d)/legal-$VERSION
    mkdir -p "$L/rust-crates" "$L/sources"
    cp -a "$LI"/manifest.csv "$LI"/host-manifest.csv "$LI"/buildroot.config "$LI"/README "$LI"/licenses "$LI"/host-licenses "$L/"
    for d in "$BUILD"/output/build/yargos-home-*/VENDOR/*/; do
        [ -d "$d" ] || continue
        mkdir -p "$L/rust-crates/$(basename "$d")"
        cp "$d"/LICENSE* "$d"/COPYING* "$d"/COPYRIGHT* "$d"/NOTICE* "$L/rust-crates/$(basename "$d")/" 2>/dev/null || true
    done
    python3 - "$LI" "$L/sources" <<'PY'
import csv, os, shutil, sys
li, out = sys.argv[1:3]
seen = set()
for r in list(csv.reader(open(os.path.join(li, "manifest.csv"))))[1:]:
    pkg, ver, lic = r[0], r[1], r[2]
    d = os.path.join(li, "sources", f"{pkg}-{ver}")
    if "GPL" not in lic or not os.path.isdir(d):
        continue
    for f in os.listdir(d):
        if f not in seen:           # linux and linux-headers share one archive
            seen.add(f)
            shutil.copy(os.path.join(d, f), out)
PY
    git -C "$HERE" archive --format=tar.gz --prefix=yargos/ -o "$L/yargos-source.tar.gz" HEAD
    { echo "YARG OS $VERSION"; echo "Source of this project: $(git -C "$HERE" remote get-url origin) commit $(git -C "$HERE" rev-parse HEAD)"; \
      echo "manifest.csv lists every package with its licence; licenses/ has the texts; sources/ has the source of the GPL/LGPL packages;"; \
      echo "rust-crates/ the licences of the Rust crates; yargos-source.tar.gz this project's own source."; } > "$L/README-legal.txt"
    tar -C "$(dirname "$L")" -cf "$OUT/legal-$VERSION.tar" "legal-$VERSION"
    rm -rf "$(dirname "$L")"
    echo "legal bundle: $(du -m "$OUT/legal-$VERSION.tar" | cut -f1) MB"
fi

echo "ok: $(du -m "$OUT/$FILE" | cut -f1) MB download (image $((RAW_SIZE / 1048576)) MB), signed"
ls -la "$OUT"
