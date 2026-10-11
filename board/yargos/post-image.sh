#!/bin/sh
# Assemble the ESP (boot loader, boot entries, this version's kernel) and the whole disk image.
set -eu
BOARD_DIR=$(dirname "$0")
. "$BOARD_DIR/rootfs-overlay/usr/lib/yargos/entries.sh"
YARGOS_EXTRA_FILE=$TARGET_DIR/usr/lib/yargos/entry-extra-opts

VERSION=$(cat "$TARGET_DIR/usr/lib/yargos/version")
ROOT_A=6d5a1c52-0d4e-4c5b-9a3f-7e3b8a1f0001
DATA=6d5a1c52-0d4e-4c5b-9a3f-7e3b8a1f0002

install -D -m 0644 "$TARGET_DIR/usr/lib/systemd/boot/efi/systemd-bootx64.efi" "$BINARIES_DIR/systemd-bootx64.efi"
ESP=$BINARIES_DIR/esp-root
rm -rf "$ESP"
mkdir -p "$ESP/EFI/BOOT" "$ESP/loader/entries" "$ESP/yargos/$VERSION"
cp "$BINARIES_DIR/systemd-bootx64.efi" "$ESP/EFI/BOOT/BOOTX64.EFI"
cp "$BINARIES_DIR/bzImage" "$ESP/yargos/$VERSION/bzImage"
write_loader "$ESP" 5
write_os_entry "$ESP" "$VERSION" "$ROOT_A" "$DATA"
write_extra_entries "$ESP" "$VERSION" "$ROOT_A" "$DATA" install

# genimage wants every file named: build the list from what is in the tree.
FILES=$(cd "$ESP" && find . -type f | sed 's|^\./||' | sort | while read -r f; do
    printf '\t\tfile %s { image = "esp-root/%s" }\n' "$f" "$f"
done)
awk -v files="$FILES" '{ if ($0 == "@ESP_FILES@") print files; else print }' "$BOARD_DIR/genimage.cfg.in" > "$BINARIES_DIR/genimage.cfg"
echo "yargos-image: version $VERSION"
exec support/scripts/genimage.sh -c "$BINARIES_DIR/genimage.cfg"
