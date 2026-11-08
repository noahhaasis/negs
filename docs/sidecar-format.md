# Photo sidecars (`.negs`), schema 1

Negs remembers each photo's rating, flag and edits in a small JSON file beside
it. There is no catalog: the folder is the library, and the sidecars travel
with it (Dropbox, a friend's Mac, the Windows and Linux builds). Design and
decisions: [tickets/30-library-design.md](../tickets/30-library-design.md).
macOS implementation: `macos/Sources/NegsApp/Sidecar.swift`. Example files,
which every reader is tested against: [sidecar-fixtures/](sidecar-fixtures/).

## Name and place

- `IMG_0412.CR3` → `IMG_0412.CR3.negs` in the same folder: the photo's whole
  file name plus `.negs`. RAW+JPEG pairs get one each, and the name never
  collides with a Lightroom, Capture One or darktable `.xmp`.
- Visible, not a dotfile, so it survives copying and people see what to share.
- Written only once a photo has a rating, flag or edit. Opening or browsing a
  photo writes nothing; neither does Negs's automatic *Match camera
  brightness* solve on a photo the user hasn't edited.

## Writing

- **Atomically:** write `.IMG_0412.CR3.negs.tmp` (hidden, so folder scans and
  watchers skip it) beside the photo, then rename it over the sidecar.
- **When:** 1 s after the last edit, on switching photos, and at quit. Never on
  the UI thread.
- **Unknown keys survive.** Read the file as a JSON object and keep it. On
  write, change only the fields you own and know; every other key, at every
  level, is written back unchanged. Inside `edits`, a stored `params`, `crop`
  or `border` path that the writer's defaults table and engine don't know is
  also kept. Otherwise an older build, or a port that lags behind, would
  erase what a newer one stored.
- **Merge with the file as it is now,** not as it was when the photo opened,
  so a rating set elsewhere meanwhile stays.
- **Never write over a file you can't read:** invalid JSON, a newer `negs`
  schema, or a `defaultsVersion` this build has no table for. Open the photo
  with the defaults, say why, and leave the file alone.
- **Remove the file** when a write leaves one variant with nothing in it
  (no rating, flag or edits) and no other keys in the file.
  A duplicate may hold nothing but its `id` (a copy of an unedited photo);
  it still counts, so a file with two variants stays.
- Keys are written sorted, two-space indented, with a final newline, so the
  same content gives the same bytes.
- At most 1 MB; a larger file isn't Negs's and is left alone.

## Contents

```json
{
  "app": "0.15.0",
  "defaultsVersion": 1,
  "negs": 1,
  "variants": [
    {
      "id": "9b1e44c0",
      "rating": 3,
      "flag": "pick",
      "edits": {
        "film": "kodak_portra_400",
        "paper": "kodak_portra_endura",
        "params": { "prep": { "highlights": -40 }, "io": { "rotate_cw": 1 } },
        "crop": { "rect": [[0.1, 0.05], [0.8, 0.9]], "aspect": "freeform" },
        "border": { "enabled": true, "style": "keyline" },
        "brightnessMatch": { "ev": 0.45, "highlightLimited": false }
      }
    }
  ]
}
```

| Key | Type | Meaning |
| --- | --- | --- |
| `negs` | integer | Schema, 1. A reader refuses a larger one (and never writes over it). |
| `app` | string | Version of the app that last wrote the file. Informational. |
| `defaultsVersion` | integer ≥ 1 | The frozen defaults table the edits are stored against (below). |
| `variants` | array | One entry per copy of the photo: the photo itself first, then its duplicates (virtual copies). |
| `variants[].id` | string | Opaque, unique within the file. Negs makes 8 lowercase hex digits. |
| `variants[].rating` | integer 0–5 | Stars. Absent means 0. XMP's −1 for rejected is *not* used here; see `flag`. |
| `variants[].flag` | `"pick"` or `"reject"` | Absent means unflagged. Independent of the rating: a photo can be a 4-star reject. |
| `variants[].edits` | object | Absent: no edits, the photo opens as a new one. |

### `edits`

Only what differs from the defaults table is stored, except the film and
paper, which are always written.

| Key | Meaning |
| --- | --- |
| `film`, `paper` | Stock ids from the engine's catalog. A reader that lacks one shows the table's film or paper (or its first) with a notice, and writes the saved id back unless the user picks another. |
| `params` | The engine's parameters (the catalog's `defaults` shape) that differ from the table's `params`. Objects nest; any other value (number, string, bool, null, array) replaces the table's value whole. Values are compared exactly, so they read back bit for bit. |
| `crop` | Differences from the table's `crop`: `rect` `[[x, y], [width, height]]` as fractions of the photo before rotation, `aspect` (`original`, `freeform`, `square`, `16:9`, `3:2`, `4:3`, `5:4`, `7:5`, `custom`), `custom` `[width, height]`, `angle` in degrees (±45). |
| `border` | Differences from the table's `border`: `enabled`, `style` (`white` is the solid mat, `keyline`, `film`, `film35`), `export`, `instagram`, `width`, `filmWidth`, `line` (fractions of the long edge), `color`, `lineColor` (`#rrggbb`). |
| `brightnessMatch` | Present while Prep › *Match camera brightness* is on. `ev`: the solved exposure, if solved; `params.prep.exposure_ev` holds the total the engine sees (the match plus the user's own exposure). `highlightLimited`: the solve held exposure back for the highlights. `solved: false`: the stored `ev` was solved for earlier settings, so solve again on opening; absent means the `ev` belongs to these edits and is not solved again. |

### Reading the edits

1. Load the table named by `defaultsVersion`.
2. Lay `params`, `crop` and `border` over the table's (`overlay`: objects
   merge key by key, anything else replaces).
3. Fill in the engine's current defaults for parameters the table doesn't
   have, and drop the ones the engine no longer has, as the app does with
   any saved edit (`JSONValue.restoring` in `Parameters.swift`).

Check the result: crop rectangle within `[0, 1]` with positive size, angle
within ±45°, `io.rotate_cw` a whole number 0–3. A sidecar that fails is not
applied and not written over.

## Defaults tables

`data/sidecar-defaults/v<n>.json` holds `defaultsVersion`, `film`, `paper`,
`params` (the engine catalog's defaults, with
`io.lens_correction.enabled` on, as the app sets it for new edits),
`border` and `crop`. A diff only means something against the defaults it was
saved with, so this works like Lightroom's process versions:

- Tables are **frozen**: a release that changes a default ships the next
  table (`scripts/sidecar_defaults.py --version <n>`) and never edits an old
  one. The macOS tests hash every table and fail if one changes, and fail
  while the newest table differs from the engine's current defaults.
- New sidecars get the newest table. A writer keeps a file's
  `defaultsVersion` and stores its diffs against that table. Old photos
  move to newer defaults only by an explicit *Update to current defaults*
  (not in v1).

## Session

The app's own `session.json` (macOS schema 3) keeps only the last photo's
path, bookmark and window state. Schema 2 sessions held that photo's edits;
the first launch of a sidecar-aware build restores them as before and then
writes them to the photo's sidecar.

## Not in schema 1

Tags (`variants[].tags`), colour labels, XMP ratings beside the photo,
sidecars kept in app data when sidecars are turned off. Exports embed the
rating as `xmp:Rating` (−1 for rejected), which is the export's metadata,
not the sidecar's.
