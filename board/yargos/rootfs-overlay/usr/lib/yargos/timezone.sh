#!/bin/sh
# timezone.sh: make the time zone the owner chose (TIMEZONE=<zone> in /data/yargos.conf, written by the dashboard's Date & time
# screen) the system's. Run by the root helper (`timezone-apply`). The system folder is replaced by every system update, so the
# choice lives on /data; it is applied again whenever the dashboard starts. /data/yargos.conf belongs to the player: it is only
# read as text, the name is checked hard, and the link can only point into /usr/share/zoneinfo.
tz=$(sed -n 's/^TIMEZONE=//p' /data/yargos.conf 2>/dev/null | tail -n 1)
case $tz in
    "" | /* | *..* | *[!A-Za-z0-9_+/-]*) echo "no time zone chosen"; exit 0 ;;
esac
[ -f "/usr/share/zoneinfo/$tz" ] || { echo "error: unknown time zone $tz"; exit 1; }
ln -sfn "/usr/share/zoneinfo/$tz" /etc/localtime && echo "$tz" > /etc/timezone && echo ok
