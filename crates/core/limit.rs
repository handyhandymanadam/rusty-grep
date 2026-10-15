/*!
`--limit` and `--offset`: one global window over the ordered result.

The files are searched on all cores, each into its own buffer, and the buffers
are looked at strictly in file order. The number of lines (or files, with
`--count` and the like) before a file is known by the time the file's turn
comes, so the buffer is printed whole when it lies inside the window, dropped
when it lies outside, and, when the window edge cuts through the file, the file
is searched again with only the lines of the window printed. The whole input is
always searched, so the totals are exact.
*/

use std::{
    collections::BTreeMap,
    sync::{
        Condvar, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
};

use crate::flags::{HiArgs, SearchMode};

/// What one searched file left behind.
struct FileOut {
    /// Index into the list of files.
    index: usize,
    /// The printed output, if any.
    buf: Option<termcolor::Buffer>,
    /// How many window units the file holds: matching lines, or 1 for a file
    /// that printed something in a count or file-list mode.
    units: u64,
}

/// The totals the summary record reports for a windowed search.
pub(crate) struct Paging {
    pub(crate) limit: Option<u64>,
    pub(crate) offset: u64,
    pub(crate) unit: &'static str,
    pub(crate) returned: u64,
    pub(crate) total: u64,
    pub(crate) total_files: u64,
    pub(crate) next_offset: Option<u64>,
    pub(crate) exact: bool,
}

pub(crate) fn search_limited(
    args: &HiArgs,
    mode: SearchMode,
) -> anyhow::Result<bool> {
    let started_at = std::time::Instant::now();
    let haystacks = crate::sorted_haystacks(args)?;
    let count = haystacks.len();
    let lines = matches!(mode, SearchMode::Standard);
    let lo = args.offset();
    let hi = args.limit().map_or(u64::MAX, |n| lo.saturating_add(n));

    let bufwtr = args.buffer_writer();
    let stats =
        std::sync::Mutex::new(args.stats().expect("--limit keeps statistics"));
    let stop = AtomicBool::new(false);
    const CHUNK: usize = 8;
    const MAX_PENDING_BYTES: usize = 8 << 20;
    let chunks = count.div_ceil(CHUNK);
    let next = AtomicUsize::new(0);
    let pending_bytes = AtomicUsize::new(0);
    let printed = (Mutex::new(0usize), Condvar::new());
    let threads = args.sorted_search_threads().min(chunks.max(1));
    let window = threads * 2;

    let mut searcher = args.search_worker(
        args.matcher()?,
        args.searcher()?,
        args.printer(mode, bufwtr.buffer()),
    )?;
    let mut paging = Paging {
        limit: args.limit(),
        offset: lo,
        unit: if lines { "lines" } else { "files" },
        returned: 0,
        total: 0,
        total_files: 0,
        next_offset: None,
        exact: true,
    };
    let (tx, rx) = mpsc::channel::<(usize, Vec<FileOut>)>();
    std::thread::scope(|scope| {
        for _ in 0..threads {
            let tx = tx.clone();
            let mut searcher = searcher.clone();
            let (haystacks, bufwtr, stats) = (&haystacks, &bufwtr, &stats);
            let (stop, next, printed, pending_bytes) =
                (&stop, &next, &printed, &pending_bytes);
            scope.spawn(move || {
                loop {
                    if stop.load(Ordering::SeqCst) || crate::agent::time_up() {
                        break;
                    }
                    let c = next.fetch_add(1, Ordering::SeqCst);
                    if c >= chunks {
                        break;
                    }
                    {
                        let (lock, cvar) = printed;
                        let mut done = lock.lock().unwrap();
                        while c > *done
                            && (c >= *done + window
                                || pending_bytes.load(Ordering::SeqCst)
                                    > MAX_PENDING_BYTES)
                            && !stop.load(Ordering::SeqCst)
                        {
                            done = cvar.wait(done).unwrap();
                        }
                    }
                    let mut out = Vec::new();
                    let mut out_bytes = 0;
                    let end = ((c + 1) * CHUNK).min(count);
                    for index in c * CHUNK..end {
                        if stop.load(Ordering::SeqCst) {
                            return;
                        }
                        if crate::agent::time_up() {
                            break;
                        }
                        let haystack = &haystacks[index];
                        searcher.printer().get_mut().clear();
                        let mut matched_lines = 0;
                        match searcher.search(haystack) {
                            Ok(result) => {
                                if let Some(s) = result.stats() {
                                    matched_lines = s.matched_lines();
                                    *stats.lock().unwrap() += s;
                                }
                            }
                            Err(err) => {
                                crate::messages::set_errored();
                                crate::agent::file_error(
                                    haystack.path(),
                                    &err,
                                );
                            }
                        }
                        let has_output =
                            !searcher.printer().get_mut().is_empty();
                        let units = if lines {
                            matched_lines
                        } else {
                            has_output as u64
                        };
                        if units == 0 && !has_output {
                            continue;
                        }
                        let buf = has_output.then(|| {
                            std::mem::replace(
                                searcher.printer().get_mut(),
                                bufwtr.buffer(),
                            )
                        });
                        out_bytes += buf.as_ref().map_or(0, |b| b.len());
                        out.push(FileOut { index, buf, units });
                    }
                    pending_bytes.fetch_add(out_bytes, Ordering::SeqCst);
                    if tx.send((c, out)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);
        let mut pending = BTreeMap::new();
        let mut want = 0usize;
        // Units before the file being looked at.
        let mut base = 0u64;
        for (c, outs) in rx {
            pending.insert(c, outs);
            let mut advanced = false;
            while let Some(outs) = pending.remove(&want) {
                for fo in outs {
                    let start = base;
                    let end = base + fo.units;
                    base = end;
                    paging.total += fo.units;
                    paging.total_files += (fo.units > 0) as u64;
                    let from = start.max(lo);
                    let to = end.min(hi);
                    if from < to {
                        paging.returned += to - from;
                    }
                    if let Some(ref buf) = fo.buf {
                        pending_bytes.fetch_sub(buf.len(), Ordering::SeqCst);
                    }
                    if from >= to || stop.load(Ordering::SeqCst) {
                        continue;
                    }
                    let whole = start >= lo && end <= hi;
                    let res = if whole {
                        match fo.buf {
                            Some(ref buf) => bufwtr.print(buf),
                            None => Ok(()),
                        }
                    } else {
                        // The window edge cuts through this file.
                        let haystack = &haystacks[fo.index];
                        searcher.printer().get_mut().clear();
                        let cut = searcher.search_window(
                            haystack,
                            from - start,
                            to - start,
                        );
                        match cut {
                            Ok(true) => {
                                let buf = std::mem::replace(
                                    searcher.printer().get_mut(),
                                    bufwtr.buffer(),
                                );
                                bufwtr.print(&buf)
                            }
                            Ok(false) => {
                                paging.exact = false;
                                match fo.buf {
                                    Some(ref buf) => bufwtr.print(buf),
                                    None => Ok(()),
                                }
                            }
                            Err(err) => Err(err),
                        }
                    };
                    if let Err(err) = res {
                        if err.kind() != std::io::ErrorKind::BrokenPipe {
                            crate::agent::output_error(&err);
                        }
                        stop.store(true, Ordering::SeqCst);
                    }
                }
                want += 1;
                advanced = true;
            }
            if advanced {
                let (lock, cvar) = &printed;
                *lock.lock().unwrap() = want;
                cvar.notify_all();
            }
            if stop.load(Ordering::SeqCst) {
                printed.1.notify_all();
                break;
            }
        }
        stop.store(true, Ordering::SeqCst);
        printed.1.notify_all();
    });
    paging.next_offset = match args.limit() {
        Some(_) if hi < paging.total => Some(hi),
        _ => None,
    };
    if args.has_implicit_path() && count == 0 {
        crate::eprint_nothing_searched();
    }
    let returned = paging.returned;
    crate::agent::note_paging(paging);
    {
        let stats = stats.lock().unwrap();
        let mut wtr = searcher.printer().get_mut();
        let _ = crate::print_stats(mode, &stats, started_at, &mut wtr);
        let _ = bufwtr.print(&mut wtr);
    }
    Ok(returned > 0)
}
