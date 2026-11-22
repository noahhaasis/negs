#!/usr/bin/env python3
"""Builds and tests Negs' shared app core (core/, Odin).

    python3 core/build.py lib               # host arch -> target/negs-core/
    python3 core/build.py lib --universal   # macOS arm64 + x86_64, lipo'd
    python3 core/build.py lib --track       # counts allocations (tests)
    python3 core/build.py test              # header, unit tests, leak check, layout
    python3 core/build.py diff              # Rust wrappers + differential tests (tracked)
    python3 core/build.py diff --seeds 50   # ... the library's state and lockstep at 50 seeds
    python3 core/build.py test --asan       # ... under AddressSanitizer (macOS: Homebrew LLVM)
    python3 core/build.py diff --asan       # ... against an ASan-instrumented core
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
# Odin wants an executable's extension on Windows.
EXE = ".exe" if WINDOWS else ""
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


def msvc_lib_exe() -> str:
    """lib.exe of the newest Visual Studio (vswhere), or the one on PATH."""
    found = shutil.which("lib.exe")
    if found:
        return found
    vswhere = Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")) / "Microsoft Visual Studio" / "Installer" / "vswhere.exe"
    if vswhere.is_file():
        out = subprocess.run(
            [str(vswhere), "-latest", "-products", "*", "-find", r"VC\Tools\MSVC\**\bin\Hostx64\x64\lib.exe"],
            capture_output=True, text=True,
        ).stdout.split()
        if out:
            return out[-1]
    sys.exit("[core] lib.exe not found: install Visual Studio's C++ build tools")


def asan_runtime() -> Path | None:
    """The folder holding the ASan runtime to link Odin's instrumented code
    with on macOS. Apple clang's runtime lacks a check Odin's LLVM emits, so
    it is Homebrew LLVM's (`brew install llvm`), newest first. None elsewhere
    (Linux's compiler brings its own)."""
    if WINDOWS:
        sys.exit("[core] --asan: not supported on Windows")
    if not DARWIN:
        return None
    found = sorted(Path("/opt/homebrew/opt").glob("llvm*/lib/clang/*/lib/darwin/libclang_rt.asan_osx_dynamic.dylib"),
                   key=lambda p: int(p.parts[-4]) if p.parts[-4].isdigit() else 0)
    if not found:
        sys.exit("[core] --asan needs Homebrew LLVM's ASan runtime: brew install llvm")
    return found[-1].parent


def asan_link_flags() -> list[str]:
    folder = asan_runtime()
    if folder is None:
        return []
    return [f"-extra-linker-flags:{folder / 'libclang_rt.asan_osx_dynamic.dylib'} -Wl,-rpath,{folder}"]


def odin_lib(odin: str, target: str, out: Path, track: bool, debug: bool, asan: bool = False, whole: bool = False) -> float:
    """Builds the static library for one target; returns the seconds it took.

    Packages are compiled as separate LLVM modules (`-use-separate-modules`):
    ~1.6 s instead of ~7 s, the marks still bit for bit. `whole` builds one
    module, for comparing."""
    out.parent.mkdir(parents=True, exist_ok=True)
    windows = target.startswith("windows")
    with tempfile.TemporaryDirectory() as tmp:
        # On Windows, Odin's own static libraries take in every library its
        # packages import, the static CRT (libucrt.lib) among them, which
        # clashes with the CRT the host links (Rust: the DLL one). So Odin
        # writes an object and lib.exe archives only that; the CRT and the
        # system libraries come from the host's link (negs-core-sys).
        cmd = [odin, "build", str(CORE / "src" / "abi"), "-no-entry-point", f"-target:{target}", "-vet", "-strict-style"]
        # Always into a fresh folder, then moved: with -use-separate-modules,
        # Odin adding to an archive that already exists keeps the old
        # members' code (a rebuilt library still said ABI 3).
        fresh = Path(tmp) / out.name
        if windows:
            cmd += ["-build-mode:obj", f"-out:{Path(tmp) / 'negs_core.obj'}"]
        else:
            cmd += ["-build-mode:static", f"-out:{fresh}"]
        cmd += ["-debug", "-o:none"] if debug or asan else ["-o:speed"]
        if not whole:
            cmd.append("-use-separate-modules")
        if asan:
            cmd.append("-sanitize:address")
        if target.startswith("darwin"):
            cmd.append(f"-minimum-os-version:{MACOS_MIN}")
        if target.startswith("linux"):
            cmd.append("-reloc-mode:pic")
        if track:
            cmd.append("-define:NEGS_CORE_TRACK=true")
        start = time.perf_counter()
        run(cmd)
        if windows:
            objects = sorted(str(p) for p in Path(tmp).glob("*.obj"))
            if not objects:
                sys.exit("[core] odin wrote no object")
            run([msvc_lib_exe(), "/NOLOGO", f"/OUT:{fresh}", *objects])
        out.unlink(missing_ok=True)
        shutil.move(fresh, out)
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
                total += odin_lib(odin, target, piece, args.track, args.debug, whole=args.whole_program)
                slices.append(str(piece))
            out.parent.mkdir(parents=True, exist_ok=True)
            run(["lipo", "-create", "-output", str(out), *slices])
        log(f"built {out} (arm64 + x86_64) in {total:.2f} s")
        return out
    target = args.target or host_target()
    seconds = odin_lib(odin, target, out, args.track, args.debug, args.asan, args.whole_program)
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


def test_packages() -> list[Path]:
    """core/tests and every folder under it holding .odin files, but the
    two that fail on purpose."""
    found = []
    for folder in sorted([CORE / "tests", *(p for p in (CORE / "tests").rglob("*") if p.is_dir())]):
        if folder.name in ("leakcheck", "asancheck"):
            continue
        if any(folder.glob("*.odin")):
            found.append(folder)
    return found


def odin_tests(odin: str, asan: bool = False) -> None:
    flags = ["-debug", "-vet", "-strict-style", "-define:ODIN_TEST_FAIL_ON_BAD_MEMORY=true", "-define:ODIN_TEST_FANCY=false"]
    if asan:
        flags += ["-sanitize:address", *asan_link_flags()]
    with tempfile.TemporaryDirectory() as tmp:
        start = time.perf_counter()
        for package in test_packages():
            name = "_".join(package.relative_to(CORE).parts)
            run([odin, "test", str(package), *flags, f"-out:{Path(tmp) / (name + EXE)}"])
        log(f"unit tests passed in {time.perf_counter() - start:.2f} s (tracking allocator, leaks fail{', ASan' if asan else ''})")
        leak = subprocess.run(
            [odin, "test", str(CORE / "tests" / "leakcheck"), *flags, f"-out:{Path(tmp) / ('leakcheck' + EXE)}"],
            capture_output=True, text=True,
        )
        text = leak.stdout + leak.stderr
        if leak.returncode == 0 or "leak" not in text.lower():
            sys.stderr.write(text)
            sys.exit("[core] the deliberate leak in tests/leakcheck was not caught")
        log("the deliberate leak in tests/leakcheck was caught")
        if asan:
            probe = subprocess.run(
                [odin, "test", str(CORE / "tests" / "asancheck"), *flags, f"-out:{Path(tmp) / ('asancheck' + EXE)}"],
                capture_output=True, text=True,
            )
            text = probe.stdout + probe.stderr
            if probe.returncode == 0 or "AddressSanitizer" not in text:
                sys.stderr.write(text)
                sys.exit("[core] the deliberate use after free in tests/asancheck was not caught")
            log("the deliberate use after free in tests/asancheck was caught")


def layout_check(odin: str) -> None:
    cc = c_compiler()
    if cc is None or WINDOWS:
        # On Windows the Rust side checks the same table (negs-core-sys's
        # and the wrappers' layout tests, run by `build.py diff`).
        log("layout check needs clang or cc on macOS or Linux (skipped)")
        return
    with tempfile.TemporaryDirectory() as tmp:
        lib = Path(tmp) / LIB_NAME
        odin_lib(odin, host_target(), lib, track=True, debug=False)
        exe = Path(tmp) / "layout_check"
        cmd = [cc, "-std=c99", "-Wall", "-Wextra", "-Werror", f"-I{CORE / 'include'}",
               *sorted(str(p) for p in (CORE / "tests" / "c").glob("*.c")), str(lib), "-o", str(exe)]
        if DARWIN:
            cmd.append(f"-mmacosx-version-min={MACOS_MIN}")
        else:
            cmd.append("-lm")
        run(cmd)
        run([str(exe)])


def test(args: argparse.Namespace) -> None:
    odin = find_odin()
    check_header()
    odin_tests(odin, args.asan)
    layout_check(odin)
    log("all core tests passed")


# The Rust crates over the core, each its own workspace (own Cargo.lock),
# then the differential tests against the Rust oracles (core/diff).
RUST_CRATES = ["rust/sys", "rust/marks", "rust/library", "rust/rules", "diff"]


def diff(args: argparse.Namespace) -> None:
    """Every crate's tests with the core's allocations counted (--features
    track), so the differential tests also check nothing leaks."""
    target = REPO / "target" / "core-diff"
    if args.asan:
        # An instrumented, tracked core in its own folder; negs-core-sys links
        # it instead of building its own (NEGS_CORE_LIB_DIR).
        lib = REPO / "target" / "negs-core-asan" / LIB_NAME
        seconds = odin_lib(find_odin(), host_target(), lib, track=True, debug=True, asan=True)
        log(f"built {lib} (ASan) in {seconds:.2f} s")
        os.environ["NEGS_CORE_LIB_DIR"] = str(lib.parent)
        os.environ.setdefault("ASAN_OPTIONS", "detect_leaks=0:abort_on_error=1")
        folder = asan_runtime()
        if folder is not None:
            flags = f"-C link-arg={folder / 'libclang_rt.asan_osx_dynamic.dylib'} -C link-arg=-Wl,-rpath,{folder}"
            os.environ["RUSTFLAGS"] = (os.environ.get("RUSTFLAGS", "") + " " + flags).strip()
        else:
            os.environ["RUSTFLAGS"] = (os.environ.get("RUSTFLAGS", "") + " -C link-arg=-fsanitize=address").strip()
        target = REPO / "target" / "core-asan"
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
    lib.add_argument("--asan", action="store_true", help="instrument with AddressSanitizer (implies -debug -o:none)")
    lib.add_argument("--whole-program", action="store_true", help="one LLVM module, not -use-separate-modules")
    lib.set_defaults(func=build_lib)
    t = sub.add_parser("test", help="header check, unit tests, leak check, layout check")
    t.add_argument("--asan", action="store_true", help="run the unit tests under AddressSanitizer")
    t.set_defaults(func=test)
    d = sub.add_parser("diff", aliases=["rust"], help="Rust wrappers' tests and the differential tests, allocations tracked")
    d.add_argument("--test", help="only this test of core/diff (e.g. marks, theme, lockstep, state)")
    d.add_argument("--seeds", type=int, help="NEGS_LIBRARY_SEEDS for the library's state and lockstep tests")
    d.add_argument("--asan", action="store_true", help="against an ASan-instrumented core (target/core-asan)")
    d.set_defaults(func=diff)
    sub.add_parser("header", help="check negs_core.h is valid C99").set_defaults(func=lambda _a: check_header())
    sub.add_parser("odin", help="print the odin used").set_defaults(func=lambda _a: print(find_odin()))
    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
