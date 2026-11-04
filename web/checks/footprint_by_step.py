"""Peak and median WebContent footprint per step of a device run:

    python3 web/checks/footprint_by_step.py web/results/device-iphone-stocks.jsonl target/iphone-preview/footprint.jsonl

The tab is the WebContent process with the largest footprint during the run;
other tabs and extensions have their own. Each step covers the samples since
the previous one. web/checks/footprint.py writes the samples."""
import json, sys
bench=[json.loads(l) for l in open(sys.argv[1])]
s=[json.loads(l) for l in open(sys.argv[2]) if l.strip()]
start=bench[0]['at']/1000-20; end=bench[-1]['at']/1000+2
web=[x for x in s if x['name']=='com.apple.WebKit.WebContent' and start<=x['t']<=end]
if not web: sys.exit("no samples yet")
# The test tab: the WebContent process with the largest footprint in the run.
pid=max(web, key=lambda x: x['mib'])['pid']
web=[x for x in web if x['pid']==pid]
prev=start
for r in bench:
    t=r['at']/1000
    w=[x['mib'] for x in web if prev<=x['t']<=t]
    print(f"{r['event']:20} pid {pid} peak {max(w) if w else '-':>8} median {sorted(w)[len(w)//2] if w else '-':>8} n {len(w):3}  wasm {r.get('memoryMiB')} tile-wasm {r.get('tileMiB')}")
    prev=t
