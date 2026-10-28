# Linux distribution for the next release

The next release must include a tested Flatpak installer for Linux x86_64.
Submit the same version to Flathub, but keep the GitHub release independent of
the external review schedule. AppImage is a useful additional download, not a
requirement for this release. This is a plan; packaging is not implemented yet.

## Recommendation

Use Flatpak with the GNOME runtime as the primary distribution for general
Linux desktops. Keep the existing Arch/AUR/Omarchy packages for their users.
Negs requires GTK 4.16+ and libadwaita 1.6+, and the current Arch-built download
requires glibc 2.44. A Flatpak built inside its SDK supplies a consistent user
space instead of requiring those versions from the host distribution.

An AppImage would offer a single executable download without installing
Flatpak. It would require a separate compatibility build and bundling of GTK,
libadwaita, OpenBLAS and their resources. Wrapping the current Arch binaries
would retain their glibc requirement. Test any future AppImage on the oldest
supported distribution as well as current desktops; its runtime and graphics
integration need their own validation.

Flatpak requires an installed Flatpak client and a shared runtime, and adds
sandbox integration work. AppImage adds dependency bundling and compatibility
maintenance. Maintaining both means testing two distribution paths.

## Release acceptance checklist

- [ ] Choose the permanent application ID before the first Flathub submission.
  `io.github.noahhaasis.negs` matches the current GitHub repository and supports
  verification through its owner. A domain-based alternative is
  `com.getnegs.Negs` if control of `getnegs.com` is available. The current
  `dev.negs.app` ID does not meet the new-submission rules, including the
  restriction on ending an ID with `.app`.
- [ ] Align GTK application ID, desktop entry, icon, MIME integration and
  AppStream metainfo. Add upstream metainfo with description, screenshots,
  release information and appropriate license declarations. Update affected
  integration checks and existing desktop/window matching references.
- [ ] Create a Flatpak manifest using the latest GNOME runtime at submission
  time and a Rust SDK extension satisfying the application's Rust requirement.
  Declare x86_64 as the initial supported architecture in `flathub.json`.
- [ ] Generate offline dependency sources from both `Cargo.lock` files. Build
  the GTK workspace, native helper and Fast f32 / Reference f64 exporters from
  source. Preserve the separate precision builds. The current scripts fetch
  Rust through rustup; adapt the Flatpak build to use the SDK toolchain.
- [ ] Install the helpers and data under `/app`; build OpenBLAS from source if
  unavailable in the runtime. Include required licenses and resources.
- [ ] Provide a sandbox launcher that resolves packaged data/helpers without
  running the native launcher's Omarchy configuration installer.
- [ ] Use portals and minimal permissions for file access. GTK already uses
  `FileDialog`, but a selected file alone does not establish access to sibling
  photographs: verify the filmstrip's folder scan and provide explicit folder
  selection where needed. Check exported files, preset import/export, external
  drives and persisted document access after restarting the application.
- [ ] Validate GPU access, Vulkan preview/Fast export and CPU fallback inside
  the sandbox. Check Reference export, RAW import, borders, cancellation,
  settings, presets and session restoration. Cover Wayland and X11 where
  supported, on Ubuntu LTS, Fedora and Arch/Omarchy.
- [ ] Add manifest/build/metainfo validation and package smoke checks to CI.
  Produce a versioned `.flatpak` bundle and checksum with each tagged release.
  Include a runtime repository URL so missing runtimes can be obtained.
- [ ] Document installation and updates. A `.flatpak` bundle requires Flatpak
  and does not include the shared runtime. Repository distribution provides
  the normal update path; a standalone bundle alone does not provide one.
- [ ] Submit the tested release to Flathub. New apps must enter its stable
  repository; beta-only first submissions are not accepted. Explicitly decide
  that the submitted version is ready for stable distribution: the current
  GitHub workflow marks all `0.*` releases as prereleases.
- [ ] Replace or supplement the Linux download instructions on the README and
  project website when the artifacts are available. Add the Flathub link after
  the listing is published. Do not advertise availability before acceptance.

## Flathub submission and updates

Flathub's standard source submission process builds the application from a
manifest; it does not take the GitHub release's `.flatpak` file as an upload.

1. Build, install and test the manifest locally with `org.flatpak.Builder`.
   Pin the application source and dependencies, and resolve validation errors.
2. Fork `flathub/flathub`, create a submission branch from `new-pr`, and add the
   manifest, generated dependency sources and supporting packaging files.
   Keep application source, binaries and upstream metainfo in Negs itself.
3. Open the submission PR through GitHub's web interface, targeting `new-pr`,
   with a title such as `Add io.github.noahhaasis.negs`. Follow the current PR
   template and submission policy. Address reviewer feedback and test builds.
4. After approval, accept the invitation to the new Flathub application
   repository. GitHub 2FA is required; accept the invitation within one week.
5. Sign into Flathub's Developer Portal and verify ownership. The GitHub ID
   supports verification through the repository owner's GitHub account;
   domain-based IDs support a website token or DNS verification.
6. For subsequent releases, open an update PR in the app's Flathub repository,
   test the resulting build and merge it to publish. Configure
   `x-checker-data` to propose source updates where appropriate; do not assume
   a successful compile establishes that the app works.

Flathub review has no guaranteed completion time. The next GitHub release must
therefore include its tested Flatpak bundle even if review is still pending.

## References

- [Flathub submission](https://docs.flathub.org/docs/for-app-authors/submission)
- [Flathub requirements](https://docs.flathub.org/docs/for-app-authors/requirements)
- [Flathub verification](https://docs.flathub.org/docs/for-app-authors/verification)
- [Flathub maintenance](https://docs.flathub.org/docs/for-app-authors/maintenance)
- [Flatpak runtimes](https://docs.flatpak.org/en/latest/available-runtimes.html)
- [Flatpak single-file bundles](https://docs.flatpak.org/en/latest/single-file-bundles.html)
- [AppImage native packaging](https://docs.appimage.org/packaging-guide/from-source/native-binaries.html)
