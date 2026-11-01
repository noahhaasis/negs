#!/usr/bin/env bash
# Build the browser app and publish it to the Negs website repo at try/,
# merging the app's response header rules into the site's root _headers.
# Usage: scripts/deploy-web-site.sh [path-to-negs-site-checkout]
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
site=${1:-../negs-site}
if [[ ! -d $site ]]; then
  echo "Website checkout not found: $site" >&2
  exit 1
fi
staging=$(mktemp -d "${TMPDIR:-/tmp}/negs-web.XXXXXX")
trap 'rm -rf "$staging"' EXIT
bash web/build.sh --threads
node web/package-static.mjs "$staging"
# The app's robots.txt forbids everything; the site's robots.txt governs the
# whole host, including /try.
rm "$staging/robots.txt"
rsync -a --delete "$staging/" "$site/try/"
python3 - "$staging/_headers" "$site/_headers" <<'PY'
import re, sys, pathlib

def block_selector(block):
    for line in block:
        if line and not line.startswith((" ", "\t")):
            return line
    return None

app_rules = pathlib.Path(sys.argv[1])
blocks = []
block = []
for raw in app_rules.read_text().splitlines():
    if not raw.strip() or raw.lstrip().startswith("#"):
        continue
    if not raw.startswith((" ", "\t")):
        if block:
            blocks.append(block)
            block = []
    if "X-Robots-Tag" in raw:
        continue
    block.append(raw.rstrip())
if block:
    blocks.append(block)
try_blocks = []
for block in blocks:
    selector = block_selector(block)
    scoped = []
    for raw in block:
        if raw and not raw.startswith((" ", "\t")):
            scoped.append("/try/" + selector.lstrip("/") if selector else raw)
        elif raw:
            scoped.append(raw)
    try_blocks.append("\n".join(scoped))
site_file = pathlib.Path(sys.argv[2])
existing = site_file.read_text() if site_file.exists() else ""
existing = re.sub(r"(?m)^/try/.*\n(?:[ \t].*\n)*", "", existing)
existing = existing.rstrip("\n") + "\n" if existing.strip() else ""
merged = existing + "\n".join(try_blocks).rstrip("\n") + "\n"
site_file.write_text(merged)
print(f"{site_file}: {len(try_blocks)} /try/* rule blocks")
PY
echo "Published $site/try/"
