#!/bin/sh
# Prepares the writable data partition and installs the seeded YARG release.
set -eu

DATA=/data
APPS=$DATA/apps/yarg
SEED=/usr/share/yarg-seed
SEED_VERSION=$(cat "$SEED/VERSION")

mkdir -p "$APPS" "$DATA/yarg" "$DATA/home/player" "$DATA/bluetooth"

install_seed() {
    cp -a "$SEED/$SEED_VERSION" "$APPS/$SEED_VERSION.tmp"
    mv "$APPS/$SEED_VERSION.tmp" "$APPS/$SEED_VERSION"
}

if [ ! -e "$APPS/current" ]; then
    # Fresh data partition.
    [ -d "$APPS/$SEED_VERSION" ] || install_seed
    ln -sfn "$SEED_VERSION" "$APPS/current"
else
    # Data partition kept across a reflash: adopt the image's YARG only if it is newer than what is
    # installed (an over-the-air update may already be ahead of the image).
    cur=$(basename "$(readlink "$APPS/current")")
    if [ "$cur" != "$SEED_VERSION" ] && [ ! -d "$APPS/$SEED_VERSION" ] &&
       [ "$(printf '%s\n%s\n' "$cur" "$SEED_VERSION" | sort -V | tail -1)" = "$SEED_VERSION" ]; then
        install_seed
        ln -sfn "$cur" "$APPS/previous"
        ln -sfn "$SEED_VERSION" "$APPS/current"
    fi
fi

if [ ! -f "$DATA/yargos.conf" ]; then
    cat > "$DATA/yargos.conf" <<'CONF'
# YARGOS settings (shell syntax).
# AUTO_UPDATE=yes  download and stage new YARG releases daily; they start the next time YARG launches.
# A new version that fails to stay up is rolled back automatically.
AUTO_UPDATE=yes
CONF
fi

# Persist Bluetooth pairings/trust across reboots.
mkdir -p /var/lib/bluetooth
mount --bind "$DATA/bluetooth" /var/lib/bluetooth

chown -R player:player "$DATA/yarg" "$DATA/home/player" "$APPS"
