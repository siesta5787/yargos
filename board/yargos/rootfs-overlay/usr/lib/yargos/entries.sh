# Boot entry writing, shared by the image build (post-image.sh), the installer and the updater. Source it; plain sh.
#
# The disk a system sits on is named in its boot entry, not in the system: the root partition by GUID, and /data by
# systemd.mount-extra (so one root filesystem image fits every disk, which is what makes updates possible).
# Layout of the ESP:  loader/loader.conf   loader/entries/*.conf   yargos/<version>/bzImage
#
# OS entries are called yargos-<version>.conf and are the ones picked by default (loader.conf: default yargos-2*). The
# newest version wins (see "Two kinds of version" below for 1.0, 1.1 ... after the date-like 2026.10.30 ones); an entry with a boot counter (yargos-<version>+2.conf) that did not boot in time is skipped, which
# is how a bad update falls back. install.conf and debug.conf are menu extras for whichever system booted last.

YARGOS_BASE_OPTS="rootwait rw usbcore.autosuspend=-1 usb-storage.delay_use=5 panic=10"
YARGOS_QUIET_OPTS="quiet loglevel=0 vt.global_cursor_default=0"
YARGOS_VERBOSE_OPTS="console=tty0 console=ttyS0,115200 loglevel=7 systemd.log_level=info systemd.show_status=1 systemd.journald.forward_to_console=1 systemd.journald.max_level_console=info"

# Extra kernel options for the quiet entries, from a file that only a test build has (YARGOS_TEST_CONSOLE=1 in
# post-build.sh puts "console=ttyS0,115200" there, so QEMU tests get a shell on the normal boot without the boot menu).
yargos_extra_opts() { cat "${YARGOS_EXTRA_FILE:-/usr/lib/yargos/entry-extra-opts}" 2>/dev/null | tr '\n' ' '; }

# entry_options ROOT_PARTUUID DATA_PARTUUID quiet|verbose|install
entry_options() {
    printf 'root=PARTUUID=%s systemd.mount-extra=PARTUUID=%s:/data:ext4:rw,noatime,nofail,x-systemd.device-timeout=10s %s' "$1" "$2" "$YARGOS_BASE_OPTS"
    case $3 in
        verbose) printf ' %s' "$YARGOS_VERBOSE_OPTS" ;;
        install) printf ' %s yargos.install=1' "$YARGOS_QUIET_OPTS" ;;
        *) printf ' %s %s' "$YARGOS_QUIET_OPTS" "$(yargos_extra_opts)" ;;
    esac
}

# Two kinds of version. Until the bridge release a version was a date, 2026.10.30. From 1.0 on it is a release number: 1.0,
# 1.1 ... 2.0. A release number is always NEWER than any date, but "1.0" compared as numbers is smaller than "2026.10.30": the
# updater (ver_gt) and this boot entry naming take care of it. systemd-boot orders entries by sort-key first (ascending), and
# entries with the same key by version (newest first), and default picks the first entry matching yargos-2*: so release entries
# get a sort-key that sorts before the date ones, and their file name keeps the yargos-2 prefix (yargos-2-1.0.conf).
version_era() { # VERSION: "date" when the first number is 1000 or more, else "release"
    case ${1%%.*} in "" | *[!0-9]*) echo date ;; *) if [ "${1%%.*}" -ge 1000 ]; then echo date; else echo release; fi ;; esac
}
os_entry_id() { # VERSION: the boot entry's name, without .conf
    if [ "$(version_era "$1")" = release ]; then echo "yargos-2-$1"; else echo "yargos-$1"; fi
}
os_sort_key() { # VERSION
    if [ "$(version_era "$1")" = release ]; then echo yargos-9; else echo yargos-a; fi
}

# write_entry FILE TITLE VERSION SORTKEY KERNEL OPTIONS  (written beside the target and renamed over it)
write_entry() {
    printf 'title   %s\nversion %s\nsort-key %s\nlinux   %s\noptions %s\n' "$2" "$3" "$4" "$5" "$6" > "$1.new" && mv "$1.new" "$1"
}

# write_os_entry ESP VERSION ROOT DATA [TRIES]: the entry that starts that version from that root partition.
write_os_entry() {
    write_entry "$1/loader/entries/$(os_entry_id "$2")${5:+"+$5"}.conf" "YARG OS $2" "$2" "$(os_sort_key "$2")" "/yargos/$2/bzImage" "$(entry_options "$3" "$4" quiet)"
}

# write_extra_entries ESP VERSION ROOT DATA [install]: the troubleshooting entry, and the installer entry on a stick.
write_extra_entries() {
    write_entry "$1/loader/entries/debug.conf" "YARG OS $2 (verbose, for troubleshooting)" "$2" yargos-c "/yargos/$2/bzImage" "$(entry_options "$3" "$4" verbose)"
    [ "${5:-}" = install ] && write_entry "$1/loader/entries/install.conf" "Install YARG OS on this computer" "$2" yargos-b "/yargos/$2/bzImage" "$(entry_options "$3" "$4" install)"
    return 0
}

# write_loader ESP TIMEOUT
write_loader() {
    printf '# default: the newest YARG OS that has not failed to start. Hold the space bar while the computer starts for this menu.\ndefault yargos-2*\ntimeout %s\nconsole-mode keep\neditor yes\n' "$2" > "$1/loader/loader.conf"
}
