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

# Firmware stays as the .bin files linux-firmware installs. Compressing it to .xz (the kernel can read that, and
# a test with regulatory.db worked) saved 250 MB but the first real-hardware run lost Wi-Fi and sound, so it
# is off until it can be checked on the actual drivers. If a build left .xz firmware behind, the package must be
# reinstalled (tools/br.sh linux-firmware-dirclean) to get the plain files back.
FW=$TARGET_DIR/usr/lib/firmware
if [ -d "$FW" ] && find "$FW" -name '*.xz' | grep -q .; then
    echo "post-build: compressed firmware found in the target; run tools/br.sh linux-firmware-dirclean" >&2
    exit 1
fi

# Modules are not compressed (the build's depmod cannot read compressed ones). Drop stale compressed copies
# left by an earlier build so depmod indexes each module once.
find "$TARGET_DIR/usr/lib/modules" -name '*.ko.gz' -delete 2>/dev/null || true
