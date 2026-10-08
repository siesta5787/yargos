#!/bin/sh
set -eu
BOARD_DIR=$(dirname "$0")

install -D -m 0644 "$TARGET_DIR/usr/lib/systemd/boot/efi/systemd-bootx64.efi" \
	"$BINARIES_DIR/systemd-bootx64.efi"
rm -rf "$BINARIES_DIR/esp"
cp -a "$BOARD_DIR/esp" "$BINARIES_DIR/esp"

exec support/scripts/genimage.sh -c "$BOARD_DIR/genimage.cfg"
