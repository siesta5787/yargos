#!/bin/bash
# Write the YARGOS image to a USB stick (or SSD), with guard rails.
#   sudo tools/flash.sh                 list removable drives, then ask which one
#   sudo tools/flash.sh /dev/sdX        write to that drive (asks you to retype it first)
# EVERYTHING on the chosen drive is erased. The image is bootable on its own (UEFI): the stick IS the system.
set -eu
IMG=${IMG:-$HOME/yargos-build/output/images/yargos.img}
[ "$(id -u)" = 0 ] || { echo "run with sudo: sudo tools/flash.sh"; exit 1; }
[ -f "$IMG" ] || { echo "image not found: $IMG"; exit 1; }
SIZE=$(stat -c %s "$IMG")

list() { lsblk -dno NAME,SIZE,MODEL,TRAN,RM | awk '$NF==1 || $0 ~ /usb/ {printf "  /dev/%-8s %-8s %s\n", $1, $2, substr($0, index($0,$3))}'; }

DEV=${1:-}
if [ -z "$DEV" ]; then
    echo "Removable / USB drives found:"; list; echo
    read -rp "Write to which drive (e.g. /dev/sdb)? " DEV
fi
[ -b "$DEV" ] || { echo "$DEV is not a block device"; exit 1; }
NAME=$(basename "$DEV")
case "$NAME" in *[0-9]) [ -e "/sys/block/$NAME" ] || { echo "$DEV looks like a partition; give the whole drive (e.g. /dev/sdb)"; exit 1; } ;; esac
RM=$(cat "/sys/block/$NAME/removable" 2>/dev/null || echo 0)
TRAN=$(lsblk -dno TRAN "$DEV" 2>/dev/null || true)
if [ "$RM" != 1 ] && [ "$TRAN" != usb ]; then
    echo "$DEV is not a removable/USB drive. If you really mean to write the internal disk, run again with FORCE=1."
    [ -n "${FORCE:-}" ] || exit 1
fi
DEVSIZE=$(blockdev --getsize64 "$DEV")
[ "$DEVSIZE" -ge "$SIZE" ] || { echo "$DEV is $((DEVSIZE/1000000000)) GB, the image needs $((SIZE/1000000000+1)) GB"; exit 1; }
if findmnt -rno SOURCE | grep -q "^$DEV"; then
    # Never write over the drive the system is running from.
    ROOTDEV=$(findmnt -no SOURCE / | sed 's/[0-9p]*$//')
    [ "$ROOTDEV" != "$DEV" ] || { echo "$DEV holds the running system"; exit 1; }
fi

echo
lsblk -o NAME,SIZE,MODEL,TRAN,MOUNTPOINTS "$DEV"
echo
echo "This will ERASE $DEV and write $(basename "$IMG") ($((SIZE/1000000)) MB)."
read -rp "Type the drive name again to confirm ($DEV): " CONFIRM
[ "$CONFIRM" = "$DEV" ] || { echo "not confirmed, nothing written"; exit 1; }

for p in $(lsblk -lnpo NAME "$DEV" | tail -n +2); do umount "$p" 2>/dev/null || true; done
dd if="$IMG" of="$DEV" bs=4M status=progress conv=fsync
sync
echo "Checking what was written..."
cmp -n "$SIZE" "$IMG" "$DEV" && echo "OK: the drive matches the image." || { echo "MISMATCH: do not use this drive"; exit 1; }
echo "Done. Safely remove the drive, plug it into the mini PC and boot from it (UEFI, Secure Boot off)."
