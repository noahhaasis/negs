# WASM correctness against the native goldens

Local validation, 2026-10-30. The original harness is documented in
[`scripts/golden/README.md`](../scripts/golden/README.md). Its gates and
historical manifests/dumps were **not changed**.

## Results

Chrome 156 on the Apple M1 Pro, production WASM engine worker, automatic
10-thread pool. These are **not physical-iPhone Safari results**.

| Check | Result |
|---|---|
| Historical inputs | All 10 SHA-256 hashes match `scripts/golden/manifests/macos-arm64.json` |
| WebGPU stock/effect matrix | **112/112 pass**, including exact repeatability and the normal RGBA readback |
| WebGPU vs f64, deterministic cases | **106/106 pass**; worst ΔE00 0.001851, p99 0.000287 |
| CPU WASM, full matrix | **104/112 pass** under all original gates |
| CPU WASM vs f64, deterministic cases | **106/106 pass**; worst ΔE00 0.002021, p99 0.000312 |
| Phone working-source path | Both a 26 MP RAF and 24 MP JPEG decode/shrink to 4242 × 2828; transferred pixels are **bit-identical** to independently generated native f32 fixtures |
| Phone-configured WebGPU vs equally configured native GPU | **6/6 pass** the original `gpu` gate, worst ΔE00 0.000657 |
| Budgeted vs exact diffusion, 12 MP | **3/4 diffusion cases pass** the separate proposed approximation gate below; one remains a failure |
| Single-thread CPU spot checks | Four cases repeat exactly and are bit-identical to their threaded CPU counterparts; their same two strict gate failures remain |

The matrix is the native harness's 10 inputs × 7 stock pairs with grain/glare
off, 16 isolated effect presets on two inputs, and five grain presets on two
inputs. Four of the latter have grain/glare off and receive strict deterministic
gates, not statistical ones. It covers negatives, reversal scans, B&W,
exposure/colour, halation, DIR, lens/scanner blurs, diffusion, curves, preflash,
sharpening, and grain. It does not establish every control combination,
alternate output colour spaces, layout/export encoding, or RAW decoder parity
for all cameras.

### Fixes and remaining failures

**Recursive CPU blur:** the Young–van Vliet IIR's coefficients and recurrence
state now use f64, retaining f32 image buffers. At large sigma, f32 cancellation
and recurrent roundoff biased even constant inputs. This affected wide DIR and
sharpening across native/WASM builds. Wide-DIR colour errors dropped from about
0.17–0.22 to 0.0021 ΔE00; all four previously failing sharpening cases pass.
The f64 path keeps its original arithmetic; no golden values were rewritten.

The CPU matrix still exits nonzero:

- Two wide-DIR cases miss the **strict f32 cross-platform value gate**:
  4 and 19 values respectively, max absolute errors 2.24e-5 and 3.91e-5;
  the second reaches three 16-bit codes. Both pass the original f64 perceptual
  quality gate and stay within one 8-bit step. They have not been relabelled as
  passing the strict gate.
- Six grain cases fail `cpu_grain`, with different grain patterns. The native
  harness already documents the shared-MT19937 stream's cross-platform
  fragility: a tiny input/sampler change can shift later draws. Deterministic
  colour agreement is not proof of matching stochastic output. We did **not**
  replace the RNG, bless new goldens, or give these cases the GPU statistical
  gate. Resolving this needs a deliberate CPU grain reproducibility policy.

**Phone diffusion:** shrinking every exponential while leaving its amplitude
unchanged overweights subpixel cores relative to broad components. Reduced
convolution now preserves discrete component mass. It also restores the narrow
core's pixel-Nyquist response from the full-size residual, without another
image buffer or a larger FFT grid budget. The exact, unbudgeted path remains
unchanged.

Max ΔE00 against exact native GPU convolution on the **same working source**:

| 12 MP case | Before | After | After p99 |
|---|---:|---:|---:|
| RAF, camera Black Pro-Mist | 0.493 | 0.270 | 0.0171 |
| RAF, enlarger diffusion | 3.768 | **0.609** | 0.0606 |
| JPEG, camera Black Pro-Mist | 0.304 | 0.137 | 0.0306 |
| JPEG, Glimmerglass | 0.996 | 0.385 | 0.0802 |

`web/checks/golden-gates.json` proposes a deliberately separate diffusion-only
bound: max ΔE00 **0.5**, p99 **0.1**, at most **two 8-bit steps**. This is not a
pre-existing native gate or a newly approved user tolerance. The enlarger case
still fails its maximum bound; it is not hidden by the accepted general
allowance for slight diffusion differences. All four now remain below ΔE00 1
and within two 8-bit steps. Non-diffusion limits have not been loosened.

### Physical phone status

Safari automation was attempted three times. It refused a session, ultimately
reporting the iPhone as disconnected. No new physical-device accuracy,
performance, memory, or stability pass is claimed. The phone-configured runs
above used Chrome touch emulation and the actual production decoder/worker
handoff, 12 MP retained source, and 160 MiB diffusion grid budget.

Float diagnostics add memory/readback overhead, so this test is **not** the
phone stability or export memory test. Re-run the existing device smoke/export
scenarios after the GPU changes. Nothing was deployed and the existing
`public/` package was not regenerated.

## Reproduce

Requires Rust, matching wasm-bindgen, Chrome (or Safari automation), and Python
with numpy. The existing local Python environment is used below; no numpy or
colour-science installation into the system interpreter is necessary.

```sh
PY="$HOME/Projects/spektrafilm-env/bin/python"
cargo build --release --locked -p spektrafilm-cli --features precision-f64 --bin spektrafilm-f64
cargo build --release --locked -p spektrafilm-cli --bin spektrafilm --bin golden_compare
PATH="$PWD/target/ipad-tools/bin:$PATH" bash web/build.sh --threads
node web/serve.mjs 8137
```

Use the original fixed TIFFs, **not freshly decoded substitutes**. The main
`~/Projects/negs/target/golden` folder was absent. Preserved dumps were found at
`~/Projects/negs/.claude/worktrees/windows-parity/target/golden/dumps/macos-arm64`.
That worktree's original `target/release/golden_inputs` executable recovered all
ten original TIFF hashes from the original RAW files. Current generators did
not reproduce the six photograph hashes. The recovered inputs are retained at
`target/wasm-correctness/historical-inputs`; `prepare` refuses mismatches.

```sh
DUMPS="$HOME/Projects/negs/.claude/worktrees/windows-parity/target/golden/dumps/macos-arm64"
$PY web/checks/golden.py prepare --inputs target/wasm-correctness/historical-inputs
$PY web/checks/golden.py reference --golden-dumps "$DUMPS"
node web/cdp.mjs 'http://localhost:8137/checks/golden.html?prefix=gpu-check&repeat=1' web/checks/golden.js 1000
$PY web/checks/golden.py compare --prefix gpu-check

node web/cdp.mjs 'http://localhost:8137/checks/golden.html?prefix=cpu-check&backend=cpu' web/checks/golden.js 1000
$PY web/checks/golden.py compare --prefix cpu-check --backend cpu
```

Historical dumps are checksum-verified before use; available native f64/GPU
stock goldens are used directly. Effects isolated from grain/glare have fresh
native f64/f32/GPU references, as their params differ from the original grain-on
presets. References are cached by input, params, binary, and diffusion budget.
`reference --engines f32cpu` refreshes the native f32 baseline after a CPU fix
without modifying the preserved goldens. `--only name,name` selects cases for
both reference and compare; the browser uses `&only=name,name`. Unknown names,
missing captures, CPU fallback, nonfinite values, changed inputs/params, size
mismatches, and gate failures cannot count as passes. Comparisons exit nonzero.

For the phone path, generate independent **f32** native working-source fixtures:

```sh
cargo build --release --locked -p spektrafilm-cli --example working_input
mkdir -p target/wasm-correctness/working
target/release/examples/working_input web/results/0955097116.raf target/wasm-correctness/working/raf.tif 12000000
target/release/examples/working_input web/results/card-24mp.jpg target/wasm-correctness/working/jpeg.tif 12000000
$PY web/checks/golden.py --cases web/results/phone-cases.json prepare-phone
$PY web/checks/golden.py --cases web/results/phone-cases.json reference --engines f64,gpu,exactgpu --jobs 1
VIEW=390x844@3m node web/cdp.mjs 'http://localhost:8137/checks/golden.html?cases=/results/phone-cases.json&prefix=phone-check&repeat=1' web/checks/golden.js 1000
$PY web/checks/golden.py --cases web/results/phone-cases.json compare --prefix phone-check
```

For physical Safari, serve HTTPS as in
[`apple-device-testing.md`](apple-device-testing.md), create a device session,
then use `web/safari.mjs page` (not `navigate`, which waits for the editor UI):

```sh
node web/safari.mjs page 'https://HOST:8443/checks/golden.html?prefix=iphone-check'
node web/safari.mjs run web/checks/golden-safari.mjs
$PY web/checks/golden.py compare --prefix iphone-check
```

The test page also has a Run button for ordinary Safari when automation is
unavailable. Start with the smaller golden corpus before the 12 MP diagnostics.
Use a unique prefix per device/build. JSON run records include backend, actual
thread count, input/parameter identity, diffusion budget, and engine asset
hashes. The normal application still avoids float readback; `renderRaw` is an
explicit diagnostic entry point.

Tests and artifacts:

- Unit results: **22/22** native golden-harness tests, **14/14** WASM bridge
  tests, **50/50** math tests in each of f32 and f64, and **22/22** GPU tests
  (one existing performance benchmark intentionally ignored).
- The first full GPU unit run exposed an old validation-test bug: it parsed
  `display_post.wgsl` as a standalone shader. That test now assembles the same
  display shader + shared Post fragment as production. It passes without
  skipping the RGBA compute shader or weakening its threadgroup assertion.
- `python3 -I -m unittest discover -s scripts/golden -p 'test_*.py'`
- `$PY -I -m unittest discover -s web/checks -p 'test_*.py'`
- `cargo test --release --locked -p spektrafilm-math` (also `--features precision-f64`)
- `cargo test --release --locked -p spektrafilm-gpu`
- Reports/logs: `target/wasm-correctness/`; raw captures: gitignored `web/results/`.
  The authoritative completed reports are `gpu-verified-report.json`,
  `cpu-final-report.json`, and `phone-final-report.json`.
- An earlier 12-case single-thread controller command hit its host timeout;
  it is retained as `browser-cpu-single-timeout.log`, not counted as a pass.
  The later four-case run completed and recorded the same expected gate failures.
