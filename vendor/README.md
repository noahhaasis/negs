# Vendored crates

## rawler 0.8.0

[rawler](https://github.com/dnglab/dnglab) (LGPL-2.1, see `rawler/LICENSE`)
from crates.io, patched in through `[patch.crates-io]` in the workspace
`Cargo.toml` and in `windows/Cargo.toml` (the Windows app's own workspace
builds `spektrafilm-io` too). Six changes:

- `std::time::Instant` is `web_time::Instant` in
  `imgop/sensor/bayer/ppg.rs`, `imgop/sensor/xtrans/{bilinear,markesteijn}.rs`,
  `decompressors/crx/decoder.rs` and `dng/writer.rs`, plus the `web-time`
  dependency. `std::time::Instant::now()` panics on `wasm32-unknown-unknown`,
  which killed the browser engine on every Bayer or X-Trans RAW. `web_time` is
  `std::time` natively.
- `RawDevelop::develop_intermediate_owned` (`imgop/develop.rs`) and
  `crop_in_place` (`pixarray.rs`) develop and crop without a second copy of
  the sensor data, for the browser's memory limit on phones.
  `spektrafilm-io` calls `develop_intermediate_owned`, so it no longer builds
  against crates.io rawler. The pixels are bit-identical.
- `RawDevelop` demosaics X-Trans with rawler's own Markesteijn 1-pass
  (`XTransMarkesteijnDemosaic`, `imgop/develop.rs`) instead of its bilinear
  X-Trans demosaic, which averages every same-colour sample in a 5×5 window
  and made Fuji RAWs look blurred by a Gaussian of about 1 to 1.5 px. 1-pass
  matches LibRaw's Markesteijn 3-pass within about 1% at every spatial
  frequency (X-T1, X-E2), and takes about 0.5 s instead of 0.14 s for a
  16 MP frame on an M1 Pro. Bayer and every other sensor are unchanged.
  The row and column probe in `build_hex_lut`
  (`imgop/sensor/xtrans/markesteijn.rs`) adds 48, a whole CFA repeat, so a
  -1 no longer overflows `CFA::color_at` in debug builds; release builds
  wrapped to the same cell, so the pixels are bit-identical.
- `RawDevelop::xtrans` (`XTransDemosaic`, `imgop/develop.rs`) chooses the
  X-Trans demosaic: Markesteijn by default, or the bilinear one, which holds
  about 12 bytes a pixel less at its peak (Markesteijn's cropped copy of the
  mosaic and its green bounds). Only the web's reduced copy of an X-Trans sensor
  over 30 MP (the 40 MP ones) on phones asks for bilinear (`spektrafilm-wasm`,
  `REDUCED_XTRANS_BILINEAR_ABOVE`), which an iPhone otherwise doesn't survive.

- The NEF decoder (`decoders/nef.rs`) allows a dummy decode (metadata only)
  of Nikon HE/HE★ files, which it can't decompress, so `spektrafilm-io` can
  develop LibRaw's sensor data with rawler's metadata for the camera. A real
  decode of those files still fails as before.

- The DNG decoder (`decoders/dng.rs`) keeps the file's AnalogBalance in the
  decoded image's `dng_tags`. iPhone 18 ProRAW stores values already scaled
  by it (AsShotNeutral 1, 1, 1), and `spektrafilm-io`'s Daylight, Tungsten
  and Custom white balance need it to work out the camera's neutral (issue
  #31). As shot is unchanged.

Tests, benches and `data/testdata` are left out. Remove this directory and
the patches once upstream rawler takes these changes.

## LibRaw 0.23.0-Snapshot202611

[LibRaw](https://www.libraw.org) by LibRaw LLC (LGPL-2.1 or CDDL-1.0, see
`libraw/LICENSE.LGPL`, `libraw/LICENSE.CDDL` and `libraw/COPYRIGHT`), from
[GitHub](https://github.com/LibRaw/LibRaw) at master `d270dbf` (2026-11-18):
the 202611 snapshot, which added Nikon HE/HE★ decoding, and its fixes since.
`crates/negs-libraw` compiles it with the `cc` crate for the native engine,
never for the browser. Negs uses it only for RAW files rawler can't decode,
and only to unpack the sensor data (see `crates/spektrafilm-io/src/libraw.rs`).

Kept: `src`, `libraw`, `internal`, the licences, `COPYRIGHT`, `README.md` and
`Changelog.txt`, unchanged. Left out: the samples, docs, build files, the
RawSpeed, DNG SDK and GoPro integrations, and `src/*/*_ph.cpp` (placeholders
the upstream build doesn't compile either). The build skips the RawSpeed, X3F
and LCMS sources, which are empty without their optional libraries, and
defines none of LibRaw's optional features (OpenMP, zlib, JPEG, LCMS).

To update: download the new snapshot, replace the kept directories and files,
and check `crates/negs-libraw/src/shim.cpp` against `libraw/libraw.h` (it
overrides the Nikon HE tile loops, `nikonhe_decode_loop1`–`3`).
