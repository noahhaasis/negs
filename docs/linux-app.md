# Native Linux app

Negs has a native GTK4/libadwaita editor connected to the existing Rust
Spektrafilm engine. The UI runs in its own Cargo workspace, `linux/`, and
uses a persistent, serial `spektrafilm-native` helper for GPU preview and
retained Save Preview. Fast export uses the f32 engine; Reference export
uses the separately built f64 binary. macOS and Windows UI sources are unchanged.
The experimental engine migration has since merged from main. Linux inspector
schema parity and fresh installed-app validation are required before releasing
that combined revision; see [the release checklist](linux-release.md).

## Build and run

Requirements: Rust **1.93.1**, GTK **4.16+**, libadwaita **1.6+**, pkgconf,
OpenBLAS, a C toolchain, and Vulkan loader/driver for hardware acceleration.
On Arch, install the dependencies declared in `linux/arch/PKGBUILD` and
install the pinned toolchain with `rustup toolchain install 1.93.1 --profile minimal`.
CPU fallback works when hardware Vulkan is unavailable or a render exceeds
GPU admission limits. The status bar reports the actual backend.

```sh
bash linux/build.sh
bash linux/run.sh /absolute/path/to/photo.NEF
```

Both workspaces build with `--locked`. Build products are
`target/linux-ui/release/negs` and `target/release/{spektrafilm-native,spektrafilm,spektrafilm-f64}`.
The f64 feature is built separately and never enabled for the helper/Fast binary.
`linux/core-env.sh` also supports this machine's optional development OpenBLAS
sysroot; production Arch installations use the declared `openblas` dependency.

Without a path, the app restores its last photograph, adjustments and view.
`--fresh` skips restoration. `NEGS_PROFILE_DIR=/absolute/test/directory` isolates
settings and permits independent test windows. `NEGS_APPEARANCE=light|dark|system`
selects an app-only appearance. Normal launches follow the desktop setting.
`NEGS_UI_ONLY=1` retains the explicit camera-preview test mode; ordinary launches
always connect the real engine. The old preview installer remains a separate,
clearly labeled UI development tool.

## Local packages and installation

Commit the intended source first; package archives are made from that commit,
never from validation directories or untracked files. No publication occurs.

```sh
bash linux/package.sh
```

This generates a source archive and checksum-pinned local PKGBUILD, then runs
`makepkg`. Outputs are under `target/linux-package/<short-commit>/assets/`, with
AUR/Omarchy recipes alongside in `recipes/`. Use an empty `NEGS_PACKAGE_OUT`
directory for a retry. The recipe builds both workspaces and installs the launcher, helper,
f32/f64 exporters, profile/LUT/border data and licenses, icon, desktop entry,
documentation and source archive. The app ID/window class is `dev.negs.app`.
Open With MIME declarations do not change file-association defaults. Install a
reviewed Arch artifact using the normal package manager; updates initially use
distribution packages. Flatpak and other distribution recipes are unfinished.

For a per-user local bundle on a compatible GTK/Arch system:

```sh
bash linux/package.sh --bundle
# Unpack the generated negs-VERSION-linux-x86_64.tar.gz into a directory.
python3 /path/to/unpacked/share/negs/install.py --from /path/to/unpacked
~/.local/bin/negs
```

The default prefix is `~/.local`; `--prefix` supports an isolated installation.
The bundle can include the official OpenBLAS shared library and license when
using the development sysroot. It still requires system GTK/libadwaita, Vulkan
loader, Python, libgfortran/libgomp and their runtime dependencies. It is not a
universal binary for arbitrary older Linux distributions.

```sh
python3 ~/.local/share/negs/install.py --uninstall
```

The per-user installer records owned files and refuses to overwrite unowned or
modified files. Uninstall preserves changed files, settings, photos, exports,
and the harmless app-specific Hyprland rule. Arch packages are removed through
the package manager instead.

## Opacity and desktop integration

Canvas and image backing are opaque; output RGB is presented as explicitly
sRGB RGBA with alpha 255. Omarchy also applies compositor opacity to entire
application windows. On an existing Hyprland **Lua** setup, installation/first
launch copies the packaged Negs-only rule, adds its include with backups, and
reloads/checks the active compositor. This removes the default opacity tag and
sets absolute focused/inactive/fullscreen opacity to 1. No global opacity,
theme or unrelated application rule is changed. Package installation as root
never edits user configuration. Other compositors use the normal opaque GTK
surface. Older Hyprland `.conf` setups require their own equivalent rule.

GApplication forwards file-open activation to the running editor. GTK native
file dialogs and GIO folder notifications are used directly. The inspector
collapses with a libadwaita breakpoint at narrow widths. Keyboard commands,
numeric fields, accessible labels, per-control reset, and one undo step per
drag are supported.

## Rendering, exports and persistence

The window paints before inspector construction and helper setup. Worker
threads perform decode, helper requests, cache I/O and exports. Edits coalesce
behind a single serial render; source/revision guards reject obsolete replies.
Film and 35mm border textures decode at native resolution on a worker and reuse
a bounded texture cache. Rapid stock changes coalesce behind one border load.
See [border performance measurements](linux-border-performance.md) for selection
latency, cache limits and validation.
Interactive renders use a viewport proxy; releasing the slider rebuilds the
full preview. The last image remains visible while processing. Re-selecting
the same filmstrip file preserves the texture, edits and history.
Filmstrip thumbnails and camera previews use the primary RAW orientation for
TIFF-based containers (including NEF), overriding conflicting embedded JPEG tags.
Portrait images keep their orientation and aspect ratio in the strip.

Original uses separate pixels and preserves the helper's retained developed
render. Crop, straighten, rotation, border/keyline/35mm settings and output sizing
are shared with export. Image Size resizes before simulation. Save Preview is
enabled only for the current retained float render; a restored cached preview
is rebuilt before becoming saveable. Fast/Reference exports capture immutable
settings while the editor remains available. They publish through a same-folder
staging file only after success. Cancellation preserves existing destinations,
and source-file identity checks reject overwrites through aliases. JPEG quality
and metadata choices affect actual output. LUT export needs no photograph.

Settings/session use `$XDG_CONFIG_HOME/negs/session.json`; the single bounded,
lossless developed preview uses `$XDG_CACHE_HOME/negs/rendered-preview.cache`.
The key includes source identity, full processing settings, profiles/data/helper,
BLAS and available GPU/driver identities. Framing is applied separately. Cache
corruption/mismatch falls back to decoding. Session writes are atomic. Temporary
helper/output files live under `$XDG_RUNTIME_DIR/negs`, with a private `/tmp`
fallback. Quit cancels/reaps child processes and flushes the cache off GTK.

The native helper protocol remains version 1. Optional save-quality/metadata
options retain old defaults; the Linux client's opt-in `crop_after_post` makes
straightened Post output match CLI/canvas order while old clients retain their
existing behavior. Capability checks reject mismatched installed components.
No stochastic or portable numerical algorithm was rewritten for this port.

## Validation and limits

The tested app is installed at `~/.local/bin/negs`. The local Arch package is
`target/linux-package/negs-0.7.0-1-x86_64.pkg.tar.zst`; the per-user bundle is
`target/linux-package/negs-0.7.0-linux-x86_64.tar.gz` (product source 41ed8c6).

See `docs/linux-core-validation.md` for numerical gates, real desktop evidence,
performance and installation results, and `docs/linux-ui-validation.md` for the
preceding UI-only measurements. These are separate experiments.

The transferred historical Mac reference predates the DIR-gamma correction;
it cannot establish same-revision Mac/Linux parity. CPU shared-stream grain
differences remain deferred. Device-loss/watchdog recovery is unresolved;
no driver timeout settings were changed. This machine provides one NVIDIA GPU
and one 1.25-scale display: other vendors, mixed-monitor scaling and wide-gamut
end-to-end color still need hardware validation. GTK receives explicit sRGB
pixels; this is not proof of calibrated wide-gamut output on every compositor.
