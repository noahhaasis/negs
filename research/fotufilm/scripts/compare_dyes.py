"""Numeric closeness of dye spectra and base density between fotufilm and Negs for shared stocks."""
import json, math, sys
F, N = sys.argv[1], sys.argv[2]
pairs = [("portra400","kodak_portra_400"),("gold200","kodak_gold_200"),("ektar100","kodak_ektar_100"),
         ("ultramax400","kodak_ultramax_400"),("c200","fujifilm_c200"),("pro400h","fujifilm_pro_400h"),
         ("vision250d","kodak_vision3_250d"),("vision500t","kodak_vision3_500t"),("verita200d","kodak_verita_200d"),
         ("provia100f","fujifilm_provia_100f"),("kodachrome64","kodak_kodachrome_64")]
def ok(x): return x is not None and not (isinstance(x,float) and math.isnan(x))
for f,n in pairs:
    fd=json.load(open(f"{F}/{f}.json")); nd=json.load(open(f"{N}/{n}.json"))
    res=[]
    for c in range(3):
        a=fd["spectral"]["imageDyeDensity"][c]; b=[r[c] for r in nd["data"]["channel_density"]]
        idx=[i for i in range(81) if ok(b[i])]
        ma=max(a[i] for i in idx); mb=max(b[i] for i in idx)
        rel=[abs(a[i]/ma-b[i]/mb) for i in idx]
        ratio=[a[i]/b[i] for i in idx if b[i]>0.2*mb]
        res.append(f"{'CMY'[c]} max|d|={max(rel):.3f} scale={min(ratio):.3f}-{max(ratio):.3f}")
    bd=nd["data"]["base_density"]; md=fd["spectral"].get("minimumDensity")
    bstr=""
    if md:
        d=[md[i]-bd[i] for i in range(81) if ok(bd[i])]; bstr=f"dmin-base {min(d):+.3f}..{max(d):+.3f}"
    print(f"{f:13}", " | ".join(res), "|", bstr)
