#!/bin/sh
# Prepares the writable data partition and installs the seeded YARG release.
set -eu

DATA=/data
APPS=$DATA/apps/yarg
SEED=/usr/share/yarg-seed
SEED_VERSION=$(cat "$SEED/VERSION")

mkdir -p "$APPS" "$DATA/yarg" "$DATA/home/player" "$DATA/bluetooth"

if [ ! -d "$APPS/$SEED_VERSION" ]; then
    cp -a "$SEED/$SEED_VERSION" "$APPS/$SEED_VERSION.tmp"
    mv "$APPS/$SEED_VERSION.tmp" "$APPS/$SEED_VERSION"
fi
if [ ! -e "$APPS/current" ]; then
    ln -sfn "$APPS/$SEED_VERSION" "$APPS/current"
fi

# Persist Bluetooth pairings/trust across reboots.
mkdir -p /var/lib/bluetooth
mount --bind "$DATA/bluetooth" /var/lib/bluetooth

chown -R player:player "$DATA/yarg" "$DATA/home/player" "$APPS"
