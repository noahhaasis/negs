# Note: a crop move costs the next quick edit its reuse (Longest side)

Noah's to-do, written on 2026-11-07 from the Windows port review. It applies to macOS and Windows
alike, and probably Linux. Nothing has been changed yet.

## What happens

- **Mode:** **Longest side**, with a crop.
- **The crop move itself:** panning or straightening the crop renders nothing, which is right.
- **The next post-only edit** is an edit the helper could answer from its kept output (contrast,
  for example). It gets a quick frame plus a full render instead of a reuse.

| Post-only edit (`post.contrast`), 6.3 MP, Longest side 3072, crop 0.8 × 0.8 | Renders | Settled |
|---|---|---|
| crop unchanged | `[Settled]`, reused | 0.15 s |
| after a pan, canvas at 0.25 | `[Quick, Settled]` | 0.41 s |
| after a pan, no canvas | `[Settled]`, the helper reuses | 0.16 s |
| after a quarter turn, canvas | `[Quick, Settled]` | 0.41 s |

The same case on the 33 MP ARW (Vulkan) takes about 2 s instead of being nearly instant.

## Why

- The app's reuse key is the outgoing render request, so it contains `crop.x`, `crop.y` and
  `crop.angle`. A pan changes the key even though the render doesn't change.
  - Windows: `windows/src/model.rs` around 2222–2225 on `win-nov5`.
  - Mac: `reuseKey` in `macos/Sources/NegsApp/EditorModel.swift` 1015–1020 and 1034–1035, and
    `cropJSON` 1699–1703.
  - The edit itself drops the position: `rendered` in `Parameters.swift` 204–210.
- The helper keys its own reuse on the resolved working size
  (`crates/spektrafilm-native/src/main.rs` 778–782), so it would have reused.
- **Quarter turns:** the helper's key also includes `params.io.rotate_cw`, which doesn't affect a
  render either. That miss predates the quick-frame work: the helper never reused across a turn.

## Fix idea (not designed or tested)

Key the app's reuse decision on what the helper actually renders: the crop's extent, or the
resolved working size, rather than its position and angle. The same thought applies to
`rotate_cw` in the helper's own key. Do it on both platforms together, to keep them alike.

To test it, extend `border_crop_and_rotation_edits_and_the_longest_side_crop`. That test, in
`windows/src/model/lifecycle_tests.rs` on `win-nov5`, currently pins the reuse loss. Add a Mac
case to `QuickFrameTests.swift`.

Source: the B2 review in `docs/windows-nov5-port/B2-quick-frame.md` (S2, N2), in the main
checkout (uncommitted).
