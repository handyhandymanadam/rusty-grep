"""Tests of the agent-mode fixes (branch agent-fixes), run against the real program.

    HW=/path/to/rg python3 -I tests/agent_fixes.py [-v] [TestName ...]

Every test builds its own files in a fresh temporary folder, runs the real
binary, and parses the JSON records it prints. `cargo test` runs this file too
(tests/agent.rs, test `agent_python`). The folder used for scratch files is
$TMPDIR (set it to something other than a tmpfs when the files are big).
"""
import hashlib
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


def env_for(agent=True, **extra):
    env = {k: v for k, v in os.environ.items() if not k.startswith("RUSTY_")}
    env.pop("RIPGREP_CONFIG_PATH", None)
    if agent:
        env["RUSTY_AGENT"] = "1"
    env.update(extra)
    return env


def run(args, cwd, agent=True, stdout=subprocess.PIPE, **extra):
    """Run the program; return (rc, stdout bytes, parsed JSON records, stderr text)."""
    p = subprocess.run([HW] + list(args), cwd=cwd, env=env_for(agent, **extra),
                       stdin=subprocess.DEVNULL, stdout=stdout, stderr=subprocess.PIPE)
    err = p.stderr.decode("utf-8", "replace")
    recs = [json.loads(l) for l in err.splitlines() if l.startswith("{")]
    return p.returncode, p.stdout, recs, err


def kinds(recs, t):
    return [r for r in recs if r.get("type") == t]


def summary(recs):
    s = kinds(recs, "summary")
    assert len(s) == 1, s
    return s[0]


class Tree(unittest.TestCase):
    def setUp(self):
        self.d = tempfile.mkdtemp(prefix="hwtest-", dir=os.environ.get("TMPDIR"))
        self.addCleanup(lambda: self.rm())

    def rm(self):
        for root, dirs, files in os.walk(self.d):
            for n in dirs + files:
                try:
                    os.chmod(os.path.join(root, n), 0o755 if n in dirs else 0o644)
                except OSError:
                    pass
        shutil.rmtree(self.d, ignore_errors=True)

    def w(self, rel, text="", mode="w"):
        path = os.path.join(self.d, rel)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, mode) as f:
            f.write(text)
        return path

    def tree(self):
        """A visible file, a hidden one, an ignored one, a binary one."""
        os.makedirs(os.path.join(self.d, ".git"), exist_ok=True)
        self.w(".gitignore", "ign/\n")
        self.w("a.txt", "needle one\n")
        self.w(".hid/h.txt", "needle hidden\n")
        self.w("ign/i.txt", "needle ignored\n")
        self.w("sub/s.rs", "needle rust\n")
        self.w("b.bin", b"needle \x00 binary\n", "wb")


class KeyOrder(Tree):
    """Fix 1 (rule C): schema and type first in every record."""

    def test_order(self):
        self.tree()
        self.w("c.txt", "line one\nneedle two\nline three\n")
        _, out, recs, _ = run(["--records", "-C1", "needle", "c.txt", "zz_missing"], self.d)
        results = [json.loads(l) for l in out.decode().splitlines()]
        self.assertTrue(kinds(results, "result") and kinds(results, "context"))
        for r in results + recs:
            self.assertEqual(["schema", "type"], list(r)[:2], r)
        for d in kinds(recs, "diagnostic"):
            self.assertEqual(["schema", "type", "code", "severity", "message"], list(d)[:5])
        # The same at every detail level.
        for lvl in ("brief", "normal", "full"):
            _, _, recs, _ = run(["--agent-detail=" + lvl, "needle", "c.txt", "zz_missing"], self.d)
            for r in recs:
                self.assertEqual(["schema", "type"], list(r)[:2], (lvl, r))
            if lvl != "full":
                self.assertEqual(["schema", "type", "match_lines"], list(summary(recs))[:3])


class Accounting(Tree):
    """Fixes 2, 3 and the lite counts."""

    def test_many_matches_still_count_skipped(self):
        # Fix 2: with 100 or more matches the files are still counted, and the
        # biggest skipped subtrees named.
        self.tree()
        self.w("many.txt", "needle\n" * 150)
        _, _, recs, _ = run(["-c", "needle"], self.d)
        s = summary(recs)
        acc = s["accounting"]
        self.assertGreaterEqual(s["match_lines"], 150)
        self.assertEqual(3, acc["skipped_files"])
        self.assertTrue(acc["skipped_note"].startswith("reasons not worked out"))
        unclassified = [g for g in acc["skipped"] if g["reason"] == "unclassified"]
        self.assertEqual(1, len(unclassified))
        self.assertEqual(3, unclassified[0]["files"])
        paths = [e["path"] for e in unclassified[0]["examples"]]
        self.assertIn(".hid/", paths)
        self.assertIn("ign/", paths)
        # --accounting works the reasons out.
        _, _, recs, _ = run(["--accounting", "-c", "needle"], self.d)
        reasons = {g["reason"] for g in summary(recs)["accounting"]["skipped"]}
        self.assertTrue({"hidden", "ignored-vcs", "binary"} <= reasons, reasons)

    def test_nothing_skipped_has_no_accounting(self):
        self.w("a.txt", "needle\n")
        _, _, recs, _ = run(["needle"], self.d)
        self.assertNotIn("accounting", summary(recs))

    def test_several_names_only_relevant_options(self):
        self.tree()
        # Hidden AND ignored: needs both options, never --follow.
        self.w(".gitignore", "ign/\n.both/\n")
        self.w(".both/x.txt", "needle both\n")
        _, _, recs, _ = run(["--accounting", "zzzz"], self.d)
        several = [g for g in summary(recs)["accounting"]["skipped"] if g["reason"] == "several"]
        self.assertEqual(1, len(several), summary(recs)["accounting"])
        self.assertEqual("--hidden --no-ignore", several[0]["option"])
        # The option really brings the file back.
        _, out, _, _ = run(["--hidden", "--no-ignore", "needle", ".both"], self.d)
        self.assertIn(b"needle both", out)
        # A hidden symbolic link to a folder also needs --follow.
        os.makedirs(os.path.join(self.d, "real"))
        self.w("real/r.txt", "needle linked\n")
        os.symlink(os.path.join(self.d, "real"), os.path.join(self.d, ".lnk"))
        _, _, recs, _ = run(["--accounting", "zzzz"], self.d)
        opts = {g["option"] for g in summary(recs)["accounting"]["skipped"] if g["reason"] == "several"}
        self.assertTrue(any("--follow" in o for o in opts), opts)


class Counts(Tree):
    """Fixes 4 and 5."""

    def test_lines_and_occurrences(self):
        self.w("a.txt", "needle needle needle\nnone\nneedle\n")
        _, _, recs, _ = run(["-n", "needle"], self.d)
        s = summary(recs)
        self.assertEqual(2, s["match_lines"])
        self.assertNotIn("match_occurrences", s)
        self.assertNotIn("matches", s)
        _, _, recs, _ = run(["--agent-detail=full", "-n", "needle"], self.d)
        s = summary(recs)
        self.assertEqual(2, s["match_lines"])
        self.assertEqual(4, s["match_occurrences"])
        # --stats keeps the exact count in normal too.
        rc, out, recs, _ = run(["--stats", "-c", "needle"], self.d)
        self.assertIn(b"2 matched lines", out)

    def test_bytes_searched_for_terms(self):
        self.w("a.txt", "alpha\nbeta\nalpha beta\n")
        self.w("b.txt", "alpha only\n" * 100)
        for flags in (["--all-in-file"], ["--within", "2"]):
            _, _, recs, _ = run(flags + ["-e", "alpha", "-e", "beta"], self.d)
            s = summary(recs)
            self.assertGreater(s["bytes_searched"], 0, flags)
            self.assertEqual(2, s["files_searched"])


class Diagnostics(Tree):
    def test_nothing_searched(self):
        # Fix 7 and B3: fires with or without a path, and the message says which.
        os.makedirs(os.path.join(self.d, "empty"))
        self.w("hid/.x/f", "needle\n")
        rc, _, recs, _ = run(["needle"], os.path.join(self.d, "empty"))
        d = kinds(recs, "diagnostic")
        self.assertEqual(["G0007"], [x["code"] for x in d])
        self.assertIn("no path was given", d[0]["message"])
        self.assertEqual(2, rc)                                  # as before: implicit path
        for args in (["needle", "empty"], ["needle", "hid"], ["-g", "!*", "needle", "."],
                     ["-t", "py", "needle", "."]):
            rc, _, recs, _ = run(args, self.d)
            d = kinds(recs, "diagnostic")
            self.assertEqual(["G0007"], [x["code"] for x in d], args)
            self.assertIn("given path", d[0]["message"])
            self.assertEqual(1, rc, args)
            self.assertEqual(1, summary(recs)["diagnostics"]["warnings"])
        # The suggestion is --debug; --accounting is in the summary already.
        self.assertEqual(["--debug"], [s.get("option") for s in d[0]["suggestions"]])
        # A search that read a file does not warn, and /dev/null counts as read.
        rc, _, recs, _ = run(["needle", "/dev/null"], self.d)
        self.assertEqual([], kinds(recs, "diagnostic"))

    def test_output_error_once(self):
        # Fix 6: a full disk is reported once, and the summary follows.
        self.tree()
        self.w("big.txt", "needle line of text to fill the output buffer\n" * 60000)
        for extra in ([], ["--no-stable-order"], ["-j1"], ["--limit", "5"], ["--sort", "path"]):
            with open("/dev/full", "wb") as full:
                rc, _, recs, _ = run(extra + ["needle"], self.d, stdout=full)
            self.assertEqual(2, rc, extra)
            g9 = [r for r in kinds(recs, "diagnostic") if r["code"] == "G0009"]
            self.assertEqual(1, len(g9), (extra, recs))
            self.assertEqual([], [r for r in kinds(recs, "diagnostic") if r["code"] == "G0002"], extra)
            self.assertEqual(1, len(kinds(recs, "summary")), extra)
            self.assertEqual(recs[-1]["type"], "summary")

    def test_no_agent_mode_wins_for_option_errors(self):
        # B4
        self.w("a.txt", "x\n")
        _, _, recs, err = run(["--no-agent-mode", "-A", "x", "alpha", "a.txt"], self.d)
        self.assertEqual([], recs)
        self.assertTrue(err.startswith("hw: "), err)
        _, _, recs, err = run(["-A", "x", "alpha", "a.txt", "--no-agent-mode"], self.d)
        self.assertEqual([], recs)
        # Without it, the environment gives a record.
        _, _, recs, _ = run(["-A", "x", "alpha", "a.txt"], self.d)
        self.assertEqual("G0004", recs[0]["code"])
        # --agent-mode after --no-agent-mode turns it on again.
        _, _, recs, _ = run(["--no-agent-mode", "--agent-mode", "-A", "x", "alpha"], self.d, agent=False)
        self.assertEqual("G0004", recs[0]["code"])

    def test_normal_is_lean(self):
        # Fix 10 / rules A, B.
        self.tree()
        _, _, recs, _ = run(["needle", "a.txt", "zz_missing", "(("], self.d)
        for r in recs:
            self.assertNotIn("rendered", r)
            self.assertNotIn("symbols", r)
            for k, v in r.items():
                self.assertNotIn(v, ([], {}, None, ""), (k, r))
        d = {x["code"]: x for x in kinds(recs, "diagnostic")}
        self.assertNotIn("files", d["G0003"])                    # the message names it
        s = summary(recs)
        self.assertNotIn("patterns", s)
        self.assertNotIn("paths", s)
        self.assertNotIn("elapsed_ms", s)                        # stable mode
        self.assertNotIn("suggestions", s)
        # full is the old shape.
        _, _, recs, _ = run(["--agent-detail=full", "needle", "a.txt", "zz_missing"], self.d)
        d = kinds(recs, "diagnostic")[0]
        for k in ("files", "symbols", "notes", "locations", "suggestions", "rendered"):
            self.assertIn(k, d)
        s = summary(recs)
        for k in ("patterns", "paths", "match_lines", "match_occurrences", "accounting", "suggestions"):
            self.assertIn(k, s)
        # An invalid level is an error; one in the environment is ignored.
        rc, _, recs, _ = run(["--agent-detail=loud", "x", "a.txt"], self.d)
        self.assertEqual(2, rc)
        self.assertEqual("G0004", recs[0]["code"])
        rc, _, recs, _ = run(["needle", "a.txt"], self.d, RUSTY_AGENT_DETAIL="loud")
        self.assertEqual(0, rc)
        # The environment variable works like the flag.
        _, _, recs, _ = run(["needle", "a.txt"], self.d, RUSTY_AGENT_DETAIL="full")
        self.assertIn("patterns", summary(recs))

    def test_repeated_suggestion_given_once(self):
        self.tree()
        for i in range(5):
            p = self.w("locked/f%d.txt" % i, "needle\n")
            os.chmod(p, 0)
        _, _, recs, _ = run(["needle", "locked", "a.txt"], self.d)
        d = kinds(recs, "diagnostic")
        self.assertEqual(5, len(d))
        self.assertEqual(1, sum("suggestions" in x for x in d))
        _, _, recs, _ = run(["--agent-detail=full", "needle", "locked", "a.txt"], self.d)
        self.assertTrue(all("suggestions" in x for x in kinds(recs, "diagnostic")))
        _, _, recs, _ = run(["--agent-detail=brief", "needle", "locked", "a.txt"], self.d)
        self.assertTrue(all("notes" not in x for x in kinds(recs, "diagnostic")))

    def test_regex_error_locations_not_repeated(self):
        self.w("a.txt", "x\n")
        _, _, recs, _ = run(["(foo", "a.txt"], self.d)
        d = kinds(recs, "diagnostic")[0]
        self.assertEqual("G0001", d["code"])
        self.assertEqual(0, d["locations"][0]["pattern"])
        self.assertNotIn("notes", d)                             # the diagram repeats the location
        _, _, recs, _ = run(["--agent-detail=full", "(foo", "a.txt"], self.d)
        self.assertTrue(kinds(recs, "diagnostic")[0]["notes"])


class Budget(Tree):
    """Rule G: 12 KB soft, 20 KB hard, with a records_omitted count."""

    def make_locked(self, n):
        self.w("readable.txt", "needle\n")
        for i in range(n):
            p = self.w("locked/file_with_a_long_name_%04d.txt" % i, "needle\n")
            os.chmod(p, 0)

    def test_budget(self):
        self.make_locked(400)
        outfile = os.path.join(self.d, "all.jsonl")
        rc, _, recs, err = run(["needle", "locked", "readable.txt"], self.d, RUSTY_AGENT_OUT=outfile)
        shown = kinds(recs, "diagnostic")
        self.assertTrue(0 < len(shown) < 400, len(shown))
        self.assertLess(sum(len(l) + 1 for l in err.splitlines() if l.startswith("{")), 22000)
        s = summary(recs)
        self.assertEqual(400 - len(shown), s["records_omitted"]["diagnostic"])
        self.assertEqual(400, s["diagnostics"]["errors"])
        with open(outfile) as f:
            every = [json.loads(l) for l in f]
        self.assertEqual(400, len(kinds(every, "diagnostic")))
        self.assertNotIn("records_omitted", summary(every))
        # full has no cap.
        _, _, recs, _ = run(["--agent-detail=full", "needle", "locked", "readable.txt"], self.d)
        self.assertEqual(400, len(kinds(recs, "diagnostic")))
        self.assertNotIn("records_omitted", summary(recs))

    def test_sizes(self):
        """A size budget per record type (bytes of JSON, normal)."""
        self.tree()
        self.w("many.txt", "needle\n" * 150)
        self.w("c.txt", "line one\nneedle two\nline three\n")
        os.symlink(os.path.join(self.d, "sub"), os.path.join(self.d, "lnk"))
        budgets = []
        _, out, recs, _ = run(["needle", "a.txt", "zz_missing", "(("], self.d)
        for r in kinds(recs, "diagnostic"):
            budgets.append(("diagnostic " + r["code"], len(json.dumps(r, separators=(",", ":"))), 700))
        _, _, recs, _ = run(["needle"], self.d)
        budgets.append(("summary with accounting", len(json.dumps(summary(recs), separators=(",", ":"))), 1500))
        _, _, recs, _ = run(["--accounting", "zzzz"], self.d)
        budgets.append(("summary --accounting", len(json.dumps(summary(recs), separators=(",", ":"))), 1500))
        _, _, recs, _ = run(["a.txt", "-c", "needle"], self.d)
        budgets.append(("summary plain", len(json.dumps(summary(recs), separators=(",", ":"))), 400))
        _, out, _, _ = run(["--records", "-C1", "needle", "c.txt"], self.d)
        for l in out.decode().splitlines():
            budgets.append(("--records line", len(l), 400))
        for name, size, limit in budgets:
            self.assertLessEqual(size, limit, name)
        if os.environ.get("HW_SIZES"):
            for name, size, limit in budgets:
                print("size %-28s %5d (budget %d)" % (name, size, limit))


class ContextRecords(Tree):
    def test_context(self):
        # Fix 8: -A/-B/-C give context records, not silence.
        self.w("c.txt", "one\ntwo\nneedle three\nfour\nfive\nsix\nneedle seven\n")
        _, out, _, _ = run(["--records", "-C1", "needle", "c.txt"], self.d)
        rs = [json.loads(l) for l in out.decode().splitlines()]
        got = [(r["type"], r["line"]) for r in rs]
        self.assertEqual([("context", 2), ("result", 3), ("context", 4),
                          ("context", 6), ("result", 7)], got)
        self.assertEqual("two", rs[0]["line_text"])
        _, out, _, _ = run(["--records", "needle", "c.txt"], self.d)
        self.assertEqual({"result"}, {json.loads(l)["type"] for l in out.decode().splitlines()})


class TimeLimit(Tree):
    def test_walk_is_cut_too(self):
        # B2: the limit is checked while the tree is walked, with partial results and G0010.
        for i in range(300):
            self.w("d%03d/f.txt" % i, "needle\n")
        rc, out, recs, _ = run(["--time-limit", "0", "-c", "needle"], self.d)
        d = kinds(recs, "diagnostic")
        self.assertEqual(["G0010"], [x["code"] for x in d])
        s = summary(recs)
        self.assertTrue(s["incomplete"])
        self.assertLess(s.get("files_searched", 0), 300)
        rc, out, recs, _ = run(["--time-limit", "30", "-c", "needle"], self.d)
        self.assertEqual(300, len(out.decode().splitlines()))
        self.assertEqual([], kinds(recs, "diagnostic"))
        self.assertNotIn("incomplete", summary(recs))

    def test_partial_results_big_tree(self):
        # The same on a tree big enough for the clock to run out during the walk.
        for i in range(3000):
            self.w("e%04d/f.txt" % i, "needle\n")
        rc, out, recs, _ = run(["--time-limit", "0.005", "-c", "needle"], self.d)
        s = summary(recs)
        if s.get("incomplete"):
            self.assertEqual("G0010", kinds(recs, "diagnostic")[0]["code"])
            self.assertLess(s.get("files_searched", 0), 3000)
        self.assertNotIn("G0007", [x["code"] for x in kinds(recs, "diagnostic")])


class Git(Tree):
    def git(self, cwd, *args):
        subprocess.run(["git", "-C", cwd, *args], check=True, stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL,
                       env=dict(env_for(False), GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@t",
                                GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@t",
                                GIT_CONFIG_GLOBAL="/dev/null"))

    def test_search_path_decides_the_repository(self):
        # B5: run from a folder that is no repository, search one that is.
        repo = os.path.join(self.d, "repo")
        os.makedirs(repo)
        self.git(repo, "init", "-q")
        self.w("repo/tracked.txt", "needle tracked\n")
        self.git(repo, "add", "tracked.txt")
        self.git(repo, "commit", "-q", "-m", "x")
        self.w("repo/new.txt", "needle new\n")
        self.w("repo/tracked.txt", "needle tracked changed\n")
        outside = os.path.join(self.d, "outside")
        os.makedirs(outside)
        rc, out, recs, err = run(["--git-tracked", "needle", repo], outside)
        self.assertEqual(0, rc, err)
        self.assertEqual({"tracked.txt"}, {os.path.basename(l.split(b":")[0].decode()) for l in out.splitlines()})
        rc, out, recs, err = run(["--git-modified", "-l", "needle", repo], outside)
        self.assertEqual({"new.txt", "tracked.txt"}, {os.path.basename(l) for l in out.decode().split()})
        # A relative path and a path inside the repository.
        rc, out, _, _ = run(["--git-tracked", "-l", "needle", "../repo"], outside)
        self.assertEqual(["tracked.txt"], [os.path.basename(l) for l in out.decode().split()])
        rc, out, _, _ = run(["--git-tracked", "-l", "needle", "repo"], self.d)
        self.assertEqual(["tracked.txt"], [os.path.basename(l) for l in out.decode().split()])
        # No repository at the searched path: G0011 as before.
        rc, _, recs, _ = run(["--git-tracked", "needle", outside], self.d)
        self.assertEqual(2, rc)
        self.assertEqual("G0011", recs[0]["code"])


class StableOrder(Tree):
    def make(self, files, lines):
        for i in range(files):
            self.w("d%02d/f%02d.txt" % (i % 7, i), "".join("needle %d %d line text to make it longer\n" % (i, j) for j in range(lines)))

    def test_same_as_sort_path(self):
        self.make(60, 20)
        _, want, _, _ = run(["--sort", "path", "-n", "needle", "."], self.d, agent=False)
        for _ in range(5):
            _, got, _, _ = run(["-n", "needle", "."], self.d)
            self.assertEqual(want, got)
        _, want, _, _ = run(["--sort", "path", "-c", "needle", "."], self.d, agent=False)
        _, got, _, _ = run(["-c", "needle", "."], self.d)
        self.assertEqual(want, got)

    def test_huge_output_falls_back(self):
        # More than 64 MB of output cannot be held: the sorted search takes over, unchanged.
        line = ("needle " + "x" * 120 + "\n")
        for name in ("b.txt", "a.txt", "c/d.txt"):
            self.w(name, line * 200000)                          # 3 files, 25 MB each: 76 MB out
        _, want, _, _ = run(["--sort", "path", "needle", "."], self.d, agent=False)
        rc, got, recs, _ = run(["needle", "."], self.d)
        self.assertEqual(hashlib.sha256(want).hexdigest(), hashlib.sha256(got).hexdigest())
        self.assertEqual(0, rc)
        self.assertEqual(3, summary(recs)["files_with_matches"])
        self.assertEqual(600000, summary(recs)["match_lines"])


class Lite(Tree):
    def test_line_counts_equal_stats(self):
        self.w("a.txt", "alpha\nbeta\nalpha beta\n" * 50 + "\nzzz\n")
        for flags in (["-c"], ["-c", "-v"], ["-n"], ["-c", "-w"], ["-c", "-F"], ["-c", "-U"], ["-l"], ["-q"]):
            _, out, recs, _ = run(flags + ["alpha", "a.txt"], self.d)
            _, out2, _, _ = run(["--stats"] + flags + ["alpha", "a.txt"], self.d, agent=False)
            s = summary(recs)
            if flags in (["-l"], ["-q"]):
                self.assertNotIn("match_lines", s, flags)       # stops at the first match
                continue
            want = [l for l in out2.decode().splitlines() if l.endswith("matched lines")]
            self.assertEqual(int(want[0].split()[0]), s["match_lines"], flags)


class Docs(Tree):
    def test_agent_help(self):
        # B6 and fix 7.
        rc, out, _, _ = run(["--agent-help"], self.d, agent=False)
        h = json.loads(out)
        self.assertIn("max-columns-window", h["switch"]["note"])
        self.assertIn("max-columns-window", h["streams"]["stdout"])
        codes = {c["code"]: c for c in h["codes"]}
        self.assertIn("G0008", codes)
        self.assertIn("unused", codes["G0008"]["meaning"])
        self.assertIn("path", codes["G0007"]["meaning"])
        self.assertTrue(any(o["name"] == "--agent-detail" for o in h["options"]))
        self.assertIn("context", h["records"])
        self.assertEqual(["schema", "type"], list(h)[:2])
        # Every code the program can print is listed.
        self.assertTrue({"G0001", "G0002", "G0003", "G0004", "G0005", "G0006", "G0007", "G0009",
                         "G0010", "G0011", "G0012"} <= set(codes))


if __name__ == "__main__":
    unittest.main(argv=[sys.argv[0]] + sys.argv[1:], verbosity=1)
