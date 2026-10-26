# macOS installation, updates, and releases

Negs is currently **0.9.0, build 9**, before the public 1.0 release. The
installer is a universal app for Apple silicon and Intel Macs on macOS 14+. The source repository
and downloads will be public from the first release. No remote repository has
been configured or release published by this implementation.

## Installation

Open the DMG, drag Negs to Applications, and launch it from there. The app
bundles its engine, exporters, profiles, and Sparkle. Nothing is installed outside
the application bundle. A ZIP containing only the app is also available.

The app uses ad-hoc signatures, **without an Apple Developer subscription or
notarization**. macOS may block a downloaded app on first launch. After attempting
to open it, use System Settings → Privacy & Security → Open Anyway. This remains
a user-visible limitation of distributing without Developer ID. Sparkle's
Ed25519 update signatures are independent of Apple signing and cost nothing.

## Removing Negs and its data

In **Negs → Settings**, choose **Remove All App Data and Quit…** and confirm.
It discards the editing session, preferences and recent-file history, and
removes app-owned caches (including the rendered startup frame), updater/HTTP
storage, logs, saved window state and abandoned temporary engine files. Original
photographs and exported images are preserved. Then move Negs from Applications
to the Trash if you also want to remove the app itself.

The action waits for an active save/export or update session to finish before
it becomes available. It stops the helper and updater and drains pending I/O
before deleting data, and quitting does not save a replacement session. Failed
removals are reported with a Retry option; another running copy's locked
temporary files are preserved. Normal quits and updates still save the session.

## Update UI and restart behavior

Release builds use Sparkle 2.10.0's native windows and release notes. The
Negs menu includes Check for Updates; Settings includes automatic checks
and the last check time. Automatic checks are enabled in release builds, run on
Sparkle's six-hour schedule, and can be disabled. No automatic installation or
system profiling is enabled. Reminder/skip behavior belongs to Sparkle.

Updater initialization waits at least three seconds and until the initial
preview is ready (or the empty editor is ready / startup fails). Network checks
never run synchronously on the launch path. A manual check reports errors;
background failures stay out of the editor. The app checks compatible OS and
architecture requirements through Sparkle's feed metadata.

When the user chooses to restart for an update, an active save/export finishes
first. Normal application termination also protects those operations. Before
termination, a session is written atomically off the main actor. If the write
fails, termination is cancelled and the app reports the error.

The session restores the source photograph, engine parameters, film, paper,
preview limit, crop, rotation, borders, inspector visibility, and fit/actual-size
mode. It includes an in-progress crop. Arbitrary trackpad zoom/pan positions and
undo history are not persisted. Sessions are saved on normal quit/update restart;
there is no crash autosave in this implementation.

The session is a small, schema-versioned JSON file at
`~/Library/Application Support/dev.negs.app/session.json`. Bookmarks can
resolve a moved photograph. Missing/unreadable photos, corrupt sessions, and
unsupported schemas leave the editor usable. Loading does not mount offline
volumes or display bookmark dialogs. Explicit Finder/command-line opens take
precedence over the previous session. Engine defaults are merged with saved
parameters before the restored photograph's first render.

Local builds without both a feed URL and public key disable the updater. They
still support session restoration. Existing 0.1.2 builds need a manual install
of the first updater-enabled release.

## First-time repository setup

Once the GitHub repository URL is chosen:

1. Push the reviewed source to the public repository. This workflow publishes
   into that same repository; no separate distribution repository is needed.
2. In Settings → Pages, choose GitHub Actions as the publishing source. The
   dedicated Pages deployment contains the update feed; do not share this
   deployment with a separate website without combining their artifacts.
3. Resolve the Swift package (`swift package --package-path macos resolve`). Run
   `macos/.build/artifacts/sparkle/Sparkle/bin/generate_keys --account negs`
   once to create the production update signing key in the login Keychain.
4. Store its public key as the repository variable `SPARKLE_PUBLIC_KEY`. Export
   the private key with `generate_keys --account negs -x /secure/path/key`,
   store its contents as the repository secret `SPARKLE_PRIVATE_KEY`, and keep an
   offline backup. Do not put the export in this checkout or release assets.
5. Optionally set the repository variable `UPDATE_FEED_URL` for a configured
   custom domain. Otherwise CI derives
   `https://OWNER.github.io/REPOSITORY/appcast.xml` from `github.repository`.
6. Exercise a real downloaded old-build → new-build upgrade before the first
   public launch, including Gatekeeper, relaunch, and an in-progress export.

The production signing key has intentionally not been generated. Without an
Apple signing identity as an alternative trust anchor, losing the Sparkle key
can require users to reinstall manually. Keep it across builds and repository
renames. Keep the installed feed URL working if hosting changes.

## Versioning and publishing

- `Cargo.toml` owns the product version. Use numeric `0.MINOR.PATCH` before 1.0;
  increment PATCH for fixes and MINOR for features. Update workspace entries in
  Cargo.lock when changing it.
- `release/build-number` is a positive integer, incremented for every release.
  It becomes CFBundleVersion and Sparkle's ordering value. Do not reset it for
  a new product version or reuse a published number.
- Add `release/notes/VERSION.md`, then tag the reviewed commit `vVERSION`.
  CI rejects tag/version mismatches and missing update configuration.
- Tags beginning with `v` run the release pipeline. Manual workflow dispatch
  builds downloadable CI artifacts without publishing or signing a live feed.
- GitHub marks `0.x` releases as prereleases. The update feed still offers these
  versions to pre-1.0 users; it does not use GitHub's “latest release” shortcut.

The workflow builds only macOS, on the explicit `macos-15`
Apple Silicon runner. macOS uses Rust 1.93.1, the committed Cargo/Swift lockfiles,
and the runner's Swift/Xcode toolchain (reported in the CI log). It tests metadata
and native lifecycle behavior, builds the DMG/update ZIP/source archive, and
checks signatures, architectures, dependencies, data, and source snapshots.

`prepare_macos_update.py` verifies the previous signed feed, rejects reused or
older build numbers, preserves compatible historical feed entries, and invokes
Sparkle's generator. It signs the ZIP, feed, and embedded release notes. An
independent CryptoKit verifier checks the archive signature against the **public
key embedded in the app's configuration**, catching a mismatched CI secret.
Private signing material is passed through stdin, never command arguments or a
file in the source/archive tree.

Only after both platform builds succeed does CI create a draft GitHub Release,
upload all assets, and publish it. It checks that the update ZIP is anonymously
downloadable, then deploys the feed to Pages last. Releases are serialized;
already-published assets are never overwritten. No Apple signing/notarization
secrets or steps are involved.

If publication fails while still a draft, inspect/remove that draft before
retrying. If only Pages deployment fails, rerun the deployment job rather than
rebuilding/replacing the release. A missing feed is accepted only for build 1;
subsequent releases require it. To withdraw a bad version, deploy a newly signed
feed without that entry and follow it with a fixed higher build number. Merely
removing an entry does not downgrade copies already installed.

GPL-3.0 corresponding source accompanies every release as an explicit archive;
GitHub also exposes the repository. Keep the tagged source and dependency
versions available. The public source and asset URLs are immutable; the feed is
the small mutable entry point. Full ZIP updates are used initially, without deltas.

## Local build and validation

Requires Rust, Xcode/Swift, and Python 3.11+. `PYTHON` can select the interpreter.

```sh
PYTHON=/opt/homebrew/bin/python3 bash scripts/package_macos_app.sh
swift test --package-path macos --force-resolved-versions
python3 -m unittest discover -s scripts -p 'test_macos_release.py'
```

The local package is `dist/releases/Negs-0.9.0-macos-universal.dmg`; the folder
also includes the update ZIP, source archive, installation instructions, build
information, and SHA-256 checksums. `scripts/verify_macos_package.py` audits an
extracted ZIP independently of the build tree. No production key is necessary
for a local build.

The local validation covered native session/lifecycle tests, the DMG and app ZIP,
a temporary-key signed feed, independent archive verification, and rejection of
a wrong public key and a tampered archive. Hosted GitHub Actions/Pages publication
and an actual user-facing upgrade remain to be exercised once the repository and
production key are configured.

## Intel Macs

The package is universal: `NegsApp` and the three Rust executables each carry
an arm64 and an x86_64 slice (`lipo -archs`), so one download and one update
feed cover both. Apple silicon Macs run the arm64 slices unchanged.

What has been checked on Apple silicon, running the x86_64 slices under
Rosetta: the Rust test suites, byte-identical GPU renders and CPU renders within
2.4e-7, and the app opening and developing a photo. Under Rosetta the GPU is
still Apple's, so Intel and AMD graphics themselves are untested.

The GPU risk is Metal's per-pipeline thread limit (`maxTotalThreadsPerThreadgroup`),
which depends on the GPU and the shader. Metal drops a larger dispatch without
an error, which already happened to the grain shader at 1024 threads on Apple
GPUs (limit 832). The flat per-pixel shaders therefore declare an
`override WORKGROUP_SIZE`, and on GPUs that are not Apple-designed the backend
compiles each one exactly as wgpu-hal does (MSL 3.1, bounds checks, binding
slots, immutable read-only buffers, the chosen size), queries its limit and
lowers the size to fit. With Metal API validation on (`MTL_DEBUG_LAYER=1`),
the real pipeline's limit for grain at 1024 matches the query exactly.
Renders are byte-identical at every size from 1024 down to 64.

Diagnostics, all off by default:

- `SPEKTRAFILM_WGPU_PROBE_WORKGROUPS=1` queries the limits on Apple GPUs too.
- `SPEKTRAFILM_WGPU_WORKGROUP_SIZE=N` caps the per-pixel workgroup size at N.
- `RUST_LOG=spektrafilm_gpu=debug` logs each queried limit; a fixed-size
  shader over its limit logs a warning.

`TODO(intel)` comments mark what still needs real Intel hardware: the GPU
choice on dual-GPU MacBook Pros, the mapped-buffer path on Intel iGPUs, the
lowered workgroup sizes, an AVX2 build of the CPU path, and RAW decoding.

## Startup measurements

Five alternating runs per variant on this Apple Silicon Mac, with the same
engine and generated 3600×2400 JPEG. Release builds use the existing benchmark's
real event loop, with one additional readiness timestamp. Medians from process
launch (including dynamic library loading):

| Variant | Editor ready | First developed preview |
| --- | ---: | ---: |
| Baseline at 2154652 | 1429.5 ms | 2009.8 ms |
| Updated app, explicit open | 1422.0 ms | 2013.3 ms |
| Updated app, restored session | 1389.6 ms | 2007.3 ms |

The explicit-open preview difference is +3.5 ms (0.17%), within run-to-run noise;
these runs do not show a material startup regression. Readiness is an event-loop
proxy for time to interactive, not a full input-to-paint measurement. No cold-cache
or large-RAW guarantee is implied. Live hosted update requests were not part of
this comparison; Sparkle initialization is deferred beyond the measured startup.
The complete measurements are in `macos-startup-update-results.json`.

Use `scripts/benchmark_macos_startup.py --help` to repeat the comparison. Both
executables must be built with `-Xswiftc -DSPEKTRAFILM_BENCHMARK`; a separate
`--session` file can exercise restoration without changing the user's saved edit.

References: [Sparkle setup](https://sparkle-project.org/documentation/),
[release publishing](https://sparkle-project.org/documentation/publishing/),
[GitHub Pages workflows](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages),
[GitHub runner architectures](https://docs.github.com/en/actions/reference/runners/github-hosted-runners),
[GPLv3](https://www.gnu.org/licenses/gpl.en.html).
