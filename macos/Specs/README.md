# Frame pipeline model

`FramePipeline.tla` is a TLA+ model of how the macOS canvas gets its frame
(`EditorModel.swift`):

- opening a photograph, and its early frames (the restored session's frame,
  the kept last develop, the camera's thumbnail)
- the render queue: the first frame's decode, quick and settled frames, drag
  frames while a slider moves
- Match camera brightness, which a render waits for, and the frame held for it
- crops, releases, retries, and requests that fail

Each `await` in the Swift code is a separate step, so TLC tries every order in
which edits, renders, solves and early frames can land. Pixels, sizes and the
helper's answers are left out. Where the code decides from them (a quick frame
pays, a solve moves the exposure, a request fails), the model allows every
outcome.

Run it with `scripts/check_frame_model.sh` (one photograph, ~4 min) or
`scripts/check_frame_model.sh full` (two photographs, an hour or more).

## What it checks

Safety, in every reachable state:

- `ImageOfThisPhoto`: the canvas never shows another photograph's frame.
- `NoPlaceholderOverFrame`: a developed frame is never covered by a placeholder.
- `PreviewCurrentIsTrue`: `previewCurrent` (Save, Export, thumbnails) means the
  frame on the canvas is the settled render of the current edit.
- `HeldIsThisPhoto`, `HeldHasSolve`: a frame held for the match belongs to this
  photograph, and a running solve will show it.
- `NoSettledRenderOfStaleMatch`: the old exposure is never rendered at export
  size (ticket 39).
- `OneRender`: one render at a time.

Liveness (`Settles`): once the user lets go, the canvas always settles. The
current edit's settled frame shows (or its error), and no match is left
pending.

## What it found (November 2026)

The `FIX n` comments in the spec mark each one, and the fixes are in
`EditorModel.swift`:

1. Matching turned on during a first frame's decode rendered the old exposure
   at export size (held, so never shown, but a wasted render).
2. A match whose decode failed left the edit's frame held for a solve that
   never came. The canvas stayed on the previous frame.
3. A match left stale by a drag, or finished during one, was never solved or
   rendered after the release. "Matching…" stayed up, and Save waited 30 s.
4. and 5. A render scheduled on release could replace the settled frame that
   landed meanwhile with a canvas-size one, while `previewCurrent` stayed true.
6. Cancelling a crop left a match pending in the same way as 3. The random
   test found this one first: the model had Cancel wrong.

`FramePipelineTests.swift` checks the same invariants on the real model and
helper, with regressions for these and seeded random action sequences
(`NEGS_PIPELINE_SEEDS`, `NEGS_PIPELINE_STEPS`, `NEGS_PIPELINE_SEED`). Its helper
kills also found that a write to a helper that had just died ended the app with
SIGPIPE (`Engine.swift`).

## Keeping it true

The model is only as good as its match with the code. When changing
`render()`, `reconcileBrightnessMatch()`, `endInteraction()`, `endCrop()`, the
held frame or the early frames, change the matching action here and run the
check. A new `await` is a new action.
