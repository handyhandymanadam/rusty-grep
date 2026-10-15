#!/usr/bin/env python3
"""Fork benchmark: median wall/CPU time of commands, output piped to `wc -l`.

Usage: forkbench.py LABEL=CMD [LABEL=CMD ...]   (CMD is a shell command line)
Env: RUNS (default 7), DATA (cwd for the commands).
Output is piped so GNU grep does not take its /dev/null shortcut. One warm-up run
is discarded. Reports the median of RUNS runs plus the line count of the output.
"""
import os, statistics, subprocess, sys, tempfile, time

runs = int(os.environ.get("RUNS", "7"))
cwd = os.environ.get("DATA", os.getcwd())

def once(cmd):
    tf = tempfile.mktemp()
    inner = f"/usr/bin/time -f '%U %S %M' -o {tf} sh -c {subprocess.list2cmdline([cmd])!r} 2>/dev/null | wc -l"
    t = time.perf_counter()
    p = subprocess.run(["sh", "-c", f"/usr/bin/time -f '%U %S %M' -o {tf} sh -c \"$1\" 2>/dev/null | wc -l", "x", cmd],
                       cwd=cwd, capture_output=True, text=True)
    wall = (time.perf_counter() - t) * 1000
    u, s, m = open(tf).read().split()[-3:]
    os.unlink(tf)
    return wall, (float(u) + float(s)) * 1000, int(m) / 1024, int(p.stdout.strip() or 0)

print(f"{'label':<34}{'wall ms':>9}{'cpu ms':>9}{'RSS MB':>8}{'lines':>10}")
for arg in sys.argv[1:]:
    label, cmd = arg.split("=", 1)
    once(cmd)
    rs = [once(cmd) for _ in range(runs)]
    print(f"{label:<34}{statistics.median(r[0] for r in rs):>9.0f}{statistics.median(r[1] for r in rs):>9.0f}"
          f"{max(r[2] for r in rs):>8.0f}{rs[-1][3]:>10}", flush=True)
