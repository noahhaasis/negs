#!/usr/bin/env bash
# Build the browser engine into web/pkg, and with --threads also into
# web/pkg-threads: rayon on Web Workers, which needs std rebuilt with atomics
# (nightly -Zbuild-std; RUSTC_BOOTSTRAP lets the stable toolchain do it, with
# the rust-src component installed). Needs wasm-bindgen-cli at the version in
# Cargo.lock.
set -euo pipefail
cd "$(dirname "$0")/.."
# The repository's toolchain when cargo isn't on PATH (target/linux-toolchain;
# in a worktree, link it from the main checkout).
source linux/core-env.sh
for target in wasm32-unknown-unknown; do
  rustup target list --installed | grep -qx "$target" || rustup target add "$target"
done
if [[ ${1-} == --threads ]] && ! rustup component list --installed | grep -q '^rust-src'; then
  rustup component add rust-src
fi
want=$(grep -A1 '^name = "wasm-bindgen"$' Cargo.lock | sed -n 's/^version = "\(.*\)"/\1/p')
if [[ "$(wasm-bindgen --version 2>/dev/null)" != "wasm-bindgen $want" ]]; then
  echo "Needs wasm-bindgen $want: cargo install wasm-bindgen-cli --version $want --locked" >&2
  exit 1
fi
cargo build --release --target wasm32-unknown-unknown -p spektrafilm-wasm
wasm-bindgen --target web --out-dir web/pkg target/wasm32-unknown-unknown/release/spektrafilm_wasm.wasm
if [[ ${1-} == --threads ]]; then
  # RUSTFLAGS replaces .cargo/config.toml's flags, so repeat getrandom's.
  # Shared, imported memory and the TLS exports are what wasm-bindgen's
  # threads need; a 4 GiB ceiling, as the default 1 GiB holds little more
  # than a 12 MP render.
  link='--shared-memory --import-memory --max-memory=4294967296 --export=__wasm_init_tls --export=__tls_size --export=__tls_align --export=__tls_base'
  RUSTC_BOOTSTRAP=1 RUSTFLAGS="--cfg getrandom_backend=\"wasm_js\" -C target-feature=+atomics,+bulk-memory,+mutable-globals $(printf -- '-C link-arg=%s ' $link)" \
    cargo build --release --target wasm32-unknown-unknown -p spektrafilm-wasm --features threads \
    -Z build-std=panic_abort,std --target-dir target/wasm-threads
  # --out-name index: the rayon worker helper imports the package by its
  # directory, which web/serve.mjs resolves to index.js.
  wasm-bindgen --target web --out-name index --out-dir web/pkg-threads target/wasm-threads/wasm32-unknown-unknown/release/spektrafilm_wasm.wasm
fi
