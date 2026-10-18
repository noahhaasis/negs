# 35mm scan sources for the border textures

Research for `docs/handoff-35mm-scan-textures.md`, done 2026-10-18. Nothing
here has been committed to `data/`. The downloads live in the session
scratchpad only.

## Summary

- **One scan is good enough to build textures from.** Dmitry Makeev's
  *135 film. Konica VX 10. DX encoding* on Wikimedia Commons is a blank,
  unexposed frame of Konica VX100. It is about 126 px/mm, so one cropped
  frame is about 4800 × 4400 px. It is CC BY-SA 4.0. It has sharp KS
  perforations, real dust and hairs, the full DX barcode and real edge
  print.
- **No shippable scan was found for Portra, Ektar, UltraMax, Velvia,
  Vision3 or Double-X.** The best stock-specific scans are Kodak Gold 200
  and 400, Kodak Elite Chrome (EBX), Fuji Provia 100F (RDP III), Fuji
  Superia 200 and Ilford Delta 400. Only the two Gold 400 scans are high
  resolution.
- **No real half-frame strip was found.** A half-frame uses the same 135
  rebate, so a half-frame texture can be cropped from a full-frame scan to
  4 perforations.
- **Recommendation: use a scanned base under generated text.** See the end
  of this document.

## Licensing notes

- CC BY-SA 4.0 is one-way compatible with GPL-3.0 (Creative Commons, 2015),
  and `data/` already ships CC BY-SA 4.0 profiles. A texture made from a
  CC BY-SA scan must itself be CC BY-SA, with an attribution line in
  `data/borders/README.md`.
- CC BY-SA 3.0 (unported) lets adaptations be licensed under a later
  version, so a cropped and masked texture can ship as CC BY-SA 4.0.
- Every Commons license below was read from the file's own metadata
  (`extmetadata`), not from search snippets.

## Candidates

Resolution is px/mm across the film, and the approximate size of one cropped
frame (38 mm along the strip × 35 mm across).

### Shippable

| # | URL | Stock | Format | Resolution | License | Attribution | Shows |
|---|---|---|---|---|---|---|---|
| 1 | [135 film. Konica VX 10. DX encoding.jpg](https://commons.wikimedia.org/wiki/File:135_film._Konica_VX_10._DX_encoding.jpg) | Konica VX100 (colour neg) | full | 10888×4480; ~126 px/mm, ~4800 px per frame | CC BY-SA 4.0 | "135 film. Konica VX 10. DX encoding" by Dmitry Makeev, CC BY-SA 4.0, via Wikimedia Commons | **Blank unexposed frame**, raw orange-mask negative. Both perforation rows, backlit perforations at pure white, edge print `KONICA VX100`, frame numbers, DX barcode, red/green density patches, dust, a hair. A partial exposed frame at the right end. The title says "VX 10" but the description says VX 100. |
| 2 | [135 film. Konica VX 100.jpg](https://commons.wikimedia.org/wiki/File:135_film._Konica_VX_100.jpg) | Konica VX100 | full | 10929×4526; ~126 px/mm | CC BY-SA 4.0 | "135 film. Konica VX 100" by Dmitry Makeev, CC BY-SA 4.0, via Wikimedia Commons | The same strip as #1, inverted to a positive: dark navy rebate, amber edge print, **black** perforations (see the notes below). |
| 3 | [Film - Kodak Gold 400-6.jpg](https://commons.wikimedia.org/wiki/File:Film_-_Kodak_Gold_400-6.jpg) | Kodak Gold 400 (gen. 6) | full | 9772×4458; ~126 px/mm | CC BY-SA 4.0 | "Film - Kodak Gold 400-6" by Dmitry Makeev, CC BY-SA 4.0, via Wikimedia Commons | Raw negative, two exposed frames (a light bulb). Edge print `GOLD 400-6` and `400-6 KODAK` alternating with frame numbers on top. Barcode and `→28A` arrows at the bottom. Clean rebate. |
| 4 | [Film. Kodak Gold 400-6.jpg](https://commons.wikimedia.org/wiki/File:Film._Kodak_Gold_400-6.jpg) | Kodak Gold 400 | full | 9856×4502; ~126 px/mm | CC BY-SA 4.0 | "Film. Kodak Gold 400-6" by Dmitry Makeev, CC BY-SA 4.0, via Wikimedia Commons | The same strip as #3, inverted. Black rebate, orange edge print, black perforations. |
| 5 | [Kodak 5096 Gold 200-2 b.png](https://commons.wikimedia.org/wiki/File:Kodak_5096_Gold_200-2_b.png) | Kodak Gold 200 (5096, gen. 2, c. 1992) | full | 1780×840; ~24 px/mm, ~910 px per frame | CC BY-SA 4.0 | "Kodak 5096 Gold 200-2 b" by Sheled Umlal, CC BY-SA 4.0, via Wikimedia Commons | Blank frame, raw negative. Edge print `GOLD 200-2`, `KODAK 5096`, frame numbers, barcode, scanner-mask notches. Low resolution. |
| 6 | [Kodak Gold 200-4.png](https://commons.wikimedia.org/wiki/File:Kodak_Gold_200-4.png) | Kodak Gold 200 (gen. 4) | full | 1780×840 | CC BY-SA 4.0 | "Kodak Gold 200-4" by Sheled Umlal, CC BY-SA 4.0, via Wikimedia Commons | Blank frame. Edge print `KODAK 200-4` and `GOLD 200-4`. The same uploader has Gold 100, Gold 400 (5097) and Fuji Super HG 100 versions. |
| 7 | [FarbDia.jpg](https://commons.wikimedia.org/wiki/File:FarbDia.jpg) | Kodak Elite Chrome Extra Color 100 (EBX), slide | full | 3088×1264; ~34 px/mm, ~1300 px per frame | CC BY-SA 3.0 | "FarbDia" by Florian Schott, CC BY-SA 3.0, via Wikimedia Commons | Slide strip with three frames. Edge print `KODAK EBX`, frame numbers, `→29A`. Black slide rebate with yellow edge print. |
| 8 | [Kodakcamsrip1.jpg](https://commons.wikimedia.org/wiki/File:Kodakcamsrip1.jpg) | Fuji Provia 100F (RDP III), slide | stereo (5-perf) | 2889×815; ~23 px/mm | CC BY-SA 4.0 | "Kodakcamsrip1" by John Alan Elson, CC BY-SA 4.0, via Wikimedia Commons | Edge print `FUJI RDPIII` and `RDPIII-043`, frame numbers, `E`, `264PFS`. Stereo-camera frames, so use it only for the rebate and the edge-print facts. |
| 9 | [Farbnegativ.jpg](https://commons.wikimedia.org/wiki/File:Farbnegativ.jpg) | Fuji Superia 200 | full | 2791×1304; ~37 px/mm | CC BY-SA 3.0 | "Farbnegativ" by Florian Schott, CC BY-SA 3.0, via Wikimedia Commons | Raw negative. Edge print `S-200`, `509`, frame numbers, DX barcode, Fuji edge stripe. |
| 10 | [SWNegativ.jpg](https://commons.wikimedia.org/wiki/File:SWNegativ.jpg) | Ilford Delta 400 Pro (B&W) | full | 2916×1200; ~34 px/mm | CC BY-SA 3.0 | "SWNegativ" by Florian Schott, CC BY-SA 3.0, via Wikimedia Commons | B&W negative with `ILFORD` and `DELTA 400 PRO`, frame numbers and barcode. A B&W rebate reference for Double-X, Tri-X and 2302. |
| 11 | [Color print film strip 01 - negative.jpg](https://commons.wikimedia.org/wiki/File:Color_print_film_strip_01_-_negative.jpg) | Kodak Ektar 100 | full | 5845×3520 photo | CC BY 4.0 | "Color print film strip 01 - negative" by LoMit, CC BY 4.0, via Wikimedia Commons | A photo of a strip held up to daylight, with a hand and perspective, so it can't be a texture. It confirms Ektar's edge print `KODAK EKTAR 100`, frame numbers and `22A`. |
| 12 | [Perforations photographic film 02.jpg](https://commons.wikimedia.org/wiki/File:Perforations_photographic_film_02.jpg) | Tasma (Soviet B&W) | full | 2795×3000 | CC BY-SA 4.0 | "Perforations photographic film 02" by Dmitry Makeev, CC BY-SA 4.0, via Wikimedia Commons | A B&W positive with `TACMA` edge print and frame numbers. A fair B&W reference. |
| 13 | [Light leak 1.jpg](https://commons.wikimedia.org/wiki/File:Light_leak_1.jpg), [Light leak 2.jpg](https://commons.wikimedia.org/wiki/File:Light_leak_2.jpg) | unknown colour neg | frame only | 3560×2405 | CC BY-SA 4.0 | "Light leak 1" by Sgroey, CC BY-SA 4.0, via Wikimedia Commons | **Light leaks**, frame only with no rebate (see the notes below). |
| 14 | [Pentax K1000 Kodak Vision3 0121 32490.jpg](https://commons.wikimedia.org/wiki/File:Pentax_K1000_Kodak_Vision3_0121_32490.jpg), [… 000000410001.jpg](https://commons.wikimedia.org/wiki/File:Pentax_K1000_Kodak_Vision3_000000410001.jpg) | Kodak Vision3 | frame only | 3024×2005 | CC BY-SA 4.0 | "Pentax K1000 Kodak Vision3 0121 32490" by OyirdniIjupRutac, CC BY-SA 4.0, via Wikimedia Commons | **End-of-roll fog and edge burn**, frame only with no rebate. |
| 15 | [Sprocket Rocket (27975049409).jpg](https://commons.wikimedia.org/wiki/File:Sprocket_Rocket_(27975049409).jpg) | unknown | panoramic, exposed over the perforations | 2137×979 | CC BY 2.0 | "Sprocket Rocket" by Kevin Dooley, CC BY 2.0, via Wikimedia Commons | Perforation silhouettes from a camera that exposes over the sprockets. A different look, not a scan of the rebate. |

### Ask or reference only

| URL | Why not shippable | Use |
|---|---|---|
| [Light leak.jpg](https://commons.wikimedia.org/wiki/File:Light_leak.jpg) | Tagged CC BY-SA 3.0 by uploader "dudeman", but the source is a DeviantArt page by "grainsnap" ("Free Real Film Light Leak"), so the uploader may not be the author. Treat as reference only. | A magenta-red leak entering from the left edge with a soft falloff. |
| [Half-Frame 4442.jpg](https://commons.wikimedia.org/wiki/File:Half-Frame_4442.jpg) (Ashley Pomeroy, CC BY-SA 4.0) | Shippable license, but it is a photo of strips at an angle, not a scan. | A half-frame layout reference. |
| [AGFA Vista 200 35 mm film strip.jpg](https://commons.wikimedia.org/wiki/File:AGFA_Vista_200_35_mm_film_strip.jpg) (Santeri Viinamäki, CC BY-SA 4.0) | Photographed at an angle on a light box. | Unexposed base colour. |
| `GfChou/film-border-studio` | `ASSET_LICENSE.md` forbids reuse. | Already ruled out. |
| Flickr, Reddit, blogs | No clear license. | Not searched in depth (see the gaps below). |

The Konica and Gold *img* scans by the same author (e.g. *Konica VX 100
film. img 02*, *Babies of Russia…*) also have full rebates at the same
resolution. They show identifiable people inside the frame. The window would
make the frame transparent, but avoid them anyway; #1–#4 are enough.

### Gaps

- **Openverse** (CC filter) returned almost nothing relevant: Flickr
  thumbnails of 1024 px or less, often off topic, plus repeated 504 errors.
- **Flickr's "Commercial use & mods allowed" search** needs the web UI or an
  API key, so I didn't run it. These are worth a manual pass:
  - https://www.flickr.com/search/?text=portra%20400%20sprockets&license=4%2C5%2C9%2C10
  - https://www.flickr.com/search/?text=film%20rebate%20scan&license=4%2C5%2C9%2C10
  - https://www.flickr.com/search/?text=half%20frame%20negative%20scan&license=4%2C5%2C9%2C10
- **No CC Portra, Ektar, UltraMax, Velvia, Vision3 or Double-X rebate scan
  exists on Commons.** Commons has only cartridges, boxes and cropped frames
  for those stocks.

## Downloads

All are in their own folders under
`/private/tmp/claude-501/-Users-noahhaasis-Projects-spektrafilm-rs--claude-worktrees-film-35mm-border/6faa75dc-c320-4716-ae9f-8f46489e8ec9/scratchpad/downloads/`,
each with an `ATTRIBUTION.txt`:

1. `01-135_film__Konica_VX_10__DX_encoding/` (#1, 38 MB)
2. `02-135_film__Konica_VX_100/` (#2)
3. `03-Film_-_Kodak_Gold_400-6/` (#3)
4. `04-Film__Kodak_Gold_400-6/` (#4)
5. `05-Kodak_5096_Gold_200-2_b/` (#5)
6. `06-FarbDia/` (#7)
7. `07-Kodakcamsrip1/` (#8)
8. `08-SWNegativ/` (#10)
9. `09-Light_leak_1/` (#13)
10. `10-Pentax_K1000_Kodak_Vision3_0121_32490/` (#14)

## What the real scans show

- **Perforations.** In a raw scan the perforations are clipped to pure white
  by the backlight. They have a crisp edge with a thin dark outline where
  the base is thick. **In an inverted scan (#2, #4) they turn black**,
  because inverting flips the backlight too. To get perforations that glow
  like the current textures, invert the film and keep the perforations
  white. In the raw scan the perforations are the only pure-white pixels,
  so they are easy to mask.
- **Rebate.** It is not uniform. There are faint density bands along the
  strip, and Fuji has coloured edge stripes. After inversion an unexposed
  colour-negative rebate is near black with a slight blue-green cast (#2,
  #4). A slide rebate is plain black. A B&W negative rebate becomes black
  in the positive.
- **Edge print colour after inversion.** Colour negatives give amber to
  orange text (#2, #4); this matches the generator's ink `[242,164,60]`.
  Kodak slide film gives yellow-orange text (#7), and so does Fuji slide
  film (#8). B&W gives white or light grey text.
- **Edge-print layout on real Kodak C-41.** The top edge alternates frame
  numbers with the stock text, about twice per frame (`28 GOLD 400-6 … 29
  400-6 KODAK`). The bottom edge has the DX barcode, frame numbers and
  `→28A` half-frame arrows. Fuji puts the emulsion number and production
  code on the top edge too (`123 002DJ0 00 PRO400H 00A`). The generator
  draws the stock text once, with a single frame number at the bottom.
  Repeating the frame numbers on the top edge would make it more convincing.
- **Dust.** In the raw scan (#1) dust is small dark specks plus a fine
  hair. In the positive it becomes **white specks and white hairs** on a
  dark rebate, which is the look to simulate.
- **Light leaks** (#13, #14, and the reference-only `Light leak.jpg`). They
  are warm: red, then orange, then yellow, then near white as intensity
  rises. They enter from one short edge or a corner of the frame, as a
  vertical band with a soft gradient a few millimetres wide; #13 has
  flare-like streaks. End-of-roll fog (#14) blows out a third of the frame
  to white with an orange-red transition edge. On a strip, a leak would
  also fog the rebate and the edge print next to it.

## Edge-print corrections

Applied on 2026-10-18 in `STOCKS` in `crates/spektrafilm-io/src/film35.rs`,
which replaced `EDGE_PRINT`. Tri-X 7266 was dropped and now gets the generic
strip.

| Profile | Current | Proposed | Source and notes |
|---|---|---|---|
| `fujifilm_c200` | `FUJICOLOR C200` | `FUJI 200` (generation `CA24`) | Fujifilm C200 datasheet §13, *Processed film edge markings*: type designation `200`, generation number `CA24`, one green solid line and two red broken lines ([PDF](https://asset.fujifilm.com/master/emea/files/2020-10/98c3d5087c253f51c132a5d46059f131/films_c200_datasheet_01.pdf)). The figure shows `C74 A921HFI 00 200 00A 24 0 200 0A CA24 1`. `FUJICOLOR C200` never appears on the film. |
| `fujifilm_pro_400h` | `FUJI PRO 400H` | `PRO400H` (+ `FUJI`) | Fujifilm Pro 400H datasheet §13 figure: `123 002DJ0 00 PRO400H 00A 36 0 PRO400H 0A 1 PRO400H 1A FUJI 2` ([PDF](https://asset.fujifilm.com/master/emea/files/2020-10/a6cb96275e4957ddc7b3ca932b7755e5/films_pro-400h_datasheet_01.pdf)). |
| `fujifilm_xtra_400` | `FUJICOLOR X-TRA 400` | `FUJI S-400` (generation `CH23`) | Fujifilm Superia X-TRA 400 datasheet §14-1: type designation `S-400`, generation `CH23`, one red solid line and one green broken line on both sides ([PDF](https://asset.fujifilm.com/master/emea/files/2020-10/9a958fdcc6bd1442a06f71e134b811f6/films_superia-xtra400_datasheet_01.pdf)). The figure shows `FUJI 2 S-400 2A H74 3`. |
| `fujifilm_provia_100f` | `FUJI RDP III` | `FUJI RDPIII` | Scan #8 shows `FUJI RDPⅢ` and `RDPⅢ-043`. The current entry is right apart from the space; the real print uses a Roman-numeral glyph. |
| `fujifilm_velvia_100` | `FUJI RVP 100` | `FUJI RVP100` (unverified) | `RVP 100` is Fuji's code (Wikipedia *Velvia*, retailer SKUs `RVP100-36`). No edge photo or datasheet figure found. Low confidence on spacing. |
| `kodak_portra_160` | `KODAK PORTRA 160` | keep | Kodak Professional spec table: "Edge Print KODAK PORTRA 160" ([page](https://kodakprofessional.com/photographers/film/color/kodak-professional-portra-160-film/529)). |
| `kodak_portra_400` | `KODAK PORTRA 400` | keep | Kodak spec table: "Edge Print KODAK PORTRA 400" ([page](https://kodakprofessional.com/photographers/film/color/kodak-professional-portra-400-film/516)). An Analog.cafe review says the film has been renamed *Ektacolor Pro 400* and that the edge print changed; not checked. |
| `kodak_portra_800*` | `KODAK PORTRA 800` | keep | Kodak spec table: "Edge print KODAK PORTRA 800" ([page](https://kodakprofessional.com/photographers/film/color/kodak-professional-portra-800-film/528)). |
| `kodak_ektar_100` | `KODAK EKTAR 100` | keep | Kodak spec table ([page](https://kodakprofessional.com/photographers/film/color/kodak-professional-ektar-100-film/530)), and scan #11. |
| `kodak_ektachrome_100` | `KODAK E100` | keep | Kodak spec table: "Edge Print KODAK E100" ([page](https://kodakprofessional.com/photographers/film/color/kodak-professional-ektachrome-film-e100/531)). |
| `kodak_gold_200` | `KODAK GB 200` | keep; optionally add a generation suffix (`GB 200-x`) | Have Camera Will Travel letter-code table: Gold 200 = `GB`. Older rolls print `GOLD 200-2` / `KODAK 5096` (#5) and `GOLD 200-4` (#6), so Kodak adds a generation suffix. The suffix for current stock is not confirmed. Kodak's Gold page is for 120 and says `KODAK 200`. |
| `kodak_ultramax_400` | `KODAK GC 400` | `KODAK GC 400-9` | Have Camera Will Travel table: UltraMax 400 = `GC / GC 400-8 / GC 400-9`. Read through a reader proxy because the site blocks direct fetches. |
| `kodak_kodachrome_64` | `KODACHROME 64` | `KODAK KR` (low confidence) | Have Camera Will Travel: Kodachrome 64 letter code `KR` (Professional: `PKR`). No 35mm edge photo found. Older Kodachrome may read `KODAK SAFETY FILM` instead. |
| `kodak_vision3_50d` | `KODAK 5203` | `5203` + KeyKode `ER` | Kodak *KEYKODE Numbers for 35 mm Camera Films* diagram ([PDF](https://www.kodak.com/content/products-brochures/Film/post-production-35mm-keycode-diagram.pdf)): current film codes 5203-ER, 5207-EN, 5213-EO, 5219-EJ, 5222-KE. The human-readable line is the product code, emulsion, roll/part, strip, printer and year (e.g. `5219 021 2301 23 174 2026`). The key number is e.g. `EJ 23 1234 5677+00`, once per foot (16 frames), with a USS-128 barcode, all outside the perforations on one edge. The word `KODAK` isn't part of the current format. |
| `kodak_vision3_250d` | `KODAK 5207` | `5207` + `EN` | As above. |
| `kodak_vision3_200t` | `KODAK 5213` | `5213` + `EO` | As above. |
| `kodak_vision3_500t` | `KODAK 5219` | `5219` + `EJ` | As above, plus Evertz FilmID (EJ = 5219). |
| `kodak_verita_200d` | `KODAK VERITA 200D` | `5206` (KeyKode letters unknown) | Verita 200D is 5206 (35mm) / 7206 (16mm), released April 2026 (Kodak investor release; ASC). As a cine camera stock it will carry KeyKode, not its name. |
| `kodak_doublex` | `EASTMAN 5222` | `5222` + `KE` | Double-X datasheet H-1-5222: "the product code numbers 5222 … KEYKODE numbers, and manufacturer/film identification code (KE) are visible" ([PDF](https://www.kodak.com/content/products-brochures/Film/DOUBLEX-Technical-Data-EN.pdf)). |
| `kodak_trix` | `KODAK 7266` | `7266`, but see the note | Tri-X Reversal 7266 is only made in **16 mm and Super 8**; no 35mm version exists. Any 35mm border for it is fictional. |
| `kodak_2383` | `KODAK 2383` | `2383 KODAK` | 2383 datasheet, *Identification*: "After processing, '2383 KODAK' is visible along the length of the film, along with strip number and date codes" ([PDF](https://www.kodak.com/content/products-brochures/Film/KODAK-VISION-Color-Print-Film-2383-3383-data-sheet.pdf)). Print films use KS-1870 (positive) perforations. |
| `kodak_2302` | `KODAK 2302` | `2302 KODAK` (by analogy) | The 2302 datasheet only says there is an "edgecode print". The wording is assumed from 2383. |
| `kodak_2393` | `KODAK 2393` | `2393 KODAK` (by analogy) | Vision Premier 2393 is no longer on Kodak's print-film list. Same assumption as 2302. |

Cine camera stocks (Vision3, Verita, Double-X) use **BH** perforations,
which are flatter with curved sides, not the KS perforations the generator
draws. Re-spooled still rolls keep the BH perforations. Print films use KS.
This only matters if those textures should be accurate.

## Recommendation: scanned base under generated text

**Use scan #1 (Konica VX100 blank frame) as the base for every colour
negative texture, and let the generator add the stock text on top.** Don't
replace the textures wholesale, and don't limit the scans to reference
material.

Why:

- Scan #1 has everything the generated drawing lacks: real perforation
  edges, uneven rebate density, dust, a hair and LED-dot edge print. At
  ~126 px/mm it is above the 4000 px target and the preview's 4096 px cap.
  It is blank, so the photo window loses nothing. Its CC BY-SA 4.0 license
  is shippable.
- No shippable scan exists for most of the 25 stocks, so one texture per
  stock can't come from real scans. Gold 400 (#3/#4) is the only Kodak
  stock at texture resolution, and it isn't one of our profiles.
- A texture doesn't need to match its stock's base. The generator already
  tints the base and ink per profile, and a scanned base can be tinted the
  same way.

How, following the handoff's recipe:

1. Crop #1 (raw negative) to one frame plus half the gap on each side:
   38 mm, about 4800 px. Crop the full 35 mm across, trimming the white
   scanner margin at the film edges. Use the clean left part of the strip
   and avoid the exposed frame at the right end.
2. Mask the perforations as the pure-white pixels. Invert the remaining
   film with the orange mask removed, so the rebate goes near black. Keep
   the perforations as warm white.
3. Paint out `KONICA VX100` and the frame numbers. Keep the DX barcode and
   the density patches as texture, or paint them out too and let the
   generator draw its own.
4. Make the window transparent, as now. Keep `frames.json` in sync.
5. Have the generator composite its stock text, frame numbers and arrows
   onto this base, using the corrected `EDGE_PRINT` table.
6. Derive the half-frame texture from the same base cropped to 4
   perforations.
7. For slide stocks, use a black base. #7 shows the colour, but it is only
   ~34 px/mm, so generate the slide base procedurally from #1's dust and
   perforation shapes. For B&W stocks, use #10 for the colour only.
8. Add `data/borders/README.md` attribution: *"35mm rebate texture derived
   from '135 film. Konica VX 10. DX encoding' by Dmitry Makeev, CC BY-SA
   4.0, via Wikimedia Commons."* The derived PNGs are then CC BY-SA 4.0.

Later, light leaks should be procedural: a warm gradient from one short edge
(see the notes above), with #13 and #14 as references.

## Needs your decision

- Whether to commit any third-party image (#1 above all). Not done.
- Whether to apply the `EDGE_PRINT` changes. Not done.
- Whether to keep a Tri-X 7266 35mm border at all, since the stock only
  exists in 16 mm and Super 8.
- No photographer has been contacted and nothing has been paid for.
