#!/usr/bin/env python3
"""Benchmark matrix: workloads (rows) x ripgrep binaries (columns), median wall ms.

Usage: forksuite.py NAME=/path/to/rg [NAME=/path ...]
Env: RUNS (default 5), SUITE (comma list of workload names to run, default all).
Data (not in git), all in ~/work/ripgrep-fork-data: big.txt, w400.txt, tiny.txt, a shallow
clone of the Linux kernel in linux/; plus /usr/share (read-only). Output is piped to `wc -l` (GNU grep
would otherwise stop at the first match when stdout is /dev/null).
"""
import os, statistics, subprocess, sys, time

runs = int(os.environ.get("RUNS", "5"))
home = os.path.expanduser("~")
D = f"{home}/work/ripgrep-fork-data"
W = f"{D}/linux"   # shallow clone of the Linux kernel (96k files), not in git
WORKLOADS = [
    # name, cwd, args
    ("tree literal",   W, "TODO ."),
    ("tree regex",     W, r"'\w+_lock\(' ."),
    ("tree -i",        W, "-i suspend ."),
    ("tree -l",        W, "-l EXPORT_SYMBOL ."),
    ("tree --files",   W, "--files ."),
    ("tree 110k rare", "/usr/share", "zzqx_nope ."),
    ("file literal",   D, "static big.txt"),
    ("file regex",     D, r"'[A-Z]+_[A-Z]+\(' big.txt"),
    ("file -i",        D, "-i return big.txt"),
    ("file -c",        D, "-c include big.txt"),
    ("file -v",        D, "-v e big.txt"),
    ("file print-all", D, "'' big.txt"),
    ("file 400 words", D, "-F -f w400.txt big.txt"),
    ("file -n dense",  D, "-n e big.txt"),
    ("startup tiny",   D, "world tiny.txt"),
]

def once(rg, cwd, args):
    t = time.perf_counter()
    subprocess.run(f"{rg} {args} | wc -l", shell=True, cwd=cwd, capture_output=True)
    return (time.perf_counter() - t) * 1000

def main():
    bins = [a.split("=", 1) for a in sys.argv[1:]]
    only = os.environ.get("SUITE")
    names = only.split(",") if only else None
    print(f"{'workload':<16}" + "".join(f"{n:>14}" for n, _ in bins))
    for name, cwd, args in WORKLOADS:
        if names and name not in names:
            continue
        row = []
        for _, rg in bins:
            once(rg, cwd, args)
            row.append(statistics.median(once(rg, cwd, args) for _ in range(runs)))
        base = row[0]
        cells = "".join(f"{v:>9.0f}{'':>5}" if i == 0 else f"{v:>9.0f} {v/base:>3.2f}x" for i, v in enumerate(row))
        print(f"{name:<16}{cells}", flush=True)

main()
