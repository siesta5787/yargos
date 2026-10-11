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
#       --fast                  quick, larger compression (for tests)
#
# Writes DIR/<channel>.json, DIR/<channel>.json.sig and DIR/rootfs-<version>.img.xz. Test it without GitHub:
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
CHANNEL=stable; OUT=; BASE=; NOTES=; KEY=$HOME/.yargos/release-ed25519.pem; XZ=-6
while [ $# -gt 0 ]; do
    case $1 in
        --channel) CHANNEL=$2; shift 2 ;;
        --out) OUT=$2; shift 2 ;;
        --base) BASE=$2; shift 2 ;;
        --notes) NOTES=$2; shift 2 ;;
        --key) KEY=$2; shift 2 ;;
        --fast) XZ=-1; shift ;;
        *) echo "unknown option $1"; exit 2 ;;
    esac
done
case $VERSION in *[!0-9.]* | "" | .*) echo "version must be dotted numbers, e.g. 2026.11.1"; exit 2 ;; esac
case $CHANNEL in stable | beta) ;; *) echo "channel is stable or beta"; exit 2 ;; esac
[ -f "$KEY" ] || { echo "signing key $KEY not found"; exit 1; }
[ -f "$IMG/rootfs.ext4" ] || { echo "no $IMG/rootfs.ext4: build first"; exit 1; }
OUT=${OUT:-$BUILD/release/$VERSION}

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
echo "ok: $(du -m "$OUT/$FILE" | cut -f1) MB download (image $((RAW_SIZE / 1048576)) MB), signed"
ls -la "$OUT"
