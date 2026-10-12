#!/bin/bash
# QEMU test of the system updater's new parts: the reasons it gives when a download fails, the retry that resumes, and
# installing an update from the song drive without internet. It needs the normal image (tools/br.sh) and the signing key
# in ~/.yargos; it does NOT rebuild anything: the "newer system" is the same image with a higher version written into it.
#
#   tools/test-offline-update.sh        about 10 minutes
set -u
HERE=$(cd "$(dirname "$0")/.." && pwd)
BUILD=${YARGOS_BUILD_DIR:-$HOME/yargos-build}
Q=$BUILD/qemu
W=$BUILD/offline-test
PORT=${PORT:-8000}
NEW=2099.1.1
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
until_g() { local end=$((SECONDS + ${2:-180})); while [ $SECONDS -lt $end ]; do g "$1" >/dev/null 2>&1 && return 0; sleep 3; done; return 1; }
serve() { kill $(ps -eo pid,args | awk '/redirect-server.py/ && !/awk/ {print $1}') 2>/dev/null; (python3 "$HERE/tools/redirect-server.py" "$1" "$PORT" >"$W/http.log" 2>&1 &); sleep 1; }

BASEVER=$(debugfs -R "cat /usr/lib/yargos/version" "$BUILD/output/images/rootfs.ext4" 2>/dev/null | tr -d '\n')
echo "== the normal image is version $BASEVER; making a signed $NEW from it"
rm -rf "$W/img" "$W/rel" "$W/songs"; mkdir -p "$W/img" "$W/songs"
cp -L "$BUILD/output/images/rootfs.ext4" "$W/img/rootfs.ext4"; cp "$BUILD/output/images/rootfs.tar" "$W/img/"
echo $NEW > "$W/ver"
debugfs -w -R "rm /usr/lib/yargos/version" "$W/img/rootfs.ext4" >/dev/null 2>&1
debugfs -w -R "write $W/ver /usr/lib/yargos/version" "$W/img/rootfs.ext4" >/dev/null 2>&1
YARGOS_IMAGES="$W/img" "$HERE/tools/release.sh" $NEW --fast --no-legal --notes "offline test" --out "$W/rel" >/dev/null || { echo "release failed"; exit 1; }

# The song drive holds the three release files.
cp "$W/rel/stable.json" "$W/rel/stable.json.sig" "$W/rel/rootfs-$NEW.img.xz" "$W/songs/"
mkfs.ext4 -q -F -L YARGSONGS -d "$W/songs" "$W/songs.img" 400M || exit 1

echo "== starting the guest with the song drive"
qstop
rm -f "$Q/overlay.qcow2" "$Q/OVMF_VARS.fd"
qemu-img create -q -f qcow2 -b "$BUILD/output/images/yargos.img" -F raw "$Q/overlay.qcow2" 64G
(cd "$HERE" && SONGS_IMG="$W/songs.img" CMDLINE="systemd.setenv=YARGOS_YARG_CMD=/bin/true" HEADLESS=1 tools/qemu-run.sh >"$W/qemu.log" 2>&1 &)
until_g 'mountpoint -q /data && [ -d /data/songs ]' 240 || { bad "the guest did not come up"; exit 1; }
sleep 5
g 'mountpoint -q /efi || { mkdir -p /efi; mount /dev/vda1 /efi; }' >/dev/null   # (a kernel booted directly has no loader to mount it)
eq "running the normal image" "$BASEVER" "$(g cat /usr/lib/yargos/version)"
g 'rm -rf /data/os-update /data/yargos.conf.test' >/dev/null

echo "== why a check fails"
g 'echo OS_UPDATE_URL=http://10.0.2.2:1 > /data/yargos.conf' >/dev/null
has "a refused connection is explained" "the connection was refused" "$(g 'yargos-os-update check 2>&1 | tail -1')"
g 'echo OS_UPDATE_URL=http://no-such-host.invalid > /data/yargos.conf' >/dev/null
has "an unknown host is explained" "could not be looked up" "$(g 'yargos-os-update check 2>&1 | tail -1')"
mkdir -p "$W/empty"; serve "$W/empty"
g "echo OS_UPDATE_URL=http://10.0.2.2:$PORT > /data/yargos.conf" >/dev/null
has "a missing file says HTTP 404" "HTTP 404" "$(g 'yargos-os-update check 2>&1 | tail -1')"
has "the screen's status file keeps the reason" "check_why=the update server answered" "$(g 'cat /data/os-update/status')"

echo "== a download that fails says why, after retrying"
serve "$W/rel"; mv "$W/rel/rootfs-$NEW.img.xz" "$W/rel-img.xz"   # the manifest is served, the image is not
out=$(g 'YARGOS_DL_TRIES=2 yargos-os-update install 2>&1 | tail -6')
has "it retried" "try 1 of 2" "$out"
has "the final message says where and why" "the download failed at 0% after 2 tries: the update server answered with an error (HTTP 404)" "$out"
mv "$W/rel-img.xz" "$W/rel/rootfs-$NEW.img.xz"

echo "== installing from the song drive (no internet: the update server is switched off)"
g 'echo OS_UPDATE_URL=http://10.0.2.2:1 > /data/yargos.conf' >/dev/null
g 'ls /media/songs >/dev/null' >/dev/null
eq "the finder sees the update" "offline $NEW" "$(g 'yargos-os-update find')"
eq "the helper word answers" "ok" "$(g 'echo os-find | yargos-home ctl | tail -1')"
g '(yargos-os-update install-offline >/tmp/oi.log 2>&1 &)' >/dev/null
for _ in $(seq 1 60); do sleep 5; g 'tail -1 /tmp/oi.log' | grep -q "installed\|failed\|cannot\|damaged" && break; done
out=$(g 'tail -3 /tmp/oi.log')
has "the install finished" "installed $NEW" "$out"
[ "$(g 'grep -c PROGRESS /tmp/oi.log')" -gt 5 ] && ok "it showed a percentage while copying" || bad "no percentage lines while copying"
eq "an entry with 2 tries was written" "1" "$(g "ls /efi/loader/entries | grep -c 'yargos-$NEW+2'")"
eq "the files on the drive are still there" "3" "$(g 'ls /media/songs | grep -c "stable.json\|rootfs-2099"')"
eq "the status says it is staged" "$NEW" "$(g 'sed -n s/^staged=//p /data/os-update/status')"

echo "== what is refused"
g 'rm -rf /data/os-update; rm -f /efi/loader/entries/yargos-2099*; rm -rf /efi/yargos/2099.1.1' >/dev/null
# (the drive is read-only inside the guest: use copies in a scratch folder with the guest's own override)
g 'mkdir -p /tmp/sd && cp /media/songs/stable.json /media/songs/rootfs-2099.1.1.img.xz /tmp/sd/ && head -c 64 /dev/urandom > /tmp/sd/stable.json.sig' >/dev/null
eq "a manifest that is not signed by us is refused" "offline-bad" "$(g 'YARGOS_SONGS=/tmp/sd yargos-os-update find 2>/dev/null')"
has "  ...and cannot be installed" "not signed by us" "$(g 'YARGOS_SONGS=/tmp/sd yargos-os-update install-offline 2>&1 | tail -1')"
g 'cp /media/songs/stable.json.sig /tmp/sd/ && head -c 1000 /dev/urandom > /tmp/sd/rootfs-2099.1.1.img.xz' >/dev/null
eq "an image of the wrong size is reported" "offline-missing rootfs-$NEW.img.xz" "$(g 'YARGOS_SONGS=/tmp/sd yargos-os-update find 2>/dev/null')"
g 'cp /media/songs/rootfs-2099.1.1.img.xz /tmp/sd/ && dd if=/dev/urandom of=/tmp/sd/rootfs-2099.1.1.img.xz bs=1 seek=5000 count=16 conv=notrunc 2>/dev/null' >/dev/null
has "a damaged image of the right size is refused" "damaged" "$(g 'YARGOS_SONGS=/tmp/sd yargos-os-update install-offline 2>&1 | tail -1')"
eq "  ...and nothing was staged" "0" "$(g "ls /efi/loader/entries | grep -c 'yargos-2099'")"

echo "== the dashboard"
g 'rm -rf /data/os-update' >/dev/null
sleep 2
for i in 1; do "$HERE/tools/qemu-key.sh" down down down down down down ret; sleep 2; "$HERE/tools/qemu-key.sh" down down down down ret; sleep 4; done
"$HERE/tools/qemu-shot.sh" "$W/os-screen.png" >/dev/null 2>&1
"$HERE/tools/qemu-key.sh" down down ret
until_g "sed -n s/^staged=//p /data/os-update/status | grep -q $NEW" 180; r=$?
eq "the dashboard row installed it from the song drive" 0 $r
sleep 4; "$HERE/tools/qemu-shot.sh" "$W/os-after.png" >/dev/null 2>&1

qstop
kill $(ps -eo pid,args | awk '/redirect-server.py/ && !/awk/ {print $1}') 2>/dev/null
echo
[ $fail = 0 ] && echo "ALL PASSED" || { echo "SOME CHECKS FAILED (screenshots in $W)"; exit 1; }
