#!/bin/bash
# End-to-end test of the system updater in QEMU: builds three systems, serves signed releases from this PC and walks a
# virtual computer through updates, a bad update, a power cut and going back. Takes ~30 minutes. Needs a finished
# build in ~/yargos-build (it rebuilds the image three times with other versions and leaves the last one in place:
# run `tools/br.sh` afterwards to get a normal image back).
#
#   tools/test-os-update.sh            everything
#   STEPS="build good" tools/test-os-update.sh     only some steps: build good bad cut back nvme
#   (build once; the other steps reuse what it made in ~/yargos-build/os-test)
set -u
HERE=$(cd "$(dirname "$0")/.." && pwd)
BUILD=${YARGOS_BUILD_DIR:-$HOME/yargos-build}
Q=$BUILD/qemu
W=$BUILD/os-test
PORT=${PORT:-8000}
STEPS=${STEPS:-build good bad cut back nvme}
V1=2000.1.1; V2=2000.1.2; V3=2000.1.3; V4=2000.1.4
mkdir -p "$W"
fail=0
ok()  { echo "  ok   $*"; }
bad() { echo "  FAIL $*"; fail=1; }
check() { local d=$1; shift; if "$@"; then ok "$d"; else bad "$d"; fi; }
want() { case " $STEPS " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }

qstop() { kill $(ps -eo pid,args | awk '/qemu-system/ && !/awk/ {print $1}') 2>/dev/null; sleep 2; }
# qstart: start the VM on the current overlay and wait for a shell. The normal boot entry has a serial console in these
# test builds (YARGOS_TEST_CONSOLE), so the boot menu is never touched.
qstart() {
    qstop
    (cd "$HERE" && HEADLESS=1 tools/qemu-run.sh >"$W/qemu.log" 2>&1 &)
    for _ in $(seq 1 80); do sleep 3; g true >/dev/null 2>&1 && return 0; done
    return 1
}
# wait_version V [SECONDS]: wait until the shell reports that version (a system on trial that fails never does), then
# give the boot a moment to be judged and the books to be closed.
wait_version() {
    local end=$((SECONDS + ${2:-300}))
    while [ $SECONDS -lt $end ]; do
        [ "$(g cat /usr/lib/yargos/version 2>/dev/null)" = "$1" ] && { sleep 45; return 0; }
        sleep 5
    done
    return 1
}
# g CMD: run in the guest, print what CMD printed (fails if the guest did not answer: the console puts stray characters
# before lines, so the markers are matched at the end of the line).
g() {
    local out
    out=$("$HERE/tools/qemu-sh.sh" "echo @@B; $*; echo @@E" 2>/dev/null | tr -d '\r')
    [ "$(grep -c '@@E$' <<< "$out")" -ge 1 ] || return 1
    sed -n '/@@B$/,/@@E$/p' <<< "$out" | sed '1d;$d'
}
reboot_guest() { "$HERE/tools/qemu-sh.sh" 'sync; (sleep 1; reboot) >/dev/null 2>&1 &' >/dev/null 2>&1; sleep 15; }
newoverlay() { rm -f "$Q/overlay.qcow2"; qemu-img create -q -f qcow2 -b "$1" -F raw "$Q/overlay.qcow2" 64G; }
serve() { kill $(ps -eo pid,args | awk '/http.server '$PORT'/ && !/awk/ {print $1}') 2>/dev/null; (cd "$1" && python3 -m http.server "$PORT" >"$W/http.log" 2>&1 &); sleep 1; }
build() { # build VERSION
    rm -f "$BUILD"/output/images/{rootfs.ext2,rootfs.ext4,yargos.img}
    (cd "$HERE" && YARGOS_TEST_CONSOLE=1 YARGOS_VERSION=$1 tools/br.sh >"$W/build-$1.log" 2>&1) || { echo "build $1 failed"; exit 1; }
}

if want build; then
    echo "== building $V1, $V2, $V4 and a broken $V3"
    build $V1; cp --reflink=auto "$BUILD/output/images/yargos.img" "$W/a.img"
    build $V4; "$HERE/tools/release.sh" $V4 --fast --notes "four" --out "$W/rel4" >/dev/null || exit 1
    build $V2; "$HERE/tools/release.sh" $V2 --fast --notes "two" --out "$W/rel2" >/dev/null || exit 1
    # The broken one: the good one with the launcher removed (it boots, but never shows a dashboard).
    mkdir -p "$W/img3"; cp -L "$BUILD/output/images/rootfs.ext4" "$W/img3/rootfs.ext4"; cp "$BUILD/output/images/rootfs.tar" "$W/img3/"
    echo $V3 > "$W/ver3"
    debugfs -w -R "rm /usr/bin/yargos-home" "$W/img3/rootfs.ext4" >/dev/null 2>&1
    debugfs -w -R "rm /usr/lib/yargos/version" "$W/img3/rootfs.ext4" >/dev/null 2>&1
    debugfs -w -R "write $W/ver3 /usr/lib/yargos/version" "$W/img3/rootfs.ext4" >/dev/null 2>&1
    YARGOS_IMAGES="$W/img3" "$HERE/tools/release.sh" $V3 --fast --notes "broken" --out "$W/rel3" >/dev/null || exit 1
fi

if want good; then
    echo "== a good update: $V1 -> $V2"
    serve "$W/rel2"
    newoverlay "$W/a.img"
    qstart || { bad "VM did not start"; exit 1; }
    g 'echo "OS_UPDATE_URL=http://10.0.2.2:'$PORT'" >> /data/yargos.conf' >/dev/null
    check "running $V1" [ "$(g cat /usr/lib/yargos/version)" = $V1 ]
    out=$(g 'yargos-os-update check'); check "the check finds $V2" [ "$(grep -c "update available: $V2" <<< "$out")" = 1 ]
    g '(yargos-os-update install >/tmp/ou.log 2>&1 &)' >/dev/null
    for _ in $(seq 1 60); do sleep 5; g 'tail -1 /tmp/ou.log' | grep -q "installed\|failed\|cannot\|damaged" && break; done
    out=$(g 'tail -1 /tmp/ou.log'); check "the install finished" [ "$(grep -c installed <<< "$out")" = 1 ]
    check "an entry with 2 tries was written" [ "$(g 'ls /efi/loader/entries | grep -c "yargos-'$V2'+2"')" = 1 ]
    check "the old entry is still there" [ "$(g 'ls /efi/loader/entries | grep -c "yargos-'$V1'.conf"')" = 1 ]
    # Restart into the default (the new system, on trial), give it time, then look from the troubleshooting entry.
    reboot_guest; wait_version $V2 400 || { bad "$V2 did not come up"; exit 1; }
    check "now running $V2" [ "$(g cat /usr/lib/yargos/version)" = $V2 ]
    check "the new system is marked good (entry has no counter)" [ "$(g 'ls /efi/loader/entries | grep -c "yargos-'$V2'.conf"')" = 1 ]
    check "the books are closed (updated=$V2)" [ "$(g 'sed -n s/^updated=//p /data/os-update/status')" = $V2 ]
    check "the second slot is the one in use" [ "$(g 'cat /proc/cmdline | grep -c 0003')" = 1 ]
fi

if want bad; then
    echo "== a bad update: $V2 -> $V3 (no launcher): must fall back"
    serve "$W/rel3"
    g '(yargos-os-update install >/tmp/ou.log 2>&1 &)' >/dev/null
    for _ in $(seq 1 60); do sleep 5; g 'tail -1 /tmp/ou.log' | grep -q "installed\|failed\|cannot\|damaged" && break; done
    check "$V3 was installed on trial" [ "$(g 'ls /efi/loader/entries | grep -c "yargos-'$V3'+2"')" = 1 ]
    check "the entry of the system that was in the way is gone" [ "$(g 'ls /efi/loader/entries | grep -c "yargos-'$V1'"')" = 0 ]
    reboot_guest
    echo "   (two failed tries of ~2.5 minutes each, then the old system)"
    wait_version $V2 900 || { bad "the old system did not come back"; exit 1; }
    check "back on $V2" [ "$(g cat /usr/lib/yargos/version)" = $V2 ]
    check "the failed entry is marked bad" [ "$(g 'ls /efi/loader/entries | grep -c "yargos-'$V3'+0"')" = 1 ]
    check "the rollback is recorded" [ "$(g 'sed -n s/^rolled_back=//p /data/os-update/status')" = $V3 ]
    check "$V3 is on the rejected list" [ "$(g 'grep -cx '$V3' /data/os-update/rejected')" = 1 ]
    g 'yargos-os-update check' | grep -q "failed to start here before" ; check "it is not offered again" [ $? -eq 0 ]
    g 'yargos-os-update ack' >/dev/null
    check "the notice can be dismissed" [ -z "$(g 'sed -n s/^rolled_back=//p /data/os-update/status')" ]
fi

if want cut; then
    echo "== a power cut while writing: $V2 -> $V4"
    serve "$W/rel4"
    g 'rm -f /data/os-update/rejected' >/dev/null
    g '(yargos-os-update install >/tmp/ou.log 2>&1 &)' >/dev/null
    for _ in $(seq 1 100); do sleep 2; [ "$(g 'grep -c "STEP writing" /tmp/ou.log')" = 1 ] && break; done
    sleep 8
    qstop
    qstart || { bad "VM did not come back after the power cut"; exit 1; }
    check "still running $V2" [ "$(g cat /usr/lib/yargos/version)" = $V2 ]
    check "no entry points at the half-written system" [ "$(g 'ls /efi/loader/entries | grep -c "yargos-2000.*+"')" = 0 ]
    g 'yargos-os-update status' >/dev/null
    g '(yargos-os-update install >/tmp/ou.log 2>&1 &)' >/dev/null
    for _ in $(seq 1 100); do sleep 5; g 'tail -1 /tmp/ou.log' | grep -q "installed\|failed\|cannot\|damaged" && break; done
    check "a second try installs $V4" [ "$(g 'ls /efi/loader/entries | grep -c "yargos-'$V4'+2"')" = 1 ]
    reboot_guest; wait_version $V4 400 || { bad "$V4 did not come up"; exit 1; }
    check "now running $V4" [ "$(g cat /usr/lib/yargos/version)" = $V4 ]
fi

if want back; then
    echo "== going back: $V4 -> $V2"
    g 'yargos-os-update go-back' >/dev/null
    check "this system is marked not to start" [ "$(g 'ls /efi/loader/entries | grep -c "yargos-'$V4'+0"')" = 1 ]
    reboot_guest; wait_version $V2 400 || { bad "$V2 did not come back"; exit 1; }
    check "back on $V2" [ "$(g cat /usr/lib/yargos/version)" = $V2 ]
fi

if want nvme; then
    echo "== installing to a second disk, starting from it alone, and updating it: $V1 -> $V2"
    serve "$W/rel2"
    qstop
    rm -f "$Q/OVMF_VARS.fd" "$Q/nvme.img"
    newoverlay "$W/a.img"
    (cd "$HERE" && NVME_IMG="$Q/nvme.img" HEADLESS=1 tools/qemu-run.sh >"$W/qemu.log" 2>&1 &)
    for _ in $(seq 1 80); do sleep 3; g true >/dev/null 2>&1 && break; done
    g '(yargos-install run /dev/nvme0n1 >/tmp/inst.log 2>&1 &)' >/dev/null
    for _ in $(seq 1 80); do sleep 5; g 'tail -1 /tmp/inst.log' | grep -q "DONE\|ERROR" && break; done
    check "the installer finished" [ "$(g 'tail -1 /tmp/inst.log')" = DONE ]
    check "it made the new layout (A and B slots)" [ "$(g 'lsblk -no PARTLABEL /dev/nvme0n1 | grep -c "yargos-[ab]"')" = 2 ]
    qstop
    local_work=$Q/nvme-work; rm -rf "$local_work"
    (cd "$HERE" && WORK=$local_work BOOT_NVME=1 IMG="$Q/nvme.img" HEADLESS=1 tools/qemu-run.sh fresh >"$W/qemu.log" 2>&1 &)
    export WORK=$local_work
    for _ in $(seq 1 80); do sleep 3; g true >/dev/null 2>&1 && break; done
    check "the installed disk starts alone as $V1" [ "$(g cat /usr/lib/yargos/version)" = $V1 ]
    g 'echo "OS_UPDATE_URL=http://10.0.2.2:'$PORT'" >> /data/yargos.conf' >/dev/null
    g '(yargos-os-update install >/tmp/ou.log 2>&1 &)' >/dev/null
    for _ in $(seq 1 100); do sleep 5; g 'tail -1 /tmp/ou.log' | grep -q "installed\|failed\|cannot\|damaged" && break; done
    check "the update went to the other slot of the installed disk" [ "$(g 'ls /efi/loader/entries | grep -c "yargos-'$V2'+2"')" = 1 ]
    reboot_guest; wait_version $V2 400 || { bad "$V2 did not come up on the installed disk"; exit 1; }
    check "the installed disk now runs $V2" [ "$(g cat /usr/lib/yargos/version)" = $V2 ]
    check "and it was marked good" [ "$(g 'sed -n s/^updated=//p /data/os-update/status')" = $V2 ]
    unset WORK
fi

qstop
kill $(ps -eo pid,args | awk '/http.server '$PORT'/ && !/awk/ {print $1}') 2>/dev/null
[ $fail = 0 ] && echo "ALL PASSED" || { echo "SOME FAILED"; exit 1; }
