"""Streams the physical footprint of a device's WebKit processes, the number
iOS's per-process memory limit is checked against, to a JSONL file.

    target/ipad-diagnostics/bin/python web/checks/footprint.py <udid> <out.jsonl> [interval ms]

Each line is a sample: { t, pid, name, mib }. WebGPU allocations count
against the WebContent process, not WebKit's GPU process. Stop with Ctrl-C
(or kill); prints each pid's peak when it ends.
"""

import asyncio
import json
import sys
import time

from pymobiledevice3.remote.userspace_tunnel import establish_userspace_rsd
from pymobiledevice3.services.dvt.instruments.dvt_provider import DvtProvider
from pymobiledevice3.services.dvt.instruments.sysmontap import Sysmontap


async def main(udid: str, out: str, interval: int) -> None:
    peaks: dict[tuple[int, str], int] = {}
    # iOS 17 and later serve instruments only through a tunnel; this one
    # needs no root.
    rsd = await establish_userspace_rsd(serial=udid)
    try:
        with open(out, "a") as f:
            async with DvtProvider(rsd) as dvt, await Sysmontap.create(dvt, interval=interval) as sysmon:
                async for snapshot in sysmon.iter_processes():
                    t = time.time()
                    for p in snapshot:
                        name = p.get("name") or ""
                        footprint = p.get("physFootprint")
                        if not name.startswith("com.apple.WebKit") or not isinstance(footprint, int):
                            continue
                        key = (p["pid"], name)
                        peaks[key] = max(peaks.get(key, 0), footprint)
                        f.write(json.dumps({"t": round(t, 3), "pid": p["pid"], "name": name, "mib": round(footprint / 2**20, 1)}) + "\n")
                    f.flush()
    finally:
        for (pid, name), peak in sorted(peaks.items()):
            print(f"{pid} {name} peak {peak / 2**20:.0f} MiB", file=sys.stderr)


if __name__ == "__main__":
    try:
        asyncio.run(main(sys.argv[1], sys.argv[2], int(sys.argv[3]) if len(sys.argv) > 3 else 250))
    except KeyboardInterrupt:
        pass
