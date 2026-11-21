#!/usr/bin/env python3
"""Builds and tests Negs' shared app core (core/, Odin).

    python3 core/build.py lib               # host arch -> target/negs-core/
    python3 core/build.py lib --universal   # macOS arm64 + x86_64, lipo'd
    python3 core/build.py lib --track       # counts allocations (tests)
    python3 core/build.py test              # header, unit tests, leak check, layout
    python3 core/build.py diff              # Rust wrappers + differential tests (tracked)
    python3 core/build.py diff --seeds 50   # ... the library's state and lockstep at 50 seeds
    python3 core/build.py odin              # which odin it uses

Odin is pinned (core/ODIN_VERSION): $NEGS_ODIN, then ~/.negs/odin/<tag>/odin,
then `odin` on PATH, whichever reports the pinned commit.
"""

from __future__ import annotations

import argparse
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

CORE = Path(__file__).resolve().parent
REPO = CORE.parent
TAG, COMMIT = (CORE / "ODIN_VERSION").read_text().split()
WINDOWS = sys.platform == "win32"
DARWIN = sys.platform == "darwin"
LIB_NAME = "negs_core.lib" if WINDOWS else "libnegs_core.a"
# The macOS app's deployment target (Info.plist, MACOSX_DEPLOYMENT_TARGET).
MACOS_MIN = "14.0"


def log(message: str) -> None:
    print(f"[core] {message}", flush=True)


def find_odin() -> str:
    exe = "odin.exe" if WINDOWS else "odin"
    candidates = []
    if os.environ.get("NEGS_ODIN"):
        candidates.append(os.environ["NEGS_ODIN"])
    candidates.append(str(Path.home() / ".negs" / "odin" / TAG / exe))
    on_path = shutil.which("odin")
    if on_path:
        candidates.append(on_path)
    seen = []
    for candidate in candidates:
        if not Path(candidate).is_file():
            continue
        try:
            out = subprocess.run([candidate, "version"], capture_output=True, text=True, check=True).stdout
        except (OSError, subprocess.CalledProcessError):
            continue
        if COMMIT in out:
            return candidate
        seen.append(f"{candidate}: {out.strip()}")
    detail = "; ".join(seen) or "none found"
    sys.exit(
        f"[core] Odin {TAG} ({COMMIT}) not found ({detail}).\n"
        f"Install it: download odin-*-{TAG} from https://github.com/odin-lang/Odin/releases/tag/{TAG}\n"
        f"into ~/.negs/odin/{TAG}/, or set NEGS_ODIN."
    )


def host_target() -> str:
    machine = platform.machine().lower()
    arch = "arm64" if machine in ("arm64", "aarch64") else "amd64"
    if DARWIN:
        return f"darwin_{arch}"
    if WINDOWS:
        return "windows_amd64"
    return f"linux_{arch}"


def run(cmd: list[str], **kwargs) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, check=True, **kwargs)


def odin_lib(odin: str, target: str, out: Path, track: bool, debug: bool) -> float:
    """Builds the static library for one target; returns the seconds it took."""
    out.parent.mkdir(parents=True, exist_ok=True)
    cmd = [
        odin, "build", str(CORE / "src" / "abi"),
        "-build-mode:static", "-no-entry-point", f"-out:{out}", f"-target:{target}", "-vet", "-strict-style",
    ]
    cmd += ["-debug", "-o:none"] if debug else ["-o:speed"]
    if target.startswith("darwin"):
        cmd.append(f"-minimum-os-version:{MACOS_MIN}")
    if target.startswith("linux"):
        cmd.append("-reloc-mode:pic")
    if track:
        cmd.append("-define:NEGS_CORE_TRACK=true")
    start = time.perf_counter()
    run(cmd)
    return time.perf_counter() - start


def build_lib(args: argparse.Namespace) -> Path:
    odin = find_odin()
    out = Path(args.out) if args.out else REPO / "target" / "negs-core" / LIB_NAME
    if args.universal:
        if not DARWIN:
            sys.exit("[core] --universal is for macOS")
        with tempfile.TemporaryDirectory() as tmp:
            slices = []
            total = 0.0
            for target in ("darwin_arm64", "darwin_amd64"):
                piece = Path(tmp) / target / LIB_NAME
                total += odin_lib(odin, target, piece, args.track, args.debug)
                slices.append(str(piece))
            out.parent.mkdir(parents=True, exist_ok=True)
            run(["lipo", "-create", "-output", str(out), *slices])
        log(f"built {out} (arm64 + x86_64) in {total:.2f} s")
        return out
    target = args.target or host_target()
    seconds = odin_lib(odin, target, out, args.track, args.debug)
    log(f"built {out} ({target}) in {seconds:.2f} s")
    return out


def c_compiler() -> str | None:
    for cc in ("clang", "cc", "gcc"):
        if shutil.which(cc):
            return cc
    return None


def check_header() -> None:
    cc = c_compiler()
    if cc is None:
        log("no C compiler: header check skipped")
        return
    header = CORE / "include" / "negs_core.h"
    run([cc, "-std=c99", "-Wall", "-Wextra", "-Werror", "-pedantic", "-fsyntax-only", "-x", "c", str(header)])
    log("negs_core.h is valid C99")


def odin_tests(odin: str) -> None:
    flags = ["-debug", "-vet", "-strict-style", "-define:ODIN_TEST_FAIL_ON_BAD_MEMORY=true", "-define:ODIN_TEST_FANCY=false"]
    with tempfile.TemporaryDirectory() as tmp:
        start = time.perf_counter()
        run([odin, "test", str(CORE / "tests"), *flags, f"-out:{Path(tmp) / 'tests'}"])
        run([odin, "test", str(CORE / "tests" / "library"), *flags, f"-out:{Path(tmp) / 'library'}"])
        log(f"unit tests passed in {time.perf_counter() - start:.2f} s (tracking allocator, leaks fail)")
        leak = subprocess.run(
            [odin, "test", str(CORE / "tests" / "leakcheck"), *flags, f"-out:{Path(tmp) / 'leakcheck'}"],
            capture_output=True, text=True,
        )
        text = leak.stdout + leak.stderr
        if leak.returncode == 0 or "leak" not in text.lower():
            sys.stderr.write(text)
            sys.exit("[core] the deliberate leak in tests/leakcheck was not caught")
        log("the deliberate leak in tests/leakcheck was caught")


def layout_check(odin: str) -> None:
    cc = c_compiler()
    if cc is None or WINDOWS:
        log("layout check needs clang or cc (skipped)")
        return
    with tempfile.TemporaryDirectory() as tmp:
        lib = Path(tmp) / LIB_NAME
        odin_lib(odin, host_target(), lib, track=True, debug=False)
        exe = Path(tmp) / "layout_check"
        cmd = [cc, "-std=c99", "-Wall", "-Wextra", "-Werror", f"-I{CORE / 'include'}",
               str(CORE / "tests" / "c" / "layout_check.c"), str(lib), "-o", str(exe)]
        if DARWIN:
            cmd.append(f"-mmacosx-version-min={MACOS_MIN}")
        else:
            cmd.append("-lm")
        run(cmd)
        run([str(exe)])


def test(_args: argparse.Namespace) -> None:
    odin = find_odin()
    check_header()
    odin_tests(odin)
    layout_check(odin)
    log("all core tests passed")


# The Rust crates over the core, each its own workspace (own Cargo.lock),
# then the differential tests against the Rust oracles (core/diff).
RUST_CRATES = ["rust/sys", "rust/marks", "rust/library", "diff"]


def diff(args: argparse.Namespace) -> None:
    """Every crate's tests with the core's allocations counted (--features
    track), so the differential tests also check nothing leaks."""
    target = REPO / "target" / "core-diff"
    if args.seeds:
        # The library's state and lockstep tests (default 3 seeds; CI and
        # the Phase 2 gate run 50).
        os.environ["NEGS_LIBRARY_SEEDS"] = str(args.seeds)
    for crate in RUST_CRATES:
        cmd = ["cargo", "test", "--release", "--manifest-path", str(CORE / crate / "Cargo.toml"),
               "--target-dir", str(target), "--features", "track"]
        if args.test:
            cmd += ["--test", args.test]
        if crate != "diff" and args.test:
            continue
        log(f"cargo test {crate}")
        started = time.monotonic()
        run(cmd)
        log(f"{crate} passed in {time.monotonic() - started:.1f} s")
    log("all Rust and differential tests passed")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    lib = sub.add_parser("lib", help="build the static library")
    lib.add_argument("--universal", action="store_true", help="macOS arm64 + x86_64")
    lib.add_argument("--track", action="store_true", help="count allocations (negs_core_alloc_stats)")
    lib.add_argument("--debug", action="store_true", help="-debug -o:none")
    lib.add_argument("--target", help="an odin -target, e.g. windows_amd64")
    lib.add_argument("--out", help=f"output path (default target/negs-core/{LIB_NAME})")
    lib.set_defaults(func=build_lib)
    sub.add_parser("test", help="header check, unit tests, leak check, layout check").set_defaults(func=test)
    d = sub.add_parser("diff", aliases=["rust"], help="Rust wrappers' tests and the differential tests, allocations tracked")
    d.add_argument("--test", help="only this test of core/diff (e.g. marks, theme, lockstep, state)")
    d.add_argument("--seeds", type=int, help="NEGS_LIBRARY_SEEDS for the library's state and lockstep tests")
    d.set_defaults(func=diff)
    sub.add_parser("header", help="check negs_core.h is valid C99").set_defaults(func=lambda _a: check_header())
    sub.add_parser("odin", help="print the odin used").set_defaults(func=lambda _a: print(find_odin()))
    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
