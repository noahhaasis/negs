"""Extract and audit a macOS release, without relying on the build directory.

python3 scripts/verify_macos_package.py --zip dist/example.zip --output-dir /tmp/example-check
"""

import argparse
import hashlib
import json
from pathlib import Path
import plistlib
import re
import subprocess
import tarfile

from macos_release import metadata


ROOT = Path(__file__).resolve().parents[1]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def checked(*args):
    return subprocess.run(args, check=True, text=True, capture_output=True).stdout


def require(condition, message):
    if not condition:
        raise ValueError(message)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--zip", required=True, type=Path)
    parser.add_argument("--source", type=Path, help="Matching source release asset")
    parser.add_argument("--output-dir", required=True, type=Path)
    args = parser.parse_args()
    out = args.output_dir.resolve()
    out.mkdir(parents=True, exist_ok=False)
    checked("/usr/bin/ditto", "-x", "-k", str(args.zip.resolve()), str(out))
    apps = list(out.glob("*.app")) + list(out.glob("*/*.app"))
    require(len(apps) == 1, "expected exactly one app bundle")
    app = apps[0]
    checked("/usr/bin/codesign", "--verify", "--deep", "--strict", "--verbose=2", str(app))
    checked("/usr/bin/plutil", "-lint", str(app / "Contents/Info.plist"))
    plist = plistlib.loads((app / "Contents/Info.plist").read_bytes())
    expected = metadata()
    require(plist["CFBundleShortVersionString"] == expected["version"], "app version differs from Cargo")
    require(plist["CFBundleVersion"] == expected["build"], "app build number differs from release metadata")
    report = {"zip_sha256": sha(args.zip), "app": str(app), "signature_verified": True,
              "minimum_macos": plist["LSMinimumSystemVersion"], "binaries": {}, "data": {}}
    for name in [plist["CFBundleExecutable"], "spektrafilm", "spektrafilm-f64"] + (["spektrafilm-native"] if plist["CFBundleExecutable"] in {"Negs", "NegsApp"} else []):
        binary = app / "Contents/MacOS" / name
        arch = checked("/usr/bin/lipo", "-archs", str(binary)).strip()
        links = checked("/usr/bin/otool", "-L", str(binary))
        # Universal binaries list each slice under its own "(architecture …)" header.
        dependencies = list(dict.fromkeys(line.strip().split(" (", 1)[0]
                                          for line in links.splitlines() if line.startswith("\t")))
        bundled_sparkle = "@rpath/Sparkle.framework/Versions/B/Sparkle"
        require(all(p.startswith(("/System/Library/", "/usr/lib/")) or
                    (name == plist["CFBundleExecutable"] and p == bundled_sparkle) for p in dependencies),
                f"{name} has a non-system dynamic dependency: {dependencies}")
        build = checked("/usr/bin/xcrun", "vtool", "-show-build", str(binary))
        minimum = max(re.findall(r"\bminos\s+([\d.]+)", build), key=lambda v: tuple(map(int, v.split("."))))
        require(tuple(map(int, minimum.split("."))) <= tuple(map(int, plist["LSMinimumSystemVersion"].split("."))),
                f"{name} requires a newer OS than Info.plist advertises")
        report["binaries"][name] = {"sha256": sha(binary), "architecture": arch,
                                     "minimum_macos": minimum, "dependencies": dependencies}
        require(set(arch.split()) == {"arm64", "x86_64"}, f"Not a universal binary: {name}: {arch}")
    framework = app / "Contents/Frameworks/Sparkle.framework"
    require(framework.is_dir(), "Sparkle framework missing")
    checked("/usr/bin/codesign", "--verify", "--deep", "--strict", str(framework))
    require((framework / "Versions/B/Autoupdate").is_file(), "Sparkle installer missing")
    require((framework / "Versions/B/Updater.app").is_dir(), "Sparkle updater UI missing")
    report["version"] = plist["CFBundleShortVersionString"]
    report["build"] = plist["CFBundleVersion"]
    data = app / "Contents/Resources/data"
    expected_paths = {p.relative_to(ROOT / "data") for p in (ROOT / "data").rglob("*") if p.is_file()}
    actual_paths = {p.relative_to(data) for p in data.rglob("*") if p.is_file()}
    require(expected_paths == actual_paths, "packaged data file set differs from source")
    for relative in sorted(expected_paths):
        digest = sha(data / relative)
        require(digest == sha(ROOT / "data" / relative), f"stale packaged data: {relative}")
        report["data"][str(relative)] = digest
    source = args.source or app.parent / "SOURCE.tar.gz"
    with tarfile.open(source) as tar:
        count = 0
        for member in tar.getmembers():
            if member.isfile():
                require(not Path(member.name).is_absolute() and ".." not in Path(member.name).parts,
                        "invalid source archive path")
                require(hashlib.sha256(tar.extractfile(member).read()).hexdigest() == sha(ROOT / member.name),
                        f"source snapshot mismatch: {member.name}")
                count += 1
    report["source"] = {"sha256": sha(source), "verified_files": count}
    report["passed"] = True
    (out / "package-report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"passed": True, "app": str(app), "source_files": count,
                      "data_files": len(actual_paths), "report": str(out / "package-report.json")}, indent=2))


if __name__ == "__main__":
    main()
