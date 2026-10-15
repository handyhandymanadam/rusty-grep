# python3 -I ab.py N CWD -- BIN1[:ENV] BIN2[:ENV] ... -- args : interleaved medians (ms). ENV like RUSTY_AGENT=1
import os, statistics, subprocess, sys, time
n = int(sys.argv[1]); cwd = sys.argv[2]
rest = sys.argv[3:]
i = rest.index("--"); bins = rest[:i]; args = rest[i+1:]
if cwd == "/usr": args = args + ["/usr"]; cwd = None
res = {b: [] for b in bins}
for _ in range(n):
    for b in bins:
        path, _, envs = b.partition(":")
        e = {k: v for k, v in os.environ.items() if not k.startswith("RUSTY_")}
        for kv in envs.split(",") if envs else []:
            k, _, v = kv.partition("="); e[k] = v
        t = time.perf_counter()
        subprocess.run([os.path.abspath(path)] + args, cwd=cwd, env=e, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        res[b].append((time.perf_counter() - t) * 1000)
for b in bins:
    print(f"{b:40s} median {statistics.median(res[b]):8.1f} ms  min {min(res[b]):8.1f}")
