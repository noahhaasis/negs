# Rating, filtering and the grid (macOS)

The folder of the photograph you open is your library: there is nothing to
import. Each photograph remembers its own rating, flag and edits in a small
file beside it, `IMG_0412.CR3.negs`, written only once you rate, flag or edit
it ([format](sidecar-format.md)). Turn this off in Settings with *Save ratings
and edits next to photos*.

## Rating and flagging

Stars (0–5) and flags (pick, reject) are independent: a photograph can be a
four-star reject. The keys work as in Lightroom and Capture One, and never
while you're typing in a field.

| Action | Key | Menu |
| --- | --- | --- |
| Rate 1–5 stars | 1–5 | Photo › Rating |
| Clear the rating | 0 | Photo › Rating › None |
| Raise / lower the rating | ] / [ | Photo › Rating |
| Pick | P | Photo › Flag › Pick |
| Reject | X | Photo › Flag › Reject |
| Unflag | U | Photo › Flag › Unflagged |

In the edit view a key rates the photograph you're editing, and a brief note
over it says what changed. In the grid it rates every selected photograph.
Photo › Advance After Rating moves on to the next photograph after each key.

Thumbnails show the stars at the bottom, a flag or ✕ at the top left, and a
sliders mark at the top right once a photograph has edits. Rejects are greyed.

Every photograph shows as developed, as Lightroom shows its own renders: an
edited one with its edits (cropped, turned and inside its border, as in the edit
view), one you haven't edited as opening it would, with the film and paper of
Settings › New photographs. The open photograph's thumbnail is made from the
preview on screen when you leave it for another photograph or the grid, so it
costs no extra render. The rest of the folder is developed in the background one
at a time, those on screen first, and Negs waits while you edit. A pasted Match
camera brightness is solved then too, from the whole photograph, and stored, as
opening it would. An unedited photograph's match is solved on the small read the
thumbnail needs and stored nowhere. Thumbnails read only as much of a RAW as they
need, except to solve a match that's stored: about 0.43 s each for a 24 MP NEF
on an M1 Pro, half of it the match, and two at a time, each in a helper of its
own, on a Mac with 8 cores and 16 GB or more (see [Large folders](#large-folders)). A new default film or paper, or turning the match for new
photographs on or off, develops the unedited photographs again; edited ones stay.
Until a thumbnail is ready the camera's preview shows. Thumbnails are kept in
`~/Library/Caches` (up to 1 GB, the least recently used go first) and are safe to
delete: Settings › Empty Thumbnail Cache shows their size and removes them, and
the open folder's are developed again in the background. They're about 48 KB each (Z 6 NEFs with the default film, grain and
halation: 17–92 KB, 72 KB for nine in ten), so 1 GB holds about 21,000 and a
folder of 1,000 takes about 50 MB. Past the limit the cache is cut to three
quarters, about 16,000. After a new default film the old unedited thumbnails are
never shown again, so they're the first to go.

## Edits per photograph

Opening a photograph shows its own edits. One you have never edited starts from
the defaults, with the film and paper of Settings › New photographs (until you
choose, the defaults table's Kodak Gold 200 on Portra Endura) and the Image Size
and output colour of the photograph before it. Changing that film or paper
changes the open photograph too while it has no edits of its own, without an
undo step; edited photographs keep theirs, since a sidecar always names its film
and paper. Its Match camera brightness is solved when it opens and
written nowhere beside it; the session keeps the open photograph's, so
relaunching on it doesn't solve it again, unless the file or the settings the
match depends on changed.

| Command | Shortcut | Does |
| --- | --- | --- |
| Copy Edits | ⇧⌘C or ⌘C | Copies the photograph's edits |
| Copy Edits… | ⌥⇧⌘C | The same, after choosing which; the choice is remembered |
| Paste Edits | ⇧⌘V or ⌘V | Applies them to the selected photographs |
| Paste from Previous | ⌥⇧⌘V | Applies the edits of the last photograph you left edited |
| Duplicate | ⌘D | A copy, `IMG_0412.CR3 (2)`, that starts with the photograph's rating, flag and edits and then keeps its own |
| Reset Edits | ⇧⌘R | Back to how an unedited photograph opens; rating and flag stay |

These are in the Photo menu and in the context menu of the film strip and the
grid. ⌘C and ⌘V are Edit › Copy and Paste: in a text field they copy and paste
text, as everywhere. A plain Copy Edits takes the film and paper, Prep, Lab,
the border, Image Size and output colour; not the crop and rotation, nor lens
correction and input interpretation, which belong to each file. Match camera brightness is
solved again for each photograph it's pasted onto, when that photograph opens.

In the grid these act on every selected photograph, and so do they in the edit
view while the film strip shows; with the strip hidden, on the open photograph.
The context menu acts on the selected photographs if you click one of them,
otherwise on the one you clicked. A paste, reset or duplicate of several
photographs is one step to undo (⌘Z), even after you've moved on to another
photograph.

A duplicate copies no file: it lives in the photograph's `.negs` file, so it
shows the same photograph developed its own way.

## Deleting

| Action | Key | Menu |
| --- | --- | --- |
| Move to the Trash, after asking | ⌫ | Photo › Move to Trash… |
| Move to the Trash at once | ⌘⌫ | Photo › Move to Trash (hold ⌘) |
| Delete every rejected photograph, after asking | | Photo › Delete Rejected Photographs… |

In the grid these act on every selected photograph; in the edit view on the
open one only, as the rating keys do. The photograph's file goes to the Trash
with its `.negs` file, and so with its duplicates. A duplicate on its own is
removed from the `.negs` file, and the photograph stays. Deleting
`IMG_0412.JPG` leaves `IMG_0412.CR3` alone. The next photograph becomes
current.

Undo (⌘Z) puts everything back, files and `.negs` files from the Trash. On a
volume without a Trash, such as some network shares, Negs always asks first,
because the files are deleted for good.

## Exporting several photographs

In the grid, Export (⇧⌘E, or the toolbar's Export) acts on the selection: the
menu reads *Export 12 Photographs…*. Choose a folder, the format, engine, size,
colour space and quality as for a single export, and two more rows:

| Row | Choices |
| --- | --- |
| File Name | Name_Film (`IMG_0412_kodak_gold_200.jpeg`, as a single export suggests), Name, or your own text with `{name}` and `{film}` |
| If a File Exists | Add a Number (`IMG_0412_kodak_gold_200 2.jpeg`), Replace, Skip |

A duplicate's number follows the name: `IMG_0412 (2)_kodak_gold_200.jpeg`. Two
photographs of the batch that would get the same name, such as a RAW+JPEG pair,
are numbered too. One photograph selected exports alone: the open one exactly
as from the edit view, another through the same Save dialog.

Each photograph exports with its own edits as they were when you chose Export,
so editing meanwhile changes nothing queued, and each file is the same, byte for
byte, as exporting that photograph alone. A photograph you never edited
exports as opening it would show it: the defaults, with the default film and
paper and the open photograph's image size and output colour. If that includes *Match camera
brightness*, it's solved just before the photograph exports. Photographs are
exported one at a time, in the background, while you keep working, and the Mac
doesn't sleep while idle meanwhile.

While a batch runs, the Export button shows a ring that fills as it goes. Click
it for the progress, any failures with their reason, and Cancel, Retry Failed
and Show in Finder. A photograph that fails doesn't stop the others; a batch
that ended with failures shows a warning on the button until you've looked. A
notification says when a batch finishes while Negs is in the background.

Quitting during a batch asks first. The next time Negs opens, it offers to
export the rest.

Every export, single or batch, carries the photograph's rating as `xmp:Rating`
(1 to 5 stars, or −1 for a reject, as Lightroom writes it), which Bridge, Photo
Mechanic, digiKam and Windows Explorer read. An unrated photograph's file gets
none. Picks have no standard field. Adding it never changes the pixels.

## Filtering

The Filter button in the toolbar shows only some photographs, in the film strip
and the grid alike: by rating (at least so many stars, or unrated only), by
flag (picks, unflagged, rejected; several at once) and edited only. Its icon is
filled while a filter is on. Clear it from the same menu or with ⌥⌘L.

A photograph you rate out of the filter stays until you move on, so rating a
run of photographs never makes the selection jump.

## The grid

The Photo | Grid switch in the toolbar, or these keys, change between editing
one photograph and seeing the whole folder:

| Action | Key |
| --- | --- |
| Show the grid | G, Esc, or double-click the photograph; a zoomed-in one fits the window first (not while cropping) |
| Edit the current photograph | E, Return, or double-click it |
| Larger / smaller thumbnails | ⌘+ / ⌘−, or the Size slider |
| Move | Arrow keys |

The grid opens on the photograph you were editing. Moving around it never
develops a RAW; only leaving it opens the current photograph.

## Large folders

Measured on 2026-11-09 (ticket 30, phase 8) on an M1 Pro with a 120 Hz display,
on mains power, with the benchmarks in [native-macos-ui.md](native-macos-ui.md#validation).
The folders are APFS clones of one 24 MP NEF, 30% of them with a sidecar and
half of those edited, read with a warm file cache.

**Launch** doesn't get slower. Twelve alternating launches of each build
against main's, each reopening the photograph its own last launch left open:

| Photograph | Window and photograph, sorted runs (ms) | Developed preview, median (ms) |
| --- | --- | --- |
| Never edited, alone | main 440–482 or 674–723; library 448–478 or 684–711 | 2466 / 2472 |
| With a sidecar | main 462–482 or 665–721; library 440–484 or 670–728 | 2685 / 2704 |
| In a folder of 1,000 | main 435–476 or 673–710; library 438–471 or 696–727 | 2495 / 2496 |
| In a folder of 5,000 | main 466–478 or 674–709; library 438–480 or 671–705 | 2488 / 2499 |

Launch times fall into two groups a few hundred milliseconds apart in both
builds, so the sorted runs are compared rather than medians. The photograph is
in the window's first frame in every run.

**Stars** (every sidecar's rating in) arrive 840 ms after the process starts
with 1,000 photographs (90 ms after the folder is listed) and 1.25–1.45 s with
5,000 (about 0.5 s after the listing). Applying them is one main-thread pass of
about 45 ms with 5,000, and about 20 ms with 1,000.

**Scrolling the grid** of 5,000 (1400 × 900 window, 7 columns, three runs each;
missed frames at 120 Hz):

| Speed | Ratings only | Edited thumbnails on disk | Edited thumbnails being made |
| --- | --- | --- | --- |
| 600 pt/s | 0–0.1% | 0.2–0.4% | 0.1% |
| 1,500 pt/s | 0% | 1.4–2.1% | 0.1% |
| 4,000 pt/s | 0.4–0.7% | 8.5–10.2% | 1.8–2.5% |

No frame took longer than 26 ms (three refreshes); the main run loop's longest
pass was 42 ms. At 1,500 pt/s and faster the cells show their placeholder while
they pass and fill in within about a second of stopping. Memory goes from about
430 MB to a peak of 555–575 MB while scrolling.

### Every photograph developed

Measured on 2026-11-10 (ticket 35) on the same M1 Pro on mains power, against
main as it was before (`1aa0fb6`). The folders are APFS clones of 109 Z 6 NEFs in
turn (24 MP), with sidecars as above (799 of 5,000 edited, their match solved);
the match for new photographs is on, as it is by default. Each NEF is read 9 or
46 times, so the file cache is warmer than with a real shoot.

**Developing a folder** from an empty cache, while the grid shows:

| Folder | Time | Each | Helper (peak) | App (peak) | On disk |
| --- | --- | --- | --- | --- | --- |
| 1,000 | 9.7 min | 0.58 s | 540 MB | 480 MB | 48 MB |
| 5,000 | 55.5 min | 0.67 s | 550 MB | 470 MB | 240 MB |
| 1,000, now | 4.3 min | 0.26 s | 640 + 520 MB | 460 MB | 48 MB |

An unedited photograph was about half a second: the reduced read and a develop
220 ms, its match 280 ms (the RAW's preview colour space 50 ms, the camera's
preview 35 ms, the placement's trial renders 200 ms), its render 20 ms and
saving it 3 ms. The helper runs at utility priority and gives way to anything
else the Mac is doing, which is why the 5,000, measured with more else
running, came out slower per photograph.

Two changes since (the last row). The decode the match is solved on is no
longer developed, and the preview's colour space is read on a second thread
while the RAW decodes: on 100 photographs, alternating, 485–500 ms each before
and 425–430 after, the same solved match, the helper about 40 MB larger. Then
two helpers at once, on a Mac with at least 8 cores and 16 GB: 422–442 ms each
with one and 237–260 with two (1.7×; they share the GPU), the second helper
490–790 MB at its peak. The editor's own helper is unchanged
(1.1–1.6 GB at its peak in both). Main's background helper, reading edited
photographs whole, peaked at 1.0–1.2 GB.

**Launch** doesn't get slower: 52 alternating launches of each build with each
folder and 12 with a single photograph, window, photograph, preview and settled
times all within the run-to-run spread (each set's window median within 21 ms
of main's, either way). With two helpers, 12 more of each against main: each
folder's window, preview and settled medians within 56 ms, either way.
With every photograph to develop, the background helper waits until the open
photograph's preview is in, and the folder's scan for the next thumbnail runs
off the main thread.

**Scrolling the grid** of 5,000 while its thumbnails are being developed (three
runs each, missed frames at 120 Hz): main develops only the 799 edited ones, from
the whole RAW; every photograph now, from the reduced read.

| Speed | Main | Every photograph |
| --- | --- | --- |
| 600 pt/s | 0–0.2% | 0% |
| 1,500 pt/s | 0–0.2% | 0–0.1% |
| 4,000 pt/s | 1.5–2.1% | 0.5–0.8% |

The app's memory while scrolling is about 25 MB more (a peak of 606–608 MB
against 580–591 MB). With two helpers (three alternating runs each), 4,000 pt/s
missed 0.8–1.7% against 0.4–1.5% with one, and 600 and 1,500 pt/s 0–0.1%; the
main thread's busy passes were the same.

## Selecting

There is always one current photograph (the one the edit view shows, outlined
thickly) and any number of other selected ones (outlined thinly).

| Action | Shortcut |
| --- | --- |
| Select one, make it current | Click |
| Add or remove one | ⌘-click |
| Select a range from the last click | ⇧-click, ⇧← / ⇧→ (⇧↑ / ⇧↓ in the grid) |
| Select all shown | ⌘A |
| Select only the current photograph | ⌥⌘A |
| Rubber-band select | Drag in empty grid space (⌘ or ⇧ adds) |

In the film strip, ← and → open the previous and next photograph once you have
clicked a thumbnail. ⌘- and ⇧-clicks only select, so they never develop each
photograph on the way.
