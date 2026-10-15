#!/usr/bin/env python3
"""A/B comparison of two rg binaries on a fixed workload list, interleaved to cancel drift.

Usage: ab.py BASE_RG NEW_RG [workload-name-substring ...]
Env: ROUNDS (default 15). Prints median wall ms of each, the NEW/BASE ratio and a verdict (>2% faster / slower).
Run `ab.py X X` first to see the noise floor. Data: ~/work/ripgrep-fork-data (kernel clone linux/, big.txt, w400.txt, tiny.txt).
"""
import os, statistics, subprocess, sys, time
D = os.path.expanduser("~/work/ripgrep-fork-data"); K = f"{D}/linux"
W = [  # name, cwd, args (output piped to wc -l)
    ("tree literal", K, "TODO ."), ("tree regex", K, r"'\w+_lock\(' ."), ("tree -i", K, "-i suspend ."),
    ("tree -c common", K, "-c include ."), ("tree -l", K, "-l EXPORT_SYMBOL ."), ("tree --files", K, "--files ."),
    ("tree small dir", f"{K}/kernel", "-c mutex ."), ("tree --sort path", K, "--sort path TODO ."),
    ("tree 110k rare", "/usr/share", "zzqx_nope ."),
    ("file literal", D, "static big.txt"), ("file regex", D, r"'[A-Z]+_[A-Z]+\(' big.txt"), ("file -i", D, "-i return big.txt"),
    ("file -c", D, "-c include big.txt"), ("file -v", D, "-v e big.txt"), ("file print-all", D, "'' big.txt"),
    ("file -n dense", D, "-n e big.txt"), ("file -n static", D, "-n static big.txt"), ("file --json", D, "--json static big.txt"),
    ("file color", D, "--color=always -n static big.txt"), ("file 400 words", D, "-F -f w400.txt big.txt"),
    ("file after ctx", D, "-A2 static big.txt"), ("startup tiny", D, "world tiny.txt"),
]
def once(rg, cwd, args):
    t = time.perf_counter(); subprocess.run(f"{rg} {args} | wc -l", shell=True, cwd=cwd, capture_output=True)
    return (time.perf_counter() - t) * 1000
def main():
    base, new = sys.argv[1], sys.argv[2]; pick = sys.argv[3:]; rounds = int(os.environ.get("ROUNDS", "15"))
    print(f"{'workload':<18}{'base ms':>9}{'new ms':>9}{'new/base':>10}  verdict")
    for name, cwd, args in W:
        if pick and not any(p in name for p in pick): continue
        once(base, cwd, args); once(new, cwd, args)
        a = []; b = []
        for i in range(rounds):
            if i % 2: b.append(once(new, cwd, args)); a.append(once(base, cwd, args))
            else: a.append(once(base, cwd, args)); b.append(once(new, cwd, args))
        ma, mb = statistics.median(a), statistics.median(b); r = mb / ma
        v = "FASTER" if r < 0.98 else ("SLOWER" if r > 1.02 else "same")
        print(f"{name:<18}{ma:>9.1f}{mb:>9.1f}{r:>10.3f}  {v}", flush=True)
main()
