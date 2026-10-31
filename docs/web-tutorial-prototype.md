# Web tutorial prototype

Status: implemented as a web prototype. This document records the design and implementation choices.

## Try it

Run `node web/serve.mjs 8137` and open `http://localhost:8137/`. A fresh browser gets the first-open invitation; **Tutorial** in the header always starts it again. No engine rebuild is needed for these UI changes. To test first-open discovery again, remove only `localStorage.removeItem("negs.tutorial")` in the browser console and reload.

Implementation: `web/tutorial.js`, `web/tutorial.css`, and the session/render adapter in `web/app.js`. No new dependencies, cookies, tracking or server preference storage.

### Validation performed

- 30 unit/integration tests passed across tutorial, samples, presets, link codec and RAW white balance; 1 native-helper comparison skipped because `NEGS_NATIVE` was not configured.
- Real Chrome walkthrough checks passed on the desktop CPU path and the 320×568 touch-emulated WebGPU path: all nine steps, fresh defaults, slide-to-paper transition, Reset step, real preset dialog, completion, cancellation, failed-download retry, and round-trip restoration of RAW WB, crop, border, zoom/pan and UI preferences.
- Responsive Chrome checks passed at 320×568, 390×844, 844×390, 768×1024, 1024×768 and 1440×900, plus enlarged tutorial text and a reduced-height viewport. Step and exposure values survive viewport changes; End and navigation stay reachable.
- Discovery checks passed for dismissal/reload, malformed and denied storage, no initial sample download, and incoming preset priority.
- Existing browser sample-picker, preset and inspector checks passed. Static packaging and its sample/offline asset-policy checks passed with the new JS/CSS included.
- Actual iOS/Safari software-keyboard behavior and VoiceOver remain manual validation items; viewport emulation is not a claim of physical-device coverage.

```sh
node --test web/checks/tutorial.test.mjs web/checks/samples.test.mjs web/checks/presets.test.mjs web/checks/presets-link.test.mjs web/checks/raw-wb.test.mjs
node web/cdp.mjs http://localhost:8137/ web/checks/tutorial.mjs 3000
VIEW=320x568@2m node web/cdp.mjs http://localhost:8137/ web/checks/tutorial.mjs 3000
node web/cdp.mjs 'http://localhost:8137/?backend=cpu' web/checks/tutorial.mjs 3000
node web/cdp.mjs http://localhost:8137/ web/checks/tutorial-layouts.mjs 3000
node web/cdp.mjs http://localhost:8137/ web/checks/tutorial-discovery.mjs 3000
```

## Decisions

- An optional, interactive tour using the existing Sample 1 and fresh app defaults.
- Invite users on first open; never launch the tour or download the sample without a click.
- Keep a labelled **Tutorial** action available in the editor at every viewport size.
- Nine short steps: Film, Paper, Metering, Exposure, Film format, Halation, DIR Couplers, Print exposure, preset sharing.
- Use the approved copy below. No conditional direct-scan lesson: every step starts from the default negative/paper setup.
- Reuse real controls and the real rendering pipeline, not screenshots or copied sliders.
- Integrate the guide into the inspector. Do not place floating cards over the photograph or add a second bottom sheet.
- First prototype is web-only. Native applications can reuse the copy and flow later.

## 1. Discovery and first open

### First-open invitation

Extend the current empty state, rather than immediately opening a blocking modal:

> **Your photo, through film**
>
> Negs simulates exposing film, developing it, and making a print. This quick tour shows how each stage shapes your photo. You can skip it or leave at any time.
>
> **Start tutorial** · **Not now**

Show only the tutorial invitation on first open, without the sample/time note or the photo-selection controls underneath it. **Not now** switches to the normal image-selection screen; completing a first-run tour also returns there. The sample/time note belongs to the sample-loading screen, not the initial invitation. Returning users get the normal empty state instead.

On explicit **Start tutorial**, remember the choice, wait for engine readiness and open Sample 1. Show **Loading sample…** with **Cancel** until its first developed frame is presented. A RAW placeholder is not sufficient to begin teaching the film controls.

On **Not now**, remember dismissal and return to the regular empty state. Opening a file instead also removes the invitation for this visit; do not infer permanent dismissal or write a preference just from opening a file.

Do not put this invitation in front of an incoming `#preset=…` link, a file opened via the installed app, or the `?image=…` automation flow. Those launch intents take priority. They must not be overwritten by late engine initialization.

### Permanent entry point

Add a labelled **Tutorial** button beside the Negs name, before Open/Export in the top bar. It must remain visible, not be hidden in the footer or only inside the More menu.

At narrow widths, let the header use two compact rows: Negs + Tutorial above Open / Export / More. Do not squeeze all five items into a single 320px row. All touch targets are at least 44 × 44 CSS pixels.

While the tour is active, its own header says **Tutorial · 1 of 9** and includes **End tour**. No second start action is needed then.

If an edit is already open, clicking Tutorial first asks:

> **Take a quick tour?**
>
> The tutorial uses a sample photo and default settings. Your current photo and edits will be restored when you leave.
>
> **Start tutorial** · **Cancel**

Disable starting only during a conflicting operation, such as opening/exporting a photo, or while a modal is open. Starting from crop mode must first resolve the crop session, rather than silently committing it.

## 2. Remembering the choice, without tracking

Recommended: one localStorage key, `negs.tutorial`, containing only:

```json
{ "version": 1, "status": "completed" }
```

Statuses: `started`, `dismissed`, `completed`.

- Write `started` after an explicit Start; `dismissed` after Not now or End tour; `completed` after Finish.
- Any recognized saved choice suppresses the first-open invitation. A copy/version update must not automatically nag existing users again.
- Do not save step history, identifiers, timestamps, photos or settings in this key.
- Do not send this preference to a server or analytics service.
- Use the existing guarded `remembered()` / `remember()` helpers. Denied, unavailable, full or malformed storage must never break the tour.
- Keep the choice in memory as a fallback so the invitation does not reappear during the same visit.
- No automatic mid-tour resume after reload. Tutorial always starts fresh when requested.
- Clearing site data resets the preference. Different browsers/devices have separate choices; private browsing may forget it.

### Options and consent caveat

| Option | Remembers across reloads? | Across browser restarts? | Recommendation |
| --- | --- | --- | --- |
| In-memory flag | No | No | Fallback; zero persistent preference |
| sessionStorage | Yes, within that tab's session | Generally no | Reasonable if repeat invitations each visit are acceptable |
| localStorage | Yes | Yes, until cleared/evicted | Best fit for a dismissed/completed tutorial |
| First-party cookie | Yes, depending on expiry | Depending on expiry | Unnecessary: adds request traffic and server exposure |
| Account/server preference | Yes | Across signed-in devices | Unnecessary account and backend complexity |

A cookie banner is not determined by whether the API has “cookie” in its name: EU/UK terminal-storage rules can cover localStorage and sessionStorage too. A narrowly scoped preference remembering a user's explicit interface choice is commonly consent-exempt, but the applicable jurisdiction and exact use matter. Do not promise universal legal compliance. Document the purpose in the site's privacy/storage information. Do not mix this preference with analytics, advertising or tracking.

Suggested disclosure:

> Negs stores your tutorial choice in this browser so it doesn't ask again. This preference is not sent to us. Clearing site data resets it.

## 3. Tour session and safety

### Baseline

Build the baseline through the same defaults/pairing path as a fresh normal launch:

- `catalog.defaults`, cloned with nullable stock-controlled values intact;
- Kodak Portra 400 and its normal paired paper (currently Kodak Portra Endura);
- `io.scan_film = false` through normal film pairing;
- default crop, no rotation, default border, no Post adjustments;
- fit-to-view, Lab pane, first relevant tab;
- Sample 1, `DSC_6377.dng`, from `DEFAULT_SAMPLE` in `web/samples.js`.

Ignore URL film/paper overrides for the temporary tour baseline. Keep the user's device capabilities, engine choice, offline preferences and decoder safety limits; those are not photographic defaults to reset.

Do not use `resetAdjustments()` as the entire baseline operation: it intentionally preserves film, paper, rotation and Image Size. Do not maintain a separate hard-coded copy of the engine's numeric defaults.

### One small experiment per step

Every step starts from that baseline, including when going Back. Changes are local to that step, not cumulative across the entire tour. Explain this once in the introduction/loading panel:

> Try the controls as you go. Each step starts fresh.

This keeps Paper visible even if someone tries a slide film in step 1, and prevents experimental metering/format settings from obscuring later lessons. Do not reset while someone is dragging or selecting a value. Reset only on an explicit step transition, then render the fresh state.

A small **Reset step** action restores the baseline without leaving the current explanation. No slider movement or exact value is required to unlock Next. No automatic advancement.

For each step, expose the real relevant control(s) and effect switch, reveal their section, highlight them with the existing amber accent and let the user experiment. Keep the ordinary photo pan/zoom and Show Original available. Do not add a custom A/B renderer to this prototype.

### Protect an existing edit

Before changing the editor, capture the current edit and presentation state in memory:

- film, paper, complete params;
- original photo descriptor and existing byte-buffer reference, not another copy of the bytes;
- crop, border and rotation;
- view zoom/pan;
- Lab/Post pane, mobile tab, sheet size, disclosure state and inspector scroll position.

Use the existing worker; do not keep a second full-resolution decoded photo/GPU pipeline alive. Returning to an earlier photo may require reloading it from the retained bytes.

On End, Cancel during preparation, failure, or Finish:

- If there was a prior photo/edit, restore it and its UI state; reload/redevelop through the existing source queue when necessary. Release tutorial-only resources.
- If there was no photo and the user completes the tutorial, clear the temporary sample and show normal photo selection. Leaving early can retain the sample for exploration. On a preparation failure, restore the original empty editor.
- Restore saved layout preferences. Tutorial-driven tab, disclosure and sheet changes must not overwrite the user's remembered UI layout.
- Do not report restoration as complete before the correct photo is actually presented.

If a user explicitly opens/drops another photo or applies a preset during the tour, end the tour first, then perform the requested action. A late sample fetch/render must never replace their new photo. Serialize these transitions and use generation tokens/cancellation; do not race restoration against the new open.

## 4. Responsive walkthrough design

### Common structure

One guide component in the inspector, using the existing dark palette, typography and amber accent:

```text
Tutorial · 3 of 9                         End tour
Decide where to measure
Like a camera's light meter, ...

[ highlighted real Metering control ]

Reset step                         Back     Next
```

The illustration above is a content order, not a requirement to duplicate or move control implementations. Add stable tutorial anchors to the real control wrappers. Keep the guide and target in the same inspector region.

Avoid dimming the photograph: seeing the effect is the point of the tour. Use a border/background emphasis around the active controls; no huge box-shadow spotlight, animated arrow or click-blocking overlay is needed. Keep progress textual, not nine tiny tappable dots.

### Desktop / roomy landscape

- Retain the photo + right inspector arrangement.
- Use a tutorial rail around 360px wide when space permits; retain the existing narrower rail when widening would squeeze the preview.
- Put progress/title/copy at the top of the rail; Back/Next in a pinned footer.
- The middle is the scrollable real inspector, automatically on Lab and scrolled to the target.
- Open the relevant disclosure and outline the active row(s). Leave their real labels, values and reset affordances intact.
- Other sections can remain visible but do not let unrelated edit actions silently invalidate the lesson: hide/disable unrelated photographic controls for the prototype. Zoom/pan and original comparison stay available.
- If long copy/large text makes the fixed guide too tall, put explanation and controls into the same middle scroll region. Navigation must remain reachable without covering a control.

### Phones / portrait tablets

Reuse the existing bottom-sheet layout and its current breakpoint:

`(max-width: 899px) and (orientation: portrait), (max-width: 639px)`

During the tour, the sheet becomes a **focused lesson panel**:

- Progress and End tour at its top.
- Explanation, then the active real control(s), in one scroll region.
- Back/Next pinned at the bottom, above the safe-area inset.
- Hide the sample switcher, normal section tabs, engine footer and unrelated sections while teaching. Their space is more useful for the lesson.
- Keep just the relevant group header/switch and controls. E.g. Halation shows Amount and Spatial scale, not all six advanced settings; DIR shows Amount. Restore everything when leaving.
- Start with roughly 55–60% of the available height for the panel and the rest for the preview. Let the user resize it with the existing grabber, without permitting a collapsed state that hides navigation.
- Respect `100dvh`, safe-area insets and visual viewport changes. No fixed-height assumption based on a particular phone.
- Do not open another floating coachmark or bottom sheet on top of this panel.

For preset sharing, reveal the actual More action in its normal header location and highlight Export preset in its menu. If the user opens the real Export preset dialog, suspend the guide's focus/keyboard handling until that dialog closes. They can inspect/copy/download a preset, but none is required to complete the tour.

### Small landscape / short windows / large text

- Keep side-by-side preview/lesson when there is sufficient width, including a typical landscape phone, rather than spending nearly all its short height on a bottom panel.
- In narrow stacked layouts, explanation and controls scroll together and the navigation stays pinned. Allow a smaller preview when the keyboard is open; never place controls behind the keyboard.
- Size based on the available viewport and measured content, not user-agent names. If the usual desktop pinned explanation leaves too little control space, switch to the shared scrolling-body variant.
- At 200% text zoom, preserve readable text and access to all actions even if the explanation/control cannot be seen simultaneously. No horizontal page scrolling or clipped buttons.
- On resize/orientation change, keep the current step and values. Re-reveal its tab/target only after the new layout settles; do not restart or reset the lesson.
- Honor reduced motion. Avoid smooth scrolling during viewport changes or slider interaction.

### Accessibility

- Tour is a labelled non-modal region, not a full-app modal: the photo and taught controls remain usable.
- Focus the step heading (`tabindex=-1`) on explicit Back/Next, then allow normal Tab navigation to the active controls and footer.
- Announce step/title once via a polite status region; do not announce every rendered frame or every slider tick.
- Escape ends the tour except when a real dialog, menu, native picker or text editor owns that key. Do not steal range-input arrows or Space/Enter from controls.
- End tour and Finish restore focus to the Tutorial button, unless the user is intentionally opening a new photo/dialog.
- Visible keyboard focus; descriptive control labels; at least 44px touch targets for tour navigation; sufficient contrast.
- Focus must not move on every render or while dragging sliders.

## 5. Steps and approved copy

The guide has nine steps. Introduction and completion do not count as steps.

| Step | Target / UI preparation |
| --- | --- |
| 1 Film | Stock tab; `#film` |
| 2 Paper | Stock tab; `#paper`, visible under fresh negative baseline |
| 3 Metering | Camera tab; `camera.auto_exposure_method`; keep Auto exposure on for the lesson |
| 4 Exposure | Camera tab; `camera.exposure_compensation_ev`; baseline print compensation stays on |
| 5 Film format | Camera tab; `camera.film_format_mm` |
| 6 Halation | Film tab; open Halation; switch, Amount and Spatial scale |
| 7 DIR Couplers | Film tab; open DIR Couplers; switch and Amount |
| 8 Print exposure | Print tab; open Enlarger; `enlarger.print_exposure` |
| 9 Preset sharing | Real More menu → Export preset; actual dialog optional |

### 1 — Choose your film

Each film responds differently to light and colour. Your choice shapes the photo’s colours, contrast, and grain. Start here to find a character you like—then use the other controls to fine-tune it.

### 2 — Choose your print

In a darkroom, light passes through the negative onto photographic paper. The paper adds its own colour and contrast, changing the final photo. Try a few papers with the same film—the difference can be surprisingly large.

### 3 — Decide where to measure

Like a camera’s light meter, this decides which parts of the photo guide automatic exposure. Center-weighted favours the middle; Highlight-weighted favours bright areas and usually gives a darker result. Try another mode if the starting exposure feels off.

### 4 — Change how the film sees light

This changes how much light reaches the simulated film, affecting colour, contrast, and grain. With the enlarger’s Exposure compensation on, print brightness stays roughly steady. Use Print exposure later to make the finished photo lighter or darker.

### 5 — Small film, bigger texture

Smaller film needs more enlargement to make the same-sized picture, so grain and glow appear larger. Lower this value for more visible texture; raise it for a finer look. It doesn’t crop your photo.

### 6 — Let highlights glow

Some light passes through film and reflects back into it, creating a warm glow around bright highlights. Raise Amount for a stronger glow, or Spatial scale to spread it wider. Look around lamps, reflections, and bright edges.

### 7 — Colour shaped by chemistry

During development, chemicals released in one colour layer slow development in nearby layers. This helps separate colours and makes edges look crisper. Lower Amount for gentler colour and less edge contrast.

### 8 — Set the final brightness

An enlarger shines light through the negative to make a print. On ordinary photographic paper, more light makes the print darker. Lower Print exposure to brighten your photo; raise it to darken it.

### 9 — Keep a look you like

Save your settings as a preset to reuse them on other photos. Export it to share the look, or send it as a link.

Additional small hint, matching the actual web preset format:

> Presets include Lab settings, not your photo, Post adjustments, crop or border.

Use **Next** on steps 1–8 and **Finish** on step 9. Completion can briefly say:

> **You’re ready**
>
> Film and paper shape the look. Print exposure sets the final brightness. You can reopen this tutorial whenever you like.

## 6. Implementation outline

Prefer a small app adapter, not a tour module that directly mutates global app state.

### New files

- `web/tutorial.js`: copy/step definitions, guarded preference handling, session controller and accessible guide UI.
- `web/tutorial.css`: responsive guide/focus-mode styles; reuse the app's CSS variables.
- `web/checks/tutorial.test.mjs`: preference, state transition and cancellation tests with a fake adapter.
- `web/checks/tutorial.mjs`: browser checks through the existing CDP harness.

### Existing integration points

- `web/index.html`: permanent Tutorial button, first-open invitation, guide container and stylesheet.
- `web/app.js`: adapter for defaults/session snapshot/restore, targets, tab/disclosure reveal, first-frame readiness, launch-intent priority and operation cancellation.
- `buildControl()` / `buildControls()`: stable `data-tutorial-*` anchors derived from parameter paths/group keys. Do not target labels by translated text or use `nth-child` selectors.
- `showPane()`, `showTab()`, `setSheet()` and disclosure toggle persistence: allow temporary presentation changes without writing normal user preferences.
- `web/samples.js`: reuse `DEFAULT_SAMPLE` and cancellation. Add an explicit load outcome if needed; current picker/open functions can swallow errors or no-op when the sample is already selected. The tour needs a reliable success/error result.
- Render/presentation lifecycle: expose a promise/event for the correct generation's first developed frame. `open()` returning after scheduling a render is not enough.
- `web/preset-ui.js`: coordinate real modal/menu focus and avoid a conflicting incoming preset invitation.
- `web/offline.mjs` and static packaging: ensure new JS/CSS are packaged and versioned with the app shell. Do not modify generated `public/` files by hand.

Suggested controller states:

`idle → preparing → active(step) → finishing/restoring → idle`

Preparation can fail/cancel back to idle. Each asynchronous transition has a generation token. Disable duplicate Start/Next actions while their transition is pending; End/Cancel remains available. Surface a failed sample load with **Retry** / **Back to editor** rather than an empty lesson.

### Deliver in two small passes

1. **Functional responsive prototype:** invitation + persistence + permanent button; default sample session; all nine copy steps; real control focus mode; safe restore/cancel; desktop and phone layouts. No extra dependency or new rendering path.
2. **Validate and polish:** real-device sizing, keyboard/VoiceOver, text zoom, offline/reload/race tests, then adjust copy density/spacing using screenshots. Verify Sample 1 visibly demonstrates grain, halation and colour effects before claiming the learning flow is done. If an effect is too subtle, discuss the sample choice rather than secretly exaggerating defaults.

## 7. Acceptance checks

- Fresh browser: invitation is prominent; editor remains usable; no sample downloaded until Start.
- Not now/Start/Finish choices survive reload; blocked/malformed storage does not throw.
- Tutorial is always discoverable from the normal editor, including a 320px phone.
- Every new step starts from the same engine-derived baseline; paper is present and print exposure starts at multiplier 1, not EV 0.
- A slide choice in Film does not hide or disable Paper on the next step.
- Back/Next/Reset step do not require changing a value; rapid taps do not race.
- Sample fetch/decode/render errors, CPU fallback and offline-without-cached-sample have a recoverable path.
- End during downloading or rendering cannot show a late sample over the restored/new photo.
- Existing photo/edit, RAW WB, crop, border, zoom and layout survive a tour round-trip.
- Existing incoming preset/file-launch/automation flows are not interrupted by first-open UI.
- Real preset dialog opens/closes without competing focus handlers or accidental tour completion.
- No tutorial preference is transmitted or used for tracking.
- Test at 320×568, 390×844, 844×390, 768×1024, 1024×768 and 1440×900 CSS pixels, plus 200% text zoom, reduced motion and software keyboard open.
- Resize/rotate during a step preserves its state; no horizontal overflow, hidden navigation, overlaid target or second mobile sheet.
- Run existing sample/preset/control browser checks in addition to the new tutorial checks; tour-disabled editor layout must remain unchanged.

## Existing worktree

The prototype was implemented additively against the existing web/app/engine/preset work. Unrelated worktree files were not overwritten or reverted.
