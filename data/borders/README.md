# Border textures

`film.png` and `LICENSE-film-borders` are copied unchanged from
https://github.com/romnn/film-borders at commit
`ff428ce31dfc77701941d4cdbd45734dac5976a6` (MIT, copyright romnn). It is
upstream's built-in 120-film border: an opaque black frame with an irregular
inner edge and a few translucent dust specks, transparent elsewhere.

`spektrafilm-io::border` stretches it over the photograph as a nine-patch,
and the macOS preview does the same with Core Animation.

## 35mm

The 35mm border is a strip of 135 film: ISO 1007 perforations, the
stock's edge print, a DX or KeyKode barcode and frame numbers.
`spektrafilm-io::film35` draws it at the resolution of each export, so it
stays sharp on a 60 MP photograph. The edge print for each stock is a
table in that module.

`35mm/material.png` holds what a drawing can't invent: the dust, specks
and uneven base density of a real negative. Its grey channel is dust
(density over the base, 0–255). Its alpha channel is the base's slow
density variation, 128 ± 500 × density. It covers one 38 mm frame at
125 px/mm in strip coordinates, and is extracted, with the original edge
print removed, by
`cargo run --release -p spektrafilm-io --example film35_material -- SCAN.jpg data 5`
from:

> "135 film. Konica VX 10. DX encoding" by Dmitry Makeev, CC BY-SA 4.0,
> https://commons.wikimedia.org/wiki/File:135_film._Konica_VX_10._DX_encoding.jpg

`material.png`, and the strips rendered from it, are therefore licensed
CC BY-SA 4.0 (https://creativecommons.org/licenses/by-sa/4.0/), with that
attribution.

`35mm/full/<film>.png` (36 × 24 mm frames, for 3:2 photographs) and
`35mm/half/<film>.png` (18 × 24 mm half-frames, for 3:4 photographs) are
the same strips at 100 px/mm, without grain, for the macOS preview. Films
without an edge print use `generic.png`. Regenerate them, and
`35mm/frames.json` (the gate as fractions `[x, y, width, height]` of the
strip), after changing the renderer or the material:

    cargo run --release -p spektrafilm-io --example film35_borders

The edge print uses Barlow Condensed (SIL OFL, in
`crates/spektrafilm-io/fonts`).
