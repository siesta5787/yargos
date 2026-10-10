#!/bin/sh
# Prepares the writable data partition and installs the seeded YARG release. Runs on every boot.
# The small things the launcher needs (its files and their owner) come first and nothing before the slow YARG
# copy may stop the script: with `set -e` a failure there used to leave asound.conf owned by root, and the
# Sound screen then could not save its choice ("Permission denied").
set -u

DATA=/data
APPS=$DATA/apps/yarg
SEED=/usr/share/yarg-seed
SEED_VERSION=$(cat "$SEED/VERSION")

# Wait for the data partition to be mounted (it comes a moment after this service may start); ask for it if it is late.
n=0
while ! mountpoint -q "$DATA" && [ $n -lt 240 ]; do
    [ $n -eq 20 ] && mount "$DATA" 2>/dev/null
    sleep 0.5
    n=$((n + 1))
done
if ! mountpoint -q "$DATA"; then
    echo "yargos-firstboot: $DATA was never mounted; YARG cannot be set up" >&2
    exit 1
fi

mkdir -p "$APPS" "$DATA/yarg" "$DATA/home/player" "$DATA/bluetooth"

if [ ! -f "$DATA/yargos.conf" ]; then
    cat > "$DATA/yargos.conf" <<'CONF'
# YARGOS settings (shell syntax).
# AUTO_UPDATE=yes  download and stage new YARG releases daily; they start the next time YARG launches.
# A new version that fails to stay up is rolled back automatically.
AUTO_UPDATE=yes
CONF
fi
[ -e "$DATA/asound.conf" ] || : > "$DATA/asound.conf"   # /etc/asound.conf links here; the Sound screen writes it

# The player owns everything the launcher or YARG writes. /data itself too, so a file that is missing can be created.
chown player:player "$DATA" "$DATA/asound.conf" "$DATA/yargos.conf" "$DATA/yarg" "$DATA/home" "$DATA/home/player" "$APPS" "$DATA/apps"

# Persist Bluetooth pairings/trust across reboots.
mkdir -p /var/lib/bluetooth
mount --bind "$DATA/bluetooth" /var/lib/bluetooth || echo "yargos-firstboot: could not bind-mount the Bluetooth folder" >&2

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

chown -R player:player "$DATA/yarg" "$DATA/home/player" "$APPS"
exit 0
