#!/bin/bash
# Run a shell command inside a HEADLESS=1 tools/qemu-run.sh guest (logs in as root on the serial console).
#   tools/qemu-sh.sh 'ls -l /dev/hidraw*; id player'
set -eu
WORK=${WORK:-$HOME/yargos-build/qemu}
python3 - "$WORK/serial.sock" "$*" <<'PY'
import re, socket, sys, time
path, cmd = sys.argv[1], sys.argv[2]
s = socket.socket(socket.AF_UNIX); s.connect(path); s.settimeout(0.5)
def read(secs):
    end, buf = time.time() + secs, b''
    while time.time() < end:
        try:
            d = s.recv(65536)
            if not d: break
            buf += d
        except socket.timeout:
            pass
    return buf.decode('utf-8', 'replace')
s.sendall(b'\n'); out = read(1.5)
if 'login:' in out:
    s.sendall(b'root\n'); out = read(2.5)
s.sendall(b'\n'); read(0.5)
marker = f"__END{int(time.time())}__"
s.sendall(f"{cmd}\necho {marker}$?\n".encode())
buf, end = '', time.time() + 60
while time.time() < end and not re.search(marker + r'\d+', buf.split(marker, 1)[-1] if buf.count(marker) > 1 else ''):
    buf += read(1.0)
    if buf.count(marker) >= 2: break
text = re.sub(r'\x1b\[[0-9;?]*[a-zA-Z]|\x1b\][^\x07\x1b]*[\x07\x1b\\]?', '', buf)
body = text.split(marker)[0]
lines = [l for l in body.replace('\r', '').split('\n')]
# drop the echoed command lines
print('\n'.join(l for l in lines if l.strip() and not l.startswith(cmd.split('\n')[0][:20]) and 'echo __END' not in l))
PY
