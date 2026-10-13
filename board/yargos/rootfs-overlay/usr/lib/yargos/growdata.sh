#!/bin/sh
# Grow the data partition (the last one on the drive) to fill the drive, then its filesystem.
# The image ships with a small data partition so it flashes quickly; this makes it use the whole stick/SSD.
# Safe to run on every boot, and again at any time (the dashboard runs it when it finds unused room after the data
# partition: `grow-data` through the helper): it does nothing when there is no free space after the partition.
set -eu

# One at a time (the boot step and the dashboard may meet).
exec 9> /run/yargos-growdata.lock
flock -n 9 || exit 0

# What is really mounted as /data wins (when this runs after the boot, from the dashboard): the boot entry's GUID is only
# how it got there, and a lookup by GUID can name another disk's partition.
PART=$(awk '$2 == "/data" && $1 ~ /^\/dev\// { print $1 }' /proc/mounts | tail -n 1)
[ -n "$PART" ] && PART=$(readlink -f "$PART")
if [ ! -b "${PART:-/nonexistent}" ]; then
    # Not mounted yet (the boot step): the partition as the boot entry names it (systemd.mount-extra=PARTUUID=<guid>:/data:...).
    SPEC=$(tr ' ' '\n' < /proc/cmdline | sed -n 's|^systemd.mount-extra=\(PARTUUID=[^:]*\):/data:.*|\1|p' | head -n 1)
    [ -n "$SPEC" ] || { echo "yargos-growdata: the boot entry does not name the data partition; nothing to grow"; exit 0; }
    find_part() {
        case $SPEC in
            PARTUUID=*) readlink -f "/dev/disk/by-partuuid/${SPEC#PARTUUID=}" 2>/dev/null ;;
            LABEL=*) readlink -f "/dev/disk/by-label/${SPEC#LABEL=}" 2>/dev/null ;;
        esac
    }
    PART=$(find_part || true)
    n=0
    while [ ! -b "${PART:-/nonexistent}" ] && [ $n -lt 20 ]; do   # slow USB drives can take a few seconds to show up
        udevadm settle --timeout=2 2>/dev/null
        sleep 0.5
        PART=$(find_part || true)
        n=$((n + 1))
    done
    [ -b "${PART:-/nonexistent}" ] || { echo "yargos-growdata: the data partition ($SPEC) never appeared"; exit 0; }
fi
NAME=$(basename "$PART")
DISK="/dev/$(basename "$(readlink -f "/sys/class/block/$NAME/..")")"
NUM=$(cat "/sys/class/block/$NAME/partition")

DISK_SECTORS=$(cat "/sys/class/block/$(basename "$DISK")/size")
PART_END=$(( $(cat "/sys/class/block/$NAME/start") + $(cat "/sys/class/block/$NAME/size") ))
FREE=$(( DISK_SECTORS - PART_END ))
if [ "$FREE" -le 32768 ]; then   # less than 16 MiB after the partition: nothing worth doing
    echo "yargos-growdata: $PART already fills $DISK ($FREE sectors free after it)"
    exit 0
fi

echo "yargos-growdata: growing partition $NUM ($PART) of $DISK by $((FREE / 2048)) MiB"
# sfdisk also moves the backup GPT header to the real end of the drive.
# (sfdisk's complaints stay visible in the journal; a stick can answer "busy" for a moment right after it appears)
n=0
until echo ", +" | sfdisk --no-reread --no-tell-kernel -N "$NUM" "$DISK" >/dev/null; do
    n=$((n + 1))
    [ $n -ge 3 ] && { echo "yargos-growdata: could not change the partition table of $DISK" >&2; exit 1; }
    sleep 2
done
partx -u "$DISK"          # tell the kernel the partition got bigger (works while it is mounted)
resize2fs "$PART"
echo "yargos-growdata: done"
