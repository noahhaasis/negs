# Linux Flatpak and AppImage

Negs for Linux ships in three forms, all from the same source and the same
payload layout ([`linux/stage.py`](../linux/stage.py)):

| Format | For | Built on | Updates |
| --- | --- | --- | --- |
| Arch package / per-user bundle | Arch and Omarchy | Arch container, system GTK | `get-negs.sh`, AUR, Omarchy |
| Flatpak | any distribution with Flatpak | GNOME 51 runtime | install the newer GitHub bundle; Flathub if accepted |
| AppImage | portable download | Ubuntu 24.04 baseline | download the newer AppImage |

The application ID is `io.github.noahhaasis.negs` in every format (it was
`dev.negs.app` before 0.11). Settings, presets and the preview cache live in
`~/.config/negs` and `~/.cache/negs` for the Arch package and the AppImage, and
in `~/.var/app/io.github.noahhaasis.negs/` for the Flatpak, which can't read
the others. Export presets from one and import them into the other to move them.

This document describes what was built and how it was tested. The
[plan](linux-distribution-plan.md) records the design and the review behind it.

## Shared payload

```
PREFIX/bin/negs                       format launcher (native and Flatpak)
PREFIX/lib/negs/{negs,spektrafilm,spektrafilm-f64,spektrafilm-native}
PREFIX/share/negs/data/               engine data
PREFIX/share/negs/payload.json        digest of executables, libraries and data
PREFIX/share/applications/io.github.noahhaasis.negs.desktop
PREFIX/share/metainfo/io.github.noahhaasis.negs.metainfo.xml
PREFIX/share/icons/hicolor/{128x128,256x256,512x512}/apps/io.github.noahhaasis.negs.png
PREFIX/share/licenses/negs/           GPL, Barlow OFL, Rust crate notices, bundled libraries
```

- Distribution builds use `NEGS_PACKAGED=1`, which compiles the GUI with the
  `packaged` feature: it never falls back to the build machine's checkout for
  helpers or data. The installed GUI also finds `../../share/negs/data` from
  `lib/negs/`, and each launcher sets `NEGS_DATA_DIR` and `NEGS_PACKAGE_ROOT`.
- `linux/build.sh` takes `NEGS_TOOLCHAIN` (empty: plain `cargo` from an SDK;
  default `+1.93.1` through rustup), `NEGS_ENGINE_TARGET_DIR` and
  `NEGS_UI_TARGET_DIR`. The helper/Fast and Reference builds stay separate.
- The preview cache keys packaged builds on `payload.json` rather than file
  paths and inodes, so an AppImage keeps cache hits although it mounts at a new
  path each launch. Any changed executable, library or data file replaces the
  digest. Vulkan ICDs are also read from `XDG_DATA_DIRS`, which covers Flatpak
  GL extensions.
- Only the native launcher installs the Omarchy opacity rule. Flatpak and
  AppImage write no desktop configuration. On Omarchy, the native rule now
  matches both the old and new IDs; Flatpak and AppImage users who want photos
  fully opaque can add the same rule from
  [`linux/omarchy/negs-window-rules.lua`](../linux/omarchy/negs-window-rules.lua).
- `linux/install.py` removes unmodified files from an older install that the
  new bundle no longer ships, such as the old desktop entry and icon.
- The About dialog shows the Cargo version and which format is running.
  `linux/release.py metadata` fails unless both Cargo versions, the tag and the
  newest metainfo release agree.
- `linux/notices.py` writes the licence texts of every crate compiled into the
  four executables (`THIRD-PARTY-RUST.txt`).

## Flatpak

Files: [`linux/flatpak/`](../linux/flatpak/).

- Runtime `org.gnome.Platform//51` with `org.freedesktop.Sdk.Extension.rust-stable//26.08`
  (Rust 1.98 at the time of writing; `rust-version = "1.93"` rejects older).
  OpenBLAS 0.3.34 is built from source with `DYNAMIC_ARCH=1 TARGET=GENERIC
  NUM_THREADS=128 USE_OPENMP=0 USE_THREAD=1 NO_AFFINITY=1 ONLY_CBLAS=1 NO_STATIC=1`
  (the AppImage uses the same settings).
- Both Cargo lockfiles are vendored through `cargo-sources.json` and
  `cargo-sources-linux.json`, written by
  [`cargo_sources.py`](../linux/flatpak/cargo_sources.py). It accepts only
  crates.io packages, so a git or other-registry dependency fails generation
  instead of reaching for the network. CI runs it with `--check`. Regenerate
  after any lockfile change. The build is offline.
- Permissions: `--share=ipc --socket=wayland --socket=fallback-x11 --device=dri`.
  No home, host, network or session-bus access. Files come through portals:
  - **Open Photograph** grants one file. The film strip then shows only it.
  - **Open Folder…** (new, every format) grants a folder, so the film strip
    shows all of its photographs. It opens the first one.
  - Save and export keep the GTK save dialog. Tested on the real document
    portal: writing a hidden staging file next to the target and renaming it
    over the target works for new files, replaced files and folder grants
    ([results](#tested)). Export stays atomic.
- **Protecting the original.** One host file seen through two portal grants
  (say, opened as a single file, then saved into a folder grant) has two
  different inode numbers, so the native inode check alone would let an export
  replace the original. [`linux/src/portal.rs`](../linux/src/portal.rs)
  compares the portal's host path (the `user.document-portal.host-path`
  attribute, else `Documents.GetHostPaths`). If either path can't be
  identified and the destination exists, Negs refuses and asks for another
  filename. Host paths are only used to compare and display, never to open files.
- Crash cleanup records live in `$XDG_RUNTIME_DIR/app/io.github.noahhaasis.negs/negs`,
  the runtime directory Flatpak shares between instances.
- "Show presets folder" opens through the OpenURI portal.

Build, install and check locally (needs `flatpak` and `flatpak-builder`):

```sh
bash linux/flatpak/build.sh target/flatpak/out      # writes negs-VERSION-linux-x86_64.flatpak
bash linux/flatpak/verify.sh target/flatpak/out/negs-*-linux-x86_64.flatpak
python3 linux/check_engine.py --flatpak io.github.noahhaasis.negs   # real Wayland/GPU audit
flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest linux/flatpak/io.github.noahhaasis.negs.json
```

`verify.sh` installs the bundle for the current user, runs the payload and CPU
engine checks inside the sandbox with read-only access to the check scripts
only, and starts and quits the GUI.

Installing the GitHub bundle:

```sh
flatpak install --user negs-0.11.0-linux-x86_64.flatpak
```

The bundle names Flathub as its runtime source, so Flatpak downloads the GNOME
runtime if needed. It is not an update channel: install the next bundle
over it.

## AppImage

Files: [`linux/appimage/`](../linux/appimage/).

- Built in `ubuntu:24.04` pinned by digest. The binaries need at most Ubuntu
  24.04's glibc (the build records the actual symbol floor in
  `library-ledger.json`).
- **GTK.** Negs needs GTK 4.16 and libadwaita 1.6. GTK 4.16.13 is the newest
  series that builds against Ubuntu 24.04's own GLib, Pango, cairo, HarfBuzz and
  Wayland libraries. 4.18 and later need newer cairo, HarfBuzz and Wayland,
  which would have to be bundled. A bundled Wayland or HarfBuzz is exactly what
  host graphics drivers and fonts would then load in place of their own. So
  the AppImage bundles GTK 4.16.13, libadwaita 1.6.10 and OpenBLAS 0.3.34 and
  takes everything drivers share from the host. GTK 4.16 is past upstream's
  maintenance window. Security fixes therefore mean rebuilding with a newer
  stack or raising the baseline; review this at every runtime refresh.
- **libjpeg.** GTK links `libjpeg.so.8`, which Fedora doesn't ship (it has
  `libjpeg.so.62`), so it is bundled. Ubuntu's libjpeg-turbo 2.1 can't be the
  bundled copy: once loaded it also serves host libraries, and Arch's libtiff
  needs the 12-bit API of libjpeg-turbo 3. The AppImage builds libjpeg-turbo
  3.2.0, a superset of both.
- **libadwaita patch.** libadwaita 1.6 links libappstream, which would bring
  curl, libxml2 and ICU. Arch has already moved libxml2 to a new soname, so
  those can't come from the host either. AppStream is only used by
  `adw_about_*_new_from_appdata()`, which Negs doesn't call.
  [`libadwaita-optional-appstream.patch`](../linux/appimage/libadwaita-optional-appstream.patch)
  makes it optional.
- **Which libraries are bundled.** [`bundle.py`](../linux/appimage/bundle.py)
  keeps an explicit host list by soname: glibc, the compiler runtime, GLib,
  Pango/HarfBuzz/fontconfig/freetype, cairo, gdk-pixbuf, libpng/libtiff, zlib,
  Wayland, X11/xcb, EGL/GL/Vulkan loaders, DRM/GBM, zstd, expat and similar.
  Everything else is copied in and gets a relative RUNPATH; nothing sets
  `LD_LIBRARY_PATH`. The build fails if any bundled object resolves a
  non-host library from outside the AppDir. The ledger lists each bundled
  library and which executables need which host library.
- GTK is built with a private prefix (`/opt/negs-gtk`), without Vulkan, media,
  printing or introspection, so it never loads host GTK modules. AppRun
  prepends the AppDir to `XDG_DATA_DIRS` for GTK's compiled GSettings schemas
  (the file chooser needs them) and the Adwaita symbolic icons, which are
  bundled for hosts without Adwaita. The originals are saved as `NEGS_ORIG_*`
  and restored for programs Negs starts, such as the file manager.
- Runtime: the static type-2 runtime `20251108` (sha256 pinned). It mounts with
  `fusermount3` or `fusermount` and doesn't need libfuse2. Without FUSE, run
  `./Negs-*.AppImage --appimage-extract-and-run`.

Build and check locally (needs Docker):

```sh
python3 linux/release.py prepare --out target/appimage/release   # committed checkout only
bash linux/appimage/package.sh target/appimage/release/assets/negs-VERSION.tar.gz target/appimage/out
bash linux/appimage/smoke.sh target/appimage/out/Negs-VERSION-x86_64.AppImage ubuntu fedora arch
python3 linux/check_engine.py --appimage target/appimage/out/Negs-VERSION-x86_64.AppImage
```

`smoke.sh` runs the AppImage in clean Ubuntu 24.04, Fedora 43 and Arch
containers that have an ordinary desktop library set but no GTK 4, libadwaita
or OpenBLAS: payload and CPU engine checks on the extracted payload, then a GUI
start and quit under Xvfb through extract-and-run.

The build also writes `negs-VERSION-appimage-sources.tar`: the GTK, libadwaita
and OpenBLAS sources, the patch and the build scripts. That is the corresponding
source for the LGPL libraries the AppImage contains. It is published with every release.

## CI and release

The [Linux workflow](../.github/workflows/linux.yml) runs three jobs: Arch,
`flatpak` and `appimage`. Each validates the release metadata. Release
publication needs the whole workflow. Before creating the draft, the publish job
runs [`scripts/check_release_assets.py`](../scripts/check_release_assets.py):
it fails unless every expected macOS, Windows, Arch, Flatpak and AppImage file
is present and matches its checksum file. Neither new format can go missing
silently. Each format writes its own checksum file (`SHA256SUMS-flatpak`,
`SHA256SUMS-appimage`), so parallel jobs never overwrite each other's.

CI caches the Flatpak builder state and, for the AppImage, the built
GTK/libadwaita/OpenBLAS prefix, source downloads and crates. A cold AppImage
build compiles GTK, libadwaita and OpenBLAS once per change to
`linux/appimage/`. CI starts the GUI under Xvfb with the CPU engine; real portal,
session and GPU checks run on a desktop before tagging (see below).

## Flathub submission

Flathub builds from source; the `.flatpak` bundle is not the submission.
Flathub's [generative AI policy](https://docs.flathub.org/docs/for-app-authors/requirements)
governs what can be submitted:

- Flathub manifests must not contain AI-generated or AI-assisted content, even
  with disclosure. The manifest in `linux/flatpak/` was written with an AI
  assistant, so it is only for this repository's CI and the GitHub bundle. The
  Flathub manifest must be written by the maintainer, and its Cargo sources
  generated with Flathub's own `flatpak-cargo-generator`.
- AI-generated code, documentation and packaging in the application must be
  disclosed in the submission, naming the affected parts and their approximate
  extent. For this work that includes the metainfo and desktop files,
  `linux/src/portal.rs` and the packaging scripts in `linux/flatpak/` and
  `linux/appimage/`. Reviewers may reject a submission based on the extent
  of AI-generated material.
- AI tools must not open or automate the submission pull request, or write its
  description, commit messages, review comments or replies.

Steps once a release tag is public: fork
[flathub/flathub](https://github.com/flathub/flathub), branch from `new-pr`,
add the manifest, Cargo sources and `flathub.json` (`{"only-arches": ["x86_64"]}`)
at the root, and open the pull request against `new-pr` in the GitHub web
interface. After merging, accept the invitation to
`flathub/io.github.noahhaasis.negs` (needs two-factor authentication) and
verify the app in the Flathub developer portal through the `noahhaasis` GitHub
account. Publish Flathub install links only after acceptance. Everything in
the "Flatpak" section above is reference material on what the app needs:
permissions, runtime, OpenBLAS options and the offline Cargo build.

## Tested

Local results on 2026-10-28, on Arch/Omarchy (Hyprland, xdg-desktop-portal
1.22.1, flatpak 1.18.4, AMD GPU). Evidence is in `target/linux-validation/distribution/`.

- Flatpak: built offline from both lockfiles (9 min cold). Inside the sandbox
  the CPU engine passed 21 requests, including f64 and Fast exports. On the
  real Wayland session the GTK/engine audit passed all 11 checks on the GPU
  (`wgpu (GPU)`, GTK Vulkan renderer, data from `/app`): LUT, preview, Fast,
  Reference and wide-gamut exports, cancellation preserving an existing file.
  Its optional RAW step only fails because it expects one specific
  6048×4024 NEF; a 4000×6000 ARW developed at its true size.
- Document portal ([`results.json`](../target/linux-validation/distribution/portal/results.json)
  locally): staging-and-rename works in single-file and folder grants; two
  aliases of one file have different inodes and the same host path; folder
  grants list siblings. Note that `flatpak document-export` 1.18.4 fails against
  this portal version; the tests used the Documents D-Bus API directly.
- AppImage (94 MB; 2 min warm, about 10 min with a cold GTK stack): on this
  desktop it ran through the real FUSE mount and passed the same 11-check
  GTK/engine audit on the GPU, with GTK's GL renderer. Relaunching restored
  the session from the preview cache (cached frame at 402 ms) although the
  mount path changed. In clean Ubuntu 24.04, Fedora 43 and Arch containers
  without GTK 4, libadwaita or OpenBLAS, the payload and CPU engine checks
  and an Xvfb GUI start passed. The glibc symbol floor is 2.39. The first
  Arch run found Ubuntu's libjpeg shadowing the host's (see above).
  Host requirement found in testing: `libGLESv2.so.2` (Ubuntu `libgles2`,
  Fedora `libglvnd-gles`), which libepoxy loads at run time. Desktop systems
  with GTK installed already have it.
- `flatpak-builder-lint`: the manifest passes. The repo check reports only
  screenshots, which resolve when the tagged screenshot URL exists and
  Flathub mirrors it.

Not yet tested and needed before calling a format fully validated:
interactive Open/Open Folder/Save through the portal UI on GNOME and KDE,
revoked grants, NVIDIA and Intel GPUs, X11 sessions, a stock Ubuntu 24.04
desktop with FUSE (normal AppImage mount), and Fedora on hardware.
