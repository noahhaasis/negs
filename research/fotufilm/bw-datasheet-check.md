# Fotufilm B&W curves against the manufacturer datasheets

Checked 2 November 2026 for the B&W port (`tools/fotufilm_port/port_stocks.py`, `BW`).
Fotufilm @ `96623ba` stores each monochrome stock's reference development as the stock's own
curves and does not record its time. Its development conditions carry their own times. Each
reference curve was drawn over the characteristic-curve graph of the datasheet named in its
`development.source`. That gave the reference time and showed whether the curve is a faithful
trace. The exposure axis is relative on the Ilford graphs, so each film gets one log-exposure
shift for the whole family (fitted on the reference time).

| Film | Datasheet used | Reference time | Other times on the graph | Agreement |
|---|---|---|---|---|
| HP5 Plus | ILFORD HP5 Plus fact sheet, April 2010, p5: ILFOTEC HC 1+31, 6½ min, 20 °C | 6.5 min | none | max 0.016 D (128 traced points) |
| FP4 Plus | ILFORD FP4 Plus fact sheet, July 2010, p4: ILFOTEC HC 1+31, 8 min, 20 °C | 8 min | none | max 0.009 D (129 points) |
| Delta 3200 | ILFORD Delta 3200 fact sheet, German edition, October 2002, p7: DD-X 1+4, 7/9/12/16 min | 9 min | 7, 12, 16 | all four curves lie on the printed lines (visual overlay) |
| Acros 100 II | Fujifilm AF3-0259E (120 edition), p3: D-76, 4/7/10 min, small tank | 7 min | 4, 10 | within about 0.06 D, the thickness of the printed lines (Fuji plots near-zero base; compared above base) |
| T-MAX 100 | Kodak F-4016, p8: D-76 small tank, 6/7.5/10 min | 6 min | 7.5 | max 0.02 D |
| Tri-X 400 (reference only) | Negs' own `kodak_trix400_d76_8min`, built from Kodak F-4017 | 8 min | | max 0.006 D after a 0.125 log-exposure shift |

Development tables confirm the times. HP5 Plus in ILFOTEC HC 1+31 is 6½ / 9½ / 14 min at
EI 400 / 800 / 1600. FP4 Plus is 6 / 8 / 9 min at EI 50 / 125 / 200. Ilford prints only the
normal-time curve for these two films. Fotufilm marks HP5's 9.5 and 14 min and FP4's 6 and 9 min
as `estimated`: its process-level rule applied to the film's own curve. They ship as estimates,
recorded in each profile's `metadata.estimated_development_times`.

Fotufilm's provenance text says FP4 Plus and T-MAX 100 hold "rendering parameters only", not
digitised curves. Their curves nonetheless match the datasheets above, so neither is marked
approximate.

Where each datasheet came from (all public):
- HP5 Plus: https://mail.kennethleegallery.com/pdf/HP5Plus.pdf (Ilford's own download is behind a
  browser check)
- FP4 Plus: https://mail.kennethleegallery.com/pdf/FP4Plus.pdf
- Delta 3200: https://www.macodirect.de/media/pdf/fa/d0/8f/ID3201_Datenblatt_d.pdf
- Acros 100 II: https://www.freestylephoto.com/static/pdf/product_pdf/fujifilm/NEOPAN_100_ACROSII_(120)_data_sheet.pdf
- T-MAX 100: https://imaging.kodakalaris.com/sites/default/files/files/products/f4016_tmax_100.pdf

Method: pages rendered at 200 dpi with `pdftoppm`. Axes were calibrated from the detected grid
lines and their printed labels. Curves were traced from dark pixels where the expected number of
curves crosses a pixel column, or read off by hand (T-MAX). Fotufilm's curve was then evaluated
with the converter's own Hermite reader (`convert.record_curve`) and overlaid. For Delta 3200 the
automatic trace mixed up curves under the graph's caption box, so the overlay image is the
evidence. The overlays are not kept in the repo because the graphs belong to the manufacturers.
