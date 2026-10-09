#!/bin/sh
# Grow the data partition (the last one on the drive) to fill the drive, then its filesystem.
# The image ships with a small data partition so it flashes quickly; this makes it use the whole stick/SSD.
# Safe to run on every boot: it does nothing when there is no free space after the partition.
set -eu

PART=$(readlink -f /dev/disk/by-label/YARGDATA 2>/dev/null || true)
[ -b "$PART" ] || exit 0
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
