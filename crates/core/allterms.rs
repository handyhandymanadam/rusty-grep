/*!
`--all-in-file` and `--within N`: every pattern in the same file, or every
pattern within N lines of each other.

One pass per file with the "any pattern" matcher finds the lines that matter;
each of those lines is then checked against the single patterns, and the
latest line of each pattern is remembered. The pass stops at the first place
where all patterns are present (for `--all-in-file`, `-l` and `-L`), so a file
is read only as far as needed.
*/

use std::{
    io,
    sync::Mutex,
    sync::atomic::{AtomicU64, AtomicUsize, Ordering},
};

use grep::{
    matcher::Matcher,
    regex::RegexMatcher,
    searcher::{Searcher, Sink, SinkFinish, SinkMatch},
};

use crate::flags::{HiArgs, SearchMode, Terms};

/// One place where all patterns are present: its lines, in file order.
struct Place {
    lines: Vec<(u64, Vec<u8>)>,
}

struct TermSink<'a> {
    terms: &'a [RegexMatcher],
    /// The latest line each pattern matched on.
    last: Vec<Option<(u64, Vec<u8>)>>,
    /// `None`: anywhere in the file.
    within: Option<u64>,
    /// Stop at the first place.
    first_only: bool,
    places: Vec<Place>,
    /// Bytes the search read (it may stop early).
    bytes: u64,
}

impl Sink for TermSink<'_> {
    type Error = io::Error;

    fn matched(
        &mut self,
        _: &Searcher,
        mat: &SinkMatch<'_>,
    ) -> Result<bool, io::Error> {
        let line = mat.line_number().unwrap_or(0);
        let text = mat.bytes();
        for (i, m) in self.terms.iter().enumerate() {
            if let Some((l, _)) = self.last[i] {
                if l == line {
                    continue;
                }
            }
            if m.is_match(text).unwrap_or(false) {
                self.last[i] = Some((line, text.to_vec()));
            }
        }
        if self.last.iter().any(|l| l.is_none()) {
            return Ok(true);
        }
        let (mut lo, mut hi) = (u64::MAX, 0);
        for l in self.last.iter().flatten() {
            lo = lo.min(l.0);
            hi = hi.max(l.0);
        }
        if self.within.is_some_and(|n| hi - lo > n) {
            return Ok(true);
        }
        let mut lines: Vec<(u64, Vec<u8>)> =
            self.last.iter_mut().filter_map(|l| l.take()).collect();
        lines.sort_by_key(|l| l.0);
        lines.dedup_by_key(|l| l.0);
        self.places.push(Place { lines });
        Ok(!self.first_only)
    }

    fn finish(
        &mut self,
        _: &Searcher,
        finish: &SinkFinish,
    ) -> Result<(), io::Error> {
        self.bytes = finish.byte_count();
        Ok(())
    }
}

struct FileResult {
    places: Vec<Place>,
}

pub(crate) fn search_terms(
    args: &HiArgs,
    mode: SearchMode,
    terms: Terms,
) -> anyhow::Result<bool> {
    use std::io::Write;

    let file_level = matches!(terms, Terms::File);
    match mode {
        SearchMode::Standard
        | SearchMode::FilesWithMatches
        | SearchMode::FilesWithoutMatch => {}
        SearchMode::Count if !file_level => {}
        _ => anyhow::bail!(
            "this output mode cannot be used with --all-in-file or --within"
        ),
    }
    let (any, each) = args.term_matchers()?;
    let haystacks = crate::sorted_haystacks(args)?;
    let first_only = file_level
        || matches!(
            mode,
            SearchMode::FilesWithMatches | SearchMode::FilesWithoutMatch
        );
    let within = match terms {
        Terms::File => None,
        Terms::Within(n) => Some(n),
    };
    let searcher = args.searcher_for_terms()?;
    let next = AtomicUsize::new(0);
    let bytes_read = AtomicU64::new(0);
    let results: Mutex<Vec<Option<FileResult>>> =
        Mutex::new((0..haystacks.len()).map(|_| None).collect());
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(32)
        .min(haystacks.len().max(1));
    std::thread::scope(|scope| {
        for _ in 0..threads {
            let mut searcher = searcher.clone();
            let (any, each, haystacks, next, results, bytes_read) =
                (&any, &each, &haystacks, &next, &results, &bytes_read);
            scope.spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    if i >= haystacks.len() || crate::agent::time_up() {
                        break;
                    }
                    let h = &haystacks[i];
                    searcher.set_binary_detection(
                        args.binary_detection(h.is_explicit()),
                    );
                    let mut sink = TermSink {
                        terms: each,
                        last: vec![None; each.len()],
                        within,
                        first_only,
                        places: Vec::new(),
                        bytes: 0,
                    };
                    let res = if h.is_stdin() {
                        searcher.search_reader(
                            any,
                            io::stdin().lock(),
                            &mut sink,
                        )
                    } else {
                        searcher.search_path(any, h.path(), &mut sink)
                    };
                    bytes_read.fetch_add(sink.bytes, Ordering::Relaxed);
                    if let Err(err) = res {
                        crate::messages::set_errored();
                        crate::agent::file_error(h.path(), &err);
                        continue;
                    }
                    if !sink.places.is_empty()
                        || matches!(mode, SearchMode::FilesWithoutMatch)
                    {
                        results.lock().unwrap()[i] =
                            Some(FileResult { places: sink.places });
                    }
                }
            });
        }
    });
    let results = results.into_inner().unwrap();
    let mut out = args.stdout();
    let term = args.path_terminator().unwrap_or(b'\n');
    let mut any_output = false;
    let (mut files_with, mut total_places) = (0u64, 0u64);
    let mut first_place = true;
    for (i, res) in results.iter().enumerate() {
        let path = haystacks[i].path();
        let bytes = path.as_os_str().as_encoded_bytes();
        let Some(res) = res else { continue };
        let has = !res.places.is_empty();
        if has {
            files_with += 1;
            total_places += res.places.len() as u64;
        }
        let r: io::Result<()> = (|| {
            match mode {
                SearchMode::FilesWithoutMatch => {
                    if !has {
                        out.write_all(bytes)?;
                        out.write_all(&[term])?;
                        any_output = true;
                    }
                }
                SearchMode::FilesWithMatches => {
                    out.write_all(bytes)?;
                    out.write_all(&[term])?;
                    any_output = true;
                }
                _ if file_level => {
                    out.write_all(bytes)?;
                    out.write_all(&[term])?;
                    any_output = true;
                }
                SearchMode::Count => {
                    out.write_all(bytes)?;
                    write!(out, ":{}\n", res.places.len())?;
                    any_output = true;
                }
                _ => {
                    for place in &res.places {
                        if !first_place {
                            out.write_all(b"--\n")?;
                        }
                        first_place = false;
                        for (line, text) in &place.lines {
                            if args.with_filename() {
                                out.write_all(bytes)?;
                                out.write_all(b":")?;
                            }
                            write!(out, "{line}:")?;
                            out.write_all(text)?;
                            if !text.ends_with(b"\n") {
                                out.write_all(b"\n")?;
                            }
                        }
                    }
                    any_output = true;
                }
            }
            Ok(())
        })();
        if let Err(err) = r {
            if err.kind() != io::ErrorKind::BrokenPipe {
                crate::agent::output_error(&err);
            }
            break;
        }
    }
    let _ = out.flush();
    if args.has_implicit_path() && haystacks.is_empty() {
        crate::eprint_nothing_searched();
    }
    crate::agent::note_terms(
        total_places,
        files_with,
        haystacks.len() as u64,
        bytes_read.load(Ordering::Relaxed),
    );
    Ok(any_output)
}
