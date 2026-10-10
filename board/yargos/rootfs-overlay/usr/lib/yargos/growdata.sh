#!/bin/sh
# Grow the data partition (the last one on the drive) to fill the drive, then its filesystem.
# The image ships with a small data partition so it flashes quickly; this makes it use the whole stick/SSD.
# Safe to run on every boot: it does nothing when there is no free space after the partition.
set -eu

# The data partition as /etc/fstab names it: by GUID (the flashed image and any installed system), or by label.
SPEC=$(awk '$2 == "/data" { print $1; exit }' /etc/fstab)
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
[ -b "${PART:-/nonexistent}" ] || exit 0
NAME=$(basename "$PART")
DISK="/dev/$(basename "$(readlink -f "/sys/class/block/$NAME/..")")"
NUM=$(cat "/sys/class/block/$NAME/partition")

DISK_SECTORS=$(cat "/sys/class/block/$(basename "$DISK")/size")
PART_END=$(( $(cat "/sys/class/block/$NAME/start") + $(cat "/sys/class/block/$NAME/size") ))
FREE=$(( DISK_SECTORS - PART_END ))
if [ "$FREE" -le 32768 ]; then   # less than 16 MiB after the partition: nothing worth doing
    exit 0
fi

echo "yargos-growdata: growing partition $NUM of $DISK by $((FREE / 2048)) MiB"
# sfdisk also moves the backup GPT header to the real end of the drive.
echo ", +" | sfdisk --no-reread --no-tell-kernel -N "$NUM" "$DISK" >/dev/null
partx -u "$DISK"          # tell the kernel the partition got bigger (works while it is mounted)
resize2fs "$PART"
echo "yargos-growdata: done"
