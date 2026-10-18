# Border textures

`film.png` and `LICENSE-film-borders` are copied unchanged from
https://github.com/romnn/film-borders at commit
`ff428ce31dfc77701941d4cdbd45734dac5976a6` (MIT, copyright romnn). It is
upstream's built-in 120-film border: an opaque black frame with an irregular
inner edge and a few translucent dust specks, transparent elsewhere.

`spektrafilm-io::border` stretches it over the photograph as a nine-patch,
and the macOS preview does the same with Core Animation.

## 35mm

`35mm/full/<film>.png` (36 × 24 mm frames, for 3:2 photographs) and
`35mm/half/<film>.png` (18 × 24 mm half-frames, for 3:4 photographs) are a
35mm negative strip for each film profile, with the stock's edge print.
`generic.png` in each folder, without a stock name, stands in for films that
have no texture of their own.

They are rendered by
`cargo run --release -p spektrafilm-io --example film35_borders`, which draws
them to 135 film dimensions with Barlow Condensed (SIL OFL, in
`crates/spektrafilm-io/examples/fonts`). The edge print for each stock is a
table in that example.

Any texture can be replaced, for example by a real scan, as long as:

- the strip runs horizontally (perforations along the top and bottom);
- the photograph's window is transparent and centred, and everything the
  photograph shouldn't show through is opaque. The window can have a soft or
  irregular edge: photographs up to 1% off the frame's ratio are scaled to
  cover the window and the rest hides under the rebate;
- `35mm/frames.json` gives the window as fractions of the texture,
  `[x, y, width, height]`, shared by every texture of that format.

Any resolution works. The exporter scales the texture to the photograph, and
the macOS preview downsamples it to 4096 pixels.
