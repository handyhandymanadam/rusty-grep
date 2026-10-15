#!/usr/bin/env python3
"""5-way benchmark: grep, ugrep, ripgrep (Fedora), ripgrep-fork, rawgrep (+ no-cache). Median of RUNS after 1 warm-up."""
import os, shlex, statistics, subprocess, sys, tempfile, time
H = os.path.expanduser("~/work/ripgrep-fork-data/fiveway")
MNT = f"{H}/mnt"
CACHE = f"{H}/rgcache"
os.makedirs(CACHE, exist_ok=True)
T = {
    "grep":    "/usr/bin/grep",
    "ugrep":   f"{H}/ugrep/src/ugrep",
    "rg":      "/usr/bin/rg",
    "fork":    os.path.expanduser("~/work/ripgrep-fork/target-pgo/release-lto/rg"),
    "rawgrep": f"{H}/rawgrep/target/release-fast/rawgrep",
}
RUNS = int(os.environ.get("RUNS", "5"))
ONLY = os.environ.get("ONLY")

def once(argv, cwd):
    tf = tempfile.mktemp(dir=H)
    inner = "/usr/bin/time -f '%U %S %M' -o {} {} 2>/dev/null | wc -l".format(tf, " ".join(shlex.quote(a) for a in argv))
    t = time.perf_counter()
    p = subprocess.run(["sh", "-c", inner], cwd=cwd, capture_output=True, text=True)
    wall = (time.perf_counter() - t) * 1000
    try:
        u, s, m = open(tf).read().split()[-3:]
        os.unlink(tf)
        return wall, (float(u) + float(s)) * 1000, int(m) / 1024, int(p.stdout.strip() or 0)
    except Exception:
        return None

def run(tool, args, cwd, runs=RUNS):
    exe = T[tool.split("+")[0]]
    argv = [exe] + args
    once(argv, cwd)
    rs = [r for r in (once(argv, cwd) for _ in range(runs)) if r]
    if not rs: return None
    return dict(tool=tool, wall=statistics.median(r[0] for r in rs), cpu=statistics.median(r[1] for r in rs),
                rss=max(r[2] for r in rs), lines=rs[-1][3])

def scen(title, cwd, spec, ref="rg"):
    if ONLY and ONLY not in title: return
    rows = [run(t, a, cwd) for t, a in spec.items() if a is not None]
    rows = [r for r in rows if r]
    best = min(r["wall"] for r in rows)
    refl = next((r["lines"] for r in rows if r["tool"] == ref), None)
    print(f"\n### {title}")
    print(f"{'tool':<12}{'wall ms':>9}{'cpu ms':>9}{'RSS MB':>8}{'lines':>10}")
    for r in rows:
        note = "  <- fastest" if r["wall"] == best else f"  x{r['wall']/best:.1f}"
        if r["tool"].startswith("rawgrep") and refl:
            if r["lines"] < refl * 0.97: note += "  ** MISSES MATCHES"
            elif r["lines"] > refl * 1.03: note += "  (+extra)"
        print(f"{r['tool']:<12}{r['wall']:>9.0f}{r['cpu']:>9.0f}{r['rss']:>8.0f}{r['lines']:>10}{note}")
    sys.stdout.flush()

def tree(title, d, pat, g=(), u=(), r=(), w=(), sub=".", rawgrep=True, word=None):
    cwd = f"{MNT}/{d}"
    spec = {
        "grep":  ["-rI", *g, "--", pat, sub],
        "ugrep": ["-rI", *u, "--", pat, sub],
        "rg":    [*r, "--", pat, sub],
        "fork":  [*r, "--", pat, sub],
    }
    if rawgrep:
        spec["rawgrep"] = ["--cache-dir", CACHE, "-j", "-N", *w, pat, sub]
        spec["rawgrep+nc"] = ["--no-cache", "-j", "-N", *w, pat, sub]
    scen(title, cwd, spec)

def main():
    which = sys.argv[1:] or ["trees", "files"]
    if "trees" in which:
        tree("kernel (96k files): literal TODO", "linux", "TODO")
        tree("kernel: regex [A-Z]+_SUSPEND", "linux", "[A-Z]+_SUSPEND", g=["-E"], u=["-E"])
        tree("kernel: -i regex \\bTODO\\([a-z]+\\)", "linux", r"\bTODO\([a-z]+\)", g=["-iE"], u=["-iE"], r=["-i"], w=["-i"])
        tree("kernel: no matches", "linux", "zzqx_not_here_9")
        tree("kernel: very common 'static'", "linux", "static")
        tree("kernel: -w mutex_lock", "linux", "mutex_lock", g=["-w"], u=["-w"], r=["-w"], w=["-w"])
        tree("kernel: alternation mutex|spinlock|rwsem", "linux", "mutex|spinlock|rwsem", g=["-E"], u=["-E"])
        tree("kernel: -i copyright", "linux", "copyright", g=["-i"], u=["-i"], r=["-i"], w=["-i"])
        tree("kernel: -l EXPORT_SYMBOL", "linux", "EXPORT_SYMBOL", g=["-l"], u=["-l"], r=["-l"], rawgrep=False)
        tree("kernel: -c include", "linux", "include", g=["-c"], u=["-c"], r=["-c"], rawgrep=False)
        tree("kernel: subdir drivers/gpu TODO", "linux", "TODO", sub="drivers/gpu")
        tree("kernel: ^ anchor '^#include <linux'", "linux", "^#include <linux", g=["-E"], u=["-E"])
        tree("usr-share (110k files): -i license", "usr-share", "license", g=["-i"], u=["-i"], r=["-i"], w=["-i"])
        tree("usr-share: non-ASCII é|ü|ñ", "usr-share", "é|ü|ñ", g=["-E"], u=["-E"])
        tree("usr-share: rare, no hits", "usr-share", "zzqx_not_here_9")
        tree("usr-include (6k files): struct", "usr-include", "struct")
        tree("python lib (8k files): def __init__", "py", "def __init__")
    if "files" in which:
        c = MNT
        def f(title, pat, extra=(), file="big.txt", raw=True, g=None, u=None, r=None, w=()):
            spec = {
                "grep":  [*(g if g is not None else extra), "--", pat, file] if True else None,
                "ugrep": [*(u if u is not None else extra), "--", pat, file],
                "rg":    [*(r if r is not None else extra), "--", pat, file],
                "fork":  [*(r if r is not None else extra), "--", pat, file],
            }
            if raw:
                spec["rawgrep"] = ["--cache-dir", CACHE, "--large", "-j", "-N", *w, pat, file]
            scen(title, c, spec)
        f("file 239MB: rare literal", "zzqx_not_here_9", g=["-F"], u=["-F"], r=["-F"])
        f("file: common literal 'static'", "static", g=["-F"], u=["-F"], r=["-F"])
        f("file: -i 'returns the'", "returns the", g=["-i"], u=["-i"], r=["-i"], w=["-i"])
        f("file: regex def-like \\w+_lock\\(", r"\w+_lock\(", g=["-E"], u=["-E"], r=[])
        f("file: alternation", "mutex|spinlock|rwsem", g=["-E"], u=["-E"], r=[])
        f("file: -c include", "include", g=["-c"], u=["-c"], r=["-c"], raw=False)
        f("file: -v e (invert)", "e", g=["-v"], u=["-v"], r=["-v"], raw=False)
        f("file: print ALL lines", "", g=[], u=[], r=[], raw=False)
        f("file: backref (\\w+) \\1", r"(\w+) \1", g=["-E"], u=["-P"], r=["-P"], raw=False)
        f("file: unicode \\w+ion\\b", r"\w+ion\b", g=["-E"], u=["-E"], r=[], raw=False)
        f("tiny file startup", "world", file="tiny.txt")
        for n, fn in [("100 words", "w400.txt")]:
            pass
        # word lists (pattern file)
        spec = {"grep": ["-F", "-f", "w400.txt", "big.txt"], "ugrep": ["-F", "-f", "w400.txt", "big.txt"],
                "rg": ["-F", "-f", "w400.txt", "big.txt"], "fork": ["-F", "-f", "w400.txt", "big.txt"]}
        scen("file: 400-word list (-f)", c, spec)
        spec = {k: ["-F", "-f", "w1000.txt", "big.txt"] for k in ("grep", "ugrep", "rg", "fork")}
        scen("file: 1000-word list (-f)", c, spec)
        spec = {k: ["-F", "-f", "m400.txt", "big.txt"] for k in ("grep", "ugrep", "rg", "fork")}
        scen("file: 400 short words (-f)", c, spec)
main()
