#!/usr/bin/env python3
"""Talk to a Wiimote directly over L2CAP (like Dolphin/wiiuse), bypassing BlueZ's HID profile.

Hold 1+2 on the Wiimote, then run:  tools/dev-test.sh probe  [BD_ADDR]
Prints whether the two HID channels (PSM 0x11 control, 0x13 interrupt) connect and what the
Wiimote answers to an LED + status request. No root needed.
"""
import socket, subprocess, sys, time

def find_wiimote():
    out = subprocess.run(["bluetoothctl", "devices"], capture_output=True, text=True).stdout
    for line in out.splitlines():
        if "Nintendo RVL" in line:
            return line.split()[1]
    return None

addr = (sys.argv[1] if len(sys.argv) > 1 else "") or find_wiimote()
if not addr:
    sys.exit("no Wiimote known to BlueZ; hold 1+2 and run 'bluetoothctl scan on' for a few seconds first")
print(f"Wiimote {addr}: connecting HID channels directly...", flush=True)

def chan(psm):
    s = socket.socket(socket.AF_BLUETOOTH, socket.SOCK_SEQPACKET, socket.BTPROTO_L2CAP)
    s.settimeout(10)
    t = time.time()
    s.connect((addr, psm))
    print(f"  PSM 0x{psm:02x} connected in {time.time() - t:.1f}s", flush=True)
    return s

try:
    ctl = chan(0x11)
    intr = chan(0x13)
except OSError as e:
    sys.exit(f"  FAILED: {e} (errno {e.errno}). Keep 1+2 held while this runs; try again.")

intr.settimeout(1.0)
intr.send(bytes([0xA2, 0x11, 0x10]))   # LED 1 on
intr.send(bytes([0xA2, 0x15, 0x00]))   # status request
print("sent LED + status request; reading 4 s (plug a guitar into the Wiimote if you have one)", flush=True)
end = time.time() + 4
while time.time() < end:
    try:
        d = intr.recv(64)
        print("  recv", d.hex(" "), flush=True)
    except socket.timeout:
        pass
print("done: if you saw 'a1 20 ...' (status report) the direct route works")
