#!/bin/bash
# Save a PNG screenshot of a HEADLESS=1 tools/qemu-run.sh guest.   tools/qemu-shot.sh [out.png]
set -eu
WORK=${WORK:-$HOME/yargos-build/qemu}
OUT=${1:-/tmp/qemu-shot.png}
python3 - "$WORK/monitor.sock" "$WORK/shot.ppm" "$OUT" <<'PY'
import socket, sys, time, zlib, struct
sock, ppm, out = sys.argv[1:4]
s = socket.socket(socket.AF_UNIX); s.connect(sock); time.sleep(0.3); s.recv(65536)
s.sendall(f"screendump {ppm}\n".encode()); time.sleep(1.0); s.close()
d = open(ppm, 'rb').read()
parts = d.split(None, 4)           # P6 w h maxval <data>
w, h = int(parts[1]), int(parts[2]); px = parts[4][:w*h*3]
raw = b''.join(b'\x00' + px[y*w*3:(y+1)*w*3] for y in range(h))
chunk = lambda t, b: struct.pack('>I', len(b)) + t + b + struct.pack('>I', zlib.crc32(t + b))
open(out, 'wb').write(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b''))
print(f"{out} ({w}x{h})")
PY
