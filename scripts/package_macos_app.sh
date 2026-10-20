#!/usr/bin/env bash
set -euo pipefail

ICON_SOURCE="${ICON_SOURCE:-assets/negs-icon.png}"
ICON_ICNS="${ICON_ICNS:-}"
BUILD_TARGET_DIR="${BUILD_TARGET_DIR:-target/macos-app}"
DIST_DIR="${DIST_DIR:-dist/negs-macos}"
APP_NAME="${APP_NAME:-Negs}"
APP_BUNDLE="$DIST_DIR/$APP_NAME.app"
PYTHON="${PYTHON:-python3}"
RELEASE_DIR="${RELEASE_DIR:-dist/releases}"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# Validate metadata before doing expensive build work. Public release settings
# come from CI; local packages intentionally have no live update configuration.
"$PYTHON" scripts/macos_release.py >/dev/null
release_name="$("$PYTHON" scripts/macos_release.py --field name)"
if [[ "$(uname -m)" != arm64 ]]; then
    echo "The initial macOS package requires an Apple Silicon build host." >&2
    exit 1
fi

# Keep the binary deployment target consistent with Info.plist.
export MACOSX_DEPLOYMENT_TARGET=14.0

cargo build --locked --release -p spektrafilm-native --target-dir "$BUILD_TARGET_DIR"
swift build --package-path macos --scratch-path "$BUILD_TARGET_DIR/swift-native" -c release --force-resolved-versions
swift_bin_dir="$(swift build --package-path macos --scratch-path "$BUILD_TARGET_DIR/swift-native" -c release --force-resolved-versions --show-bin-path)"
cargo build --locked --release -p spektrafilm-cli --bin spektrafilm --target-dir "$BUILD_TARGET_DIR"
cargo build --locked --release -p spektrafilm-cli --bin spektrafilm-f64 --features precision-f64 --target-dir "$BUILD_TARGET_DIR"

rm -rf "$DIST_DIR"
mkdir -p "$APP_BUNDLE/Contents/MacOS" "$APP_BUNDLE/Contents/Resources" "$APP_BUNDLE/Contents/Frameworks" "$RELEASE_DIR"
# Preserve framework symlinks, executable bits and Sparkle’s nested signatures.
ditto "$swift_bin_dir/Sparkle.framework" "$APP_BUNDLE/Contents/Frameworks/Sparkle.framework"

cp "$swift_bin_dir/NegsApp" "$APP_BUNDLE/Contents/MacOS/NegsApp"
cp "$BUILD_TARGET_DIR/release/spektrafilm-native" "$APP_BUNDLE/Contents/MacOS/spektrafilm-native"
cp "$BUILD_TARGET_DIR/release/spektrafilm" "$APP_BUNDLE/Contents/MacOS/spektrafilm"
cp "$BUILD_TARGET_DIR/release/spektrafilm-f64" "$APP_BUNDLE/Contents/MacOS/spektrafilm-f64"
cp -R data "$APP_BUNDLE/Contents/Resources/data"
cp LICENSE "$APP_BUNDLE/Contents/Resources/LICENSE.txt"
chmod +x "$APP_BUNDLE/Contents/MacOS/NegsApp" \
    "$APP_BUNDLE/Contents/MacOS/spektrafilm-native" \
    "$APP_BUNDLE/Contents/MacOS/spektrafilm" \
    "$APP_BUNDLE/Contents/MacOS/spektrafilm-f64"

"$PYTHON" scripts/macos_release.py --plist "$APP_BUNDLE/Contents/Info.plist" > "$DIST_DIR/release.json"

if [[ -n "$ICON_ICNS" ]]; then
    cp "$ICON_ICNS" "$APP_BUNDLE/Contents/Resources/negs.icns"
elif [[ -f "$ICON_SOURCE" ]]; then
    iconset="$BUILD_TARGET_DIR/negs.iconset"
    rm -rf "$iconset"
    mkdir -p "$iconset"
    sips -s format png -z 16 16 "$ICON_SOURCE" --out "$iconset/icon_16x16.png" >/dev/null
    sips -s format png -z 32 32 "$ICON_SOURCE" --out "$iconset/icon_16x16@2x.png" >/dev/null
    sips -s format png -z 32 32 "$ICON_SOURCE" --out "$iconset/icon_32x32.png" >/dev/null
    sips -s format png -z 64 64 "$ICON_SOURCE" --out "$iconset/icon_32x32@2x.png" >/dev/null
    sips -s format png -z 128 128 "$ICON_SOURCE" --out "$iconset/icon_128x128.png" >/dev/null
    sips -s format png -z 256 256 "$ICON_SOURCE" --out "$iconset/icon_128x128@2x.png" >/dev/null
    sips -s format png -z 256 256 "$ICON_SOURCE" --out "$iconset/icon_256x256.png" >/dev/null
    sips -s format png -z 512 512 "$ICON_SOURCE" --out "$iconset/icon_256x256@2x.png" >/dev/null
    sips -s format png -z 512 512 "$ICON_SOURCE" --out "$iconset/icon_512x512.png" >/dev/null
    sips -s format png -z 1024 1024 "$ICON_SOURCE" --out "$iconset/icon_512x512@2x.png" >/dev/null
    iconutil -c icns "$iconset" -o "$APP_BUNDLE/Contents/Resources/negs.icns"
fi

cat > "$DIST_DIR/README.txt" <<'README'
Negs for macOS

Open the disk image, drag Negs.app to Applications, and double-click it.
For ZIP downloads, unzip the app and move it to Applications.
No Rust, Homebrew, or separate data download is needed.
The native SwiftUI interface requires macOS 14 or newer; see BUILD-INFO.txt for its CPU architecture.

Native interface:
- Standard macOS menus, toolbar, adjustment inspector, and open/save dialogs.
- Pinch to zoom, scroll or drag to pan, double-click to fit.
- Cmd-O opens, Cmd-S saves the preview, Shift-Cmd-E exports at f64 precision.
- Cmd-0 fits, Cmd-1 shows actual size, Cmd-R rotates, Option-Cmd-I toggles adjustments.

Backends:
- macOS uses the WGSL/wgpu backend through Metal by default.
- CUDA is not available on modern macOS builds.

f64 export:
- spektrafilm-f64 is bundled inside Negs.app/Contents/MacOS.
- The GUI uses it for CPU f64 Export.

Black and white stocks:
- Film: Kodak Tri-X 400 / 400TX, D-76, 8 min, 35 mm (camera negative, experimental).
- Tri-X 400 pairs automatically with the experimental neutral Ilford
  Multigrade RC Deluxe grade-2 paper. This is a fixed-grade rendering for
  neutral B&W negatives; paper tint and stock-specific grain are not measured.
- Film: Kodak Double-X 5222 or Kodak Tri-X 7266 reversal.
- For Double-X prints, select Kodak 2302 in the paper/print selector.
- Tri-X 7266 uses direct film scan. Development times are available for Double-X/2302.
- The experimental gamut preset is available to the CLI as
  Contents/Resources/data/presets/experimental-gamut.json inside the app.

Source:
- The matching source archive is available beside the download.

Updates:
- Negs > Check for Updates checks manually.
- Settings lets you turn automatic checks off or on.
- Your photograph and adjustments are restored after a normal quit or update.
- Undo history and work after a crash are not restored.

Gatekeeper:
- This package is ad-hoc signed. It is not Apple-notarized.
- If macOS blocks it after download, dismiss the alert, open System Settings >
  Privacy & Security, and click Open Anyway for Negs. Confirm Open.
  You only need to do this on the first launch.
README

{
    date -u '+Built: %Y-%m-%dT%H:%M:%SZ'
    uname -m
    rustc -vV
    git rev-parse HEAD
    git status --short
    "$PYTHON" scripts/macos_release.py
} > "$DIST_DIR/BUILD-INFO.txt"

# Snapshot the actual working source, including uncommitted port changes.
# Exclude build products and local files unrelated to reproducing the app.
COPYFILE_DISABLE=1 tar --exclude='.DS_Store' --exclude='__pycache__' --exclude='.build' --exclude='.swiftpm' \
    -czf "$DIST_DIR/SOURCE.tar.gz" Cargo.toml Cargo.lock LICENSE README.md \
    crates macos data assets scripts docs .github release

# Ad-hoc sign our own executables and the outer app. Do not recursively
# overwrite Sparkle's supplied signatures. No Apple account is required.
for binary in NegsApp spektrafilm-native spektrafilm spektrafilm-f64; do
    codesign --force --sign - "$APP_BUNDLE/Contents/MacOS/$binary"
done
codesign --force --sign - "$APP_BUNDLE"
codesign --verify --deep --strict --verbose=2 "$APP_BUNDLE"
plutil -lint "$APP_BUNDLE/Contents/Info.plist"

# Only the app goes in the update archive. Corresponding source is a separate
# release asset so it doesn't double every updater download.
zip_path="$RELEASE_DIR/$release_name.zip"
ditto -c -k --sequesterRsrc --keepParent "$APP_BUNDLE" "$zip_path"
cp "$DIST_DIR/SOURCE.tar.gz" "$RELEASE_DIR/$release_name-source.tar.gz"
cp "$DIST_DIR/README.txt" "$RELEASE_DIR/INSTALL.txt"
cp "$DIST_DIR/BUILD-INFO.txt" "$RELEASE_DIR/BUILD-INFO.txt"

# A native drag-to-Applications installer, with no system-wide components.
stage="$(mktemp -d "$BUILD_TARGET_DIR/dmg.XXXXXX")"
trap 'rm -rf "$stage"' EXIT
ditto "$APP_BUNDLE" "$stage/$APP_NAME.app"
ln -s /Applications "$stage/Applications"
cp "$DIST_DIR/README.txt" "$stage/Installation.txt"
hdiutil create -volname "Negs" -srcfolder "$stage" -format UDZO -ov "$RELEASE_DIR/$release_name.dmg"
hdiutil verify "$RELEASE_DIR/$release_name.dmg"
"$PYTHON" scripts/verify_macos_package.py --zip "$zip_path" \
    --source "$RELEASE_DIR/$release_name-source.tar.gz" \
    --output-dir "$(mktemp -d "$BUILD_TARGET_DIR/verify.XXXXXX")/unpacked"
(
    cd "$RELEASE_DIR"
    shasum -a 256 "$release_name.zip" "$release_name.dmg" "$release_name-source.tar.gz" > SHA256SUMS
)
echo "Packaged: $repo_root/$RELEASE_DIR/$release_name.dmg"
