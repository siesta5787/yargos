#!/bin/bash
# QEMU test: the System updates screen looks at the song drive every few seconds (`yargos-os-update find`) while an update
# check or install fetches the update information from the server. Both used the same temporary file names, so a check could
# end up with the manifest of one and the signature of the other and say "not signed by us" for a release that is fine. This
# runs `find` in a loop while checks run and counts the wrong "not signed" answers (there must be none). Hand-made signed
# manifests only (no image is built or served). Needs the normal image and the signing key. About 4 minutes.
#
#   tools/test-update-race.sh
set -u
HERE=$(cd "$(dirname "$0")/.." && pwd)
BUILD=${YARGOS_BUILD_DIR:-$HOME/yargos-build}
Q=$BUILD/qemu
W=$BUILD/race-test
PORT=${PORT:-8000}
KEY=${YARGOS_KEY:-$HOME/.yargos/release-ed25519.pem}
fail=0
ok()  { echo "  ok   $*"; }
bad() { echo "  FAIL $*"; fail=1; }
qstop() { kill $(ps -eo pid,args | awk '/qemu-system/ && !/awk/ {print $1}') 2>/dev/null; sleep 2; }
g() {
    local out
    out=$("$HERE/tools/qemu-sh.sh" "echo @@B; $*; echo @@E" 2>/dev/null | tr -d '\r')
    [ "$(grep -c '@@E$' <<< "$out")" -ge 1 ] || return 1
    sed -n '/@@B$/,/@@E$/p' <<< "$out" | sed '1d;$d'
}
until_g() { local end=$((SECONDS + ${2:-120})); while [ $SECONDS -lt $end ]; do g "$1" >/dev/null 2>&1 && return 0; sleep 2; done; return 1; }
manifest() { # manifest VERSION OUTDIR: a signed manifest (the image it names does not exist: only the manifest is looked at)
    mkdir -p "$2"
    jq -n --arg v "$1" '{version: $v, notes: "race test", image: {file: ("rootfs-" + $v + ".img.xz"), size: 123456, sha256: ("ab" * 32), raw_size: 654321, raw_sha256: ("cd" * 32)}}' > "$2/stable.json"
    openssl pkeyutl -sign -inkey "$KEY" -rawin -in "$2/stable.json" -out "$2/stable.json.sig" || { echo "cannot sign"; exit 1; }
}
[ -f "$KEY" ] || { echo "no signing key $KEY"; exit 1; }
rm -rf "$W"; mkdir -p "$W/drive" "$W/online"
manifest 2099.1.1 "$W/drive"        # on the song drive
manifest 2098.1.1 "$W/online"       # on the server (a different, equally good pair)
mkfs.ext4 -q -F -L YARGSONGS -d "$W/drive" "$W/songs.img" 64M || exit 1
kill $(ps -eo pid,args | awk '/redirect-server.py/ && !/awk/ {print $1}') 2>/dev/null
(python3 "$HERE/tools/redirect-server.py" "$W/online" "$PORT" >"$W/http.log" 2>&1 &); sleep 1

echo "== starting the guest with the song drive"
qstop
rm -f "$Q/overlay.qcow2" "$Q/OVMF_VARS.fd"
qemu-img create -q -f qcow2 -b "$BUILD/output/images/yargos.img" -F raw "$Q/overlay.qcow2" 64G
(cd "$HERE" && SONGS_IMG="$W/songs.img" CMDLINE="systemd.setenv=YARGOS_YARG_CMD=/bin/true" HEADLESS=1 tools/qemu-run.sh >"$W/qemu.log" 2>&1 &)
until_g 'mountpoint -q /data && mountpoint -q /media/songs' 240 || { bad "the guest did not come up"; exit 1; }
sleep 5
g "echo OS_UPDATE_URL=http://10.0.2.2:$PORT > /data/yargos.conf; rm -rf /data/os-update" >/dev/null

echo "== the song drive is looked at over and over while the server is asked 40 times"
res=$(g 'YARGOS_SIG_TRIES=1; export YARGOS_SIG_TRIES; (while :; do yargos-os-update find >/dev/null 2>&1; done) & lp=$!; n=0; w=0; for i in $(seq 1 40); do out=$(yargos-os-update check 2>&1); case $out in *"up to date"*|*"update available"*) ;; *) w=$((w+1)); echo "$out" | tail -1 >> /tmp/race.log ;; esac; n=$((n+1)); done; kill $lp 2>/dev/null; echo "checks=$n wrong=$w"')
echo "   $res"
case $res in
    *"checks=40 wrong=0"*) ok "no check was wrongly refused while the drive was being looked at" ;;
    *) bad "checks were wrongly refused: $res ($(g 'sort /tmp/race.log | uniq -c | head -3' | tr '\n' ' '))" ;;
esac
qstop
kill $(ps -eo pid,args | awk '/redirect-server.py/ && !/awk/ {print $1}') 2>/dev/null
echo
[ $fail = 0 ] && echo "ALL PASSED" || { echo "SOME CHECKS FAILED"; exit 1; }
