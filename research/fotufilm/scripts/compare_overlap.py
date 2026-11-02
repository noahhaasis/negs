"""Compare fotufilm and Negs (spektrafilm) data for stocks both projects digitised."""
import json, math, sys
F = sys.argv[1]; N = sys.argv[2]
pairs = [("portra400","kodak_portra_400"),("gold200","kodak_gold_200"),("ektar100","kodak_ektar_100"),
         ("ultramax400","kodak_ultramax_400"),("c200","fujifilm_c200"),("pro400h","fujifilm_pro_400h"),
         ("vision250d","kodak_vision3_250d"),("vision500t","kodak_vision3_500t"),("verita200d","kodak_verita_200d"),
         ("provia100f","fujifilm_provia_100f"),("kodachrome64","kodak_kodachrome_64")]
wl = [380+5*i for i in range(81)]
def peak(v):
    best=max(range(81), key=lambda i: (v[i] if v[i] is not None and not math.isnan(v[i]) else -1e9)); return wl[best]
for f,n in pairs:
    fd=json.load(open(f"{F}/{f}.json")); nd=json.load(open(f"{N}/{n}.json"))
    fs=fd["spectral"]["layerSensitivity"]  # linear, per layer R,G,B
    ns=[[ (10**r[c] if r[c] is not None else float('nan')) for r in nd["data"]["log_sensitivity"]] for c in range(3)]
    out=[]
    for c in range(3):
        a=fs[c]; b=ns[c]
        ma=max(a); mb=max(x for x in b if not math.isnan(x))
        la=[math.log10(x/ma) if x>0 else None for x in a]; lb=[math.log10(x/mb) if (not math.isnan(x) and x>0) else None for x in b]
        d=[abs(p-q) for p,q in zip(la,lb) if p is not None and q is not None and p>-1.5 and q>-1.5]
        out.append(f"{'RGB'[c]} pk {peak(a)}/{peak(b)} mean|dlog|={sum(d)/max(len(d),1):.3f}")
    fdye=fd["spectral"]["imageDyeDensity"]; ndye=[[r[c] for r in nd["data"]["channel_density"]] for c in range(3)]
    dp=[f"{peak(fdye[c])}/{peak(ndye[c])}" for c in range(3)]
    fc=fd["curves"]; gam=[round(c["gamma"],2) for c in fc]
    print(f"{f:13} {' | '.join(out)} | dye pk C,M,Y {dp} | fotu gamma {gam} kind={fd['spectral']['kind']}")
