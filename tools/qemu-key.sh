#!/bin/bash
# Send keys to a HEADLESS=1 tools/qemu-run.sh guest.   tools/qemu-key.sh ret down down ret
# Names are QEMU key names: up down left right ret esc backspace spc a..z 0..9 shift-a ...
set -eu
WORK=${WORK:-$HOME/yargos-build/qemu}
python3 - "$WORK/monitor.sock" "$@" <<'PY'
import socket, sys, time
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1]); time.sleep(0.3); s.recv(65536)
for k in sys.argv[2:]:
    s.sendall(f"sendkey {k}\n".encode()); time.sleep(0.35); s.recv(65536)
PY
