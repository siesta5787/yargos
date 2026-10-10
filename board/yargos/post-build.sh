#!/bin/sh
set -eu
TARGET_DIR=$1

# Quiet, console-like boot: no login banner.
: > "$TARGET_DIR/etc/issue"
: > "$TARGET_DIR/etc/motd"

mkdir -p "$TARGET_DIR/data" "$TARGET_DIR/media/songs"

# ---- trimming ---------------------------------------------------------------------------------
# Mesa's build pulls in Clang, libclc and the SPIR-V tools for OpenCL, which the appliance never uses.
# Nothing at runtime links the Clang libraries (checked with readelf); only these tools themselves do.
L=$TARGET_DIR/usr/lib
rm -f "$L"/libclang*.so* "$L"/libLLVMSPIRVLib*.so*
rm -rf "$TARGET_DIR/usr/share/clc" "$L/libscanbuild" "$L/python"*
rm -f "$TARGET_DIR"/usr/bin/diagtool "$TARGET_DIR"/usr/bin/llvm-spirv "$TARGET_DIR"/usr/bin/hmaptool \
      "$TARGET_DIR"/usr/bin/analyze-build "$TARGET_DIR"/usr/bin/scan-build-py "$TARGET_DIR"/usr/bin/spirv-*
rm -f "$TARGET_DIR"/usr/libexec/analyze-c++ "$TARGET_DIR"/usr/libexec/analyze-cc \
      "$TARGET_DIR"/usr/libexec/intercept-c++ "$TARGET_DIR"/usr/libexec/intercept-cc

# Firmware is the biggest part of the system. The kernel reads .xz files (CONFIG_FW_LOADER_COMPRESS_XZ), as
# distributions ship them, and they shrink to about a third. regulatory.db stays plain: cfg80211 wants it by name.
FW=$TARGET_DIR/usr/lib/firmware
if [ -d "$FW" ] && command -v xz >/dev/null; then
    find "$FW" -type f ! -name '*.xz' ! -name '*.zst' ! -name 'regulatory.db*' \
        -exec xz -f --check=crc32 --lzma2=dict=512KiB {} +
    # A link must point at the compressed file under a compressed name (foo.bin -> bar.bin becomes foo.bin.xz -> bar.bin.xz).
    find "$FW" -type l ! -name '*.xz' | while read -r link; do
        target=$(readlink "$link")
        case $target in
            /*) real=$TARGET_DIR$target ;;
            *) real=$(dirname "$link")/$target ;;
        esac
        if [ -f "$real.xz" ]; then
            ln -sf "$target.xz" "$link.xz"
            rm -f "$link"
        fi
    done
fi

# Modules are not compressed (the build's depmod cannot read compressed ones). Drop stale compressed copies
# left by an earlier build so depmod indexes each module once.
find "$TARGET_DIR/usr/lib/modules" -name '*.ko.gz' -delete 2>/dev/null || true
