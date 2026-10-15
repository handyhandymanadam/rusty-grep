# python3 -I run.py BIN [N] : medians (ms) of N runs for plain and agent jobs; also sha256 of plain outputs.
import hashlib, os, statistics, subprocess, sys, time
BIN = os.path.abspath(sys.argv[1])
N = int(sys.argv[2]) if len(sys.argv) > 2 else 5
HOME = os.path.expanduser("~")
T40 = HOME + "/work/tmp/tree40k"
JOBS = [
  ("40k -n alpha", T40, ["-n", "alpha"]),
  ("40k -c alpha", T40, ["-c", "alpha"]),
  ("40k -l zzzz",  T40, ["-l", "zzzz"]),
  ("40k rare",     T40, ["-n", "alpha beta gamma delta epsilon zeta eta theta"]),
  ("usr -c e",     "/usr", ["-c", "e"]),
  ("usr -l needle_not_there_xyz", "/usr", ["-l", "needle_not_there_xyz"]),
]
def env(agent):
    e = {k: v for k, v in os.environ.items() if not k.startswith("RUSTY_")}
    if agent: e["RUSTY_AGENT"] = "1"
    return e
def one(cwd, args, agent):
    t = time.perf_counter()
    p = subprocess.run([BIN] + args + ([] if cwd == "/usr" else []) + (["/usr"] if cwd == "/usr" else []),
                       cwd=None if cwd == "/usr" else cwd, env=env(agent),
                       stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    return (time.perf_counter() - t) * 1000, p
for name, cwd, args in JOBS:
    row = {}
    for agent in (False, True):
        ts = []
        for i in range(N):
            ms, p = one(cwd, args, agent)
            ts.append(ms)
        row[agent] = (statistics.median(ts), len(p.stdout), len(p.stderr), hashlib.sha256(b"\n".join(sorted(p.stdout.split(b"\n")))).hexdigest()[:12] if not agent else "")
    print(f"{name:30s} plain {row[False][0]:7.1f} ms  agent {row[True][0]:7.1f} ms  ratio {row[True][0]/row[False][0]:.2f}  stdout {row[False][1]}B hash {row[False][3]}  agent-stderr {row[True][2]}B")
