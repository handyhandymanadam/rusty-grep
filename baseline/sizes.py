# python3 -I sizes.py BIN [LEVEL] : bytes of the JSON records on stderr (agent mode) for the reference jobs
import json, os, subprocess, sys
BIN = os.path.abspath(sys.argv[1]); LEVEL = sys.argv[2] if len(sys.argv) > 2 else None
home = os.path.expanduser("~"); T = home + "/work/tmp/tree40k"
JOBS = [
  ("40k -n alpha (success)", T, ["-n", "alpha"]),
  ("40k -c zzzz (no match)", T, ["-c", "zzzz"]),
  ("40k -l zzzz -t py (nothing searched)", T, ["-l", "zzzz", "-t", "py"]),
  ("40k bad regex", T, ["(unclosed"]),
  ("40k missing path", T, ["alpha", "nonexistent_dir"]),
  ("40k --limit 10 -n alpha", T, ["--limit", "10", "-n", "alpha"]),
  ("40k --within 2", T, ["--within", "2", "-e", "alpha", "-e", "beta"]),
  ("/usr -c e (errors flood)", None, ["-c", "e", "/usr"]),
  ("/usr -l needle_not_there_xyz", None, ["-l", "needle_not_there_xyz", "/usr"]),
]
total = {}
for name, cwd, args in JOBS:
    env = {k: v for k, v in os.environ.items() if not k.startswith("RUSTY_")}
    env["RUSTY_AGENT"] = "1"
    if LEVEL: env["RUSTY_AGENT_DETAIL"] = LEVEL
    p = subprocess.run([BIN] + args, cwd=cwd, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    per = {}
    n = 0
    for l in p.stderr.decode().splitlines():
        if l.startswith("{"):
            r = json.loads(l); per[r["type"]] = per.get(r["type"], 0) + len(l) + 1; n += 1
    print(f"{name:42s} {sum(per.values()):6d} B  {n:4d} records  {per}")
