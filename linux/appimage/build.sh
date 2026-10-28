#!/usr/bin/env bash
# Runs inside the linux/appimage/Dockerfile image (see package.sh).
#   build.sh SOURCE_ARCHIVE OUTPUT_DIRECTORY
# Builds the pinned GTK stack and OpenBLAS under a private prefix (cached in
# /cache), then Negs, assembles the AppDir and writes the AppImage plus the
# corresponding source for the bundled LGPL libraries.
set -euo pipefail
source_archive=$(readlink -f "$1")
out=$(readlink -f "$2")
epoch=${SOURCE_DATE_EPOCH:?Set SOURCE_DATE_EPOCH to the source commit time}
cache=${NEGS_APPIMAGE_CACHE:-/cache}
prefix=/opt/negs-gtk
jobs=$(nproc)

GTK_VERSION=4.16.13
GTK_SHA256=ddf3d9e12b848139a945d191d5ca56b78d0647f53b55b8bca5f9902b61624498
ADW_VERSION=1.6.10
ADW_SHA256=714a486e748e1b7de29101436fba3bbb61f74997f373c6af9651e23e859eb1f6
# Newer than Ubuntu's 2.1: a superset of the libjpeg.so.8 ABI that host
# libraries such as Arch's libtiff expect once the bundled copy is loaded.
JPEG_VERSION=3.2.0
JPEG_SHA256=6f30092cef9fb839779646608f4ee14ae3cbac989c47fa05e841b0841f09878e
OPENBLAS_VERSION=0.3.34
OPENBLAS_SHA256=cd7e129868320cc2d033afa920e31202dfe0b8066a5b66661900ccc0f197dfed
RUNTIME_VERSION=20251108
RUNTIME_SHA256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
# Same OpenBLAS configuration as the Flatpak manifest.
OPENBLAS_FLAGS=(DYNAMIC_ARCH=1 TARGET=GENERIC NUM_THREADS=128 USE_OPENMP=0 USE_THREAD=1 NO_AFFINITY=1 ONLY_CBLAS=1 NO_STATIC=1)

mkdir -p "$cache/src" "$out"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
tar -xzf "$source_archive" -C "$work"
src=$(echo "$work"/negs-*)
fetch() { # url sha256
    local file="$cache/src/$(basename "$1")"
    if ! echo "$2  $file" | sha256sum -c --quiet >/dev/null 2>&1; then
        curl -sSfL --retry 3 -o "$file.part" "$1"
        echo "$2  $file.part" | sha256sum -c --quiet >&2
        mv "$file.part" "$file"
    fi
    printf '%s\n' "$file"
}
gtk_tar=$(fetch "https://download.gnome.org/sources/gtk/${GTK_VERSION%.*}/gtk-$GTK_VERSION.tar.xz" "$GTK_SHA256")
adw_tar=$(fetch "https://download.gnome.org/sources/libadwaita/${ADW_VERSION%.*}/libadwaita-$ADW_VERSION.tar.xz" "$ADW_SHA256")
jpeg_tar=$(fetch "https://github.com/libjpeg-turbo/libjpeg-turbo/releases/download/$JPEG_VERSION/libjpeg-turbo-$JPEG_VERSION.tar.gz" "$JPEG_SHA256")
blas_tar=$(fetch "https://github.com/OpenMathLib/OpenBLAS/releases/download/v$OPENBLAS_VERSION/OpenBLAS-$OPENBLAS_VERSION.tar.gz" "$OPENBLAS_SHA256")
runtime=$(fetch "https://github.com/AppImage/type2-runtime/releases/download/$RUNTIME_VERSION/runtime-x86_64" "$RUNTIME_SHA256")

# The private prefix is cached per input set; /opt is rebuilt from it.
key=$(printf '%s\n' "$GTK_SHA256" "$ADW_SHA256" "$JPEG_SHA256" "$OPENBLAS_SHA256" "${OPENBLAS_FLAGS[*]}" \
      | cat - "$src/linux/appimage/libadwaita-optional-appstream.patch" "$src/linux/appimage/Dockerfile" \
      | sha256sum | cut -c1-16)
stack="$cache/stack-$key"
if [ ! -f "$stack/.complete" ]; then
    rm -rf "$stack" "$cache/build"
    mkdir -p "$cache/build" "$stack"
    tar -xJf "$gtk_tar" -C "$cache/build"
    tar -xJf "$adw_tar" -C "$cache/build"
    tar -xzf "$blas_tar" -C "$cache/build"
    tar -xzf "$jpeg_tar" -C "$cache/build"
    cmake -S "$cache/build/libjpeg-turbo-$JPEG_VERSION" -B "$cache/build/jpeg" -G Ninja \
        -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$prefix" -DCMAKE_INSTALL_LIBDIR=lib \
        -DWITH_JPEG8=1 -DENABLE_STATIC=0 -DWITH_TURBOJPEG=0
    cmake --build "$cache/build/jpeg"
    DESTDIR="$stack" cmake --install "$cache/build/jpeg"
    rm -rf "$prefix" && mkdir -p "$(dirname "$prefix")" && cp -a "$stack$prefix" "$prefix"
    # Media, printing, Vulkan and introspection are unused; fewer host links.
    PKG_CONFIG_PATH="$prefix/lib/pkgconfig" meson setup "$cache/build/gtk" "$cache/build/gtk-$GTK_VERSION" --prefix="$prefix" --libdir=lib \
        --buildtype=release -Dx11-backend=true -Dwayland-backend=true -Dbroadway-backend=false \
        -Dmedia-gstreamer=disabled -Dprint-cpdb=disabled -Dprint-cups=disabled -Dvulkan=disabled \
        -Dcloudproviders=disabled -Dsysprof=disabled -Dtracker=disabled -Dcolord=disabled \
        -Dintrospection=disabled -Ddocumentation=false -Dman-pages=false -Dbuild-demos=false \
        -Dbuild-testsuite=false -Dbuild-examples=false -Dbuild-tests=false
    meson install -C "$cache/build/gtk" --destdir "$stack"
    rm -rf "$prefix" && mkdir -p "$(dirname "$prefix")" && cp -a "$stack$prefix" "$prefix"
    patch -d "$cache/build/libadwaita-$ADW_VERSION" -p1 < "$src/linux/appimage/libadwaita-optional-appstream.patch"
    PKG_CONFIG_PATH="$prefix/lib/pkgconfig" meson setup "$cache/build/adw" "$cache/build/libadwaita-$ADW_VERSION" \
        --prefix="$prefix" --libdir=lib --buildtype=release -Dintrospection=disabled -Dvapi=false \
        -Dtests=false -Dexamples=false -Dgtk_doc=false
    meson install -C "$cache/build/adw" --destdir "$stack"
    make -C "$cache/build/OpenBLAS-$OPENBLAS_VERSION" -j"$jobs" MAKE_NB_JOBS="$jobs" "${OPENBLAS_FLAGS[@]}"
    make -C "$cache/build/OpenBLAS-$OPENBLAS_VERSION" "${OPENBLAS_FLAGS[@]}" PREFIX="$stack$prefix" install
    sed -i "s|$stack||g" "$stack$prefix"/lib/pkgconfig/openblas.pc
    touch "$stack/.complete"
    rm -rf "$cache/build"
fi
rm -rf "$prefix" && mkdir -p "$(dirname "$prefix")" && cp -a "$stack$prefix" "$prefix"
export PKG_CONFIG_PATH="$prefix/lib/pkgconfig"

# Negs from the exact release source archive.
version=$(python3 -c 'import sys, tomllib; print(tomllib.load(open(sys.argv[1], "rb"))["package"]["version"])' "$src/linux/Cargo.toml")
export CARGO_HOME="$cache/cargo" NEGS_PACKAGED=1 NEGS_ENGINE_TARGET_DIR="$cache/target" NEGS_UI_TARGET_DIR="$cache/target/linux-ui"
bash "$src/linux/build.sh"

appdir="$work/AppDir"
python3 "$src/linux/stage.py" --format appimage --destdir "$appdir" --prefix /usr \
    --engine-target "$cache/target" --ui-target "$cache/target/linux-ui" --source-archive "$source_archive"
python3 "$src/linux/appimage/bundle.py" "$appdir" --search "$prefix/lib" --ledger "$out/library-ledger.json"
# GTK's GSettings schemas (the file chooser aborts without them) and the
# Adwaita symbolic icons Negs's editor uses, for hosts without them.
mkdir -p "$appdir/usr/share/glib-2.0/schemas" "$appdir/usr/share/icons/Adwaita"
cp "$prefix"/share/glib-2.0/schemas/*.gschema.xml "$appdir/usr/share/glib-2.0/schemas/"
glib-compile-schemas "$appdir/usr/share/glib-2.0/schemas"
cp -a /usr/share/icons/Adwaita/index.theme /usr/share/icons/Adwaita/symbolic "$appdir/usr/share/icons/Adwaita/"
licenses="$appdir/usr/share/licenses/negs/bundled"
mkdir -p "$licenses"
cp /usr/share/doc/adwaita-icon-theme/copyright "$licenses/adwaita-icon-theme.copyright"
tar -xzOf "$jpeg_tar" "libjpeg-turbo-$JPEG_VERSION/LICENSE.md" > "$licenses/libjpeg-turbo-LICENSE.md"
tar -xzOf "$jpeg_tar" "libjpeg-turbo-$JPEG_VERSION/README.ijg" > "$licenses/libjpeg-turbo-README.ijg"
for package in libgraphene-1.0-0 libepoxy0; do
    cp "/usr/share/doc/$package/copyright" "$licenses/$package.copyright"
done
tar -xJOf "$gtk_tar" "gtk-$GTK_VERSION/COPYING" > "$licenses/gtk-COPYING"
tar -xJOf "$adw_tar" "libadwaita-$ADW_VERSION/COPYING" > "$licenses/libadwaita-COPYING"
tar -xzOf "$blas_tar" "OpenBLAS-$OPENBLAS_VERSION/LICENSE" > "$licenses/OpenBLAS-LICENSE"
cat > "$licenses/README" <<EOF
This AppImage bundles GTK $GTK_VERSION and libadwaita $ADW_VERSION (LGPL-2.1-or-later),
OpenBLAS $OPENBLAS_VERSION (BSD-3-Clause), libjpeg-turbo $JPEG_VERSION (IJG and
BSD-3-Clause), and graphene, libepoxy and the cairo script interpreter from Ubuntu 24.04. libadwaita carries libadwaita-optional-appstream.patch. The
complete corresponding source for the LGPL libraries, with build instructions,
is published beside the AppImage as negs-$version-appimage-sources.tar.
EOF
cp "$src/linux/appimage/AppRun" "$appdir/AppRun"
cp "$appdir/usr/share/applications/io.github.noahhaasis.negs.desktop" "$appdir/"
cp "$appdir/usr/share/icons/hicolor/256x256/apps/io.github.noahhaasis.negs.png" "$appdir/"
ln -sf io.github.noahhaasis.negs.png "$appdir/.DirIcon"

# mksquashfs takes file and filesystem times from SOURCE_DATE_EPOCH.
mksquashfs "$appdir" "$work/negs.squashfs" -root-owned -noappend -comp zstd -Xcompression-level 19 -quiet
image="$out/Negs-$version-x86_64.AppImage"
cat "$runtime" "$work/negs.squashfs" > "$image"
chmod 755 "$image"

sources="$out/negs-$version-appimage-sources.tar"
mkdir -p "$work/sources"
cp "$gtk_tar" "$adw_tar" "$jpeg_tar" "$blas_tar" "$src/linux/appimage/libadwaita-optional-appstream.patch" \
   "$src/linux/appimage/build.sh" "$src/linux/appimage/Dockerfile" "$work/sources/"
tar --sort=name --mtime="@$epoch" --owner=0 --group=0 --numeric-owner -cf "$sources" -C "$work" sources
printf 'AppImage: %s\nSources: %s\n' "$image" "$sources"
