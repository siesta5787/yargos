#!/bin/bash
# End-to-end test of SYSTEM > Storage in QEMU: three fake USB drives (ext4 images made without root) are attached, then
# the real dashboard is driven with key presses to import them, and the result is checked from the guest's shell: the
# folders copied exactly, housekeeping and links left out, the owner, a second drive with the same name getting its own
# folder, an import repeated, song-sources and YARG's settings, then deleting a folder, a song and everything.
# Needs a finished image in ~/yargos-build (the normal one: no test console needed). Takes about 10 minutes.
#
#   tools/test-songs.sh
set -u
HERE=$(cd "$(dirname "$0")/.." && pwd)
BUILD=${YARGOS_BUILD_DIR:-$HOME/yargos-build}
Q=$BUILD/qemu
W=$BUILD/songs-test
mkdir -p "$W/shots"
fail=0
ok()  { echo "  ok   $*"; }
bad() { echo "  FAIL $*"; fail=1; }
check() { local d=$1; shift; if "$@"; then ok "$d"; else bad "$d"; fi; }
# eq DESCRIPTION EXPECTED ACTUAL: a failure shows both
eq() { if [ "$2" = "$3" ]; then ok "$1"; else bad "$1 (expected '$2', got '$3')"; fi; }
# has PATH / hasnt PATH: does it exist in the guest
has() { [ "$(g "[ -e '$1' ] && echo yes")" = yes ]; }
hasnt() { [ "$(g "[ -e '$1' ] || [ -L '$1' ] && echo yes")" != yes ]; }
owner() { g "find '$1' -maxdepth 0 -user player | wc -l"; }
names() { g "ls -1A '$1'" | sort | tr '\n' '|'; }

qstop() { kill $(ps -eo pid,args | awk '/qemu-system/ && !/awk/ {print $1}') 2>/dev/null; sleep 2; }
g() {
    local out
    out=$("$HERE/tools/qemu-sh.sh" "echo @@B; $*; echo @@E" 2>/dev/null | tr -d '\r')
    [ "$(grep -c '@@E$' <<< "$out")" -ge 1 ] || return 1
    sed -n '/@@B$/,/@@E$/p' <<< "$out" | sed '1d;$d'
}
k() { "$HERE/tools/qemu-key.sh" "$@"; }
shot() { "$HERE/tools/qemu-shot.sh" "$W/shots/$1.png" >/dev/null 2>&1; }
# until CMD...: wait up to 120 s for the guest command to succeed
until_g() { local end=$((SECONDS + 120)); while [ $SECONDS -lt $end ]; do g "$*" >/dev/null 2>&1 && return 0; sleep 3; done; return 1; }

# ---- the drives ----
rm -rf "$W/a" "$W/b" "$W/c"; mkdir -p "$W/a" "$W/b" "$W/c"
song() { mkdir -p "$1/$2"; printf 'name = %s\n' "$3" > "$1/$2/song.ini"; head -c "${4:-300000}" /dev/urandom > "$1/$2/song.opus"; }
song "$W/a" "Rock/AC-DC - Back in Black" "Back in Black"
song "$W/a" "Rock/Aerosmith - Walk This Way" "Walk This Way" 2500000
song "$W/a" "Pop/Abba - Dancing Queen" "Dancing Queen"
mkdir -p "$W/a/\$RECYCLE.BIN" "$W/a/System Volume Information"; echo x > "$W/a/\$RECYCLE.BIN/junk"
ln -s /etc "$W/a/Rock/link"
echo hi > "$W/a/Rock/readme.txt"
song "$W/b" "Songs/Queen - Radio Gaga" "Radio Gaga"
song "$W/c" "Other/Muse - Hysteria" "Hysteria"
mkfs.ext4 -q -F -L MYSONGS -d "$W/a" "$W/a.run" 64M && mkfs.ext4 -q -F -L YARGSONGS -d "$W/b" "$W/b.run" 32M && mkfs.ext4 -q -F -L MYSONGS -d "$W/c" "$W/c.run" 32M || exit 1

# ---- boot ----
echo "== starting the guest with three USB drives"
qstop
rm -f "$Q/overlay.qcow2" "$Q/OVMF_VARS.fd"
qemu-img create -q -f qcow2 -b "$BUILD/output/images/yargos.img" -F raw "$Q/overlay.qcow2" 64G
(cd "$HERE" && SONGS_IMG="$W/a.run $W/b.run $W/c.run" CMDLINE="systemd.setenv=YARGOS_YARG_CMD=/bin/true" HEADLESS=1 tools/qemu-run.sh >"$W/qemu.log" 2>&1 &)
until_g 'mountpoint -q /data && [ -d /data/songs ]' || { bad "the guest did not come up"; exit 1; }
sleep 8   # the dashboard
eq "the song library belongs to the player" 1 "$(owner /data/songs)"
check "three portable drives are offered" [ "$(g 'yargos-songs list | wc -l')" = 3 ]
check "the system's own disk is not offered" [ "$(g 'yargos-songs list | grep -c "/dev/vd"')" = 0 ]
out=$(g 'echo "songs-mount /dev/vda2" | yargos-home ctl'); check "the helper refuses the system disk" [ "$(grep -c '^error' <<< "$out")" = 1 ]
out=$(g 'echo "songs-mount /dev/sda" | yargos-home ctl'); check "the helper mounts a listed drive" [ "$(grep -c '^ok' <<< "$out")" = 1 ]
check "  ...read-only" [ "$(g 'grep -c "/run/yargos-import/mnt ext4 ro,nosuid,nodev,noexec" /proc/mounts')" = 1 ]
g 'echo songs-umount | yargos-home ctl' >/dev/null
check "  ...and unmounts it again" [ "$(g 'mountpoint -q /run/yargos-import/mnt && echo mounted')" != mounted ]

# ---- import, with the dashboard ----
to_storage() { k down down down down ret; sleep 2; k down down ret; sleep 3; }   # (SYSTEM is the 5th main row, Storage its 3rd)
import_drive() { # import_drive N: the N-th drive of the list, then wait for the dialog and agree
    k ret; sleep 4; k $(printf 'down %.0s' $(seq 1 "$1")) ret; sleep 10; shot "confirm-$1"; k right ret
}
echo "== importing MYSONGS (drive 1)"
to_storage; shot storage
import_drive 1
until_g '[ -f "/data/songs/MYSONGS/Rock/AC-DC - Back in Black/song.opus" ] && [ -f "/data/songs/MYSONGS/Pop/Abba - Dancing Queen/song.opus" ] && ! mountpoint -q /run/yargos-import/mnt'; r=$?
check "the songs were copied" [ $r = 0 ]
sleep 3; shot after-import-1
eq "all seven files and the drive's record are there" 8 "$(g 'find /data/songs/MYSONGS -type f | wc -l')"
check "the folder structure is as on the drive" has "/data/songs/MYSONGS/Rock/Aerosmith - Walk This Way/song.ini"
eq "the folders directly in the drive's folder" "Pop|Rock|" "$(names /data/songs/MYSONGS | sed 's/\.yargos-source|//')"
check "a big file arrived whole" [ "$(g 'wc -c < "/data/songs/MYSONGS/Rock/Aerosmith - Walk This Way/song.opus"')" = 2500000 ]
check "the Windows folders were left out" hasnt '/data/songs/MYSONGS/$RECYCLE.BIN'
check "  ...and lost+found" hasnt '/data/songs/MYSONGS/lost+found'
check "the link was left out" hasnt /data/songs/MYSONGS/Rock/link
eq "the player owns what was copied" 0 "$(g 'find /data/songs ! -user player | wc -l')"
eq "no half-copied file is left" 0 "$(g 'find /data/songs -name "*.yargos-part" | wc -l')"
check "the drive is unmounted again" [ "$(g 'mountpoint -q /run/yargos-import/mnt && echo mounted')" != mounted ]

echo "== a second drive with the same name gets its own folder"
k esc; sleep 2
import_drive 3
until_g '[ -f "/data/songs/MYSONGS (2)/Other/Muse - Hysteria/song.opus" ] && ! mountpoint -q /run/yargos-import/mnt'; r=$?
check "MYSONGS (2) holds the other drive" [ $r = 0 ]
check "the first folder was not mixed with it" hasnt /data/songs/MYSONGS/Other
eq "the two drives have different records" 2 "$(g 'cat /data/songs/MYSONGS/.yargos-source /data/songs/"MYSONGS (2)"/.yargos-source | sort -u | wc -l')"

echo "== the song drive (already mounted at /media/songs)"
g 'ls /media/songs >/dev/null' >/dev/null; sleep 2
k esc; sleep 2
import_drive 2
until_g '[ -f "/data/songs/YARGSONGS/Songs/Queen - Radio Gaga/song.opus" ] && ! mountpoint -q /run/yargos-import/mnt'; r=$?
check "a drive that is already mounted can be imported too" [ $r = 0 ]

echo "== the same drive again: nothing to do"
k esc; sleep 2
k ret; sleep 4; k down down ret; sleep 10; shot already
eq "importing it again made no new folder (the screenshot shows the message)" 3 "$(g 'ls -1 /data/songs | wc -l')"
sleep 5

echo "== what YARG will scan"
out=$(g '/usr/lib/yargos/song-sources')
check "every imported folder is a source" [ "$(grep -c '^/data/songs/' <<< "$out")" -ge 4 ]
check "the song drive is a source too" [ "$(grep -c '^/media/songs' <<< "$out")" = 1 ]
g 'mkdir -p /data/yarg && echo "{\"SongFolders\":[\"/home/x/mine\",\"/data/songs/Gone\"]}" > /data/yarg/settings.json && chown player /data/yarg/settings.json; su player -s /bin/sh -c "DISPLAY= /usr/lib/yargos/run-yarg" >/dev/null 2>&1' >/dev/null
check "YARG's list keeps a folder of the user's and drops a gone one" [ "$(g 'jq -r ".SongFolders | map(select(. == \"/home/x/mine\" or . == \"/data/songs/Gone\")) | join(\",\")" /data/yarg/settings.json')" = /home/x/mine ]
check "YARG's list has the internal folders" [ "$(g 'jq -r ".SongFolders[]" /data/yarg/settings.json | grep -c "^/data/songs/"')" -ge 4 ]

echo "== browsing and deleting"
k esc; sleep 2; k down ret; sleep 3; shot manage
k down left; sleep 1; shot ask-folder; k right ret
until_g '[ ! -d /data/songs/MYSONGS ]'; r=$?
check "a folder (an imported drive) was deleted" [ $r = 0 ]
eq "the others are still there" "MYSONGS (2)|YARGSONGS|" "$(names /data/songs)"
sleep 2; shot after-folder-delete
# the highlight stayed on row 1 (MYSONGS (2)): down to YARGSONGS, open it, open Songs, then delete the one song
k down ret; sleep 2; k down ret; sleep 2; shot songs-level
k down ret; sleep 1; shot ask-song; k right ret
until_g '[ ! -d "/data/songs/YARGSONGS/Songs/Queen - Radio Gaga" ]'; r=$?
check "a single song was deleted" [ $r = 0 ]
check "its folder is still there" has /data/songs/YARGSONGS/Songs
# back to the top, delete everything
k esc; sleep 2; k esc; sleep 2; k ret; sleep 1; shot ask-all; k right ret
until_g '[ -z "$(ls -A /data/songs)" ]'; r=$?
check "everything was deleted" [ $r = 0 ]
eq "the library folder itself is still there, and the player's" 1 "$(owner /data/songs)"
sleep 2; shot after-all

qstop
echo
[ $fail = 0 ] && echo "ALL PASSED" || { echo "SOME CHECKS FAILED (screenshots in $W/shots)"; exit 1; }
