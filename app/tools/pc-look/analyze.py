#!/usr/bin/env python3
"""Reads negs-meter JSON files and prints per-phase frame statistics.

Script marks understood:
  mark begin NAME GROUP   ... mark end NAME
      frames in between whose GROUP probe rows changed: fps over the span
      from the first to the last change, interval percentiles, long frames.
  mark latency NAME GROUP
      time from the next input event (down/keydown) to the first frame whose
      GROUP rows changed, and until the last change within 700 ms (settled).

  analyze.py run1.json [run2.json ...]  -> one JSON line per run on stdout
"""
import json
import sys


def pct(xs, p):
    if not xs:
        return None
    s = sorted(xs)
    k = (len(s) - 1) * p / 100.0
    f = int(k)
    c = min(f + 1, len(s) - 1)
    return s[f] + (s[c] - s[f]) * (k - f)


def analyze(path):
    d = json.load(open(path))
    freq = d["freq"]
    groups = d["groups"]
    frames = d["frames"]
    events = d["events"]
    ms = lambda dt: dt * 1000.0 / freq
    out = {"file": path, "mem": d.get("mem"), "phases": {}, "latency": {}}
    open_phase = {}
    pending = []
    for i, (t, e) in enumerate(events):
        w = e.split()
        if w[0] == "begin":
            open_phase[w[1]] = (t, groups.index(w[2]))
        elif w[0] == "end":
            t0, g = open_phase.pop(w[1])
            bit = 1 << g
            ts = [f[0] for f in frames if t0 <= f[0] <= t and f[3] & bit]
            acc = sum(f[1] - 1 for f in frames if t0 <= f[0] <= t and f[1] > 1)
            iv = [ms(b - a) for a, b in zip(ts, ts[1:])]
            span = ms(ts[-1] - ts[0]) if len(ts) > 1 else 0
            out["phases"][w[1]] = {
                "frames": len(ts),
                "span_ms": round(span, 1),
                "fps": round((len(ts) - 1) / span * 1000, 1) if span else None,
                "median": round(pct(iv, 50), 2) if iv else None,
                "p95": round(pct(iv, 95), 2) if iv else None,
                "p99": round(pct(iv, 99), 2) if iv else None,
                "max": round(max(iv), 2) if iv else None,
                "over25": sum(1 for x in iv if x > 25),
                "missed_by_meter": acc,
            }
        elif w[0] == "latency":
            # The next input event after this mark.
            for t2, e2 in events[i + 1:]:
                if e2.startswith("down") or e2.startswith("keydown"):
                    pending.append((w[1], groups.index(w[2]), t2))
                    break
    for name, g, t0 in pending:
        bit = 1 << g
        ts = [f[0] for f in frames if f[0] >= t0 and f[3] & bit and ms(f[0] - t0) < 1100]
        if not ts:
            out["latency"].setdefault(name, []).append(None)
            continue
        first = ts[0]
        last = first
        for t in ts[1:]:
            if ms(t - last) > 700:
                break
            last = t
        out["latency"].setdefault(name, []).append((round(ms(first - t0), 1), round(ms(last - t0), 1)))
    return out


if __name__ == "__main__":
    for p in sys.argv[1:]:
        print(json.dumps(analyze(p)))
