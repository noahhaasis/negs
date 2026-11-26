# Android app

A native Android app (Kotlin, Jetpack Compose) with the web app's
one-photo workflow: open a photo, develop it, export it. Branch
`android-app`, folder `android/`.

## How it fits together

```
Compose UI (android/app/src/main/java/dev/negs/app)
  Editor.kt       the edit, the render pump, previews, export, brightness
                  match, LUT export, favourites, presets (web/app.js)
  Presets.kt      preset names, the presets folder, web preset links
  Controls.kt     the inspector's controls (web/controls.js)
  Crop.kt         crop geometry and the crop session (web/crop.js)
  Border.kt       border settings and layout (web/border.js)
  ui/             photo view, crop editor, sheet with tabs, stock grid
  Engine.kt       starts the engine process, JSON lines over stdin/stdout
        │
        ▼
libspektrafilm_native.so: crates/spektrafilm-native, the desktop apps'
helper, built for aarch64-linux-android and run as a separate process
```

The engine is the desktop helper unchanged, not a new JNI crate: the same
protocol, caches and exports as the Windows/Linux apps. Android only lets an
app run programs from its native library directory, so the helper ships as
`lib*.so` with `useLegacyPackaging` (unpacked on install).

Its own process is the resource management the browser didn't give us:

- The photograph's memory (≈450 MB resident for 24 MP) is outside the UI's
  heap. If the engine is killed, the editor stays up, the next request
  starts it again, and every request names the photo, which it reloads.
- `onTrimMemory`: out of sight, the engine's preview caches go; in the
  background (`TRIM_MEMORY_BACKGROUND`) the engine exits, and returning
  renders again.
- `MemoryPolicy.kt` sets limits from measured RAM (`ActivityManager`), not the
  web's `pointer: coarse`: the GPU admission budget (2/5 of RAM, through
  `SPEKTRAFILM_WGPU_MEMORY_LIMIT_MB`), the diffusion FFT grid budget (160 MiB
  under 6 GB, 512 MiB under 10 GB, exact above), and previews at the
  screen's size.

Engine changes: `target_os = "android"` joins Linux in the Vulkan cfgs
(capacity checks, software-adapter rejection) and joins Windows/wasm in using
the plain dgemm loops instead of OpenBLAS.

## Build and run

```sh
android/build-engine.sh                  # engine → app/src/main/jniLibs (NDK 28, API 29)
cd android && ./gradlew assembleDebug    # needs local.properties sdk.dir
adb install -r app/build/outputs/apk/debug/app-debug.apk
android/smoke-engine.sh [photo]          # the engine alone over adb: catalog, render, save
```

Toolchain: `sdkmanager "ndk;28.2.13676358" "platforms;android-35"
"build-tools;35.0.0" emulator`, `rustup target add aarch64-linux-android`.
The emulator must be 35.x or newer for the API 35 image (31.2 boot-looped).

## Tested on the emulator (Pixel 4, API 35, arm64, 4 GB)

- Engine over adb: Vulkan through the emulator to the Mac GPU; a 24 MP
  render in ≈2 s, JPEG saved.
- App: data unpacked on first launch, catalog, sample opened, 2280×1520
  settled preview in ≈0.4–0.6 s, stock tiles with previews, Exposure drag
  (interactive frames, then settled), export to Pictures/Negs with Share.
- Engine killed (`kill -9`) mid-session: UI stays, next edit restarts it.
- `am send-trim-memory dev.negs.app BACKGROUND`: engine exits; reopening
  restarts it and renders.
- Crop: aspect ratios, landscape/portrait, straighten (dial and slider),
  quarter turns, Back cancels. The preview and stock tiles show the crop;
  the export is cropped the same way. Handles near the screen's edges are
  kept from the back gesture.
- Borders: solid, keyline, the 120 film frame and the 35mm strip (with the
  film's edge print, turned for portrait) preview as they export; a
  6384 × 5880 35mm export matched the preview.
- Match camera brightness on the sample DNG: −0.33 EV, solved in ≈1–2 s
  (`brightness_reference`, then `place_exposure`); Exposure is the offset
  above it; turning it off keeps the brightness; an export waits for a
  pending match.
- Export LUT… (⋮ menu): a 33³ `.cube` through the system's save dialog.
- Favourites: web's defaults, star or long-press a tile, kept in prefs.
- Presets: Import… (several files; one refused with the engine's reason),
  tiles with the photo developed through each, tap to apply (film, paper,
  Lab; Prep kept, so the brightness match re-solves), New preset named
  after the film (clashes refused case-insensitively, keyboard Done saves),
  Rename, Move earlier/later, Delete, Share… (the file plus a web link; the
  link decodes with web/presets.js), a web link shared to Negs, a preset
  file shared to Negs ("name 2" on a clash), applying with no photo open,
  Save as file… (byte-identical to the imported file).

## Presets

As desktop's library (docs/presets-design.md), not the web's one-at-a-time
import: one `<name>.json` per preset in `files/presets/`, with desktop's
`.order.json`, name rules and name keys, so files move between the apps
unchanged. The engine's `preset_import` / `preset_export` read and write
them; the tiles are the helper's `thumbnail` with `preset`, keyed on what a
preset keeps (`preset_kept` in the catalog).

Incoming: preset files opened with or shared to Negs are added to the
presets, and so is a web `#preset=` link shared as text. Tapping a
getnegs.com link doesn't open the app: that needs App Links (an
`assetlinks.json` on the site). Share… sends the file with the link as
text; Save as file… writes a copy through the system's save dialog.

Desktop sorts by dragging; here the tile menu has Move earlier/later.

Not testable here: real phone GPUs (Mali/Adreno drivers, speed), memory
behaviour on a real 4–8 GB phone. Needs one device or Firebase Test Lab.

## Language

Settings › Language: English or 简体中文, else the phone's language. As on
the web, strings pass through `tr()` (I18n.kt) with their English as the
key, against web/locales/zh-CN.js (copied into the assets at build time)
and then `app/src/main/assets/locales/zh-CN.android.jsonc`, the app's own
strings the web doesn't have. Switching redraws at once; messages already
shown stay as they were. `node android/check-i18n.mjs` checks that every
string the app shows has a translation and every app-own entry is used.

## Not done yet

- Help texts (the controls' `help` isn't shown, so isn't translated either).
- Presets: drag to sort; negative scans' preset rules (the app has no
  negative scan).
- The crop isn't kept per photo (the web app doesn't either); no flip.
- Thumbnails run at the interactive slot; a settled `preview_long_side` in
  the helper would keep the preview's resample identical to the web's.
- APK size (137 MB debug): 50 MB border PNGs and 21 MB LUTs.
- Comparing Android exports with the Linux/Windows Vulkan goldens.
- The export's render writes its full-size RGBA preview file (≈100 MB at
  24 MP) that nothing reads; the helper has no way to skip it yet.
