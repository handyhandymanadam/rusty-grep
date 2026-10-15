/*!
Opt-in "agent mode": output meant for programs and scripts that read it.

Everything here is off unless `--agent-mode` (or `RUSTY_AGENT=1`) or one of the
flags it turns on (`--accounting`) is given. The one JSON-lines format is shared
with the other rusty tools: each line is an object that starts with
`"schema": 1` and a `"type"`.
*/

use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    time::Instant,
};

use serde_json::{Map, Value, json};

use crate::flags::HiArgs;

/// The version of the record format. A change to a field or a code raises it.
pub(crate) const SCHEMA: u32 = 1;

/// Under agent mode alone, the reasons for skipped files are only worked out
/// when fewer matches than this were found (a search with plenty of results
/// was not "empty for a hidden reason"); the number of skipped files is
/// always counted. `--accounting` always works the reasons out.
pub(crate) const MANY_MATCHES: u64 = 100;

/// How many example paths each group of skipped files shows (`normal`).
pub(crate) const EXAMPLES: usize = 5;

/// How much the records say (`--agent-detail`, `RUSTY_AGENT_DETAIL`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Detail {
    Brief,
    Normal,
    Full,
}

/// Size budget of the records on standard error (`brief` and `normal`):
/// past the soft limit only errors go on, past the hard limit only the
/// summary. `full` has no budget; the `RUSTY_AGENT_OUT` copy never has one.
const SOFT_BUDGET: u64 = 12_000;
const HARD_BUDGET: u64 = 20_000;

static DETAIL: AtomicU8 = AtomicU8::new(1);
static EMITTED: AtomicU64 = AtomicU64::new(0);
static OMITTED: Mutex<BTreeMap<&'static str, u64>> =
    Mutex::new(BTreeMap::new());
static SUGGESTED: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static NOTHING_REPORTED: AtomicBool = AtomicBool::new(false);
static OUTPUT_FAILED: AtomicBool = AtomicBool::new(false);
static LITE: AtomicBool = AtomicBool::new(false);
static SEARCHED: Mutex<Option<crate::account::SearchedSet>> = Mutex::new(None);

static COLLECT: AtomicBool = AtomicBool::new(false);
static ON: AtomicBool = AtomicBool::new(false);
static QUIET_STATS: AtomicBool = AtomicBool::new(false);
static START: OnceLock<Instant> = OnceLock::new();
static BINARY: Mutex<Vec<(PathBuf, u64)>> = Mutex::new(Vec::new());
static TIME_UP: AtomicBool = AtomicBool::new(false);
static SKIPPED: AtomicBool = AtomicBool::new(false);
static LIMIT_S: Mutex<Option<f64>> = Mutex::new(None);
static GIT_ON: AtomicBool = AtomicBool::new(false);
static OUT: Mutex<Option<std::fs::File>> = Mutex::new(None);
/// The `RUSTY_AGENT_FD` handle: a second copy of the records, like `OUT`.
static FD: Mutex<Option<std::fs::File>> = Mutex::new(None);
static OUT_TRIED: AtomicBool = AtomicBool::new(false);
static STABLE: AtomicBool = AtomicBool::new(false);
static HELD: Mutex<Vec<(String, Coded)>> = Mutex::new(Vec::new());
static ERRORS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static PAGING: Mutex<Option<crate::limit::Paging>> = Mutex::new(None);
static TERMS: Mutex<Option<(u64, u64, u64, u64)>> = Mutex::new(None);
static STATS: Mutex<Option<grep::printer::Stats>> = Mutex::new(None);

/// True when the environment asks for agent mode: `RUSTY_AGENT` is exactly
/// `1`. Any other value, or none, changes nothing.
pub(crate) fn env_on() -> bool {
    std::env::var_os("RUSTY_AGENT").is_some_and(|v| v == "1")
}

/// Called once after the arguments are parsed.
pub(crate) fn start(args: &HiArgs) {
    set_lite(args.lite_stats());
    GIT_ON.store(args.git_filter().is_some(), Ordering::Relaxed);
    STABLE.store(args.stable_order(), Ordering::Relaxed);
    if let Some(secs) = args.time_limit() {
        *LIMIT_S.lock().unwrap() = Some(secs);
        let _ = START.set(Instant::now());
        COLLECT.store(true, Ordering::Relaxed);
        QUIET_STATS.store(!args.stats_printed(), Ordering::Relaxed);
        if secs <= 0.0 {
            TIME_UP.store(true, Ordering::Relaxed);
        } else {
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs_f64(secs));
                TIME_UP.store(true, Ordering::Relaxed);
            });
        }
    }
    if args.accounting() || args.paged() {
        let _ = START.set(Instant::now());
        COLLECT.store(true, Ordering::Relaxed);
        // The statistics are gathered for the summary only; do not print the
        // text report the user did not ask for.
        QUIET_STATS.store(!args.stats_printed(), Ordering::Relaxed);
    }
}

/// The records file named by `RUSTY_AGENT_OUT`, if any. An empty value is an
/// error: it is never a request for "no file".
fn out_path() -> anyhow::Result<Option<std::path::PathBuf>> {
    match std::env::var_os("RUSTY_AGENT_OUT") {
        None => Ok(None),
        Some(p) if p.is_empty() => {
            anyhow::bail!("RUSTY_AGENT_OUT is set but empty")
        }
        Some(p) => Ok(Some(p.into())),
    }
}

fn open_out(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new().create(true).append(true).open(path)
}

/// The descriptor named by `RUSTY_AGENT_FD`, if any, as a new append handle on
/// the same file, pipe or terminal. It is opened again through `/proc/self/fd`
/// (so there is no unsafe code and the caller's descriptor is left alone);
/// Linux's fdinfo says how it was opened, so a read-only one is turned down.
fn fd_channel() -> Option<Result<std::fs::File, String>> {
    let v = std::env::var_os("RUSTY_AGENT_FD")?;
    let v = v.to_string_lossy();
    let bad = |why: String| Err(format!("RUSTY_AGENT_FD={v}: {why}"));
    if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
        return Some(bad("not a decimal number".into()));
    }
    let Ok(n) = v.parse::<u64>() else {
        return Some(bad("number too large".into()));
    };
    if n < 3 {
        return Some(bad(format!(
            "{n} is below 3 (0, 1 and 2 are standard input, output and error)"
        )));
    }
    match std::fs::read_to_string(format!("/proc/self/fdinfo/{n}")) {
        Ok(t) => {
            let flags = t
                .lines()
                .find_map(|l| l.strip_prefix("flags:"))
                .and_then(|f| u32::from_str_radix(f.trim(), 8).ok());
            if flags.is_some_and(|f| f & 3 == 0) {
                return Some(bad(format!(
                    "descriptor {n} is open for reading only"
                )));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Some(bad(format!("descriptor {n} is not open")));
        }
        Err(_) => {}
    }
    Some(
        std::fs::OpenOptions::new()
            .append(true)
            .open(format!("/proc/self/fd/{n}"))
            .or_else(|e| bad(format!("descriptor {n} cannot be written ({e})"))),
    )
}

/// Decide, once the options are known, whether the records also go to the
/// `RUSTY_AGENT_OUT` file and the `RUSTY_AGENT_FD` descriptor: only when agent
/// mode is on. An empty or unopenable path gives one warning (`G0012`), a bad
/// descriptor one warning (`G0013`), and the search goes on without that
/// channel: a failed log must not stop the work. The warnings are written
/// after both channels are open, so a channel that works gets the warning
/// about the other. With agent mode off nothing is read, opened or written.
/// Call it after the diagnostics format is set.
pub(crate) fn setup_out(agent_on: bool) {
    ON.store(agent_on, Ordering::Relaxed);
    OUT_TRIED.store(true, Ordering::Relaxed);
    if !agent_on {
        *OUT.lock().unwrap() = None;
        *FD.lock().unwrap() = None;
        return;
    }
    // The descriptor first: a file opened below could otherwise take the
    // number that a caller means by a descriptor it has closed.
    let mut problems: Vec<(&'static str, String, &'static str)> = vec![];
    match fd_channel() {
        None => {}
        Some(Ok(f)) => *FD.lock().unwrap() = Some(f),
        Some(Err(e)) => problems.push((
            "G0013",
            format!("{e}; no records go to that descriptor"),
            "check the descriptor in RUSTY_AGENT_FD (3 or more, open for \
             writing: a file or pipe the caller left open), or unset it",
        )),
    }
    let file_problem = match out_path() {
        Ok(None) => None,
        Ok(Some(path)) => match open_out(&path) {
            Ok(f) => {
                *OUT.lock().unwrap() = Some(f);
                None
            }
            Err(e) => Some(format!(
                "cannot open the RUSTY_AGENT_OUT file {}: {e}",
                path.display()
            )),
        },
        Err(e) => Some(e.to_string()),
    };
    if let Some(p) = file_problem {
        problems.push((
            "G0012",
            format!("{p}; the records go to standard error only"),
            "check the path in RUSTY_AGENT_OUT (it must be a file that can be \
             created or appended to), or unset it",
        ));
    }
    for (code, message, hint) in problems {
        let mut d = Coded::new(code, Severity::Warning, message);
        d.suggestions.push((hint.to_string(), None));
        report(d);
    }
}

/// Agent mode as far as the environment and the command line say, for a
/// message that comes before the options are applied.
pub(crate) fn early_agent() -> bool {
    let mut on = env_on();
    for a in std::env::args_os().skip(1) {
        if a == "--agent-mode" {
            on = true;
        } else if a == "--no-agent-mode" {
            on = false;
        }
    }
    on
}

/// Append one record (without its newline) to the records file, if agent mode
/// is on and `RUSTY_AGENT_OUT` names one. One write and a flush per record.
pub(crate) fn tee(line: &str) {
    if !(ON.load(Ordering::Relaxed)
        || ARGS_STAGE.load(Ordering::Relaxed) && early_agent())
    {
        return;
    }
    let mut out = OUT.lock().unwrap();
    let mut fd = FD.lock().unwrap();
    if out.is_none() && fd.is_none() && !OUT_TRIED.swap(true, Ordering::Relaxed) {
        // A record before the options were read (a bad option): open the
        // channels now, quietly; setup_out has not run.
        if let Ok(Some(path)) = out_path() {
            *out = open_out(&path).ok();
        }
        *fd = fd_channel().and_then(Result::ok);
    }
    if out.is_some() || fd.is_some() {
        use std::io::Write;
        let mut buf = String::with_capacity(line.len() + 1);
        buf.push_str(line);
        buf.push('\n');
        for f in [out.as_mut(), fd.as_mut()].into_iter().flatten() {
            let _ = f.write_all(buf.as_bytes());
            let _ = f.flush();
        }
    }
}

/// Remember whether agent mode is on (for the `--version` line).
pub(crate) fn set_on(yes: bool) {
    ON.store(yes, Ordering::Relaxed);
}

/// The extra `--version` line, only when agent mode is on.
pub(crate) fn version_line() -> Option<String> {
    ON.load(Ordering::Relaxed)
        .then(|| format!("agent mode: yes, schema {SCHEMA}"))
}

/// True once the `--time-limit` has passed. Called before each unit of work;
/// a true answer means that unit is skipped, so the result is incomplete.
#[inline]
pub(crate) fn time_up() -> bool {
    if TIME_UP.load(Ordering::Relaxed) {
        SKIPPED.store(true, Ordering::Relaxed);
        true
    } else {
        false
    }
}

/// Whether the `--time-limit` has passed (without counting it as a skip).
pub(crate) fn time_limit_hit() -> bool {
    TIME_UP.load(Ordering::Relaxed)
}

/// Whether the search is limited to git's file list (then "nothing was
/// searched" is an ordinary result, not an error).
pub(crate) fn git_active() -> bool {
    GIT_ON.load(Ordering::Relaxed)
}

/// Whether anything is being collected for the summary.
#[inline]
pub(crate) fn collecting() -> bool {
    COLLECT.load(Ordering::Relaxed)
}

/// Whether the usual statistics report should stay unprinted.
pub(crate) fn quiet_stats() -> bool {
    QUIET_STATS.load(Ordering::Relaxed)
}

/// Remember a file that was opened but where the search met a NUL byte.
pub(crate) fn note_binary(path: &Path, offset: u64) {
    BINARY.lock().unwrap().push((path.to_path_buf(), offset));
}

/// Remember an error message (an unreadable file or directory, ...).
pub(crate) fn note_error(message: String) {
    ERRORS.lock().unwrap().push(message);
}

/// Remember the totals of a paged search (`--limit`, `--offset`).
pub(crate) fn note_paging(p: crate::limit::Paging) {
    *PAGING.lock().unwrap() = Some(p);
}

/// Remember the totals of an `--all-in-file` / `--within` search: places,
/// files with a place, files searched, bytes read.
pub(crate) fn note_terms(
    places: u64,
    files_with: u64,
    searched: u64,
    bytes: u64,
) {
    *TERMS.lock().unwrap() = Some((places, files_with, searched, bytes));
}

/// Remember the search statistics for the summary.
pub(crate) fn note_stats(stats: &grep::printer::Stats) {
    *STATS.lock().unwrap() = Some(stats.clone());
}

/// The effective detail level.
#[inline]
pub(crate) fn detail() -> Detail {
    match DETAIL.load(Ordering::Relaxed) {
        0 => Detail::Brief,
        1 => Detail::Normal,
        _ => Detail::Full,
    }
}

/// Whether every field is wanted.
#[inline]
pub(crate) fn full() -> bool {
    detail() == Detail::Full
}

/// Read `RUSTY_AGENT_DETAIL`; a value that is not a level is ignored.
fn env_detail() -> Option<Detail> {
    match std::env::var("RUSTY_AGENT_DETAIL").ok()?.as_str() {
        "brief" => Some(Detail::Brief),
        "normal" => Some(Detail::Normal),
        "full" => Some(Detail::Full),
        _ => None,
    }
}

/// Choose the detail level once the options are read: the flag, else (under
/// agent mode) the environment variable, else `normal` under agent mode and
/// `full` for the JSON options used alone, which wrote `full` before.
pub(crate) fn set_detail(flag: Option<Detail>, agent_on: bool) {
    let d = flag
        .or_else(|| if agent_on { env_detail() } else { None })
        .unwrap_or(if agent_on { Detail::Normal } else { Detail::Full });
    DETAIL.store(
        match d {
            Detail::Brief => 0,
            Detail::Normal => 1,
            Detail::Full => 2,
        },
        Ordering::Relaxed,
    );
}

/// Hand over the files the search read (when it kept track of them), so the
/// accounting does not have to walk the tree again to learn them.
pub(crate) fn note_searched(set: crate::account::SearchedSet) {
    *SEARCHED.lock().unwrap() = Some(set);
}

/// Remember that the statistics count matching lines only.
pub(crate) fn set_lite(yes: bool) {
    LITE.store(yes, Ordering::Relaxed);
}

/// Whether the statistics count matching lines only (not each match).
pub(crate) fn lite() -> bool {
    LITE.load(Ordering::Relaxed)
}

/// True when the run is in stable mode.
fn stable() -> bool {
    STABLE.load(Ordering::Relaxed)
}

/// Forget what a first attempt at a search reported, so a second attempt (the
/// sorted search that follows a search that buffered too much) starts clean.
pub(crate) fn reset_run() {
    HELD.lock().unwrap().clear();
    *SEARCHED.lock().unwrap() = None;
    ERRORS.lock().unwrap().clear();
    BINARY.lock().unwrap().clear();
    ERROR_COUNT.store(0, Ordering::Relaxed);
    WARNING_COUNT.store(0, Ordering::Relaxed);
    NOTHING_REPORTED.store(false, Ordering::Relaxed);
    OUTPUT_FAILED.store(false, Ordering::Relaxed);
}

/// Print the diagnostics held back for a stable order, sorted.
fn flush_held() {
    let mut held = std::mem::take(&mut *HELD.lock().unwrap());
    held.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, d) in held {
        emit(&d);
    }
}

/// The number of matching lines and files and the like, as the summary shows
/// them.
struct Counts {
    match_lines: u64,
    /// `None` when individual matches were not counted.
    match_occurrences: Option<u64>,
    /// Counts places (`--all-in-file`, `--within`) instead of lines.
    places: Option<u64>,
    files_with: u64,
    searched: u64,
    bytes: u64,
}

fn counts(args: &HiArgs) -> Counts {
    let stats = STATS.lock().unwrap().take();
    match stats {
        Some(ref s) => Counts {
            match_lines: s.matched_lines(),
            match_occurrences: (!lite()).then(|| s.matches()),
            places: None,
            files_with: s.searches_with_match(),
            searched: s.searches(),
            bytes: s.bytes_searched(),
        },
        None => match *TERMS.lock().unwrap() {
            Some((places, files_with, searched, bytes)) => Counts {
                match_lines: 0,
                match_occurrences: None,
                places: Some(places),
                files_with,
                searched,
                bytes,
            },
            None => {
                let _ = args;
                Counts {
                    match_lines: 0,
                    match_occurrences: None,
                    places: None,
                    files_with: 0,
                    searched: 0,
                    bytes: 0,
                }
            }
        },
    }
}

/// One group of skipped files, as a record fragment.
fn group_json(g: &crate::account::GroupOut, d: Detail) -> Value {
    let mut m = Map::new();
    m.insert("reason".into(), json!(g.reason));
    m.insert("files".into(), json!(g.files));
    match g.option {
        Some(o) => {
            m.insert("option".into(), json!(o));
        }
        None if d == Detail::Full => {
            m.insert("option".into(), Value::Null);
        }
        None => {}
    }
    if d == Detail::Full || d == Detail::Normal && !g.examples.is_empty() {
        let ex: Vec<Value> = g
            .examples
            .iter()
            .take(EXAMPLES)
            .map(|(p, n)| json!({"path": p, "files": n}))
            .collect();
        m.insert("examples".into(), Value::Array(ex));
    }
    Value::Object(m)
}

/// Print the summary record on standard error. Called once, after the search.
pub(crate) fn finish(args: &HiArgs, exit: u8) {
    let d = detail();
    let incomplete = SKIPPED.load(Ordering::Relaxed);
    if incomplete {
        let secs = LIMIT_S.lock().unwrap().unwrap_or(0.0);
        let mut w = Coded::new(
            "G0010",
            Severity::Warning,
            format!(
                "the search stopped at the time limit of {secs} s; the \
                 result is incomplete"
            ),
        );
        w.suggestions.push((
            "a longer --time-limit, or a narrower search (a path, -t or -g), \
             covers the rest"
                .to_string(),
            None,
        ));
        report(w);
    }
    let found = if collecting() { Some(counts(args)) } else { None };
    // Nothing was searched, and the search was not cut short: a filter hid
    // everything (with no path given, the search code has said so already).
    if let Some(ref c) = found {
        if ON.load(Ordering::Relaxed)
            && c.searched == 0
            && !time_limit_hit()
            && !NOTHING_REPORTED.swap(true, Ordering::Relaxed)
        {
            report(nothing_searched_coded(args.has_implicit_path()));
        }
    }
    flush_held();
    let Some(c) = found else { return };
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let searched_ms = START.get().map_or(0, |s| s.elapsed().as_millis());
    let audit_started = Instant::now();
    let mut suggestions: Vec<Value> = Vec::new();
    let mut skipped_total = 0u64;
    let mut groups: Vec<Value> = Vec::new();
    let mut note: Option<&str> = None;
    if args.accounting() {
        if args.only_stdin() {
            // Nothing can be skipped.
        } else if incomplete {
            note = Some(
                "not worked out: the search stopped at the time limit",
            );
        } else {
            let thorough = args.accounting_always()
                || c.match_lines < MANY_MATCHES
                    && c.match_occurrences.unwrap_or(0) < MANY_MATCHES;
            let known = SEARCHED.lock().unwrap().take();
            let rr = crate::account::skipped(args, thorough, known);
            match rr {
                Ok(sk) => {
                    skipped_total = sk.total;
                    if !sk.classified {
                        note = Some(
                            "reasons not worked out: 100 or more matches; \
                             --accounting names them",
                        );
                    }
                    for g in &sk.groups {
                        groups.push(group_json(g, d));
                        let first = g
                            .examples
                            .first()
                            .map(|e| e.0.as_str())
                            .unwrap_or("");
                        suggestions.push(json!({
                            "message": format!(
                                "{} {} not searched: {} (for example {})",
                                g.files,
                                if g.files == 1 { "file was" } else { "files were" },
                                g.advice,
                                first
                            ),
                            "option": g.option,
                        }));
                    }
                }
                Err(err) => {
                    note_error(format!("accounting walk failed: {err}"));
                }
            }
        }
        let binary = BINARY.lock().unwrap();
        if !binary.is_empty() {
            // The five smallest names, without sorting every one.
            let mut files: Vec<&PathBuf> = binary.iter().map(|(p, _)| p).collect();
            let n = files.len();
            if n > EXAMPLES {
                files.select_nth_unstable(EXAMPLES - 1);
                files.truncate(EXAMPLES);
            }
            files.sort();
            let examples: Vec<(String, u64)> = files
                .iter()
                .map(|p| (crate::account::display(args, p), 1))
                .collect();
            let g = crate::account::GroupOut {
                reason: "binary",
                option: Some("--text"),
                advice: "",
                files: n as u64,
                examples,
            };
            groups.push(group_json(&g, d));
            suggestions.push(json!({
                "message": format!(
                    "{} file(s) contain a NUL byte, so the search stopped \
                     there or hid their matches; --text searches them as text",
                    n
                ),
                "option": "--text",
            }));
        }
    }
    let errors = ERRORS.lock().unwrap();
    let mut error_examples: Vec<&String> = errors.iter().collect();
    error_examples.sort();
    error_examples.truncate(EXAMPLES);
    let nonzero = |n: u64| d == Detail::Full || n > 0;
    let mut r = Map::new();
    r.insert("schema".into(), json!(SCHEMA));
    r.insert("type".into(), json!("summary"));
    if d == Detail::Full {
        r.insert("patterns".into(), json!(args.pattern_strings()));
        r.insert(
            "paths".into(),
            json!(
                args.search_paths()
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
            ),
        );
    }
    // In -l and -q searches that stop at the first match, the line count is
    // not the number of matching lines: leave it out.
    if c.places.is_none() && !(lite() && args.line_count_partial()) {
        if nonzero(c.match_lines) {
            r.insert("match_lines".into(), json!(c.match_lines));
        }
        if let Some(n) = c.match_occurrences {
            if d == Detail::Full {
                r.insert("match_occurrences".into(), json!(n));
            }
        }
    }
    if let Some(n) = c.places {
        r.insert("places".into(), json!(n));
    }
    if nonzero(c.files_with) {
        r.insert("files_with_matches".into(), json!(c.files_with));
    }
    if nonzero(c.searched) {
        r.insert("files_searched".into(), json!(c.searched));
    }
    if nonzero(c.bytes) {
        r.insert("bytes_searched".into(), json!(c.bytes));
    }
    if !stable() {
        r.insert("elapsed_ms".into(), json!(searched_ms as u64));
    }
    r.insert("exit".into(), json!(exit));
    let (ec, wc) = (
        ERROR_COUNT.load(Ordering::Relaxed),
        WARNING_COUNT.load(Ordering::Relaxed),
    );
    if d == Detail::Full {
        r.insert("diagnostics".into(), json!({"errors": ec, "warnings": wc}));
    } else if ec > 0 || wc > 0 {
        let mut m = Map::new();
        if ec > 0 {
            m.insert("errors".into(), json!(ec));
        }
        if wc > 0 {
            m.insert("warnings".into(), json!(wc));
        }
        r.insert("diagnostics".into(), Value::Object(m));
    }
    if let Some(limit) = *LIMIT_S.lock().unwrap() {
        if d == Detail::Full || incomplete {
            r.insert("incomplete".into(), json!(incomplete));
            r.insert("time_limit_s".into(), json!(limit));
        }
        if incomplete && !stable() {
            r.insert("stopped_after_ms".into(), json!(searched_ms as u64));
        }
    }
    if args.accounting() {
        let mut m = Map::new();
        if d == Detail::Full {
            m.insert("files".into(), json!(c.searched));
            m.insert("skipped".into(), Value::Array(groups));
            m.insert(
                "skipped_note".into(),
                note.map_or(Value::Null, |n| json!(n)),
            );
            m.insert("skipped_files".into(), json!(skipped_total));
            m.insert(
                "errors".into(),
                json!({"count": errors.len(), "examples": error_examples}),
            );
            if !stable() {
                m.insert(
                    "elapsed_ms".into(),
                    json!(audit_started.elapsed().as_millis() as u64),
                );
            }
            r.insert("accounting".into(), Value::Object(m));
            r.insert("suggestions".into(), Value::Array(suggestions));
        } else {
            if skipped_total > 0 {
                m.insert("skipped_files".into(), json!(skipped_total));
            }
            if !groups.is_empty() {
                m.insert("skipped".into(), Value::Array(groups));
            }
            if let Some(n) = note {
                if skipped_total > 0 || d == Detail::Normal && incomplete {
                    m.insert("skipped_note".into(), json!(n));
                }
            }
            if !m.is_empty() {
                r.insert("accounting".into(), Value::Object(m));
            }
        }
    }
    if let Some(p) = PAGING.lock().unwrap().take() {
        let mut m = Map::new();
        if let Some(l) = p.limit {
            m.insert("limit".into(), json!(l));
        } else if d == Detail::Full {
            m.insert("limit".into(), Value::Null);
        }
        if p.offset > 0 || d == Detail::Full {
            m.insert("offset".into(), json!(p.offset));
        }
        m.insert("unit".into(), json!(p.unit));
        m.insert("returned".into(), json!(p.returned));
        m.insert("total".into(), json!(p.total));
        m.insert("total_files".into(), json!(p.total_files));
        match p.next_offset {
            Some(n) => {
                m.insert("next_offset".into(), json!(n));
            }
            None if d == Detail::Full => {
                m.insert("next_offset".into(), Value::Null);
            }
            None => {}
        }
        m.insert("exact".into(), json!(p.exact));
        r.insert("limit".into(), Value::Object(m));
    }
    // The copy in RUSTY_AGENT_OUT has every record, so it has no count of
    // records left out.
    let copy = Value::Object(r.clone()).to_string();
    let omitted = std::mem::take(&mut *OMITTED.lock().unwrap());
    if !omitted.is_empty() {
        let mut m = Map::new();
        for (k, n) in omitted {
            m.insert(k.into(), json!(n));
        }
        m.insert(
            "more".into(),
            json!(
                "the size budget kept these off; RUSTY_AGENT_OUT=FILE or RUSTY_AGENT_FD=N keeps \
                 every record, --agent-detail=full has no budget"
            ),
        );
        r.insert("records_omitted".into(), Value::Object(m));
    }
    let record = Value::Object(r).to_string();
    let _ = writeln!(std::io::stderr().lock(), "{record}");
    tee(&copy);
}

/// Every diagnostic code: code, severity, meaning. The code is set where the
/// message is made, never from its text; a test checks that every code used in
/// the source is in this table.
pub(crate) const CODES: &[(&str, &str, &str)] = &[
    ("G0001", "fatal", "a pattern is not a valid regular expression"),
    ("G0002", "error", "a file or directory could not be read"),
    ("G0003", "error", "a path was not found"),
    ("G0004", "fatal", "bad option, value or combination of options"),
    ("G0005", "fatal", "the pattern file given with -f could not be read"),
    ("G0006", "warning", "a bad ignore-file or glob rule (skipped)"),
    (
        "G0007",
        "warning",
        "no file was searched: filters probably hid everything (with no path \
         given the exit status is 2; with a path it is 1 as usual)",
    ),
    ("G0008", "unused", "unused (reserved; the number is kept free)"),
    ("G0009", "error", "the output could not be written (reported once)"),
    ("G0010", "warning", "the search stopped at --time-limit; incomplete"),
    (
        "G0011",
        "fatal",
        "--git-modified/--git-tracked without a git repository or git",
    ),
    (
        "G0012",
        "warning",
        "RUSTY_AGENT_OUT is empty or cannot be opened; records go to stderr only",
    ),
    (
        "G0013",
        "warning",
        "RUSTY_AGENT_FD is not a usable descriptor (bad number, closed, read-only); no records go to it",
    ),
    ("G0100", "error", "any other error"),
    ("G0200", "warning", "any other warning"),
    ("G0300", "fatal", "any other fatal error"),
];

/// The reasons a file can be in `skipped`: reason, the option that includes
/// the files (null when the fix is to change another option), meaning.
pub(crate) const SKIPPED_REASONS: &[(&str, Option<&str>, &str)] = &[
    ("hidden", Some("--hidden"), "name starts with a dot"),
    (
        "ignored-vcs",
        Some("--no-ignore-vcs"),
        ".gitignore or git exclude files",
    ),
    (
        "ignored-parent",
        Some("--no-ignore-parent"),
        "ignore files in parent directories",
    ),
    ("ignored-dot", Some("--no-ignore-dot"), ".ignore or .rgignore"),
    ("ignored-files", Some("--no-ignore-files"), "--ignore-file files"),
    ("ignored", Some("--no-ignore"), "several kinds of ignore rule together"),
    ("size", None, "over --max-filesize"),
    ("glob", None, "excluded by -g"),
    ("type", None, "excluded by -t/-T"),
    ("depth", None, "deeper than --max-depth"),
    ("symlink", Some("--follow"), "symbolic link not followed"),
    ("git", None, "not in git's list (--git-modified/--git-tracked)"),
    ("several", None, "a combination of different filters"),
    (
        "binary",
        Some("--text"),
        "searched, but stopped or hidden at the first NUL byte",
    ),
];

// ---------------------------------------------------------------------------
// Diagnostics: errors and warnings as JSON records.
//
// Codes (stable; a change raises SCHEMA). The code is chosen at the place the
// message is made, never from its text.
//   G0001 invalid regular expression      G0006 bad ignore or glob rule
//   G0002 cannot read a file or directory G0007 nothing was searched
//   G0003 path not found                  G0009 cannot write the output
//   G0004 bad option, value or combination
//   G0005 cannot read a pattern file (-f)
//   G0100 other error, G0200 other warning, G0300 other fatal error
// ---------------------------------------------------------------------------

static JSON_DIAGNOSTICS: AtomicBool = AtomicBool::new(false);
static ARGS_STAGE: AtomicBool = AtomicBool::new(true);
static ERROR_COUNT: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
static WARNING_COUNT: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Severity {
    Fatal,
    Error,
    Warning,
}

impl Severity {
    fn as_str(self) -> &'static str {
        match self {
            Severity::Fatal => "fatal",
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

/// Switch JSON diagnostics on or off.
pub(crate) fn set_json_diagnostics(yes: bool) {
    JSON_DIAGNOSTICS.store(yes, Ordering::Relaxed);
}

/// Whether messages go out as JSON records.
#[inline]
pub(crate) fn json_diagnostics() -> bool {
    JSON_DIAGNOSTICS.load(Ordering::Relaxed)
}

/// Mark that the arguments are parsed: later fatal errors are run errors.
pub(crate) fn set_args_done() {
    ARGS_STAGE.store(false, Ordering::Relaxed);
}

/// Whether to use JSON for an error that happened while the arguments were
/// still being read (before the options that ask for it could be applied).
pub(crate) fn json_for_early_error() -> bool {
    // An explicit --diagnostics-format=text, or --no-agent-mode, wins over
    // the environment variable, wherever it stands on the command line.
    let mut format = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(a) = args.next() {
        if a == "--diagnostics-format=json" {
            format = Some(true);
        } else if a == "--diagnostics-format=text" {
            format = Some(false);
        } else if a == "--diagnostics-format" {
            match args.next() {
                Some(v) if v == "json" => format = Some(true),
                Some(v) if v == "text" => format = Some(false),
                _ => {}
            }
        }
    }
    format.unwrap_or_else(early_agent)
}

/// A message with everything the JSON record needs. Used as an error: its
/// text is exactly the text the message has without agent mode.
#[derive(Debug)]
pub(crate) struct Coded {
    pub(crate) code: &'static str,
    pub(crate) severity: Severity,
    /// First line, for the record's `message`.
    pub(crate) message: String,
    pub(crate) files: Vec<String>,
    pub(crate) notes: Vec<String>,
    pub(crate) suggestions: Vec<(String, Option<&'static str>)>,
    pub(crate) locations: Vec<Value>,
    /// The text as printed without agent mode (without the `hw: ` prefix).
    pub(crate) rendered: String,
}

impl std::fmt::Display for Coded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.rendered)
    }
}

impl std::error::Error for Coded {}

impl Coded {
    pub(crate) fn new(
        code: &'static str,
        severity: Severity,
        rendered: String,
    ) -> Coded {
        let mut lines = rendered.lines();
        let message = lines.next().unwrap_or("").to_string();
        let notes = lines.map(|l| l.to_string()).collect();
        Coded {
            code,
            severity,
            message,
            files: Vec::new(),
            notes,
            suggestions: Vec::new(),
            locations: Vec::new(),
            rendered,
        }
    }

    /// The JSON record, shaped by the detail level, and the suggestion texts
    /// that were new (a suggestion given in an earlier record is not
    /// repeated at `normal` and `brief`).
    fn record_tracked(&self) -> (Value, Vec<String>) {
        let d = detail();
        let mut m = Map::new();
        m.insert("schema".into(), json!(SCHEMA));
        m.insert("type".into(), json!("diagnostic"));
        m.insert("code".into(), json!(self.code));
        m.insert("severity".into(), json!(self.severity.as_str()));
        m.insert("message".into(), json!(self.message));
        let mut fresh = Vec::new();
        if d == Detail::Full {
            m.insert("files".into(), json!(self.files));
            m.insert("symbols".into(), json!(Vec::<String>::new()));
            m.insert("notes".into(), json!(self.notes));
            m.insert("locations".into(), json!(self.locations));
            m.insert(
                "suggestions".into(),
                Value::Array(
                    self.suggestions
                        .iter()
                        .map(|(m, o)| json!({"message": m, "option": o}))
                        .collect(),
                ),
            );
            m.insert(
                "rendered".into(),
                json!(format!("hw: {}", self.rendered)),
            );
            return (Value::Object(m), fresh);
        }
        // A path the message already names is not repeated.
        if d == Detail::Normal
            && self.files.iter().any(|f| !self.message.contains(f.as_str()))
        {
            m.insert("files".into(), json!(self.files));
        }
        if d == Detail::Normal && !self.notes.is_empty() {
            m.insert("notes".into(), json!(self.notes));
        }
        if !self.locations.is_empty() {
            let take = if d == Detail::Brief { 1 } else { self.locations.len() };
            let locs: Vec<Value> = self
                .locations
                .iter()
                .take(take)
                .map(|l| {
                    let mut l = l.clone();
                    if d == Detail::Brief {
                        if let Some(o) = l.as_object_mut() {
                            o.remove("text");
                        }
                    }
                    l
                })
                .collect();
            m.insert("locations".into(), Value::Array(locs));
        }
        let mut sugg = Vec::new();
        {
            let mut seen = SUGGESTED.lock().unwrap();
            let seen = seen.get_or_insert_with(HashSet::new);
            for (msg, opt) in &self.suggestions {
                if sugg.len() >= if d == Detail::Brief { 1 } else { 3 } {
                    break;
                }
                if !seen.insert(msg.clone()) {
                    continue;
                }
                fresh.push(msg.clone());
                let mut e = Map::new();
                e.insert("message".into(), json!(msg));
                if let Some(o) = opt {
                    e.insert("option".into(), json!(o));
                }
                sugg.push(Value::Object(e));
            }
        }
        if !sugg.is_empty() {
            m.insert("suggestions".into(), Value::Array(sugg));
        }
        (Value::Object(m), fresh)
    }

    fn record(&self) -> Value {
        self.record_tracked().0
    }
}

fn count(severity: Severity) {
    let c = if severity == Severity::Warning {
        &WARNING_COUNT
    } else {
        &ERROR_COUNT
    };
    c.fetch_add(1, Ordering::Relaxed);
}

/// Print one diagnostic, as JSON or as the usual text. The usual `--no-messages`
/// switch silences both. In stable mode the diagnostics are held until the
/// end and printed sorted.
#[cold]
#[inline(never)]
pub(crate) fn report(d: Coded) {
    count(d.severity);
    if d.severity != Severity::Warning && collecting() {
        note_error(d.rendered.clone());
    }
    if !crate::messages::messages() {
        return;
    }
    if STABLE.load(Ordering::Relaxed) {
        let key = format!("{}\0{}\0{}", d.code, d.files.join(","), d.message);
        HELD.lock().unwrap().push((key, d));
    } else {
        emit(&d);
    }
}

/// Print one diagnostic now. At `normal` and `brief` the records on standard
/// error keep to a size budget: past 12 KB only errors, past 20 KB nothing
/// (the summary counts what was left out). The `RUSTY_AGENT_OUT` copy is
/// never cut.
fn emit(d: &Coded) {
    if !json_diagnostics() {
        eprintln_locked!("{}", d.rendered);
        return;
    }
    let (record, fresh) = d.record_tracked();
    let line = record.to_string();
    tee(&line);
    if !full() {
        let used = EMITTED.load(Ordering::Relaxed);
        if used >= HARD_BUDGET
            || used >= SOFT_BUDGET && d.severity == Severity::Warning
        {
            *OMITTED.lock().unwrap().entry("diagnostic").or_insert(0) += 1;
            if let Some(seen) = SUGGESTED.lock().unwrap().as_mut() {
                for f in &fresh {
                    seen.remove(f);
                }
            }
            return;
        }
    }
    EMITTED.fetch_add(line.len() as u64 + 1, Ordering::Relaxed);
    crate::messages::write_stderr_line(&line);
}

/// Report a problem with one file or directory, from its I/O error.
#[cold]
#[inline(never)]
pub(crate) fn file_error(path: &Path, err: &std::io::Error) {
    // Reading a file never fails because the disk is full: this is the
    // output, which the search writes as it goes. Say so once.
    if matches!(
        err.kind(),
        std::io::ErrorKind::StorageFull
            | std::io::ErrorKind::QuotaExceeded
            | std::io::ErrorKind::WriteZero
            | std::io::ErrorKind::FileTooLarge
    ) {
        output_error(err);
        return;
    }
    let code = io_code(err);
    let mut d = Coded::new(
        code,
        Severity::Error,
        format!("{}: {}", path.display(), err),
    );
    d.files.push(path.to_string_lossy().into_owned());
    io_suggestions(&mut d, err);
    report(d);
}

/// Report an error from the directory walk (it names its own path).
#[cold]
#[inline(never)]
pub(crate) fn walk_error(err: &ignore::Error) {
    let io = err.io_error();
    let code = io.map_or("G0002", io_code);
    let mut d = Coded::new(code, Severity::Error, err.to_string());
    if let Some(p) = error_path(err) {
        d.files.push(p.to_string_lossy().into_owned());
    }
    if let Some(io) = io {
        io_suggestions(&mut d, io);
    } else if matches!(err, ignore::Error::Loop { .. }) {
        d.suggestions.push((
            "a symbolic link points back to a parent directory; the link is \
             not followed again"
                .to_string(),
            None,
        ));
    }
    report(d);
}

fn error_path(err: &ignore::Error) -> Option<&Path> {
    match err {
        ignore::Error::WithPath { path, .. } => Some(path),
        ignore::Error::WithDepth { err, .. } => error_path(err),
        ignore::Error::WithLineNumber { err, .. } => error_path(err),
        ignore::Error::Loop { child, .. } => Some(child),
        ignore::Error::Partial(errs) => errs.iter().find_map(error_path),
        _ => None,
    }
}

fn io_code(err: &std::io::Error) -> &'static str {
    if err.kind() == std::io::ErrorKind::NotFound { "G0003" } else { "G0002" }
}

fn io_suggestions(d: &mut Coded, err: &std::io::Error) {
    match err.kind() {
        std::io::ErrorKind::NotFound => d.suggestions.push((
            "check the spelling of the path; nothing was searched there"
                .to_string(),
            None,
        )),
        std::io::ErrorKind::PermissionDenied => d.suggestions.push((
            "no permission to read it, so it was skipped and its matches \
             are missing from the result; fix the permissions or run as a \
             user who can read it"
                .to_string(),
            None,
        )),
        _ => {}
    }
}

/// Report a warning about ignore files or globs.
#[cold]
#[inline(never)]
pub(crate) fn ignore_warning(text: String) {
    if !crate::messages::ignore_messages() {
        return;
    }
    let mut d = Coded::new("G0006", Severity::Warning, text);
    d.suggestions.push((
        "the rule is skipped; fix the line in the ignore file, or turn the \
         ignore file off"
            .to_string(),
        Some("--no-ignore"),
    ));
    report(d);
}

/// Report that the output could not be written.
#[cold]
#[inline(never)]
pub(crate) fn output_error(err: &std::io::Error) {
    crate::messages::set_errored();
    // Once: every later write fails the same way, and one record says so.
    if OUTPUT_FAILED.swap(true, Ordering::Relaxed) {
        return;
    }
    let mut d = Coded::new("G0009", Severity::Error, err.to_string());
    d.suggestions.push((
        "the results could not be written (the output is full, closed or \
         not writable); the summary counts what was searched"
            .to_string(),
        None,
    ));
    report(d);
}

/// The text the search prints (without agent mode) when it searched nothing.
const NOTHING_TEXT: &str = "No files were searched, which means ripgrep \
probably applied a filter you didn't expect.\n\
Running with --debug will show why files are being skipped.";

/// The diagnostic for "no file was searched". `implicit` is true when no
/// path was given (the current directory was searched).
fn nothing_searched_coded(implicit: bool) -> Coded {
    let mut d = Coded::new(
        "G0007",
        Severity::Warning,
        NOTHING_TEXT.to_string(),
    );
    d.message = if implicit {
        "no files were searched in the current directory (no path was \
         given): a filter probably hid everything"
            .to_string()
    } else {
        "no files were searched in the given path(s): a filter probably hid \
         everything"
            .to_string()
    };
    if !full() {
        // The advice is a suggestion below; the second line says it again.
        d.notes.clear();
    }
    d.suggestions.push((
        "--debug shows why each file is skipped".to_string(),
        Some("--debug"),
    ));
    if !collecting() {
        d.suggestions.push((
            "--accounting lists what was skipped and the option that \
             includes it"
                .to_string(),
            Some("--accounting"),
        ));
    }
    d
}

/// Whether the output could not be written (the search may as well stop).
pub(crate) fn output_failed() -> bool {
    OUTPUT_FAILED.load(Ordering::Relaxed)
}

/// "No files were searched" with no path given: the filters probably hid
/// everything. (With a path given, the end of the run says the same.)
pub(crate) fn nothing_searched(_text: &str) {
    NOTHING_REPORTED.store(true, Ordering::Relaxed);
    report(nothing_searched_coded(true));
}

/// Print the error that ended the run. The text is what is printed without
/// agent mode, unless JSON diagnostics are on.
pub(crate) fn fatal(err: &anyhow::Error) {
    let coded = err.chain().find_map(|c| c.downcast_ref::<Coded>());
    let early = ARGS_STAGE.load(Ordering::Relaxed);
    // While the options are still being read the environment and the command
    // line decide (the flags that set JSON_DIAGNOSTICS are not applied yet).
    let json = if early { json_for_early_error() } else { json_diagnostics() };
    if !json {
        eprintln_locked!("{:#}", err);
        return;
    }
    count(Severity::Fatal);
    flush_held();
    let record = match coded {
        Some(c) => c.record(),
        None => {
            let mut d = Coded::new(
                if early { "G0004" } else { "G0300" },
                Severity::Fatal,
                format!("{:#}", err),
            );
            if early {
                d.suggestions.push((
                    "--help lists the options".to_string(),
                    Some("--help"),
                ));
            }
            d.record()
        }
    };
    let mut record = record;
    if coded.is_some_and(|c| c.severity != Severity::Fatal) {
        record["severity"] = json!("fatal");
    }
    crate::messages::print_raw_stderr(&record.to_string());
}

/// Build the diagnostic for a pattern that did not compile. Each pattern is
/// parsed alone to find which one is wrong and where.
pub(crate) fn regex_error(
    patterns: &[String],
    rendered: String,
    fixed: bool,
) -> Coded {
    let mut d = Coded::new("G0001", Severity::Fatal, rendered.clone());
    if !fixed {
        for (i, pat) in patterns.iter().enumerate() {
            let found = regex_syntax::ast::parse::Parser::new()
                .parse(pat)
                .err()
                .map(|e| (e.span().clone(), e.kind().to_string(), Some(e)))
                .or_else(|| {
                    regex_syntax::ParserBuilder::new()
                        .build()
                        .parse(pat)
                        .err()
                        .and_then(|e| match e {
                            regex_syntax::Error::Translate(t) => Some((
                                t.span().clone(),
                                t.kind().to_string(),
                                None,
                            )),
                            _ => None,
                        })
                });
            let Some((span, kind, ast)) = found else { continue };
            d.message = format!("regex parse error: {kind}");
            d.locations.push(json!({
                "pattern": i,
                "text": pat,
                "start": span.start.offset,
                "end": span.end.offset,
                "column": span.start.column,
            }));
            d.files.clear();
            let advice = match ast.as_ref().map(|e| e.kind()) {
                Some(
                    regex_syntax::ast::ErrorKind::UnsupportedBackreference,
                )
                | Some(regex_syntax::ast::ErrorKind::UnsupportedLookAround) => {
                    Some((
                        "look-around and backreferences need the PCRE2 engine"
                            .to_string(),
                        if cfg!(feature = "pcre2") {
                            Some("-P")
                        } else {
                            None
                        },
                    ))
                }
                Some(regex_syntax::ast::ErrorKind::GroupUnclosed)
                | Some(regex_syntax::ast::ErrorKind::GroupUnopened) => Some((
                    "close the group, escape a literal parenthesis as \\( \
                         or \\), or use -F to search for the text as it is"
                        .to_string(),
                    Some("-F"),
                )),
                Some(regex_syntax::ast::ErrorKind::RepetitionMissing) => {
                    Some((
                        "a repetition operator (*, + or ?) needs something \
                         before it: escape it as \\* or use -F to search for \
                         the text as it is"
                            .to_string(),
                        Some("-F"),
                    ))
                }
                Some(regex_syntax::ast::ErrorKind::ClassUnclosed) => Some((
                    "close the character class with ] or escape [ as \\[, or \
                     use -F to search for the text as it is"
                        .to_string(),
                    Some("-F"),
                )),
                _ => Some((
                    "to search for the text literally, use -F/--fixed-strings"
                        .to_string(),
                    Some("-F"),
                )),
            };
            d.suggestions.extend(advice);
            break;
        }
    }
    if !d.locations.is_empty() && !full() {
        // The locations say where the mistake is; the text-mode diagram of
        // the same place is not repeated.
        d.notes.clear();
    }
    if d.locations.is_empty() && rendered.contains("exceeds size limit") {
        d.suggestions.push((
            "the pattern compiles to more than the size limit: shorten it, \
             or raise --regex-size-limit"
                .to_string(),
            None,
        ));
    }
    d
}

/// An error reading a pattern file given with `-f`.
pub(crate) fn pattern_file_error(
    path: &Path,
    err: anyhow::Error,
) -> anyhow::Error {
    if !json_diagnostics() {
        return err;
    }
    let mut d = Coded::new("G0005", Severity::Fatal, format!("{:#}", err));
    d.files.push(path.to_string_lossy().into_owned());
    d.suggestions
        .push(("check the path given to -f/--file".to_string(), None));
    anyhow::Error::new(d)
}

#[cfg(test)]
mod tests {
    use super::CODES;

    /// Every code written in the source is in the table, and the other way.
    #[test]
    fn codes_are_listed() {
        let sources = [
            include_str!("agent.rs"),
            include_str!("git.rs"),
            include_str!("main.rs"),
            include_str!("haystack.rs"),
            include_str!("limit.rs"),
            include_str!("allterms.rs"),
        ];
        let mut seen = std::collections::BTreeSet::new();
        for src in sources {
            for (i, _) in src.match_indices("\"G0") {
                let code = &src[i + 1..i + 6];
                if code.chars().skip(1).all(|c| c.is_ascii_digit()) {
                    seen.insert(code.to_string());
                }
            }
        }
        let listed: std::collections::BTreeSet<String> =
            CODES.iter().map(|c| c.0.to_string()).collect();
        assert_eq!(listed, seen);
    }
}
