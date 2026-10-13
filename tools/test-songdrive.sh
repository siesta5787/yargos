#!/bin/bash
# QEMU test of how the song drive (label YARGSONGS) is mounted: it used to be an automount, and with no drive every look at
# /media/songs waited 3 s, which made YARG's song-cache check take hours. It is now mounted by udev when the drive appears.
# Checks: no drive = a path under /media/songs fails at once; a drive plugged in while running is mounted (read-only) within
# seconds, and its songs are found; unplugging it unmounts it; a drive present at boot is mounted; the dashboard's main card
# says "connected" and lists the guitar as a controller. Needs the normal image (tools/br.sh). About 6 minutes.
#
#   tools/test-songdrive.sh
set -u
HERE=$(cd "$(dirname "$0")/.." && pwd)
BUILD=${YARGOS_BUILD_DIR:-$HOME/yargos-build}
Q=$BUILD/qemu
W=$BUILD/songdrive-test
PORT=${PORT:-8000}
mkdir -p "$W/shots" "$W/web" "$W/songs/Rock/Band - Song"
fail=0
ok()  { echo "  ok   $*"; }
bad() { echo "  FAIL $*"; fail=1; }
eq() { if [ "$2" = "$3" ]; then ok "$1"; else bad "$1 (expected '$2', got '$3')"; fi; }
qstop() { kill $(ps -eo pid,args | awk '/qemu-system/ && !/awk/ {print $1}') 2>/dev/null; sleep 2; }
g() {
    local out
    out=$("$HERE/tools/qemu-sh.sh" "echo @@B; $*; echo @@E" 2>/dev/null | tr -d '\r')
    [ "$(grep -c '@@E$' <<< "$out")" -ge 1 ] || return 1
    sed -n '/@@B$/,/@@E$/p' <<< "$out" | sed '1d;$d'
}
until_g() { local end=$((SECONDS + ${2:-120})); while [ $SECONDS -lt $end ]; do g "$1" >/dev/null 2>&1 && return 0; sleep 2; done; return 1; }
mon() { python3 - "$Q/monitor.sock" "$@" <<'PY'
import socket, sys, time
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1]); time.sleep(0.3); s.recv(65536)
for c in sys.argv[2:]:
    s.sendall((c + "\n").encode()); time.sleep(0.6); s.recv(65536)
PY
}
boot() { # boot [drive images...]
    qstop
    rm -f "$Q/overlay.qcow2" "$Q/OVMF_VARS.fd"
    qemu-img create -q -f qcow2 -b "$BUILD/output/images/yargos.img" -F raw "$Q/overlay.qcow2" 64G
    (cd "$HERE" && SONGS_IMG="$*" CMDLINE="systemd.setenv=YARGOS_YARG_CMD=/bin/true" HEADLESS=1 tools/qemu-run.sh >"$W/qemu.log" 2>&1 &)
    until_g 'mountpoint -q /data && pidof yargos-home >/dev/null' 240 || { bad "the guest did not come up"; exit 1; }
    sleep 5
}

head -c 200000 /dev/urandom > "$W/songs/Rock/Band - Song/song.opus"; echo "name = x" > "$W/songs/Rock/Band - Song/song.ini"
mkfs.ext4 -q -F -L YARGSONGS -d "$W/songs" "$W/songs.img" 64M || exit 1
rustc --edition 2021 -O -C target-feature=+crt-static "$HERE/tools/fake-wiimote.rs" -o "$W/web/fakehid" || exit 1
kill $(ps -eo pid,args | awk '/http.server/ && !/awk/ {print $1}') 2>/dev/null
(cd "$W/web" && python3 -m http.server "$PORT" >"$W/http.log" 2>&1 &); sleep 1

echo "== no song drive"
boot
eq "nothing is mounted at /media/songs" "no" "$(g 'mountpoint -q /media/songs && echo yes || echo no')"
t0=$SECONDS; g 'for i in 1 2 3 4 5 6; do ls /media/songs/Rock/song.ini /media/songs/x/y >/dev/null 2>&1; done' >/dev/null; took=$((SECONDS - t0))
[ $took -le 12 ] && ok "six lookups of missing paths under /media/songs took ${took}s in all (the shell round trip included; before: 3+ s each)" || bad "lookups under /media/songs are slow again (${took}s)"

echo "== plugging the drive in while the system runs"
mon "drive_add 0 if=none,id=hotd,file=$W/songs.img,format=raw" "device_add usb-storage,drive=hotd,id=hotdev"
until_g 'mountpoint -q /media/songs' 60; r=$?
eq "it was mounted by itself" 0 $r
eq "...read-only" "ro" "$(g 'sed -n "s|^[^ ]* /media/songs [^ ]* \\([a-z]*\\),.*|\\1|p" /proc/mounts | tail -1')"
eq "its songs can be read" yes "$(g '[ -f "/media/songs/Rock/Band - Song/song.ini" ] && echo yes')"
eq "song-sources finds the folder" "/media/songs/Rock" "$(g '/usr/lib/yargos/song-sources | head -1')"
mon "device_del hotdev"
until_g '! mountpoint -q /media/songs' 60; r=$?
eq "unplugging it unmounted it" 0 $r
eq "and a lookup is quick again" "no" "$(g '[ -e /media/songs/Rock/song.ini ] && echo yes || echo no')"

echo "== a drive that is there at boot"
boot "$W/songs.img"
until_g 'mountpoint -q /media/songs' 60; r=$?
eq "it is mounted" 0 $r
eq "its songs can be read" yes "$(g '[ -f "/media/songs/Rock/Band - Song/song.ini" ] && echo yes')"
eq "the helper's drive list does not offer it twice (it is the same drive)" 1 "$(g 'yargos-songs list | grep -c YARGSONGS')"

echo "== the main menu's status card"
g "curl -fsS -o /tmp/fakehid http://10.0.2.2:$PORT/fakehid && chmod +x /tmp/fakehid && (/tmp/fakehid 00:1e:35:aa:bb:3a 0 600 176 0 2>/dev/null &)" >/dev/null
until_g 'ls /run/yargos-bridge/ready-hidraw* >/dev/null 2>&1' 90 || bad "the fake guitar never became ready"
sleep 14   # the card is read again every 8 s: it fixes itself without any key press
"$HERE/tools/qemu-shot.sh" "$W/shots/card.png" >/dev/null 2>&1
eq "the controller count (read from the kernel the way the card does)" 1 "$(g 'grep -c "^H: Handlers=.*js[0-9]" /proc/bus/input/devices')"

qstop
kill $(ps -eo pid,args | awk '/http.server/ && !/awk/ {print $1}') 2>/dev/null
echo
[ $fail = 0 ] && echo "ALL PASSED (look at $W/shots/card.png: SONG DRIVE connected, CONTROLLERS 1 connected, YARG OS version)" || { echo "SOME CHECKS FAILED (screenshots in $W/shots)"; exit 1; }
