#!/bin/bash
# Try yargos-bridge with real instruments on a normal desktop Linux machine.
#   tools/dev-test.sh build            build the bridge (no root)
#   sudo tools/dev-test.sh setup       stop hid_wiimote grabbing Wiimotes; open hidraw to your user
#   sudo tools/dev-test.sh drums       run the Ion Drum Rocker bridge
#   sudo tools/dev-test.sh wii         run the bridge for the first connected Wiimote
#   sudo tools/dev-test.sh watch       print reports from the virtual devices
#   sudo tools/dev-test.sh raw         print what the Ion Drum Rocker itself sends (before the bridge)
#   tools/dev-test.sh yarg             run the downloaded YARG in a window (as YOU, not root)
#   sudo tools/dev-test.sh teardown    undo setup
set -eu
HERE=$(cd "$(dirname "$0")/.." && pwd)
BIN=$HERE/src/bridge/target/release/yargos-bridge

case "${1:-}" in
build)
    (cd "$HERE/src/bridge" && cargo build --release)
    echo "built: $BIN" ;;
setup)
    echo 'blacklist hid_wiimote' > /etc/modprobe.d/yargos-dev.conf
    modprobe -r hid_wiimote 2>/dev/null || true
    cp "$HERE/board/yargos/rootfs-overlay/etc/udev/rules.d/69-hid.rules" /etc/udev/rules.d/69-yargos-dev.rules
    cp "$HERE/board/yargos/rootfs-overlay/etc/udev/rules.d/72-yargos-hide-raw.rules" /etc/udev/rules.d/72-yargos-dev-hide-raw.rules
    udevadm control --reload && udevadm trigger --subsystem-match=hidraw --subsystem-match=input
    echo "setup done. Pair the Wiimote with: bluetoothctl (scan on, hold 1+2, pair/trust/connect)" ;;
drums)
    DEV=$(for e in /sys/class/input/event*; do
        [ "$(cat $e/device/id/vendor 2>/dev/null)" = 1bad ] && [ "$(cat $e/device/id/product 2>/dev/null)" = 0130 ] && echo /dev/input/$(basename $e)
    done | head -1)
    [ -n "$DEV" ] || { echo "Ion Drum Rocker (1bad:0130) not found; is xpad loaded and the kit plugged in?"; exit 1; }
    echo "bridging $DEV"; exec "$BIN" drums "$DEV" ;;
wii)
    DEV=$(for h in /sys/class/hidraw/hidraw*; do
        grep -qE 'HID_ID=0005:0000057E:0000(0306|0330)' $h/device/uevent 2>/dev/null && echo /dev/$(basename $h)
    done | head -1)
    [ -n "$DEV" ] || { echo "No Wiimote hidraw found. Pair it first; make sure hid_wiimote is not loaded (lsmod | grep wiimote)."; exit 1; }
    echo "bridging $DEV"; exec "$BIN" wii "$DEV" ;;
raw)
    DEV=$(for e in /sys/class/input/event*; do
        [ "$(cat $e/device/id/vendor 2>/dev/null)" = 1bad ] && [ "$(cat $e/device/id/product 2>/dev/null)" = 0130 ] && echo /dev/input/$(basename $e)
    done | head -1)
    [ -n "$DEV" ] || { echo "Ion Drum Rocker not found"; exit 1; }
    echo "reading $DEV (stop the drums bridge first, it grabs the device). Ctrl-C to quit"
    python3 - "$DEV" <<'PY'
import struct, sys
BTN = {0x130:'green',0x131:'red',0x133:'blue',0x134:'yellow',0x136:'kick1(LB)',0x137:'CYMBAL',0x13d:'kick2(L3)',
       0x13e:'PAD',0x13b:'start',0x13a:'back',0x220:'dpad-up',0x221:'dpad-down'}
down, ax = set(), {0:0, 1:0, 3:0, 4:0}
last = None
print("quiet while idle; hit things (say what you hit as you go). Ctrl-C to stop.", flush=True)
with open(sys.argv[1], 'rb') as f:
    while True:
        t, c, v = struct.unpack('<HHi', f.read(24)[16:])
        if t == 1:
            (down.add if v else down.discard)(BTN.get(c, hex(c)))
        elif t == 3 and c in ax:
            ax[c] = v
        elif t == 0 and c == 0 and down:
            # xpad inverts the Y axes (~raw); undo that to show the kit's real 16-bit values
            r = lambda k: ax[k] & 0xFFFF if k in (0, 3) else (~ax[k]) & 0xFFFF
            line = f"{'+'.join(sorted(down)):<28} red={r(0):04x} yellow={r(1):04x} blue={r(3):04x} green={r(4):04x}"
            if line != last:
                print(line, flush=True); last = line
        if not down: last = None
PY
    ;;
watch)
    python3 - <<'PY'
import glob, os, select
devs = {}
for h in glob.glob('/sys/class/hidraw/hidraw*'):
    try: uev = open(h + '/device/uevent').read()
    except OSError: continue
    if 'HID_ID=0005:000012BA' in uev:
        n = '/dev/' + os.path.basename(h)
        devs[os.open(n, os.O_RDONLY)] = n
if not devs: raise SystemExit('no virtual 12BA devices found; is a bridge running?')
print('watching', list(devs.values()))
last = {}
while True:
    for fd in select.select(list(devs), [], [])[0]:
        r = os.read(fd, 64)
        if r != last.get(fd):
            last[fd] = r
            print(devs[fd], r.hex(' '))
PY
    ;;
yarg)
    [ "$(id -u)" != 0 ] || { echo "run this one without sudo"; exit 1; }
    Y=$HOME/yargos-build/yarg-test
    [ -x "$Y/yarg/YARG" ] || { echo "YARG not found in $Y/yarg"; exit 1; }
    ls -l /dev/hidraw* 2>/dev/null | awk '{print $1, $NF}' | grep -q '+' || \
        echo "note: hidraw nodes have no user access (no '+' ACL). Run: sudo tools/dev-test.sh setup, then restart the bridge."
    exec "$Y/yarg/YARG" -screen-fullscreen 0 -screen-width 1280 -screen-height 720 \
        -offline -persistent-data-path "$Y/data" ;;
teardown)
    rm -f /etc/modprobe.d/yargos-dev.conf /etc/udev/rules.d/69-yargos-dev.rules /etc/udev/rules.d/72-yargos-dev-hide-raw.rules
    udevadm control --reload ;;
*) sed -n 2,9p "$0" ;;
esac
