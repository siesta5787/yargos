#!/bin/sh
set -eu
TARGET_DIR=$1

# Quiet, console-like boot: no login banner.
: > "$TARGET_DIR/etc/issue"
: > "$TARGET_DIR/etc/motd"

mkdir -p "$TARGET_DIR/data" "$TARGET_DIR/media/songs"
