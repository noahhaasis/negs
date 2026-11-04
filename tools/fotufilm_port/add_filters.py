"""Solve neutral print filters for test films and add them to a *test* data dir's filter table.

The table in the test dir is a symlink into the repository's data/; it is replaced by a real copy
first so the repository's table is never modified.
"""
import json, os, subprocess, sys
from pathlib import Path
data_dir, paper, films = Path(sys.argv[1]), sys.argv[2], sys.argv[3:]
table = data_dir / "filters/neutral_print_filters.json"
if table.is_symlink():
    content = table.read_text(); table.unlink(); table.write_text(content)
db = json.loads(table.read_text())
env = dict(os.environ, LD_LIBRARY_PATH="/home/noah/Work/Negs/target/linux-toolchain/sysroot/usr/lib")
out = subprocess.run(["/home/noah/Work/Negs-fotufilm/target/release/solve_neutral_filters", str(data_dir), paper, *films],
                     check=True, capture_output=True, text=True, env=env).stdout
for line in out.splitlines():
    r = json.loads(line)
    if "cmy" in r:
        db.setdefault(paper, {}).setdefault("TH-KG3", {})[r["film"]] = r["cmy"]
    print(line)
table.write_text(json.dumps(db, indent=1))
