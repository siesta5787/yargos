#!/bin/sh
# Prepares the writable data partition. Runs as root on every boot, before anything of the player's runs.
#
# Rule: root never follows a path the player can change. /data and the folders directly under it (apps, home,
# bluetooth, iwd, alsa) belong to root, so the player cannot swap them for links. Only the leaf folders the
# player's programs write to belong to the player (apps/yarg, yarg, home/player), and root only ever chowns those
# leaves themselves, never what is inside them (a link planted in there would hand over whatever it points at).
# Installing the seeded YARG into apps/yarg is the player's job (yargos-seed.service), not root's.
#
# The small things the launcher needs come first and nothing may stop the script (no `set -e`): a failure used to
# leave asound.conf unusable and the Sound screen could not save its choice.
set -u

DATA=/data

# Wait for the data partition to be mounted (it comes a moment after this service may start); ask for it if it is late.
n=0
while ! mountpoint -q "$DATA" && [ $n -lt 240 ]; do
    [ $n -eq 20 ] && systemctl start --no-block data.mount 2>/dev/null
    sleep 0.5
    n=$((n + 1))
done
if ! mountpoint -q "$DATA"; then
    echo "yargos-firstboot: $DATA was never mounted; YARG cannot be set up" >&2
    exit 1
fi

# dir PATH OWNER MODE: a real folder (never a link) with this owner, created if missing.
dir() {
    [ -L "$1" ] && rm -f "$1"
    [ -d "$1" ] || { rm -rf "$1"; mkdir -p "$1"; }
    chown -h "$2" "$1"
    chmod "$3" "$1"
}
# file PATH OWNER MODE: a real file (never a link or anything else) with this owner; kept if it is already one.
file() {
    { [ -L "$1" ] || [ ! -f "$1" ]; } && { rm -rf "$1"; : > "$1"; }
    chown -h "$2" "$1"
    chmod "$3" "$1"
}

chown root:root "$DATA"
chmod 0755 "$DATA"
for d in apps home bluetooth iwd alsa; do dir "$DATA/$d" root:root 0755; done
chmod 0700 "$DATA/bluetooth" "$DATA/iwd"
dir "$DATA/apps/yarg" player:player 0755
dir "$DATA/yarg" player:player 0755
dir "$DATA/home/player" player:player 0755

if [ ! -f "$DATA/yargos.conf" ] || [ -L "$DATA/yargos.conf" ]; then
    rm -rf "$DATA/yargos.conf"
    cat > "$DATA/yargos.conf" <<'CONF'
# YARGOS settings. Only these exact lines are read (as text, nothing in here is ever run).
# AUTO_UPDATE=yes  download and stage new YARG releases daily; they start the next time YARG launches.
# A new version that fails to stay up is rolled back automatically.
AUTO_UPDATE=yes
CONF
fi
# The launcher edits it in place, so it belongs to the player; root only reads it as text.
file "$DATA/yargos.conf" player:player 0644

# /etc/asound.conf links here. ROOT owns it: root's own audio tools (alsactl, restoring the volume at boot) read it,
# and an ALSA config can load a program, so the player must not be able to write it. The Sound screen asks the system
# helper to write it. One written by an older image belonged to the player and cannot be trusted: it is dropped.
# (-O: owned by this process's user, i.e. root. There is no stat command in this image.)
[ -O "$DATA/asound.conf" ] && [ ! -L "$DATA/asound.conf" ] || rm -rf "$DATA/asound.conf"
file "$DATA/asound.conf" root:root 0644

# State that must outlive the system itself lives on /data: Bluetooth pairings, Wi-Fi passwords, the volume.
# (An older system kept the last two on its own disk: they are carried over once.)
persist() { # persist NAME
    src=/var/lib/$1
    mkdir -p "$src"
    if [ -z "$(ls -A "$DATA/$1" 2>/dev/null)" ] && [ -n "$(ls -A "$src" 2>/dev/null)" ]; then
        cp -a "$src/." "$DATA/$1/" 2>/dev/null
    fi
    mount --bind "$DATA/$1" "$src" || echo "yargos-firstboot: could not bind-mount $DATA/$1" >&2
}
for s in bluetooth iwd alsa; do persist "$s"; done

exit 0
