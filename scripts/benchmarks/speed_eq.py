#!/usr/bin/env python3
"""Speed at EQUAL semantics: rg -uuu -a vs rawgrep -a (cache built, cache off, first run with empty cache); warm and optionally cold."""
import json, os, shlex, statistics, subprocess, sys, time
H = os.path.expanduser("~/work/ripgrep-fork-data/audit")
CORPUS = f"{H}/mnt/linux"; RAW = f"{H}/rawgrep"; CACHE = f"{H}/rgcache_speed"
FORK = os.path.expanduser("~/work/ripgrep-fork/target-pgo/release-lto/rg")
PATS = json.load(open(f"{H}/patterns.json"))
COLD = os.environ.get("COLD") == "1"
picked = []; seen = {}
for p in PATS:
    if p["cat"] in ("literal-common", "literal-rare", "literal-absent", "literal-short", "ignore-case", "alternation-4", "class-repetition", "inner-required", "anchor-start", "word-boundary-w") and seen.get(p["cat"], 0) < 2:
        picked.append(p); seen[p["cat"]] = seen.get(p["cat"], 0) + 1
def drop(): subprocess.run(["sudo", "sh", "-c", "sync; echo 3 > /proc/sys/vm/drop_caches"], check=True)
def t(argv, cold=False):
    if cold: drop(); time.sleep(0.4)
    a = time.perf_counter(); p = subprocess.run(" ".join(map(shlex.quote, argv)) + " | wc -l", shell=True, cwd=CORPUS, capture_output=True, text=True)
    return time.perf_counter() - a, int(p.stdout.strip() or 0)
def med(argv, n, cold=False): xs = [t(argv, cold) for _ in range(n)]; return statistics.median(x[0] for x in xs) * 1000, xs[-1][1]
print(f"{'pattern':<28}{'rg ms':>8}{'fork ms':>9}{'raw(cache)':>12}{'raw(nc)':>9}{'raw 1st run':>13}   lines rg/raw")
for p in picked:
    rg = ["/usr/bin/rg", "-uuu", "-a", "-n", "--no-heading", "-H", *p["rg"], "-e", p["pat"], "."]
    fk = [FORK, "-uuu", "-a", "-n", "--no-heading", "-H", *p["rg"], "-e", p["pat"], "."]
    rw = [RAW, "-a", "-j", "-n", "--cache-dir", CACHE, *p["raw"], p["pat"], "."]
    nc = [RAW, "-a", "-j", "-n", "--no-cache", *p["raw"], p["pat"], "."]
    subprocess.run(["rm", "-rf", CACHE]); os.makedirs(CACHE)
    first, _ = med(rw, 1)                       # first run, cache empty
    n = 3 if COLD else 7
    r1, l1 = med(rg, n, COLD); r2, _ = med(fk, n, COLD); r3, l3 = med(rw, n, COLD); r4, _ = med(nc, n, COLD)
    print(f"{p['pat'][:26]!r:<28}{r1:>8.0f}{r2:>9.0f}{r3:>12.0f}{r4:>9.0f}{first:>13.0f}   {l1}/{l3}", flush=True)
