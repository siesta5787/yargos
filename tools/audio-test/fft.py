import wave, struct, math, sys, cmath
def fft(x):
    n=len(x)
    if n==1: return x
    e=fft(x[0::2]); o=fft(x[1::2])
    t=[cmath.exp(-2j*math.pi*k/n)*o[k] for k in range(n//2)]
    return [e[k]+t[k] for k in range(n//2)]+[e[k]-t[k] for k in range(n//2)]
def analyse(path, expect=(1000,10000)):
    w=wave.open(path,'rb'); sr=w.getframerate(); nch=w.getnchannels(); raw=w.readframes(w.getnframes())
    s=struct.unpack('<%dh'%(len(raw)//2),raw)[0::nch]
    N=16384; start=int(sr*2.5)           # well inside the tone
    seg=[s[start+i]/32768*(0.5-0.5*math.cos(2*math.pi*i/N)) for i in range(N)]
    X=fft(seg)
    mag=[abs(X[k])/(N/4) for k in range(N//2)]
    hz=lambda k:k*sr/N
    peak=max(mag)
    db=lambda m:20*math.log10(max(m,1e-12)/peak)
    print(f"{path}: rate {sr} Hz, {nch} ch, tone level reference = {peak:.3f}")
    # strongest lines away from the two expected tones
    lines=[(db(mag[k]),hz(k)) for k in range(5,N//2-1) if mag[k]>mag[k-1] and mag[k]>=mag[k+1] and all(abs(hz(k)-f)>150 for f in expect)]
    lines.sort(reverse=True)
    for d,f in lines[:6]: print(f"   spurious line at {f:8.0f} Hz: {d:6.1f} dB")
    worst=lines[0][0] if lines else -120
    print(f"   worst unwanted component: {worst:.1f} dB below the tone  ->  {'clean' if worst<-80 else 'DIRTY (audible harshness)' if worst>-60 else 'so-so'}")
if __name__=='__main__': analyse(sys.argv[1])
