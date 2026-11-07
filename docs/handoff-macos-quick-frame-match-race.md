# Handoff (macOS): wasted render when matching turns on during a quick frame

**Who:** a macOS agent.
**Found by:** the Windows port's review of the quick frame (ticket 28), on 2026-11-07.
**Line numbers:** `macos/Sources/NegsApp/EditorModel.swift` on `origin/main` at `3c9e166`. Re-find
them by name if the file has moved.

## The bug

**Symptom:** turning on camera-brightness matching while a quick frame is on screen can send one
export-size render at the **old** exposure. The app then holds that render and throws it away,
so the matched frame arrives roughly one full render later (about 2.5 s at 60 MP). It never
shows a wrong frame; it wastes one render.

**Sequence:**
1. `settleQuickFrame()` (1146–1150) schedules the settled render as a `Task` that yields first
   (977–983).
2. Until `render()` sets `rendering` (1047), `reconcileBrightnessMatch()` can start the solve.
   Its guard checks `!rendering`, but nothing about `pendingRender` (1468).
3. `render()` checks only `!rendering, !closing` (1023), so it sends the settled render at the
   old exposure anyway.
4. If the match moves the exposure, that render is held (1127–1128) and then dropped by the edit
   (965). If the solve fails, the render was the right frame and nothing is lost.

**How likely:** the window is a single yield, so it's rare. It's still real.

## The fix

**Suggested fix:** in `render()`, return early when `quickStep == frame && matchTask != nil`.
The match task's completion already calls `settleQuickFrame()` (1483), so the settled render
follows the solve instead of racing it. Windows does this (see `windows/src/model.rs` on `main`,
the settled-render guard).

**Test:** add a case to `macos/Tests/NegsTests/QuickFrameTests.swift`. Hold a quick frame, then
turn matching on before the scheduled settled render starts, using a photo and film pair whose
match moves the exposure by more than 0.05 EV. Assert:
- no export-size render at the old exposure is sent
- exactly one settled render follows the solve

**Mutation check:** revert the guard and confirm the test fails.

## Also worth a look (minor)

`helperOutput` (1044, 1074) isn't cleared when the helper process restarts. A new helper has no
kept output. The result is harmless: the app skips a quick frame, thinking it can reuse, and
then does a full render.
- **Windows** clears it with a helper epoch counter in `EngineClient`.
- **Linux** fixed the same thing in `a6f4337`.

Clear `helperOutput` whenever the helper is relaunched.

## Rules

Follow the repo's usual macOS workflow. Don't push or merge without Noah saying so.
