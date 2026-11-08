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

A photograph you've edited shows as developed: cropped, turned and inside its
border, as in the edit view. Its thumbnail is made from the preview on screen
when you leave it for another photograph or the grid, so it costs no extra
render. Other edited photographs, such as after a paste or with edits from
another Mac, are developed in the background one at a time, and Negs waits
while you edit. A pasted Match camera brightness is solved then too, as
opening the photograph would. Until a thumbnail is ready the camera's preview
shows. Thumbnails are kept in `~/Library/Caches` (up to about 500 MB, the
least recently used go first) and are safe to delete.

## Edits per photograph

Opening a photograph shows its own edits. One you have never edited starts from
the defaults, with the film, paper, Image Size and output colour of the
photograph before it.

| Command | Shortcut | Does |
| --- | --- | --- |
| Copy Edits | ⇧⌘C | Copies the photograph's edits |
| Copy Edits… | ⌥⇧⌘C | The same, after choosing which; the choice is remembered |
| Paste Edits | ⇧⌘V | Applies them to the selected photographs |
| Paste from Previous | ⌥⇧⌘V | Applies the edits of the last photograph you left edited |
| Duplicate | ⌘D | A copy, `IMG_0412.CR3 (2)`, that starts with the photograph's rating, flag and edits and then keeps its own |
| Reset Edits | ⇧⌘R | Back to how an unedited photograph opens; rating and flag stay |

These are in the Photo menu and in the context menu of the film strip and the
grid. A plain Copy Edits takes the film and paper, Prep, Lab, the border, Image
Size and output colour; not the crop and rotation, nor lens correction and
input interpretation, which belong to each file. Match camera brightness is
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
exports as opening it would show it: the defaults, with the open photograph's
film, paper, image size and output colour. If that includes *Match camera
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
| Show the grid | G or Esc (not while cropping) |
| Edit the current photograph | E, Return, or double-click it |
| Larger / smaller thumbnails | ⌘+ / ⌘−, or the Size slider |
| Move | Arrow keys |

The grid opens on the photograph you were editing. Moving around it never
develops a RAW; only leaving it opens the current photograph.

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
