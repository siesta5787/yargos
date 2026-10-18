#!/bin/bash
# QEMU test of SYSTEM > Storage > Import venues from the song drive: a song drive (label YARGSONGS) with a folder called
# VENUES holds venue files (one nested like the YARC Launcher lays them out), a text file and a link to /etc/passwd. The test
# drives the real dashboard with key presses: the question, the copy into /data/yarg/venue (owned by the player, the link and
# the text file left behind), a second scan finding nothing new, and removing everything again. Needs the normal image
# (tools/br.sh). About 4 minutes.
#
#   tools/test-venues.sh
set -u
HERE=$(cd "$(dirname "$0")/.." && pwd)
BUILD=${YARGOS_BUILD_DIR:-$HOME/yargos-build}
Q=$BUILD/qemu
W=$BUILD/venues-test
mkdir -p "$W/shots" "$W/drive/VENUES/abc-123/installation"
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
k() { "$HERE/tools/qemu-key.sh" "$@"; }
shot() { "$HERE/tools/qemu-shot.sh" "$W/shots/$1.png" >/dev/null 2>&1; }

rm -rf "$W/drive"; mkdir -p "$W/drive/VENUES/abc-123/installation" "$W/drive/Rock/Band - Song"
head -c 200000 /dev/urandom > "$W/drive/VENUES/Outdoor.yarground"
head -c 5000 /dev/urandom > "$W/drive/VENUES/abc-123/installation/Club.yarground"
echo "not a venue" > "$W/drive/VENUES/readme.txt"
ln -s /etc/passwd "$W/drive/VENUES/evil.yarground"
head -c 1000 /dev/urandom > "$W/drive/Rock/Band - Song/song.opus"; echo "name = x" > "$W/drive/Rock/Band - Song/song.ini"
mkfs.ext4 -q -F -L YARGSONGS -d "$W/drive" "$W/drive.img" 64M || exit 1

echo "== starting the guest with the song drive"
qstop
rm -f "$Q/overlay.qcow2" "$Q/OVMF_VARS.fd"
qemu-img create -q -f qcow2 -b "$BUILD/output/images/yargos.img" -F raw "$Q/overlay.qcow2" 64G
(cd "$HERE" && SONGS_IMG="$W/drive.img" CMDLINE="systemd.setenv=YARGOS_YARG_CMD=/bin/true" HEADLESS=1 tools/qemu-run.sh >"$W/qemu.log" 2>&1 &)
until_g 'mountpoint -q /data && mountpoint -q /media/songs && pidof yargos-home >/dev/null' 240 || { bad "the guest did not come up"; exit 1; }
sleep 8
eq "YARG's data folder belongs to the player" player "$(g 'ls -ld /data/yarg | awk "{print \$3}"')"
eq "no venues yet" 0 "$(g 'ls /data/yarg/venue 2>/dev/null | wc -l')"

echo "== SYSTEM > Storage > Import venues from the song drive"
k down down down down ret; sleep 2; k down down ret; sleep 4        # SYSTEM, then Storage
k down down down ret; sleep 5                                        # the 4th row: venues
shot question
k right; sleep 1; k ret; sleep 6
eq "the venue folder has the two venue files" 2 "$(g 'find /data/yarg/venue -type f | wc -l')"
eq "the nested one keeps its folder" yes "$(g '[ -f /data/yarg/venue/abc-123/installation/Club.yarground ] && echo yes')"
eq "the link was not copied" 0 "$(g 'find /data/yarg/venue -name "evil*" | wc -l')"
eq "the text file was not copied" 0 "$(g 'find /data/yarg/venue -name "*.txt" | wc -l')"
eq "the files belong to the player" 0 "$(g 'find /data/yarg/venue ! -user player | wc -l')"
eq "a copy is byte for byte" "$(cmp -s "$W/drive/VENUES/Outdoor.yarground" /dev/null; sha256sum < "$W/drive/VENUES/Outdoor.yarground")" "$(g 'sha256sum < /data/yarg/venue/Outdoor.yarground')"
eq "no half-copied file is left" 0 "$(g 'find /data/yarg/venue -name "*.yargos-part" | wc -l')"
shot after

echo "== again: nothing new"
sleep 3; k esc; sleep 3; k ret; sleep 4; k down down down ret; sleep 5
shot again
eq "still two files" 2 "$(g 'find /data/yarg/venue -type f | wc -l')"

echo "== Remove added venues"
sleep 3
k esc; sleep 3; k ret; sleep 4; k down down down down ret; sleep 4
shot remove-question
k right; sleep 1; k ret; sleep 5
eq "everything was removed" 0 "$(g 'find /data/yarg/venue -type f | wc -l')"
eq "the folder itself stays" yes "$(g '[ -d /data/yarg/venue ] && echo yes')"
eq "the real file the link pointed to is untouched" yes "$(g '[ -s /etc/passwd ] && echo yes')"

qstop
echo
[ $fail = 0 ] && echo "ALL PASSED (look at $W/shots: question, after, again, remove-question)" || { echo "SOME CHECKS FAILED (screenshots in $W/shots)"; exit 1; }
