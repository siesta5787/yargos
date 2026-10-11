#!/bin/bash
# Exercise yargos-update against a local fake GitHub (no root, no internet).
#   tools/test-update.sh [path/to/extracted/real/YARG/folder]   (the folder adds a real-library check)
set -u
HERE=$(cd "$(dirname "$0")/.." && pwd)
UPD=$HERE/board/yargos/rootfs-overlay/usr/bin/yargos-update
T=$(mktemp -d); trap 'kill $SRV 2>/dev/null; rm -rf "$T"' EXIT
PORT=${PORT:-18765}
fail=0
ok()   { echo "  ok   $*"; }
bad()  { echo "  FAIL $*"; fail=1; }
check() { local d=$1; shift; if "$@"; then ok "$d"; else bad "$d"; fi; }

export YARGOS_DATA=$T/data YARGOS_OWNER=nobody-such-user YARGOS_MAX_ATTEMPTS=3
APPS=$YARGOS_DATA/apps/yarg; mkdir -p "$APPS" "$T/www"

mkzip() { # mkzip VERSION [broken]  -> $T/www/yarg-VERSION.zip
    local v=$1 d=$T/pkg-$1; mkdir -p "$d/YARG_Data/Plugins"
    printf '#!/bin/sh\nexit 0\n' > "$d/YARG"; chmod 755 "$d/YARG"
    if [ "${2:-}" = broken ]; then echo "not an elf" > "$d/UnityPlayer.so"; else cp /lib/x86_64-linux-gnu/libz.so.1 "$d/UnityPlayer.so"; fi
    cp /lib/x86_64-linux-gnu/libz.so.1 "$d/YARG_Data/Plugins/libfake.so"
    (cd "$d" && zip -qr "$T/www/yarg-$v.zip" .)
}
mkapi() { # mkapi FILE VERSION ZIP [digest-override]
    local sum; sum=$(sha256sum "$T/www/$3" | cut -d' ' -f1); [ -n "${4:-}" ] && sum=$4
    printf '{"tag_name":"v%s","assets":[{"name":"YARG_v%s-Linux-x86_64.zip","browser_download_url":"http://127.0.0.1:%s/%s","digest":"sha256:%s"}]}' \
        "$2" "$2" "$PORT" "$3" "$sum" > "$T/www/$1"
}
(cd "$T/www" && exec python3 -m http.server "$PORT" >/dev/null 2>&1) & SRV=$!
sleep 1

# installed: 0.15.0
mkdir -p "$APPS/0.15.0"; ln -s 0.15.0 "$APPS/current"
mkzip 0.99.0; mkzip 0.98.0 broken
mkapi same.json 0.15.0 yarg-0.99.0.zip
mkapi good.json 0.99.0 yarg-0.99.0.zip
mkapi badsum.json 0.99.0 yarg-0.99.0.zip 0000000000000000000000000000000000000000000000000000000000000000
mkapi broken.json 0.98.0 yarg-0.98.0.zip
printf '{"tag_name":"v0.99.0","assets":[{"name":"YARG_v0.99.0-Linux-x86_64.zip","browser_download_url":"http://127.0.0.1:%s/yarg-0.99.0.zip"}]}' "$PORT" > "$T/www/nodigest.json"
api() { YARGOS_API="http://127.0.0.1:$PORT/$1"; export YARGOS_API; }
cur() { basename "$(readlink "$APPS/current")"; }

echo "up to date"
api same.json
"$UPD" apply 2>&1 | grep -q "up to date"; check "reports up to date" [ $? -eq 0 ]
check "changes nothing" [ "$(cur)" = 0.15.0 ]
echo "refusals"
api nodigest.json; "$UPD" apply >/dev/null 2>&1; check "refuses a release with no digest" [ "$(cur)" = 0.15.0 ]
api badsum.json;   "$UPD" apply >/dev/null 2>&1; check "refuses a bad checksum" [ "$(cur)" = 0.15.0 ]
api broken.json;   "$UPD" apply >/dev/null 2>&1; check "refuses a package that would not start" [ "$(cur)" = 0.15.0 ]
check "leaves no staging debris" bash -c '[ -z "$(ls -A '"$APPS"'/.stage-* '"$APPS"'/.dl 2>/dev/null)" ]'
echo "update"
api good.json
"$UPD" check | grep -q "update available"; check "check sees the update" [ $? -eq 0 ]
"$UPD" apply >/dev/null 2>&1
check "switches current to 0.99.0" [ "$(cur)" = 0.99.0 ]
check "keeps 0.15.0 as previous" [ "$(basename "$(readlink "$APPS/previous")")" = 0.15.0 ]
check "marks the new version as on trial" [ "$(cat "$APPS/.pending")" = "0.99.0 0" ]
check "installed YARG is executable" [ -x "$APPS/current/YARG" ]
check "the new version folder is intact" [ -f "$APPS/0.99.0/UnityPlayer.so" ]
check "the previous version folder is intact" [ -d "$APPS/0.15.0" ]
echo "rollback after repeated failed starts"
for i in 1 2 3; do "$UPD" boot-check; done
check "still 0.99.0 after 3 starts" [ "$(cur)" = 0.99.0 ]
"$UPD" boot-check
check "4th start rolls back to 0.15.0" [ "$(cur)" = 0.15.0 ]
check "remembers 0.99.0 as rejected" grep -qx 0.99.0 "$APPS/.rejected"
check "clears the trial marker" [ ! -e "$APPS/.pending" ]
"$UPD" apply 2>&1 | grep -q "rolled back before"; check "does not offer the rejected version again" [ $? -eq 0 ]
echo "offline"
YARGOS_API="http://127.0.0.1:1/none" "$UPD" auto >/dev/null 2>&1; check "auto exits cleanly when offline" [ $? -eq 0 ]
echo "a healthy new version"
rm -f "$APPS/.rejected"; "$UPD" apply >/dev/null 2>&1; "$UPD" boot-check; "$UPD" mark-good
check "mark-good clears the trial" [ ! -e "$APPS/.pending" ]
for i in 1 2 3 4 5; do "$UPD" boot-check; done
check "good version is never rolled back" [ "$(cur)" = 0.99.0 ]
"$UPD" rollback >/dev/null 2>&1
check "manual rollback works" [ "$(cur)" = 0.15.0 ]

echo "first install (an image without YARG)"
rm -rf "$APPS"; mkdir -p "$APPS"
api good.json
"$UPD" auto 2>&1 | grep -q "not installed"; check "the daily update never installs YARG by itself" [ $? -eq 0 ]
check "and installed nothing" [ ! -e "$APPS/current" ]
out=$("$UPD" install 2>&1); case $out in *"STEP unpacking"*) ok "install reports its steps" ;; *) bad "install reports its steps" ;; esac
check "install makes it current" [ "$(cur)" = 0.99.0 ]
check "with no previous version" [ ! -e "$APPS/previous" ]
check "on trial" [ "$(cat "$APPS/.pending")" = "0.99.0 0" ]
out=$("$UPD" install 2>&1); case $out in *"already installed"*) ok "install refuses when YARG is there" ;; *) bad "install refuses when YARG is there" ;; esac
for i in 1 2 3 4 5; do "$UPD" boot-check; done
check "a first install with nothing to go back to is never rolled back" [ "$(cur)" = 0.99.0 ]

echo "install from a zip (no internet)"
rm -rf "$APPS"; mkdir -p "$APPS" "$T/songs"
export YARGOS_SONGS=$T/songs
check "no zip on the drive: nothing listed" [ -z "$("$UPD" find-zip)" ]
cp "$T/www/yarg-0.99.0.zip" "$T/songs/YARG_v0.99.0-Linux-x86_64.zip"
check "find-zip lists it" [ "$("$UPD" find-zip)" = "$T/songs/YARG_v0.99.0-Linux-x86_64.zip" ]
"$UPD" install-zip "$T/songs/YARG_v0.99.0-Linux-x86_64.zip" >/dev/null 2>&1
check "install-zip makes it current" [ "$(cur)" = 0.99.0 ]
check "and it is launchable" [ -x "$APPS/current/YARG" ]
cp "$T/www/yarg-0.99.0.zip" "$T/songs/odd name.zip"
"$UPD" install-zip "$T/songs/odd name.zip" 2>&1 | grep -q "cannot tell the version"; check "a zip without a version in its name is refused" [ $? -eq 0 ]
echo "not a zip" > "$T/songs/YARG_v1.0.0-Linux-x86_64.zip"
"$UPD" install-zip "$T/songs/YARG_v1.0.0-Linux-x86_64.zip" >/dev/null 2>&1; check "a broken zip is refused" [ "$(cur)" = 0.99.0 ]
check "and leaves no debris" bash -c '[ -z "$(ls -A '"$APPS"'/.stage-* 2>/dev/null)" ]'
unset YARGOS_SONGS

echo "a lock left by an install that was cut short"
rm -rf "$APPS"; mkdir -p "$APPS"
api good.json
mkdir "$APPS/.lock" "$APPS/.stage-0.99.0"; echo 999999 > "$APPS/.lock/pid"; cat /proc/sys/kernel/random/boot_id > "$APPS/.lock/boot"
out=$("$UPD" install 2>&1); case $out in *"clearing what an unfinished update left"*) ok "a dead owner's lock is cleared" ;; *) bad "a dead owner's lock is cleared" ;; esac
check "and the install went through" [ "$(cur)" = 0.99.0 ]
rm -rf "$APPS"; mkdir -p "$APPS/.lock"; echo $$ > "$APPS/.lock/pid"; cat /proc/sys/kernel/random/boot_id > "$APPS/.lock/boot"
out=$("$UPD" install 2>&1); case $out in *"another update is running"*) ok "a live owner's lock is respected" ;; *) bad "a live owner's lock is respected" ;; esac
rm -rf "$APPS"; mkdir -p "$APPS/.lock"; echo $$ > "$APPS/.lock/pid"; echo some-earlier-boot > "$APPS/.lock/boot"
"$UPD" install >/dev/null 2>&1; check "a lock from an earlier boot is cleared even if the pid is in use again" [ "$(cur)" = 0.99.0 ]
check "no lock is left after an install" [ ! -e "$APPS/.lock" ]

if [ -n "${1:-}" ]; then
    echo "real YARG folder"
    check "real YARG passes the library check here" "$UPD" verify-dir "$1"
fi
[ $fail = 0 ] && echo "ALL PASSED" || { echo "SOME FAILED"; exit 1; }
