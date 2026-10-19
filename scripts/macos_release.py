#!/usr/bin/env python3
"""Shared macOS release metadata; Python 3.11+. Cargo is the version authority."""
import argparse
import base64
import json
import os
from pathlib import Path
import plistlib
import re
import tomllib
from urllib.parse import urlparse

ROOT = Path(__file__).resolve().parents[1]


def metadata(root=ROOT, env=os.environ):
    version = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise ValueError("Use a numeric major.minor.patch version, including before 1.0")
    build = (root / "release/build-number").read_text().strip()
    if not re.fullmatch(r"[1-9]\d*", build):
        raise ValueError("release/build-number must be a positive, increasing integer")
    tag = env.get("RELEASE_TAG", "")
    if tag and tag != f"v{version}":
        raise ValueError(f"Tag {tag!r} does not match Cargo version v{version}")
    notes = root / f"release/notes/{version}.md"
    if not notes.is_file():
        raise ValueError(f"Missing release notes: {notes}")
    repository = env.get("RELEASE_REPOSITORY", "")
    if repository and not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("RELEASE_REPOSITORY must be owner/repo")
    feed = env.get("UPDATE_FEED_URL", "")
    if not feed and repository:
        owner, name = repository.split("/")
        feed = f"https://{owner.lower()}.github.io/{name}/appcast.xml"
    key = env.get("SPARKLE_PUBLIC_KEY", "")
    if bool(feed) != bool(key):
        raise ValueError("Update feed and SPARKLE_PUBLIC_KEY must be provided together")
    if feed:
        parsed = urlparse(feed)
        if parsed.scheme != "https" or not parsed.hostname or parsed.username or parsed.password:
            raise ValueError("The update feed must be an HTTPS URL without credentials")
        if len(base64.b64decode(key, validate=True)) != 32:
            raise ValueError("SPARKLE_PUBLIC_KEY must be a base64 Ed25519 public key")
    if tag and not feed:
        raise ValueError("Tagged releases must have an update feed and public key")
    architecture = env.get("RELEASE_ARCH", "arm64")
    if architecture != "arm64":
        raise ValueError("The initial macOS release supports arm64 only")
    return dict(version=version, build=build, tag=f"v{version}", repository=repository,
                feed=feed, public_key=key, architecture=architecture,
                name=f"Negs-{version}-macos-{architecture}")


def write_plist(path, meta):
    info = dict(CFBundleDevelopmentRegion="en", CFBundleDisplayName="Negs",
                CFBundleExecutable="NegsApp", CFBundleIconFile="negs.icns",
                CFBundleIdentifier="dev.negs.app", CFBundleInfoDictionaryVersion="6.0",
                CFBundleName="Negs", CFBundlePackageType="APPL",
                CFBundleShortVersionString=meta["version"], CFBundleVersion=meta["build"],
                LSMinimumSystemVersion="14.0", NSHighResolutionCapable=True,
                CFBundleDocumentTypes=[dict(CFBundleTypeName="Photograph", CFBundleTypeRole="Viewer",
                    LSHandlerRank="Alternate", LSItemContentTypes=["public.png", "public.tiff", "public.jpeg", "public.camera-raw-image"])])
    if meta["feed"]:
        info.update(SUFeedURL=meta["feed"], SUPublicEDKey=meta["public_key"],
                    SUEnableAutomaticChecks=True, SUScheduledCheckInterval=21600,
                    SUAutomaticallyUpdate=False, SUAllowsAutomaticUpdates=False,
                    SUEnableSystemProfiling=False, SUVerifyUpdateBeforeExtraction=True,
                    SURequireSignedFeed=True)
    path.write_bytes(plistlib.dumps(info))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plist", type=Path)
    parser.add_argument("--field")
    parser.add_argument("--github-output", type=Path)
    args = parser.parse_args()
    meta = metadata()
    if args.plist:
        write_plist(args.plist, meta)
    if args.github_output:
        with args.github_output.open("a") as out:
            for key, value in meta.items():
                out.write(f"{key}={value}\n")
    if args.field:
        print(meta[args.field])
    else:
        print(json.dumps(meta, indent=2))


if __name__ == "__main__":
    main()
