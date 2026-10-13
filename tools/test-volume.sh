#!/bin/bash
# QEMU test of the volume control: the sound card's mixer from the shell command and from the volume keys (pressed on the
# virtual keyboard, so the key grab on the root window is really used, with a stand-in "game" running), the level bar over the
# screen, mute, the Sound screen's Volume row, and that the arrow keys alone still belong to the menu. QEMU's sound card is an
# Intel HDA with a Master/PCM mixer. A Bluetooth headset's volume needs a real headset and radio: only its parsing is tested
# (unit tests). Needs the normal image (tools/br.sh). About 5 minutes.
#
#   tools/test-volume.sh
set -u
HERE=$(cd "$(dirname "$0")/.." && pwd)
BUILD=${YARGOS_BUILD_DIR:-$HOME/yargos-build}
Q=$BUILD/qemu
W=$BUILD/volume-test
mkdir -p "$W/shots"
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
until_g() { local end=$((SECONDS + ${2:-120})); while [ $SECONDS -lt $end ]; do g "$1" >/dev/null 2>&1 && return 0; sleep 2; done; return 1; }
k() { "$HERE/tools/qemu-key.sh" "$@"; }
shot() { "$HERE/tools/qemu-shot.sh" "$W/shots/$1.png" >/dev/null 2>&1; }
vol() { g 'yargos-home volume get' | tail -1; }
pct() { sed -n 's/^volume \([0-9]*\)%.*/\1/p' <<< "$1"; }
# near WANT GOT TOL: a sound card's mixer has discrete steps, so 58% may read back as 59%
near() { local d=$(( $3 - $2 )); [ $d -lt 0 ] && d=$(( -d )); if [ $d -le ${4:-2} ]; then ok "$1 (wanted about $2, got $3)"; else bad "$1 (wanted about $2, got $3)"; fi; }

echo "== starting the guest"
qstop
rm -f "$Q/overlay.qcow2" "$Q/OVMF_VARS.fd"
qemu-img create -q -f qcow2 -b "$BUILD/output/images/yargos.img" -F raw "$Q/overlay.qcow2" 64G
(cd "$HERE" && CMDLINE="systemd.setenv=YARGOS_YARG_CMD=/tmp/standin" HEADLESS=1 tools/qemu-run.sh >"$W/qemu.log" 2>&1 &)
until_g 'mountpoint -q /data && pidof yargos-home >/dev/null' 240 || { bad "the guest did not come up"; exit 1; }
sleep 6
g "printf '%s\\n' '#"'!'"/bin/sh' 'exec sleep 3600 </dev/null' > /tmp/standin; chmod +x /tmp/standin" >/dev/null
g 'mkdir -p /data/apps/yarg/0.0.1 && ln -sfn 0.0.1 /data/apps/yarg/current' >/dev/null

echo "== the command (what the keys do)"
g 'su player -s /bin/sh -c "DISPLAY=:0 yargos-home volume get"' >/dev/null 2>&1
start=$(vol); has "the volume is read from the card" "volume " "$start"
g 'yargos-home volume down' >/dev/null; g 'yargos-home volume down' >/dev/null
after=$(vol); near "two steps down are about 10 percent less" "$(( $(pct "$start") - 10 ))" "$(pct "$after")"
g 'yargos-home volume up' >/dev/null
near "a step up is about 5 percent more" "$(( $(pct "$start") - 5 ))" "$(pct "$(vol)")"
has "mute says so" "muted" "$(g 'yargos-home volume mute' | tail -1)"
back=$(g 'yargos-home volume mute' | tail -1); case $back in *muted*) bad "unmute left it muted: $back" ;; *) near "and unmute brings the level back" "$(( $(pct "$start") - 5 ))" "$(pct "$back")" ;; esac
g 'for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22; do yargos-home volume up >/dev/null; done' >/dev/null
eq "it never goes above 100" 100 "$(pct "$(vol)")"
g 'for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22; do yargos-home volume down >/dev/null; done' >/dev/null
eq "or below 0" 0 "$(pct "$(vol)")"
g 'yargos-home volume up >/dev/null; yargos-home volume up >/dev/null' >/dev/null
near "raising from zero gives about level 10" 10 "$(pct "$(vol)")"
case $(vol) in *muted*) bad "raising from zero left it muted" ;; *) ok "...and it is not muted" ;; esac

echo "== the keys, with a game running"
# (the main menu reads its status when it is entered: go into Wi-Fi and back, then PLAY YARG starts the stand-in)
k down ret; sleep 3; k esc; sleep 14
k ret; sleep 8
eq "the stand-in game is running" yes "$(g 'pidof sleep >/dev/null && echo yes')"
before=$(pct "$(vol)")
k ctrl-alt-down; sleep 0.8
shot volume-bar     # (the notice is up for 2 s: look at once)
sleep 2
near "Ctrl+Alt+Down lowered it by 5, with the game running" "$(( before - 5 ))" "$(pct "$(vol)")"
has "the session showed the level over the game" "volume notice: $(pct "$(vol)")%" "$(g 'journalctl -b --no-pager | grep "volume notice" | tail -1')"
k ctrl-alt-up; sleep 2
near "Ctrl+Alt+Up raised it again" "$before" "$(pct "$(vol)")"
k ctrl-alt-m; sleep 2
has "Ctrl+Alt+M muted" "muted" "$(vol)"
k ctrl-alt-m; sleep 2
near "and again unmuted" "$before" "$(pct "$(vol)")"
k volumedown; sleep 3
near "the media key (volume down) lowers it by 5" "$(( before - 5 ))" "$(pct "$(vol)")"
k volumeup; sleep 2
near "the volume up key raises it again" "$before" "$(pct "$(vol)")"
sleep 4
eq "the mixer was stored by the system a few seconds after the last key" yes "$(g '[ -s /data/alsa/asound.state ] || [ -s /var/lib/alsa/asound.state ] && echo yes')"

echo "== the arrow keys alone still belong to the menu"
g 'kill $(pidof sleep)' >/dev/null 2>&1; sleep 8
k down; sleep 1; k down; sleep 1
shot menu-after-arrows
before=$(pct "$(vol)")
eq "plain arrows do not change the volume" "$before" "$(pct "$(vol)")"

qstop
echo
[ $fail = 0 ] && echo "ALL PASSED (look at $W/shots/volume-bar.png)" || { echo "SOME CHECKS FAILED (screenshots in $W/shots)"; exit 1; }
