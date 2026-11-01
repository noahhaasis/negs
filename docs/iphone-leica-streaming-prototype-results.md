# Bounded Leica RAW prototype: results (2026-10-30)

**Later follow-up, now paused:** see [the regression handoff](iphone-leica-bounded-regression-handoff.md) for genuine original-versus-current production timing and WASM pixel comparisons. Typical 24 MP native times were essentially unchanged, but latency tails remain unresolved. Further phone testing was stopped at the user's request after device/automation problems. The results below describe the earlier experiment, not a completed new A/B matrix.

## Decision: useful decode improvement, **not an iPhone reliability pass**

The actual Leica Q3 RAW (`~/Downloads/3980774776.dng`, SHA-256
`bfb398233396e5f9c6c3b3b4069712609bdc913583d43d1e0427effbb2921d2e`)
now has an experimental Bayer development path that never makes its full-size
RGB image. Final native output and single-thread WASM output are **bit-identical**
to legacy development followed by the existing area downsampler.

- Native peak above the file buffer: **999 → 307 MiB** (−69%).
- Physical iPhone, four-thread decoder WASM high-water: **1329 → 543 MiB** (−59%).
- Final phone build: **3 of 4** fresh-tab Leica sequences completed open → reopen
  → working-copy zoom/edit → export → edit → reopen twice → Original view.
  **The fourth was killed during the zoom edit**, confirmed by syslog at
  **1635 MiB**. Earlier versions/runs failed too; they are not discarded.
- A completed sequence still reached **1916 MiB WebContent footprint during
  the zoom edit**, with only 211–217 MiB reported by the editing engine's WASM.
  There is not substantial headroom below the observed 1.5 GiB soft limit.

**Keep the route opt-in.** This does not establish reliable Leica operation under
phone memory pressure, even though several real-file sequences now finish. It
does not implement full-resolution browser export. The successful Leica JPEGs
were the existing **4246 × 2826 working-copy exports**, each 2,765,063 bytes,
not 60 MP exports. No embedded JPEG was substituted for RAW development.

See [the original failure](iphone-leica-60mp.md),
[the implementation request](iphone-leica-streaming-prototype-handoff.md), and
[device testing](apple-device-testing.md).

## Implementation and routing

- `crates/spektrafilm-io/src/bounded.rs`: dedicated opt-in API returning the
  small unoriented image, full cropped dimensions, EXIF orientation, actual
  route, and sensor/develop timings. RAW decoding happens only once, including
  on the small-file legacy route. The copied `RawSource` is dropped after
  decoding and reading orientation, before development/resampling.
- The Q3's **whole u16 mosaic is still decompressed**. Its single lossless-JPEG
  strip is not treated as independently decodable tiles. Unlike prototype v1,
  the final version does **not** make a full-height scaled f32 mosaic either.
- Scale original sensor-width **even row pairs** with rawler's existing kernel,
  then run its PPG demosaicer on full-active-width, overlapping horizontal
  bands. Preserve the original CFA phase and active/default crop coordinates.
  Reuse Negs's existing calibration/highlight desaturation and WB functions.
- Bands normally cover 32 target rows, additionally capped at 128 contributing
  source rows, with 16-row halos. Keep only the band RGB plus the final small
  output. The reducer uses precisely the existing fractional spans and
  **vertical f32 accumulation before horizontal accumulation**.
- The 16-row halo was tested, **not minimized or claimed to be the minimum**.
  An extreme reduction where a single target row spans >128 source rows is
  explicitly rejected in this prototype; it cannot accidentally allocate a
  full-height RGB band for a 60 MP → 1×1 request. A future fully incremental
  reducer can remove that restriction.
- `web/app.js` → `web/decoder.js` → WASM `decodeBounded` is selected only by
  **`?rawDevelop=bounded`**, on the existing capped/short-lived-worker path.
  The large-file threshold is **>40 MP**, using sensor size as well as crop size
  so a tiny default crop cannot conceal a costly sensor/active-area develop.
- **`?rawDevelop=legacy` or no switch** uses the original route.
  **`?rawDevelop=force`** also bands small supported Bayer RAWs, for benchmarks.
  Decoder logs report **the actual route**, not just the requested flag.
- Ordinary 24 MP Bayer, 24 MP linear DNG, and 26 MP X-Trans **bypass banding**
  with `bounded`. Selected unsupported large sensors/metadata, or unsupported
  forced inputs, return an error rather than falling back to full-size RGB.
  The initial sensor decode still allocates memory: this is not an arbitrary
  RAW-file memory-admission guarantee.
- JPEG stays on its raster path. Uncapped desktop (`maxPixels=0`), normal
  `Engine::load`, and native defaults do not opt in. The shared legacy IO
  developer was extracted into `develop_decoded`; its operations are unchanged.
  The existing core `preview::downsample_area` was **not refactored**.

## Pixel correctness and tests

`cargo test --release -p spektrafilm-io` passed **102 tests**, with two existing
fixture-dependent tests ignored. Targeted core preview tests passed **9 tests**.
JS syntax checks and `git diff --check` passed. Both WASM packages were rebuilt
with `PATH="$PWD/target/ipad-tools/bin:$PATH" bash web/build.sh --threads`.

Synthetic tests compare full development with banded development for RGGB,
BGGR, GBRG and GRBG; odd/even and small dimensions; nonzero active/default
crops; true sensor borders; integer and float mosaics; band heights 1, 7, 16,
32 and 128; multiple target sizes; WB modes/custom tint; clipped/black samples;
and EXIF orientations 1–8 applied after reduction. They preserve even rawler's
legacy **unscaled odd trailing row/column** behavior rather than silently
"fixing" it. Invalid crops, linear/4-colour/X-Trans forced inputs and extreme
reduction ratios have explicit rejection coverage.

Real-file comparisons use the existing core downsampler as the independent
reference. Every output scalar was finite and **zero scalars changed**;
max/mean absolute error were both **0**, including all Q3 stripe/crop-edge rows.
Per-row error JSON is saved in the ignored measurement directory.

| Input | Full cropped size, upright | Kept size, upright | Actual `bounded` route | Native kept-pixel FNV-1a |
|---|---|---|---|---|
| Q3 60 MP | 9520 × 6336 | 4246 × 2826 | Banded | `7e9100fdf59f0cd4` |
| Sony Bayer 24 MP | 4000 × 6000 | 2828 × 4242 | Legacy | `4ac42801ae390048` |
| Linear DNG 24 MP | 6048 × 4032 | 4242 × 2828 | Legacy | `2b7341c183ffd20b` |
| Fuji X-Trans 26 MP | 6240 × 4160 | 4242 × 2828 | Legacy | `3bcdb13ff4867bb4` |

`web/checks/bounded-pixels.mjs` also compared actual **single-thread WASM**
`decode` and `decodeBounded` pixels, with **bitwise zero differences** on all
four RAWs, a JPEG control, and force-banded Sony. That test is not a threaded
phone pixel comparison; the phone checked presented frames, editing, Original
and JPEG completion, not every source scalar through WebDriver.

## Native A/B: same cap, production allocator, four rayon threads

Host: MacBookPro18,3, 10 logical CPUs, rustc/cargo 1.93.1; exact toolchain/hashes in the saved identities.
Each file was buffered once. Alternate legacy/bounded order, one warmup per
route, then **20 warm measurements per route**. The filesystem was warm; these
are not disk-cold numbers. p95 is nearest-rank of those 20 observations, not a
population tail estimate. Separate fresh-process counting-allocator runs
(**three per cell**) measured peak heap. Do not substitute allocator-instrumented
latency for the production timing below. Earlier contended runs remain archived.

| Input | Native peak above file, legacy → prototype (MiB) | Warm decode+reduce+orient median, legacy → prototype (ms) | p95, legacy → prototype (ms) |
|---|---:|---:|---:|
| Q3 | 999 → **307** | 1064 → **1246** | 1149 → 1329 |
| Sony 24 MP, banding bypassed | 419 → 419 | 271 → 269 | 281 → 273 |
| Linear DNG 24 MP, bypassed | 568 → 565 | 139 → 139 | 146 → 143 |
| X-Trans 26 MP, bypassed | 461 → 441 | 431 → 431 | 889 → 468 |

The Q3 is **17% slower** by warm median in this native run. Small-file medians
were essentially unchanged; source-copy lifetime can reduce a control's heap
peak even when banding is bypassed. The X-Trans tails had large outliers (up to
999 ms legacy / 714 ms prototype); don't interpret its p95 as a speedup.
The earlier 580 MiB Sony full-decode baseline includes full-size orientation;
this harness shrinks **before** orientation, matching the actual phone path.
It is a different measurement, not a claimed 580 → 419 optimization here.

## Physical iPhone results

USB iPhone14,3, UDID `00008110-000240403AD1801E`, iOS 26.6.2 (23G90),
Safari 26.6.1, trusted HTTPS, secure and cross-origin-isolated, WebGPU,
**four WASM threads**, coarse pointer and 12 MP cap. Fresh automation tab per
sequence; reopens happen **in the same tab**. URL imports, **not Files-picker
imports**. The asset server uses `Cache-Control: no-store`; before/after asset
hashes and matching package builds were saved. The final phone run logged
`RAW develop: bounded`, 543 MiB decoder memory, 9520 × 6336 photo dimensions,
1212 × 807 fitted frames and 4246 × 2826 presented zoom frames.

### Repetition outcomes (do not discard failed runs)

| Build / run group | Outcome |
|---|---|
| Original baseline `baseline-1` | First two opens, zoom/edit and export passed; **jetsam on reopen after export**, PID 2659, 2136 MiB at kill |
| Original baseline `baseline-2` | **Jetsam on first open**, PID 2682, 1782 MiB |
| v1, full f32 mosaic retained, `bounded-4` | Opens/zoom/Original/export passed; **jetsam reopening after export**, PID 2761, 1539 MiB |
| v2 morning `v2-bounded-1` | Opens/zoom/Original passed; **jetsam at export**, PID 2786, 1691 MiB |
| v2 morning `v2-bounded-2` | Open/reopen/zoom edit passed; lost window returning to fit; **failed, cause not established by captured syslog** |
| Final evening `resume/q3-bounded-1`, `-3`, `q3-measured` | **All three full sequences passed**, including two post-export reopens and Original view |
| Final evening `resume/q3-bounded-2` | **Jetsam during zoom edit**, PID 4037, 1635 MiB |
| Matching rebuilt legacy `resume/q3-legacy-1` | Open/reopen/zoom/export passed; **jetsam reopening after export**, PID 4029, 1610 MiB |

v2/final use the same Q3 pixel kernel and 543 MiB phone high-water. Later safety
checks bound extreme reduction ratios and prevent large sensors with tiny crops
from entering legacy; they do not change the tested Q3 path. Locked-phone,
disconnection and controller-timeout attempts are preserved separately, not
counted as passes or automatically called jetsam.

### Footprint: decoder savings do not bound zoom/export memory

The final `resume/q3-measured` run completed without a kill; its correct
WebContent PID was **4515**. Approximate sampled peaks (MiB):

| Stage | Original baseline `baseline-1` | Final measured bounded run |
|---|---:|---:|
| First open | 1719 | **926** |
| Second open | 1938 | **1062** |
| Zoom/edit | 1343 | **1916** |
| 12 MP export | 1372 | **1456** |
| First reopen after export | Killed, 2136 at kill | **1400**, passed |
| Next reopen | Not reached | **1436**, passed |
| Decoder WASM high-water | **1329** | **543** |
| Editing-engine WASM, separate from decoder/GPU | 211 | 211 (217 in other completed runs) |

These are **individual runs, not paired peak medians**; memory pressure and
WebKit reclamation varied markedly. Sysmon samples were approximately 0.5 s
apart and can miss brief spikes. Some evening sysmon/syslog streams stalled:
those runs have **missing footprint data**, not zero usage. Restarting the
capture produced the complete final PID 4515 sequence above. OS kill figures
come from syslog, not interpolation of samples. Neither 1.5 GiB nor 2.1 GiB is
an invariant kill threshold.

### Phone latency and ordinary 24 MP controls

App `timeline` measures import → first completed frame; it excludes URL
fetching. Driver `step` time includes fetching 70 MiB over the LAN and polling
and is deliberately not used as decode/open latency. Cold below means a fresh
tab/module/pipeline, **not cold filesystem or cold browser HTTP caches**.

| Q3, final evening builds | Worker decode (ms) | Import → first frame (ms) |
|---|---:|---:|
| Legacy fresh first open (one completed) | 1792 | 5334 |
| Legacy reopen (one completed before later kill) | 1694 | 2220 |
| Bounded fresh first opens, n=4 | median **2132**, range 2029–2215 | median **4992**, range 4250–5435 |
| Bounded subsequent completed opens, n=10 | median **2070**, range 1954–2143 | median **2603**, range 2458–2665 |

This suggests slower per-open RAW work on the phone too, but only one matching
legacy sequence completed its first two opens before being killed. It is not a
well-powered latency comparison. Original pre-edit baseline worker times were
2039 / 2158 ms; don't mix these with the evening samples to manufacture a speedup.

**Normal 24 MP was not routed through the banded kernel.** Both controls were
opened on the actual phone before edits and on the rebuilt default/forced-off
route, as well as with the opt-in flag and banding bypassed:

| Phone control | Original baseline worker times, 4 opens (ms) | Rebuilt legacy completed opens (ms) | `bounded`, actual legacy completed opens (ms) | Phone decoder WASM |
|---|---|---|---|---:|
| Sony Bayer 24 MP | 868, 727, 781, 711 | 985, 817 | 1983 | 524 MiB in both modes |
| Linear DNG 24 MP | 574, 610, 569, 548 | 999, 895, 800, 787 | 669, 651, 579, 609 | 604 MiB in both modes |

The rebuilt linear sequences both completed zoom/export/two reopens/Original;
their first-frame times were 3894 vs 3860 ms fresh, then 1288–1460 vs 1008–1334 ms.
Sony's rebuilt legacy run **was killed at export** (PID 4046, 1607 MiB), and the
flagged run **lost its window on reopen**, without sufficient captured evidence
to classify the cause. Original Sony and linear baseline sequences passed;
original sampled WebContent peaks were 1422 and 1469 MiB respectively. Earlier
rebuilt Sony repetitions also include a passing full sequence.

These pressure-sensitive, incomplete phone controls **do not establish zero
24 MP performance hit**. Native medians show essentially unchanged performance
between the rebuilt legacy and flagged-but-bypassed routes; they do **not**
isolate the shared legacy extraction against an old production-allocator binary.
Original-versus-rebuilt phone timings varied substantially, so default 24 MP
performance remains inconclusive despite unchanged routing. Ordinary 24 MP
stability still needs work. Do not present only the faster flagged linear
samples or the successful Sony repetition.

## Next step: retained rendering/open working set, not another full RGB fix

The bounded decoder is a real win, but **zoom alone still reaches 1.9 GiB**
after the decoder has ended. The engine WASM metric stays around 211–217 MiB;
it does not account for JS heaps, GPU buffers/textures, delayed WebKit
reclamation, file/placeholder copies or the decoder. Remaining work should
measure those separately, rather than infer live memory from WASM high-water.

1. Inspect/reduce the touch GPU image-set pool (currently a 512 MiB pool limit)
   and overlap of old zoom/export resources with new renders. Measure actual
   destruction/reclamation on Safari, including repeat cycles under pressure.
2. The short-lived-worker route currently calls `loadDecoded`/`forget` **after**
   RAW decoding. Consider forgetting old GPU inputs/frames **before** starting
   the next decoder, with explicit failed-load restoration, then A/B it. Freeing
   Rust allocations alone does not shrink WASM or guarantee physical reclamation.
3. Profile remaining file and pixel copies across page → decoder → engine;
   sensor decompression is still whole-frame. Optimize transfers/lifetimes only
   with live/process measurements and a recovery plan for failed imports.
4. Repeat the default 24 MP controls, threaded/single-thread and CPU/GPU as
   separate experiments. Do not infer CPU or iPad safety from these WebGPU runs.
5. **Manual iPhone Downloads/Files import is still untested**, as are native
   pinch/rotation/backgrounding in this experiment. Complete them separately.
6. If headroom remains inadequate, preflight sensor metadata and offer a clear
   user-selected camera-JPEG preview mode or warning. Never describe a jetsam
   as a catchable decode error, and never label the JPEG fallback developed RAW.

## Reproduction and artifacts

All logs/build caches/fixture links are ignored, under `target/q3-bounded/`.
Original asset snapshots and identities are in `baseline-assets/` and
`baseline-identity.txt`; v1 in `v1/`; final matching JS/WASM SHA-256 values in
`resume/identity.txt`. `resume/timing.jsonl`, `peaks.jsonl`,
`pixels-native.jsonl`, `pixels-wasm.jsonl` and `*-rows.json` contain final native
and numeric results. Phone sessions each have `events.jsonl` and driver logs.
Both early and resumed syslog/footprint captures and their stalls are retained.
The earlier investigation in `target/q3-iphone/` is untouched.

```sh
# Production timing and separate allocator peak / exact pixel comparison:
cargo build --release -p spektrafilm-core --example bounded_timing --example bounded_decode
RAYON_NUM_THREADS=4 target/release/examples/bounded_timing ~/Downloads/3980774776.dng 21
RAYON_NUM_THREADS=4 target/release/examples/bounded_decode legacy ~/Downloads/3980774776.dng
RAYON_NUM_THREADS=4 target/release/examples/bounded_decode bounded ~/Downloads/3980774776.dng
RAYON_NUM_THREADS=4 target/release/examples/bounded_decode compare ~/Downloads/3980774776.dng 1 target/q3-rows
node web/checks/bounded-pixels.mjs ~/Downloads/3980774776.dng
node web/checks/bounded-pixels.mjs web/results/sony.arw force

# Trusted HTTPS, device setup and capture: see apple-device-testing.md.
# Serve the Mac fixture via a temporary ignored web/results/ symlink.
export SAFARI_DEVICE_TYPE=iPhone SAFARI_DEVICE_UDID=00008110-000240403AD1801E
export SAFARI_TEST_DIR=target/q3-repeat-fresh
node web/safari.mjs create --insecure
node web/safari.mjs navigate 'https://192.168.178.88:8443/?rawDevelop=bounded'
RAW_FIXTURE=results/3980774776.dng node web/safari.mjs run web/checks/raw-bounded.mjs
node web/safari.mjs end
# Repeat in a NEW directory/session with rawDevelop=legacy and the controls.
```

No proprietary RAW data, device certificates or generated WASM packages were
added to source control. Temporary fixture symlink, test HTTPS server, driver
and capture processes were removed/stopped after testing; certificates and
measurement artifacts were retained. The user subsequently requested a WIP
checkpoint of the prototype, checks and handoffs on `wasm-investigation`;
unrelated user files were preserved. The feature remains opt-in, device testing
is paused, and the checkpoint does not establish reliable phone operation.
