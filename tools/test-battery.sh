#!/bin/bash
# QEMU test of the Wii guitar battery display: two fake guitars (tools/fake-wiimote.rs, which can report any battery level)
# are plugged into the guest; the test checks what the bridge writes, how long it takes to refresh while the guitar idles,
# that the dashboard shows a toast and the status card line for a low guitar, and that a guitar that runs low while the
# "game" runs gets a notice drawn over it. A stand-in program (a script that just sleeps; NOT cat: it would read the
# console the session owns and freeze the session) plays the game, so what is checked is the notice and its window,
# not YARG's own window.
# Needs the normal image (tools/br.sh). Takes about 6 minutes.
#
#   tools/test-battery.sh
set -u
HERE=$(cd "$(dirname "$0")/.." && pwd)
BUILD=${YARGOS_BUILD_DIR:-$HOME/yargos-build}
Q=$BUILD/qemu
W=$BUILD/battery-test
PORT=${PORT:-8000}
mkdir -p "$W/web" "$W/shots"
fail=0
ok()  { echo "  ok   $*"; }
bad() { echo "  FAIL $*"; fail=1; }
eq() { if [ "$2" = "$3" ]; then ok "$1"; else bad "$1 (expected '$2', got '$3')"; fi; }
has() { case $3 in *"$2"*) ok "$1" ;; *) bad "$1 (wanted '$2' in: $3)" ;; esac; }
qstop() { kill $(ps -eo pid,args | awk '/qemu-system/ && !/awk/ {print $1}') 2>/dev/null; sleep 2; }
g() {
    local out
    out=$("$HERE/tools/qemu-sh.sh" "echo @@B; $*; echo @@E" 2>/dev/null | tr -d '\r')
    [ "$(grep -c '@@E$' <<< "$out")" -ge 1 ] || return 1
    sed -n '/@@B$/,/@@E$/p' <<< "$out" | sed '1d;$d'
}
until_g() { local end=$((SECONDS + ${2:-120})); while [ $SECONDS -lt $end ]; do g "$1" >/dev/null 2>&1 && return 0; sleep 3; done; return 1; }
shot() { "$HERE/tools/qemu-shot.sh" "$W/shots/$1.png" >/dev/null 2>&1; }

rustc --edition 2021 -O -C target-feature=+crt-static "$HERE/tools/fake-wiimote.rs" -o "$W/web/fakehid" || exit 1
kill $(ps -eo pid,args | awk '/http.server/ && !/awk/ {print $1}') 2>/dev/null
(cd "$W/web" && python3 -m http.server "$PORT" >"$W/http.log" 2>&1 &); sleep 1

echo "== starting the guest (the game is a stand-in program)"
qstop
rm -f "$Q/overlay.qcow2" "$Q/OVMF_VARS.fd"
qemu-img create -q -f qcow2 -b "$BUILD/output/images/yargos.img" -F raw "$Q/overlay.qcow2" 64G
(cd "$HERE" && CMDLINE="systemd.setenv=YARGOS_YARG_CMD=/tmp/standin" HEADLESS=1 tools/qemu-run.sh >"$W/qemu.log" 2>&1 &)
until_g 'mountpoint -q /data && pidof yargos-home >/dev/null' 240 || { bad "the guest did not come up"; exit 1; }
sleep 6
g "curl -fsS -o /tmp/fakehid http://10.0.2.2:$PORT/fakehid && chmod +x /tmp/fakehid" >/dev/null
g "printf '%s\n' '#"'!'"/bin/sh' 'exec sleep 3600 </dev/null' > /tmp/standin; chmod +x /tmp/standin" >/dev/null   # (the console shell would take a bare !/ for history)
eq "the stand-in game program exists" yes "$(g '[ -x /tmp/standin ] && echo yes')"
eq "the fake guitar program arrived" yes "$(g '[ -x /tmp/fakehid ] && echo yes')"

echo "== a guitar with plenty of battery"
g '(/tmp/fakehid 00:1e:35:aa:bb:3a 0 900 176 0 2>/tmp/fk-a.log &)' >/dev/null
until_g 'ls /run/yargos-bridge/battery-hidraw* 2>/dev/null | grep -q .' 90; r=$?
eq "the bridge wrote its battery" 0 $r
eq "88% (176 of 200), not low" "88 0" "$(g 'cat /run/yargos-bridge/battery-hidraw*')"
has "wiicheck shows it" "battery: 88%" "$(g 'yargos-home wiicheck')"
eq "no warning for a healthy guitar" "" "$(g 'journalctl -b --no-pager | grep "Low battery" | head -1')"

echo "== it asks again while the guitar idles (never while it is played)"
sleep 75
eq "a second status request arrived after the idle wait" 1 "$(g 'grep -c "status ->" /tmp/fk-a.log | awk "{print (\$1 >= 2)}"')"
eq "the guitar is still streaming (ready marker present)" yes "$(g 'ls /run/yargos-bridge/ready-hidraw* >/dev/null 2>&1 && echo yes')"

echo "== a second guitar runs low while the dashboard is up"
g '(/tmp/fakehid 00:1e:35:aa:bb:3b 0 900 24 1 2>/tmp/fk-b.log &)' >/dev/null
until_g 'cat /run/yargos-bridge/battery-hidraw* 2>/dev/null | grep -q "^12 1"' 90; r=$?
eq "its low battery (12%, flag raised) was written" 0 $r
sleep 14
shot dashboard-low
has "the dashboard announced it, named by the end of its address" "low battery: Wii guitar (3B) 12%" "$(g 'journalctl -b --no-pager | grep "low battery:"')"
eq "it was announced only once so far" 1 "$(g 'journalctl -b --no-pager | grep -c "low battery:"')"

echo "== a third guitar runs low while the game runs"
# The image has no YARG, so the first entry would be INSTALL YARG: make it look installed, then reopen the main menu.
g 'mkdir -p /data/apps/yarg/0.0.1 && ln -sfn 0.0.1 /data/apps/yarg/current' >/dev/null
shot home-before
# (the main menu reads its status when it is entered: go into Wi-Fi and back)
"$HERE/tools/qemu-key.sh" down ret; sleep 3; "$HERE/tools/qemu-key.sh" esc; sleep 14   # (reading the status waits on the empty song-drive automount)
shot home-after-setup
"$HERE/tools/qemu-key.sh" ret; sleep 8   # the highlight is back on the first entry, PLAY YARG: the stand-in game starts
eq "the stand-in game is running" yes "$(g 'pidof sleep >/dev/null && echo yes')"
g '(/tmp/fakehid 00:1e:35:aa:bb:3c 0 900 20 1 2>/tmp/fk-c.log &)' >/dev/null
until_g 'ls /run/yargos-bridge/battery-hidraw* | wc -l | grep -q 3' 90; r=$?
eq "the third guitar told its battery" 0 $r
# the notice appears within 10 s of the guitar telling its battery and stays 10 s: look as soon as the session says so
until_g 'journalctl -b --no-pager | grep -q "banner over the game"' 60; r=$?
eq "the session showed a notice over the game" 0 $r
sleep 1
X=$(g 'DISPLAY=:0 yargos-home xinfo 2>/dev/null')   # (one look, while the notice is up: it stays only 10 s)
shot game-low
has "  ...for the right guitar and level (a level byte of 20 is 10%)" "banner over the game: Wii guitar (3C) 10%" "$(g 'journalctl -b --no-pager | grep "banner over the game"')"
eq "two windows on screen: the session's own and the notice" 2 "$(grep 'map=VIEWABLE' <<< "$X" | grep -c 'override=true')"
eq "the notice is a small window near the top, not a full-screen one" 1 "$(grep 'map=VIEWABLE' <<< "$X" | grep 'override=true' | grep -v '1280x800' | grep -c '+[0-9]*+[0-9]\{1,2\} ')"
eq "the input focus is still the session's (the notice takes none)" 1 "$(grep -c '^input focus: 0x400000' <<< "$X")"
sleep 13
shot game-after
eq "the notice is taken down by itself" 1 "$(g 'DISPLAY=:0 yargos-home xinfo 2>/dev/null | grep "map=VIEWABLE" | grep -c "override=true"')"

qstop
kill $(ps -eo pid,args | awk '/http.server/ && !/awk/ {print $1}') 2>/dev/null
echo
[ $fail = 0 ] && echo "ALL PASSED (look at the screenshots in $W/shots)" || { echo "SOME CHECKS FAILED (screenshots in $W/shots)"; exit 1; }
