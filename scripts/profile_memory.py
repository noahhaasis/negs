#!/usr/bin/env python3
"""macOS/Linux: profile one command; retain timestamped output and sampled RSS.

Usage: python3 scripts/profile_memory.py PREFIX COMMAND [ARGS...]
Writes PREFIX.log, PREFIX.rss.csv, PREFIX.summary.json. No extra dependencies.
RSS samples cover this process only, not its children or all GPU allocations.
Peak RSS comes from wait4 (bytes on macOS, KiB on Linux).
"""
import csv
import ctypes
import json
import os
from pathlib import Path
import subprocess
import sys
import threading
import time

prefix, *command = sys.argv[1:]
Path(prefix).parent.mkdir(parents=True, exist_ok=True)
start = time.monotonic()
process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1)
stop = threading.Event()
footprints = []
if sys.platform == 'darwin':
    # SDK sys/resource.h: rusage_info_v2; libproc reports the footprint
    # charged to the process, including compressed memory, unlike RSS.
    class RUsageInfoV2(ctypes.Structure):
        _fields_ = [('uuid', ctypes.c_uint8 * 16)] + [(name, ctypes.c_uint64) for name in (
            'user', 'system', 'idle_wakeups', 'interrupt_wakeups', 'pageins',
            'wired', 'resident', 'footprint', 'start', 'exit', 'child_user',
            'child_system', 'child_idle_wakeups', 'child_interrupt_wakeups',
            'child_pageins', 'child_elapsed', 'diskread', 'diskwritten')]
    libproc = ctypes.CDLL('/usr/lib/libproc.dylib')
    libproc.proc_pid_rusage.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_void_p]
    libproc.proc_pid_rusage.restype = ctypes.c_int

def sample():
    with open(prefix + '.rss.csv', 'w') as out:
        writer = csv.writer(out)
        writer.writerow(['seconds', 'rss_bytes', 'footprint_bytes'])
        while not stop.is_set():
            rss, footprint = None, None
            if sys.platform == 'darwin':
                info = RUsageInfoV2()
                if libproc.proc_pid_rusage(process.pid, 2, ctypes.byref(info)) == 0:
                    rss, footprint = info.resident, info.footprint
                    footprints.append(footprint)
            else:
                p = subprocess.run(['ps', '-o', 'rss=', '-p', str(process.pid)], capture_output=True, text=True)
                if p.returncode == 0 and p.stdout.strip():
                    rss = int(p.stdout.strip()) * 1024
            if rss is not None:
                writer.writerow([round(time.monotonic() - start, 3), rss, footprint])
                out.flush()
            stop.wait(0.1)

thread = threading.Thread(target=sample)
thread.start()
with open(prefix + '.log', 'w') as out:
    for line in process.stdout:
        out.write(f'{time.monotonic() - start:.3f} {line}')
        out.flush()
_, status, usage = os.wait4(process.pid, 0)
process.returncode = os.waitstatus_to_exitcode(status)
stop.set()
thread.join()
summary = dict(command=command, exit_code=process.returncode,
    wall_seconds=time.monotonic()-start, user_seconds=usage.ru_utime,
    system_seconds=usage.ru_stime,
    peak_rss_bytes=usage.ru_maxrss * (1 if sys.platform == 'darwin' else 1024),
    sampled_peak_footprint_bytes=max(footprints, default=None))
Path(prefix + '.summary.json').write_text(json.dumps(summary, indent=2) + '\n')
print(json.dumps(summary, indent=2))
sys.exit(process.returncode)
