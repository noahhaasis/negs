# Starting brightness through the selected film

The Prep view remains graded. Its Exposure control changes scene light before
the film, independently of the Lab's camera-exposure/print-compensation pair.
This matters because print compensation can largely cancel a camera exposure
change when judging the finished print.

**Match camera brightness** uses the RAW's embedded camera preview as a
brightness reference. It solves one absolute Prep exposure within ±3 EV
through the selected film and paper. Repeating the action replaces that value;
it does not add exposure to the previous result. The resulting value is saved
with the edit and used by preview, Fast export and Reference export.

Fresh RAW imports can run this once automatically: Settings › Match new RAWs to
camera brightness, on by default. Restored sessions retain their saved edits. If a reference is
missing, unusable, or the photo/settings change while analysis runs, the app
keeps the existing exposure. Manual Prep controls remain available.

A preview that doesn't say its colour space is read as sRGB, on every
platform: cameras and Lightroom make sRGB previews unless set otherwise.
Adobe RGB is converted; a DNG preview declared ProPhoto or gray gamma 2.2
is refused, as reading it as sRGB would be about half a stop off.

A failed match turns the button off and says why in a line below it, as
long as trying again couldn't change the answer: for the photograph when the
preview is the problem (none, too small, framed differently, in a colour
space other than sRGB or Adobe RGB, too few midtones), until the next edit when no reliable match was
found or it would take more than ±3 EV. The engine's own reasons are mapped to
these short notes, never shown as they are. An automatic match fails the same
quiet way.

Some cameras letterbox a 4:3 preview of their 3:2 frame in black bars: the
Leica M8 (320×240, bars top and bottom), M (Typ 240) and Monochrom (Typ 246)
(160×120, one bar at the top), Sony A100, Minolta Dynax 7D and Nikon D1. Only
when the preview's shape differs from the photo's, the bars are trimmed and the
picture inside is used if it has the photo's shape. Size alone is no problem:
shrinking 14 ordinary previews from 512 to 160 pixels this way moved their
matches by 0–0.06 EV (median 0.02).

## Metering contract

The method follows Sawmill's one-shot camera-preview exposure placement:

- Compare luminance distributions in the central 80% of both full frames.
- Use nine fixed quantile ranks, selecting usable camera-preview midtones.
- Freeze the source auto-exposure while evaluating absolute Prep exposure.
- Render a small deterministic proxy through the selected film and paper.
- Compare both images in sRGB. Wide-gamut output retains its selected colour
  space in the actual preview/export; highly saturated out-of-sRGB colours can
  differ from this common-space metering proxy.
- Bracket and refine the median logarithmic luminance error; decline unreliable
  or out-of-range solutions.
- Limit positive corrections when they add more than one percentage point of
  near-white area, relative to the greater of the original and camera preview.
  Thresholds account for the selected paper's white.

This preserves the scene's own brightness reference, including low-key scenes.
It matches brightness rather than the camera's colour rendering or tone curve.
One exposure cannot make every shadow and highlight match a different film
response. The highlight limit can deliberately leave a remaining brightness
difference.

## Why the decoder was left linear

The reported OM-family sample's Apple RAW 9 and Sawmill Resolve-prepared source
luminances differed by approximately 0.05 EV. A uniform 18% gray also developed
to the expected encoded sRGB level, about 0.46, in Negs. The darker foliage
appeared after film/paper development: scene-average metering and the film toe
placed shadows low. Multiplying the RAW by a fixed camera-specific gain would
mostly be canceled by the existing automatic meter.

The correction therefore leaves the Apple 9 → Apple 8 → rawler decoding
contract intact and places exposure through the actual selected film response.

## Photographic verification

Three local OM-family RAWs, developed with Portra 400 and Portra Endura, selected
Prep exposures of +0.75, +0.80 and +0.91 EV. Their median luminance residuals
against the embedded preview improved from −1.03, −1.00 and −1.22 EV to within
0.004 EV of zero. None required highlight backoff. Analysis took 121–133 ms
after the initial source/film render on the test Mac. These are proxy-meter
measurements, not a claim that every pixel matches the camera JPEG.

Before/after renders were visually reviewed. Private photographs and extracted
previews are excluded from the repository.
