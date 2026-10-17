#!/bin/bash
# QEMU test of the parts of Bluetooth sound that do not need a radio (QEMU has none, so pairing, the audio link, its delay and
# its sharing of the radio with the Wii guitars can only be tried on real hardware). Checks: BlueZ has its audio plugin;
# bluealsa starts, offers only the A2DP source role, and its ALSA plugin loads; the root helper takes only a real address;
# a chosen headset that is NOT connected leaves YARG with a real output (the speakers play, the choice is kept), and a
# stale headset config with nothing chosen is replaced; choosing a sound card forgets the headset. Needs the normal image
# (tools/br.sh). About 4 minutes.
#
#   tools/test-btaudio.sh
set -u
HERE=$(cd "$(dirname "$0")/.." && pwd)
BUILD=${YARGOS_BUILD_DIR:-$HOME/yargos-build}
Q=$BUILD/qemu
W=$BUILD/btaudio-test
mkdir -p "$W"
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
MAC=08:DF:1F:AA:BB:CC

echo "== starting the guest (the game is a stand-in program)"
qstop
rm -f "$Q/overlay.qcow2" "$Q/OVMF_VARS.fd"
qemu-img create -q -f qcow2 -b "$BUILD/output/images/yargos.img" -F raw "$Q/overlay.qcow2" 64G
(cd "$HERE" && CMDLINE="systemd.setenv=YARGOS_YARG_CMD=/tmp/standin" HEADLESS=1 tools/qemu-run.sh >"$W/qemu.log" 2>&1 &)
until_g 'mountpoint -q /data && pidof yargos-home >/dev/null' 240 || { bad "the guest did not come up"; exit 1; }
sleep 6
g "printf '%s\\n' '#"'!'"/bin/sh' 'exec sleep 3600 </dev/null' > /tmp/standin; chmod +x /tmp/standin" >/dev/null   # (the console shell takes a bare !/ for history)
g 'mkdir -p /data/apps/yarg/0.0.1 && ln -sfn 0.0.1 /data/apps/yarg/current' >/dev/null

echo "== the pieces are in the image"
has "bluetoothd has the audio (A2DP) plugin" "a2dp" "$(g 'strings -a /usr/libexec/bluetooth/bluetoothd | grep -m1 -i a2dp')"
eq "the bluealsa ALSA plugin is installed" yes "$(g '[ -f /usr/lib/alsa-lib/libasound_module_pcm_bluealsa.so ] && echo yes')"
eq "bluealsa is wanted by bluetooth.target" yes "$(g '[ -e /etc/systemd/system/bluetooth.target.wants/bluealsa.service ] && echo yes')"
has "it only offers the A2DP source role (the command systemd will run)" "-p a2dp-source" "$(g 'systemctl show bluealsa.service -p ExecStart')"
eq "and not the speaker (sink) role" 0 "$(g 'systemctl show bluealsa.service -p ExecStart | grep -c a2dp-sink')"
g 'systemctl start bluetooth bluealsa' >/dev/null 2>&1; sleep 4
eq "bluealsa runs" active "$(g 'systemctl is-active bluealsa')"
eq "the player may talk to it (group audio)" yes "$(g 'id player | grep -q "(audio)" && echo yes')"
eq "bluetoothctl can list connected devices (what the dashboard asks)" 0 "$(g 'bluetoothctl devices Connected >/dev/null 2>&1; echo $?')"

echo "== the helper takes only a real address"
eq "a good address is accepted" ok "$(g "echo 'sound-bt $MAC' | yargos-home ctl | tail -1")"
has "the config names the headset" "device \"$MAC\"" "$(g 'cat /data/asound.conf')"
eq "and the choice is recorded" "$MAC" "$(g 'cat /data/asound.prefer')"
eq "owned by root, not the player" 0 "$(g 'find /data/asound.conf /data/asound.prefer ! -user root | wc -l')"
has "a bad address is refused" "error" "$(g "echo 'sound-bt 08:DF:1F:AA:BB:CC; reboot' | yargos-home ctl | tail -1")"
has "a short address is refused" "error" "$(g "echo 'sound-bt 08:DF:1F:AA:BB' | yargos-home ctl | tail -1")"
eq "the config was not changed by them" "$MAC" "$(g 'cat /data/asound.prefer')"

echo "== a default output that does not exist must not be left for YARG"
t0=$SECONDS; r=$(g 'aplay -D default -d 1 /dev/zero 2>&1 | tail -1'); took=$((SECONDS - t0))
[ $took -le 20 ] && ok "playing to a headset that is not there fails quickly instead of hanging (${took}s): $r" || bad "playing to a missing headset hung (${took}s)"
# Start the game from the dashboard: the stand-in has no sound, but what the dashboard writes before it starts is the point.
"$HERE/tools/qemu-key.sh" down ret; sleep 3; "$HERE/tools/qemu-key.sh" esc; sleep 14
"$HERE/tools/qemu-key.sh" ret; sleep 10
eq "the stand-in game started" yes "$(g 'pidof sleep >/dev/null && echo yes')"
eq "the choice of headset is kept" "$MAC" "$(g 'cat /data/asound.prefer')"
eq "but asound.conf now names a real sound card, not the absent headset" 0 "$(g 'grep -c type.bluealsa /data/asound.conf')"
has "a real card is in the config" "hw:CARD=" "$(g 'cat /data/asound.conf')"
eq "and the default output opens" 0 "$(g 'aplay -D default -d 1 /dev/zero >/dev/null 2>&1; echo $?')"

echo "== choosing a sound card forgets the headset"
eq "ok" ok "$(g "echo 'sound 1 0' | yargos-home ctl | tail -1")"
eq "the preference is gone" no "$(g '[ -e /data/asound.prefer ] && echo yes || echo no')"
echo "== a stale headset config with nothing chosen is replaced when YARG starts"
g "echo 'sound-bt $MAC' | yargos-home ctl >/dev/null; rm -f /data/asound.prefer" >/dev/null
eq "(set up: the config names the headset, no preference)" 1 "$(g 'grep -c type.bluealsa /data/asound.conf')"
g 'kill $(pidof sleep)' >/dev/null 2>&1; sleep 8     # the stand-in game ends: back on the dashboard
"$HERE/tools/qemu-key.sh" ret; sleep 10
eq "the stale config was replaced by a real sound card" 0 "$(g 'grep -c type.bluealsa /data/asound.conf')"

qstop
echo
[ $fail = 0 ] && echo "ALL PASSED (a radio is needed to try the rest)" || { echo "SOME CHECKS FAILED"; exit 1; }
