#!/bin/bash
# Boot the YARGOS image in QEMU without touching it (writes go to a throwaway overlay).
#   tools/qemu-run.sh            boot, with a window; kernel/systemd log also prints in this terminal
#                                (Ctrl-A then X quits from the terminal)
#   tools/qemu-run.sh fresh      discard the previous session's changes first
#   USB passthrough (needs access to the device, e.g. run with sudo):
#     USB="1bad:0130" tools/qemu-run.sh        Ion Drum Rocker
#     USB="1bad:0130 8087:0a2b" ...            several, space separated
# Needs: sudo apt install qemu-system-x86 ovmf ; user in the kvm group.
set -eu
IMG=${IMG:-$HOME/yargos-build/output/images/yargos.img}
WORK=${WORK:-$HOME/yargos-build/qemu}
mkdir -p "$WORK"

CODE=/usr/share/OVMF/OVMF_CODE_4M.fd
VARS_SRC=/usr/share/OVMF/OVMF_VARS_4M.fd
[ -f "$CODE" ] || CODE=/usr/share/OVMF/OVMF_CODE.fd
[ -f "$VARS_SRC" ] || VARS_SRC=/usr/share/OVMF/OVMF_VARS.fd
[ -f "$CODE" ] || { echo "UEFI firmware not found; sudo apt install ovmf"; exit 1; }
command -v qemu-system-x86_64 >/dev/null || { echo "sudo apt install qemu-system-x86"; exit 1; }
[ -f "$IMG" ] || { echo "image not found: $IMG (run tools/br.sh)"; exit 1; }

[ "${1:-}" = fresh ] && rm -f "$WORK/overlay.qcow2" "$WORK/OVMF_VARS.fd"
[ -f "$WORK/OVMF_VARS.fd" ] || cp "$VARS_SRC" "$WORK/OVMF_VARS.fd"
[ -f "$WORK/overlay.qcow2" ] || qemu-img create -q -f qcow2 -b "$IMG" -F raw "$WORK/overlay.qcow2"

ACCEL=tcg; [ -w /dev/kvm ] && ACCEL=kvm
[ $ACCEL = kvm ] || echo "WARNING: no KVM access, this will be very slow (add yourself to the kvm group)"

USBARGS=()
for id in ${USB:-}; do USBARGS+=(-device "usb-host,vendorid=0x${id%%:*},productid=0x${id##*:}"); done

exec qemu-system-x86_64 \
    -machine q35,accel=$ACCEL -cpu max -smp 4 -m 4G \
    -drive if=pflash,format=raw,readonly=on,file="$CODE" \
    -drive if=pflash,format=raw,file="$WORK/OVMF_VARS.fd" \
    -drive file="$WORK/overlay.qcow2",format=qcow2,if=virtio \
    -device virtio-vga -display gtk \
    -device qemu-xhci -device usb-kbd -device usb-mouse \
    -device intel-hda -device hda-duplex \
    -nic user,model=virtio-net-pci \
    -serial mon:stdio \
    "${USBARGS[@]}"
