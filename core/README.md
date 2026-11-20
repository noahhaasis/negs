# core/: Negs' shared app core, in Odin

The non-UI app logic the macOS, Windows and Linux apps share, behind one C
ABI (`include/negs_core.h`). The film engine stays in Rust (`crates/`);
the core never touches it. Plan, rules and progress:
[shared-core-plan.md](../docs/shared-core-plan.md),
[shared-core-progress.md](../docs/shared-core-progress.md).

```
python3 core/install_odin.py      # the pinned Odin (ODIN_VERSION) into ~/.negs/odin/<tag>
python3 core/build.py lib         # target/negs-core/libnegs_core.a (negs_core.lib on Windows)
python3 core/build.py lib --universal   # macOS arm64 + x86_64
python3 core/build.py test        # C99 header, unit tests (leaks fail), leak self-check, layout
python3 core/build.py diff        # Rust wrappers + differential tests against the Rust oracles
```

## Modules

| Module | Replaces | Gate |
| --- | --- | --- |
| `marks` | `windows/marks`, HandStar/RejectCross/PickBox/PencilStroke/PencilRing (Swift) | `diff/tests/marks.rs`: 10,000 seeded cases, bit for bit on Apple silicon (1e-12 elsewhere); Swift paths identical |
| `theme` | `AppTheme.swift` tables and Theme maths; `windows/src/ui/theme.rs` tables (Phase 3) | `diff/tests/theme.rs`: Windows' tables read from source, colour maths bit for bit, parse fuzzed |

## Layout

| Path | What |
| --- | --- |
| `include/negs_core.h` | The C99 ABI: versioned, explicit widths, owned blocks freed by the core |
| `src/abi/` | Package `negs_core`, the library's root: every `@(export)` and the context they run in |
| `src/<module>/` | One package per ported module, free of the ABI (plain Odin types) |
| `tests/` | `odin test` unit tests; each test runs under `mem.Tracking_Allocator` and fails on a leak |
| `tests/leakcheck/` | Leaks on purpose; `build.py test` fails unless the leak is caught |
| `tests/c/` | The C side's layout and smoke check |
| `diff/` | Rust differential tests and benchmarks against the Rust oracles |

## Rules

- Pure Odin and `core:*`; libc only for maths that must match Rust and
  Swift bit for bit (`libc.sin`, `libc.hypot`, ...: both of them call the
  platform's libm).
- Every export starts with `context = core_context()`; results the host
  owns are one block in the core's allocator, freed through the ABI.
- `build.py lib --track` counts allocations (`negs_core_alloc_stats`), so the
  differential tests can check that nothing leaks across the ABI.
- Bounds checks stay on until Phase 7 measures where turning them off pays.
