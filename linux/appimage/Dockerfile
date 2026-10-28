# AppImage compatibility baseline: Ubuntu 24.04 LTS. Binaries built here need
# at most its glibc, and take GLib, Pango, cairo, Wayland, X11 and graphics
# drivers from the host. See docs/linux-distribution.md.
FROM ubuntu:24.04@sha256:24737cef17f4e22b1977df7d9a05e77c4e28b7da5b8911f8d8ccda136bf3573c
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential pkg-config meson ninja-build git curl ca-certificates xz-utils \
        python3 patchelf file desktop-file-utils squashfs-tools sassc gettext cmake nasm \
        libglib2.0-dev libglib2.0-bin libpango1.0-dev libcairo2-dev libharfbuzz-dev \
        libfribidi-dev libgraphene-1.0-dev libepoxy-dev libwayland-dev wayland-protocols \
        libxkbcommon-dev libx11-dev libxrandr-dev libxi-dev libxext-dev libxcursor-dev \
        libxdamage-dev libxfixes-dev libxinerama-dev libgdk-pixbuf-2.0-dev libpng-dev \
        libtiff-dev libdrm-dev libxrender-dev adwaita-icon-theme \
    && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH
RUN curl -sSfLo /tmp/rustup-init https://static.rust-lang.org/rustup/archive/1.29.1/x86_64-unknown-linux-gnu/rustup-init \
    && echo 'dda7234360b7f578ca8b0ddcb80145646fa61a67c1720a5abc7051b35c9fcb71  /tmp/rustup-init' | sha256sum -c \
    && chmod +x /tmp/rustup-init && /tmp/rustup-init -y --no-modify-path --profile minimal --default-toolchain 1.93.1 \
    && rm /tmp/rustup-init && rustc -vV
