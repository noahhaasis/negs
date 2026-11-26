#!/usr/bin/env bash
# Builds the engine helper (crates/spektrafilm-native) for arm64 Android and
# puts it where Gradle packages native libraries, named as one
# (lib*.so): Android only lets an app run programs from its native library
# directory. The app starts it as a separate process (EngineProcess.kt).
#
#   android/build-engine.sh            release, as Claude sessions build it
#   ANDROID_NDK_HOME=… android/build-engine.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
sdk="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
ndk="${ANDROID_NDK_HOME:-$(ls -d "$sdk"/ndk/* 2>/dev/null | sort -V | tail -1)}"
[ -d "$ndk" ] || { echo "No Android NDK under $sdk/ndk (sdkmanager \"ndk;28.2.13676358\")" >&2; exit 1; }
api=29
host="$(ls "$ndk/toolchains/llvm/prebuilt" | head -1)"
bin="$ndk/toolchains/llvm/prebuilt/$host/bin"
target=aarch64-linux-android

export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$bin/$target$api-clang"
export CC_aarch64_linux_android="$bin/$target$api-clang"
export CXX_aarch64_linux_android="$bin/$target$api-clang++"
export AR_aarch64_linux_android="$bin/llvm-ar"
# LibRaw is C++: link libc++ into the helper rather than shipping
# libc++_shared.so beside it.
export CXXSTDLIB_aarch64_linux_android=c++_static
# libc++_static.a leaves the C++ ABI (exceptions, new/delete) to libc++abi.a.
export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="-C link-arg=-lc++abi"

cd "$root"
cargo build --release --target "$target" -p spektrafilm-native "$@"

out="$root/android/app/src/main/jniLibs/arm64-v8a"
mkdir -p "$out"
cp "$root/target/$target/release/spektrafilm-native" "$out/libspektrafilm_native.so"
"$bin/llvm-strip" "$out/libspektrafilm_native.so"
ls -l "$out/libspektrafilm_native.so"
