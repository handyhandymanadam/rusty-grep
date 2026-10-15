"""Item A (branch agent-fd): RUSTY_AGENT_FD=N appends the same JSON-lines records to an already-open descriptor,
like RUSTY_AGENT_OUT. Real program, real files, every record parsed as JSON.

    HW=/path/to/rg python3 -I tests/agent_fd.py [-v]

`cargo test` runs this file too (tests/agent.rs, test `agent_fd_python`).
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest

HW = os.environ.get("HW")
if not HW:
    sys.exit("set HW to the program to test")
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def env_for(agent=True, **extra):
    env = {k: v for k, v in os.environ.items() if not k.startswith("RUSTY_")}
    env.pop("RIPGREP_CONFIG_PATH", None)
    env["RUSTY_AGENT_DETAIL"] = "full"
    if agent:
        env["RUSTY_AGENT"] = "1"
    env.update(extra)
    return env


def run(args, cwd, agent=True, pass_fds=(), **extra):
    p = subprocess.run([HW] + list(args), cwd=cwd, env=env_for(agent, **extra), stdin=subprocess.DEVNULL,
                       stdout=subprocess.PIPE, stderr=subprocess.PIPE, pass_fds=pass_fds)
    err = p.stderr.decode("utf-8", "replace")
    recs = [json.loads(l) for l in err.splitlines() if l.startswith("{")]
    return p.returncode, p.stdout, recs, err


def lines(path):
    with open(path) as f:
        return [json.loads(l) for l in f.read().splitlines()]


def codes(recs):
    return [r.get("code") for r in recs if r.get("type") == "diagnostic"]


class FD(unittest.TestCase):
    def setUp(self):
        self.d = tempfile.mkdtemp(prefix="hwfd-", dir=os.environ.get("TMPDIR"))
        self.addCleanup(shutil.rmtree, self.d, True)
        with open(self.d + "/a.txt", "w") as f:
            f.write("needle one\nhay\nneedle two\n")
        self.args = ["-n", "needle", "a.txt", "nosuchfile"]  # a match, exit 2 and a G0003 record

    def fd(self, name="fd.jsonl"):
        return os.open(self.d + "/" + name, os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o644)

    def test_valid_fd_file(self):
        fd = self.fd()
        rc, out, recs, err = run(self.args, self.d, pass_fds=(fd,), RUSTY_AGENT_FD=str(fd))
        os.close(fd)
        got = lines(self.d + "/fd.jsonl")
        self.assertEqual(recs, got)
        self.assertGreaterEqual(len(got), 2)  # the G0003 diagnostic and the summary
        self.assertEqual(["diagnostic", "summary"], [r["type"] for r in got][-2:])
        base = run(self.args, self.d)
        self.assertEqual(base[0], rc)
        self.assertEqual(base[1], out)
        self.assertNotIn("G0013", codes(recs))

    def test_valid_fd_pipe(self):
        r, w = os.pipe()
        rc, out, recs, err = run(self.args, self.d, pass_fds=(w,), RUSTY_AGENT_FD=str(w))
        os.close(w)
        got = [json.loads(l) for l in os.read(r, 1 << 22).decode().splitlines()]
        os.close(r)
        self.assertEqual(recs, got)

    def test_second_run_appends(self):
        fd = self.fd()
        run(self.args, self.d, pass_fds=(fd,), RUSTY_AGENT_FD=str(fd))
        n = len(lines(self.d + "/fd.jsonl"))
        rc, out, recs, err = run(self.args, self.d, pass_fds=(fd,), RUSTY_AGENT_FD=str(fd))
        os.close(fd)
        self.assertEqual(2 * n, len(lines(self.d + "/fd.jsonl")))

    def bad(self, value, why, pass_fds=()):
        rc, out, recs, err = run(self.args, self.d, pass_fds=pass_fds, RUSTY_AGENT_FD=value)
        w = [r for r in recs if r.get("code") == "G0013"]
        self.assertEqual(1, len(w), (value, err))
        self.assertEqual("warning", w[0]["severity"])
        self.assertIn("RUSTY_AGENT_FD", w[0]["message"])
        self.assertIn(why, w[0]["message"])
        self.assertTrue(w[0]["suggestions"])
        self.assertTrue(all(l.startswith("{") for l in err.splitlines()), err)  # no plain text among the records
        base = run(self.args, self.d)
        self.assertEqual((base[0], base[1]), (rc, out))  # exit code and stdout unchanged
        s = [r for r in recs if r.get("type") == "summary"][0]
        self.assertEqual(rc, s["exit"])

    def test_bad_not_a_number(self):
        self.bad("abc", "not a decimal number")
        self.bad("", "not a decimal number")
        self.bad("-1", "not a decimal number")

    def test_bad_too_large(self):
        self.bad("99999999999999999999999", "too large")

    def test_bad_standard_stream(self):
        self.bad("2", "below 3")

    def test_closed_fd(self):
        self.bad("57", "not open")

    def test_read_only_fd(self):
        fd = os.open(self.d + "/a.txt", os.O_RDONLY)
        self.bad(str(fd), "reading only", (fd,))
        os.close(fd)

    def test_closed_fd_on_success_exits_zero(self):
        rc, out, recs, err = run(["needle", "a.txt"], self.d, RUSTY_AGENT_FD="57")
        self.assertEqual(0, rc)
        self.assertEqual(["G0013"], codes(recs))

    def test_bad_option_before_options_are_read(self):
        # a record that comes before the options are applied also reaches the descriptor
        fd = self.fd()
        rc, out, recs, err = run(["-A", "x", "needle", "a.txt"], self.d, pass_fds=(fd,), RUSTY_AGENT_FD=str(fd))
        os.close(fd)
        self.assertEqual(2, rc)
        self.assertEqual(recs, lines(self.d + "/fd.jsonl"))
        self.assertIn("G0004", codes(recs))

    def test_out_and_fd_both(self):
        fd = self.fd()
        rc, out, recs, err = run(self.args, self.d, pass_fds=(fd,), RUSTY_AGENT_FD=str(fd), RUSTY_AGENT_OUT=self.d + "/o.jsonl")
        os.close(fd)
        self.assertEqual(recs, lines(self.d + "/o.jsonl"))
        self.assertEqual(recs, lines(self.d + "/fd.jsonl"))

    def test_out_good_fd_closed(self):
        rc, out, recs, err = run(self.args, self.d, RUSTY_AGENT_FD="57", RUSTY_AGENT_OUT=self.d + "/o.jsonl")
        self.assertEqual(recs, lines(self.d + "/o.jsonl"))
        self.assertEqual(1, codes(recs).count("G0013"))
        self.assertEqual(2, rc)

    def test_out_bad_fd_good(self):
        fd = self.fd()
        rc, out, recs, err = run(self.args, self.d, pass_fds=(fd,), RUSTY_AGENT_FD=str(fd), RUSTY_AGENT_OUT=self.d + "/nodir/o")
        os.close(fd)
        self.assertEqual(recs, lines(self.d + "/fd.jsonl"))
        self.assertEqual(1, codes(recs).count("G0012"))

    def test_both_bad(self):
        rc, out, recs, err = run(self.args, self.d, RUSTY_AGENT_FD="57", RUSTY_AGENT_OUT=self.d + "/nodir/o")
        self.assertEqual(["G0012", "G0013"], sorted(c for c in codes(recs) if c in ("G0012", "G0013")))
        self.assertEqual(2, rc)

    def test_agent_off_ignores_fd(self):
        fd = self.fd("off.jsonl")
        off = run(self.args, self.d, agent=False, pass_fds=(fd,), RUSTY_AGENT_FD=str(fd))
        plain = run(self.args, self.d, agent=False)
        os.close(fd)
        self.assertEqual((plain[0], plain[1], plain[3]), (off[0], off[1], off[3]))
        self.assertEqual(0, os.path.getsize(self.d + "/off.jsonl"))
        bad = run(self.args, self.d, agent=False, RUSTY_AGENT_FD="57")
        self.assertEqual((plain[0], plain[1], plain[3]), (bad[0], bad[1], bad[3]))
        self.assertNotIn("G0013", bad[3])

    def test_no_agent_mode_beats_env(self):
        fd = self.fd("off.jsonl")
        rc, out, recs, err = run(["--no-agent-mode"] + self.args, self.d, pass_fds=(fd,), RUSTY_AGENT_FD=str(fd))
        os.close(fd)
        self.assertEqual(0, os.path.getsize(self.d + "/off.jsonl"))
        self.assertEqual([], recs)

    def test_children_do_not_inherit(self):
        # the --pre program and the -z decompression program must not see the variable
        dump = self.d + "/env.txt"
        pre = self.d + "/pre.sh"
        with open(pre, "w") as f:
            f.write("#!/bin/sh\nenv | grep '^RUSTY_AGENT' >> %s\ncat \"$1\"\n" % dump)
        os.chmod(pre, 0o755)
        fd = self.fd()
        rc, out, recs, err = run(["--pre", pre, "needle", "a.txt"], self.d, pass_fds=(fd,), RUSTY_AGENT_FD=str(fd))
        self.assertEqual(0, rc)
        with open(dump) as f:
            env = f.read()
        self.assertIn("RUSTY_AGENT=1", env)
        self.assertNotIn("RUSTY_AGENT_FD", env)
        os.remove(dump)
        # git children: --git-tracked runs git in the folder
        subprocess.run(["git", "init", "-q"], cwd=self.d, check=True)
        subprocess.run(["git", "add", "a.txt"], cwd=self.d, check=True)
        shim = self.d + "/bin"
        os.mkdir(shim)
        with open(shim + "/git", "w") as f:
            f.write("#!/bin/sh\nenv | grep '^RUSTY_AGENT' >> %s\nexec /usr/bin/git \"$@\"\n" % dump)
        os.chmod(shim + "/git", 0o755)
        rc, out, recs, err = run(["--git-tracked", "needle"], self.d, pass_fds=(fd,), RUSTY_AGENT_FD=str(fd),
                                 PATH=shim + ":" + os.environ["PATH"])
        os.close(fd)
        self.assertEqual(0, rc, err)
        with open(dump) as f:
            env = f.read()
        self.assertTrue(env.strip(), "the git shim did not run")
        self.assertNotIn("RUSTY_AGENT_FD", env)

    def test_docs(self):
        rc, out, recs, err = run(["--agent-help"], self.d, agent=False)
        h = json.loads(out)
        self.assertIn("RUSTY_AGENT_FD", h["switch"]["records_fd"])
        self.assertIn("G0013", h["switch"]["records_fd"])
        c = {x["code"]: x for x in h["codes"]}
        self.assertIn("G0013", c)
        self.assertEqual("warning", c["G0013"]["severity"])
        with open(ROOT + "/AGENT-MODE.md") as f:
            doc = f.read()
        self.assertIn("RUSTY_AGENT_FD", doc)
        self.assertIn("G0013", doc)


if __name__ == "__main__":
    unittest.main(argv=[sys.argv[0]] + sys.argv[1:], verbosity=1)
