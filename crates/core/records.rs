/*!
`--records` and `--patch`: two extra ways to print search results, for
programs and scripts.

`--records` prints one JSON line per match with everything needed to edit
there (path, line, column, byte offsets, the exact text, the whole line).
`--patch` (with `--replace`) prints what `--replace` would change as a
unified diff and writes nothing: neither printer ever writes to a file, they
only produce text.
*/

use std::{io, path::Path};

use {
    grep::{
        matcher::{Captures, Matcher},
        searcher::{
            Searcher, Sink, SinkContext, SinkContextKind, SinkFinish,
            SinkMatch,
        },
    },
    serde_json::{Value, json},
};

use crate::agent::SCHEMA;

fn io_err<E: std::fmt::Display>(e: E) -> io::Error {
    io::Error::other(e.to_string())
}

/// A JSON string for the bytes, and whether they were not valid UTF-8.
fn text(bytes: &[u8]) -> (Value, bool) {
    match std::str::from_utf8(bytes) {
        Ok(s) => (Value::String(s.to_string()), false),
        Err(_) => {
            (Value::String(String::from_utf8_lossy(bytes).into_owned()), true)
        }
    }
}

fn path_text(path: &Path) -> Value {
    Value::String(path.to_string_lossy().into_owned())
}

fn trim_line(line: &[u8]) -> &[u8] {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    line.strip_suffix(b"\r").unwrap_or(line)
}

fn stats_for(
    matches: u64,
    matched_lines: u64,
    finish: &SinkFinish,
    started: std::time::Instant,
) -> grep::printer::Stats {
    let mut s = grep::printer::Stats::new();
    s.add_searches(1);
    if matched_lines > 0 {
        s.add_searches_with_match(1);
    }
    s.add_matches(matches);
    s.add_matched_lines(matched_lines);
    s.add_bytes_searched(finish.byte_count());
    s.add_elapsed(started.elapsed());
    s
}

// ---------------------------------------------------------------------------
// --records
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub(crate) struct RecordsPrinter<W> {
    wtr: W,
    enclosing: bool,
}

impl<W: io::Write> RecordsPrinter<W> {
    pub(crate) fn new(wtr: W, enclosing: bool) -> RecordsPrinter<W> {
        RecordsPrinter { wtr, enclosing }
    }

    pub(crate) fn get_mut(&mut self) -> &mut W {
        &mut self.wtr
    }

    pub(crate) fn sink_with_path<'p, 's, M: Matcher>(
        &'s mut self,
        matcher: &'s M,
        path: &'p Path,
    ) -> RecordsSink<'p, 's, M, W> {
        RecordsSink {
            wtr: &mut self.wtr,
            matcher,
            path,
            want_enclosing: self.enclosing,
            index: None,
            index_tried: false,
            matches: 0,
            lines: 0,
            binary: None,
            stats: None,
            started: std::time::Instant::now(),
        }
    }
}

pub(crate) struct RecordsSink<'p, 's, M, W> {
    wtr: &'s mut W,
    matcher: &'s M,
    path: &'p Path,
    want_enclosing: bool,
    index: Option<crate::enclosing::Index>,
    index_tried: bool,
    matches: u64,
    lines: u64,
    binary: Option<u64>,
    stats: Option<grep::printer::Stats>,
    started: std::time::Instant,
}

impl<M, W> RecordsSink<'_, '_, M, W> {
    pub(crate) fn has_match(&self) -> bool {
        self.lines > 0
    }
    pub(crate) fn stats(&self) -> Option<grep::printer::Stats> {
        self.stats.clone()
    }
    pub(crate) fn binary_byte_offset(&self) -> Option<u64> {
        self.binary
    }
}

impl<M: Matcher, W: io::Write> Sink for RecordsSink<'_, '_, M, W> {
    type Error = io::Error;

    fn matched(
        &mut self,
        searcher: &Searcher,
        mat: &SinkMatch<'_>,
    ) -> Result<bool, io::Error> {
        let bytes = mat.bytes();
        let base = mat.absolute_byte_offset();
        let first_line = mat.line_number();
        let mut found = Vec::new();
        if !searcher.invert_match() {
            self.matcher
                .find_iter(bytes, |m| {
                    found.push(m);
                    true
                })
                .map_err(io_err)?;
        }
        self.lines +=
            bytes.iter().filter(|&&b| b == b'\n').count().max(1) as u64;
        let line_info = |pos: usize| -> (u64, usize, usize) {
            let idx = bytes[..pos].iter().filter(|&&b| b == b'\n').count();
            let start = bytes[..pos]
                .iter()
                .rposition(|&b| b == b'\n')
                .map_or(0, |p| p + 1);
            let end = bytes[pos..]
                .iter()
                .position(|&b| b == b'\n')
                .map_or(bytes.len(), |p| pos + p);
            (idx as u64, start, end)
        };
        if found.is_empty() {
            // No single match to point at (inverted search): one record per
            // line.
            let mut pos = 0;
            while pos < bytes.len() {
                let (idx, start, end) = line_info(pos);
                let (line_text, lossy) = text(trim_line(&bytes[start..end]));
                let mut rec = json!({
                    "schema": SCHEMA,
                    "type": "result",
                    "path": path_text(self.path),
                    "line": first_line.map(|l| l + idx),
                    "column": Value::Null,
                    "byte_offset": base + start as u64,
                    "line_offset": base + start as u64,
                    "match": Value::Null,
                    "line_text": line_text,
                });
                if lossy {
                    rec["lossy"] = json!(true);
                }
                self.add_enclosing(&mut rec);
                self.write_rec(&rec)?;
                pos = end + 1;
                self.matches += 1;
            }
            return Ok(true);
        }
        for m in found {
            let (idx, start, end) = line_info(m.start());
            let (matched, l1) = text(&bytes[m.start()..m.end()]);
            let (line_text, l2) = text(trim_line(&bytes[start..end]));
            let mut rec = json!({
                "schema": SCHEMA,
                "type": "result",
                "path": path_text(self.path),
                "line": first_line.map(|l| l + idx),
                "column": m.start() - start + 1,
                "byte_offset": base + m.start() as u64,
                "end_byte_offset": base + m.end() as u64,
                "line_offset": base + start as u64,
                "match": matched,
                "line_text": line_text,
            });
            if m.end() > end + 1 {
                let (idx2, _, _) = line_info(m.end().saturating_sub(1));
                rec["end_line"] = json!(first_line.map(|l| l + idx2));
            }
            if l1 || l2 {
                rec["lossy"] = json!(true);
            }
            self.add_enclosing(&mut rec);
            self.write_rec(&rec)?;
            self.matches += 1;
        }
        Ok(true)
    }

    /// A context line (`-A`, `-B`, `-C`): a `context` record with the line
    /// number and text, so the lines around a result are not lost.
    fn context(
        &mut self,
        _: &Searcher,
        ctx: &SinkContext<'_>,
    ) -> Result<bool, io::Error> {
        let (line_text, lossy) = text(trim_line(ctx.bytes()));
        let mut rec = json!({
            "schema": SCHEMA,
            "type": "context",
            "path": path_text(self.path),
            "line": ctx.line_number(),
            "line_offset": ctx.absolute_byte_offset(),
            "line_text": line_text,
        });
        if lossy {
            rec["lossy"] = json!(true);
        }
        self.write_rec(&rec)?;
        Ok(true)
    }

    fn binary_data(
        &mut self,
        _: &Searcher,
        offset: u64,
    ) -> Result<bool, io::Error> {
        self.binary = Some(offset);
        Ok(true)
    }

    fn finish(
        &mut self,
        _: &Searcher,
        finish: &SinkFinish,
    ) -> Result<(), io::Error> {
        if self.binary.is_none() {
            self.binary = finish.binary_byte_offset();
        }
        self.stats =
            Some(stats_for(self.matches, self.lines, finish, self.started));
        Ok(())
    }
}

impl<M, W: io::Write> RecordsSink<'_, '_, M, W> {
    /// Add the `enclosing` field for the record's line, when asked for.
    fn add_enclosing(&mut self, rec: &mut Value) {
        if !self.want_enclosing {
            return;
        }
        if !self.index_tried {
            self.index_tried = true;
            self.index = crate::enclosing::Index::load(self.path);
        }
        let (Some(index), Some(line)) =
            (self.index.as_ref(), rec["line"].as_u64())
        else {
            return;
        };
        rec["enclosing"] = Value::Array(
            index
                .enclosing(line)
                .into_iter()
                .map(|s| json!({"kind": s.kind, "name": s.name, "line": s.line}))
                .collect(),
        );
    }

    fn write_rec(&mut self, rec: &Value) -> io::Result<()> {
        serde_json::to_writer(&mut *self.wtr, rec)?;
        self.wtr.write_all(b"\n")
    }
}

// ---------------------------------------------------------------------------
// --patch
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub(crate) struct PatchPrinter<W> {
    wtr: W,
    replacement: Vec<u8>,
}

impl<W: io::Write> PatchPrinter<W> {
    pub(crate) fn new(wtr: W, replacement: Vec<u8>) -> PatchPrinter<W> {
        PatchPrinter { wtr, replacement }
    }

    pub(crate) fn get_mut(&mut self) -> &mut W {
        &mut self.wtr
    }

    pub(crate) fn sink_with_path<'p, 's, M: Matcher>(
        &'s mut self,
        matcher: &'s M,
        path: &'p Path,
    ) -> PatchSink<'p, 's, M, W> {
        PatchSink {
            wtr: &mut self.wtr,
            replacement: &self.replacement,
            matcher,
            path,
            entries: Vec::new(),
            lines: 0,
            matches: 0,
            binary: None,
            stats: None,
            started: std::time::Instant::now(),
        }
    }
}

struct Entry {
    /// Line number of the first line.
    line: u64,
    old: Vec<u8>,
    /// The replaced text; `None` for a context line.
    new: Option<Vec<u8>>,
    /// A break (a gap in the line numbers) comes before this entry.
    gap: bool,
}

pub(crate) struct PatchSink<'p, 's, M, W> {
    wtr: &'s mut W,
    replacement: &'s [u8],
    matcher: &'s M,
    path: &'p Path,
    entries: Vec<Entry>,
    lines: u64,
    matches: u64,
    binary: Option<u64>,
    stats: Option<grep::printer::Stats>,
    started: std::time::Instant,
}

impl<M, W> PatchSink<'_, '_, M, W> {
    pub(crate) fn has_match(&self) -> bool {
        self.lines > 0
    }
    pub(crate) fn stats(&self) -> Option<grep::printer::Stats> {
        self.stats.clone()
    }
    pub(crate) fn binary_byte_offset(&self) -> Option<u64> {
        self.binary
    }
}

fn count_lines(b: &[u8]) -> u64 {
    if b.is_empty() {
        return 0;
    }
    let n = b.iter().filter(|&&c| c == b'\n').count() as u64;
    if b.ends_with(b"\n") { n } else { n + 1 }
}

impl<M: Matcher, W: io::Write> Sink for PatchSink<'_, '_, M, W> {
    type Error = io::Error;

    fn matched(
        &mut self,
        _: &Searcher,
        mat: &SinkMatch<'_>,
    ) -> Result<bool, io::Error> {
        let line = mat.line_number().unwrap_or(0);
        let old = mat.bytes();
        let (body, term) = match old.strip_suffix(b"\n") {
            Some(b) => (b, &b"\n"[..]),
            None => (old, &b""[..]),
        };
        let mut caps = self.matcher.new_captures().map_err(io_err)?;
        let mut dst = Vec::new();
        let mut n = 0u64;
        let replacement = self.replacement;
        let matcher = self.matcher;
        matcher
            .replace_with_captures(body, &mut caps, &mut dst, |caps, dst| {
                n += 1;
                caps.interpolate(
                    |name| matcher.capture_index(name),
                    body,
                    replacement,
                    dst,
                );
                true
            })
            .map_err(io_err)?;
        dst.extend_from_slice(term);
        self.lines += count_lines(old);
        self.matches += n;
        let changed = dst != old;
        self.push(line, old.to_vec(), changed.then_some(dst));
        Ok(true)
    }

    fn context(
        &mut self,
        _: &Searcher,
        ctx: &SinkContext<'_>,
    ) -> Result<bool, io::Error> {
        if matches!(ctx.kind(), SinkContextKind::Other) {
            return Ok(true);
        }
        let line = ctx.line_number().unwrap_or(0);
        self.push(line, ctx.bytes().to_vec(), None);
        Ok(true)
    }

    fn binary_data(
        &mut self,
        _: &Searcher,
        offset: u64,
    ) -> Result<bool, io::Error> {
        self.binary = Some(offset);
        Ok(true)
    }

    fn finish(
        &mut self,
        _: &Searcher,
        finish: &SinkFinish,
    ) -> Result<(), io::Error> {
        if self.binary.is_none() {
            self.binary = finish.binary_byte_offset();
        }
        self.write_patch()?;
        self.stats =
            Some(stats_for(self.matches, self.lines, finish, self.started));
        Ok(())
    }
}

impl<M, W> PatchSink<'_, '_, M, W> {
    fn push(&mut self, line: u64, old: Vec<u8>, new: Option<Vec<u8>>) {
        let gap = match self.entries.last() {
            Some(prev) => prev.line + count_lines(&prev.old) != line,
            None => false,
        };
        self.entries.push(Entry { line, old, new, gap });
    }
}

impl<M, W: io::Write> PatchSink<'_, '_, M, W> {
    fn write_patch(&mut self) -> io::Result<()> {
        if !self.entries.iter().any(|e| e.new.is_some()) {
            return Ok(());
        }
        let path = self.path.to_string_lossy();
        let path = path.strip_prefix("./").unwrap_or(&path);
        writeln!(self.wtr, "--- a/{path}")?;
        writeln!(self.wtr, "+++ b/{path}")?;
        // Hunks: runs of entries without a gap.
        let mut delta: i64 = 0;
        let mut i = 0;
        let entries = std::mem::take(&mut self.entries);
        while i < entries.len() {
            let mut j = i + 1;
            while j < entries.len() && !entries[j].gap {
                j += 1;
            }
            let hunk = &entries[i..j];
            // Only hunks with a change.
            if hunk.iter().any(|e| e.new.is_some()) {
                let old_len: u64 =
                    hunk.iter().map(|e| count_lines(&e.old)).sum();
                let new_len: u64 = hunk
                    .iter()
                    .map(|e| count_lines(e.new.as_deref().unwrap_or(&e.old)))
                    .sum();
                let old_start = hunk[0].line;
                let new_start = (old_start as i64 + delta) as u64;
                writeln!(
                    self.wtr,
                    "@@ -{},{} +{},{} @@",
                    old_start, old_len, new_start, new_len
                )?;
                for e in hunk {
                    match e.new {
                        None => write_lines(self.wtr, b' ', &e.old)?,
                        Some(ref new) => {
                            write_lines(self.wtr, b'-', &e.old)?;
                            write_lines(self.wtr, b'+', new)?;
                        }
                    }
                }
                delta += new_len as i64 - old_len as i64;
            }
            i = j;
        }
        Ok(())
    }
}

fn write_lines<W: io::Write>(
    wtr: &mut W,
    sign: u8,
    text: &[u8],
) -> io::Result<()> {
    let mut rest = text;
    while !rest.is_empty() {
        let (line, tail, nl) = match rest.iter().position(|&b| b == b'\n') {
            Some(p) => (&rest[..p], &rest[p + 1..], true),
            None => (rest, &rest[rest.len()..], false),
        };
        wtr.write_all(&[sign])?;
        wtr.write_all(line)?;
        wtr.write_all(b"\n")?;
        if !nl {
            wtr.write_all(b"\\ No newline at end of file\n")?;
        }
        rest = tail;
    }
    Ok(())
}
