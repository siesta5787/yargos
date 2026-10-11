#!/bin/sh
set -eu
TARGET_DIR=$1

# Quiet, console-like boot: no login banner.
: > "$TARGET_DIR/etc/issue"
: > "$TARGET_DIR/etc/motd"

mkdir -p "$TARGET_DIR/data" "$TARGET_DIR/media/songs"

# The system's version: what the updater compares and what names its boot entry and kernel. A release sets
# YARGOS_VERSION (tools/release.sh); a development build gets the build time, which is newer than the one before.
VERSION=${YARGOS_VERSION:-$(date +%Y.%m.%d.%H%M)}
mkdir -p "$TARGET_DIR/usr/lib/yargos"
echo "$VERSION" > "$TARGET_DIR/usr/lib/yargos/version"
# A test build (tools/test-os-update.sh) gets a serial console on the normal boot entries; a real build must not.
if [ -n "${YARGOS_TEST_CONSOLE:-}" ]; then
    echo "console=ttyS0,115200" > "$TARGET_DIR/usr/lib/yargos/entry-extra-opts"
else
    rm -f "$TARGET_DIR/usr/lib/yargos/entry-extra-opts"
fi
sed -i -e '/^NAME=/c NAME="YARG OS"' -e '/^PRETTY_NAME=/c PRETTY_NAME="YARG OS '"$VERSION"'"' -e '/^VERSION_ID=/c VERSION_ID='"$VERSION" "$TARGET_DIR/usr/lib/os-release"

# The image has no YARG unless it was asked for (tools/br.sh with YARGOS_WITH_YARG=1). Buildroot never removes what a
# package installed before, so take the seed out by hand when the package is not selected.
grep -q '^BR2_PACKAGE_YARG=y' "$BR2_CONFIG" || rm -rf "$TARGET_DIR/usr/share/yarg-seed"

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

# alsa-plugins is here only for the high-quality sample-rate converter (libsamplerate). Its other plugins (upmix,
# vdownmix, OSS emulation, the Arcam control) would each show up as another output device in YARG's list.
A=$TARGET_DIR/usr/lib/alsa-lib
rm -f "$A"/libasound_module_pcm_upmix.so "$A"/libasound_module_pcm_vdownmix.so "$A"/libasound_module_pcm_oss.so \
      "$A"/libasound_module_ctl_oss.so "$A"/libasound_module_ctl_arcam_av.so
for f in 10-samplerate.conf 50-oss.conf 50-arcam-av-ctl.conf 60-upmix.conf 60-vdownmix.conf; do
    rm -f "$TARGET_DIR/etc/alsa/conf.d/$f" "$TARGET_DIR/usr/share/alsa/alsa.conf.d/$f"
done
