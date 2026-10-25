# Independent review of the early-upscale plan

Date: 2026-10-25. Reviewer: Claude Opus 5.5 (`claude-opus-5-5`), invoked
with `--effort high` through Claude Code 2.1.310. Read-only tools: Read,
Glob, Grep; permission mode: plan. The CLI completed successfully with
`is_error: false`, actual model usage `claude-opus-5-5`, and no tool
permission denials. No fallback model was used.

Reviewed input: the original `docs/upscale-plan.md` in the main checkout
at Negs `6e70e5e`. The reviewer inspected source but did not run tests,
open comparison images, or independently read the forum posts; its
inspection limits are recorded below. The parent agent had read those
posts and inspected the comparison images when preparing the draft.

The response below is unedited. The final plan in this worktree records
how the findings were addressed; this review evaluates the original
draft and does not certify an implementation or the revised plan.

---

# Review: `docs/upscale-plan.md` (early upscale / Image Size)

Reviewed against Negs `6e70e5e` and Python `spektrafilm@28bf883`. I only read files; nothing was built, run or edited.

## 1. Verdict

**Proceed with changes.**

The core design is the right bounded choice: develop the full frame at a larger size, crop after the simulation, and treat the target as the cropped photograph's longest side.
- Physically it matches Python. Python computes `pixel_size_um` from the full frame before the crop and then divides it by the scale (`resize.py:22,32`). Negs gets the same result because `pixel_size_um` uses the full working frame (`filming.rs:41-43`).
- It also keeps crop panning free of re-renders.

Four things must be fixed before an agent starts:
- The resampler clamps values to [0, 1].
- The plan never says where the size is resolved or what identifies a render.
- The plan assumes capacity protection that does not exist on macOS.
- The plan's fix for drag frames would make dragging slow.

## 2. Prioritized findings

### H1. The shared resampler clips highlights to [0, 1] before halation (high)

**What the source shows:**
- Both `pipeline.rs:676-700` (`rescale_working`) and `preview.rs:75-87` (`rescale_preview_input_fast`) call `image::imageops::resize` on `Rgb<f32>`.
- In the locked `image 0.25.10`, `horizontal_sample` clamps every output sample to `[DEFAULT_MIN_VALUE, DEFAULT_MAX_VALUE]` (`sample.rs:257-258, 316-319`), and for `f32` that range is `0.0..1.0` (`traits.rs:68`).
- The RAW/PNG loader deliberately leaves highlights unclipped: "highlights are not clipped" (`spektrafilm-io/src/lib.rs:162`).
- Python's `skimage` `rescale` clips to the input's own range, not to [0, 1]. That is from my knowledge of skimage's default `clip=True`; I did not re-read skimage here.

**Why it matters (inference):** In Longest side mode, every render goes through this resize. RAW highlights above 1.0 would be flattened before halation and auto-exposure. Halation is exactly what mikae1 asked to preserve, and Original mode would render it differently. Today the same clipping already affects the 12 MP preview cap and sub-1 factors.

**Fix:**
- Add one parallel, separable Catmull–Rom resampler that does not clamp to 1.0. Clamping negatives at 0 is fine.
- Model it on the existing `taps` pattern in `spektrafilm-io/src/resize.rs:101-117`. Use the same kernel, support, weight normalisation and vertical-then-horizontal order as `image`, so results are identical wherever no clamping happened.
- Use it for the native preview, the CLI (both precisions) and `rescale_working`.
- This also removes the single-threaded `image` resize from an enlargement path that will now cover 50–100 MP.
- The plan must say whether legacy sub-1 factor outputs may change for RAW files with highlights (see decision D7).

### H2. Where the size is resolved and what identifies a render are undefined (high)

**What the source shows:**
- The native `render_key` is `{input, params, limit_preview, interactive_scale…}` (`native main.rs:264-268`). Crop is not part of it.
- In Swift and Windows, `EditState.rendered` clears the crop (`Parameters.swift:151-155`, `windows/src/state.rs:55-61`). Both preview caches key on `rendered` (`RenderedPreviewCache.swift:54`, `rendered_preview.rs:115`).
- `Crop::pixels` rounds both edges, so the pixel width depends on `x` as well as the crop width (`crop.rs:63-72`). `straightened_size` uses only `width × W` (`crop.rs:76-81`).
- Windows depends only on `spektrafilm-io`, not on core (`windows/Cargo.toml:20`).

**Fixes:**
- **Put the resolver in `spektrafilm-io`, next to `crop.rs`.** Then the CLI, the native helper and Windows readouts all share one implementation.
- **Resolve from the crop's fractional `width × W0` and `height × H0` only, ignoring the origin.** Then:
  - `s = target / max(cw·W0, ch·H0)`
  - `W = round(W0·s)`, `H = round(H0·s)`
  - The rounded crop's long side lands within ±1 px of the target. Width error ≤ `cw·0.5` plus edge rounding stays under 1.5, so the result rounds to ±1.
  - Panning never changes the working size, so the helper key, the Swift/Windows identity and the CLI all agree.
- **Identity:** in Longest side mode only, `rendered` keeps `crop.rect.size` and clears the origin, aspect, custom ratio and angle. The helper adds the *resolved working dimensions* to `render_key`, not the crop geometry, so an aspect change that keeps the same long side reuses the render.
  - `CropTests.testCropEditsDoNotChangeTheRenderedState` (`CropTests.swift:153-158`) must become mode-dependent.
- **No double resize:** whichever layer resolves the size must set `upscale_factor = 1` *and* clear the target before `Pipeline::with_params` (native `main.rs:327-334`). `lut_export.rs:33` must clear the target as well, or the LUT grid itself would get enlarged.

### H3. The memory and capacity plan assumes protection that does not exist (high)

**What the source shows:**
- `ComputeBackend::image_capacity` returns `Ok(())` by default (`gpu/lib.rs:21-23`). Only Windows wgpu overrides it (`wgpu_backend.rs:2161-2164`, `capacity.rs:57`). macOS Metal and the CPU backend never reject a size.
- On Windows, a rejection silently falls back to the CPU rather than raising an error (`pipeline.rs:531-534, 565-570`). The native helper's `process_resident_borrowed` returns `None` and also falls back.
- `rescale_working` allocates the enlarged buffer *before* the capacity check (`pipeline.rs:527` vs `531`).
- The Windows test admits 12000×8000 and rejects 14400×10000 on a 6 GiB budget (`capacity.rs:118-140`).
- Pixel count grows with the square of the zoom. A crop of one third of the width at the same target needs 9× the pixels.
- The helper keeps `source`, `scaled` (enlarged), `interactive` and the full-frame `output` all resident at once (`native main.rs:94-106`).

**Fix:**
- Add a host-side working-pixel limit to the resolver. It must return an error before any allocation in both the helper and the CLI.
- Start with a provisional limit of about 100 MP of working pixels and adjust it after measuring.
- Keep the existing Windows CPU fallback, but keep showing "on the CPU".
- Remove the plan's wording "report unsupported sizes with a useful error" as a description of capacity checks: on Windows that path falls back, and on macOS it is a no-op.

### H4. Drag-frame fix: clamp instead of a bicubic fallback (high)

**What the source shows:**
- `interactive_scale` is screen pixels per *photo* pixel, `< 1` (`EditorModel.swift:597`, Windows `model.rs:986-988`).
- The helper renders the drag frame at `photo_dims × s` using `downsample_area` (`main.rs:320-344`).
- `downsample_area` asserts `new ≤ source` (`preview.rs:94`).
- A panic is caught, but it resets the whole `Session`, dropping the source cache, the pipeline and the retained render (`main.rs:441-447`).

**Why the plan's fix is wrong:** falling back to bicubic enlargement during a drag would be a full-size enlargement on every new size. Today that resize is serial, and the source comment calls it about a second at 60 MP (`preview.rs:91-92`).

**Fix:** clamp the drag-frame size to fit inside the source with the same aspect ratio, and always use `downsample_area`. Only the settled frame is enlarged.

### M1. The 8192 px display cap undermines checking grain inside the app (medium)

**What the source shows:**
- `DISPLAY_MAX_DIM = 8192` applies to the *uncropped* working frame (`main.rs:27, 388-399`). Swift and Windows reject anything larger (`EditorModel.swift:696`, `picture.rs:15`).

**Inference:** with full-frame enlargement, even a modest target shows a reduced image once the photo is cropped. Example: crop to half the width with a 6000 px target, so the working frame is 12000 px. The proxy is then 8192 px, about 0.68 of the export resolution. At 100% zoom the user sees interpolated grain, which is the very artifact mikae1 described.

**Fix:**
- Make "Display reduced: W × H shown of W × H" a required status/zoom label whenever the proxy is smaller than the photo size.
- Add an acceptance check that compares exported 100% crops, not on-screen views.
- A viewport-only render stays out of scope.

### M2. Auto exposure depends on the working size (medium)

**What the source shows:**
- Rust meters the image it is given, after the resize, using a nearest-neighbour 256 px downsample (`pipeline.rs:719-728`, `filming.rs:60-91`).
- Python meters the full frame *before* the crop and rescale (`resize.py:24-26`, `stages/filming.py:92-105`).

**Inference:** in Longest side mode, changing the crop size changes the scale, which can nudge the exposure. The effect should be small once H1 is fixed.

**Fix:**
- Add an acceptance check: auto-exposure EV at Original vs 2× and 3× targets differs by less than 0.01 EV.
- If it fails, meter the source before resizing and pass the EV in.

### M3. macOS session compatibility: keep the target in `params` (medium)

**What the source shows:**
- `EditState` uses synthesized `Codable` with no `decodeIfPresent` (`Parameters.swift:138`, unlike `Border.swift:164-175`). A new stored field would make old `session.json` files fail to decode.
- `JSONValue.restoring` fills in new engine defaults, including null to saved (`Parameters.swift:65-76`).

**Fix:** store the target as `io.<name>: Option<u32>` with `#[serde(default)]`. Sessions, presets, undo, the CLI `--params` file and the helper key then all carry it with no new Swift state.

### M4. The size readouts take the wrong numbers (medium)

**What the source shows:**
- `photo_width/height` are the dimensions *after* the 12 MP limit is applied (`main.rs:318-319`).
- `imageSize`, `photoSize` and the crop readout are built from them (`EditorModel.swift:115, 623`; `Crop.swift:698-702`).

**Fix:**
- The helper response adds `working_width/height` (unlimited) and `photo_crop_width/height` (crop resolved at the working size).
- The inspector's "Photograph" row uses those values.
- In the crop session in Longest side mode, show `≈ target × short px · N× from W×H source px`. This is simple ratio maths and needs no exact mirror of the rounding.

### M5. Cancellation and long renders (medium)

**What the source shows:**
- There is no in-flight cancellation for previews. The helper is serial, edits only coalesce (`EditorModel.swift:583-591`), and the only kill is `EngineClient.stop()` (`Engine.swift:310-321`).
- Export cancellation sends SIGKILL to the CLI (`Engine.swift:381-387`), so that works at any size.
- Windows throws away previews that took more than 1.5 s on the GPU if the display driver reset meanwhile (`model.rs:1021-1026`).

**Fix:** reword the plan's "preserve cancellation" to "export cancellation unchanged; previews are not cancellable". Add a Windows check that a large enlarged render does not trigger a driver reset.

### M6. Showing finished file dimensions is extra scope (medium)

Border and Instagram geometry are computed in Rust (`add_border`, `extend_for_instagram`). Showing exact file dimensions in the Swift and Windows export panels would mean duplicating that geometry.

**Fix:** for v1, show the photograph size plus the existing "adds border" and fit summary. Exact file dimensions only if the helper returns them.

### L1. Naming clashes with the export "Long Edge" (low)

`Fit::LongEdge`, the CLI's `--long-edge` and `ExportSize`'s "Long Edge" all mean *after the border, reduce only*. Name the new field distinctly (for example `io.photo_long_side_px`) and spell the difference out in the UI text.

### L2. Python's crop fields are dead in Negs (low)

`io.crop`, `crop_center` and `crop_size` exist in `params.rs:546-551` but nothing in Negs reads them, apart from `lut_export` turning `crop` off. Do not reuse them: in Python they crop *before* the simulation.

### L3. The legacy egui GUI has no crop tool (low)

`crop` never appears in `spektrafilm-gui/src/main.rs`. Limit egui to ignoring a null target and keeping its tests compiling. Updating its working-size control is extra scope.

### L4. The Original commands need no enlargement (low)

**What the source shows:**
- The Windows canvas draws Original at `image_size` (`canvas.rs:303-304`).
- macOS Original uses ImageIO, not the helper (`EditorModel.swift:551-556`).

**Fix:** the helper's `Original` command must *accept* params that contain a target but ignore it. Never enlarge the Original; matching the aspect ratio is enough.

### L5. The planned placement test cannot fail (low)

The check "Enlarged 2× equals resize → process" passes by construction.

**Fix:** add a negative control (process → resize must *differ*) and a check of physical scale: a bright point's halation or blur radius in pixels should grow about 2× at 2×.

## 3. Decisions the final plan must settle

| # | Decision | Recommendation |
|---|---|---|
| D1 | Field name and type | `io.photo_long_side_px: Option<u32>`; `None` means Original |
| D2 | Target vs legacy factor | Choosing Longest side sets factor = 1. The helper and CLI reject a target combined with factor ≠ 1. Never multiply them |
| D3 | Legacy factor < 1 in the UI | The Size picker shows a read-only "Scaled N% (older edit)" entry until the user picks Original or Longest side |
| D4 | Default value when switching to Longest side | Last-used target from UserDefaults or the Windows equivalent; otherwise the current crop's native long side |
| D5 | Rounding | Accept ±1 px and show the actual size. No trimming, no resizing after development |
| D6 | Limits | Target 16–65 535 (matches `ExportSize.pixelRange`); provisional working limit about 100 MP, tuned after measuring. Over the limit: error before allocation, last good preview stays, warning in the crop readout before Done |
| D7 | Non-clamping resampler scope | Use it on all early-resize paths. Accept and document that sub-1 / 12 MP-capped outputs change for RAW highlights (a bug fix) |
| D8 | Reset Adjustments | Keep the image size, like crop, border and rotation (copy it the way `io.rotate_cw` is copied in `reset()`). The legacy factor still resets to 1 |
| D9 | What enters render identity | Longest side mode: `rect.size` only. Original mode: unchanged. Helper key: resolved working dimensions |
| D10 | Crop request field | `Render` gains `crop: Option<Crop>`, used only for sizing and sent only in Longest side mode |
| D11 | CLI path | Resolve with `--crop` (full frame if absent), resize with the shared resampler, clear factor and target, then build the `Pipeline` |
| D12 | Drag frames | Clamp to the source size; always `downsample_area` |
| D13 | Placement | Lab, new "Image Size" section before Camera, as proposed. Working resolution slider removed; "Limit preview" stays in Output |
| D14 | egui | Out of scope beyond compiling |

## 4. Lean implementation sequence

1. **`spektrafilm-io`**
   - Add the resolver: source size, target, optional crop → working size, or an error for the pixel limit, range or overflow.
   - Add the non-clamping parallel Catmull–Rom resampler.
   - Unit tests: the ±1 bound across random crops, landscape and portrait, quarter-turn invariance, straightened crops using `straightened_size`, overflow and limit errors, equality with `image::resize` for inputs within [0, 1], values above 1 preserved.
2. **Core**
   - Add the param with its serde default, and clear it in `lut_export`.
   - `rescale_working` uses the new resampler. If a target is still set, resolve it full-frame.
   - Add one helper that prepares the working image and clears the factor and target (no double resize).
   - Update `gpu_capacity_fallback.rs`.
3. **CLI**
   - Resolve the size before resizing, using `--crop`.
   - Error before the enlarged allocation.
4. **Native helper**
   - `Render` gains `crop` and target validation.
   - Resolved dimensions go into the key; `export_resolution` compares against the resolved, uncapped size.
   - Clamp drag frames.
   - The response gains working and cropped dimensions.
   - `Original` ignores the target.
   - Tests: drag frame larger than the source (no panic), Fast-export reuse true only at the resolved size, key reused when only the crop is panned.
5. **macOS**
   - Mode-aware `rendered`.
   - Image Size section with the text field committing on Return or focus loss.
   - Crop readout, Reset keeping the size, legacy factor entry, "display reduced" label.
   - Update `CropTests`, `SessionTests` (an old session with `upscale_factor: 0.5` still restores) and `RenderedPreviewCacheTests`.
6. **Windows:** mirror step 5, calling the `spektrafilm-io` resolver directly.

**Acceptance checks**
- An old session or params file with factor 0.5 restores. Without RAW highlights the output is pixel-identical to before; any change is limited to values above 1.0, per D7.
- For a 6000×4000 source, a centred 0.5×0.5 crop and a 6000 target: working ≈ 12000×8000, exported photograph long side 6000 ± 1. Same for portrait, rotated, straightened, and with or without a border.
- A Fast CLI export matches the native Save of a full-size render byte for byte. Reference export stays within the existing tolerance.
- The 12 MP cap and the 8192 proxy never shrink the export.
- Negative control: process → enlarge differs from early enlargement. Halation and blur radius in pixels scale about 2× at 2×.
- A RAW file with highlights above 1.0: halation at a 2× target looks like Original's (no clipping).
- Auto-exposure EV changes by less than 0.01 between Original and 3×, and between two crop sizes.
- Drag with zoom > fit on an enlarged photo: no engine reset, and the drag frame stays within the source size.
- Crop in Longest side mode, then Done, Undo, Redo, Cancel, reopen the session and restore from the startup cache: target, dimensions and render all agree. Panning alone does not re-render.
- An over-limit target or crop is rejected before allocation (check the log), and the last good preview stays.
- Measure elapsed time and peak RSS for 12 MP → 24, 48 and about 96 MP, using Fast and Reference, on macOS and Windows. Record the Windows CPU fallback and any driver reset. Export cancellation still works.

## 5. Claims in the draft that overpromise or contradict the code

- **"Check … backend capacity … report unsupported sizes with a useful error":** the capacity check does nothing on macOS, and on Windows it falls back to the CPU instead of erroring (H3).
- **"Use the bicubic path" for drag frames:** this is serial and slow; clamp instead (H4).
- **"Preserve today's interpolation behavior at legacy sizes" / "rescale_working … can already enlarge":** true, but it hides the clamp to [0, 1] that clips the highlights halation depends on (H1).
- **"Cancel must restore the previous render" / "coalesce changes during cropping":** already true by design. `state.crop` only changes on Done (`EditorModel.swift:399-412`), so no extra work is needed and none should be invented.
- **"Preserve cancellation":** only export cancellation exists (M5).
- **"Show the finished file dimensions in the export panel":** needs border geometry Swift doesn't have (M6).
- **The planned placement test:** it is circular and needs a negative control (L5).
- **The "Original retains pixel density" readout:** today `photo_width` is the size after the 12 MP preview limit, not the export size (M4).
- **Parity with Python:** Python meters exposure before the resize and its cubic filter differs from Catmull–Rom. The draft rightly avoids claiming pixel parity; it should also not imply matching exposure (M2).
- **Measured parity or memory:** nothing in this review is measured. Memory and performance figures are inferences until the acceptance measurements are run.

---

**Limits of this inspection:**
- I couldn't read the forum JSON with grep (single very long lines), so I relied on the summaries in the plan and your prompt.
- I did not open the comparison JPEGs.
- I only skimmed `PreviewCanvas.swift`, `Original.swift`, `windows/src/ui/inspector.rs` and `ui/dialogs.rs`.
- The skimage clipping behaviour is from my knowledge, not re-read.
- No builds or tests were run.
- I had no Write or ExitPlanMode tool here, so per your instructions the review is only in this response; no plan file was written.

Model: Claude Opus 5.5 (`claude-opus-5-5`).
