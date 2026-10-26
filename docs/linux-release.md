# Linux release and repository distribution

The GTK app, persistent engine, f32 Fast exporter and f64 Reference exporter
were integrated and validated in local Arch packages and per-user bundles
before the experimental engine changes merged from main. The subsequent Linux
compatibility update replaces removed grain fields and passes the live-helper
schema check. The desktop audit passes. Numerical validation exposed a Vulkan
border-indexing bug; the correction passes all 76 GPU-versus-f64 quality cases.
See [the post-merge validation report](linux-post-main-validation.md).

## Work before the first public Linux release

1. Finish the Linux inspector's optional control inventory: per-stock grain
   arrays, film/print bases, camera colour filters and Balance Print. Those
   controls landed in the macOS/Windows interfaces on main; Linux has its own
   interface. Existing Linux controls now accept the merged engine, preserve
   its unexposed defaults and use the current Arctic Beta identifier. The
   release recipe runs `parameters::tests::live_helper_schema` to catch future
   binding mismatches. Include defaults, reset and session restoration when
   adding the remaining controls.
2. Run fresh numerical gates and installed-app preview/export checks against
   this engine revision. The earlier 484-render baseline is historical; the
   main merge intentionally changes output. Establish a new Linux baseline and
   compare with Mac/Windows references from the same revision. Repeat desktop
   RAW, crop/Post, border, cancellation, session and startup checks.
3. Run the Linux workflow on GitHub and confirm the clean Arch build and
   installed-engine smoke test pass. Local recipe tests do not prove a clean
   container build. Review runtime dependencies on the target Omarchy channel.
4. Choose the next version, update both Cargo workspaces and their lockfiles,
   add release notes, and increment `release/build-number` for the existing
   macOS release process. The Linux recipe version is generated from Cargo;
   the repository template's version is replaced during generation.
5. Publish the tag through the existing multi-platform Release workflow. Review
   and upload the generated AUR recipes, then submit the Omarchy package PR
   using those publicly downloadable, checksum-pinned sources.

Other GPU vendors, mixed monitor scales, calibrated wide-gamut presentation,
screen-reader usability and device-loss recovery remain follow-up work. They
are distinct from establishing the first tested Arch/Omarchy release.

## Pipeline

Local audit on 2026-10-26: the offline release build passed, all five release
generator tests passed, and the rebuilt helper/exporters passed 21 CPU smoke
requests. The initial live helper schema failure on the removed
`film_render.grain.agx_particle_area_um2` binding was corrected, and the gate
now passes. The new real Wayland/RAW audit passes all 11 checks. The initial
GPU quality result was only 1/76; the corrected FIR shaders now pass 76/76,
with a worst Delta E00 of 0.001555. All 19 GPU unit tests pass. Final exact
repeatability is 484/484 identical; final package checks are in progress.
Package validation also found and fixed a concurrent scratch-lock publication
race, covered by a 16-thread regression test. Initial failing evidence is
preserved under `target/linux-validation/post-main/`. The clean GitHub runner
build awaits approval to push the source branch. No package was published.

[Linux workflow](../.github/workflows/linux.yml) runs on relevant pull requests,
manual dispatch, and as part of [Release](../.github/workflows/release.yml).
Release publication requires Linux, macOS and Windows to succeed.

The Linux job uses an Arch base-devel container, installs declared build/runtime
dependencies, and runs `makepkg` as an unprivileged user. `prepare()` fetches
the pinned Rust 1.93.1 toolchain and locked dependencies; compilation and tests
then run offline. The helper/Fast f32 build and Reference f64 build remain
separate. Tests cover the native helper and GTK workspace, including the real
helper's compatibility with the Linux control bindings. The extracted
package is checked for missing shared libraries, desktop entry validity, and
real helper/render/save/Fast/Reference behavior on CPU. Hardware Vulkan and
actual Wayland interaction remain desktop validation tasks.

The job produces:

- `negs-VERSION.tar.gz`: committed Linux source with data and licenses.
- `negs-VERSION-1-x86_64.pkg.tar.zst`: Arch package.
- `negs-VERSION-linux-x86_64.tar.gz`: the same package's application files,
  usable by the per-user installer or the binary AUR recipe. System GTK,
  libadwaita and OpenBLAS are required.
- `negs-VERSION-linux-recipes.tar.gz`: AUR `negs` and `negs-bin` recipes plus
  an Omarchy package directory, including `.SRCINFO` and `.omarchy` metadata.
- `SHA256SUMS-linux`: hashes of those assets.

`negs-linux` and `negs-linux-recipes` are the Actions artifact names. Tagged
releases also publish the assets to GitHub Releases. Branch/PR builds only
upload Actions artifacts: generated release URLs become usable after the tag
is published. No AUR or Omarchy write credentials are needed to build.

For a local run after committing the intended source:

```sh
bash linux/package.sh
# target/linux-package/<short-commit>/assets/ and recipes/
```

Outputs must be empty to avoid mixing builds or overwriting release assets.
Set `NEGS_PACKAGE_OUT=/absolute/empty/directory` for a fresh local retry. The
workflow uses `target/linux-release/`. Source archives exclude private photos,
validation dumps, caches and the original untracked handoff. The recorded
commit, version and source hash must remain the same through asset generation.

## AUR

Publish `negs` for a source build and optionally `negs-bin` for the prebuilt
Arch x86_64 bundle. The binary recipe provides/conflicts with `negs` so users
choose one. Recipes use versioned release URLs and actual SHA-256 hashes;
`.SRCINFO` is generated by `makepkg --printsrcinfo`.

Set the repository variable `AUR_MAINTAINER` to `Name <public email>` before
building the first release. Without it, recipes identify the repository owner
by their GitHub profile; review this before submission. An AUR account and its
registered SSH key are required to publish. Neither is stored in this repo.
The `negs` and `negs-bin` AUR Git repositories had no HEAD at the time of this
audit (2026-10-26); recheck availability before first publication.

After downloading and checking the release assets:

```sh
git clone ssh://aur@aur.archlinux.org/negs.git
# Copy recipes/aur/negs/{PKGBUILD,.SRCINFO,LICENSE} into that checkout.
cd negs
makepkg --verifysource
makepkg --cleanbuild --syncdeps
git add PKGBUILD .SRCINFO LICENSE
git commit -m 'Release Negs VERSION'
git push origin HEAD:master
```

Repeat for `negs-bin` if publishing both. Future releases generate updated
recipes automatically; uploading them is currently a maintainer step. AUR
automation can be enabled separately once the account and SSH deployment
credentials are established. Follow the
[AUR submission guidelines](https://wiki.archlinux.org/title/AUR_submission_guidelines).

## Omarchy package repository

Target [omacom/omarchy-pkgs](https://github.com/omacom/omarchy-pkgs), not the
desktop configuration repository. Copy `recipes/omarchy/pkgbuilds/negs/` into
a fork based on its current `master`, follow its contribution instructions,
and open a PR restricted to that package directory.

The generated recipe builds from source for x86_64 against Omarchy's channel
libraries. It uses the normal release ring. ARM support needs its own successful
build and validation before adding `aarch64`.

The `.omarchy/package.json` watch tracks this repository's versioned GitHub
releases and lets Omarchy propose checksum/version updates. It explicitly
accepts GitHub prereleases because the current Negs release workflow marks all
0.x releases as prereleases. This is separate from Omarchy channel promotion.

Omarchy maintainers must approve the first PR's builds. Their CI builds the
package, and accepted changes publish to edge; rc/stable follow their channel
promotion process. This project's pipeline cannot guarantee acceptance or
the date of stable inclusion. See the current
[contribution process](https://github.com/omacom/omarchy-pkgs#contribute-a-package)
and [upstream watch documentation](https://github.com/omacom/omarchy-pkgs/blob/master/docs/upstream-sources.md).

## Handoffs

`linux-agent-handoff.md` was a machine-transfer brief for starting the port.
`linux-core-agent-handoff.md` predates completed engine integration. Neither is
needed for packaging or as a current work queue. Preserve any useful historical
provenance; use this release checklist, `linux-app.md`, and the dated validation
reports for current work.
