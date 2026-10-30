# Web sample photographs

Six additional inputs are prepared alongside the original sample. The static
packager includes all seven files. **Try a sample photo** opens `DSC_6377.dng`
immediately, then shows a numbered **Sample 1–7** picker and **Next sample →**.
No descriptive photo names are displayed or stored in the catalog.

## Assets

Sizes are decimal MB. Dimensions below include EXIF orientation.

| Sample | File | Dimensions | Size |
| --- | --- | --- | --- |
| 1 | DSC_6377.dng (existing, unchanged) | 6048 × 4032 | 3.36 MB |
| 2 | DSC_6028.dng | 2560 × 1707 | 0.71 MB |
| 3 | DSC_6426.dng | 1707 × 2560 | 0.91 MB |
| 4 | DSC_6008.dng | 2560 × 1707 | 0.59 MB |
| 5 | DSC_2498.dng | 1709 × 2560 | 0.48 MB |
| 6 | IMG_0316.dng | 1707 × 2560 | 0.48 MB |
| 7 | IMG_4416.dng | 1706 × 2560 | 0.53 MB |

The six new files total about **3.70 MB**. Each is about 4.4 MP, enough for
interactive experimentation without distributing full-resolution 20–36 MP RAWs.
They are *lossy linear DNGs*: demosaiced camera RGB, resized in linear light,
with a reversible encoding curve and JPEG tiles at quality 85 (4:4:4).
The DNG keeps as-shot white balance, a D65 camera matrix, orientation, and an
embedded camera preview (at most 1280 px). The original sensor mosaic and
full-resolution detail are not retained. GPS, serial numbers and editing-history
metadata are not copied. An existing Artist tag is retained where provided.

**Do not replace these with ordinary Adobe Converter `-lossy -side` output.**
Those files may use per-channel polynomial opcodes that this rawler version
does not apply. A real decoder/visual check found a severe pink cast. This
generator uses an explicit shared `LinearizationTable` instead.

## Provenance and rights

- The four Nikon Z f photographs are supplied by the project owner from
  `~/Projects/negs-site/candidates/raw/`. There is no separate photo license
  supplied with those files; do not infer GPL or public-domain rights from the
  app's license. Their inclusion follows the owner's request, not an independent
  verification of original authorship or third-party releases.
- The other three originals are from
  [Signature Edits' free RAW collection](https://www.signatureedits.com/free-raw-photos/).
  See [the license review](LICENSE-signature-edits.txt) and
  [full source terms](https://www.signatureedits.com/free-raw-license-terms/),
  checked 2026-10-30. Modification and redistribution are expressly permitted,
  including commercially. A competing photo library and false authorship claims
  are prohibited. Recognizable people, trademarks and artwork have separate
  rights; no model/property releases were independently verified. These samples
  are editor inputs, not an endorsement or a stock-photo collection.
- `DSC_2498` credits **Skrypchenko** in its original Artist metadata. The two
  Canon originals do not identify a photographer. Credit the source rather than
  inventing attribution. `catalog.json` retains the exact original filenames,
  source links, camera, oriented dimensions, byte sizes, and generated hashes.

The photographs are not covered by the application's GPL. User-facing source
credit and rights caveats are also recorded in `web/credits.html`.

## Loading and offline budget

- No new runtime imports, preview images, preloads or prefetches are added.
  The app still downloads **zero sample photographs at page load**.
- Download a RAW only after a sample button/selection. Do not prefetch the next
  photograph or load other RAWs just to extract thumbnails.
- The fixed offline sample is **DSC_6377.dng**. `web/offline.mjs` excludes the six
  other DNGs from the service-worker manifest altogether: neither runtime use
  nor **Save everything** stores them in Cache Storage. The default remains
  saved when used (or when explicitly saving everything), not at installation.
- `web/static/_headers` marks sample DNGs `Cache-Control: no-store` so online-only
  inputs do not accumulate in the HTTP cache either. The explicit Cache API
  storage of the default is intentional and independent of that HTTP policy.
- `catalog.json` and license text are small metadata, not photo preloads. Fetch
  the catalog after the first sample interaction, not at startup. No extra
  thumbnail files are needed for the numbered picker.
- The switcher disables online-only choices while offline and explains that
  Sample 1 is available offline once saved. Film/paper/adjustments are preserved
  between samples; crop/rotation reset as normal. Superseded downloads are
  aborted and no history of RAW byte buffers is retained. Failed selections
  return the picker to the photograph that actually opened. Opening a local
  photograph hides the switcher; crop mode temporarily hides it too.

## UI

`web/samples.js` manages the numbered picker and Next action, with no photo
bytes stored in that controller. Its catalog request begins only after the first
sample click, in parallel with the default RAW. Next wraps from Sample 7 to
Sample 1; the native select allows direct jumps. Labels are only **Sample 1**,
**Sample 2**, etc. Sample export filenames use the same numbers.

## Regeneration

Requirements: Python 3.11+, `numpy`, `pillow`, `rawpy`, and `exiv2` on PATH.
For example (dependencies installed into a temporary environment, not the repo):

```sh
python3.13 -m venv /tmp/negs-sample-venv
/tmp/negs-sample-venv/bin/pip install numpy pillow rawpy
/tmp/negs-sample-venv/bin/python scripts/prepare-web-samples.py
```

Use `--raw-dir PATH` and `--downloads PATH` to override the original locations.
The script leaves the existing default unchanged. `--catalog-only` rebuilds
metadata from the existing DNGs without regeneration. To prepare one input:

```sh
/tmp/negs-sample-venv/bin/python web/sample/make-dng.py INPUT OUTPUT 85 2560
```

The original Z f path can also use `dcraw_emu`. Canon matrices come from the
vendored rawler camera profiles; the D800 DNG supplies its own D65 matrix.
Different LibRaw/Pillow versions can produce different lossy bytes: regenerate
the catalog and recheck the rendered output when changing those dependencies.

## Validation

```sh
node --test web/checks/samples.test.mjs
node web/checks/samples.mjs
# With the dev server running (the harness starts Chrome):
node web/cdp.mjs http://localhost:8137/ web/checks/sample-picker.mjs 3000
# On a packaged server, use ?sw=1 to test actual offline cache/switching too.
node web/package-static.mjs /tmp/negs-samples-package  # destination must be empty
node web/checks/samples.mjs /tmp/negs-samples-package
```

All six new files were also decoded through native rawler (`golden_inputs`)
and visually checked for color/orientation. `bounded_decode compare` checked
that both current decode paths produce identical finite pixels for each file.
This is not a claim of pixel identity with the full-resolution original RAW:
resizing, demosaicing and JPEG quantization are deliberately lossy.
