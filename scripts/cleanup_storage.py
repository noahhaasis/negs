#!/usr/bin/env python3
"""Preview or remove known disposable outputs in the current checkout.

Run with builds stopped. Defaults to a dry run; --apply deletes the listed paths.
Other worktrees, release packages, RAW inputs, source, logs and measurements
are left alone. --debug also removes the complete Cargo development build.
--sweep [DAYS] also runs cargo-sweep on target/, deleting Cargo artifacts that no
build has used for DAYS days (default 1). Cargo keeps every stale copy otherwise.
"""

import argparse
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]


def safe_path(root, path):
    """Never traverse a symlink outside the checkout's generated directories."""
    current = root
    for part in path.relative_to(root).parts:
        current = current / part
        if current.is_symlink():
            return False
    return True


def candidates(root, debug=False):
    target = root / "target"
    if not safe_path(root, target):
        return []
    paths = [target / "debug" if debug else target / "debug/incremental"]
    package = target / "macos-app"
    if safe_path(root, package):
        for pattern in ("verify.*", "dmg.*", "package.*"):
            paths.extend(p for p in package.glob(pattern) if p.is_dir())
    for name in ("memory-study", "speed-study"):
        study = target / name
        if safe_path(root, study):
            for pattern in ("*.tif", "*.tiff"):
                paths.extend(p for p in study.glob(pattern) if p.is_file())
    return sorted(p for p in paths if safe_path(root, p) and p.exists())


def allocated_bytes(path):
    """Count allocated space once per inode, without following symlinks."""
    seen = set()

    def size(p):
        info = p.lstat()
        inode = (info.st_dev, info.st_ino)
        if inode in seen:
            return 0
        seen.add(inode)
        return info.st_blocks * 512 if hasattr(info, "st_blocks") else info.st_size

    total = size(path)
    if path.is_dir():
        for base, dirs, files in os.walk(path, followlinks=False):
            total += sum(size(Path(base) / name) for name in dirs + files)
    return total


def has_cargo_sweep():
    """Ask Cargo, which also finds subcommands in ~/.cargo/bin when it is not on PATH."""
    try:
        result = subprocess.run(["cargo", "sweep", "--version"], capture_output=True)
    except FileNotFoundError:
        return False
    return result.returncode == 0


def sweep(root, days, apply):
    """Run cargo-sweep on the checkout's target directory, a dry run unless apply."""
    if not has_cargo_sweep():
        print("cargo-sweep is not installed: cargo install cargo-sweep")
        return
    if not safe_path(root, root / "target"):
        print("Skipping cargo-sweep: target/ is a symlink.")
        return
    command = ["cargo", "sweep", "--time", str(days)]
    if not apply:
        command.append("--dry-run")
    sys.stdout.flush()  # keep this script's report ahead of cargo-sweep's
    subprocess.run(command + [str(root)], check=True)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true", help="delete the listed disposable outputs")
    parser.add_argument("--debug", action="store_true",
                        help="remove all target/debug outputs instead of just incremental caches")
    parser.add_argument("--sweep", type=int, nargs="?", const=1, metavar="DAYS",
                        help="also remove Cargo artifacts unused for DAYS days (default 1)")
    args = parser.parse_args(argv)
    clean_paths(args)
    if args.sweep is not None:
        sweep(ROOT, args.sweep, args.apply)


def clean_paths(args):
    paths = candidates(ROOT, args.debug)
    total = 0
    for path in paths:
        amount = allocated_bytes(path)
        total += amount
        print(f"{amount / 2**30:8.3f} GiB  {path.relative_to(ROOT)}")
    print(f"{'Removing' if args.apply else 'Dry run:'} {total / 2**30:.3f} GiB in {len(paths)} paths.")
    if not paths:
        return
    if not args.apply:
        print("Stop builds, then add --apply to delete these outputs.")
        return
    for path in paths:
        # Check again immediately before deletion; rmtree never follows child symlinks.
        if not safe_path(ROOT, path):
            raise RuntimeError(f"refusing symlinked cleanup path: {path}")
        if stat.S_ISDIR(path.lstat().st_mode):
            shutil.rmtree(path)
        else:
            path.unlink()
    print("Cleanup complete.")


if __name__ == "__main__":
    main()
