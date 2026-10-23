#!/usr/bin/env python3
"""Write a large deterministic linear float RGB TIFF using bounded RAM.

For GPU allocation/watchdog experiments, not photographic quality or timing
comparisons. The pattern includes bright edges, saturated colours and a ramp.
No imaging dependencies are needed on the Windows test machine.
"""

import argparse
import struct
from pathlib import Path


def write_tiff(path, width, height):
    count = 11
    extra = 8 + 2 + count * 12 + 4
    pixels = extra + 12  # BitsPerSample and SampleFormat: three SHORTs each
    size = width * height * 12
    if width < 1 or height < 1 or pixels + size >= 2**32:
        raise ValueError("positive dimensions fitting a classic TIFF are required")
    tags = [
        (256, 4, 1, width), (257, 4, 1, height), (258, 3, 3, extra),
        (259, 3, 1, 1), (262, 3, 1, 2), (273, 4, 1, pixels),
        (277, 3, 1, 3), (278, 4, 1, height), (279, 4, 1, size),
        (284, 3, 1, 1), (339, 3, 3, extra + 6),
    ]
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("wb") as f:
        f.write(struct.pack("<2sHIH", b"II", 42, 8, count))
        for tag in tags:
            f.write(struct.pack("<HHII", *tag))
        f.write(struct.pack("<I6H", 0, 32, 32, 32, 3, 3, 3))
        rows = []
        for bright in (False, True):
            row = bytearray()
            for x in range(width):
                ramp = x / max(1, width - 1)
                pixel = (40.0, 20.0, 10.0) if bright and (x // 64) % 2 else (ramp, ramp * 0.5, 0.18)
                row.extend(struct.pack("<3f", *pixel))
            rows.append(row)
        for y in range(height):
            f.write(rows[(y // 64) % 2])


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("width", type=int)
    parser.add_argument("height", type=int)
    args = parser.parse_args()
    write_tiff(args.output, args.width, args.height)
