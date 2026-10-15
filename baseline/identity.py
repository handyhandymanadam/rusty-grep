# python3 -I identity.py BASE NEW : agent OFF; compare rc, stdout, stderr of many commands.
# Parallel unsorted output has no fixed order, so those compare sorted lines.
import hashlib, os, subprocess, sys
base, new = [os.path.abspath(a) for a in sys.argv[1:3]]
home = os.path.expanduser("~")
T = home + "/work/tmp/tree40k"
M = home + "/work/tmp/mt"
CMDS = [
  (T, ["-n", "alpha"]), (T, ["-c", "alpha"]), (T, ["-l", "alpha"]), (T, ["-L", "alpha"]), (T, ["-q", "alpha"]),
  (T, ["--sort", "path", "-n", "alpha"]), (T, ["-j1", "-n", "alpha"]), (T, ["--json", "alpha"]), (T, ["-j1", "--json", "alpha"]),
  (T, ["--stats", "-c", "alpha"]), (T, ["--stats", "-j1", "-n", "alpha"]), (T, ["-o", "alpha"]), (T, ["-v", "-c", "alpha"]),
  (T, ["-w", "-c", "alpha"]), (T, ["-F", "-c", "alpha beta"]), (T, ["-U", "-c", "alpha\nbeta"]), (T, ["-A2", "-n", "alpha beta gamma"]),
  (T, ["--files"]), (T, ["--hidden", "--no-ignore", "-c", "alpha"]), (T, ["-t", "txt", "-c", "alpha"]), (T, ["-g", "*.log", "-c", "alpha"]),
  (T, ["--limit", "5", "-n", "alpha"]), (T, ["--accounting", "-c", "zzzz"]), (T, ["--time-limit", "30", "-c", "alpha"]),
  (T, ["--all-in-file", "-e", "alpha", "-e", "beta"]), (T, ["--within", "2", "-e", "alpha", "-e", "beta"]), (T, ["--records", "-C1", "alpha beta gamma"]),
  (T, ["--max-columns-window", "-n", "alpha"]), (T, ["--stable-order", "-n", "alpha"]), (T, ["--stable-order", "--diagnostics-format=json", "-c", "alpha", "nonexistent"]),
  (T, ["alpha", "nonexistent"]), (T, ["(unclosed"]), (T, ["-A", "x", "alpha"]), (T, ["alpha", "-f", "no_such_file"]),
  (T, ["--diagnostics-format=json", "(unclosed"]), (T, ["--git-tracked", "alpha"]), (M, ["needle"]), (M, ["-c", "needle", "."]), (M, ["--hidden", "-c", "needle"]),
  (M, ["--accounting", "needle"]), (M, ["--accounting", "zzzz"]), (M, ["--no-ignore", "-c", "needle"]), (M, ["-g", "!*", "needle"]),
  ("/usr/include", ["-c", "define"]), ("/usr/include", ["--sort", "path", "-c", "define"]), ("/usr/include", ["--stats", "-c", "include"]),
  (None, ["--version"]), (None, ["--help"]), (None, ["--type-list"]),
]
def run(binp, cwd, args):
    env = {k: v for k, v in os.environ.items() if not k.startswith("RUSTY_")}
    env["TZ"] = "UTC"
    p = subprocess.run([binp] + args, cwd=cwd, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    return p.returncode, p.stdout, p.stderr
import re
def mask(b):
    b = re.sub(rb"\(rev [0-9a-f]+\)", b"(rev X)", b)
    b = re.sub(rb'("elapsed[^,}]*|"(?:secs|nanos)":)[0-9.]+', rb"\1T", b)
    b = re.sub(rb'"human":"[^"]*"', b'"human":"T"', b)
    b = re.sub(rb'"(?:elapsed|elapsed_total|bytes_printed)[^}]*}', b'"E"', b)
    return b
def norm(b):
    return b"\n".join(sorted(mask(b).split(b"\n")))
def norm_stats(b):  # --stats prints seconds
    out = []
    for l in b.split(b"\n"):
        if b"seconds" in l: l = b"<seconds>"
        out.append(l)
    return b"\n".join(out)
bad = 0
for cwd, args in CMDS:
    a = run(base, cwd, args); b = run(new, cwd, args)
    stat = "--stats" in args or "--json" in args
    f = (lambda x: norm(norm_stats(x))) if stat else norm
    expected = any(x in args for x in ("--limit","--accounting","--time-limit","--records","--help","--stable-order","--diagnostics-format=json","--max-columns-window","--all-in-file","--within"))
    if expected:
        print("agent-flag", args, "rc", a[0], b[0], "stdout same" if a[1]==b[1] else "stdout differs", "stderr same" if a[2]==b[2] else "stderr differs")
        continue
    ok = a[0] == b[0] and f(a[1]) == f(b[1]) and norm(norm_stats(a[2])) == norm(norm_stats(b[2]))
    exact = a[1] == b[1]
    if not ok:
        bad += 1
        print("DIFF", cwd, args, a[0], b[0], len(a[1]), len(b[1]), len(a[2]), len(b[2]))
    elif not (mask(a[1]) == mask(b[1])) and ("-j1" in args or "--sort" in args):
        bad += 1
        print("ORDER", cwd, args)
print(f"{len(CMDS)} commands, {bad} differ")
