"""Extract datasheet curves from vector PDFs (docs/adding-film-stocks.md D1-D3).

For each stock in the recipe's "extraction" block:
  * the PDF is identified by SHA-256 (refuses another revision);
  * each panel is found by its frame (a stroked 're' near the given rectangle);
  * its gridlines are the axis-parallel 'l' segments inside the frame; the spec lists
    the values they carry (in order), so a missing or extra line stops the run;
  * each axis is a least-squares line through its gridlines (PDF coordinates are
    unrotated: on a page with /Rotate 90 the printed x axis is PDF y). Gate: every
    gridline reproduces its value to <= 0.2 % of the axis span, and labelled frame
    edges land on their values within the same tolerance;
  * curves are stroked paths inside the frame picked by colour and dash state; Bezier
    segments are sampled at 64 points each (de Casteljau), line segments at their ends.

Options for charts drawn in one ink (the Fujifilm Professional Data Guide), all opt-in so
the earlier recipes extract exactly as before:
  * panel "frame_kind": "fill" takes the frame from a filled rectangle (the grey plot
    area) instead of a stroked one;
  * panel "grid_crossing": true also takes gridlines that run past the frame (ticks drawn
    as one line with the gridline), using the part inside it;
  * axis "relative_reason": a relative axis (e.g. "Relative Sensitivity (log)" with a
    one-unit scale bar) may be fitted through two gridlines; its offset is arbitrary and
    the reason must say what fixes the scale and why the offset does not matter;
  * curve "color": "any"; "dashes": the exact dash string, instead of "dashed";
  * curve "select": {"sort": "x_mid"|"y_mid", "index": k, "of": n}: the paths are split
    into their connected subpaths (one PDF path may hold all three curves), those
    matching the style are sorted by the mean of their PDF x or y, there must be n of
    them, and the k-th is taken. "sort": "x_at_min_y" orders them by the PDF x of their
    highest point on an unrotated page (a dye curve's peak);
  * curve "parts": [spec, ...] for a curve drawn in pieces, as where the PFDG draws one
    solid line once the three characteristic curves coincide: each part is selected like
    a curve (style, "select" or "paths"), parts after the first keep only their points
    beyond the end of the curve so far along the exposure axis's PDF coordinate, and a
    part with "until_next": true also stops where the next part starts. Each join (the
    distance from the end so far to the next part's path) must be within "join_tol" pt
    (default 0.5) and is reported.

Writes, per stock, <out>/<stock>/<curve>.csv (two columns, no header, as spektrafilm's
loader reads them), trace/<curve>.csv (data x, data y, pdf x, pdf y), extraction.json
(hashes, axis fits, residuals, point counts) and overlay PNGs of every panel with the
extracted points drawn back onto the page render.

Usage: python -I extract_pdf.py --recipe recipe.json --pdf-dir <folder> --out <packet dir>
         [--stock <id>] [--overlays <dir>]
"""
import argparse
import csv
import hashlib
import json
from pathlib import Path

import numpy as np
import pymupdf

LOG10_2 = 0.30103  # log10 H per camera stop (docs/adding-film-stocks.md D3; 0.30103 = log10 2 to 5 places)
N_BEZIER = 64


def bezier(p, n=N_BEZIER):
    t = np.linspace(0, 1, n)[:, None]
    return (1 - t) ** 3 * p[0] + 3 * (1 - t) ** 2 * t * p[1] + 3 * (1 - t) * t * t * p[2] + t ** 3 * p[3]


def sample_items(items):
    pts = []
    for it in items:
        kind = it[0]
        if kind == "l":
            seg = np.array([[it[1].x, it[1].y], [it[2].x, it[2].y]])
        elif kind == "c":
            seg = bezier(np.array([[v.x, v.y] for v in it[1:5]]))
        else:
            raise ValueError(f"unexpected path item {kind}")
        if pts and np.allclose(pts[-1][-1], seg[0], atol=1e-6):
            seg = seg[1:]
        pts.append(seg)
    return np.concatenate(pts)


def split_subpaths(items, tol=1e-3):
    """A drawing's items cut into runs where each item starts where the last one ended."""
    runs = []
    for it in items:
        start = it[1]
        if runs:
            prev = runs[-1][-1]
            end = prev[2] if prev[0] == "l" else prev[4]
            if abs(end.x - start.x) <= tol and abs(end.y - start.y) <= tol:
                runs[-1].append(it)
                continue
        runs.append([it])
    return runs


def find_frame(drawings, rect, tol=3.0, kinds=("s", "fs")):
    best = None
    for d in drawings:
        if d["type"] not in kinds or len(d["items"]) != 1 or d["items"][0][0] != "re":
            continue
        r = d["items"][0][1]
        err = max(abs(r.x0 - rect[0]), abs(r.y0 - rect[1]), abs(r.x1 - rect[2]), abs(r.y1 - rect[3]))
        if err < tol and (best is None or err < best[0]):
            best = (err, r)
    if best is None:
        raise ValueError(f"no frame near {rect}")
    return best[1]


def gridlines(drawings, frame, min_frac=0.3, crossing=False):
    """Axis-parallel stroked line segments inside the frame: {'x': [...], 'y': [...]}
    holding the constant coordinate of each (deduplicated to 0.05 pt). With crossing,
    a segment may run past the frame; its part inside must still be min_frac long."""
    found = {"x": [], "y": []}
    for d in drawings:
        if d["type"] not in ("s", "fs"):
            continue
        for it in d["items"]:
            if it[0] != "l":
                continue
            a, b = it[1], it[2]
            inside = all(frame.x0 - 0.6 <= p.x <= frame.x1 + 0.6 and frame.y0 - 0.6 <= p.y <= frame.y1 + 0.6 for p in (a, b))
            if crossing and not inside:
                if abs(a.x - b.x) < 0.02 and frame.x0 < a.x < frame.x1:
                    lo, hi = max(min(a.y, b.y), frame.y0), min(max(a.y, b.y), frame.y1)
                    if hi - lo >= min_frac * frame.height:
                        found["x"].append((a.x + b.x) / 2)
                elif abs(a.y - b.y) < 0.02 and frame.y0 < a.y < frame.y1:
                    lo, hi = max(min(a.x, b.x), frame.x0), min(max(a.x, b.x), frame.x1)
                    if hi - lo >= min_frac * frame.width:
                        found["y"].append((a.y + b.y) / 2)
                continue
            if not inside:
                continue
            if abs(a.x - b.x) < 0.02 and abs(a.y - b.y) >= min_frac * frame.height:
                found["x"].append((a.x + b.x) / 2)
            elif abs(a.y - b.y) < 0.02 and abs(a.x - b.x) >= min_frac * frame.width:
                found["y"].append((a.y + b.y) / 2)
    out = {}
    for k, v in found.items():
        v = sorted(v)
        dedup = []
        for x in v:
            if not dedup or abs(x - dedup[-1]) > 0.05:
                dedup.append(x)
        edge = (frame.x0, frame.x1) if k == "x" else (frame.y0, frame.y1)
        out[k] = [x for x in dedup if min(abs(x - edge[0]), abs(x - edge[1])) > 0.5]
    return out


def fit_axis(spec, grid, frame):
    """spec: {"pdf": "x"|"y", "gridlines": [values in increasing PDF coordinate],
    "frame": [value at lower PDF edge, value at upper PDF edge] or null}."""
    pos = np.array(grid[spec["pdf"]])
    vals = np.array(spec["gridlines"], float)
    if pos.size != vals.size:
        raise ValueError(f"axis on pdf {spec['pdf']}: found {pos.size} gridlines {np.round(pos, 2).tolist()}, spec lists {vals.size}")
    edges = (frame.x0, frame.x1) if spec["pdf"] == "x" else (frame.y0, frame.y1)
    fit_pos, fit_vals = pos, vals
    if spec.get("frame") is not None:  # labelled frame edges are ticks too
        fit_pos, fit_vals = np.concatenate([pos, edges]), np.concatenate([vals, spec["frame"]])
    if fit_pos.size < (2 if spec.get("relative_reason") else 3):
        raise ValueError(f"axis on pdf {spec['pdf']}: fewer than 3 labelled positions")
    A = np.column_stack([fit_pos, np.ones_like(fit_pos)])
    (a, b), *_ = np.linalg.lstsq(A, fit_vals, rcond=None)
    span = abs(a * (edges[1] - edges[0]))
    res = {"pdf": spec["pdf"], "slope": a, "offset": b, "span": span,
           "gridline_pdf": pos.round(4).tolist(), "gridline_residual": (a * pos + b - vals).round(6).tolist(),
           "frame_pdf": list(edges), "frame_value": [a * e + b for e in edges]}
    worst = float(np.max(np.abs(a * pos + b - vals))) / span
    if spec.get("frame") is not None:
        fr = np.array(spec["frame"], float)
        res["frame_residual"] = (np.array(res["frame_value"]) - fr).round(6).tolist()
        worst = max(worst, float(np.max(np.abs(np.array(res["frame_value"]) - fr))) / span)
    res["worst_over_span"] = worst
    # "gate": a recorded exception for a hand-drafted chart whose own gridlines are
    # uneven (no straight line reproduces them); it needs "gate_reason" and the physical
    # check that justified it. Default is the method's 0.2 % of span.
    gate = spec.get("gate", 0.002)
    if gate != 0.002 and not spec.get("gate_reason"):
        raise ValueError(f"axis on pdf {spec['pdf']}: a gate other than 0.2 % needs a gate_reason")
    res["gate"], res["pass"] = gate, worst <= gate
    if spec.get("gate_reason"):
        res["gate_reason"] = spec["gate_reason"]
    if spec.get("relative_reason"):
        res["relative_reason"] = spec["relative_reason"]
    return res


def colour_match(c, want, tol=0.08):
    if want == "any":
        return c is not None
    return c is not None and max(abs(a - b) for a, b in zip(c, want)) <= tol


def style_match(d, cs):
    dashed = bool(d.get("dashes")) and d["dashes"].strip() not in ("[] 0", "[]0")
    if "dashes" in cs:
        want = cs["dashes"]
        style = (d.get("dashes") or "").strip() == want or (want in ("[] 0", "") and not dashed)
    else:
        style = dashed == cs.get("dashed", False)
    return colour_match(d.get("color"), cs["color"]) and style


def select_subpath(drawings, frame, name, cs):
    sel = cs["select"]
    cands = []
    for i, d in enumerate(drawings):
        if d["type"] not in ("s", "fs") or not style_match(d, cs):
            continue
        for j, run in enumerate(split_subpaths(d["items"])):
            if not any(it[0] == "c" for it in run):
                continue
            pts = sample_items(run)
            if not (pts[:, 0].min() >= frame.x0 - 1 and pts[:, 0].max() <= frame.x1 + 1
                    and pts[:, 1].min() >= frame.y0 - 1 and pts[:, 1].max() <= frame.y1 + 1):
                continue
            cands.append(((i, j), pts))
    if len(cands) != sel["of"]:
        raise ValueError(f"{name}: found {len(cands)} matching subpaths {[k for k, _ in cands]}, expected {sel['of']}")
    if sel["sort"] == "x_at_min_y":
        cands.sort(key=lambda c: c[1][np.argmin(c[1][:, 1]), 0])
    else:
        axis = {"x_mid": 0, "y_mid": 1}[sel["sort"]]
        cands.sort(key=lambda c: c[1][:, axis].mean())
    key, pts = cands[sel["index"]]
    return [(f"{key[0]}.{key[1]}", pts)]


def select_paths(drawings, frame, name, cs):
    if "select" in cs:
        return select_subpath(drawings, frame, name, cs)
    paths = []
    for i, d in enumerate(drawings):
        if d["type"] not in ("s", "fs") or not any(it[0] == "c" for it in d["items"]):
            continue
        r = d["rect"]
        if not (r.x0 >= frame.x0 - 1 and r.x1 <= frame.x1 + 1 and r.y0 >= frame.y0 - 1 and r.y1 <= frame.y1 + 1):
            continue
        if style_match(d, cs):
            paths.append((i, sample_items(d["items"])))
    if len(paths) != cs.get("paths", 1):
        raise ValueError(f"{name}: found {len(paths)} matching paths (indices {[i for i, _ in paths]}), expected {cs.get('paths', 1)}")
    return paths


def join_parts(drawings, frame, name, cs, k):
    """A curve drawn in pieces (curve "parts", see the module docstring). k is the PDF
    coordinate (0 x, 1 y) along which the exposure axis runs."""
    sel = [select_paths(drawings, frame, f"{name} part {n}", ps) for n, ps in enumerate(cs["parts"])]
    ids = [f"part{n}:" + "+".join(str(i) for i, _ in s) for n, s in enumerate(sel)]
    parts = [np.concatenate([p for _, p in s]) for s in sel]
    parts = [p[np.argsort(p[:, k], kind="stable")] for p in parts]
    tol = cs.get("join_tol", 0.5)
    out, joins = [(ids[0], parts[0])], []
    for n in range(1, len(parts)):
        end = out[-1][1][-1]
        p = parts[n]
        gap = float(np.min(np.hypot(*(p - end).T)))
        if gap > tol:
            raise ValueError(f"{name}: part {n} starts {gap:.3f} pt from the end of part {n - 1}, over {tol} pt")
        keep = p[:, k] > end[k]
        if cs["parts"][n].get("until_next"):
            if n + 1 == len(parts):
                raise ValueError(f"{name}: part {n} has until_next but is the last part")
            keep &= p[:, k] < parts[n + 1][:, k].min()
        if not keep.any():
            raise ValueError(f"{name}: part {n} has no points beyond the end of part {n - 1}")
        out.append((ids[n], p[keep]))
        joins.append(round(gap, 4))
    return out, joins


def extract_panel(page, drawings, panel):
    frame = find_frame(drawings, panel["frame"], kinds=("f", "fs") if panel.get("frame_kind") == "fill" else ("s", "fs"))
    grid = gridlines(drawings, frame, crossing=panel.get("grid_crossing", False))
    axes = {k: fit_axis(panel["axes"][k], grid, frame) for k in ("x", "y")}
    curves = {}
    for name, cs in panel["curves"].items():
        joins = None
        if "parts" in cs:
            paths, joins = join_parts(drawings, frame, name, cs, 0 if axes["x"]["pdf"] == "x" else 1)
        else:
            paths = select_paths(drawings, frame, name, cs)
        pdf = np.concatenate([p for _, p in paths])
        x = axes["x"]["slope"] * pdf[:, 0 if axes["x"]["pdf"] == "x" else 1] + axes["x"]["offset"]
        y = axes["y"]["slope"] * pdf[:, 0 if axes["y"]["pdf"] == "x" else 1] + axes["y"]["offset"]
        order = np.argsort(x, kind="stable")
        x, y, pdf = x[order], y[order], pdf[order]
        keep = np.concatenate([[True], np.diff(x) > 1e-9])
        x, y, pdf = x[keep], y[keep], pdf[keep]
        curves[name] = {"x": x, "y": y, "pdf": pdf, "paths": [i for i, _ in paths]}
        if joins is not None:
            curves[name]["joins_pt"] = joins
    return frame, axes, curves


def convert(name, x, y, conv):
    if conv == "stops_to_log10H":
        return x * LOG10_2, y
    if conv is None:
        return x, y
    raise ValueError(conv)


def write_csv(path, rows, header=None):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", newline="") as f:
        w = csv.writer(f, lineterminator="\n")
        if header:
            w.writerow(header)
        for r in rows:
            w.writerow([repr(float(v)) for v in r])


def overlay(page, frame, curves, path, zoom=4.0):
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    clip = pymupdf.Rect(frame.x0 - 25, frame.y0 - 25, frame.x1 + 25, frame.y1 + 25)
    m = page.rotation_matrix
    dclip = clip * m
    pix = page.get_pixmap(matrix=pymupdf.Matrix(zoom, zoom), clip=dclip)
    img = np.frombuffer(pix.samples, np.uint8).reshape(pix.height, pix.width, pix.n)
    fig, ax = plt.subplots(figsize=(pix.width / 100, pix.height / 100), dpi=100)
    ax.imshow(img[:, :, :3], extent=(dclip.x0, dclip.x1, dclip.y1, dclip.y0))
    for name, c in curves.items():
        p = np.array([[*(pymupdf.Point(*q) * m)] for q in c["pdf"][::3]])
        ax.plot(p[:, 0], p[:, 1], ".", ms=1.2, color="black")
    ax.set_axis_off()
    fig.savefig(path, bbox_inches="tight", pad_inches=0)
    plt.close(fig)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--recipe", required=True)
    ap.add_argument("--pdf-dir", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--stock")
    ap.add_argument("--overlays")
    a = ap.parse_args()
    recipe = json.loads(Path(a.recipe).read_text())
    for stock, spec in recipe["extraction"].items():
        if a.stock and stock != a.stock:
            continue
        src = spec["source"]
        pdf_path = Path(a.pdf_dir) / src["file"]
        digest = hashlib.sha256(pdf_path.read_bytes()).hexdigest()
        if digest != src["sha256"]:
            raise SystemExit(f"{stock}: {src['file']} is not the pinned revision ({digest})")
        doc = pymupdf.open(pdf_path)
        if doc.needs_pass:
            doc.authenticate("")
        report = {"stock": stock, "source": src, "panels": {}}
        out = Path(a.out) / stock
        for pname, panel in spec["panels"].items():
            page = doc[panel["page"]]
            drawings = page.get_drawings()
            frame, axes, curves = extract_panel(page, drawings, panel)
            rep = {"page": panel["page"], "page_rotation": page.rotation, "frame_pdf": list(frame), "axes": axes,
                   "conversion": panel.get("conversion"), "curves": {}}
            for name, c in curves.items():
                x, y = convert(name, c["x"], c["y"], panel.get("conversion"))
                write_csv(out / f"{name}.csv", np.column_stack([x, y]))
                write_csv(out / "trace" / f"{name}.csv", np.column_stack([x, y, c["pdf"]]),
                          header=["x", "y", "pdf_x_pt", "pdf_y_pt"])
                rep["curves"][name] = {"paths": c["paths"], "points": int(x.size), "x_range": [float(x[0]), float(x[-1])],
                                       "y_range": [float(np.min(y)), float(np.max(y))]}
                if "joins_pt" in c:
                    rep["curves"][name]["joins_pt"] = c["joins_pt"]
            report["panels"][pname] = rep
            if a.overlays:
                Path(a.overlays).mkdir(parents=True, exist_ok=True)
                overlay(page, frame, curves, Path(a.overlays) / f"{stock}-{pname}.png")
            print(stock, pname, "axes", {k: (round(v["worst_over_span"] * 100, 4), v["pass"]) for k, v in axes.items()},
                  {n: c["points"] for n, c in rep["curves"].items()})
        report["pass"] = all(ax["pass"] for p in report["panels"].values() for ax in p["axes"].values())
        (out / "extraction.json").write_text(json.dumps(report, indent=1, default=float) + "\n")
        (out / "info.txt").write_text(spec["info"])
        if not report["pass"]:
            raise SystemExit(f"{stock}: axis calibration gate failed, see {out / 'extraction.json'}")


if __name__ == "__main__":
    main()
