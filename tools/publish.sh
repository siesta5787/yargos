#!/bin/bash
# Publish a system release made by tools/release.sh to GitHub (the repo's own releases), so devices find it.
#   tools/release.sh 2026.11.1 --github --notes "What is new"      make and sign it (after YARGOS_VERSION=2026.11.1 tools/br.sh)
#   tools/publish.sh 2026.11.1                                     shows what it would do
#   tools/publish.sh 2026.11.1 --yes                               does it
# It publishes in this order: the image and the legal bundle as release os-<version>; then the channel files into the
# standing release "os-channel" (devices look there). Devices ignore the new version until the channel files change,
# so the order makes a half-finished publish harmless. Needs `gh` logged in; the repo must be public.
set -eu
BUILD=${YARGOS_BUILD_DIR:-$HOME/yargos-build}
VERSION=${1:-}; [ -n "$VERSION" ] || { sed -n '2,10p' "$0"; exit 2; }
YES=no; CHANNEL=stable
shift
while [ $# -gt 0 ]; do
    case $1 in --yes) YES=yes ;; --channel) CHANNEL=$2; shift ;; *) echo "unknown option $1"; exit 2 ;; esac
    shift
done
DIR=${YARGOS_RELEASE_DIR:-$BUILD/release/$VERSION}
REPO=$(git -C "$(dirname "$0")/.." remote get-url origin | sed -e 's|^git@github.com:||' -e 's|^https://github.com/||' -e 's|\.git$||')
for f in "rootfs-$VERSION.img.xz" "legal-$VERSION.tar" "$CHANNEL.json" "$CHANNEL.json.sig"; do
    [ -f "$DIR/$f" ] || { echo "missing $DIR/$f (tools/release.sh $VERSION --github, with the legal bundle)"; exit 1; }
done
jq -e --arg b "https://github.com/$REPO/releases/download/os-$VERSION" '.base == $b' "$DIR/$CHANNEL.json" >/dev/null ||
    { echo "the manifest does not point at https://github.com/$REPO/releases/download/os-$VERSION: make it with --github"; exit 1; }
[ "$(gh repo view "$REPO" --json visibility -q .visibility)" = PUBLIC ] || { echo "$REPO is not public: devices could not download from it"; exit 1; }
echo "repo:        $REPO"
echo "version:     $VERSION ($CHANNEL channel)"
# The signed manifest goes on the version's own release page too: someone without internet on the device downloads these
# three files (the manifest, its signature, the image) on another computer and puts them on the song drive.
ASSETS=("$DIR/rootfs-$VERSION.img.xz" "$DIR/legal-$VERSION.tar" "$DIR/$CHANNEL.json" "$DIR/$CHANNEL.json.sig")
[ -f "$DIR/yargos-$VERSION.img.xz" ] && ASSETS+=("$DIR/yargos-$VERSION.img.xz")
echo "release:     os-$VERSION  <- $(for a in "${ASSETS[@]}"; do printf '%s (%s MB) ' "$(basename "$a")" "$(du -m "$a" | cut -f1)"; done)"
echo "channel:     os-channel  <- $CHANNEL.json, $CHANNEL.json.sig (replaces the old ones)"
[ "$YES" = yes ] || { echo "(nothing done: add --yes)"; exit 0; }
NOTES=$(jq -r '.notes // ""' "$DIR/$CHANNEL.json")
gh release create "os-$VERSION" "${ASSETS[@]}" --repo "$REPO" \
    --title "YARG OS $VERSION" --notes "${NOTES:-YARG OS $VERSION}. yargos-$VERSION.img.xz is the whole disk image to write to a USB stick (tools/flash.sh takes it as it is); rootfs-$VERSION.img.xz is what installed devices download themselves (SYSTEM > System update). No internet on the device? Download rootfs-$VERSION.img.xz, $CHANNEL.json and $CHANNEL.json.sig from this page, put the three files on the song drive and choose SYSTEM > System update > Install from the song drive. The legal file has the licences and the source of the GPL/LGPL parts."
gh release view os-channel --repo "$REPO" >/dev/null 2>&1 ||
    gh release create os-channel --repo "$REPO" --title "Update channel" --notes "Files the devices read to find the newest system. Do not edit by hand." --latest=false
gh release upload os-channel "$DIR/$CHANNEL.json" "$DIR/$CHANNEL.json.sig" --repo "$REPO" --clobber
echo "check: $(curl -fsSL "https://github.com/$REPO/releases/download/os-channel/$CHANNEL.json" | jq -r .version)"
