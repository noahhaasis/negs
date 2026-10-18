# Handoff: find real 35mm scans for the border textures

## Goal

Find real scans of 35mm film online that we may legally ship as border
textures, or at least use as reference. The current textures are clean
generated drawings. The user wants the 35mm border to look like a real scan:
perforations glowing from the backlight, dust, uneven rebate density, real
edge print and, later, light leaks.

This is a research task. Report what you find and download candidates to a
scratch folder. Don't commit third-party images or change code without the
user's approval.

## Context

Branch `film-35mm-border` (worktree `.claude/worktrees/film-35mm-border`)
adds a **35mm** border style:

- `data/borders/35mm/full/<film>.png` holds 36 × 24 mm frames for 3:2 photos.
  `data/borders/35mm/half/<film>.png` holds 18 × 24 mm half-frames for 3:4
  photos. Each format folder also has a `generic.png`.
- `data/borders/35mm/frames.json` gives the photo window for each format.
- The PNGs are rendered by `crates/spektrafilm-io/examples/film35_borders.rs`.
  Its `EDGE_PRINT` table maps each film profile to its edge-print text.
- `data/borders/README.md` defines the texture contract. Read it first. In
  short:
  - the strip runs horizontally, with perforations along the top and bottom;
  - the photo window is transparent and centred;
  - everything else is opaque;
  - `frames.json` gives the window as fractions `[x, y, width, height]`;
  - any resolution works.
- `spektrafilm-io::border` (export) and `macos/Sources/Spektrafilm/Border.swift`
  (preview) composite the textures. Upgrading the look should only mean
  replacing PNGs. Don't change that code.

The film profiles a border can show are the 25 `data/profiles/*.json` files
whose `info.support` is `"film"`. They include:
- Kodak Portra 160/400/800, Ektar 100, Gold 200 and UltraMax 400;
- Fuji C200, Pro 400H and X-Tra 400;
- the slide films Ektachrome E100, Kodachrome 64, Provia 100F and Velvia 100;
- Vision3 50D/250D/200T/500T, Verita 200D, Double-X 5222 and Tri-X 7266;
- the print films 2302/2383/2393.

## Licensing is the hard constraint

The project is GPL-3.0, and `data/` ships inside the app.

- **Shippable:**
  - CC0 or public domain;
  - CC BY or CC BY-SA 4.0, with attribution (the stock profiles in `data/`
    are already CC BY-SA 4.0);
  - MIT/BSD-style licenses on image assets;
  - explicit written permission from the photographer.
- **Reference only** (may be studied and linked, never shipped):
  - anything without a clear license, such as Reddit, forums, Flickr "All
    rights reserved" and most blogs;
  - Unsplash and Pexels, whose licenses restrict redistributing photos as-is
    or bundling them into a competing product. Flag them, don't ship them.
  - NC (non-commercial) and ND (no-derivatives) Creative Commons licenses.
    ND forbids the cropping and masking we need.
- **Already ruled out:**
  - `GfChou/film-border-studio`, whose `ASSET_LICENSE.md` forbids reuse of
    its frame images (fine to look at for reference);
  - Adobe Stock, Getty, Uppbeat, Gumroad packs and VSCO;
  - `romnn/film-borders`, which has no 35mm asset;
  - Wikimedia's `Photographic_Film_135.svg`, which is only a diagram.

Record the exact license and author for every candidate. When a page
contradicts itself about the license, treat the image as reference only.

## What to look for

Look for scans that show the **whole frame plus the full rebate**: both rows
of perforations and the edge print. This is often called a "full-frame
scan", a "filed carrier" scan or "sprockets showing". The most useful are:

1. **High resolution**: at least about 4000 px along the strip, inverted to a
   positive, with sharp perforations. Even one good generic strip helps,
   because the generator can put stock-specific text on top of a scanned base.
2. **Specific stocks**, especially Portra 400, Gold 200, UltraMax 400, Ektar
   100, Velvia, Ektachrome E100, Vision3 500T and Double-X.
3. **Half-frame** strips (18 × 24 mm, four perforations per frame).
4. **Blank rebate or leader**: an unexposed frame, or the rebate with little
   picture content. These are the easiest to turn into a texture.
5. **Light-leak and dust references** for simulating them later. Note what
   they look like (colour, falloff, where they appear on the strip).

Places to search:
- Wikimedia Commons (search "35mm film strip", "film negative scan",
  "sprocket holes", "Kodak Portra negative", "135 film");
- Openverse (openverse.org, with a license filter);
- Flickr advanced search filtered to "Commercial use & mods allowed";
- Internet Archive;
- museum and library open-access collections;
- GitHub repos of film-emulation and scanning tools (check the asset
  licenses, not just the code licenses);
- scanner and carrier makers' sample galleries (usually reference only).

## Second job: check the edge-print table

The `EDGE_PRINT` table in `film35_borders.rs` is partly guessed. Only Gold
200 "GB 200" and UltraMax "GC 400" come from a source, the Kodak edge-code
table at https://havecamerawilltravel.com/kodak-film-edgeprint-codes/.

For each stock, find a photo of the real edge print and record:
- the exact text;
- the emulsion or letter code;
- what else appears (frame numbers, arrows, DX barcode, KeyKode on cine
  stocks).

Reference-only sources are fine for this, because we copy facts, not images.
Note the colour the edge print appears in after inversion.

## Deliverables

1. **`docs/35mm-scan-sources.md`**, a table of candidates. Columns: URL,
   stock, format (full or half), resolution, license, author/attribution
   line, shippable (yes, no or ask), and what it shows (perforations, edge
   print, DX code, dust, light leak). Shippable candidates go first.
2. **Corrections to `EDGE_PRINT`**, as a list of stock → text with a source
   for each.
3. **Downloads** of the best shippable candidates (at most about 10) into
   the session scratchpad. Each download goes in its own new directory,
   outside the repo. Don't put them in `data/`.
4. **Recommendation**: whether the scans are good enough to replace the
   generated textures, to serve as a scanned base under generated text, or
   only as reference for a more realistic generator (procedural dust,
   perforation glow, light leaks). Say which and why.

Stop and ask the user before you:
- commit any third-party image;
- email or contact a photographer for permission;
- pay for anything.

## How a scan would become a texture (for the recommendation)

- Crop to one frame plus half the gap to each neighbouring frame, along the
  strip. Keep the full 35 mm film width across it.
- Rotate so the strip runs horizontally.
- Make the picture area transparent. A soft or irregular edge is fine, and
  photos up to 1% off the frame ratio still hide under the rebate.
- Keep the window centred and update `frames.json` to match.
- For one texture per stock, either use a scan of that stock or paint out the
  scan's edge print and let the generator add the stock text.
