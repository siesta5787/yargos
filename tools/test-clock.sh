#!/bin/bash
# QEMU test of the corner clock and SYSTEM > Date & time: the clock text, the time zone chosen with key presses on the real
# dashboard (kept in /data/yargos.conf, applied to /etc/localtime by the root helper), the 12/24-hour switch, that a new
# system folder (UTC again) gets the chosen zone back when the dashboard starts, and that the time sync service runs.
# Needs the normal image (tools/br.sh). About 4 minutes.
#
#   tools/test-clock.sh
set -u
HERE=$(cd "$(dirname "$0")/.." && pwd)
BUILD=${YARGOS_BUILD_DIR:-$HOME/yargos-build}
Q=$BUILD/qemu
W=$BUILD/clock-test
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

echo "== starting the guest"
qstop
rm -f "$Q/overlay.qcow2" "$Q/OVMF_VARS.fd"
qemu-img create -q -f qcow2 -b "$BUILD/output/images/yargos.img" -F raw "$Q/overlay.qcow2" 64G
(cd "$HERE" && CMDLINE="systemd.setenv=YARGOS_YARG_CMD=/bin/true" HEADLESS=1 tools/qemu-run.sh >"$W/qemu.log" 2>&1 &)
until_g 'mountpoint -q /data && pidof yargos-home >/dev/null' 240 || { bad "the guest did not come up"; exit 1; }
sleep 8
shot clock-home
eq "the system starts in UTC" Etc/UTC "$(g 'cat /etc/timezone')"
c=$(g 'yargos-home clock')
case $c in [A-Z][a-z][a-z]\ [A-Z][a-z][a-z]\ [0-9]*\ [0-9]*:[0-9][0-9]\ [AP]M) ok "the clock text has the 12-hour form: $c" ;; *) bad "the clock text is odd: '$c'" ;; esac
eq "the clock in the guest matches the guest's own time" "$(g 'date +%H:%M | cut -c1-2')" "$(g 'TZ=UTC date +%H')"
has "the time sync service runs" "active" "$(g 'systemctl is-active systemd-timesyncd')"

echo "== SYSTEM > Date & time > Time zone > Pacific"
k down down down down ret; sleep 2; k down down down ret; sleep 3     # SYSTEM, then Date & time (its 4th row)
shot datetime
k ret; sleep 3; shot timezone; k down down down down ret; sleep 6     # Time zone; the 5th common zone: Pacific
eq "the choice is kept" "TIMEZONE=America/Los_Angeles" "$(g 'grep ^TIMEZONE= /data/yargos.conf')"
eq "the root helper applied it" America/Los_Angeles "$(g 'cat /etc/timezone')"
eq "the link points into the zone folder" /usr/share/zoneinfo/America/Los_Angeles "$(g 'readlink /etc/localtime')"
has "the clock follows the zone" "$(g 'TZ=America/Los_Angeles date +%-I:%M | cut -c1-4')" "$(g 'yargos-home clock')"
shot after-zone

echo "== 24-hour clock"
k down ret; sleep 4
eq "the switch is kept" "CLOCK24=yes" "$(g 'grep ^CLOCK24= /data/yargos.conf')"
case $(g 'yargos-home clock') in *M) bad "still 12-hour" ;; *) ok "the clock text has no AM/PM now" ;; esac
shot clock24
k ret; sleep 3
eq "and back to 12-hour" "CLOCK24=no" "$(g 'grep ^CLOCK24= /data/yargos.conf')"

echo "== a new system folder (UTC again) gets the zone back when the dashboard starts"
g 'ln -sfn /usr/share/zoneinfo/Etc/UTC /etc/localtime; echo Etc/UTC > /etc/timezone; kill $(pidof yargos-home)' >/dev/null
sleep 25
until_g 'pidof yargos-home >/dev/null' 60
sleep 8
eq "the chosen zone is back" America/Los_Angeles "$(g 'cat /etc/timezone')"

echo "== what the helper refuses"
g "echo 'TIMEZONE=../../etc/passwd' > /tmp/bad.conf; sed 's|/data/yargos.conf|/tmp/bad.conf|' /usr/lib/yargos/timezone.sh > /tmp/tz-test.sh; sh /tmp/tz-test.sh" | grep -q "no time zone chosen"; [ $? -eq 0 ] && ok "a path trick is refused" || bad "a path trick was not refused"
g "echo 'TIMEZONE=Nowhere/Land' > /tmp/bad.conf; sh /tmp/tz-test.sh" | grep -q "unknown time zone"; [ $? -eq 0 ] && ok "an unknown zone is refused" || bad "an unknown zone was not refused"
eq "the real zone was not touched by those" America/Los_Angeles "$(g 'cat /etc/timezone')"

qstop
echo
[ $fail = 0 ] && echo "ALL PASSED (look at $W/shots)" || { echo "SOME CHECKS FAILED (screenshots in $W/shots)"; exit 1; }
