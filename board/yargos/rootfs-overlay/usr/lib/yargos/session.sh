#!/bin/sh
# Xorg refuses /dev/stderr as a log file, so log to /tmp and print it to the journal if it exits.
xinit /usr/lib/yargos/run-yarg -- /usr/lib/yargos/xorg :0 vt1 -nolisten tcp -br -nocursor -keeptty -logfile /tmp/Xorg.0.log -verbose 3
rc=$?
sed 's/^/xorg: /' /tmp/Xorg.0.log >&2
exit $rc
