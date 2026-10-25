#!/usr/bin/env bash
# Source from the repository root. Does not change the caller's HOME.
negs_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
if ! command -v cargo >/dev/null; then
  export RUSTUP_HOME="$negs_root/target/linux-toolchain/rustup"
  export CARGO_HOME="$negs_root/target/linux-toolchain/cargo"
  export PATH="$CARGO_HOME/bin:$PATH"
fi
# Optional development sysroot, used when system package installation needs
# privileges. Production packages declare openblas as a runtime dependency.
negs_sysroot="$negs_root/target/linux-toolchain/sysroot"
if ! pkg-config --exists openblas && [[ -f "$negs_sysroot/usr/lib/pkgconfig/openblas.pc" ]]; then
  # Relocate only this package, so GTK's system pkg-config paths stay valid.
  mkdir -p "$negs_sysroot/pkgconfig"
  sed "s|=/usr/|=$negs_sysroot/usr/|g" "$negs_sysroot/usr/lib/pkgconfig/openblas.pc" > "$negs_sysroot/pkgconfig/openblas.pc"
  export PKG_CONFIG_PATH="$negs_sysroot/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
  export LD_LIBRARY_PATH="$negs_sysroot/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi
unset negs_root negs_sysroot
