use crate::hay::{SHERLOCK, SHERLOCK_CRLF};
use crate::util::{Dir, TestCommand, sort_lines};

// See: https://github.com/BurntSushi/ripgrep/issues/1
rgtest!(f1_sjis, |dir: Dir, mut cmd: TestCommand| {
    dir.create_bytes(
        "foo",
        b"\x84Y\x84u\x84\x82\x84|\x84\x80\x84{ \x84V\x84\x80\x84|\x84}\x84\x83"
    );
    cmd.arg("-Esjis").arg("Шерлок Холмс");
    eqnice!("foo:Шерлок Холмс\n", cmd.stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/1
rgtest!(f1_utf16_auto, |dir: Dir, mut cmd: TestCommand| {
    dir.create_bytes(
        "foo",
        b"\xff\xfe(\x045\x04@\x04;\x04>\x04:\x04 \x00%\x04>\x04;\x04<\x04A\x04"
    );
    cmd.arg("Шерлок Холмс");
    eqnice!("foo:Шерлок Холмс\n", cmd.stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/1
rgtest!(f1_utf16_explicit, |dir: Dir, mut cmd: TestCommand| {
    dir.create_bytes(
        "foo",
        b"\xff\xfe(\x045\x04@\x04;\x04>\x04:\x04 \x00%\x04>\x04;\x04<\x04A\x04"
    );
    cmd.arg("-Eutf-16le").arg("Шерлок Холмс");
    eqnice!("foo:Шерлок Холмс\n", cmd.stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/1
rgtest!(f1_eucjp, |dir: Dir, mut cmd: TestCommand| {
    dir.create_bytes(
        "foo",
        b"\xa7\xba\xa7\xd6\xa7\xe2\xa7\xdd\xa7\xe0\xa7\xdc \xa7\xb7\xa7\xe0\xa7\xdd\xa7\xde\xa7\xe3"
    );
    cmd.arg("-Eeuc-jp").arg("Шерлок Холмс");
    eqnice!("foo:Шерлок Холмс\n", cmd.stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/1
rgtest!(f1_unknown_encoding, |_: Dir, mut cmd: TestCommand| {
    cmd.arg("-Efoobar").assert_non_empty_stderr();
});

// See: https://github.com/BurntSushi/ripgrep/issues/1
rgtest!(f1_replacement_encoding, |_: Dir, mut cmd: TestCommand| {
    cmd.arg("-Ecsiso2022kr").assert_non_empty_stderr();
});

// See: https://github.com/BurntSushi/ripgrep/issues/7
rgtest!(f7, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sherlock", SHERLOCK);
    dir.create("pat", "Sherlock\nHolmes");

    let expected = "\
For the Doctor Watsons of this world, as opposed to the Sherlock
Holmeses, success in the province of detective work must always
be, to a very large extent, the result of luck. Sherlock Holmes
";
    eqnice!(expected, cmd.arg("-fpat").arg("sherlock").stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/7
rgtest!(f7_stdin, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sherlock", SHERLOCK);

    let expected = "\
sherlock:For the Doctor Watsons of this world, as opposed to the Sherlock
sherlock:be, to a very large extent, the result of luck. Sherlock Holmes
";
    eqnice!(expected, cmd.arg("-f-").pipe(b"Sherlock"));
});

// See: https://github.com/BurntSushi/ripgrep/issues/20
rgtest!(f20_no_filename, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sherlock", SHERLOCK);
    cmd.arg("--no-filename");

    let expected = "\
For the Doctor Watsons of this world, as opposed to the Sherlock
be, to a very large extent, the result of luck. Sherlock Holmes
";
    eqnice!(expected, cmd.arg("--no-filename").arg("Sherlock").stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/34
rgtest!(f34_only_matching, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sherlock", SHERLOCK);

    let expected = "\
sherlock:Sherlock
sherlock:Sherlock
";
    eqnice!(expected, cmd.arg("-o").arg("Sherlock").stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/34
rgtest!(f34_only_matching_line_column, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sherlock", SHERLOCK);

    let expected = "\
sherlock:1:57:Sherlock
sherlock:3:49:Sherlock
";
    cmd.arg("-o").arg("--column").arg("-n").arg("Sherlock");
    eqnice!(expected, cmd.stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/45
rgtest!(f45_relative_cwd, |dir: Dir, mut cmd: TestCommand| {
    dir.create(".not-an-ignore", "foo\n/bar");
    dir.create_dir("bar");
    dir.create_dir("baz/bar");
    dir.create_dir("baz/baz/bar");
    dir.create("bar/test", "test");
    dir.create("baz/bar/test", "test");
    dir.create("baz/baz/bar/test", "test");
    dir.create("baz/foo", "test");
    dir.create("baz/test", "test");
    dir.create("foo", "test");
    dir.create("test", "test");

    cmd.arg("-l").arg("test");

    // First, get a baseline without applying ignore rules.
    let expected = "
bar/test
baz/bar/test
baz/baz/bar/test
baz/foo
baz/test
foo
test
";
    eqnice!(sort_lines(expected), sort_lines(&cmd.stdout()));

    // Now try again with the ignore file activated.
    cmd.arg("--ignore-file").arg(".not-an-ignore");
    let expected = "
baz/bar/test
baz/baz/bar/test
baz/test
test
";
    eqnice!(sort_lines(expected), sort_lines(&cmd.stdout()));

    // Now do it again, but inside the baz directory. Since the ignore file
    // is interpreted relative to the CWD, this will cause the /bar anchored
    // pattern to filter out baz/bar, which is a subtle difference between true
    // parent ignore files and manually specified ignore files.
    let mut cmd = dir.command();
    cmd.args(&["--ignore-file", "../.not-an-ignore", "-l", "test"]);
    cmd.current_dir("baz");
    let expected = "
baz/bar/test
test
";
    eqnice!(sort_lines(expected), sort_lines(&cmd.stdout()));
});

// See: https://github.com/BurntSushi/ripgrep/issues/45
rgtest!(f45_precedence_with_others, |dir: Dir, mut cmd: TestCommand| {
    dir.create(".not-an-ignore", "*.log");
    dir.create(".ignore", "!imp.log");
    dir.create("imp.log", "test");
    dir.create("wat.log", "test");

    cmd.arg("--ignore-file").arg(".not-an-ignore").arg("test");
    eqnice!("imp.log:test\n", cmd.stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/45
rgtest!(f45_precedence_internal, |dir: Dir, mut cmd: TestCommand| {
    dir.create(".not-an-ignore1", "*.log");
    dir.create(".not-an-ignore2", "!imp.log");
    dir.create("imp.log", "test");
    dir.create("wat.log", "test");

    cmd.args(&[
        "--ignore-file",
        ".not-an-ignore1",
        "--ignore-file",
        ".not-an-ignore2",
        "test",
    ]);
    eqnice!("imp.log:test\n", cmd.stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/68
rgtest!(f68_no_ignore_vcs, |dir: Dir, mut cmd: TestCommand| {
    dir.create_dir(".git");
    dir.create(".gitignore", "foo");
    dir.create(".ignore", "bar");
    dir.create("foo", "test");
    dir.create("bar", "test");

    eqnice!("foo:test\n", cmd.arg("--no-ignore-vcs").arg("test").stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/70
rgtest!(f70_smart_case, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sherlock", SHERLOCK);

    let expected = "\
sherlock:For the Doctor Watsons of this world, as opposed to the Sherlock
sherlock:be, to a very large extent, the result of luck. Sherlock Holmes
";
    eqnice!(expected, cmd.arg("-S").arg("sherlock").stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/89
rgtest!(f89_files_with_matches, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sherlock", SHERLOCK);

    cmd.arg("--null").arg("--files-with-matches").arg("Sherlock");
    eqnice!("sherlock\x00", cmd.stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/89
rgtest!(f89_files_without_match, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sherlock", SHERLOCK);
    dir.create("file.py", "foo");

    cmd.arg("--null").arg("--files-without-match").arg("Sherlock");
    eqnice!("file.py\x00", cmd.stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/89
rgtest!(f89_count, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sherlock", SHERLOCK);

    cmd.arg("--null").arg("--count").arg("Sherlock");
    eqnice!("sherlock\x002\n", cmd.stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/89
rgtest!(f89_files, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sherlock", SHERLOCK);

    eqnice!("sherlock\x00", cmd.arg("--null").arg("--files").stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/89
rgtest!(f89_match, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sherlock", SHERLOCK);

    let expected = "\
sherlock\x00For the Doctor Watsons of this world, as opposed to the Sherlock
sherlock\x00Holmeses, success in the province of detective work must always
sherlock\x00be, to a very large extent, the result of luck. Sherlock Holmes
sherlock\x00can extract a clew from a wisp of straw or a flake of cigar ash;
";
    eqnice!(expected, cmd.arg("--null").arg("-C1").arg("Sherlock").stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/109
rgtest!(f109_max_depth, |dir: Dir, mut cmd: TestCommand| {
    dir.create_dir("one");
    dir.create("one/pass", "far");
    dir.create_dir("one/too");
    dir.create("one/too/many", "far");

    cmd.arg("--maxdepth").arg("2").arg("far");
    eqnice!("one/pass:far\n", cmd.stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/124
rgtest!(f109_case_sensitive_part1, |dir: Dir, mut cmd: TestCommand| {
    dir.create("foo", "tEsT");

    cmd.arg("--smart-case").arg("--case-sensitive").arg("test").assert_err();
});

// See: https://github.com/BurntSushi/ripgrep/issues/124
rgtest!(f109_case_sensitive_part2, |dir: Dir, mut cmd: TestCommand| {
    dir.create("foo", "tEsT");
    cmd.arg("--ignore-case").arg("--case-sensitive").arg("test").assert_err();
});

// See: https://github.com/BurntSushi/ripgrep/issues/129
rgtest!(f129_matches, |dir: Dir, mut cmd: TestCommand| {
    dir.create("foo", "test\ntest abcdefghijklmnopqrstuvwxyz test");

    let expected = "foo:test\nfoo:[Omitted long matching line]\n";
    eqnice!(expected, cmd.arg("-M26").arg("test").stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/129
rgtest!(f129_context, |dir: Dir, mut cmd: TestCommand| {
    dir.create("foo", "test\nabcdefghijklmnopqrstuvwxyz");

    let expected = "foo:test\nfoo-[Omitted long context line]\n";
    eqnice!(expected, cmd.arg("-M20").arg("-C1").arg("test").stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/129
rgtest!(f129_replace, |dir: Dir, mut cmd: TestCommand| {
    dir.create("foo", "test\ntest abcdefghijklmnopqrstuvwxyz test");

    let expected = "foo:foo\nfoo:[Omitted long line with 2 matches]\n";
    eqnice!(expected, cmd.arg("-M26").arg("-rfoo").arg("test").stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/159
rgtest!(f159_max_count, |dir: Dir, mut cmd: TestCommand| {
    dir.create("foo", "test\ntest");

    eqnice!("foo:test\n", cmd.arg("-m1").arg("test").stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/159
rgtest!(f159_max_count_zero, |dir: Dir, mut cmd: TestCommand| {
    dir.create("foo", "test\ntest");

    cmd.arg("-m0").arg("test").assert_err();
});

// See: https://github.com/BurntSushi/ripgrep/issues/196
rgtest!(f196_persistent_config, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sherlock", SHERLOCK);
    cmd.arg("sherlock").arg("sherlock");

    // Make sure we get no matches by default.
    cmd.assert_err();

    // Now add our config file, and make sure it impacts ripgrep.
    dir.create(".ripgreprc", "--ignore-case");
    cmd.cmd().env("RIPGREP_CONFIG_PATH", ".ripgreprc");

    let expected = "\
For the Doctor Watsons of this world, as opposed to the Sherlock
be, to a very large extent, the result of luck. Sherlock Holmes
";
    eqnice!(expected, cmd.stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/243
rgtest!(f243_column_line, |dir: Dir, mut cmd: TestCommand| {
    dir.create("foo", "test");

    eqnice!("foo:1:1:test\n", cmd.arg("--column").arg("test").stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/263
rgtest!(f263_sort_files, |dir: Dir, mut cmd: TestCommand| {
    dir.create("foo", "test");
    dir.create("abc", "test");
    dir.create("zoo", "test");
    dir.create("bar", "test");

    let expected = "abc:test\nbar:test\nfoo:test\nzoo:test\n";
    eqnice!(expected, cmd.arg("--sort-files").arg("test").stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/263
rgtest!(f263_sort_files_reverse, |dir: Dir, mut cmd: TestCommand| {
    dir.create("foo", "test");
    dir.create("abc", "test");
    dir.create("zoo", "test");
    dir.create("bar", "test");

    let expected = "zoo:test\nfoo:test\nbar:test\nabc:test\n";
    eqnice!(expected, cmd.arg("--sortr=path").arg("test").stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/275
rgtest!(f275_pathsep, |dir: Dir, mut cmd: TestCommand| {
    dir.create_dir("foo");
    dir.create("foo/bar", "test");

    cmd.arg("test").arg("--path-separator").arg("Z");
    eqnice!("fooZbar:test\n", cmd.stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/362
rgtest!(f362_dfa_size_limit, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sherlock", SHERLOCK);

    // This should fall back to the nfa engine but should still produce the
    // expected result.
    cmd.arg("--dfa-size-limit").arg("10").arg(r"For\s").arg("sherlock");

    let expected = "\
For the Doctor Watsons of this world, as opposed to the Sherlock
";
    eqnice!(expected, cmd.stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/362
rgtest!(f362_exceeds_regex_size_limit, |dir: Dir, mut cmd: TestCommand| {
    // --regex-size-limit doesn't apply to PCRE2.
    if dir.is_pcre2() {
        return;
    }
    cmd.arg("--regex-size-limit").arg("10K").arg(r"[0-9]\w+").assert_err();
});

// See: https://github.com/BurntSushi/ripgrep/issues/362
#[cfg(target_pointer_width = "32")]
rgtest!(
    f362_u64_to_narrow_usize_overflow,
    |dir: Dir, mut cmd: TestCommand| {
        // --dfa-size-limit doesn't apply to PCRE2.
        if dir.is_pcre2() {
            return;
        }
        dir.create_size("foo", 1000000);

        // 2^35 * 2^20 is ok for u64, but not for usize
        cmd.arg("--dfa-size-limit").arg("34359738368M").arg("--files");
        cmd.assert_err();
    }
);

// See: https://github.com/BurntSushi/ripgrep/issues/411
rgtest!(
    f411_single_threaded_search_stats,
    |dir: Dir, mut cmd: TestCommand| {
        dir.create("sherlock", SHERLOCK);

        let lines = cmd.arg("-j1").arg("--stats").arg("Sherlock").stdout();
        assert!(lines.contains("Sherlock"));
        assert!(lines.contains("2 matched lines"));
        assert!(lines.contains("1 files contained matches"));
        assert!(lines.contains("1 files searched"));
        assert!(lines.contains("seconds"));
    }
);

rgtest!(f411_parallel_search_stats, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sherlock_1", SHERLOCK);
    dir.create("sherlock_2", SHERLOCK);

    let lines = cmd.arg("-j2").arg("--stats").arg("Sherlock").stdout();
    assert!(lines.contains("4 matched lines"));
    assert!(lines.contains("2 files contained matches"));
    assert!(lines.contains("2 files searched"));
    assert!(lines.contains("seconds"));
});

rgtest!(
    f411_single_threaded_quiet_search_stats,
    |dir: Dir, mut cmd: TestCommand| {
        dir.create("sherlock", SHERLOCK);

        let lines = cmd
            .arg("--quiet")
            .arg("-j1")
            .arg("--stats")
            .arg("Sherlock")
            .stdout();
        assert!(!lines.contains("Sherlock"));
        assert!(lines.contains("2 matched lines"));
        assert!(lines.contains("1 files contained matches"));
        assert!(lines.contains("1 files searched"));
        assert!(lines.contains("seconds"));
    }
);

rgtest!(f411_parallel_quiet_search_stats, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sherlock_1", SHERLOCK);
    dir.create("sherlock_2", SHERLOCK);

    let lines =
        cmd.arg("-j2").arg("--quiet").arg("--stats").arg("Sherlock").stdout();
    assert!(!lines.contains("Sherlock"));
    assert!(lines.contains("4 matched lines"));
    assert!(lines.contains("2 files contained matches"));
    assert!(lines.contains("2 files searched"));
    assert!(lines.contains("seconds"));
});

// See: https://github.com/BurntSushi/ripgrep/issues/416
rgtest!(f416_crlf, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sherlock", SHERLOCK_CRLF);
    cmd.arg("--crlf").arg(r"Sherlock$").arg("sherlock");

    let expected = "\
For the Doctor Watsons of this world, as opposed to the Sherlock\r
";
    eqnice!(expected, cmd.stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/416
rgtest!(f416_crlf_multiline, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sherlock", SHERLOCK_CRLF);
    cmd.arg("--crlf").arg("-U").arg(r"Sherlock$").arg("sherlock");

    let expected = "\
For the Doctor Watsons of this world, as opposed to the Sherlock\r
";
    eqnice!(expected, cmd.stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/416
rgtest!(f416_crlf_only_matching, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sherlock", SHERLOCK_CRLF);
    cmd.arg("--crlf").arg("-o").arg(r"Sherlock$").arg("sherlock");

    let expected = "\
Sherlock\r
";
    eqnice!(expected, cmd.stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/419
rgtest!(f419_zero_as_shortcut_for_null, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sherlock", SHERLOCK);

    cmd.arg("-0").arg("--count").arg("Sherlock");
    eqnice!("sherlock\x002\n", cmd.stdout());
});

rgtest!(f740_passthru, |dir: Dir, mut cmd: TestCommand| {
    dir.create("file", "\nfoo\nbar\nfoobar\n\nbaz\n");
    dir.create("patterns", "foo\nbar\n");

    // We can't assume that the way colour specs are translated to ANSI
    // sequences will remain stable, and --replace doesn't currently work with
    // pass-through, so for now we don't actually test the match sub-strings
    let common_args = &["-n", "--passthru"];
    let foo_expected = "\
1-
2:foo
3-bar
4:foobar
5-
6-baz
";

    // With single pattern
    cmd.args(common_args).arg("foo").arg("file");
    eqnice!(foo_expected, cmd.stdout());

    let foo_bar_expected = "\
1-
2:foo
3:bar
4:foobar
5-
6-baz
";

    // With multiple -e patterns
    let mut cmd = dir.command();
    cmd.args(common_args);
    cmd.args(&["-e", "foo", "-e", "bar", "file"]);
    eqnice!(foo_bar_expected, cmd.stdout());

    // With multiple -f patterns
    let mut cmd = dir.command();
    cmd.args(common_args);
    cmd.args(&["-f", "patterns", "file"]);
    eqnice!(foo_bar_expected, cmd.stdout());

    // -c should override
    let mut cmd = dir.command();
    cmd.args(common_args);
    cmd.args(&["-c", "foo", "file"]);
    eqnice!("2\n", cmd.stdout());

    let only_foo_expected = "\
1-
2:foo
3-bar
4:foo
5-
6-baz
";

    // -o should work
    let mut cmd = dir.command();
    cmd.args(common_args);
    cmd.args(&["-o", "foo", "file"]);
    eqnice!(only_foo_expected, cmd.stdout());

    let replace_foo_expected = "\
1-
2:wat
3-bar
4:watbar
5-
6-baz
";

    // -r should work
    let mut cmd = dir.command();
    cmd.args(common_args);
    cmd.args(&["-r", "wat", "foo", "file"]);
    eqnice!(replace_foo_expected, cmd.stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/948
rgtest!(f948_exit_code_match, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sherlock", SHERLOCK);
    cmd.arg(".");

    cmd.assert_exit_code(0);
});

// See: https://github.com/BurntSushi/ripgrep/issues/948
rgtest!(f948_exit_code_no_match, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sherlock", SHERLOCK);
    cmd.arg("NADA");

    cmd.assert_exit_code(1);
});

// See: https://github.com/BurntSushi/ripgrep/issues/948
rgtest!(f948_exit_code_error, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sherlock", SHERLOCK);
    cmd.arg("*");

    cmd.assert_exit_code(2);
});

// See: https://github.com/BurntSushi/ripgrep/issues/917
rgtest!(f917_trim, |dir: Dir, mut cmd: TestCommand| {
    const SHERLOCK: &'static str = "\
zzz
    For the Doctor Watsons of this world, as opposed to the Sherlock
  Holmeses, success in the province of detective work must always
\tbe, to a very large extent, the result of luck. Sherlock Holmes
     can extract a clew from a wisp of straw or a flake of cigar ash;
but Doctor Watson has to have it taken out for him and dusted,
 and exhibited clearly, with a label attached.
";
    dir.create("sherlock", SHERLOCK);
    cmd.args(&["-n", "-B1", "-A2", "--trim", "Holmeses", "sherlock"]);

    let expected = "\
2-For the Doctor Watsons of this world, as opposed to the Sherlock
3:Holmeses, success in the province of detective work must always
4-be, to a very large extent, the result of luck. Sherlock Holmes
5-can extract a clew from a wisp of straw or a flake of cigar ash;
";
    eqnice!(expected, cmd.stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/917
//
// This is like f917_trim, except this tests that trimming occurs even when the
// whitespace is part of a match.
rgtest!(f917_trim_match, |dir: Dir, mut cmd: TestCommand| {
    const SHERLOCK: &'static str = "\
zzz
    For the Doctor Watsons of this world, as opposed to the Sherlock
  Holmeses, success in the province of detective work must always
\tbe, to a very large extent, the result of luck. Sherlock Holmes
     can extract a clew from a wisp of straw or a flake of cigar ash;
but Doctor Watson has to have it taken out for him and dusted,
 and exhibited clearly, with a label attached.
";
    dir.create("sherlock", SHERLOCK);
    cmd.args(&["-n", "-B1", "-A2", "--trim", r"\s+Holmeses", "sherlock"]);

    let expected = "\
2-For the Doctor Watsons of this world, as opposed to the Sherlock
3:Holmeses, success in the province of detective work must always
4-be, to a very large extent, the result of luck. Sherlock Holmes
5-can extract a clew from a wisp of straw or a flake of cigar ash;
";
    eqnice!(expected, cmd.stdout());
});

rgtest!(f917_trim_multi_standard, |dir: Dir, mut cmd: TestCommand| {
    const HAYSTACK: &str = "     0123456789abcdefghijklmnopqrstuvwxyz";
    dir.create("haystack", HAYSTACK);
    cmd.args(&["--multiline", "--trim", "-r$0", "--no-filename", r"a\n?bc"]);

    let expected = "0123456789abcdefghijklmnopqrstuvwxyz\n";
    eqnice!(expected, cmd.stdout());
});

rgtest!(f917_trim_max_columns_normal, |dir: Dir, mut cmd: TestCommand| {
    const HAYSTACK: &str = "     0123456789abcdefghijklmnopqrstuvwxyz";
    dir.create("haystack", HAYSTACK);
    cmd.args(&[
        "--trim",
        "--max-columns-preview",
        "-M8",
        "--no-filename",
        "abc",
    ]);

    let expected = "01234567 [... omitted end of long line]\n";
    eqnice!(expected, cmd.stdout());
});

rgtest!(f917_trim_max_columns_matches, |dir: Dir, mut cmd: TestCommand| {
    const HAYSTACK: &str = "     0123456789abcdefghijklmnopqrstuvwxyz";
    dir.create("haystack", HAYSTACK);
    cmd.args(&[
        "--trim",
        "--max-columns-preview",
        "-M8",
        "--color=always",
        "--colors=path:none",
        "--no-filename",
        "abc",
    ]);

    let expected = "01234567 [... 1 more match]\n";
    eqnice!(expected, cmd.stdout());
});

rgtest!(
    f917_trim_max_columns_multi_standard,
    |dir: Dir, mut cmd: TestCommand| {
        const HAYSTACK: &str = "     0123456789abcdefghijklmnopqrstuvwxyz";
        dir.create("haystack", HAYSTACK);
        cmd.args(&[
            "--multiline",
            "--trim",
            "--max-columns-preview",
            "-M8",
            // Force the "slow" printing path without actually
            // putting colors in the output.
            "--color=always",
            "--colors=path:none",
            "--no-filename",
            r"a\n?bc",
        ]);

        let expected = "01234567 [... 1 more match]\n";
        eqnice!(expected, cmd.stdout());
    }
);

rgtest!(
    f917_trim_max_columns_multi_only_matching,
    |dir: Dir, mut cmd: TestCommand| {
        const HAYSTACK: &str = "     0123456789abcdefghijklmnopqrstuvwxyz";
        dir.create("haystack", HAYSTACK);
        cmd.args(&[
            "--multiline",
            "--trim",
            "--max-columns-preview",
            "-M8",
            "--only-matching",
            "--no-filename",
            r".*a\n?bc.*",
        ]);

        let expected = "01234567 [... 0 more matches]\n";
        eqnice!(expected, cmd.stdout());
    }
);

rgtest!(
    f917_trim_max_columns_multi_per_match,
    |dir: Dir, mut cmd: TestCommand| {
        const HAYSTACK: &str = "     0123456789abcdefghijklmnopqrstuvwxyz";
        dir.create("haystack", HAYSTACK);
        cmd.args(&[
            "--multiline",
            "--trim",
            "--max-columns-preview",
            "-M8",
            "--vimgrep",
            "--no-filename",
            r".*a\n?bc.*",
        ]);

        let expected = "1:1:01234567 [... 0 more matches]\n";
        eqnice!(expected, cmd.stdout());
    }
);

// See: https://github.com/BurntSushi/ripgrep/issues/993
rgtest!(f993_null_data, |dir: Dir, mut cmd: TestCommand| {
    dir.create("test", "foo\x00bar\x00\x00\x00baz\x00");
    cmd.arg("--null-data").arg(r".+").arg("test");

    // If we just used -a instead of --null-data, then the result would include
    // all NUL bytes.
    let expected = "foo\x00bar\x00baz\x00";
    eqnice!(expected, cmd.stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/1078
//
// N.B. There are many more tests in the grep-printer crate.
rgtest!(f1078_max_columns_preview1, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sherlock", SHERLOCK);
    cmd.args(&[
        "-M46",
        "--max-columns-preview",
        "exhibited|dusted|has to have it",
    ]);

    let expected = "\
sherlock:but Doctor Watson has to have it taken out for [... omitted end of long line]
sherlock:and exhibited clearly, with a label attached.
";
    eqnice!(expected, cmd.stdout());
});

rgtest!(f1078_max_columns_preview2, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sherlock", SHERLOCK);
    cmd.args(&[
        "-M43",
        "--max-columns-preview",
        // Doing a replacement forces ripgrep to show the number of remaining
        // matches. Normally, this happens by default when printing a tty with
        // colors.
        "-rxxx",
        "exhibited|dusted|has to have it",
    ]);

    let expected = "\
sherlock:but Doctor Watson xxx taken out for him and [... 1 more match]
sherlock:and xxx clearly, with a label attached.
";
    eqnice!(expected, cmd.stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/1138
rgtest!(f1138_no_ignore_dot, |dir: Dir, mut cmd: TestCommand| {
    dir.create_dir(".git");
    dir.create(".gitignore", "foo");
    dir.create(".ignore", "bar");
    dir.create(".fzf-ignore", "quux");
    dir.create("foo", "");
    dir.create("bar", "");
    dir.create("quux", "");

    cmd.arg("--sort").arg("path").arg("--files");
    eqnice!("quux\n", cmd.stdout());
    eqnice!("bar\nquux\n", cmd.arg("--no-ignore-dot").stdout());
    eqnice!("bar\n", cmd.arg("--ignore-file").arg(".fzf-ignore").stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/1155
rgtest!(f1155_auto_hybrid_regex, |dir: Dir, mut cmd: TestCommand| {
    // No sense in testing a hybrid regex engine with only one engine!
    if !dir.is_pcre2() {
        return;
    }

    dir.create("sherlock", SHERLOCK);
    cmd.arg("--no-pcre2").arg("--auto-hybrid-regex").arg(r"(?<=the )Sherlock");

    let expected = "\
sherlock:For the Doctor Watsons of this world, as opposed to the Sherlock
";
    eqnice!(expected, cmd.stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/1207
//
// Tests if without encoding 'none' flag null bytes are consumed by automatic
// encoding detection.
rgtest!(f1207_auto_encoding, |dir: Dir, mut cmd: TestCommand| {
    dir.create_bytes("foo", b"\xFF\xFE\x00\x62");
    cmd.arg("-a").arg("\\x00").arg("foo");
    cmd.assert_exit_code(1);
});

// See: https://github.com/BurntSushi/ripgrep/issues/1207
//
// Tests if encoding 'none' flag does treat file as raw bytes
rgtest!(f1207_ignore_encoding, |dir: Dir, mut cmd: TestCommand| {
    // PCRE2 chokes on this test because it can't search invalid non-UTF-8
    // and the point of this test is to search raw UTF-16.
    if dir.is_pcre2() {
        return;
    }

    dir.create_bytes("foo", b"\xFF\xFE\x00\x62");
    cmd.arg("--encoding").arg("none").arg("-a").arg("\\x00").arg("foo");
    eqnice!("\u{FFFD}\u{FFFD}\x00b\n", cmd.stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/1414
rgtest!(f1414_no_require_git, |dir: Dir, mut cmd: TestCommand| {
    dir.create(".gitignore", "foo");
    dir.create("foo", "");
    dir.create("bar", "");

    let stdout = cmd.args(&["--sort", "path", "--files"]).stdout();
    eqnice!("bar\nfoo\n", stdout);

    let stdout =
        cmd.args(&["--sort", "path", "--files", "--no-require-git"]).stdout();
    eqnice!("bar\n", stdout);

    let stdout = cmd
        .args(&[
            "--sort",
            "path",
            "--files",
            "--no-require-git",
            "--require-git",
        ])
        .stdout();
    eqnice!("bar\nfoo\n", stdout);
});

// See: https://github.com/BurntSushi/ripgrep/pull/1420
rgtest!(f1420_no_ignore_exclude, |dir: Dir, mut cmd: TestCommand| {
    dir.create_dir(".git/info");
    dir.create(".git/info/exclude", "foo");
    dir.create("bar", "");
    dir.create("foo", "");

    cmd.arg("--sort").arg("path").arg("--files");
    eqnice!("bar\n", cmd.stdout());
    eqnice!("bar\nfoo\n", cmd.arg("--no-ignore-exclude").stdout());
});

// See: https://github.com/BurntSushi/ripgrep/pull/1466
rgtest!(f1466_no_ignore_files, |dir: Dir, mut cmd: TestCommand| {
    dir.create(".myignore", "bar");
    dir.create("bar", "");
    dir.create("foo", "");

    // Test that --no-ignore-files disables --ignore-file.
    // And that --ignore-files overrides --no-ignore-files.
    cmd.arg("--sort").arg("path").arg("--files");
    eqnice!("bar\nfoo\n", cmd.stdout());
    eqnice!("foo\n", cmd.arg("--ignore-file").arg(".myignore").stdout());
    eqnice!("bar\nfoo\n", cmd.arg("--no-ignore-files").stdout());
    eqnice!("foo\n", cmd.arg("--ignore-files").stdout());

    // Test that the -u flag does not disable --ignore-file.
    let mut cmd = dir.command();
    cmd.arg("--sort").arg("path").arg("--files");
    cmd.arg("--ignore-file").arg(".myignore");
    eqnice!("foo\n", cmd.stdout());
    eqnice!("foo\n", cmd.arg("-u").stdout());
});

// See: https://github.com/BurntSushi/ripgrep/pull/2361
rgtest!(f2361_sort_nested_files, |dir: Dir, mut cmd: TestCommand| {
    use std::{thread::sleep, time::Duration};

    if crate::util::is_cross() {
        return;
    }
    dir.create("foo", "1");
    sleep(Duration::from_millis(200));
    dir.create_dir("dir");
    sleep(Duration::from_millis(200));
    dir.create(dir.path().join("dir").join("bar"), "1");

    cmd.arg("--sort").arg("accessed").arg("--files");
    eqnice!("foo\ndir/bar\n", cmd.stdout());

    dir.create("foo", "2");
    sleep(Duration::from_millis(200));
    dir.create(dir.path().join("dir").join("bar"), "2");
    sleep(Duration::from_millis(200));

    cmd.arg("--sort").arg("accessed").arg("--files");
    eqnice!("foo\ndir/bar\n", cmd.stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/1404
rgtest!(f1404_nothing_searched_warning, |dir: Dir, mut cmd: TestCommand| {
    dir.create(".ignore", "ignored-dir/**");
    dir.create_dir("ignored-dir");
    dir.create("ignored-dir/foo", "needle");

    // Test that, if ripgrep searches only ignored folders/files, then there
    // is a non-zero exit code.
    cmd.arg("needle");
    cmd.assert_err();

    // Test that we actually get an error message that we expect.
    let output = cmd.raw_output();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected = "\
        hw: No files were searched, which means ripgrep probably applied \
        a filter you didn't expect.\n\
        Running with --debug will show why files are being skipped.\n\
    ";
    eqnice!(expected, stderr);
});

// See: https://github.com/BurntSushi/ripgrep/issues/1404
rgtest!(f1404_nothing_searched_ignored, |dir: Dir, mut cmd: TestCommand| {
    dir.create(".ignore", "ignored-dir/**");
    dir.create_dir("ignored-dir");
    dir.create("ignored-dir/foo", "needle");

    // Test that, if ripgrep searches only ignored folders/files, then there
    // is a non-zero exit code.
    cmd.arg("--no-messages").arg("needle");
    cmd.assert_err();

    // But since --no-messages is given, there should not be any error message
    // printed.
    let output = cmd.raw_output();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected = "";
    eqnice!(expected, stderr);
});

// See: https://github.com/BurntSushi/ripgrep/issues/1842
rgtest!(f1842_field_context_separator, |dir: Dir, _: TestCommand| {
    dir.create("sherlock", SHERLOCK);

    // Test the default.
    let base = &["-n", "-A1", "Doctor Watsons", "sherlock"];
    let expected = "\
1:For the Doctor Watsons of this world, as opposed to the Sherlock
2-Holmeses, success in the province of detective work must always
";
    eqnice!(expected, dir.command().args(base).stdout());

    // Test that it can be overridden.
    let mut args = vec!["--field-context-separator", "!"];
    args.extend(base);
    let expected = "\
1:For the Doctor Watsons of this world, as opposed to the Sherlock
2!Holmeses, success in the province of detective work must always
";
    eqnice!(expected, dir.command().args(&args).stdout());

    // Test that it can use multiple bytes.
    let mut args = vec!["--field-context-separator", "!!"];
    args.extend(base);
    let expected = "\
1:For the Doctor Watsons of this world, as opposed to the Sherlock
2!!Holmeses, success in the province of detective work must always
";
    eqnice!(expected, dir.command().args(&args).stdout());

    // Test that unescaping works.
    let mut args = vec!["--field-context-separator", r"\x7F"];
    args.extend(base);
    let expected = "\
1:For the Doctor Watsons of this world, as opposed to the Sherlock
2\x7FHolmeses, success in the province of detective work must always
";
    eqnice!(expected, dir.command().args(&args).stdout());

    // Test that an empty separator is OK.
    let mut args = vec!["--field-context-separator", r""];
    args.extend(base);
    let expected = "\
1:For the Doctor Watsons of this world, as opposed to the Sherlock
2Holmeses, success in the province of detective work must always
";
    eqnice!(expected, dir.command().args(&args).stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/1842
rgtest!(f1842_field_match_separator, |dir: Dir, _: TestCommand| {
    dir.create("sherlock", SHERLOCK);

    // Test the default.
    let base = &["-n", "Doctor Watsons", "sherlock"];
    let expected = "\
1:For the Doctor Watsons of this world, as opposed to the Sherlock
";
    eqnice!(expected, dir.command().args(base).stdout());

    // Test that it can be overridden.
    let mut args = vec!["--field-match-separator", "!"];
    args.extend(base);
    let expected = "\
1!For the Doctor Watsons of this world, as opposed to the Sherlock
";
    eqnice!(expected, dir.command().args(&args).stdout());

    // Test that it can use multiple bytes.
    let mut args = vec!["--field-match-separator", "!!"];
    args.extend(base);
    let expected = "\
1!!For the Doctor Watsons of this world, as opposed to the Sherlock
";
    eqnice!(expected, dir.command().args(&args).stdout());

    // Test that unescaping works.
    let mut args = vec!["--field-match-separator", r"\x7F"];
    args.extend(base);
    let expected = "\
1\x7FFor the Doctor Watsons of this world, as opposed to the Sherlock
";
    eqnice!(expected, dir.command().args(&args).stdout());

    // Test that an empty separator is OK.
    let mut args = vec!["--field-match-separator", r""];
    args.extend(base);
    let expected = "\
1For the Doctor Watsons of this world, as opposed to the Sherlock
";
    eqnice!(expected, dir.command().args(&args).stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/2288
rgtest!(f2288_context_partial_override, |dir: Dir, mut cmd: TestCommand| {
    dir.create("test", "1\n2\n3\n4\n5\n6\n7\n8\n9\n");
    cmd.args(&["-C1", "-A2", "5", "test"]);
    eqnice!("4\n5\n6\n7\n", cmd.stdout());
});

// See: https://github.com/BurntSushi/ripgrep/issues/2288
rgtest!(
    f2288_context_partial_override_rev,
    |dir: Dir, mut cmd: TestCommand| {
        dir.create("test", "1\n2\n3\n4\n5\n6\n7\n8\n9\n");
        cmd.args(&["-A2", "-C1", "5", "test"]);
        eqnice!("4\n5\n6\n7\n", cmd.stdout());
    }
);

rgtest!(no_context_sep, |dir: Dir, mut cmd: TestCommand| {
    dir.create("test", "foo\nctx\nbar\nctx\nfoo\nctx");
    cmd.args(&["-A1", "--no-context-separator", "foo", "test"]);
    eqnice!("foo\nctx\nfoo\nctx\n", cmd.stdout());
});

rgtest!(no_context_sep_overrides, |dir: Dir, mut cmd: TestCommand| {
    dir.create("test", "foo\nctx\nbar\nctx\nfoo\nctx");
    cmd.args(&[
        "-A1",
        "--context-separator",
        "AAA",
        "--no-context-separator",
        "foo",
        "test",
    ]);
    eqnice!("foo\nctx\nfoo\nctx\n", cmd.stdout());
});

rgtest!(no_context_sep_overridden, |dir: Dir, mut cmd: TestCommand| {
    dir.create("test", "foo\nctx\nbar\nctx\nfoo\nctx");
    cmd.args(&[
        "-A1",
        "--no-context-separator",
        "--context-separator",
        "AAA",
        "foo",
        "test",
    ]);
    eqnice!("foo\nctx\nAAA\nfoo\nctx\n", cmd.stdout());
});

rgtest!(context_sep, |dir: Dir, mut cmd: TestCommand| {
    dir.create("test", "foo\nctx\nbar\nctx\nfoo\nctx");
    cmd.args(&["-A1", "--context-separator", "AAA", "foo", "test"]);
    eqnice!("foo\nctx\nAAA\nfoo\nctx\n", cmd.stdout());
});

rgtest!(context_sep_default, |dir: Dir, mut cmd: TestCommand| {
    dir.create("test", "foo\nctx\nbar\nctx\nfoo\nctx");
    cmd.args(&["-A1", "foo", "test"]);
    eqnice!("foo\nctx\n--\nfoo\nctx\n", cmd.stdout());
});

rgtest!(context_sep_empty, |dir: Dir, mut cmd: TestCommand| {
    dir.create("test", "foo\nctx\nbar\nctx\nfoo\nctx");
    cmd.args(&["-A1", "--context-separator", "", "foo", "test"]);
    eqnice!("foo\nctx\n\nfoo\nctx\n", cmd.stdout());
});

rgtest!(no_unicode, |dir: Dir, mut cmd: TestCommand| {
    dir.create("test", "δ");
    cmd.arg("-i").arg("--no-unicode").arg("Δ").assert_err();
});

// See: https://github.com/BurntSushi/ripgrep/issues/1790
rgtest!(stop_on_nonmatch, |dir: Dir, mut cmd: TestCommand| {
    dir.create("test", "line1\nline2\nline3\nline4\nline5");
    cmd.args(&["--stop-on-nonmatch", "[235]"]);
    eqnice!("test:line2\ntest:line3\n", cmd.stdout());
});

// A line must contain every pattern, in any order.
rgtest_rust!(all_of, |dir: Dir, mut cmd: TestCommand| {
    dir.create(
        "sample",
        "mutex then unlock\nunlock then mutex\nmutex alone\nunlock alone\nneither\n",
    );
    cmd.args(["--all-of", "-e", "mutex", "-e", "unlock", "sample"]);
    eqnice!("mutex then unlock\nunlock then mutex\n", cmd.stdout());
});

// Overlapping terms still both count, and the flag works with -c and -v.
rgtest_rust!(all_of_count_and_invert, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sample", "ab\nb\na\nxab\n");
    cmd.args(["--all-of", "-c", "-e", "ab", "-e", "b", "sample"]);
    eqnice!("2\n", cmd.stdout());

    let mut cmd = dir.command();
    cmd.args(["--all-of", "-v", "-e", "ab", "-e", "b", "sample"]);
    eqnice!("b\na\n", cmd.stdout());
});

// Per-term settings such as -i and -w apply to every term.
rgtest_rust!(all_of_case_and_word, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sample", "Int Char\nint chars\nprint char\n");
    cmd.args(["--all-of", "-i", "-w", "-e", "int", "-e", "char", "sample"]);
    eqnice!("Int Char\n", cmd.stdout());
});

// Terms on different lines never match together, across several files.
rgtest_rust!(all_of_files, |dir: Dir, mut cmd: TestCommand| {
    dir.create("one", "foo\nbar\n");
    dir.create("two", "foo bar\n");
    cmd.args(["--all-of", "-l", "-e", "foo", "-e", "bar"]);
    eqnice!("two\n", cmd.stdout());
});

// The flag refuses modes it cannot honor.
rgtest_rust!(all_of_rejects_multiline, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sample", "a b\n");
    cmd.args(["--all-of", "-U", "-e", "a", "-e", "b", "sample"]);
    cmd.assert_err();
});

// Patterns that match every line (the empty pattern, `^`, `$`, `.*`) take a
// shortcut. Check that the shortcut gives the same answers as a normal search.
rgtest!(every_line_patterns, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sample", "one\n\ntwo\nthree");
    for pat in ["", "^", "$", ".*", "x*", "(^|zz)"] {
        let mut cmd = dir.command();
        cmd.args(["-n", "-e", pat, "sample"]);
        eqnice!("1:one\n2:\n3:two\n4:three\n", cmd.stdout());
    }
    cmd.args(["-c", "", "sample"]);
    eqnice!("4\n", cmd.stdout());
});

// A pattern that only looks universal must not take the shortcut.
rgtest!(every_line_lookalikes, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sample", "one\n\ntwo\n");
    cmd.args(["-n", "^$", "sample"]);
    eqnice!("2:\n", cmd.stdout());

    let mut cmd = dir.command();
    cmd.args(["-n", "-x", "", "sample"]);
    eqnice!("2:\n", cmd.stdout());

    let mut cmd = dir.command();
    cmd.args(["-n", "^.+$", "sample"]);
    eqnice!("1:one\n3:two\n", cmd.stdout());
});

// Context, invert and max-count still behave with the shortcut.
rgtest_rust!(every_line_context_and_limits, |dir: Dir, mut cmd: TestCommand| {
    dir.create("sample", "a\nb\nc\nd\n");
    cmd.args(["-n", "-m", "2", "", "sample"]);
    eqnice!("1:a\n2:b\n", cmd.stdout());

    let mut cmd = dir.command();
    cmd.args(["-v", "", "sample"]);
    cmd.assert_err();

    let mut cmd = dir.command();
    cmd.args(["-n", "-A1", "-m1", "", "sample"]);
    // Same as stock ripgrep: lines after the limit that still match are shown.
    eqnice!("1:a\n2:b\n", cmd.stdout());
});

// A very large set of plain literals uses a dedicated matcher. It must agree
// with the regex path: leftmost-first, per-line output, counts and context.
rgtest_rust!(large_literal_set, |dir: Dir, mut cmd: TestCommand| {
    let mut words = String::new();
    for i in 0..3500 {
        words.push_str(&format!("word{i:05}\n"));
    }
    // "ab" is listed before "abc", so it wins where both start together.
    words.push_str("ab\nabc\nneedle\n");
    dir.create("words", &words);
    dir.create(
        "hay",
        "nothing here\nxx abc yy\nthe needle line\nword00042 and word03499\nlast",
    );
    cmd.args(["-F", "-n", "-f", "words", "hay"]);
    eqnice!(
        "2:xx abc yy\n3:the needle line\n4:word00042 and word03499\n",
        cmd.stdout()
    );

    // Leftmost-first: "ab" is reported, not "abc".
    let mut cmd = dir.command();
    cmd.args(["-F", "-o", "-f", "words", "hay"]);
    eqnice!("ab\nneedle\nword00042\nword03499\n", cmd.stdout());

    let mut cmd = dir.command();
    cmd.args(["-F", "-c", "-v", "-f", "words", "hay"]);
    eqnice!("2\n", cmd.stdout());

    // Without -F the same plain words still work.
    let mut cmd = dir.command();
    cmd.args(["-c", "-f", "words", "hay"]);
    eqnice!("3\n", cmd.stdout());
});

// A two-member zip archive: d/one.txt and d/two.txt.
const ZIP_FIXTURE: &[u8] = b"\x50\x4b\x03\x04\x14\x00\x00\x00\x00\x00\x00\x00\x21\x50\xbd\x09\x29\xe0\x1e\x00\x00\x00\x1e\x00\x00\x00\x09\x00\x00\x00\x64\x2f\x6f\x6e\x65\x2e\x74\x78\x74\x66\x69\x72\x73\x74\x20\x6e\x65\x65\x64\x6c\x65\x20\x68\x65\x72\x65\x0a\x73\x65\x63\x6f\x6e\x64\x20\x6c\x69\x6e\x65\x0a\x50\x4b\x03\x04\x14\x00\x00\x00\x00\x00\x00\x00\x21\x50\xfa\x08\xb3\xf5\x17\x00\x00\x00\x17\x00\x00\x00\x09\x00\x00\x00\x64\x2f\x74\x77\x6f\x2e\x74\x78\x74\x6e\x6f\x74\x68\x69\x6e\x67\x0a\x61\x6e\x6f\x74\x68\x65\x72\x20\x6e\x65\x65\x64\x6c\x65\x0a\x50\x4b\x01\x02\x14\x03\x14\x00\x00\x00\x00\x00\x00\x00\x21\x50\xbd\x09\x29\xe0\x1e\x00\x00\x00\x1e\x00\x00\x00\x09\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x80\x01\x00\x00\x00\x00\x64\x2f\x6f\x6e\x65\x2e\x74\x78\x74\x50\x4b\x01\x02\x14\x03\x14\x00\x00\x00\x00\x00\x00\x00\x21\x50\xfa\x08\xb3\xf5\x17\x00\x00\x00\x17\x00\x00\x00\x09\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x80\x01\x45\x00\x00\x00\x64\x2f\x74\x77\x6f\x2e\x74\x78\x74\x50\x4b\x05\x06\x00\x00\x00\x00\x02\x00\x02\x00\x6e\x00\x00\x00\x83\x00\x00\x00\x00\x00";

// Build a minimal tar archive in memory (one entry per `(name, data)`).
fn tar_bytes(files: &[(&str, &str)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, data) in files {
        let mut h = [0u8; 512];
        h[..name.len()].copy_from_slice(name.as_bytes());
        h[100..108].copy_from_slice(b"0000644\0");
        h[108..116].copy_from_slice(b"0000000\0");
        h[116..124].copy_from_slice(b"0000000\0");
        h[124..136].copy_from_slice(format!("{:011o}\0", data.len()).as_bytes());
        h[136..148].copy_from_slice(b"00000000000\0");
        h[148..156].copy_from_slice(b"        ");
        h[156] = b'0';
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");
        let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
        h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        out.extend_from_slice(&h);
        out.extend_from_slice(data.as_bytes());
        out.resize(out.len().div_ceil(512) * 512, 0);
    }
    out.resize(out.len() + 1024, 0);
    out
}

// With -z, the members of a zip archive are searched and named ARCHIVE{MEMBER}.
rgtest!(search_zip_archive, |dir: Dir, mut cmd: TestCommand| {
    dir.create_bytes("a.zip", ZIP_FIXTURE);
    cmd.args(["-z", "-n", "needle", "a.zip"]);
    eqnice!(
        "a.zip{d/one.txt}:1:first needle here\na.zip{d/two.txt}:2:another needle\n",
        cmd.stdout()
    );

    // Without -z the archive is treated as one binary file, not opened.
    let mut cmd = dir.command();
    cmd.args(["needle", "a.zip"]);
    assert!(!cmd.stdout().contains("a.zip{"));
});

// Tar archives work too, with or without a single-file argument.
rgtest!(search_tar_archive, |dir: Dir, mut cmd: TestCommand| {
    dir.create_bytes(
        "a.tar",
        &tar_bytes(&[("x/a.txt", "one needle\n"), ("x/b.txt", "none\n")]),
    );
    cmd.args(["-z", "needle", "a.tar"]);
    eqnice!("a.tar{x/a.txt}:one needle\n", cmd.stdout());

    let mut cmd = dir.command();
    cmd.args(["-z", "-l", "needle"]);
    eqnice!("a.tar{x/a.txt}\n", cmd.stdout());

    let mut cmd = dir.command();
    cmd.args(["-z", "-c", "e", "a.tar"]);
    eqnice!("a.tar{x/a.txt}:1\na.tar{x/b.txt}:1\n", cmd.stdout());
});
