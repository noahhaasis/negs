#!/usr/bin/env bash
set -euo pipefail

ICON_SOURCE="${ICON_SOURCE:-assets/spektrafilm-icon.jpg}"
ICON_ICNS="${ICON_ICNS:-}"
BUILD_TARGET_DIR="${BUILD_TARGET_DIR:-target/macos-app}"
DIST_DIR="${DIST_DIR:-dist/spektrafilm-macos}"
APP_NAME="${APP_NAME:-Spektrafilm}"
APP_BUNDLE="$DIST_DIR/$APP_NAME.app"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# Keep the binary deployment target consistent with Info.plist.
export MACOSX_DEPLOYMENT_TARGET=14.0

cargo build --locked --release -p spektrafilm-native --target-dir "$BUILD_TARGET_DIR"
swift build --package-path macos --scratch-path "$BUILD_TARGET_DIR/swift-native" -c release
swift_bin_dir="$(swift build --package-path macos --scratch-path "$BUILD_TARGET_DIR/swift-native" -c release --show-bin-path)"
cargo build --locked --release -p spektrafilm-cli --bin spektrafilm --target-dir "$BUILD_TARGET_DIR"
cargo build --locked --release -p spektrafilm-cli --bin spektrafilm-f64 --features precision-f64 --target-dir "$BUILD_TARGET_DIR"

rm -rf "$DIST_DIR"
mkdir -p "$APP_BUNDLE/Contents/MacOS" "$APP_BUNDLE/Contents/Resources"

cp "$swift_bin_dir/SpektrafilmApp" "$APP_BUNDLE/Contents/MacOS/SpektrafilmApp"
cp "$BUILD_TARGET_DIR/release/spektrafilm-native" "$APP_BUNDLE/Contents/MacOS/spektrafilm-native"
cp "$BUILD_TARGET_DIR/release/spektrafilm" "$APP_BUNDLE/Contents/MacOS/spektrafilm"
cp "$BUILD_TARGET_DIR/release/spektrafilm-f64" "$APP_BUNDLE/Contents/MacOS/spektrafilm-f64"
cp -R data "$APP_BUNDLE/Contents/Resources/data"
cp LICENSE "$APP_BUNDLE/Contents/Resources/LICENSE.txt"
chmod +x "$APP_BUNDLE/Contents/MacOS/SpektrafilmApp" \
    "$APP_BUNDLE/Contents/MacOS/spektrafilm-native" \
    "$APP_BUNDLE/Contents/MacOS/spektrafilm" \
    "$APP_BUNDLE/Contents/MacOS/spektrafilm-f64"

cat > "$APP_BUNDLE/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>Spektrafilm</string>
  <key>CFBundleExecutable</key>
  <string>SpektrafilmApp</string>
  <key>CFBundleIconFile</key>
  <string>spektrafilm.icns</string>
  <key>CFBundleIdentifier</key>
  <string>dev.spektrafilm.app</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>Spektrafilm</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.2</string>
  <key>CFBundleVersion</key>
  <string>0.1.2</string>
  <key>LSMinimumSystemVersion</key>
  <string>14.0</string>
  <key>CFBundleDocumentTypes</key>
  <array><dict>
    <key>CFBundleTypeName</key><string>Photograph</string>
    <key>CFBundleTypeRole</key><string>Viewer</string>
    <key>LSHandlerRank</key><string>Alternate</string>
    <key>LSItemContentTypes</key>
    <array><string>public.png</string><string>public.tiff</string><string>public.jpeg</string><string>public.camera-raw-image</string></array>
  </dict></array>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

if [[ -n "$ICON_ICNS" ]]; then
    cp "$ICON_ICNS" "$APP_BUNDLE/Contents/Resources/spektrafilm.icns"
elif [[ -f "$ICON_SOURCE" ]]; then
    iconset="$BUILD_TARGET_DIR/spektrafilm.iconset"
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
    iconutil -c icns "$iconset" -o "$APP_BUNDLE/Contents/Resources/spektrafilm.icns"
fi

cat > "$DIST_DIR/README.txt" <<'README'
spektrafilm-rs macOS

Unzip the download, drag Spektrafilm.app to Applications, and double-click it.
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
- spektrafilm-f64 is bundled inside Spektrafilm.app/Contents/MacOS.
- The GUI uses it for CPU f64 Export.

Black and white stocks:
- Film: Kodak Double-X 5222 or Kodak Tri-X 7266 reversal.
- For Double-X prints, select Kodak 2302 in the paper/print selector.
- Tri-X uses direct film scan. Development times are available for Double-X/2302.
- The experimental gamut preset is available to the CLI as
  Contents/Resources/data/presets/experimental-gamut.json inside the app.

Source:
- SOURCE.tar.gz contains the exact application source and data for this build.

Gatekeeper:
- This package is ad-hoc signed. It is not Apple-notarized.
- If macOS blocks it after download, dismiss the alert, open System Settings >
  Privacy & Security, and click Open Anyway for Spektrafilm. Confirm Open.
  You only need to do this on the first launch.
README

{
    date -u '+Built: %Y-%m-%dT%H:%M:%SZ'
    uname -m
    rustc -vV
    git rev-parse HEAD
    git status --short
} > "$DIST_DIR/BUILD-INFO.txt"

# Snapshot the actual working source, including uncommitted port changes.
# Exclude build products and local files unrelated to reproducing the app.
COPYFILE_DISABLE=1 tar --exclude='.DS_Store' --exclude='__pycache__' --exclude='.build' --exclude='.swiftpm' \
    -czf "$DIST_DIR/SOURCE.tar.gz" Cargo.toml Cargo.lock LICENSE README.md \
    crates macos data assets scripts docs .github

codesign --force --deep --sign - "$APP_BUNDLE" >/dev/null
codesign --verify --deep --strict --verbose=2 "$APP_BUNDLE"
plutil -lint "$APP_BUNDLE/Contents/Info.plist"

zip_path="$DIST_DIR.zip"
rm -f "$zip_path"
(
    cd "$DIST_DIR/.."
    ditto -c -k --sequesterRsrc --keepParent "$(basename "$DIST_DIR")" "$(basename "$zip_path")"
)

echo "Packaged: $repo_root/$DIST_DIR"
echo "Zip:      $repo_root/$zip_path"
