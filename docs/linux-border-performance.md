# Linux border selection performance

Measured on 2026-10-26, based on `37ba53b` plus working-tree changes, using
Rust 1.93.1 release builds. Real Hyprland/Wayland session, GTK Vulkan renderer,
1320 × 900 window, surface scale 1.25, Ryzen 5 3600. Existing OS/driver caches;
these measurements do not represent a cold filesystem or driver cache.

## Findings and changes

The canvas sent bundled frame PNGs through the photograph decoder with a 4096 px
limit. GdkPixbuf enlarged Film from 2518 × 1550 to 4096 × 2521 and full-frame
35mm from 3800 × 3500 to 4096 × 3773. That path also read metadata again,
copied the pixel buffer and calculated an unused histogram. Switching away from
a border discarded its texture, requiring all that work and another GPU upload
on the next selection. Rapid selections could start concurrent obsolete loads.

Borders now use the Rust PNG decoder at native resolution on a worker, with
owned bytes passed directly into an explicitly sRGB texture. An LRU cache
retains at most four textures and 96 MiB of pixel backing. GPU allocations are
additional to that backing budget. Only one load runs per canvas; its completion
checks the current selection and starts the latest pending request. Cached
textures remain immediately selectable while a different texture is loading.

Native-resolution RGBA backing is 14.9 MiB for Film and 50.7 MiB for full-frame
35mm, down from 39.4 MiB and 59.0 MiB respectively. Recently selected textures
remain resident, so total memory depends on cache contents.

## Measurements

Three fresh processes per version, five Film and five Portra 400 full-frame
35mm selections per process, including disabling/re-enabling. The benchmark
applies canvas state directly to isolate border loading from film simulation.
Times include worker scheduling and texture creation, and end when the texture
is available to the canvas. They exclude GPU upload completion and compositor
presentation. Worker completion is polled at 1 ms intervals; cache hits are
measured synchronously.

| Selection | Before, median | After, median |
| --- | ---: | ---: |
| Film, first in process | 219.298 ms | 30.477 ms |
| 35mm, first in process | 439.486 ms | 91.715 ms |
| Film, repeated | 234.209 ms | 0.020 ms |
| 35mm, repeated | 444.528 ms | 0.023 ms |

All 24 measured cached selections completed within 0.031 ms. A visible change
still waits for the next display frame. First 35mm selection ranged from
47.335–106.805 ms after the change. An evicted texture must be decoded again.

The existing 240-frame slider/zoom benchmark was already near 60 fps. One run
per border/version took 4.000 → 4.006 s for Film and 4.086 → 4.143 s for 35mm;
these runs establish no improvement in sustained frame rate. The demonstrated
improvement is selection latency. Film's nine-patch rendering remains intact,
including faint alpha details inside the frame.

Raw reports, original binaries and before/after widget captures are under
ignored `target/linux-validation/border-performance/`. The combined selection
data is in `comparison.json`; complete baseline runs are in
`before-selection-complete/`, and optimized runs in `after-selection/`.

## Validation and reproduction

- Release build, formatting and diff checks passed.
- All 20 ordinary Linux tests passed. These include all bundled border PNGs,
  native sizes, transparency, full/half gate geometry, generic-stock fallback,
  LRU eviction and cache limits.
- The additional explicit desktop regression passed: native PNG pixels match
  GdkPixbuf byte-for-byte, cached selections reuse texture identity, rapid
  selections publish the latest stock's pixels, full/half formats load, and
  stale completion cannot restore a disabled or unsupported border.
- Matching Film/35mm widget captures were visually reviewed. Frame placement,
  lettering, sprockets and photo placement remain consistent. Native sampling
  removes the previous intermediate upscale, so rendered screenshot pixels
  need not be identical.

```sh
bash linux/cargo.sh build --release --locked
bash linux/cargo.sh test --release --locked
source linux/desktop-env.sh
bash linux/cargo.sh test --release --locked \
  border_texture_reuse_and_latest_selection -- --ignored --test-threads=1
NEGS_UI_ONLY=1 python3 linux/measure_ui.py --border-benchmark --runs 3 \
  --inspector-mode lazy --size 1320,900 \
  --out target/linux-validation/border-performance/recheck-selection
NEGS_UI_ONLY=1 python3 linux/measure_ui.py --benchmark --scene post \
  --border-style 3 --runs 1 --size 1320,900 \
  --out target/linux-validation/border-performance/recheck-35mm
```

Initial sandboxed tests could not write their runtime fixtures; rerunning with
the required filesystem/process access passed. No film math, helper protocol or
export implementation changed; numerical engine baselines were not rerun for
this presentation-only change.

The tested UI binary was installed at `~/.local/lib/negs/negs` and its ownership
manifest updated. The previous binary is backed up at
`target/linux-validation/border-performance/negs-installed-before`. Existing
running instances use the previous binary until restarted. Package archives were
not regenerated.
