// Tests for agent mode (this fork): opt-in JSON records on standard error for
// programs and scripts. They run the real binary on real files.
use serde_json::Value;

use crate::util::{Dir, TestCommand};

/// A small tree where one word is found in several kinds of file, only one of
/// which a plain search reads.
fn make_tree(dir: &Dir) {
    dir.create_dir(".git");
    dir.create(".gitignore", "ign/\n");
    dir.create("a.txt", "needle in a visible file\n");
    dir.create_dir(".hid");
    dir.create(".hid/h.txt", "needle in a hidden file\n");
    dir.create_dir("ign");
    dir.create("ign/i.txt", "needle in an ignored file\n");
    dir.create_dir("sub");
    dir.create("sub/s.rs", "needle in a rust file\n");
    dir.create_bytes("b.bin", b"needle then a NUL \x00 byte\n");
}

/// Run the command; return (stdout, parsed JSON records on stderr).
fn run(cmd: &mut TestCommand) -> (String, Vec<Value>) {
    // The old tests read every field: ask for the full records (the level
    // only matters under agent mode). tests/agent_fixes.py covers `normal`.
    cmd.env("RUSTY_AGENT_DETAIL", "full");
    let out = cmd.raw_output();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let records = stderr
        .lines()
        .filter(|l| l.starts_with('{'))
        .map(|l| serde_json::from_str(l).expect("a JSON line"))
        .collect();
    (stdout, records)
}

fn group<'a>(summary: &'a Value, reason: &str) -> Option<&'a Value> {
    summary["accounting"]["skipped"]
        .as_array()?
        .iter()
        .find(|g| g["reason"] == reason)
}

rgtest_rust!(agent_accounting_groups, |dir: Dir, mut cmd: TestCommand| {
    make_tree(&dir);
    cmd.args(&["--accounting", "zzzz"]);
    let (stdout, records) = run(&mut cmd);
    assert_eq!("", stdout);
    assert_eq!(1, records.len());
    let s = &records[0];
    assert_eq!(1, s["schema"]);
    assert_eq!("summary", s["type"]);
    assert_eq!(0, s["match_lines"]);
    assert_eq!(serde_json::json!(["zzzz"]), s["patterns"]);

    let hidden = group(s, "hidden").expect("hidden group");
    assert_eq!("--hidden", hidden["option"]);
    // .gitignore itself, .git (empty here) and .hid/h.txt
    assert!(hidden["files"].as_u64().unwrap() >= 2);
    let ex = hidden["examples"].as_array().unwrap();
    assert!(ex.iter().any(|e| e["path"] == ".hid/"));

    let ignored = group(s, "ignored-vcs").expect("ignored-vcs group");
    assert_eq!("--no-ignore-vcs", ignored["option"]);
    assert_eq!(1, ignored["files"]);
    assert_eq!("ign/", ignored["examples"][0]["path"]);

    let binary = group(s, "binary").expect("binary group");
    assert_eq!("--text", binary["option"]);
    assert_eq!("b.bin", binary["examples"][0]["path"]);

    // a.txt and sub/s.rs were searched.
    assert_eq!(3, s["accounting"]["files"]);
    assert_eq!(0, s["accounting"]["errors"]["count"]);
});

// Each option the summary names must really include the skipped files.
rgtest_rust!(agent_accounting_options_work, |dir: Dir, _: TestCommand| {
    make_tree(&dir);
    let mut cmd = dir.command();
    cmd.args(&["--accounting", "needle"]);
    let (stdout, records) = run(&mut cmd);
    assert_eq!(
        "a.txt:needle in a visible file\nsub/s.rs:needle in a rust file\n",
        {
            let mut lines: Vec<&str> = stdout.lines().collect();
            lines.sort();
            lines.join("\n") + "\n"
        }
    );
    let s = &records[0];
    assert_eq!(2, s["match_lines"]);
    assert_eq!(2, s["match_occurrences"]);

    let mut cmd = dir.command();
    cmd.args(&["--hidden", "needle", ".hid"]);
    assert!(cmd.stdout().contains("hidden file"));

    for (reason, expect) in
        [("hidden", ".hid/h.txt"), ("ignored-vcs", "ign/i.txt")]
    {
        let opt = group(s, reason).unwrap()["option"].as_str().unwrap();
        let mut cmd = dir.command();
        cmd.args(&[opt, "-l", "needle"]);
        let found = cmd.stdout();
        assert!(found.contains(expect), "{opt}: {found}");
    }
});

rgtest_rust!(agent_accounting_filters, |dir: Dir, mut cmd: TestCommand| {
    make_tree(&dir);
    dir.create("sub/long.rs", &format!("{}needle\n", "x".repeat(300)));
    cmd.args(&[
        "--accounting",
        "-t",
        "rust",
        "--max-filesize",
        "100",
        "needle",
    ]);
    let (_, records) = run(&mut cmd);
    let s = &records[0];
    // s.rs is searched; long.rs is over the size limit.
    assert_eq!(1, s["accounting"]["files"]);
    assert_eq!(1, group(s, "size").unwrap()["files"]);
    assert!(group(s, "size").unwrap()["option"].is_null());
    // a.txt and b.bin are not rust files; the others have several reasons.
    assert_eq!(2, group(s, "type").unwrap()["files"]);
    assert!(group(s, "several").is_some());

    let mut cmd = dir.command();
    cmd.args(&["--accounting", "-g", "!*.txt", "needle"]);
    let (_, records) = run(&mut cmd);
    assert_eq!(1, group(&records[0], "glob").unwrap()["files"]);

    let mut cmd = dir.command();
    cmd.args(&["--accounting", "--max-depth", "1", "needle"]);
    let (_, records) = run(&mut cmd);
    assert_eq!(2, group(&records[0], "depth").unwrap()["files"]);
});

// The switch: RUSTY_AGENT=1 or --agent-mode, nothing else.
rgtest_rust!(agent_env_switch, |dir: Dir, _: TestCommand| {
    make_tree(&dir);
    let plain = {
        let mut cmd = dir.command();
        cmd.args(&["--sort", "path", "needle"]);
        run(&mut cmd)
    };
    assert!(plain.1.is_empty());

    for value in ["0", "true", "yes", "", "11"] {
        let mut cmd = dir.command();
        cmd.env("RUSTY_AGENT", value).args(&["--sort", "path", "needle"]);
        let (stdout, records) = run(&mut cmd);
        assert!(records.is_empty(), "RUSTY_AGENT={value}");
        assert_eq!(plain.0, stdout);
    }

    let mut cmd = dir.command();
    cmd.env("RUSTY_AGENT", "1").args(&["--sort", "path", "needle"]);
    let (stdout, records) = run(&mut cmd);
    assert_eq!(plain.0, stdout, "agent mode leaves standard output alone");
    assert_eq!(1, records.len());
    assert_eq!("summary", records[0]["type"]);
    assert!(group(&records[0], "hidden").is_some());

    let mut cmd = dir.command();
    cmd.args(&["--agent-mode", "--sort", "path", "needle"]);
    assert_eq!(1, run(&mut cmd).1.len());

    let mut cmd = dir.command();
    cmd.env("RUSTY_AGENT", "1").args(&[
        "--no-agent-mode",
        "--sort",
        "path",
        "needle",
    ]);
    assert!(run(&mut cmd).1.is_empty());
});

rgtest_rust!(agent_version_line, |dir: Dir, _: TestCommand| {
    let ver = |env: Option<&str>, flag: &str| {
        let mut cmd = dir.command();
        if let Some(v) = env {
            cmd.env("RUSTY_AGENT", v);
        }
        cmd.arg(flag);
        cmd.stdout()
    };
    for flag in ["--version", "-V"] {
        let off = ver(None, flag);
        assert!(!off.contains("agent mode"));
        assert!(!ver(Some("0"), flag).contains("agent mode"));
        let on = ver(Some("1"), flag);
        assert!(on.ends_with("agent mode: yes, schema 1\n"), "{on}");
        assert_eq!(off.trim_end(), on.trim_end().rsplit_once('\n').unwrap().0);
    }
});

// With plenty of matches, agent mode alone counts the skipped files and leaves
// the reasons out (more in tests/agent_fixes.py); --accounting always works
// them out.
rgtest_rust!(agent_many_matches, |dir: Dir, _: TestCommand| {
    make_tree(&dir);
    dir.create("many.txt", &"needle\n".repeat(150));
    let mut cmd = dir.command();
    cmd.arg("--agent-mode").args(&["-c", "needle"]);
    let (_, records) = run(&mut cmd);
    let s = &records[0];
    assert!(s["match_lines"].as_u64().unwrap() >= 150);
    // The files are counted; the reasons are not worked out.
    assert!(s["accounting"]["skipped_files"].as_u64().unwrap() >= 3);
    assert_eq!("unclassified", s["accounting"]["skipped"][0]["reason"]);
    assert!(s["accounting"]["skipped_note"].is_string());

    let mut cmd = dir.command();
    cmd.arg("--agent-mode").args(&["--accounting", "-c", "needle"]);
    let (_, records) = run(&mut cmd);
    assert!(group(&records[0], "hidden").is_some());
});

// --stats keeps working next to the summary, and without --stats the text
// report is not printed.
rgtest_rust!(agent_stats_unchanged, |dir: Dir, _: TestCommand| {
    make_tree(&dir);
    let mut cmd = dir.command();
    cmd.args(&["--accounting", "--sort", "path", "needle"]);
    let (stdout, _) = run(&mut cmd);
    assert!(!stdout.contains("matches"), "{stdout}");

    let mut cmd = dir.command();
    cmd.args(&["--accounting", "--stats", "--sort", "path", "needle"]);
    let (stdout, records) = run(&mut cmd);
    assert!(stdout.contains("files contained matches"), "{stdout}");
    assert_eq!(1, records.len());
});

// An unreadable directory is counted in the errors.
#[cfg(unix)]
rgtest_rust!(agent_unreadable, |dir: Dir, mut cmd: TestCommand| {
    use std::os::unix::fs::PermissionsExt;
    dir.create("ok.txt", "needle\n");
    dir.create_dir("locked");
    dir.create("locked/x.txt", "needle\n");
    let path = dir.path().join("locked");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))
        .unwrap();
    // Running as root would read it anyway; then there is nothing to test.
    let readable = std::fs::read_dir(&path).is_ok();
    cmd.args(&["--accounting", "needle"]);
    let (_, records) = run(&mut cmd);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .unwrap();
    if !readable {
        let errors = &records[0]["accounting"]["errors"];
        assert_eq!(1, errors["count"]);
        assert!(errors["examples"][0].as_str().unwrap().contains("locked"));
    }
});

// Searching standard input has no tree to walk.
rgtest_rust!(agent_stdin, |_: Dir, mut cmd: TestCommand| {
    cmd.args(&["--accounting", "needle", "-"]);
    let out = cmd.pipe(b"a needle\n");
    assert_eq!("a needle\n", out);
});

fn make_pages(dir: &Dir) {
    for f in 0..5 {
        let mut text = String::new();
        for l in 0..9 {
            if l % 3 == 0 {
                text.push_str(&format!("hit {f}.{l}\n"));
            } else {
                text.push_str(&format!("filler {f}.{l}\n"));
            }
        }
        dir.create(format!("f{f}.txt"), &text);
    }
    dir.create_dir("sub");
    dir.create("sub/g.txt", "hit g.0\nhit g.1\n");
}

fn limit_of(records: &[Value]) -> &Value {
    &records[0]["limit"]
}

// Pages follow each other: every match exactly once, in the sorted order,
// and the summary says how to get the next page.
rgtest_rust!(agent_limit_paging, |dir: Dir, _: TestCommand| {
    make_pages(&dir);
    let mut cmd = dir.command();
    cmd.args(&["--sort", "path", "-n", "hit"]);
    let full: Vec<String> =
        cmd.stdout().lines().map(|l| l.to_string()).collect();
    assert_eq!(17, full.len());

    for page in [1u64, 2, 3, 4, 5, 6, 17, 40] {
        let mut got: Vec<String> = Vec::new();
        let mut offset = 0u64;
        let mut rounds = 0;
        loop {
            let mut cmd = dir.command();
            cmd.args(&[
                "--limit",
                &page.to_string(),
                "--offset",
                &offset.to_string(),
                "-n",
                "hit",
            ]);
            let (stdout, records) = run(&mut cmd);
            let lim = limit_of(&records);
            assert_eq!(17, lim["total"], "page {page}");
            assert_eq!(6, lim["total_files"]);
            assert_eq!("lines", lim["unit"]);
            assert_eq!(true, lim["exact"]);
            let lines: Vec<String> =
                stdout.lines().map(|l| l.to_string()).collect();
            assert_eq!(lines.len() as u64, lim["returned"].as_u64().unwrap());
            assert!(lines.len() as u64 <= page);
            got.extend(lines);
            rounds += 1;
            match lim["next_offset"].as_u64() {
                Some(n) => offset = n,
                None => break,
            }
            assert!(rounds < 30);
        }
        assert_eq!(full, got, "pages of {page}");
    }
});

// A window edge in the middle of a file keeps that file's context lines for
// the lines that are printed, and only those.
rgtest_rust!(agent_limit_context, |dir: Dir, _: TestCommand| {
    make_pages(&dir);
    // Lines 2..4 of the ordered matches: f0 has hit 0.0, 0.3, 0.6.
    let mut cmd = dir.command();
    cmd.args(&["--limit", "1", "--offset", "1", "-n", "-A1", "-B1", "hit"]);
    let (stdout, records) = run(&mut cmd);
    assert!(stdout.starts_with("f0.txt"), "{stdout}");
    let body: Vec<&str> = stdout.lines().collect();
    assert!(body.iter().any(|l| l.contains("hit 0.3")), "{stdout}");
    assert!(body.iter().any(|l| l.contains("filler 0.2")), "{stdout}");
    assert!(body.iter().any(|l| l.contains("filler 0.4")), "{stdout}");
    assert!(!body.iter().any(|l| l.contains("hit 0.0")), "{stdout}");
    assert!(!body.iter().any(|l| l.contains("hit 0.6")), "{stdout}");
    assert_eq!(1, limit_of(&records)["returned"]);
    assert_eq!(2, limit_of(&records)["next_offset"]);
});

// With --count the unit is the file.
rgtest_rust!(agent_limit_files, |dir: Dir, _: TestCommand| {
    make_pages(&dir);
    let mut cmd = dir.command();
    cmd.args(&["--limit", "2", "--offset", "1", "-c", "hit"]);
    let (stdout, records) = run(&mut cmd);
    assert_eq!("f1.txt:3\nf2.txt:3\n", stdout);
    let lim = limit_of(&records);
    assert_eq!("files", lim["unit"]);
    assert_eq!(6, lim["total"]);
    assert_eq!(3, lim["next_offset"]);
});

// The limit is global, not per file; an offset beyond the end prints nothing
// but still reports the totals.
rgtest_rust!(agent_limit_edges, |dir: Dir, _: TestCommand| {
    make_pages(&dir);
    let mut cmd = dir.command();
    cmd.args(&["--limit", "0", "hit"]);
    let (stdout, records) = run(&mut cmd);
    assert_eq!("", stdout);
    assert_eq!(17, limit_of(&records)["total"]);
    assert_eq!(0, limit_of(&records)["next_offset"]);

    let mut cmd = dir.command();
    cmd.args(&["--offset", "100", "hit"]);
    let out = cmd.raw_output();
    assert_eq!(Some(1), out.status.code());
    assert_eq!("", String::from_utf8_lossy(&out.stdout));

    let mut cmd = dir.command();
    cmd.args(&["--limit", "100", "hit"]);
    let (stdout, records) = run(&mut cmd);
    assert_eq!(17, stdout.lines().count());
    assert!(limit_of(&records)["next_offset"].is_null());

    let mut cmd = dir.command();
    cmd.args(&["--limit", "3", "--json", "hit"]);
    cmd.assert_err();
});

// Without --limit and --offset nothing changes: no summary, normal output.
rgtest_rust!(agent_limit_off, |dir: Dir, _: TestCommand| {
    make_pages(&dir);
    let mut cmd = dir.command();
    cmd.args(&["--sort", "path", "hit"]);
    let (stdout, records) = run(&mut cmd);
    assert_eq!(17, stdout.lines().count());
    assert!(records.is_empty());
});

fn text_stderr(cmd: &mut TestCommand) -> String {
    String::from_utf8_lossy(&cmd.raw_output().stderr).into_owned()
}

fn diagnostics(records: &[Value]) -> Vec<&Value> {
    records.iter().filter(|r| r["type"] == "diagnostic").collect()
}

// A bad pattern: the code, the place of the mistake in the user's own pattern,
// a fix, and `rendered` equal to what the text mode prints.
rgtest_rust!(agent_diag_regex, |dir: Dir, _: TestCommand| {
    dir.create("a.txt", "x\n");
    let mut text = dir.command();
    text.args(&["ab(cd", "a.txt"]);
    let plain = text_stderr(&mut text);
    assert!(plain.starts_with("hw: regex parse error:"), "{plain}");

    let mut cmd = dir.command();
    cmd.args(&[
        "--diagnostics-format=json",
        "-e",
        "ok",
        "-e",
        "ab(cd",
        "a.txt",
    ]);
    let out = cmd.raw_output();
    assert_eq!(Some(2), out.status.code());
    assert_eq!("", String::from_utf8_lossy(&out.stdout));
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(1, stderr.lines().count(), "{stderr}");
    let d: Value = serde_json::from_str(stderr.trim_end()).unwrap();
    assert_eq!(1, d["schema"]);
    assert_eq!("diagnostic", d["type"]);
    assert_eq!("G0001", d["code"]);
    assert_eq!("fatal", d["severity"]);
    assert_eq!("regex parse error: unclosed group", d["message"]);
    let loc = &d["locations"][0];
    assert_eq!(1, loc["pattern"]); // the second pattern, not the first
    assert_eq!("ab(cd", loc["text"]);
    assert_eq!(2, loc["start"]);
    assert_eq!(3, loc["end"]);
    assert_eq!("-F", d["suggestions"][0]["option"]);
    // Same words as the text mode (the only difference is the pattern list).
    assert!(
        d["rendered"].as_str().unwrap().starts_with("hw: regex parse error:")
    );
});

rgtest_rust!(agent_diag_lookaround, |dir: Dir, mut cmd: TestCommand| {
    dir.create("a.txt", "x\n");
    cmd.args(&["--agent-mode", "(?<=a)b", "a.txt"]);
    let (_, records) = run(&mut cmd);
    let d = diagnostics(&records)[0];
    assert_eq!("G0001", d["code"]);
    assert!(d["message"].as_str().unwrap().contains("look-around"));
    let opt = &d["suggestions"][0]["option"];
    assert_eq!(cfg!(feature = "pcre2"), opt == "-P", "{d}");
});

rgtest_rust!(agent_diag_files, |dir: Dir, mut cmd: TestCommand| {
    dir.create("ok.txt", "needle\n");
    cmd.args(&["--agent-mode", "needle", "ok.txt", "missing.txt"])
        .env("RUSTY_AGENT_DETAIL", "full");
    let out = cmd.raw_output();
    assert_eq!(Some(2), out.status.code());
    assert_eq!("ok.txt:needle\n", String::from_utf8_lossy(&out.stdout));
    let (_, records) = {
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        let recs: Vec<Value> =
            stderr.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        ((), recs)
    };
    let d = diagnostics(&records)[0];
    assert_eq!("G0003", d["code"]);
    assert_eq!("error", d["severity"]);
    assert_eq!("missing.txt", d["files"][0]);
    let s = records.last().unwrap();
    assert_eq!("summary", s["type"]);
    assert_eq!(2, s["exit"]);
    assert_eq!(1, s["diagnostics"]["errors"]);
    assert_eq!(1, s["accounting"]["errors"]["count"]);
});

#[cfg(unix)]
rgtest_rust!(agent_diag_unreadable, |dir: Dir, mut cmd: TestCommand| {
    use std::os::unix::fs::PermissionsExt;
    dir.create("ok.txt", "needle\n");
    dir.create("locked.txt", "needle\n");
    let path = dir.path().join("locked.txt");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))
        .unwrap();
    let readable = std::fs::File::open(&path).is_ok();
    cmd.args(&["--diagnostics-format=json", "needle"]);
    let (_, records) = run(&mut cmd);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
        .unwrap();
    if !readable {
        let d = diagnostics(&records)[0];
        assert_eq!("G0002", d["code"]);
        assert_eq!("locked.txt", d["files"][0]);
        assert!(
            d["suggestions"][0]["message"]
                .as_str()
                .unwrap()
                .contains("permission")
        );
    }
});

rgtest_rust!(agent_diag_options, |dir: Dir, _: TestCommand| {
    dir.create("a.txt", "x\n");
    // Bad flag, named in the environment switch only.
    let mut cmd = dir.command();
    cmd.env("RUSTY_AGENT", "1").args(&["--bogus-flag", "x"]);
    let (_, records) = run(&mut cmd);
    assert_eq!("G0004", records[0]["code"]);
    assert_eq!("fatal", records[0]["severity"]);
    assert_eq!("--help", records[0]["suggestions"][0]["option"]);

    // Bad flag with the flag on the command line.
    let mut cmd = dir.command();
    cmd.args(&["--agent-mode", "--bogus-flag", "x"]);
    let (_, records) = run(&mut cmd);
    assert_eq!("G0004", records[0]["code"]);

    // Missing pattern file.
    let mut cmd = dir.command();
    cmd.args(&["--agent-mode", "-f", "nope.txt", "a.txt"]);
    let (_, records) = run(&mut cmd);
    assert_eq!("G0005", records[0]["code"]);
    assert_eq!("nope.txt", records[0]["files"][0]);

    // An explicit "text" wins over the umbrella switch; --no-messages is quiet.
    let mut cmd = dir.command();
    cmd.args(&["--agent-mode", "--diagnostics-format=text", "(", "a.txt"]);
    let stderr = text_stderr(&mut cmd);
    assert!(stderr.starts_with("hw: regex parse error:"), "{stderr}");
    let mut cmd = dir.command();
    cmd.args(&["--agent-mode", "--no-messages", "needle", "missing.txt"]);
    let (_, records) = run(&mut cmd);
    assert!(diagnostics(&records).is_empty());

    // Unknown choice.
    let mut cmd = dir.command();
    cmd.args(&["--diagnostics-format=xml", "x"]);
    cmd.assert_err();
});

// "No files were searched": a warning that points at --accounting.
rgtest_rust!(agent_diag_nothing_searched, |dir: Dir, mut cmd: TestCommand| {
    dir.create_dir(".git");
    dir.create(".gitignore", "*\n");
    dir.create("a.txt", "needle\n");
    cmd.args(&["--diagnostics-format=json", "needle"]);
    let (_, records) = run(&mut cmd);
    let d = diagnostics(&records)[0];
    assert_eq!("G0007", d["code"]);
    assert_eq!("warning", d["severity"]);
    let opts: Vec<&Value> =
        d["suggestions"].as_array().unwrap().iter().map(|s| &s["option"]).collect();
    assert!(opts.contains(&&Value::from("--accounting")), "{opts:?}");
    assert!(opts.contains(&&Value::from("--debug")), "{opts:?}");
});

// Text messages are the same as ever when the options are off.
rgtest_rust!(agent_diag_text_unchanged, |dir: Dir, _: TestCommand| {
    dir.create("ok.txt", "needle\n");
    let mut cmd = dir.command();
    cmd.args(&["needle", "ok.txt", "missing.txt"]);
    let stderr = text_stderr(&mut cmd);
    assert_eq!(
        "hw: missing.txt: No such file or directory (os error 2)\n",
        stderr
    );
});

// Agent mode prints in a fixed order, the same as --sort path, run after run;
// held-back diagnostics come out sorted.
rgtest_rust!(agent_stable_order, |dir: Dir, _: TestCommand| {
    for i in 0..30 {
        dir.create_dir(format!("d{i:02}"));
        for j in 0..3 {
            dir.create(
                format!("d{i:02}/f{j}.txt"),
                &format!("needle {i} {j}\n"),
            );
        }
    }
    let mut cmd = dir.command();
    cmd.args(&["--sort", "path", "-n", "needle"]);
    let want = cmd.stdout();
    assert_eq!(90, want.lines().count());
    let mut first_err = None;
    for _ in 0..8 {
        let mut cmd = dir.command();
        cmd.arg("--agent-mode").args(&["-n", "needle", ".", "zz2", "zz1"]);
        let out = cmd.raw_output();
        let stdout = String::from_utf8_lossy(&out.stdout).replace("./", "");
        assert_eq!(want, stdout);
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        let diag: Vec<Value> = stderr
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .filter(|r: &Value| r["type"] == "diagnostic")
            .collect();
        assert_eq!("zz1", diag[0]["files"][0]);
        assert_eq!("zz2", diag[1]["files"][0]);
        let norm = regex_free_strip_elapsed(&stderr);
        match first_err {
            None => first_err = Some(norm),
            Some(ref e) => assert_eq!(e, &norm),
        }
    }
    // The order can be turned off again.
    let mut cmd = dir.command();
    cmd.args(&["--agent-mode", "--no-stable-order", "-c", "needle"]);
    assert_eq!(30 * 3, cmd.stdout().lines().count());
});

fn regex_free_strip_elapsed(s: &str) -> String {
    let mut out = String::new();
    for line in s.lines() {
        let mut v: Value = serde_json::from_str(line).unwrap();
        if let Some(o) = v.as_object_mut() {
            o.remove("elapsed_ms");
            if let Some(a) =
                o.get_mut("accounting").and_then(|a| a.as_object_mut())
            {
                a.remove("elapsed_ms");
            }
        }
        out.push_str(&v.to_string());
        out.push('\n');
    }
    out
}

// A very long line is cut to a window around its first match, with the cuts
// marked; short lines and lines without a limit are untouched.
rgtest_rust!(agent_long_line_window, |dir: Dir, _: TestCommand| {
    let long = format!(
        "{}needle{}é{}",
        "x".repeat(1000),
        "y".repeat(500),
        "z".repeat(1000)
    );
    dir.create("min.js", &format!("a needle here\n{long}\n"));

    let mut cmd = dir.command();
    cmd.args(&["--max-columns-window", "-M", "60", "-n", "needle", "min.js"]);
    let out = cmd.stdout();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!("1:a needle here", lines[0]);
    assert!(lines[1].starts_with("2:[... "), "{}", lines[1]);
    assert!(lines[1].contains("bytes omitted] "), "{}", lines[1]);
    assert!(lines[1].contains("needle"), "{}", lines[1]);
    assert!(lines[1].contains("[... "), "{}", lines[1]);
    assert!(lines[1].len() < 60 + 80, "{}", lines[1].len());
    // the second line is cut on both sides: 1000-ish bytes before, much after
    assert!(lines[1].ends_with(" more bytes]"), "{}", lines[1]);

    // The cut never splits a character.
    let mut cmd = dir.command();
    cmd.args(&["--max-columns-window", "-M", "7", "-o", "é", "min.js"]);
    let out = cmd.output();
    assert!(String::from_utf8(out.stdout).is_ok());

    // Agent mode turns it on with 400 bytes; -M 0 removes the limit.
    let mut cmd = dir.command();
    cmd.arg("--agent-mode").args(&["-n", "needle", "min.js"]);
    let (stdout, _) = run(&mut cmd);
    assert!(stdout.lines().nth(1).unwrap().len() < 480, "{stdout}");
    let mut cmd = dir.command();
    cmd.arg("--agent-mode").args(&["-M", "0", "-n", "needle", "min.js"]);
    let (stdout, _) = run(&mut cmd);
    assert!(stdout.lines().nth(1).unwrap().len() > 2500);

    // Off by default.
    let mut cmd = dir.command();
    cmd.args(&["-n", "needle", "min.js"]);
    assert!(cmd.stdout().lines().nth(1).unwrap().len() > 2500);
});

fn make_terms(dir: &Dir) {
    dir.create("both.txt", "alpha one\nfiller\nfiller\nbeta two\nend\n");
    dir.create("same.txt", "alpha and beta together\nnothing\n");
    dir.create("far.txt", "alpha first\n1\n2\n3\n4\n5\n6\nbeta last\n");
    dir.create("only_a.txt", "alpha only\n");
    dir.create("only_b.txt", "beta only\n");
    dir.create("twice.txt", "alpha\nbeta\nx\nx\nx\nx\nalpha\nx\nbeta\n");
}

rgtest_rust!(agent_all_in_file, |dir: Dir, _: TestCommand| {
    make_terms(&dir);
    let mut cmd = dir.command();
    cmd.args(&["--all-in-file", "-e", "alpha", "-e", "beta"]);
    assert_eq!("both.txt\nfar.txt\nsame.txt\ntwice.txt\n", cmd.stdout());

    let mut cmd = dir.command();
    cmd.args(&[
        "--all-in-file",
        "--files-without-match",
        "-e",
        "alpha",
        "-e",
        "beta",
    ]);
    assert_eq!("only_a.txt\nonly_b.txt\n", cmd.stdout());

    // A single pattern is just "contains it".
    let mut cmd = dir.command();
    cmd.args(&["--all-in-file", "-e", "only"]);
    assert_eq!("only_a.txt\nonly_b.txt\n", cmd.stdout());

    let mut cmd = dir.command();
    cmd.args(&["--all-in-file", "-c", "-e", "alpha", "-e", "beta"]);
    cmd.assert_err();
});

rgtest_rust!(agent_within, |dir: Dir, _: TestCommand| {
    make_terms(&dir);
    // Same line only.
    let mut cmd = dir.command();
    cmd.args(&["--within", "0", "-e", "alpha", "-e", "beta"]);
    assert_eq!("same.txt:1:alpha and beta together\n", cmd.stdout());

    // Up to three lines apart: both.txt (lines 1 and 4) qualifies.
    let mut cmd = dir.command();
    cmd.args(&["--within", "3", "-e", "alpha", "-e", "beta"]);
    assert_eq!(
        "\
both.txt:1:alpha one
both.txt:4:beta two
--
same.txt:1:alpha and beta together
--
twice.txt:1:alpha
twice.txt:2:beta
--
twice.txt:7:alpha
twice.txt:9:beta
",
        cmd.stdout()
    );

    let mut cmd = dir.command();
    cmd.args(&["--within", "3", "-c", "-e", "alpha", "-e", "beta"]);
    assert_eq!("both.txt:1\nsame.txt:1\ntwice.txt:2\n", cmd.stdout());

    let mut cmd = dir.command();
    cmd.args(&["--within", "7", "-l", "-e", "alpha", "-e", "beta"]);
    assert_eq!("both.txt\nfar.txt\nsame.txt\ntwice.txt\n", cmd.stdout());

    // Not allowed together with --all-of, --json, --limit.
    for bad in [vec!["--all-of"], vec!["--json"], vec!["--limit", "3"]] {
        let mut cmd = dir.command();
        cmd.args(&["--within", "2", "-e", "alpha"]).args(&bad);
        cmd.assert_err();
    }
});

// A single file: the file name is left out as usual.
rgtest_rust!(agent_within_one_file, |dir: Dir, _: TestCommand| {
    make_terms(&dir);
    let mut cmd = dir.command();
    cmd.args(&["--within", "3", "-e", "alpha", "-e", "beta", "both.txt"]);
    assert_eq!("1:alpha one\n4:beta two\n", cmd.stdout());
});

fn record_lines(out: &str) -> Vec<Value> {
    out.lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

rgtest_rust!(agent_records, |dir: Dir, _: TestCommand| {
    dir.create("a.txt", "one\nfoo bar foo\nthree\n");
    dir.create_bytes("bad.txt", b"x \xFF foo\n");
    let mut cmd = dir.command();
    cmd.args(&["--records", "--sort", "path", "foo"]);
    let recs = record_lines(&cmd.stdout());
    assert_eq!(3, recs.len());
    let r = &recs[0];
    assert_eq!(1, r["schema"]);
    assert_eq!("result", r["type"]);
    assert_eq!("a.txt", r["path"]);
    assert_eq!(2, r["line"]);
    assert_eq!(1, r["column"]);
    assert_eq!(4, r["byte_offset"]); // after "one\n"
    assert_eq!(7, r["end_byte_offset"]);
    assert_eq!(4, r["line_offset"]);
    assert_eq!("foo", r["match"]);
    assert_eq!("foo bar foo", r["line_text"]);
    assert_eq!(9, recs[1]["column"]);
    assert_eq!(12, recs[1]["byte_offset"]);
    assert_eq!("bad.txt", recs[2]["path"]);
    assert_eq!(true, recs[2]["lossy"]);
    assert_eq!(5, recs[2]["column"]);

    // The offsets point at the text in the file.
    let text = std::fs::read(dir.path().join("a.txt")).unwrap();
    let (s, e) = (
        r["byte_offset"].as_u64().unwrap() as usize,
        r["end_byte_offset"].as_u64().unwrap() as usize,
    );
    assert_eq!(b"foo", &text[s..e]);

    // Inverted: one record per line without a match, no column.
    let mut cmd = dir.command();
    cmd.args(&["--records", "-v", "foo", "a.txt"]);
    let recs = record_lines(&cmd.stdout());
    assert_eq!(2, recs.len());
    assert!(recs[0]["column"].is_null());
    assert_eq!("one", recs[0]["line_text"]);
    assert_eq!(3, recs[1]["line"]);

    // Not with --json and friends.
    let mut cmd = dir.command();
    cmd.args(&["--records", "--json", "foo"]);
    cmd.assert_err();
});

rgtest_rust!(agent_patch, |dir: Dir, _: TestCommand| {
    dir.create(
        "a.txt",
        "one\nfoo bar foo\nthree\nfour\nfive\nsix\nseven\neight foo\nnine\nten",
    );
    dir.create("b.txt", "nothing\n");
    dir.create("c.txt", "foo\n");
    let before: Vec<Vec<u8>> = ["a.txt", "b.txt", "c.txt"]
        .iter()
        .map(|f| std::fs::read(dir.path().join(f)).unwrap())
        .collect();
    let mut cmd = dir.command();
    cmd.args(&["--sort", "path", "--patch", "-r", "BAZ", "foo"]);
    assert_eq!(
        "\
--- a/a.txt
+++ b/a.txt
@@ -1,10 +1,10 @@
 one
-foo bar foo
+BAZ bar BAZ
 three
 four
 five
 six
 seven
-eight foo
+eight BAZ
 nine
 ten
\\ No newline at end of file
--- a/c.txt
+++ b/c.txt
@@ -1,1 +1,1 @@
-foo
+BAZ
",
        cmd.stdout()
    );
    // Nothing was written.
    let after: Vec<Vec<u8>> = ["a.txt", "b.txt", "c.txt"]
        .iter()
        .map(|f| std::fs::read(dir.path().join(f)).unwrap())
        .collect();
    assert_eq!(before, after);

    // Captures, a replacement that adds lines, and two separate hunks.
    let mut text = String::from("name: x\n");
    for i in 0..10 {
        text.push_str(&format!("l{i}\n"));
    }
    text.push_str("name: y\n");
    dir.create("d.txt", &text);
    let mut cmd = dir.command();
    cmd.args(&["--patch", "-r", "id=$1\nextra", "name: (\\w)", "d.txt"]);
    assert_eq!(
        "\
--- a/d.txt
+++ b/d.txt
@@ -1,4 +1,5 @@
-name: x
+id=x
+extra
 l0
 l1
 l2
@@ -9,4 +10,5 @@
 l7
 l8
 l9
-name: y
+id=y
+extra
",
        cmd.stdout()
    );

    // --patch needs --replace.
    let mut cmd = dir.command();
    cmd.args(&["--patch", "foo"]);
    cmd.assert_err();
});

fn summary_of(records: &[Value]) -> &Value {
    records.iter().find(|r| r["type"] == "summary").expect("a summary")
}

// A search that is stopped by the clock says so, in every way of searching.
rgtest_rust!(agent_time_limit, |dir: Dir, _: TestCommand| {
    for i in 0..20 {
        dir.create(format!("f{i}.txt"), "needle\n");
    }
    let variants: [&[&str]; 6] = [
        &[],
        &["-j1"],
        &["--sort", "path"],
        &["--limit", "5"],
        &["--within", "2", "-e", "needle", "-e", "needle"],
        &["--all-in-file", "-e", "needle"],
    ];
    for extra in variants {
        let mut cmd = dir.command();
        cmd.arg("--agent-mode").args(&["--time-limit", "0"]).args(extra);
        if extra.is_empty()
            || extra[0] != "--within" && extra[0] != "--all-in-file"
        {
            cmd.arg("needle");
        }
        let out = cmd.raw_output();
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        let recs: Vec<Value> =
            stderr.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        let s = summary_of(&recs);
        assert_eq!(true, s["incomplete"], "{extra:?}: {stderr}");
        assert_eq!(0.0, s["time_limit_s"]);
        assert_eq!(0, s["files_searched"].as_u64().unwrap_or(0).min(0));
        let d = diagnostics(&recs)[0];
        assert_eq!("G0010", d["code"]);
        assert_eq!("warning", d["severity"]);
        assert_eq!(Some(1), out.status.code(), "{extra:?}");
        assert_eq!("", String::from_utf8_lossy(&out.stdout), "{extra:?}");
    }

    // A generous limit changes nothing.
    let mut cmd = dir.command();
    cmd.args(&["--time-limit", "100", "-c", "needle"]);
    let (stdout, recs) = run(&mut cmd);
    assert_eq!(20, stdout.lines().count());
    let s = summary_of(&recs);
    assert_eq!(false, s["incomplete"]);
    assert_eq!(100.0, s["time_limit_s"]);
    assert!(diagnostics(&recs).is_empty());

    // Text mode: one warning line.
    let mut cmd = dir.command();
    cmd.args(&["--time-limit", "0", "needle"]);
    let stderr = text_stderr(&mut cmd);
    assert!(
        stderr.starts_with("hw: the search stopped at the time limit"),
        "{stderr}"
    );

    let mut cmd = dir.command();
    cmd.args(&["--time-limit", "soon", "needle"]);
    cmd.assert_err();
});

fn git(dir: &Dir, args: &[&str]) -> bool {
    std::process::Command::new("git")
        .args(args)
        .current_dir(dir.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

rgtest_rust!(agent_git_filters, |dir: Dir, _: TestCommand| {
    if !git(&dir, &["init", "-q", "."]) {
        return; // no git program here
    }
    dir.create("a.txt", "needle one\n");
    dir.create("b.txt", "needle two\n");
    dir.create_dir("sub");
    dir.create("sub/s.txt", "needle sub\n");
    assert!(git(&dir, &["add", "."]));
    assert!(git(&dir, &["commit", "-q", "-m", "init"]));
    dir.create("a.txt", "needle one\nneedle changed\n");
    dir.create("new.txt", "needle new\n");
    assert!(git(&dir, &["mv", "b.txt", "c.txt"]));
    dir.create(".gitignore", "*.log\n");
    dir.create("ign.log", "needle ignored\n");

    let list = |extra: &[&str]| {
        let mut cmd = dir.command();
        cmd.args(extra).args(&["-l", "--sort", "path", "needle"]);
        cmd.stdout()
    };
    assert_eq!("a.txt\nc.txt\nnew.txt\n", list(&["--git-modified"]));
    assert_eq!("a.txt\nc.txt\nsub/s.txt\n", list(&["--git-tracked"]));
    assert_eq!("a.txt\nc.txt\n", list(&["--git-modified", "--git-tracked"]));
    // Everything, as before.
    assert!(list(&[]).contains("sub/s.txt") && list(&[]).contains("new.txt"));

    // A file named on the command line is always searched.
    let mut cmd = dir.command();
    cmd.args(&["--git-modified", "-l", "needle", "sub/s.txt"]);
    assert_eq!("sub/s.txt\n", cmd.stdout());

    // From a subdirectory the paths are relative to it, and nothing changed
    // there is not an error.
    let mut cmd = dir.command();
    cmd.current_dir("sub").args(&["--git-tracked", "needle"]);
    assert_eq!("s.txt:needle sub\n", cmd.stdout());
    let mut cmd = dir.command();
    cmd.current_dir("sub").args(&["--git-modified", "needle"]);
    let out = cmd.raw_output();
    assert_eq!(Some(1), out.status.code());

    // The skipped files are accounted for.
    let mut cmd = dir.command();
    cmd.args(&["--accounting", "--git-modified", "zzzz"]);
    let (_, records) = run(&mut cmd);
    let g = group(&records[0], "git").expect("git group");
    assert_eq!(1, g["files"]); // sub/s.txt
    assert!(g["option"].is_null());
});

rgtest_rust!(agent_git_outside_repo, |dir: Dir, _: TestCommand| {
    dir.create("a.txt", "needle\n");
    // The test directory might itself sit inside a repository; make sure it
    // does not.
    let mut cmd = dir.command();
    cmd.env("GIT_CEILING_DIRECTORIES", dir.path().parent().unwrap()).args(&[
        "--diagnostics-format=json",
        "--git-tracked",
        "needle",
    ]);
    let out = cmd.raw_output();
    assert_eq!(Some(2), out.status.code());
    assert_eq!("", String::from_utf8_lossy(&out.stdout));
    let d: Value =
        serde_json::from_str(String::from_utf8_lossy(&out.stderr).trim_end())
            .unwrap();
    assert_eq!("G0011", d["code"]);
    assert_eq!("fatal", d["severity"]);
});

rgtest_rust!(agent_help_json, |dir: Dir, _: TestCommand| {
    let mut cmd = dir.command();
    cmd.arg("--agent-help");
    let out = cmd.stdout();
    assert_eq!(1, out.lines().count());
    let h: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(1, h["schema"]);
    assert_eq!("agent-help", h["type"]);
    assert_eq!("RUSTY_AGENT=1", h["switch"]["environment"]);
    // Every option listed is a real option with its own help text.
    let mut cmd = dir.command();
    cmd.arg("--help");
    let help = cmd.stdout();
    for o in h["options"].as_array().unwrap() {
        let name = o["name"].as_str().unwrap();
        let plain = name.replace("--no-", "--");
        assert!(help.contains(&plain), "{name} is not in --help");
    }
    let names: Vec<&str> = h["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["name"].as_str().unwrap())
        .collect();
    for want in [
        "--agent-mode",
        "--accounting",
        "--limit",
        "--offset",
        "--records",
        "--patch",
        "--time-limit",
        "--git-modified",
        "--within",
    ] {
        assert!(names.contains(&want), "{want}");
    }
    let codes: Vec<&str> = h["codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"G0001") && codes.contains(&"G0300"));
    assert!(h["records"]["summary"]["fields"].as_array().unwrap().len() > 10);
    // It works without any pattern.
    assert!(dir.command().arg("--agent-help").raw_output().status.success());
});

rgtest_rust!(agent_enclosing, |dir: Dir, _: TestCommand| {
    dir.create(
        "m.py",
        "class A:\n    def m(self):\n        return needle\n\ndef f():\n    return needle\n",
    );
    dir.create(
        "x.c",
        "#include <x.h>\n\nint foo(int a)\n{\n        if (a) {\n                return needle(a);\n        }\n        return 0;\n}\n",
    );
    dir.create("notes.txt", "needle\n");
    let mut cmd = dir.command();
    cmd.args(&["--records", "--enclosing", "--sort", "path", "needle"]);
    let recs = record_lines(&cmd.stdout());
    assert_eq!(4, recs.len());
    let by = |p: &str, l: u64| {
        recs.iter().find(|r| r["path"] == p && r["line"] == l).unwrap()
    };
    let e = &by("m.py", 3)["enclosing"];
    assert_eq!("A", e[0]["name"]);
    assert_eq!("class", e[0]["kind"]);
    assert_eq!("m", e[1]["name"]);
    assert_eq!(2, e[1]["line"]);
    assert_eq!("f", by("m.py", 6)["enclosing"][0]["name"]);
    assert_eq!("foo", by("x.c", 6)["enclosing"][0]["name"]);
    assert_eq!(3, by("x.c", 6)["enclosing"][0]["line"]);
    // A text file has no symbols: the list is empty, not missing.
    assert_eq!(0, by("notes.txt", 1)["enclosing"].as_array().unwrap().len());

    // Without --enclosing the field is absent; --enclosing needs --records.
    let mut cmd = dir.command();
    cmd.args(&["--records", "--sort", "path", "needle"]);
    assert!(record_lines(&cmd.stdout())[0].get("enclosing").is_none());
    let mut cmd = dir.command();
    cmd.args(&["--enclosing", "needle"]);
    cmd.assert_err();
});

// RUSTY_AGENT_OUT: the records also go to a file, clean of matched lines.
rgtest_rust!(agent_out_file, |dir: Dir, _: TestCommand| {
    dir.create("ok.txt", "needle\n");
    let out = dir.path().join("records.jsonl");
    let out_s = out.to_str().unwrap().to_string();

    let mut cmd = dir.command();
    cmd.env("RUSTY_AGENT", "1").env("RUSTY_AGENT_OUT", &out_s).args(&[
        "-n",
        "needle",
        "ok.txt",
        "missing.txt",
    ]);
    let o = cmd.raw_output();
    let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
    assert_eq!("ok.txt:1:needle\n", String::from_utf8_lossy(&o.stdout));
    let file = std::fs::read_to_string(&out).unwrap();
    // The same records as on stderr, one per line, none of the match lines.
    assert_eq!(stderr, file);
    let recs: Vec<Value> =
        file.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!("diagnostic", recs[0]["type"]);
    assert_eq!("summary", recs.last().unwrap()["type"]);
    assert!(!file.contains("needle\\n") && !file.contains("ok.txt:1"));

    // It appends.
    let mut cmd = dir.command();
    cmd.env("RUSTY_AGENT", "1")
        .env("RUSTY_AGENT_OUT", &out_s)
        .args(&["needle", "ok.txt"]);
    cmd.raw_output();
    let again = std::fs::read_to_string(&out).unwrap();
    assert!(again.starts_with(&file));
    assert_eq!(file.lines().count() + 1, again.lines().count());

    // --records lines stay on standard output only.
    let out2 = dir.path().join("records2.jsonl");
    let mut cmd = dir.command();
    cmd.env("RUSTY_AGENT", "1")
        .env("RUSTY_AGENT_OUT", out2.to_str().unwrap())
        .args(&["--records", "needle", "ok.txt"]);
    let o = cmd.raw_output();
    assert_eq!(
        "result",
        record_lines(&String::from_utf8_lossy(&o.stdout))[0]["type"]
    );
    let f2 = std::fs::read_to_string(&out2).unwrap();
    assert!(f2.lines().all(|l| !l.contains("\"result\"")));
    assert_eq!(1, f2.lines().count()); // the summary

    // An error before the options are read is recorded too.
    let out3 = dir.path().join("records3.jsonl");
    let mut cmd = dir.command();
    cmd.env("RUSTY_AGENT", "1")
        .env("RUSTY_AGENT_OUT", out3.to_str().unwrap())
        .args(&["--bogus-flag", "x"]);
    cmd.raw_output();
    let f3 = std::fs::read_to_string(&out3).unwrap();
    assert_eq!(
        "G0004",
        serde_json::from_str::<Value>(f3.trim_end()).unwrap()["code"]
    );
});

rgtest_rust!(agent_out_file_off_or_bad, |dir: Dir, _: TestCommand| {
    dir.create("ok.txt", "needle\n");
    let out = dir.path().join("never.jsonl");
    let out_s = out.to_str().unwrap().to_string();

    // Agent mode off: nothing is written, not even created.
    for args in
        [vec!["needle", "ok.txt"], vec!["--no-agent-mode", "needle", "ok.txt"]]
    {
        let mut cmd = dir.command();
        cmd.env("RUSTY_AGENT_OUT", &out_s);
        if args[0] == "--no-agent-mode" {
            cmd.env("RUSTY_AGENT", "1");
        }
        cmd.args(&args);
        cmd.raw_output();
        assert!(!out.exists(), "{args:?}");
    }
    // Only --diagnostics-format=json is not agent mode.
    let mut cmd = dir.command();
    cmd.env("RUSTY_AGENT_OUT", &out_s).args(&[
        "--diagnostics-format=json",
        "needle",
        "missing.txt",
    ]);
    cmd.raw_output();
    assert!(!out.exists());

    // The flag turns it on without the environment variable.
    let mut cmd = dir.command();
    cmd.env("RUSTY_AGENT_OUT", &out_s).args(&[
        "--agent-mode",
        "needle",
        "ok.txt",
    ]);
    cmd.raw_output();
    assert_eq!(1, std::fs::read_to_string(&out).unwrap().lines().count());

    // An empty or unusable path is one warning; the search goes on.
    for bad in ["", dir.path().join("no/such/dir/f").to_str().unwrap()] {
        let mut cmd = dir.command();
        cmd.env("RUSTY_AGENT", "1")
            .env("RUSTY_AGENT_OUT", bad)
            .args(&["needle", "ok.txt"]);
        let o = cmd.raw_output();
        assert_eq!(Some(0), o.status.code(), "{bad:?}");
        assert_eq!("needle\n", String::from_utf8_lossy(&o.stdout));
        let recs: Vec<Value> = String::from_utf8_lossy(&o.stderr)
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let d = diagnostics(&recs);
        assert_eq!(1, d.len(), "{bad:?}");
        assert_eq!("G0012", d[0]["code"]);
        assert_eq!("warning", d[0]["severity"]);
        assert!(d[0]["message"].as_str().unwrap().contains("RUSTY_AGENT_OUT"));
        assert!(
            d[0]["suggestions"][0]["message"]
                .as_str()
                .unwrap()
                .contains("check the path")
        );
        let s = summary_of(&recs);
        assert_eq!(1, s["diagnostics"]["warnings"], "{bad:?}");
        assert_eq!(0, s["diagnostics"]["errors"]);
        assert_eq!(0, s["exit"]);
    }
    assert!(!dir.path().join("no").exists());
    // No match: exit 1 as usual, still only the warning.
    let mut cmd = dir.command();
    cmd.env("RUSTY_AGENT", "1")
        .env("RUSTY_AGENT_OUT", "")
        .args(&["zzzz", "ok.txt"]);
    assert_eq!(Some(1), cmd.raw_output().status.code());
    // Ignored when agent mode is off, even if empty.
    let mut cmd = dir.command();
    cmd.env("RUSTY_AGENT_OUT", "").args(&["needle", "ok.txt"]);
    assert_eq!("needle\n", cmd.stdout());
});

// The fixes of the agent-fixes branch are tested in Python (tests/agent_fixes.py):
// the real program on real files, the JSON parsed with `python3 -I`.
#[test]
fn agent_python() {
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/agent_fixes.py");
    let out = match std::process::Command::new("python3")
        .arg("-I")
        .arg(script)
        .env("HW", env!("CARGO_BIN_EXE_rg"))
        .output()
    {
        Ok(out) => out,
        Err(e) => {
            eprintln!("python3 is not available ({e}); skipped");
            return;
        }
    };
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

// RUSTY_AGENT_FD (branch agent-fd): the real program, a real descriptor, the JSON parsed with `python3 -I`.
#[test]
fn agent_fd_python() {
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/agent_fd.py");
    let out = match std::process::Command::new("python3")
        .arg("-I")
        .arg(script)
        .env("HW", env!("CARGO_BIN_EXE_rg"))
        .output()
    {
        Ok(out) => out,
        Err(e) => {
            eprintln!("python3 is not available ({e}); skipped");
            return;
        }
    };
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
