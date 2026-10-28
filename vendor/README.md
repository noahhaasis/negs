# Vendored crates

## rawler 0.8.0

[rawler](https://github.com/dnglab/dnglab) (LGPL-2.1, see `rawler/LICENSE`)
from crates.io, patched in through `[patch.crates-io]` in the workspace
`Cargo.toml`. The only change: `std::time::Instant` is `web_time::Instant` in
`imgop/sensor/bayer/ppg.rs`, `imgop/sensor/xtrans/{bilinear,markesteijn}.rs`,
`decompressors/crx/decoder.rs` and `dng/writer.rs`, plus the `web-time`
dependency. `std::time::Instant::now()` panics on `wasm32-unknown-unknown`,
which killed the browser engine on every Bayer or X-Trans RAW. `web_time` is
`std::time` natively, so native builds are unchanged.

Tests, benches and `data/testdata` are left out. Remove this directory and
the patch once upstream rawler takes the change.
