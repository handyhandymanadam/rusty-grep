/*!
`--agent-help`: the agent-mode options, record types and codes as one JSON
object, so a program or script can learn them with a single call.
*/

use serde_json::{Value, json};

use crate::flags::{Flag, defs::FLAGS};

/// The options that belong to agent mode, in the order they are shown, and
/// whether `--agent-mode` turns them on.
const AGENT_OPTIONS: &[(&str, bool)] = &[
    ("agent-mode", false),
    ("no-agent-mode", false),
    ("agent-detail", false),
    ("accounting", true),
    ("diagnostics-format", true),
    ("stable-order", true),
    ("max-columns-window", true),
    ("limit", false),
    ("offset", false),
    ("time-limit", false),
    ("all-in-file", false),
    ("within", false),
    ("records", false),
    ("enclosing", false),
    ("patch", false),
    ("git-modified", false),
    ("git-tracked", false),
    ("agent-help", false),
];

fn find(name: &str) -> Option<&'static dyn Flag> {
    let plain = name.strip_prefix("no-").unwrap_or(name);
    FLAGS
        .iter()
        .copied()
        .find(|f| f.name_long() == name || f.name_negated() == Some(name))
        .or_else(|| FLAGS.iter().copied().find(|f| f.name_long() == plain))
}

pub(crate) fn generate() -> String {
    let options: Vec<Value> = AGENT_OPTIONS
        .iter()
        .filter_map(|&(name, on)| {
            let flag = find(name)?;
            let negated = flag.name_long() != name;
            Some(json!({
                "name": format!("--{name}"),
                "value": if negated { None } else { flag.doc_variable() },
                "choices": if flag.doc_choices().is_empty() || negated {
                    Value::Null
                } else {
                    json!(flag.doc_choices())
                },
                "summary": if negated {
                    format!("turns off --{}", flag.name_long())
                } else {
                    flag.doc_short().to_string()
                },
                "on_in_agent_mode": on,
            }))
        })
        .collect();
    let codes: Vec<Value> = crate::agent::CODES
        .iter()
        .map(|&(code, severity, meaning)| {
            json!({"code": code, "severity": severity, "meaning": meaning})
        })
        .collect();
    let help = json!({
        "schema": crate::agent::SCHEMA,
        "type": "agent-help",
        "tool": "hw",
        "version": crate::flags::doc::version::generate_digits(),
        "switch": {
            "environment": "RUSTY_AGENT=1",
            "flag": "--agent-mode",
            "off": "--no-agent-mode",
            "version_line": "agent mode: yes, schema 1",
            "note": "any other value of RUSTY_AGENT changes nothing; \
                     agent mode adds records on standard error and changes \
                     standard output only by --max-columns-window (a line \
                     longer than --max-columns is cut to a window around its \
                     first match, marked [... N bytes omitted]) and by the \
                     fixed order of the results",
            "detail": "RUSTY_AGENT_DETAIL or --agent-detail=brief|normal|full \
                       (default normal under agent mode; the JSON options \
                       without agent mode write full). normal leaves out \
                       rendered text, empty and zero fields, the echo of \
                       the patterns and paths, repeated suggestions and the \
                       match_occurrences count (it costs a pass over every \
                       matching line; full counts them); an absent field \
                       means empty, zero or null. Budget at normal and \
                       brief: about 12 KB (soft, then only errors) and 20 KB \
                       (hard, then only the summary) of records on standard \
                       error; the summary's records_omitted counts what was \
                       left out, RUSTY_AGENT_OUT or RUSTY_AGENT_FD keeps every record",
            "records_file": "RUSTY_AGENT_OUT=<path>: with agent mode on, every \
                             diagnostic and the summary record is also \
                             appended to this file, one JSON object per \
                             line, flushed per record; --records lines are \
                             not; an empty or unopenable path is one \
                             warning (G0012) and the search goes on; \
                             ignored when agent mode is off",
            "records_fd": "RUSTY_AGENT_FD=<n>: with agent mode on, the same \
                           records (every diagnostic and the summary) are \
                           also appended to the already-open descriptor n \
                           (3 or more, open for writing: a file or pipe the \
                           caller left open), one JSON object per line, \
                           flushed per record; with RUSTY_AGENT_OUT both get \
                           every record; a bad, closed or read-only \
                           descriptor is one warning (G0013) and the search \
                           goes on, exit code unchanged; --pre, \
                           decompression and git children do not inherit it; \
                           ignored when agent mode is off",
        },
        "agent_mode_turns_on": [
            "--accounting",
            "--diagnostics-format=json",
            "--stable-order",
            "--max-columns-window",
        ],
        "options": options,
        "streams": {
            "stderr": "JSON lines: diagnostic records, then one summary record",
            "stdout": "the usual output (in a fixed order under agent mode, \
                       and with long lines cut to a window around the first \
                       match: --max-columns-window, see switch.note); with --records one result \
                       record per match and one context record per context \
                       line; with --patch a unified diff",
        },
        "records": {
            "diagnostic": {
                "fields": [
                    "schema", "type", "code", "severity", "message", "files",
                    "symbols", "notes", "locations", "suggestions", "rendered",
                ],
                "normal": "schema type code severity message, then only what \
                           is not empty or already said: files (when the \
                           message does not name them), notes, locations, \
                           suggestions[{message, option?}] (up to 3; one \
                           already given in an earlier record is not \
                           repeated). full adds symbols, rendered and the \
                           empty fields.",
                "locations": "for a bad regex: [{pattern, text, start, end, column}]",
                "suggestions": "[{message, option}]; option is the argument \
                                that applies it, or null",
            },
            "summary": {
                "fields": [
                    "schema", "type", "patterns (full)", "paths (full)",
                    "match_lines", "match_occurrences (full)", "places",
                    "files_with_matches", "files_searched", "bytes_searched",
                    "elapsed_ms (not in stable mode)", "exit",
                    "diagnostics", "incomplete", "time_limit_s",
                    "stopped_after_ms", "accounting", "suggestions (full)",
                    "limit", "records_omitted",
                ],
                "match_lines": "lines that matched (one line with three \
                                matches counts once); with -l and -q, which \
                                stop at the first match of a file, it is \
                                left out",
                "match_occurrences": "individual matches (full only: counting \
                                      them costs a pass over every matching \
                                      line)",
                "places": "--all-in-file / --within: places found",
                "accounting": "{skipped_files, skipped[{reason, files, \
                               option?, examples[{path, files}]}], \
                               skipped_note?}; absent when nothing was \
                               skipped. skipped_files is always counted; the \
                               reasons are worked out for a small result \
                               (under 100 matches) or with --accounting, \
                               otherwise one group, reason unclassified. \
                               full also has files, errors{count, examples}, \
                               elapsed_ms.",
                "limit": "{limit?, offset?, unit, returned, total, \
                          total_files, next_offset?, exact}",
                "records_omitted": "{diagnostic: N, more}: the size budget \
                                    kept N records off standard error",
            },
            "result": {
                "fields": [
                    "schema", "type", "path", "line", "column", "byte_offset",
                    "end_byte_offset", "line_offset", "match", "line_text",
                    "end_line", "lossy", "enclosing",
                ],
            },
            "context": {
                "fields": [
                    "schema", "type", "path", "line", "line_offset",
                    "line_text", "lossy",
                ],
                "note": "--records with -A, -B or -C: one record per context \
                         line, in file order between the results",
            },
        },
        "skipped_reasons": crate::agent::SKIPPED_REASONS
            .iter()
            .map(|&(r, o, m)| json!({"reason": r, "option": o, "meaning": m}))
            .collect::<Vec<_>>(),
        "codes": codes,
        "exit_codes": {
            "0": "a match was found",
            "1": "no match (see the summary: incomplete, accounting)",
            "2": "an error happened",
        },
    });
    help.to_string()
}
