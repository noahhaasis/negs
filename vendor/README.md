# Vendored crates

## rawler 0.8.0

[rawler](https://github.com/dnglab/dnglab) (LGPL-2.1, see `rawler/LICENSE`)
from crates.io, patched in through `[patch.crates-io]` in the workspace
`Cargo.toml` and in `windows/Cargo.toml` (the Windows app's own workspace
builds `spektrafilm-io` too). Two changes:

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

Tests, benches and `data/testdata` are left out. Remove this directory and
the patches once upstream rawler takes both changes.
