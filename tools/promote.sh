#!/bin/bash
# Make a beta release the stable one: devices that follow the normal (stable) channel, and their automatic updates, see it
# from now on.
#   tools/promote.sh 2026.11.1          shows what it would do
#   tools/promote.sh 2026.11.1 --yes    does it
# Needs the files tools/release.sh made for the beta (beta.json and beta.json.sig in ~/yargos-build/release/<version>) and
# `gh` logged in. The manifest has no channel inside, and an Ed25519 signature is over the file's bytes, so stable.json is
# beta.json byte for byte, with the same signature: nothing is re-signed and no key is needed. The version page loses its
# pre-release mark and gets stable.json too (for installing from a song drive), then os-channel gets stable.json.
set -eu
BUILD=${YARGOS_BUILD_DIR:-$HOME/yargos-build}
VERSION=${1:-}; [ -n "$VERSION" ] || { sed -n '2,11p' "$0"; exit 2; }
YES=no; [ "${2:-}" = "--yes" ] && YES=yes
DIR=${YARGOS_RELEASE_DIR:-$BUILD/release/$VERSION}
REPO=$(git -C "$(dirname "$0")/.." remote get-url origin | sed -e 's|^git@github.com:||' -e 's|^https://github.com/||' -e 's|\.git$||')
HERE=$(cd "$(dirname "$0")/.." && pwd)
for f in beta.json beta.json.sig; do [ -f "$DIR/$f" ] || { echo "missing $DIR/$f (this version was not made as a beta)"; exit 1; }; done
gh release view "os-$VERSION" --repo "$REPO" >/dev/null 2>&1 || { echo "release os-$VERSION is not published: tools/publish.sh $VERSION --channel beta --yes first"; exit 1; }
# the copy has to verify the way a device checks it
cp "$DIR/beta.json" "$DIR/stable.json"
cp "$DIR/beta.json.sig" "$DIR/stable.json.sig"
ok=0
for pub in "$HERE"/board/yargos/rootfs-overlay/usr/lib/yargos/keys/*.pem; do
    openssl pkeyutl -verify -pubin -inkey "$pub" -rawin -in "$DIR/stable.json" -sigfile "$DIR/stable.json.sig" >/dev/null 2>&1 && { ok=1; break; }
done
[ $ok = 1 ] || { echo "the signature does not verify against any key in the image"; exit 1; }
CUR=$(curl -fsSL "https://github.com/$REPO/releases/download/os-channel/stable.json" 2>/dev/null | jq -r .version 2>/dev/null || echo none)
echo "repo:      $REPO"
echo "promote:   $VERSION (the stable channel now says $CUR)"
echo "does:      os-$VERSION is no longer a pre-release and becomes the latest release; stable.json + stable.json.sig go on its page and into os-channel"
[ "$YES" = yes ] || { echo "(nothing done: add --yes)"; exit 0; }
gh release upload "os-$VERSION" "$DIR/stable.json" "$DIR/stable.json.sig" --repo "$REPO" --clobber
gh release edit "os-$VERSION" --repo "$REPO" --prerelease=false --latest --title "YARG OS $VERSION"
gh release upload os-channel "$DIR/stable.json" "$DIR/stable.json.sig" --repo "$REPO" --clobber
echo "check: $(curl -fsSL "https://github.com/$REPO/releases/download/os-channel/stable.json" | jq -r .version)"
