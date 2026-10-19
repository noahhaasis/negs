#!/usr/bin/env python3
"""Generate and verify a signed feed. Does not publish anything. Python 3.11+."""
import argparse
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import urllib.error
import urllib.request
import xml.etree.ElementTree as ET
import zipfile

from macos_release import ROOT, metadata

SPARKLE = "http://www.andymatuschak.org/xml-namespaces/sparkle"


def check_previous_feed(data, build):
    tree = ET.fromstring(data)
    versions = [item.findtext(f"{{{SPARKLE}}}version") or item.find("enclosure").get(f"{{{SPARKLE}}}version")
                for item in tree.findall("./channel/item")]
    if any(not value or not value.isdecimal() or int(value) >= int(build) for value in versions):
        raise ValueError("Build number must be newer than every published build; never replace a released build")


def check_archive_configuration(archive, meta):
    with zipfile.ZipFile(archive) as zipped:
        info = plistlib.loads(zipped.read("Spektrafilm.app/Contents/Info.plist"))
    expected = dict(CFBundleShortVersionString=meta["version"], CFBundleVersion=meta["build"],
                    SUFeedURL=meta["feed"], SUPublicEDKey=meta["public_key"],
                    SURequireSignedFeed=True, SUVerifyUpdateBeforeExtraction=True)
    if any(info.get(key) != value for key, value in expected.items()):
        raise ValueError("Update archive metadata/key differs from the release configuration")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tools", type=Path, required=True)
    parser.add_argument("--artifacts", type=Path, default=Path("dist/releases"))
    parser.add_argument("--site", type=Path, default=Path("dist/update-site"))
    parser.add_argument("--previous-feed", type=Path, help="Use a local feed for offline release rehearsals")
    args = parser.parse_args()
    meta = metadata()
    if not meta["repository"] or not meta["feed"]:
        raise ValueError("Release repository and updater configuration are required")
    key = os.environ.get("SPARKLE_PRIVATE_KEY", "").strip()
    if not key:
        raise ValueError("SPARKLE_PRIVATE_KEY is required")
    # Pass secrets through stdin, never argv or a file in the source/artifact tree.
    def signed_tool(name, *arguments):
        subprocess.run([str(args.tools.resolve() / name), "--ed-key-file", "-", *map(str, arguments)],
                       input=key + "\n", text=True, check=True)

    work = args.artifacts.parent / "appcast-work"
    work.mkdir(parents=True, exist_ok=False)
    feed = work / "appcast.xml"
    if args.previous_feed:
        previous = args.previous_feed.read_bytes()
    else:
        try:
            with urllib.request.urlopen(meta["feed"], timeout=30) as response:
                previous = response.read(4 * 1024 * 1024)
        except urllib.error.HTTPError as error:
            if error.code != 404 or meta["build"] != "1":
                raise
            previous = None
    if previous:
        feed.write_bytes(previous)
        signed_tool("sign_update", "--verify", feed)
        check_previous_feed(previous, meta["build"])
    archive = work / f"{meta['name']}.zip"
    shutil.copy2(args.artifacts / archive.name, archive)
    check_archive_configuration(archive, meta)
    shutil.copy2(ROOT / f"release/notes/{meta['version']}.md", archive.with_suffix(".md"))
    signed_tool("generate_appcast", "--maximum-deltas", "0", "--maximum-versions", "0",
                "--embed-release-notes", "--download-url-prefix",
                f"https://github.com/{meta['repository']}/releases/download/{meta['tag']}/",
                "--link", f"https://github.com/{meta['repository']}/releases", work)
    signed_tool("sign_update", "--verify", feed)
    items = ET.parse(feed).findall("./channel/item")
    item = next(item for item in items if item.findtext(f"{{{SPARKLE}}}version") == meta["build"])
    enclosure = item.find("enclosure")
    signature = enclosure.attrib[f"{{{SPARKLE}}}edSignature"]
    subprocess.run(["swift", str(ROOT / "scripts/verify_update.swift"), meta["public_key"], signature, str(archive)], check=True)
    args.site.mkdir(parents=True, exist_ok=True)
    shutil.copy2(feed, args.site / "appcast.xml")
    # Keep a matching feed snapshot with each immutable release for diagnosis.
    shutil.copy2(feed, args.artifacts / "appcast.xml")
    (args.site / "index.html").write_text("<!doctype html><title>Spektrafilm updates</title><p>Spektrafilm update feed.</p>\n")
    print(f"Prepared verified feed in {args.site}; publish release assets before deploying it.")


if __name__ == "__main__":
    main()
