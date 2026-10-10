import struct, math, sys
import os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from fft import fft
# Usage: python3 -I analyze_raw.py converted.raw   (48 kHz stereo s16 from the guest; see README)
def run(path, sr=48000, expect=(1000,10000)):
    raw=open(path,'rb').read(); s=struct.unpack('<%dh'%(len(raw)//2),raw)[0::2]
    N=16384; start=int(sr*1.0)
    seg=[s[start+i]/32768*(0.5-0.5*math.cos(2*math.pi*i/N)) for i in range(N)]
    X=fft(seg); mag=[abs(X[k])/(N/4) for k in range(N//2)]
    hz=lambda k:k*sr/N; peak=max(mag); db=lambda m:20*math.log10(max(m,1e-12)/peak)
    lines=[(db(mag[k]),hz(k)) for k in range(5,N//2-1) if mag[k]>mag[k-1] and mag[k]>=mag[k+1] and all(abs(hz(k)-f)>150 for f in expect)]
    lines.sort(reverse=True)
    print(f"{path}: {len(s)/sr:.1f} s at {sr} Hz")
    for d,f in lines[:5]: print(f"   unwanted line at {f:8.0f} Hz: {d:6.1f} dB below the tone")
    w=lines[0][0] if lines else -120
    print(f"   worst unwanted component: {w:.1f} dB  ->  {'clean (inaudible)' if w<-80 else 'audible harshness likely' if w>-60 else 'borderline'}")
if __name__=='__main__': run(sys.argv[1])
