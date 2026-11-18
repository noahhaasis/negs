# negs-library

The library half of the macOS app, without a UI: `.negs` sidecars, ratings
and flags, per-photograph edits, Copy and Paste, Duplicate, Delete, undo,
RAW+JPEG pairs, the filter, the selection, thumbnail looks and keys, and
`xmp:Rating` on exports. Plain Rust and `std::fs`, no dependencies, so the
Windows and Linux apps share it. The macOS Swift on `main` is the
reference; where it and the docs disagree, this follows the Swift.

## What it guarantees

- **Sidecars are written as the Mac writes them**, byte for byte:
  `JSONEncoder`'s pretty, sorted output with `" : "`, two-space indents,
  its escapes, `Double.description` numbers (ties to even included) and a
  trailing newline. Every fixture in `docs/sidecar-fixtures/` reads and
  rewrites to what the Mac's encoder gives for it (`tests/mac-encoded/`);
  the fixtures themselves use `": "`, the only difference.
- **Reading accepts what `JSONDecoder` accepts**: a BOM, duplicate keys
  (the first wins), trailing commas; and refuses what it refuses.
- **Writes are atomic** (a hidden `.NAME.negs.tmp` beside it, then a
  rename) and **merge with the file as it is on disk** at that moment. A
  change touches only the fields it sets; unknown top-level keys, unknown
  variant fields, unknown edit keys and parameter values this version
  doesn't know stay as they were.
- **An unreadable, too large or newer sidecar is never written.** The
  photograph opens with the defaults, and the problem is said once, in the
  Mac's words.
- **A sidecar with nothing left in it is removed**, so an action undone
  leaves no empty file behind.
- **Undo leaves the files byte for byte as they were** when nothing else
  changed them since. When another app did, a rating or flag undo puts back
  only the fields the action changed, merged with the file as it is now;
  an edits undo puts the file back as it was (as the Mac does).
- **What the library shows is what is on disk** after every call; the
  open photograph's unsaved edits are the one exception, until `save`.

## Which Swift each part mirrors

| Module | Swift |
| --- | --- |
| `json` | `JSONValue`, `PhotoSidecar.encode`, `JSONDecoder` quirks |
| `sidecar` | `PhotoSidecar`, `SidecarPatch`, `SidecarStore`, `SidecarDefaults` |
| `edits` | `PhotoSidecar.encodeEdits/decodeEdits`, `EditGroup`, `EditorModel.userEdits`, `newPhotoState`, `restoredState`, `holdsNoEdits` |
| `folder` | `PhotoFolder.listing`, RAW+JPEG pairing, `localizedStandardCompare` |
| `item` | `PhotoLibrary` items and summaries, `ratingText`, `flagText` |
| `filter` | `LibraryFilter`, `SheetWords`, `FilterMenu` wording |
| `selection` | `LibrarySelection` |
| `delete` | `DeletePlan` and its alert (`Delete.swift`), `PhotoTrash` |
| `thumbnail` | `PhotoSidecar.look`, `EditedThumbnails` keys |
| `xmp` | `ExportRating` (`BatchExport.swift`) |
| `library` | `PhotoLibrary` and the library half of `EditorModel`: `changeLibrary`, `saveLibraryChange`, `copyEdits`, `pasteEdits`, `changeEdits`, `duplicate`, `delete`, `undoDelete`, `finishSidecarChange`, `restoreSidecars`, `flushSidecar`, `sidecarEditsToSend`, `sidecarChangedOnDisk`, `followNewPhotoStocks` |

## Edits stay JSON

The crate never depends on the engine's parameter types. A photograph's
edits are `EditState { film, paper, params: Json, crop: Json, border:
Json, brightness_match, roll }`, in the sidecar's own shapes, and
everything the crate needs to know about the engine comes in as plain data
in `EditDefaults`: the catalog's `defaults` and `nullable_paths`, its
films (and which are positives) and papers, and Settings › New
photographs. `EditDefaults::for_table` stands in until the catalog
arrives. The front end converts `params` to and from the engine's
parameters at its edge.

## How a front end drives it

```rust
use negs_library::*;

let mut library = Library::new(folder, Config {
    data,                       // the engine's data folder (sidecar-defaults/)
    app: "0.18.0".into(),       // written into each sidecar as `app`
    bin: Box::new(RecycleBin),  // the platform's bin: impl Bin
    pairing: true,              // Settings › Show camera JPEGs separately, off
    sidecars: true,             // Settings › Save ratings and edits
    id_seed: None,
})?;
library.set_defaults(defaults_from_catalog);

// The grid and strip.
for item in library.shown(library.open_item()) {
    let name = library.name(&item);           // "IMG_0412.CR3 (2)"
    let summary = library.summary(&item);     // rating, flag, edited, problem
    let look = library.look(&item);           // names its edited thumbnail
}

// Developing a thumbnail (nothing opened or written): the stored edits as
// opening would show them, else the unedited state and its look.
let (state, match_solved) = library.thumbnail_state(&item)
    .unwrap_or((library.unedited_state(), false));
let unedited = thumbnail::unedited_look(&library.unedited_state());

// The edit view.
let opened = library.open(&item);             // saves the one before
show(opened.state); if let Some(p) = opened.problem { say_once(p) }
library.edit(new_state);                      // on every change
library.save()?;                              // a second after the last change,
                                              // on leaving, at quit (or close())

// Library actions: each returns an Outcome.
let outcome = library.set_rating(&selected, 3);
show_note(outcome.note);                      // "★★★", "Edits Pasted", ...
if outcome.open_changed { show(library.state()); drop_editor_undo(); }
if let Some(item) = outcome.open { library.open(&item); }   // after Undo of a delete
select(outcome.select);                       // new duplicates, what Undo put back

library.copy_edits(&item, EditGroup::standard());
library.paste_edits(&selected);
library.paste_from_previous(&selected);
library.reset_edits(&selected);
library.duplicate(&selected);

let plan = library.delete_plan(&selected);
let question = library.delete_question(&plan);   // ask; always when permanent
library.delete(&plan);

// Undo: the front end keeps one undo stack with the editor's own steps,
// and calls these for the library's, in order.
library.undo_name(); library.undo(); library.redo();
let (undo_steps, redo_steps) = library.undo_depth();   // did an action add one?

// The folder watcher (and window activation) calls:
let (changed_by_others, outcome) = library.refresh(interacting);
```

`Bin` is the one platform hook: `trash(file) -> where it went`,
`put_back(trashed, original)`, `available(folder)`, `remove(file)` and
its `name()`. `FolderBin` moves files into a folder, for tests.

Everything else is plain functions and types: `Filter` (with its words
and `to_json`/`from_json` in the Mac's settings form), `Selection`,
`thumbnail::key`, `xmp::embed_in_file(path, rating)` after an export with
`xmp::export_rating(&summary)`.

## Tests

```
cargo test --release -p negs-library
NEGS_LIBRARY_SEEDS=25 cargo test --release -p negs-library --test state
```

`tests/fixtures.rs` round-trips the fixtures; `tests/library.rs` checks
each behaviour; `tests/state.rs` runs random action sequences as
`LibraryStateTests` does (`NEGS_LIBRARY_SEEDS`, `NEGS_LIBRARY_STEPS`,
`NEGS_LIBRARY_SEED`). Tests work only in temporary folders.
