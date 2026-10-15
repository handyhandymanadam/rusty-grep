#!/usr/bin/env python3
"""Differential audit: rawgrep vs ripgrep (ground truth) on matched settings.

For every pattern in patterns.json:
  truth        = rg -uuu -a -n (searches hidden, ignored, binary; as text)
  raw_cache1   = rawgrep -a with a PERSISTENT cache shared across the whole run (learning as it goes)
  raw_nc       = rawgrep -a --no-cache
  raw_cache2   = rawgrep -a with the now-warm cache, patterns in shuffled order
Compares sets of (path, line number, text). Reports missing/extra lines, which mode lost them, and why.
"""
import json, os, random, re, subprocess, sys, collections, time
H = os.path.expanduser("~/work/ripgrep-fork-data")
RG = "/usr/bin/rg"
RAW = os.environ.get("RAWGREP", f"{H}/audit/rawgrep")
CORPUS = os.environ.get("CORPUS", f"{H}/audit/mnt/linux")
CACHE = f"{H}/audit/rgcache"
PATS = json.load(open(f"{H}/audit/patterns.json"))
LIMIT = int(os.environ.get("LIMIT", len(PATS)))
if os.environ.get("CATS"): PATS = [p for p in PATS if p["cat"] in os.environ["CATS"].split(",")]
PATS = PATS[:LIMIT]

WORK = f"{H}/audit/work"
SEP = b"\x01"

def stream_to_file(argv, out_path, raw):
    """Run argv in CORPUS, normalise each output line to path SEP line SEP text, write to out_path (no big sets in memory)."""
    t = time.perf_counter(); n = bad = 0
    with open(out_path, "wb") as out:
        p = subprocess.Popen(argv, cwd=CORPUS, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        for ln in p.stdout:
            ln = ln.rstrip(b"\n")
            if not ln: continue
            parts = ln.split(b":", 2)
            if len(parts) < 3 or not parts[1].isdigit(): bad += 1; continue
            path = parts[0][2:] if parts[0].startswith(b"./") else parts[0]
            text = parts[2][1:] if raw and parts[2][:1] == b" " else parts[2]
            out.write(path + SEP + parts[1] + SEP + text.rstrip(b"\r") + b"\n"); n += 1
        p.wait()
    return time.perf_counter() - t, p.returncode, n, bad

ENV = dict(os.environ, LC_ALL="C")
def sort_unique(src, dst):
    subprocess.run(["sort", "-u", "-S", "256M", "-T", WORK, "-o", dst, src], env=ENV, check=True)
def wc(path):
    return sum(1 for _ in open(path, "rb"))
def comm(mode, a, b, limit=None):
    """Lines only in a (mode 23) or only in b (mode 13): returns path of result file."""
    out = a + (".only" if mode == "23" else ".extra")
    with open(out, "wb") as f: subprocess.run(["comm", f"-{mode}", a, b], env=ENV, stdout=f, check=True)
    return out
def first_lines(path, n=3):
    with open(path, "rb") as f: return [f.readline().rstrip(b"\n") for _ in range(n)]
def files_of(path, limit=200):
    out = subprocess.run(f"cut -d $'\\001' -f1 {path} | sort -u | head -n {limit}", shell=True, executable="/bin/bash", env=ENV, capture_output=True).stdout
    return [x.decode("utf-8", "replace") for x in out.split(b"\n") if x]

def truth_cmd(p): return [RG, "-uuu", "-a", "-n", "--no-heading", "-H", *p["rg"], "-e", p["pat"], "."]
def raw_cmd(p, mode):
    base = [RAW, "-a", "--hidden", "--binary", "--large", "--reserved-tool-dirs", "--no-ignore", "-j", "-n"]
    base += ["--no-cache"] if mode == "nc" else ["--cache-dir", CACHE]
    return base + p["raw"] + [p["pat"], "."]

def compare(p, mode, truth_sorted):
    """Run rawgrep in `mode`, compare with the sorted truth file. Returns result dict."""
    rawf = f"{WORK}/raw.txt"
    t, rc, n, bad = stream_to_file(raw_cmd(p, mode), rawf, True)
    sort_unique(rawf, rawf + ".s"); os.unlink(rawf)
    only = comm("23", truth_sorted, rawf + ".s"); extra = comm("13", truth_sorted, rawf + ".s")
    r = dict(found=wc(rawf + ".s"), missing=wc(only), extra=wc(extra), t=t, bad=bad, rc=rc,
             miss_ex=[l.split(SEP)[:2] and [x.decode("utf-8", "replace") for x in l.split(SEP)[:2]] for l in first_lines(only) if l],
             miss_files=files_of(only))
    # same (path, line) with different text
    r["text_diff"] = 0
    for f in (rawf + ".s", only, extra): pass
    for f in (only, extra, rawf + ".s"): os.unlink(f)
    return r

def main():
    os.makedirs(CACHE, exist_ok=True); os.makedirs(WORK, exist_ok=True)
    for f in os.listdir(CACHE): os.unlink(os.path.join(CACHE, f))
    res = []
    print(f"{len(PATS)} patterns, corpus {CORPUS}", flush=True)
    for p in PATS:
        tf = f"{WORK}/truth.txt"
        tt, _, tn, _ = stream_to_file(truth_cmd(p), tf, False)
        sort_unique(tf, tf + ".s"); os.unlink(tf)
        r = dict(id=p["id"], cat=p["cat"], pat=p["pat"], flags=p["rg"], truth=wc(tf + ".s"), t_rg=tt)
        for mode in ("cache1", "nc"): r[mode] = compare(p, mode, tf + ".s")
        os.unlink(tf + ".s"); res.append(r)
        if (len(res) % 25) == 0: print(f"  ...{len(res)}/{len(PATS)}", flush=True)
    random.seed(7); idx = list(range(len(PATS))); random.shuffle(idx)
    for k, i in enumerate(idx):
        p = PATS[i]; tf = f"{WORK}/truth.txt"
        stream_to_file(truth_cmd(p), tf, False); sort_unique(tf, tf + ".s"); os.unlink(tf)
        res[i]["cache2"] = compare(p, "cache2", tf + ".s"); os.unlink(tf + ".s")
        if ((k + 1) % 50) == 0: print(f"  pass2 ...{k+1}/{len(PATS)}", flush=True)
    json.dump(res, open(f"{H}/audit/results.json", "w"))
    summarize(res)

def why(path):
    """Plausible reason a path might be skipped by default rules."""
    full = os.path.join(CORPUS, path); r = []
    try:
        sz = os.path.getsize(full)
        if sz > 8 * 1024 * 1024: r.append("size>8MB")
        with open(full, "rb") as f: head = f.read(65536)
        if b"\0" in head: r.append("has-NUL")
    except OSError: r.append("unreadable")
    parts = path.split("/")
    if any(x.startswith(".") for x in parts): r.append("hidden")
    if "target" in parts[:-1]: r.append("dir-named-target")
    return r or ["no-obvious-reason"]

def summarize(res):
    print("\n=== per category: patterns / patterns-with-any-loss (cache1, nc, cache2) / lines truth, missing (cache1, nc, cache2), extra (cache1)")
    cats = collections.OrderedDict()
    for r in res: cats.setdefault(r["cat"], []).append(r)
    tot = collections.Counter()
    for c, rs in cats.items():
        n = len(rs); truth = sum(r["truth"] for r in rs)
        loss = {m: sum(1 for r in rs if r[m]["missing"] > 0) for m in ("cache1", "nc", "cache2")}
        mis = {m: sum(r[m]["missing"] for r in rs) for m in ("cache1", "nc", "cache2")}
        ext = sum(r["cache1"]["extra"] for r in rs)
        print(f"{c:<20} n={n:<3} loss-patterns c1/nc/c2 = {loss['cache1']:>3}/{loss['nc']:>3}/{loss['cache2']:>3}   truth={truth:>8}  missing c1/nc/c2 = {mis['cache1']:>7}/{mis['nc']:>7}/{mis['cache2']:>7}  extra={ext}")
        tot["n"] += n; tot["truth"] += truth
        for m in ("cache1", "nc", "cache2"): tot["miss_" + m] += mis[m]; tot["loss_" + m] += loss[m]
    print(f"\nTOTAL patterns={tot['n']} truth-lines={tot['truth']}")
    for m in ("cache1", "nc", "cache2"):
        print(f"  {m:<7} patterns with any missing line: {tot['loss_'+m]}/{tot['n']}   missing lines: {tot['miss_'+m]} ({100*tot['miss_'+m]/max(1,tot['truth']):.2f}% of truth)")
    # attribution of missing lines by file reason (nc mode and cache-only difference)
    reason = collections.Counter(); cache_only = 0
    for r in res:
        nc_files = set(r["nc"]["miss_files"])
        for f in r["cache1"]["miss_files"]:
            if f not in nc_files: cache_only += 1
        for f in nc_files:
            for w in why(f): reason[w] += 1
    print("\nfiles with missing lines even with --no-cache, by plausible reason (file-pattern pairs):", dict(reason))
    print("file-pattern pairs lost ONLY when the cache is on (cache1 but not nc):", cache_only)
    t_rg = sum(r["t_rg"] for r in res)
    print(f"\nTime over all patterns: rg {t_rg:.1f}s | rawgrep cache1 {sum(r['cache1']['t'] for r in res):.1f}s | nc {sum(r['nc']['t'] for r in res):.1f}s | cache2 {sum(r['cache2']['t'] for r in res):.1f}s")
    print("\nWorst patterns (cache1):")
    for r in sorted(res, key=lambda r: -r["cache1"]["missing"])[:12]:
        print(f"  [{r['cat']}] {r['pat'][:40]!r} {r['flags']} truth={r['truth']} c1 missing={r['cache1']['missing']} nc missing={r['nc']['missing']} c2 missing={r['cache2']['missing']} ex={r['cache1']['miss_ex'][:1]}")
if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "summary": summarize(json.load(open(f"{H}/audit/results.json")))
    else: main()
