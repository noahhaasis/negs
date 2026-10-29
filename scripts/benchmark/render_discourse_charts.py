#!/usr/bin/env python3
"""Make phone-readable benchmark PNGs and SVGs from the README's Speed table.

Requires rsvg-convert (librsvg) and Noto Sans. Run from any directory:
    python3 scripts/benchmark/render_discourse_charts.py [--megapixels 24]
"""

import argparse
import html
from pathlib import Path
import re
import subprocess


ROOT = Path(__file__).resolve().parents[2]
WIDTH = 720
HEIGHT = 1236
LEFT = 38
RIGHT = WIDTH - LEFT
TRACK = RIGHT - LEFT
ORANGE = "#c85331"
OLIVE = "#747d66"
INK = "#272b25"
MUTED = "#62685b"
LABELS = [
    ("Negs", "GPU · full-resolution render", True),
    ("spektrafilm-rs", "GPU · full-resolution render", False),
    ("Negs Reference", "CPU · 64-bit", True),
    ("Negs Reference", "CPU · Use less memory", True),
    ("Negs", "CPU · 64-bit · LUTs", True),
    ("spektrafilm-rs", "CPU · 64-bit · LUTs", False),
    ("Python spektrafilm", "CPU · 64-bit · LUTs", False),
]


def measurements(megapixels):
    speed = (ROOT / "README.md").read_text().split("## Speed\n", 1)[1].split("\n## ", 1)[0]
    values = []
    column = 0 if megapixels == 24 else 1
    for line in speed.splitlines():
        matches = re.findall(r"([\d.]+) s · ([\d.]+) GB", line)
        if matches:
            values.append(tuple(map(float, matches[column])))
    if len(values) != len(LABELS):
        raise ValueError("Expected seven benchmark rows in the README Speed table")
    return values


def render(values, metric, megapixels):
    unit = "s" if metric == 0 else "GB"
    maximum = (120 if megapixels == 60 else 50) if metric == 0 else (30 if megapixels == 60 else 15)
    detail_max = (5 if megapixels == 60 else 1.5) if metric == 0 else (10 if megapixels == 60 else 5)
    metric_name = "Render time" if metric == 0 else "Peak memory"
    elements = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{HEIGHT}" viewBox="0 0 {WIDTH} {HEIGHT}" role="img" aria-labelledby="title desc">',
        f'<title id="title">{metric_name}: {megapixels} MP benchmark</title>',
        '<desc id="desc">M1 Pro MacBook Pro, 32 GB RAM. Full-resolution renders of the same photo with Kodak Gold 200 and Portra Endura. Lower is better. All seven processing modes, followed by an enlarged GPU comparison. '
        + html.escape("; ".join(f"{name}, {mode}: {values[i][metric]:.1f} {unit}" for i, (name, mode, _) in enumerate(LABELS)))
        + '</desc>',
        '<rect width="100%" height="100%" fill="#fdfcf8"/>',
        '<g font-family="Noto Sans, sans-serif" font-weight="700">',
    ]

    def text(x, y, content, size=27, color=INK, anchor="start"):
        elements.append(f'<text x="{x}" y="{y}" font-size="{size}" fill="{color}" text-anchor="{anchor}">{html.escape(content)}</text>')

    def line(x1, y1, x2, y2, color, width=1.5):
        elements.append(f'<line x1="{x1:.2f}" y1="{y1}" x2="{x2:.2f}" y2="{y2}" stroke="{color}" stroke-width="{width}" stroke-linecap="round"/>')

    def grid(top, bottom):
        for i in range(5):
            x = LEFT + TRACK * i / 4
            line(x, top, x, bottom, "#e3e2d8")

    def ticks(y, limit):
        for i in range(5):
            x = LEFT + TRACK * i / 4
            value = limit * i / 4
            label = f"{value:g}"
            text(x, y, label + (f" {unit}" if i == 4 else ""), 23, MUTED,
                 "start" if i == 0 else "end" if i == 4 else "middle")

    def row(index, top, limit):
        name, mode, is_negs = LABELS[index]
        value = values[index][metric]
        color = ORANGE if is_negs else OLIVE
        label_color = ORANGE if is_negs else INK
        # Text sits above its line, so long labels never collide with a marker.
        text(LEFT, top + 29, name, 28, label_color)
        text(RIGHT, top + 29, f"{value:.1f} {unit}" + ("*" if metric == 1 and megapixels == 60 and index == 6 else ""), 29, label_color, "end")
        text(LEFT, top + 60, mode, 23, MUTED)
        y = top + 83
        end = LEFT + TRACK * value / limit
        line(LEFT, y, RIGHT, y, "#e8e8df", 3)
        line(LEFT, y, end, y, color, 7)
        elements.append(f'<circle cx="{end:.3f}" cy="{y}" r="8" fill="{color}"/>')

    grid(29, 772)
    for i in range(7):
        row(i, 14 + i * 105, maximum)
    ticks(806, maximum)

    elements.append('<rect x="18" y="846" width="684" height="352" rx="12" fill="#f7efe4" stroke="#e4d0b9" stroke-width="2"/>')
    text(LEFT, 886, "GPU detail · enlarged scale", 25)
    grid(908, 1138)
    row(0, 908, detail_max)
    row(1, 1023, detail_max)
    ticks(1173, detail_max)
    elements.append('</g></svg>')
    return "\n".join(elements) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--megapixels", type=int, choices=(24, 60), default=60)
    args = parser.parse_args()
    values = measurements(args.megapixels)
    destination = ROOT / "docs" / "discourse"
    destination.mkdir(parents=True, exist_ok=True)
    for metric, slug in enumerate(("render-time", "peak-memory")):
        svg = destination / f"{slug}-{args.megapixels}mp.svg"
        png = svg.with_suffix(".png")
        svg.write_text(render(values, metric, args.megapixels))
        subprocess.run(["rsvg-convert", "--width", str(WIDTH * 2), "--height", str(HEIGHT * 2), "--output", str(png), str(svg)], check=True)
        print(png.relative_to(ROOT))


if __name__ == "__main__":
    main()
