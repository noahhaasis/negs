# Negs: desktop features missing on the web (inventory)

Research by a sub-agent, 2026-10-28. Paths are relative to `/home/noah/Work/Negs`
(main), or `[wasm]` for `/home/noah/Work/Negs-wasm`.

GPU path key: **Resident** the WebGPU chain already does it (a parameter change);
**Rebuild** in `preview_pipeline_cache_key`, so a change rebuilds the spectral
pipeline on the CPU (~300 ms for a film change); **Readback** forces reading the
float scan back (sRGB with no Post is the only output that presents); **CPU**
CPU-only engine code; **New GPU** needs a new shader or pass; **UI-only** no
engine work.

Do now:
| Film/paper pickers, grouped | Present | Rebuild, then Resident | – |
| Automatic paper pairing | Partial (no development-time reset) | Rebuild | S |
| Scan film directly, for any film | Slides only, no toggle | Rebuild | S |
| Engine catalog/defaults from wasm instead of static JSON and JS defaults | Missing | UI-only, wasm `catalog()` | S |
| B&W-aware controls (one grain lane) | Partial | UI-only | S |
| Metering method (7 modes) | Toggle only | CPU, 30–70 ms re-meter | S |
| Exposure, film format | Present | Resident | – |
| Camera lens blur | Missing | Resident | S |
| Lens Diffusion (Pro-Mist etc., 9 controls) | Missing | Resident | S |
| Film Curves (7) | Missing | Resident | S |
| Grain, full (15 controls) | Amount×/Size× only | Resident | S |
| Halation, full (6) | 3 of 6 | Resident | S |
| DIR Couplers (4) | Missing | Resident | S |
| Film Base (5) | Missing | Resident | S |
| Enlarger: compensation, preflash | Print exposure, Y/M only | Resident, but shifts/preflash Rebuild every move | S–M |
| Balance Print button | Missing | CPU, tiny | S |
| Print Curves, Print Base | Missing | Resident | S |
| Glare: roughness, blur | Amount only | Resident | S |
| Print Diffusion (planned on desktop) | Missing | Resident | S |
| Scanner: lens blur, unsharp, white/black correction | Missing | Resident | S |
| Colour management: output gamut, input gamut, CAT16, upsampling method | Missing | Output gamut Resident; rest Rebuild; other methods +6 MB LUT each | S |
| Output colour space / encoding | Missing | non-sRGB = Readback | S–M |
| Per-control reset, type-in values, Reset Adjustments | Double-click reset only | UI-only | S |
| Placeholder while developing (embedded RAW preview) | Missing (~1.8 s blank for a 24 MP ARW) | CPU / browser decode | S–M |
| Zoom/pan, 100%, pinch | Missing | Real detail needs full-res render (24 MP 0.7 s); 8192 px texture cap; no tile rendering | L |
| Preview = export resolution (grain fidelity) | Viewport-size only, no notice | tied to zoom | S notice / L real |
| Interactive frames | Present (adaptive) | Resident | – |

second stage:
| RAW white balance (As Shot/Daylight/Tungsten/Custom) | Missing | CPU re-decode ~0.8 s at 24 MP (maybe foldable into front matrix, unverified) | M |
| Show Original (hold) | Present (MVP piece 8; `[wasm]` docs/web-wasm.md item 16) | Resident: the chain's input through the display pass | – |
| Data download/caching (service worker, lazy LUTs and textures) | none | S–M |
| About / licence attribution | none (needed: profile, film.png MIT, 35mm CC BY-SA) | S |


research:
| HEIC (iPhone photos) | No (no platform has it) | – | ? |

## Stocks & catalog
| Feature | Web today | GPU path | Effort |
|---|---|---|---|
| Development time (B&W push/pull) | Missing (catalog has `times`) | Rebuild | S |
| Film/paper search (Linux only) | Missing | UI-only | S |

## Controls (Lab; about 110 controls on desktop)
| Feature | Web today | GPU path | Effort |
|---|---|---|---|

## RAW & input
| Feature | Web today | GPU path | Effort |
|---|---|---|---|
| RAW decoding (rawler) | Present | CPU once | – |
| Nikon HE★ (Apple decoder) | Fails cleanly | – | desktop-only |
| Input colour interpretation (Assume/Manual) | Automatic only | CPU; Manual decode costs every frame | S–M |

## Geometry
| Feature | Web today | GPU path | Effort |
|---|---|---|---|
| Rotate 90° | Missing | UI-only preview, CPU on export | S |
| Crop with aspect presets, handles, grid | Missing | UI-only preview; export crop needs bytes/float path | M–L |
| Straighten ±45° | Missing | as crop | M |
| Image Size (longest side) | Missing | **Forces CPU today**; wasm must resolve/clear like the helper | M |
| Borders (solid, keyline, 120 film, 35mm strip, 3:4 Instagram) | Missing | Preview UI-only; export CPU compositing; textures 17 MB total | M–L |

## Post
| Feature | Web today | GPU path | Effort |
|---|---|---|---|
| Tone (Brightness, Contrast, Whites, Blacks) + Saturation | Missing | Readback today; fits in the display shader (M), and should not re-render the film | M (S as CPU+readback) |
| Highlights/Shadows | Not implemented anywhere (draft plan) | New GPU, spatial | L |

## Viewing
| Feature | Web today | GPU path | Effort |
|---|---|---|---|
| Fit to window | Present | – | – |
| Histogram | Missing | GPU histogram pass or readback | S–M |
| Wide-gamut / P3 preview | sRGB only | Readback; P3 canvas in wgpu unverified | M |

## Export
| Feature | Web today | GPU path | Effort |
|---|---|---|---|
| Full-res export | 8-bit sRGB JPEG q0.92 | Resident + RGBA8 readback | – |
| PNG/TIFF | JPEG only | CPU encode, needs bytes-based save API | S–M |
| 16-bit | No | float readback (memory ×2) | M |
| P3/Adobe/ProPhoto + ICC | sRGB, no ICC | Readback; Rust encoders | M |
| JPEG quality | fixed | CPU | S |
| Export size (long/short edge) | Missing | CPU resize | S–M |
| Copy EXIF | Missing | CPU, path-based today | S–M |
| Reference f64 engine | n/a | desktop-only | – |
| Export .cube LUT | Missing | CPU, small | S |
| Share sheet / save to Photos (iOS) | Download link | UI-only (`navigator.share`) | S |
| Remembered choices, progress/cancel | Minimal | UI-only | S |

## Workflow
| Feature | Web today | GPU path | Effort |
|---|---|---|---|
| Open, recent files | Open only | UI-only | S |
| Drag and drop | Present | – | – |
| Film strip / folder browsing | Missing | UI-only; iOS has no folder picker | L |
| Undo/redo | Missing | UI-only | S–M |
| Session restore + cached preview | Missing | IndexedDB/OPFS | M |
| Presets (apply/new/rename/delete/import/export) | Missing | engine side easy in wasm; apply may Rebuild | M |
| Built-in presets | none shipped anywhere | – | – |
| Keyboard shortcuts | Missing | UI-only | S |

## Other
| Feature | Web today | Effort |
|---|---|---|
| Light/dark appearance | dark only | S |
| Accessibility labels | basic | S–M |
| Warn before leaving during export | none | S |

## Observations
- **Full parameter state first:** presets, undo, sessions, Reset and the full
  control set all assume a complete `RuntimeParams` with engine defaults; the web
  sends only changed values and grain as multipliers. A wasm `catalog()` export
  is the foundation.
- **Display pass on the GPU:** Post, P3/non-sRGB output and the histogram are
  what keep frames presented without readback. Desktop applies Post without
  re-rendering; matching that means keeping the scan resident and re-running
  only the display pass.
- **Bytes-based save in `spektrafilm-io`:** formats, 16-bit, ICC and EXIF all
  need it.
- **Image Size** silently forces the CPU engine on the web until the wasm crate
  resolves it like the helper.
- **Enlarger Y/M shifts and preflash** rebuild the spectral pipeline on every
  slider move in the browser (`Pipeline::set_print_filter_shifts` may avoid it).
- **Zoom/100% and grain fidelity are one problem:** the web never shows
  export-resolution pixels, and the engine has no region rendering.
- **Awkward on phones:** crop handles and straighten dial, ~110 nested controls,
  hold-to-compare, folder browsing, shortcuts, 16-bit/enlarged exports (memory),
  RAW WB re-decodes, HEIC, grain invisible at phone preview size.
- **Unverified:** Safari 26 WebGPU in a worker; iOS tab/wasm memory limits; P3
  canvas through wgpu; shift-only rebuild time; folding Custom/Tungsten WB into
  the front matrix.
