#!/usr/bin/env python3
"""Cold-page-cache timings: drop caches before every run. grep, ugrep, rg, fork, rawgrep (+no-cache)."""
import os, shlex, statistics, subprocess, sys, time
H = os.path.expanduser("~/work/ripgrep-fork-data/fiveway")
MNT = f"{H}/mnt"; CACHE = f"{H}/rgcache"
T = {"grep": "/usr/bin/grep", "ugrep": f"{H}/ugrep/src/ugrep", "rg": "/usr/bin/rg",
     "fork": os.path.expanduser("~/work/ripgrep-fork/target-pgo/release-lto/rg"),
     "rawgrep": f"{H}/rawgrep/target/release-fast/rawgrep"}
N = int(os.environ.get("RUNS", "3"))
def drop(): subprocess.run(["sudo", "sh", "-c", "sync; echo 3 > /proc/sys/vm/drop_caches"], check=True)
def timed(argv, cwd):
    drop(); time.sleep(0.5)
    t = time.perf_counter()
    p = subprocess.run(" ".join(map(shlex.quote, argv)) + " | wc -l", shell=True, cwd=cwd, capture_output=True, text=True)
    return time.perf_counter() - t, int(p.stdout.strip() or 0)
def scen(title, d, pat, g=(), u=(), r=(), w=(), sub="."):
    cwd = f"{MNT}/{d}"
    spec = {"grep": ["-rI", *g, "--", pat, sub], "ugrep": ["-rI", *u, "--", pat, sub],
            "rg": [*r, "--", pat, sub], "fork": [*r, "--", pat, sub],
            "rawgrep": ["--cache-dir", CACHE, "-j", "-N", *w, pat, sub],
            "rawgrep+nc": ["--no-cache", "-j", "-N", *w, pat, sub]}
    # rawgrep with its fragment cache already built (warm it once, then measure cold page cache)
    subprocess.run([T["rawgrep"], "--cache-dir", CACHE, "-j", "-N", *w, pat, sub], cwd=cwd, capture_output=True)
    print(f"\n### COLD {title}\n{'tool':<12}{'median s':>10}   runs   lines", flush=True)
    for name, args in spec.items():
        ts = []; l = 0
        for _ in range(N):
            t, l = timed([T[name.split('+')[0]]] + args, cwd); ts.append(t)
        print(f"{name:<12}{statistics.median(ts):>10.2f}   {['%.2f' % x for x in ts]}   {l}", flush=True)
scen("kernel 96k files: TODO", "linux", "TODO")
scen("kernel: regex [A-Z]+_SUSPEND", "linux", "[A-Z]+_SUSPEND", g=["-E"], u=["-E"])
scen("usr-share 110k files: rare", "usr-share", "zzqx_not_here_9")
scen("usr-include 6k files: struct", "usr-include", "struct")
scen("python lib 8k files: def __init__", "py", "def __init__")
