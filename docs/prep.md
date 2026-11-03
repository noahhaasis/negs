# Prep

The Prep tab, camera brightness matching, the scopes and the Apple RAW 9 → 8 →
rawler fallback come from yosoyelchris's fork of Negs (branch `prep-raw-scopes`,
commit `d2a144d`, based on `d3f06c7`). The shared Rust engine accepts the Prep
parameters on every platform; for now only the native macOS inspector shows
them. Product spec: ticket 26.

- **Prep / Lab / Post:** Prep sits immediately before Lab. Tone has Highlights,
  Shadows, Contrast, Lift and Gain; Color has Color Boost and Saturation; Detail
  has Midtone Detail. All eight default to zero. Highlights spans −180…100,
  retaining Sawmill's extended recovery range; the others span −100…100.
- **Starting brightness:** Prep also has an independent Exposure control and
  **Match camera brightness**, which places the graded brightness against the
  RAW's embedded camera preview while limiting highlight crowding. Settings ›
  *Match new RAWs to camera brightness* (on by default) runs it once on every
  fresh RAW open. See [exposure placement](exposure-placement.md).
- **Default marker:** on macOS 26 each slider's track fills from its default
  (`NSSlider.neutralValue`), so a moved slider shows where it started.
- **New photograph:** Settings › New photographs. *Keep the previous edits*
  (the default) carries Prep, Lab and Post over as Negs always did; *Start
  fresh* resets Prep, Lab and Post adjustments, white balance and the border,
  keeping film/paper, Image Size and output colour. Crop, rotation and lens
  corrections reset either way. Relaunching restores the current session's
  edits, including Prep. Fresh RAW opens are then matched to the camera's
  brightness when that's on.
- **Scopes:** a 190-point RGB histogram replaces the old 88-point plot; drag the
  handle below it to resize it (88–420 pt, remembered). The dropdown switches
  to RGB waveform and remembers the choice. Both use the
  developed preview, honour crop/straightening, exclude borders, and update off
  the main thread. The waveform follows quarter-turn rotation and uses bounded
  sampling even for very wide or tall images.
- **RAW:** test each file against Apple RAW 9, then Apple RAW 8, then rawler.
  Selection uses the filter's supported versions and completed render; no
  camera-model allowlist. See [camera results](raw-compatibility-2026-11-01.md).

## Processing and persistence

Prep ports Sawmill's recovered Resolve CameraRawAdjustFilter control math and
guided filter. Input decoding and white balance precede auto/manual exposure,
then Prep exposure and controls, then the film simulation. Prep exposure is
independent of Lab print compensation. The recovered controls operate in a Rec.709 encoded
domain and return signed scene-linear pixels to the input colour space. HDR
and negative values are retained. Neutral settings bypass every conversion.
This is a port of those controls, not a claim of complete Resolve RAW-decoder
or rendering parity.

Preview, Fast export, and Reference export share the same Prep stage. Active
Prep invalidates source-only GPU upload keys; successive edits cannot reuse
stale prepared pixels. Reference builds retain the recovered f32 control math
inside the otherwise f64 pipeline. Whole-frame spatial processing runs on CPU
(in parallel) before the GPU film stage. Interactive previews apply it at
preview resolution. The macOS helper keeps the prepared photograph-size and
interactive frames (`Pipeline::with_prepared_frame`) keyed by everything Prep
reads, bit for bit: the Prep settings, both exposures before it, the input
decoding and the frame size. Lab edits that change none of those (print
exposure, filters, grain, …) skip Prep entirely.

Prep values are saved with the photograph's session and undo history. Film/Lab
presets preserve the photograph's existing Prep, just as they preserve Post.
Film-look LUT export excludes Prep because it is image-specific and spatial.

## Verification

The implementation includes numerical golden tests, full-frame guided-filter
references, signed/HDR and colour-space checks, default/old-params compatibility,
CPU/GPU output checks, GPU cache transitions, RAW fallback tests, scope geometry
tests and slider/session lifecycle tests. The fork's own validation report, from before the port, lists the checks run
on its build machine: [validation results](prep-validation-2026-11-01.md).

## Open

- **Linux and web inspectors.** The engine is shared, so those apps render Prep (neutral
  unless set), but only macOS and Windows have the controls (Windows: `docs/windows-app.md`,
  branch `windows-prep-port`). Needed before Prep counts as shipped on every platform.
- **Highlights centring.** The range is −180…100 (the recovered extended recovery travel), so 0
  sits at 64 % of the track. Options: a piecewise mapping that centres 0, −100…100, or leave it.
  Above an encoded base of 1.0 (linear ≈ 4.2) the tone table's end slope is extrapolated, so
  negative Highlights brightens extreme speculars (−180: +2.3 stops at linear 8).
- **Tab-switch lag.** Already on main before Prep (Lab → Output ≈ 180 ms, back ≈ 305 ms; → Prep
  110–170 ms). Keeping the tabs alive switches in 45–60 ms but makes a slider drag cost ≈ 50 %
  more main-thread time.
- **Apple RAW 9.** No RAW decodes with RAW 9 on macOS 26.5; it seems to arrive with macOS 27.
  The RAW 9 → 8 → rawler fallback needs testing there, and a real launch on macOS 14/15.
- **GPU Prep.** The recovered controls as a GPU pass, measured and parked on the
  `feature/prep-gpu` branch: Prep drags about 200 ms faster at 24 MP, rounding-level
  differences in preview and Fast export.
