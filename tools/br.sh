#!/bin/bash
# Run Buildroot for YARGOS from its permanent home (~/yargos-build).
#   tools/br.sh                       build / rebuild the image
#   tools/br.sh yargos-bridge-rebuild rebuild one package, then re-run for the image
#   tools/br.sh menuconfig            any other Buildroot target
#   YARGOS_WITH_YARG=1 tools/br.sh    a PRIVATE test image with YARG preloaded (never publish it: its release bundles
#                                     proprietary parts). Without the variable the image has no YARG: it is installed
#                                     on the device. The result is also kept as yargos-clean.img / yargos-with-yarg.img.
#
# The first build was done at /tmp/brout and /tmp/buildroot, and ~480 files in the
# host tools still embed those paths. /tmp is wiped at boot, so recreate them as
# symlinks to the real folders before every run.
set -eu
HOME_DIR=${YARGOS_BUILD_DIR:-$HOME/yargos-build}
for pair in "buildroot:/tmp/buildroot" "output:/tmp/brout"; do
    real=$HOME_DIR/${pair%%:*}; link=${pair##*:}
    [ -e "$link" ] || ln -s "$real" "$link"
done
cd /tmp/brout
# Switch the YARG package on or off to match the request (a switch back on needs its install step redone).
if [ -f .config ]; then
    has=$(grep -c '^BR2_PACKAGE_YARG=y' .config || true)
    if [ -n "${YARGOS_WITH_YARG:-}" ] && [ "$has" = 0 ]; then
        sed -i 's/^# BR2_PACKAGE_YARG is not set/BR2_PACKAGE_YARG=y/' .config
        make olddefconfig >/dev/null && make yarg-reinstall
    elif [ -z "${YARGOS_WITH_YARG:-}" ] && [ "$has" != 0 ]; then
        sed -i 's/^BR2_PACKAGE_YARG=y/# BR2_PACKAGE_YARG is not set/' .config
        make olddefconfig >/dev/null
    fi
fi
make -j"$(nproc)" "$@"
# A package target (e.g. yargos-bridge-rebuild) does not assemble the image; do that too.
if [ $# -gt 0 ] && [[ "$1" == *-rebuild || "$1" == *-reconfigure ]]; then make; fi
IMG_DIR=$HOME_DIR/output/images
if [ -f "$IMG_DIR/yargos.img" ]; then
    variant=clean; [ -n "${YARGOS_WITH_YARG:-}" ] && variant=with-yarg
    cp --reflink=auto "$IMG_DIR/yargos.img" "$IMG_DIR/yargos-$variant.img"
    echo "variant: $variant (YARG $([ $variant = clean ] && echo 'is installed on the device' || echo 'preloaded: PRIVATE, do not publish'))"
fi
echo "image: $IMG_DIR/yargos.img"
