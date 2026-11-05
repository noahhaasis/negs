# Handoff: quick frame first for film and paper picks, Windows app (ticket 28)

Prepared 2026-11-05 for an agent on Noah's Windows test box (the desktop session, checkout
`C:\Users\Noah\Projects\negs`). Port ticket 28 from the macOS app to the Win32 app
(`windows/`). Linux has its own handoff, [handoff-quick-frame-linux.md](handoff-quick-frame-linux.md),
and may be in progress at the same time; the two touch different directories.

## Where to work

- The macOS work is on `main` (commits `e4a9f92` hold a frame for the brightness match,
  `30de494` and `8a73944` ticket 28). **Ask Noah to push main** if `git fetch origin` doesn't
  show them.
- Check `git status` in the checkout first and don't disturb uncommitted work. Make a worktree
  on a new branch from it: `git -C C:\Users\Noah\Projects\negs fetch origin` then
  `git -C C:\Users\Noah\Projects\negs worktree add ..\negs-quick-frame -b quick-frame-windows origin/main`.
- Commit on `quick-frame-windows`. **Don't merge into main and don't push** unless Noah asks.
- Commit dates: one minute apart, continuing from the branch's last commit
  (`GIT_AUTHOR_DATE` and `GIT_COMMITTER_DATE`).
- End commit messages with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Read first

1. `tickets/28-film-click-screen-frame-first.md` is untracked on the Mac; the summary below is
   what you need. The user-facing description is in
   [native-macos-ui.md](native-macos-ui.md) (the paragraph after the drag frames).
2. The macOS implementation, the behavioural reference, all in
   `macos/Sources/NegsApp/EditorModel.swift`: `RenderStep`, `renderStep(...)`, `reuseKey`,
   `render()`, `settleQuickFrame()`, `showsQuickFrame`, `heldPreview` / `showHeldPreview()`,
   the guard of `reconcileBrightnessMatch()` and its completion, and `endInteraction`.
3. `macos/Tests/NegsTests/QuickFrameTests.swift`: what to test (step decision, reuse key, and
   the lifecycle tests with a real helper).
4. The Windows render path: `windows/src/model.rs` `changed`, `schedule_render`, `render`,
   `render_finished`, `should_show`, `end_interaction`, `reconcile_brightness_match` and its
   completion; `ui/canvas.rs` `screen_pixels_per_photo_pixel`. [windows-app.md](windows-app.md)
   for building and testing.

No helper or engine change: the helper already renders `interactive_scale` frames (ticket 06).
The settled render is byte-identical after a quick frame; the helper test
`a_quick_frame_before_a_film_change_leaves_its_settled_render_unchanged` covers it.

## What to build

The render step, decided when a render starts:

- **drag**: a slider moves and the canvas shows fewer pixels than the photo (as today).
- **quick**: not interacting, and all of: a developed frame of this photograph is on screen (not
  the saved launch frame), the canvas scale is below 1 with scale² ≤ 0.5, the photo frame
  (`image_size`, i.e. `photo_width × photo_height`) is ≥ 4.5 MP, this edit hasn't had its quick
  step yet, and the request isn't one the helper answers by reusing its kept output.
- **settled**: everything else (export size, as today).

State to add (names from macOS):

- `quick_step: Option<(revision, generation)>`: the edit whose quick frame is done (shown, held,
  or failed). Set when a quick render of the current revision finishes; cleared after a settled
  render of the current revision runs, failed or not. Also set in `end_interaction` (letting go
  of a slider goes straight to export size, as before: the drag frames were canvas-size).
- `quick_frame: Option<(revision, generation)>`: the shown frame is that edit's quick frame.
- `helper_output: Option<Value>`: the last *settled* request without `preview_path` and with
  `params.post` set to null. Clear it right before sending any render (the helper drops its kept
  output at the start of every render, and keeps none after a failure); set it after a settled
  render returns. Equal to the next request's key ⇒ a post-only edit ⇒ no quick frame (a quick
  frame would destroy the reuse and turn a ~10 ms answer into a full render).

Behaviour:

1. **After a quick frame**: show it with the "Preview · W × H · developed in …" status, leave
   `preview_current` false (Save, thumbnails and the stock previews wait), then start the
   settled render at once if nothing below holds it. No delay (a 400 ms debounce was tried on
   macOS and dropped).
2. **Superseded quick frames still show**: a quick frame whose revision is no longer current is
   shown if the generation matches (holding Ctrl+Z, fast clicks); the current edit renders next.
   Settled frames keep the existing rule: only the current revision.
3. **Brightness match** (Prep › Match camera brightness, on by default for new photos):
   - Port macOS `e4a9f92` too: a settled frame that lands while the match is pending
     (`brightness_match_pending`) is held, not shown, and shown when the match completes without
     changing the render; a change drops it.
   - A quick frame that lands while the match is pending is held the same way.
   - The match may start once a quick frame of the current revision is shown or held (the helper
     then holds the decode for these settings), not only after a settled frame.
   - The settled render waits while a match is running or a frame is held. On the match's
     completion, show a held frame, then start the settled render if the quick step is current.
     A match that changes the exposure is a new edit, with its own quick frame.
   - Visible: previous frame → matched quick frame → final frame. The old exposure never shows,
     and the old exposure's export-size render never runs.
4. **Failures**: a failed quick render is silent; the settled render that follows reports its
   own error once. No retry loop (that's why `quick_step` is cleared after the settled render).
5. **Unchanged**: drags; zoomed in past half the pixels; a new photograph's first frame; the
   restored launch frame; canvas-only edits (crop, rotation, border).
6. The settled render still saves to the rendered-preview cache; the quick frame doesn't.

## Check

- Unit tests for the step decision and the reuse key (mirror `QuickFrameStepTests`).
- Lifecycle tests with the real helper, a ≥ 6 MP test photo and a fixed canvas scale (macOS adds
  `CanvasScale.fixed`; add an equivalent test hook): film click = quick then settled, settled
  frame bytes equal to a run without quick frames, rapid switching ends on the last film settled
  with no extra renders, undo right after a quick frame, post-only edits (also right after a film
  change), a drag after a quick frame and a plain drag (no quick frame on release), the match
  running before the settled render (exactly one settled render), a failing edit (`io.upscale_factor`
  = 5) reporting once.
- Timings on a large RAW, before and after, match off and on: time to the first visible change
  and to the settled frame. macOS, 60 MP, mains power: first change 163 ms (was 2355), settled
  2498 ms (was 2355); match on: 597 ms (was 4921), settled 2916 ms (was 4921).
- Memory: the helper's idle footprint grows by the canvas-size source copy (macOS: +211 MB on
  60 MP), which a drag already allocated. Note the Windows number.
- Report back: what you built, test results, timings, anything that differs from macOS.
