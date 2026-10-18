#!/bin/bash
# QEMU test of the change from date-like versions (2026.10.30) to release numbers (1.0, 1.1 ...): the updater must see 1.0 as
# NEWER than a date, the boot loader must really start it (entry naming and sort key, see entries.sh), a release number must
# be newer than a lower release number, going back must work across the two kinds, and a broken 1.0 must fall back to the date
# version. One real build (version 2000.1.1, a test build with a serial console), the other versions are made from it by
# writing a different number into a copy (as tools/test-os-update.sh does for its broken system). About 35 minutes.
#
#   tools/test-version-bridge.sh        (run tools/br.sh afterwards: it leaves a test image in ~/yargos-build/output/images)
set -u
HERE=$(cd "$(dirname "$0")/.." && pwd)
BUILD=${YARGOS_BUILD_DIR:-$HOME/yargos-build}
Q=$BUILD/qemu
W=$BUILD/bridge-test
PORT=${PORT:-8000}
A=2000.1.1
mkdir -p "$W"
fail=0
ok()  { echo "  ok   $*"; }
bad() { echo "  FAIL $*"; fail=1; }
check() { local d=$1; shift; if "$@"; then ok "$d"; else bad "$d"; fi; }
qstop() { kill $(ps -eo pid,args | awk '/qemu-system/ && !/awk/ {print $1}') 2>/dev/null; sleep 2; }
g() {
    local out
    out=$("$HERE/tools/qemu-sh.sh" "echo @@B; $*; echo @@E" 2>/dev/null | tr -d '\r')
    [ "$(grep -c '@@E$' <<< "$out")" -ge 1 ] || return 1
    sed -n '/@@B$/,/@@E$/p' <<< "$out" | sed '1d;$d'
}
qstart() {
    qstop
    (cd "$HERE" && HEADLESS=1 tools/qemu-run.sh >"$W/qemu.log" 2>&1 &)
    for _ in $(seq 1 80); do sleep 3; g true >/dev/null 2>&1 && return 0; done
    return 1
}
wait_version() {
    local end=$((SECONDS + ${2:-300}))
    while [ $SECONDS -lt $end ]; do
        [ "$(g cat /usr/lib/yargos/version 2>/dev/null)" = "$1" ] && { sleep 45; return 0; }
        sleep 5
    done
    return 1
}
reboot_guest() { "$HERE/tools/qemu-sh.sh" 'sync; (sleep 1; reboot) >/dev/null 2>&1 &' >/dev/null 2>&1; sleep 15; }
newoverlay() { rm -f "$Q/overlay.qcow2"; qemu-img create -q -f qcow2 -b "$1" -F raw "$Q/overlay.qcow2" 64G; }
serve() { kill $(ps -eo pid,args | awk '/redirect-server.py/ && !/awk/ {print $1}') 2>/dev/null; (python3 "$HERE/tools/redirect-server.py" "$1" "$PORT" >"$W/http.log" 2>&1 &); sleep 1; }
install_update() {
    g '(yargos-os-update install >/tmp/ou.log 2>&1 &)' >/dev/null
    for _ in $(seq 1 60); do sleep 5; g 'tail -1 /tmp/ou.log' | grep -q "installed\|failed\|cannot\|damaged" && break; done
}
# variant VERSION DIR [broken]: a copy of the built system that says it is VERSION, signed as a release
variant() {
    local v=$1 d=$2
    rm -rf "$d" "$d-rel"; mkdir -p "$d"
    cp -L "$BUILD/output/images/rootfs.ext4" "$d/rootfs.ext4"; cp "$BUILD/output/images/rootfs.tar" "$d/"
    echo "$v" > "$d/ver"
    debugfs -w -R "rm /usr/lib/yargos/version" "$d/rootfs.ext4" >/dev/null 2>&1
    debugfs -w -R "write $d/ver /usr/lib/yargos/version" "$d/rootfs.ext4" >/dev/null 2>&1
    [ "${3:-}" = broken ] && debugfs -w -R "rm /usr/bin/yargos-home" "$d/rootfs.ext4" >/dev/null 2>&1
    YARGOS_IMAGES="$d" "$HERE/tools/release.sh" "$v" --fast --no-legal --notes "$v" --out "$d-rel" >/dev/null || { echo "release $v failed"; exit 1; }
}

echo "== building $A (the one real build)"
rm -f "$BUILD"/output/images/{rootfs.ext2,rootfs.ext4,yargos.img}
(cd "$HERE" && YARGOS_TEST_CONSOLE=1 YARGOS_VERSION=$A tools/br.sh >"$W/build.log" 2>&1) || { echo "build failed"; exit 1; }
cp --reflink=auto "$BUILD/output/images/yargos.img" "$W/a.img"
variant 1.0 "$W/v10"; variant 1.1 "$W/v11"; variant 1.0 "$W/v10bad" broken

echo "== a good update from a date to a release number: $A -> 1.0"
serve "$W/v10-rel"
newoverlay "$W/a.img"
qstart || { bad "VM did not start"; exit 1; }
g 'echo "OS_UPDATE_URL=http://10.0.2.2:'$PORT'" >> /data/yargos.conf' >/dev/null
check "running $A" [ "$(g cat /usr/lib/yargos/version)" = $A ]
out=$(g 'yargos-os-update check'); check "the check finds 1.0 as newer than $A" [ "$(grep -c "update available: 1.0" <<< "$out")" = 1 ]
install_update
check "the install finished" [ "$(g 'tail -1 /tmp/ou.log' | grep -c installed)" = 1 ]
check "the entry is yargos-2-1.0+2 (the name the default pattern matches)" [ "$(g 'ls /efi/loader/entries | grep -c "^yargos-2-1.0+2.conf"')" = 1 ]
check "it sorts before the date entries" [ "$(g 'sed -n s/^sort-key[[:space:]]*//p /efi/loader/entries/yargos-2-1.0+2.conf')" = yargos-9 ]
check "the date entry is still there" [ "$(g 'ls /efi/loader/entries | grep -c "^yargos-'$A'.conf"')" = 1 ]
reboot_guest; wait_version 1.0 400 || { bad "1.0 did not come up: the boot loader kept the date version?"; exit 1; }
check "now running 1.0" [ "$(g cat /usr/lib/yargos/version)" = 1.0 ]
check "marked good (the counter is gone)" [ "$(g 'ls /efi/loader/entries | grep -c "^yargos-2-1.0.conf"')" = 1 ]
check "the books are closed (updated=1.0)" [ "$(g 'sed -n s/^updated=//p /data/os-update/status')" = 1.0 ]
g 'yargos-os-update check' | grep -q "up to date"; check "1.0 is up to date" [ $? -eq 0 ]
check "the second slot is the one in use" [ "$(g 'cat /proc/cmdline | grep -c 0003')" = 1 ]

echo "== a release number is newer than a lower one: 1.0 -> 1.1"
serve "$W/v11-rel"
g 'rm -f /data/os-update/rejected' >/dev/null
out=$(g 'yargos-os-update check'); check "the check finds 1.1" [ "$(grep -c "update available: 1.1" <<< "$out")" = 1 ]
install_update
check "an entry yargos-2-1.1+2 was written" [ "$(g 'ls /efi/loader/entries | grep -c "^yargos-2-1.1+2.conf"')" = 1 ]
check "the entry of the date system in the way is gone" [ "$(g 'ls /efi/loader/entries | grep -c "^yargos-'$A'"')" = 0 ]
reboot_guest; wait_version 1.1 400 || { bad "1.1 did not come up: 1.0 and 1.1 in the wrong order?"; exit 1; }
check "now running 1.1" [ "$(g cat /usr/lib/yargos/version)" = 1.1 ]

echo "== going back: 1.1 -> 1.0"
g 'yargos-os-update go-back' >/dev/null
check "this system is marked not to start (yargos-2-1.1+0)" [ "$(g 'ls /efi/loader/entries | grep -c "^yargos-2-1.1+0"')" = 1 ]
reboot_guest; wait_version 1.0 400 || { bad "1.0 did not come back"; exit 1; }
check "back on 1.0" [ "$(g cat /usr/lib/yargos/version)" = 1.0 ]

echo "== a broken 1.0 must fall back to the date version"
serve "$W/v10bad-rel"
newoverlay "$W/a.img"
qstart || { bad "VM did not start"; exit 1; }
g 'echo "OS_UPDATE_URL=http://10.0.2.2:'$PORT'" >> /data/yargos.conf' >/dev/null
install_update
check "the broken 1.0 was installed on trial" [ "$(g 'ls /efi/loader/entries | grep -c "^yargos-2-1.0+2"')" = 1 ]
reboot_guest
echo "   (two failed tries of ~2.5 minutes each, then the old system)"
wait_version $A 900 || { bad "the date system did not come back"; exit 1; }
check "back on $A" [ "$(g cat /usr/lib/yargos/version)" = $A ]
check "the failed entry is marked bad" [ "$(g 'ls /efi/loader/entries | grep -c "^yargos-2-1.0+0"')" = 1 ]
check "the rollback is recorded" [ "$(g 'sed -n s/^rolled_back=//p /data/os-update/status')" = 1.0 ]

qstop
kill $(ps -eo pid,args | awk '/redirect-server.py/ && !/awk/ {print $1}') 2>/dev/null
echo
[ $fail = 0 ] && echo "ALL PASSED (run tools/br.sh for a normal image)" || { echo "SOME CHECKS FAILED"; exit 1; }
