#!/bin/bash
# Run Buildroot for YARGOS from its permanent home (~/yargos-build).
#   tools/br.sh                       build / rebuild the image
#   tools/br.sh yargos-bridge-rebuild rebuild one package, then re-run for the image
#   tools/br.sh menuconfig            any other Buildroot target
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
make -j"$(nproc)" "$@"
# A package target (e.g. yargos-bridge-rebuild) does not assemble the image; do that too.
if [ $# -gt 0 ] && [[ "$1" == *-rebuild || "$1" == *-reconfigure ]]; then make; fi
echo "image: $HOME_DIR/output/images/yargos.img"
