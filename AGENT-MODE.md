# Agent mode (Harrier, `hw`)

Agent mode is for programs and scripts that read the output of `hw`. It is off unless asked for:
`--agent-mode`, or the environment variable `RUSTY_AGENT=1` (the other rusty tools read the same variable;
any other value changes nothing; `--no-agent-mode` turns it off again). Without it, everything `hw` prints and
writes is exactly as before. `hw --version` adds the line `agent mode: yes, schema 1` only when the mode is on.

Machine-readable output is one JSON object per line on standard error. Every object starts with `"schema": 1` and
a `"type"`. The format is shared with the other rusty tools; a change to a field or a code raises `schema`.
Standard output is not changed by agent mode (`--json` keeps its own format).

## Records file: `RUSTY_AGENT_OUT`

Harnesses often merge standard output and standard error, which tangles the JSON records with the matched lines. With
agent mode on and `RUSTY_AGENT_OUT=<path>` set, every record that goes to standard error (each `diagnostic`, including
a fatal one before the options are read, and the `summary`) is also appended to that file: created if missing, one JSON
object per line, written and flushed one record at a time, so it can be read while the search runs. The `--records` lines
(search results on standard output) are not copied: they can be large and standard output stays the place for results.
An empty value, or a path that cannot be opened, gives one warning (`G0012`, counted in the summary's `diagnostics`);
nothing is created and the search runs normally with the normal exit status (a failed log file must not stop the work).
With agent mode off
(or `--no-agent-mode`) the variable is ignored: nothing is read, opened or written. `--agent-help` lists it.

## Records descriptor: `RUSTY_AGENT_FD`

Same records, same order and same rules as `RUSTY_AGENT_OUT`, appended to descriptor `n`, which the caller opened and left open
(for example `3>records.jsonl` in a shell, or a pipe it reads). The descriptor is opened again through `/proc/self/fd/n` as a new
append handle, so the caller's descriptor is untouched and there is no unsafe code. `n` is a decimal number of 3 or more and the
descriptor must be open for writing. One write and a flush per record. If `RUSTY_AGENT_OUT` is set too, both get every record. A
value that is not a number, a closed descriptor or a read-only one gives one warning (`G0013`, counted in the summary's
`diagnostics`) on standard error and on the channel that works; the search runs normally with the normal exit status. The `--pre`,
decompression and `git` child processes do not inherit the variable. With agent mode off (or `--no-agent-mode`) it is ignored:
nothing is read, opened or written. `--agent-help` lists it (`records_fd`).

## Features

| Flag | What | In `--agent-mode` |
|---|---|---|
| `--accounting` | the `summary` record: what was searched, what was skipped and why, and the option that includes it | yes |
| `--diagnostics-format=json` | every error and warning as a `diagnostic` record (see below) | yes |
| `--stable-order` | output in a fixed order (files sorted by path, as `--sort path`; held-back diagnostics sorted; summary examples chosen by name). With one search path the files are searched while the tree is walked and only the outputs are put in order (up to 64 MB of output are held; more, and the sorted search takes over) | yes |
| `--max-columns-window` | a line longer than `--max-columns` (400 bytes if none given) is cut to a window around its first match; cuts marked `[... N bytes omitted]` / `[... N more bytes, M more matches]`; `-M 0` removes the limit | yes |
| `--all-in-file` | list the files in which every pattern matches somewhere (stops reading at the first complete set) | no |
| `--within N` | every pattern within N lines of each other, printed as `PATH:LINE:TEXT` places separated by `--` (`0` = same line) | no |
| `--records` | one `result` JSON record per match on stdout: `path, line, column, byte_offset, end_byte_offset, line_offset, match, line_text` (+`end_line`, `lossy`); with `-A`/`-B`/`-C` also one `context` record per context line (`path, line, line_offset, line_text`) | no |
| `--enclosing` (needs `--records`) | adds `enclosing: [{kind, name, line}]` (outermost first) to each record: the function/class that contains the match, by a light indentation + keyword heuristic per language family (accuracy vs ctags/syn: C 99.6%, Rust 95.4%, Python 96.4%; see NOTES.md) | no |
| `--patch` (needs `--replace`) | the changes `--replace` would make, as a unified diff on stdout; writes nothing (neither `--replace` nor `--patch` ever writes a file) | no |
| `--time-limit SECONDS` | stop after the time, print what was found, mark the result incomplete (`G0010` warning; `incomplete`, `time_limit_s`, `stopped_after_ms` in the summary); checked while the tree is walked and between files | no |
| `--git-modified` | only files git reports as changed, new or renamed (`git status`); the repository is the one that holds the search path (the current directory when no path is given) | no |
| `--git-tracked` | only files in git's index (`git ls-files`); with `--git-modified`, files that are both; outside a repository: error `G0011` | no |
| `--agent-help` | prints this document's options, record fields, codes and skipped reasons as one JSON object | no |
| `--limit N`, `--offset M` | one global window over the ordered result (files by path, lines in order) with exact totals and a `next_offset`; adds the `limit` object to the `summary` record | no (separate flags) |

## Record `summary` (feature: accounting)

Fields (`full`; see the detail levels for what `normal` leaves out): `schema`, `type` = `"summary"`, `patterns`, `paths`,
`match_lines`, `match_occurrences`, `places` (instead of the two counts for `--all-in-file` and `--within`),
`files_with_matches`, `files_searched`, `bytes_searched`, `elapsed_ms` (not in stable mode), `exit`, `diagnostics` (`{errors, warnings}`),
`incomplete`/`time_limit_s`/`stopped_after_ms`, `accounting`, `suggestions` (full only), `limit`, `records_omitted`.

`match_lines` is the number of lines that matched; `match_occurrences` the number of matches (a line with three matches counts
once and three). This replaces the single `matches` of the first version, which meant occurrences in some searches and lines in
others. Counting occurrences costs a pass over every matching line, so only `full` (or `--stats`, which prints its own counts) does
it. With `-l` and `-q`, which stop at the first match of a file, `match_lines` is left out (`files_with_matches` says it).
`bytes_searched` is the bytes read (for `--all-in-file` a file is read only as far as needed).

`accounting` (`normal`): `skipped_files` (how many files the filters hid, always counted), `skipped` (groups, biggest first) and
`skipped_note`. Absent when nothing was skipped. The reasons are worked out for a small result (fewer than 100 matches) or with
`--accounting`; a bigger result gets one group, `reason: "unclassified"`, naming the biggest skipped subtrees, and the note. `full`
adds `files` (searched), `errors` (`count`, `examples`: the first five messages) and `elapsed_ms`.

A group of `skipped`: `reason`, `files`, `option` (the argument that includes them; absent when the fix is to change
another option), `examples` (up to five `{path, files}`: the largest skipped directories, shown with a trailing `/`,
or files).

| reason | skipped because | option |
|---|---|---|
| `hidden` | name starts with a dot | `--hidden` |
| `ignored-vcs` | `.gitignore`, git exclude files | `--no-ignore-vcs` |
| `ignored-parent` | ignore files in parent directories | `--no-ignore-parent` |
| `ignored-dot` | `.ignore`, `.rgignore` | `--no-ignore-dot` |
| `ignored-files` | `--ignore-file` files | `--no-ignore-files` |
| `ignored` | several kinds of ignore rule together | `--no-ignore` |
| `size` | over `--max-filesize` | null (raise it) |
| `glob` | excluded by `-g` | null (change the globs) |
| `type` | excluded by `-t` / `-T` | null (change the types) |
| `depth` | deeper than `--max-depth` | null (raise it) |
| `symlink` | symbolic link not followed | `--follow` |
| `git` | not in git's list (`--git-modified` / `--git-tracked`) | null (drop the option) |
| `several` | a combination of different filters | the options that can bring the files back: `--hidden` when a name on the way starts with a dot, `--follow` when a symbolic link is on the way, `--no-ignore` when ignore rules could apply (never all three by default) |
| `binary` | opened, but the search stopped (or the matches were hidden) at the first NUL byte | `--text` |
| `unclassified` | the filters hid them; the reason was not worked out (100 or more matches) | none |

A file is put in the first group (in this order) whose rule, switched off alone, makes the file appear. The extra
directory walks run only with `--accounting` or agent mode, so a search without them costs nothing more. The count of skipped
files takes one extra walk; the reasons one more per filter that is on, until every file has one.
Not covered: files that `-z`/`--pre` could not read, mount points skipped by `--one-file-system`.

## Paging: `--limit N` and `--offset M`

Prints at most N matching lines in total (not per file), after skipping the first M, in a fixed order: files sorted
by path (or by `--sort`), lines in file order. The whole input is still searched so the totals are exact; `--json`
cannot be combined with it. Context lines of a printed match are printed with it. With `-c`, `-l` and the other
file-list modes the unit is the file. The `summary` record (always printed on standard error when either flag is
given, with no `accounting` object unless `--accounting` is also on) gets:

`limit`: `{limit, offset, unit ("lines"|"files"), returned, total, total_files, next_offset, exact}`.
`next_offset` is the `--offset` of the next page, `null` when nothing is left (at `normal`, `limit` is left out when there is none, `offset` when 0, and `next_offset` when null). `exact` is `false` when a file that the
window edge cuts could not be cut (standard input, `--pre`, `-z`, archives) and was printed whole.
Exit status: 0 when the page has lines, 1 when it is empty.

## Record `diagnostic` (feature: structured errors and warnings)

With `--diagnostics-format=json` (agent mode turns it on; an explicit `=text` wins; `--no-messages` still silences
everything) each error and warning is one line on standard error instead of the usual text. The fields below are the `full`
record; `normal` leaves out `rendered`, `symbols` and everything empty (see the detail levels). Fields, the same as
`rusty-ld`'s: `schema`, `type` = `"diagnostic"`, `code`, `severity` (`fatal`, `error`, `warning`), `message` (first
line), `files`, `symbols` (unused, always empty), `notes` (the other lines of the text), `suggestions` (list of
`{message, option}`; `option` is the argument that applies the suggestion or `null`), `rendered` (exactly what the
text mode prints, with its `hw: ` prefix). `hw` adds `locations`: for a bad regular expression a list of
`{pattern, text, start, end, column}` (the number of the pattern, the pattern itself, and the byte range and column of
the mistake inside it; pattern numbers count `-e`/`-f` patterns in order).

| code | meaning | severity |
|---|---|---|
| `G0001` | a pattern is not a valid regular expression (suggestion: `-F`, or `-P` for look-around and backreferences) | fatal |
| `G0002` | a file or directory could not be read (permission, I/O error, symlink loop); it was skipped | error |
| `G0003` | a path was not found | error |
| `G0004` | bad option, value or combination of options | fatal |
| `G0005` | the pattern file given with `-f` could not be read | fatal |
| `G0006` | a bad ignore-file or glob rule (the rule is skipped) | warning |
| `G0007` | no file was searched: the filters probably hid everything. It fires whenever the summary would say `files_searched` 0, with or without a path (the message says which); with no path given the exit status is 2 as always, with a path it stays 1. Suggestion: `--debug` (and `--accounting` when the summary has none) | warning |
| `G0008` | unused (reserved; the number is kept free) | |
| `G0009` | the output could not be written (reported once; the summary follows) | error |
| `G0012` | `RUSTY_AGENT_OUT` is empty or cannot be opened; the records go to standard error only | warning |
| `G0013` | `RUSTY_AGENT_FD` is not a usable descriptor (not a number, below 3, closed, read-only); no records go to it | warning |
| `G0011` | `--git-modified` / `--git-tracked` outside a git repository, or no `git` program | fatal |
| `G0010` | the search stopped at `--time-limit`; the result is incomplete | warning |
| `G0100`, `G0200`, `G0300` | any other error, warning, fatal error (also warnings about `RIPGREP_CONFIG_PATH`: `G0200`) | |

The code is set where the message is made, never from its text. `RUSTY_AGENT` is read before the options, so
an error in the options themselves is a JSON `G0004` record under the environment switch, and also when
`--agent-mode` or `--diagnostics-format=json` is anywhere on the command line. `--no-agent-mode` (or
`--diagnostics-format=text`) anywhere on the command line wins over the environment variable for these errors too; between
`--agent-mode` and `--no-agent-mode` the last one wins.

The `summary` record also has `exit` (the exit status) and `diagnostics` (`{errors, warnings}`).
