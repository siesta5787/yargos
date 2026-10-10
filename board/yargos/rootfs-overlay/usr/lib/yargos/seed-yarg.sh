#!/bin/sh
# Installs the YARG release that came with the image into /data/apps/yarg. Runs as the PLAYER (yargos-seed.service):
# the folder is the player's, so root never has to copy into, or change the owner of, anything the player controls.
set -u

APPS=/data/apps/yarg
SEED=/usr/share/yarg-seed

[ -r "$SEED/VERSION" ] || exit 0   # an image without YARG: it is installed over the air later
SEED_VERSION=$(cat "$SEED/VERSION")
[ -w "$APPS" ] || { echo "yargos-seed: $APPS is not writable" >&2; exit 1; }

install_seed() {
    cp -a "$SEED/$SEED_VERSION" "$APPS/$SEED_VERSION.tmp" || { rm -rf "$APPS/$SEED_VERSION.tmp"; return 1; }
    mv "$APPS/$SEED_VERSION.tmp" "$APPS/$SEED_VERSION"
}

if [ ! -e "$APPS/current" ]; then
    # Fresh data partition.
    { [ -d "$APPS/$SEED_VERSION" ] || install_seed; } && ln -sfn "$SEED_VERSION" "$APPS/current"
else
    # Data partition kept across a reflash: adopt the image's YARG only if it is newer than what is
    # installed (an over-the-air update may already be ahead of the image).
    cur=$(basename "$(readlink "$APPS/current")")
    if [ "$cur" != "$SEED_VERSION" ] && [ ! -d "$APPS/$SEED_VERSION" ] &&
       [ "$(printf '%s\n%s\n' "$cur" "$SEED_VERSION" | sort -V | tail -1)" = "$SEED_VERSION" ]; then
        if install_seed; then
            ln -sfn "$cur" "$APPS/previous"
            ln -sfn "$SEED_VERSION" "$APPS/current"
        fi
    fi
fi
exit 0
