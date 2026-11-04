# Handoff: "Keep paper when changing film" setting (macOS and Windows)

Prepared 2026-11-02. Linux has it (on `main`, `linux/`); please port it to the Mac app
(`macos/`) and the Win32 app (`windows/`). One agent per platform; each section below is
self-contained. Commit straight to `main` (see `AGENTS.md`).

## The feature

Today, choosing a film also switches the paper to the film's own paper (its `target_print`
in the catalog) and resets the print development time. Some users want to keep a paper they
picked. The new setting turns that off.

- **Setting**, in Settings next to "Film, paper and preset previews":
  - Label: **Keep paper when changing film**
  - Subtitle/help: *Leave the paper as it is when you choose another film, instead of
    switching to the paper made for that film.* (let it wrap)
  - **Off by default** (off = today's behaviour, paper follows the film).
  - Preference key: `keep_paper` (Linux: `Preferences.keep_paper` in the session's
    `preferences`). A missing value means off.
- **When on**, a film change still does everything else it does now: its own development
  time, grain/DIR/halation presets, and scan mode (slides still scan directly). Only the
  paper, and the paper's print development time, stay as they are.
- **Applies only to choosing a film by hand** (the film menu and the film preview grid).
  It does **not** apply to:
  - applying a preset (a preset sets its own film and paper — Linux's `presets::apply`
    doesn't go through the film handler),
  - the state reset when the catalog loads / a photograph opens (that pairing stays).
- **Film preview tiles** show what a click would develop, so with the setting on they
  render each film on the *current* paper, not the film's own. Toggling the setting must
  make the film tiles stale and re-render (Linux just re-runs the thumbnail pump; tile keys
  include the paper, so they go stale on their own).
- Paper menu/grid and undo are unchanged; a film change is still one "Change Film" step.

Linux reference: `linux/src/catalog.rs` `choose(state, stock, keep_paper)` and its test
`keep_paper_leaves_the_paper_alone`; the switch in `linux/src/preferences.rs` `settings()`;
callers in `linux/src/inspector.rs` (menu), `linux/src/stock_browser.rs` (`request` for
tiles, and the tile click).

## macOS

**Done 2026-11-02** (uncommitted on `main` at the time of writing): `PreviewRules.choosing`
and `request(for:)` take `keepPaper`, and `pairFilm` takes `paper:` (default true, so the
catalog-load pairing is unchanged). The setting is `Previews.keepPaper` (UserDefaults
`keep_paper`), toggled through `EditorModel.setKeepPaper`, which re-pumps the tiles. Tests:
`testKeepPaperLeavesThePaperAlone` and `testMissingSettingMeansThePaperFollowsTheFilm` in
`PreviewTests.swift`.

- Pairing lives in `macos/Sources/NegsApp/Previews.swift`: `PreviewRules.choosing(_:paper:in:stocks:)`
  calls `pairFilm(in:stocks:)`, which sets scan mode **and** the paper. Split it (or add a
  `keepPaper` parameter) so `choosing` skips only the paper part when the setting is on.
  `choosing` is used both for the real film change and for tile requests (`request(for:…)`),
  so passing the flag there covers both.
- Leave the `pairFilm` call in `EditorModel.swift` (catalog load, ~line 307) pairing as now.
- Store it in UserDefaults under `keep_paper` (like `Previews.defaultsKey = "previews"`),
  default false. Add the Toggle in the Settings view in `Updates.swift`, next to the
  "Film, paper and preset previews" toggle, with the subtitle as help text.
- Toggling must re-pump previews so film tiles redraw (their key includes the paper).
- Add a test beside `macos/Tests/NegsTests/PreviewTests.swift`: with the flag on, choosing a
  film whose `target_print` differs keeps `paper` and its `print_render.development_time`;
  with it off, it switches as today.

## Windows

**Done 2026-11-02.** `state::choosing` takes `keep_paper` and `state::pair_film` a `paper`
flag; the catalog-load pairing (`Model::pair_film`) always pairs. The film menu, the film
tiles and their thumbnail requests all go through `choosing`, so with the setting on the film
tiles render on the current paper and redraw when it changes (their key includes the paper).
The preference is `keep_paper` in `settings.json` (`#[serde(rename)]`, the Mac's key; missing
means off), set through File ▸ Settings… next to the previews switch, under a "Film & Paper"
heading. Tests: `model::tests::keep_paper_leaves_the_paper_alone` (film menu, one "Change
Film" step, catalog load still pairs), `previews::tests::keep_paper_leaves_the_paper_alone`,
`a_film_tile_follows_the_keep_paper_setting`, and the settings-file test. Checked by hand in
the release build with a D750 NEF: with it on, Kodak Vision3 250D kept Fujifilm Crystal
Archive Type II; off, Kodak Vision3 500T switched to Kodak Vision 2383; the setting persisted.

- Pairing lives in `windows/src/model.rs`: `choose_film` calls `pair_film`, which sets
  `io.scan_film` **and** the paper. Skip only the paper part in `choose_film` when the
  setting is on. Keep the `pair_film` call in the catalog-load/reset path (~line 634) as is.
- Store it with the other preferences in `windows/src/session.rs` (key `keep_paper`,
  default false — it mirrors the Mac's UserDefaults keys).
- Settings UI is `windows/src/ui/dialogs.rs` `settings()`. It's currently an updates-only
  task dialog that bails out when updates aren't configured; the setting must be reachable
  regardless, so restructure it as needed (and alongside the previews setting if that has
  landed on Windows by then).
- If the Windows film preview grid exists by the time you do this, make its film tiles use
  the same rule and redraw when the setting changes.
- Add a unit test in `model.rs` mirroring the Linux one.

## Done when

Each platform: setting present and off by default, persisted across launches; with it on,
changing the film keeps the paper (and the film tiles show the current paper); with it off,
behaviour is unchanged; tests pass. Tell Noah what you checked by hand.
