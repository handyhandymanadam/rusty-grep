#!/usr/bin/env python3
"""Staleness audit: does rawgrep's cache notice changed files? Ground truth = ripgrep.
Each scenario: (1) search for a unique token so the cache learns it is absent everywhere,
(2) change something, (3) search again and compare with ripgrep."""
import os, shutil, subprocess, sys, time, random
H = os.path.expanduser("~/work/ripgrep-fork-data/audit")
MNT = f"{H}/mnt"; D = f"{MNT}/stale"; CACHE = f"{H}/rgcache_stale"; RAW = f"{H}/rawgrep"
def sh(argv, cwd=D): return subprocess.run(argv, cwd=cwd, capture_output=True, text=True).stdout
def files_rg(tok): return sorted(l.lstrip("./") for l in sh(["/usr/bin/rg", "-uuu", "-a", "-l", tok, "."]).split("\n") if l)
def files_raw(tok, nc=False):
    out = sh([RAW, "-a", "-j", "-n", *(["--no-cache"] if nc else ["--cache-dir", CACHE]), tok, "."])
    return sorted({l.split(":", 1)[0].lstrip("./") for l in out.split("\n") if ":" in l})
def setup():
    shutil.rmtree(D, ignore_errors=True); shutil.rmtree(CACHE, ignore_errors=True); os.makedirs(CACHE)
    src = f"{MNT}/linux/kernel"; shutil.copytree(src, D, symlinks=True)
    subprocess.run(["sync"]); time.sleep(3)           # let every file become "old"
FILES = lambda: sorted(os.path.join(dp, f)[len(D) + 1:] for dp, _, fs in os.walk(D) for f in fs if f.endswith(".c"))
def edit_same_size(path, tok, keep_mtime):
    full = os.path.join(D, path); st = os.stat(full); data = bytearray(open(full, "rb").read())
    data[10:10 + len(tok)] = tok.encode(); open(full, "wb").write(data)
    if keep_mtime: os.utime(full, ns=(st.st_atime_ns, st.st_mtime_ns))
def scenario(name, token, mutate, expect_changed):
    setup(); fl = FILES(); random.seed(1); picks = random.sample(fl, 5)
    files_raw(token)                                   # learn "absent everywhere"
    before = files_raw(token)
    mutate(picks)
    t_rg, t_raw, t_nc = files_rg(token), files_raw(token), files_raw(token, True)
    ok = t_raw == t_rg
    print(f"{'PASS' if ok else 'FAIL'}  {name:<58} rg={len(t_rg)} files, rawgrep(cache)={len(t_raw)}, rawgrep(--no-cache)={len(t_nc)}")
    if not ok: print(f"      missed: {sorted(set(t_rg) - set(t_raw))[:3]}  extra: {sorted(set(t_raw) - set(t_rg))[:3]}")
    return ok
def append_tok(tok): return lambda ps: [open(os.path.join(D, p), "a").write(f"\n/* {tok} */\n") for p in ps]
T = lambda n: f"STALETOKEN{n}QZ"
res = []
res.append(scenario("append a line (size changes)", T(1), append_tok(T(1)), 5))
res.append(scenario("same-size edit, new mtime a few seconds later", T(2), lambda ps: (time.sleep(3), [edit_same_size(p, T(2), False) for p in ps]), 5))
res.append(scenario("same-size edit in the SAME SECOND as the search", T(3), lambda ps: [edit_same_size(p, T(3), False) for p in ps], 5))
res.append(scenario("same-size edit, mtime restored (touch -r)", T(4), lambda ps: [edit_same_size(p, T(4), True) for p in ps], 5))
def replace_inode(ps):
    for p in ps:
        full = os.path.join(D, p); st = os.stat(full); data = bytearray(open(full, "rb").read()); data[10:10 + len(T(5))] = T(5).encode()
        tmp = full + ".new"; open(tmp, "wb").write(data); os.utime(tmp, ns=(st.st_atime_ns, st.st_mtime_ns)); os.rename(tmp, full)
res.append(scenario("replace file (new inode, same size, mtime restored)", T(5), replace_inode, 5))
res.append(scenario("create a new file with the token", T(6), lambda ps: open(os.path.join(D, "brand_new.c"), "w").write(f"x {T(6)} y\n"), 1))
def add_then_delete(ps):
    for p in ps: open(os.path.join(D, p), "a").write(f"\n{T(7)}\n")
res.append(scenario("append token then search (learn present), delete file", T(7), lambda ps: (add_then_delete(ps), files_raw(T(7)), [os.unlink(os.path.join(D, p)) for p in ps]), 0))
def rename(ps):
    for p in ps: open(os.path.join(D, p), "a").write(f"\n{T(8)}\n")
    files_raw(T(8))
    for p in ps: os.rename(os.path.join(D, p), os.path.join(D, p + ".renamed.c"))
res.append(scenario("token present, then file renamed", T(8), rename, 5))
def remove_tok(ps):
    for p in ps: open(os.path.join(D, p), "a").write(f"\n{T(9)}\n")
    files_raw(T(9))
    for p in ps:
        full = os.path.join(D, p); open(full, "w").write(open(full).read().replace(T(9), "gone"))
res.append(scenario("token present (cache says yes), then token removed", T(9), remove_tok, 0))
def swap_content(ps):
    a, b = ps[0], ps[1]
    open(os.path.join(D, a), "a").write(f"\n{T(10)}\n"); os.utime(os.path.join(D, b), None)
    shutil.copyfile(os.path.join(D, a), os.path.join(D, b))   # b now has a's content (size differs; keep simple)
res.append(scenario("copy a file over another (content changes)", T(10), swap_content, 2))
print(f"\n{sum(res)}/{len(res)} scenarios passed")
