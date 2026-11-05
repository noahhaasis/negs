# Linux port of the 5 November macOS UI and performance changes

Implemented 2026-11-05 from the [survey and plan](linux-macos-nov5-port-plan.md).

## Result

- The Film grid searches names, spaced IDs and groups. Every query word must
  match, ignoring case and accents. Search lists films once in catalog order,
  keeps the existing tile/thumbnail widgets, selects the first result on Return,
  clears on Escape, and restores the current film's scroll position when cleared.
  Paper and the plain menus retain their existing behavior.
- Rendering now distinguishes quick, drag and settled requests. Large-photo
  discrete edits use the physical canvas scale for a quick frame when at least
  half the pixels can be avoided. Settling starts immediately. Slider release
  skips the extra quick step; small photos, first open and restored-cache pixels
  also go directly to settled.
- The helper's actual retained-output key preserves the post-only fast path.
  Every render invalidates that assumption until a settled request succeeds;
  helper connection epochs invalidate it after restarts/cancellation.
- A current quick or settled frame can be held while matching uses its source
  decode. The old image remains visible until matching resolves. An exposure
  change discards the held frame; an unchanged solution, failure or disabling
  matching releases it. Source/revision guards cover replacements and undo.
- Quick frames never supply Save Preview or the launch cache. Optional stock,
  preset and graph requests wait for a current settled frame, including when
  matching is off. A failed quick request falls through silently to exactly
  one settled attempt; a settled failure reports once and stops.
- The merged shared input-gamut coordinate cache, 40-entry calibrated pipeline
  cache and four default favourite films are used by the rebuilt Linux helper.
  Existing explicitly empty/custom favourite lists remain intact.

Primary implementation: `linux/src/runtime.rs`, `brightness.rs`,
`stock_browser.rs`, `catalog.rs`, and helper connection tracking in `engine.rs`.
No simulation math, profile data, export algorithm or reference image changed.

## Measured behavior

The comparison uses the same rebuilt helper and shared caches on both sides.
The baseline is the GTK scheduler/brightness lifecycle at `1d9a0e0` (the same
source tree as the original survey commit `424076a`), compiled
in an isolated source copy with the same timing harness. Both use an isolated
profile, the same Sony ILCE-7M3 RAW, Gold 200 / Portra 400 alternating four times
per setting, unlimited preview, bounded BLAS threads and physical canvas scale
0.16625. Thumbnails are disabled. These are warm-process measurements with OS
and driver caches in place, not cold launch or pre-cache comparisons.

GDK frame-clock presentation feedback measures the first painted changed frame
and the final settled frame separately. Medians in milliseconds:

| Backend / matching | First changed frame, before → after | Settled frame, before → after |
|---|---:|---:|
| Vulkan, off | 1165 → 79 | 1165 → 1245 |
| Vulkan, on | 1236 → 471 | 2573 → 1620 |
| CPU, off | 30090 → 895 | 30090 → 31002 |
| CPU, on | 30317 → 3881 | 62689 → 33860 |

Before the port, matching-on's first change displayed the old exposure; the new
first frame has already passed matching. With Vulkan and matching off, the extra
quick render adds about 80 ms to final presentation (about 0.91 s on CPU),
while making the first change
visible much earlier. Texture-ready medians (before compositor presentation)
were 1111 → 55 ms / 1111 → 1193 ms with matching off, and 1176 → 447 ms /
2522 → 1563 ms with matching on. Results apply to this photo, hardware and scale.

The Vulkan helper's sampled settled resident memory ranged from 1013–1054 MiB
before and 1027–1069 MiB after. Process peak RSS was about 1878 → 1892 MiB.
CPU resident samples were 984–995 MiB before and 1033–1069 MiB after;
peak RSS was about 2917 → 2990 MiB.
Allocator and backend caches affect these samples; quick rendering keeps an
additional size-dependent source/preparation cache and can add total work.

Raw timing reports and private validation artifacts are kept under
`target/linux-validation/macos-nov5/` rather than committed.

## Validation

- Linux release unit suite: 68 passing tests, including all-word/accent search, policy thresholds,
  request-reuse identity, and missing/custom/empty favourite preference cases.
- Core gamut-cache bit-identity checks, including the expanded ignored films.
- Native helper suite: all 14 checks pass serially on CPU and Vulkan, including
  pipeline pin/eviction behavior and quick-before-settled output identity.
  A persistent-helper probe also verified thumbnail → main → tone-curve cache
  hits, brightness solving, and main-pipeline reuse after releasing thumbnails.
  The cold thumbnail build took 48.1 ms, then main/curve cache lookups took
  about 0.34–0.41 ms; this is a cache-hit sanity check, not a full-frame benchmark.
- Mapped GTK/real-helper lifecycle test uses a deterministic 6 MP photo and
  fixed canvas scale. It verifies quick → settled, unchanged final pixels against
  an independent helper, no quick-frame disk cache/Save, post-only reuse,
  drag → settled, held matching and deferred Save, undo and coalesced edits,
  failed quick fallback, restored-cache exclusion, Original and source changes,
  main-before-thumbnail request ordering with matching off,
  and one reported settled failure without a retry loop.
- Existing live-matching test covers unchanged solutions, per-look/per-photo
  failure release, stale solves, disabling during a solve, deferred Save and
  source-bound cancellation. Deferred Save now allows up to ten minutes for the
  bounded reference/solve/render sequence; its previous 30-second timeout could
  abandon a valid large-RAW CPU render.
- Mapped GTK search test covers typing without an edit, catalog ordering,
  thumbnail retention, favourite changes, Return, empty results and Escape.
- Full app/helper/f32/f64 release build. GTK/export audit: 14 checks pass on
  Vulkan with a Nikon RAW (6048 × 4032), including custom WB and copied metadata;
  11 checks pass on CPU with Vulkan disabled for both helper and Fast exporter.
  These cover retained Save vs independent Fast byte identity, crop/rotation/
  border geometry, Display P3/16-bit ICC export, separate f64 Reference export,
  LUT export, and cancellation preserving an existing destination.
- Six release recipe/source-archive tests and Flatpak Cargo source-list check.

An initial parallel native test invocation inside the restricted sandbox
terminated with SIGSEGV. Its core (PID 38119, recorded under the generated test
binary name) shows a null call from `libvulkan.so.1` during
`vkEnumerateInstanceExtensionProperties`; four test threads were initializing
Vulkan instances concurrently. This locates the failure in loader initialization,
but does not establish whether the loader, driver or sandbox interaction caused
it. The same 14 tests passed serially on CPU and desktop Vulkan; the production
helper serializes requests. No user photograph was written, no raw core was
extracted, and no crash-related system setting was changed.

Desktop tests must run separately: GTK initialization belongs to one test
thread. Source `linux/desktop-env.sh` and `linux/core-env.sh`, set an isolated
`NEGS_PROFILE_DIR`, and run each named test with `--release --locked --offline`
and `-- --ignored --test-threads=1`. `film_change_timings` additionally accepts
`NEGS_RENDER_TEST_SOURCE` and writes JSON to `NEGS_RENDER_TEST_REPORT`.
Set `SPEKTRAFILM_BACKEND=cpu` to repeat against CPU fallback.

The cancellation audit now waits for the preceding export’s preview-space render
before testing the next active export job; clicking Cancel before deferred output
starts does not exercise job cancellation.

Package CI for implementation commit `a6f4337` completed successfully:
[Linux workflow 42058673345](https://github.com/noahhaasis/negs/actions/runs/42058673345).
Arch built and tested both Cargo workspaces from the committed archive and
verified the package and per-user OpenBLAS bundle. Flatpak built offline and
verified inside its sandbox. AppImage built on Ubuntu 24.04 and passed clean
Ubuntu, Fedora and Arch checks. The source archive contains the shared tooltip
parity reference, fixing the earlier Arch packaging failure.

A local native bundle staged from the clean `a6f4337` checkout also passes the
packaged-binary/desktop/OpenBLAS verification and 40 CPU protocol/export requests
(all 62 profiles, f64 export and retained Fast export). Its source archive and
staged bundle are under `target/linux-validation/macos-nov5/local-package-clean/`.
No installed app, real user profile, existing worktree, unrelated `linux/run.sh`
mode change or highlights/shadows draft is replaced by this port.
