# rusty-grep

rusty-grep is a fork of [ripgrep](https://github.com/BurntSushi/ripgrep)
15.2.0, the recursive line-oriented search tool. It keeps ripgrep's behaviour and
options and adds faster paths for some workloads, a few new search features, and
an opt-in machine-readable mode for scripts. The program reports itself as
`harrier 15.2.0 (a fork of ripgrep)`, and when installed from a package its
command is **`hw`**, so it never shadows `rg`. Cargo still builds the binary under
the name `rg`; the crate names are upstream's.

This is a personal fork. It is published so the work can be read and reused, not
as a replacement for ripgrep.

## Why this fork exists

We search large trees, single multi-hundred-megabyte files and long word lists
all day, and wanted to see how far ripgrep's design could be pushed on those,
measuring each idea against the stock program and keeping only what won. We
also wanted a search tool that tells a script what it skipped and why, in a
format a program can parse, instead of leaving it to guess from silence.
Upstream has its own priorities, so the changes live here.

## What is different from upstream

Speed work, each measured and kept only when it helped (details in
[How it compares](#how-it-compares)):

- Patterns that match every line (`''`, `^`, `$`, `.*`, `x*`) skip the regex
  engine.
- A word-list matcher for `-F -f` and similar large literal sets: a hashed
  shift-or filter for about 100 to 1,000 words, a full Aho-Corasick DFA for
  3,000 to 50,000, and an adaptive switch to the DFA when matches are dense.
- Sorted output (`--sort`, `--sortr`) searches with all cores and still prints in
  order; with one search path the tree is walked in parallel and sorted
  afterwards.
- Extra walker threads are added while the disk cache is cold, and only then.
- Smaller printer and searcher costs: a 64 KB output buffer, one write for the
  line-number prefix, no per-line newline count when lines are consecutive, one
  write per JSON message, no `realpath` for matching files when hyperlinks are off.
- Thread cap raised from 12 to 32.
- Release builds with profile-guided optimisation and `-C target-cpu=x86-64-v3`
  (see `scripts/build-pgo.sh`).

New features:

- `--all-of`: print lines that contain every given pattern (`hw --all-of -e mutex -e unlock`).
- `-z` / `--search-zip` opens zip files (`.zip .jar .whl .apk`) and tar files
  (`.tar`, `.tar.gz`, `.tgz`, `.tar.bz2`, `.tar.xz`, `.tar.zst` and so on) and
  searches each member, shown as `ARCHIVE{MEMBER}`. Archives inside archives are
  not opened.
- Agent-mode features: `--accounting`, `--limit` and `--offset`,
  `--all-in-file`, `--within`, `--records`, `--patch`, `--time-limit`,
  `--git-modified`, `--git-tracked`, `--enclosing`, `--agent-help`. See
  [Agent mode](#agent-mode).
- The program's name, version line, help and man page say `harrier` / `hw`.

What is upstream's and what is ours: the regex engine, searcher, printer,
directory walker, ignore handling, flags, tests and documentation (`GUIDE.md`,
`FAQ.md`, `CHANGELOG.md`) are upstream's. The additions above are ours, mostly in
`crates/core` (agent mode, `--all-of`, archives, paging, records), `crates/regex`
(word-list matcher, patterns that match every line), `crates/ignore` (walker threads)
and the printer and searcher crates. The upstream guide and FAQ describe `rg`; read
them with `hw` in place of `rg`.

With agent mode off, output and exit status are meant to be identical to the code we
forked from. When the agent-mode output was slimmed (2026-10-14) we compared 49 commands
(standard output, standard error, exit status) with agent mode off against the commit
before: all equal, except commands that use an agent-mode flag on purpose. The
descriptor channel added the next day was checked the same way: 48 of 49 equal, and the
49th is flagged the same way when the old commit is compared with itself (its `--stats` output prints
a time).

## How it compares

All timings are wall-clock medians recorded in the project's own notes and benchmark
files. Unless noted, "ripgrep" means the 15.2.0 build shipped by Fedora, and the
fork is built with profile-guided optimisation and `x86-64-v3`. The speed work ran on a Ryzen 7800X3D desktop (16 threads), which the notes
record for the profile-guided figures and do not repeat for every later run; the agent-mode
timings were taken on a laptop whose model the notes do not record. Test data: a shallow clone of the Linux
kernel (95,951 files) and a 239 MB file made from its `.c` files. The data is not in this
repository; `scripts/benchmarks/README.md` says how to rebuild it.

Against ripgrep 15.2.0, 2026-10-04 (`scripts/benchmarks/ab.py`, interleaved, 11 rounds, 22 workloads):

| Workload | Fork against ripgrep |
|---|---|
| Faster | 21 of the 22 workloads. The notes name the ranges but not which workload was the one that was not faster. |
| Tree searches | 0.70 to 0.88 of ripgrep's time (a plain literal 95 to 77 ms) |
| `--sort path` on the kernel tree | 847 to 133 ms (0.16x) |
| `-v` on the big file | 0.65x |
| Print every line | 0.40x |
| `-n` on dense matches | 0.54x |
| `--json` | 0.55x |
| Colour output | 0.75x |
| 400-word list | 0.91x |
| Start-up | equal, 2.6 ms for both |

Word lists, 239 MB file, `-c -F -f`, median of 5, 2026-10-04, fork against the better of ripgrep and
ugrep 7.8.4: 100 rare words 116 ms against 154; 400 rare 161 against 194; 400 short (5 letters
or less) 127 against 208; 1,000 rare 214 against 311; mixed lists of 100 to 900 words
136 to 213 ms against 162 to 333.

Where it is slower, equal or untested:

- Dense word lists (a match about every 240 bytes): 400 common words take 318 ms, ugrep
  278 ms, ripgrep 321 ms, so ugrep wins and ripgrep is a tie. 1,000 common words: 348 ms
  against ripgrep 342 ms, a tie. The cost is per-call overhead and branch misses, and
  several attempts to cut it did not help.
- Start-up on a 12-byte file: 2.7 ms against 2.5 ms for ripgrep. The cause was not
  investigated.
- Cold disk cache: equal to ripgrep (kernel tree `TODO`: 2.43 s against 2.41 s), because
  the work is disk-bound.
- ugrep beat the fork on `-i 'returns the'` (35 against 40 ms) and a 1,000-word list
  (181 against 220 ms) in the first 5-way benchmark, before the word-list matcher was
  reworked. A third tool with a per-file cache (rawgrep) was faster on warm plain tree
  searches (kernel tree `TODO`: 28 ms against 76 ms for the fork).
- Plain `--sort path -c e` over `/usr` is about 1.2 percent slower than before agent
  mode was added (428.5 to 433.9 ms, 41 runs), and the cause was not found.
- Memory: 292 MB resident on one large file in an early measurement (cause not investigated).
- Not measured: other platforms, non-x86 CPUs, Windows, macOS.

Profile-guided optimisation plus `x86-64-v3` made large-file searches 5 to 28 percent faster
than a plain `cargo build --release` (2026-10-03, same Ryzen machine; `-v` 0.75x, print every line
0.72x, dense `-n` 0.77x, `-i` 0.89x, 400 words 0.85x). Tree searches were within noise.

Agent mode costs (agent-fixes work, 2026-10-14, interleaved medians, `normal` detail):

| Job | Plain | Agent mode |
|---|---|---|
| `-c e` over `/usr` (135,343 files searched) | 216 ms | 306 ms (1.42x) |
| zero-match `-l` over `/usr` | 129 ms | 483 ms (3.7x) |
| `-n alpha`, 40,000-line test tree | 2.8 ms | 3.6 ms |

The zero-match case is slower because the tool walks the tree again to say why files were
skipped.

## Test results

`cargo test --release --features pcre2 --workspace` on a fresh clone of this repository
(commit `ef5d622`, 2026-10-15, with `RUSTY_AGENT` unset): **1,319 passed, 0 failed, 3 ignored**
(25 test binaries, counted from the cargo output).

Reference points from the project notes: upstream's tree passed 1,229 on 2026-10-03 (before any
change) and the fork passed 1,259 before agent mode was added. The extra tests are ours: the
word-list matcher (with a brute-force comparison against a naive search), `--all-of`, archives, paging
and the agent-mode options; some run the real program on real files through Python
(`tests/agent_fixes.py`, `tests/agent_fd.py`). Output equality with stock ripgrep was also checked:
`scripts/benchmarks/eqout.sh` covers 23 option combinations, and the word-list matcher was compared
with Fedora's ripgrep on 10 lists with 7 option sets each.

## Agent mode

Agent mode is a feature for scripts and tools that read the output. It is off unless
asked for: `--agent-mode` or `RUSTY_AGENT=1` in the environment (`--no-agent-mode`
turns it off again). Standard output is unchanged by it, except for `--max-columns-window`, which
cuts very long lines to a window around the first match.

- Records go to standard error, one JSON object per line, each beginning with
  `"schema": 1` and a `"type"`: a `summary` (counts of lines, files and bytes, what was skipped
  and why, with the option that would include it) and a `diagnostic` for every error and
  warning, with stable codes (`G0001` bad regex, `G0003` path not found, `G0007` nothing was
  searched, and so on) that do not depend on the message text.
- `--records` prints one JSON `result` record per match on standard output (path, line,
  column, byte offsets, matched text); `--enclosing` adds the function or class that contains it.
- Detail levels: `--agent-detail=brief|normal|full` or `RUSTY_AGENT_DETAIL`, `normal` by
  default. At `normal` and `brief`, diagnostics on standard error are limited to 12 KB
  (then only errors) and 20 KB (then only the summary), and the summary says how many were left out.
- Output channels: `RUSTY_AGENT_OUT=path` appends every record to a file and
  `RUSTY_AGENT_FD=n` to an open descriptor, so a caller that merges both streams still gets
  clean JSON. A bad value is one warning (`G0012`, `G0013`), never a failed search.
- Fixed order: files sorted by path, diagnostics held and sorted, so repeated runs give identical output.
- Paging and limits: `--limit N --offset M`, `--time-limit SECONDS`.
- `--agent-help` prints everything above (options, fields, codes) as one JSON object.

Example:

    $ hw --records alpha a.txt        # a.txt: three lines, two contain "alpha"; first record shown
    {"schema":1,"type":"result","path":"a.txt","line":1,"column":1,"byte_offset":0,"end_byte_offset":5,"line_offset":0,"match":"alpha","line_text":"alpha"}

    $ RUSTY_AGENT=1 hw -c alpha a.txt
    2
    {"schema":1,"type":"summary","match_lines":2,"files_with_matches":1,"files_searched":1,"bytes_searched":28,"exit":0}

The full description, with every record field and code, is in [AGENT-MODE.md](AGENT-MODE.md).

## Install

From source, the binary is built as `rg`; copy it under the name you want.
The package we use installs it as `/usr/bin/hw` with a man page and shell completions, and
does not install `rg` or touch another ripgrep package, so both can sit side by side.

    cargo build --locked --profile release-lto --features pcre2
    install -Dm755 target/release-lto/rg ~/.local/bin/hw

For the fast build used in the comparison above, add
`RUSTFLAGS="-C target-cpu=x86-64-v3"` (needs a CPU with AVX2; the binary crashes with an illegal
instruction on one without) or run `scripts/build-pgo.sh`, which also trains a
profile (`pgo/README.md`; the profile file itself is not in the repository). Without that flag the
binary runs on any x86-64 CPU. `--features pcre2` adds `-P` and needs the PCRE2 library and
headers installed.

## Build and test

    cargo build --locked --profile release-lto --features pcre2
    env -u RUSTY_AGENT cargo test --release --features pcre2 --workspace

`RUSTY_AGENT` must not be set when running the tests. The release-lto build took 1 minute 32 seconds with two jobs (2026-10-15, from a fresh clone); the test run builds its own copy.

## Known limits

- `--limit` and `--offset` cut a file at the window edge only for plain files; standard
  input, `--pre`, `-z` and archive members are printed whole and the summary says
  `exact: false`.
- `--time-limit` is checked while the tree is walked and between files, so one very
  large file can overrun it.
- `--enclosing` is a light indentation and keyword heuristic, not a parser. Against
  ctags and a Rust parse it was right on 99.6 percent of 2,104 C matches, 96.4 percent
  of 470 Python matches and 95.4 percent of 3,095 Rust matches; multi-line string
  literals and calls that look like definitions fool it.
- Archives inside archives are not opened; an encrypted zip entry stops the archive search with an error.
- With 100 or more matches on very large trees, skipped files are counted but their reasons
  are left as one `unclassified` group.
- Only x86-64 Linux is built and tested.
- Changes here are not sent upstream; this is a separate fork.

## Licence and credit

Dual-licensed as upstream: Unlicense or MIT, your choice (`UNLICENSE`, `LICENSE-MIT`, `COPYING`).
ripgrep was written by Andrew Gallant and its contributors; the regex engine and most of the
code are theirs. The additions in this fork are under the same terms. This fork is not
affiliated with, or endorsed by, the upstream project.
