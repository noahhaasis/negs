# Discourse benchmark graphics

Upload the PNG files directly to Discourse. Each is 1440 × 2472 pixels,
designed to remain readable at a displayed width of 360 pixels. SVGs are
included as editable originals. The graphics have no top heading; introduce
each one in the post.

- [Render time, 60 MP](render-time-60mp.png)
- [Peak memory, 60 MP](peak-memory-60mp.png)

Suggested text before the graphics:

> Full-resolution rendering of the same 60 MP photo (Leica Q3), using Kodak
> Gold 200 printed on Portra Endura, on an M1 Pro MacBook Pro with 32 GB RAM.
> Lower is better. The enlarged GPU detail uses a different scale.

> **Render time** — seconds spent rendering, excluding startup and file I/O.

> **Peak memory** — maximum program memory, including GPU buffers.
> \* The Python run swapped up to 7 GB to disk.

Follow the graphics with the processing-mode explanation from the
[README](../../README.md#speed) and a link to the
[benchmark method](../benchmarks.md). The CPU LUT row is a benchmark-only
comparison in Negs. These measurements compare processing paths and do not
demonstrate image accuracy or identical images across engines.

Regenerate from the README's Speed table with Python 3, `rsvg-convert`
(librsvg), and Noto Sans installed:

```sh
python3 scripts/benchmark/render_discourse_charts.py
```

For the optional 24 MP pair, add `--megapixels 24`.
