/*!
`--enclosing`: the function, class or similar that contains a match.

This is a light heuristic, not a parser: the lines above the match are scanned
upwards for a line that looks like a definition and is indented less than the
code below it, the way a reader finds "which function am I in". Languages are
told apart by file extension. It is right for ordinary code and can be wrong
for unusual layouts (macros, definitions split over many lines, code in
strings); a parser (tree-sitter) would be exact but is a large dependency.
*/

use std::path::Path;

use regex::Regex;

/// Files bigger than this are not scanned.
const MAX_BYTES: u64 = 32 << 20;
/// How far up the scan goes.
const MAX_LINES_UP: usize = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    Rust,
    Python,
    Js,
    Go,
    CLike,
    Ruby,
    Shell,
    Generic,
}

fn family(path: &Path) -> Family {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "rs" => Family::Rust,
        "py" | "pyi" => Family::Python,
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => Family::Js,
        "go" => Family::Go,
        "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" | "java"
        | "cs" | "kt" | "kts" | "swift" | "php" | "scala" | "m" | "mm"
        | "dart" => Family::CLike,
        "rb" => Family::Ruby,
        "sh" | "bash" | "zsh" => Family::Shell,
        _ => Family::Generic,
    }
}

struct Patterns {
    /// (kind, regex with the name in group 1).
    defs: Vec<(&'static str, Regex)>,
}

fn re(s: &str) -> Regex {
    Regex::new(s).expect("a valid built-in pattern")
}

fn patterns(f: Family) -> Patterns {
    let defs = match f {
        Family::Rust => vec![
            (
                "fn",
                re(
                    r#"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:default\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?(?:extern\s+"[^"]*"\s+)?fn\s+(\w+)"#,
                ),
            ),
            (
                "impl",
                re(
                    r"^\s*(?:unsafe\s+)?impl\b(?:<[^>]*>)?\s*([^{]*?)\s*(?:\{|where\b|$)",
                ),
            ),
            (
                "trait",
                re(
                    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?trait\s+(\w+)",
                ),
            ),
            ("struct", re(r"^\s*(?:pub(?:\([^)]*\))?\s+)?struct\s+(\w+)")),
            ("enum", re(r"^\s*(?:pub(?:\([^)]*\))?\s+)?enum\s+(\w+)")),
            ("mod", re(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*\{")),
        ],
        Family::Python => vec![
            ("def", re(r"^\s*(?:async\s+)?def\s+(\w+)")),
            ("class", re(r"^\s*class\s+(\w+)")),
        ],
        Family::Js => vec![
            (
                "function",
                re(
                    r"^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\s*\*?\s*(\w+)",
                ),
            ),
            (
                "class",
                re(
                    r"^\s*(?:export\s+)?(?:default\s+)?(?:abstract\s+)?class\s+(\w+)",
                ),
            ),
            (
                "function",
                re(
                    r"^\s*(?:export\s+)?(?:const|let|var)\s+(\w+)\s*(?::[^=]+)?=\s*(?:async\s*)?(?:function\b|\([^)]*\)\s*(?::[^=]+)?=>|\w+\s*=>)",
                ),
            ),
            (
                "method",
                re(
                    r"^\s*(?:(?:public|private|protected|static|async|get|set)\s+)*(\w+)\s*\([^)]*\)\s*(?::\s*[^{]+)?\{\s*$",
                ),
            ),
        ],
        Family::Go => vec![
            ("func", re(r"^func\s+(?:\([^)]*\)\s*)?(\w+)")),
            ("type", re(r"^type\s+(\w+)\s+(?:struct|interface)\b")),
        ],
        Family::CLike => vec![
            (
                "class",
                re(
                    r"^\s*(?:(?:public|private|protected|static|final|abstract|sealed|partial|export|data|open)\s+)*(?:class|interface|struct|enum|namespace|object|trait)\s+(\w+)",
                ),
            ),
            (
                "function",
                re(
                    r"^\s*(?:(?:public|private|protected|static|final|abstract|virtual|override|inline|extern|const|async|synchronized|unsafe)\s+)*(?:fun|func|function)\s+(\w+)",
                ),
            ),
            // A C-style definition: a type and a name before "(", not a
            // statement. Lines that end in ";" are calls or declarations.
            (
                "function",
                re(
                    r"^\s*(?:[A-Za-z_][\w:<>,\*&\s\[\]]*?[\s\*&])?(~?[A-Za-z_][\w:]*)\s*\([^;]*$",
                ),
            ),
        ],
        Family::Ruby => vec![
            ("def", re(r"^\s*def\s+(?:self\.)?(\w+[?!=]?)")),
            ("class", re(r"^\s*class\s+(\w+)")),
            ("module", re(r"^\s*module\s+(\w+)")),
        ],
        Family::Shell => vec![
            ("function", re(r"^\s*(?:function\s+)?(\w[\w-]*)\s*\(\)\s*\{?")),
            ("function", re(r"^\s*function\s+(\w[\w-]*)")),
        ],
        Family::Generic => vec![(
            "def",
            re(
                r"^\s*(?:(?:pub|public|private|static|export|async)\s+)*(?:fn|func|function|def|class|struct|interface|impl|trait|module|sub|proc)\s+(\w+)",
            ),
        )],
    };
    Patterns { defs }
}

const C_KEYWORDS: &[&str] = &[
    "if", "else", "for", "while", "do", "switch", "case", "return", "sizeof",
    "catch", "try", "defined", "typeof", "alignof", "assert", "elif",
    "foreach", "using", "lock", "throw", "new", "delete", "goto", "when",
];

/// One enclosing symbol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Symbol {
    pub(crate) kind: &'static str,
    pub(crate) name: String,
    pub(crate) line: u64,
}

pub(crate) struct Index {
    lines: Vec<(usize, usize)>,
    text: Vec<u8>,
    family: Family,
    pats: Patterns,
}

/// `name:` on a line of its own (a goto label or an access specifier).
fn is_label(t: &[u8]) -> bool {
    t.len() > 1
        && t.ends_with(b":")
        && t[..t.len() - 1]
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || b == b'_')
}

fn indent_of(line: &[u8]) -> usize {
    let mut n = 0;
    for &b in line {
        match b {
            b' ' => n += 1,
            b'\t' => n += 8,
            _ => break,
        }
    }
    n
}

impl Index {
    /// Read the file; `None` when it cannot be read as a plain file or is
    /// too big.
    pub(crate) fn load(path: &Path) -> Option<Index> {
        let meta = std::fs::metadata(path).ok()?;
        if !meta.is_file() || meta.len() > MAX_BYTES {
            return None;
        }
        let text = std::fs::read(path).ok()?;
        Some(Index::from_bytes(text, family(path)))
    }

    fn from_bytes(text: Vec<u8>, family: Family) -> Index {
        let mut lines = Vec::new();
        let mut start = 0;
        for (i, &b) in text.iter().enumerate() {
            if b == b'\n' {
                lines.push((start, i));
                start = i + 1;
            }
        }
        if start < text.len() {
            lines.push((start, text.len()));
        }
        let pats = patterns(family);
        Index { lines, text, family, pats }
    }

    fn line(&self, n: usize) -> &[u8] {
        let (s, e) = self.lines[n];
        let l = &self.text[s..e];
        l.strip_suffix(b"\r").unwrap_or(l)
    }

    fn definition(&self, line: &[u8]) -> Option<(&'static str, String)> {
        let s = std::str::from_utf8(line).ok()?;
        for (kind, re) in &self.pats.defs {
            if let Some(c) = re.captures(s) {
                let name = c.get(1)?.as_str().trim().to_string();
                if name.is_empty() {
                    continue;
                }
                if self.family == Family::CLike
                    && *kind == "function"
                    && (C_KEYWORDS.contains(&name.as_str())
                        // FOREACH(...) { and other macro "loops"
                        || (name.len() > 1
                            && name.chars().all(|c| {
                                c.is_ascii_uppercase()
                                    || c.is_ascii_digit()
                                    || c == '_'
                            }))
                        || s.trim_start().starts_with('#')
                        || s.trim_start().starts_with("//")
                        || s.trim_start().starts_with('*')
                        || s.contains(" = ")
                        || (s.trim_end().ends_with(',') && indent_of(line) > 0)
                        || s.trim_end().ends_with("&&")
                        || s.trim_end().ends_with("||"))
                {
                    continue;
                }
                return Some((kind, name));
            }
        }
        None
    }

    /// The symbols around the 1-based line `line`, outermost first.
    pub(crate) fn enclosing(&self, line: u64) -> Vec<Symbol> {
        let mut out = Vec::new();
        if line == 0 || line as usize > self.lines.len() {
            return out;
        }
        let at = line as usize - 1;
        let here = self.line(at);
        let mut need = if here.iter().all(|b| b.is_ascii_whitespace()) {
            usize::MAX
        } else {
            indent_of(here) + 1
        };
        // The line itself, if it is a definition.
        if let Some((kind, name)) = self.definition(here) {
            if need != usize::MAX {
                need = indent_of(here);
            }
            out.push(Symbol { kind, name, line });
        } else if need != usize::MAX {
            need = indent_of(here) + 1;
        }
        if need == 0 {
            out.reverse();
            return out;
        }
        let first = at.saturating_sub(MAX_LINES_UP);
        for i in (first..at).rev() {
            let l = self.line(i);
            let t = l.trim_ascii();
            if t.is_empty() {
                continue;
            }
            let ind = indent_of(l);
            if ind >= need {
                continue;
            }
            // A lone brace (C-style): the signature is the line above. A
            // goto label or "public:" says nothing about what encloses.
            if t == b"{"
                || t == b"{}"
                || is_label(t)
                || t.starts_with(b"where ")
                || t == b"where"
            {
                continue;
            }
            if let Some((kind, name)) = self.definition(l) {
                out.push(Symbol { kind, name, line: i as u64 + 1 });
                need = ind;
            } else if !matches!(t[0], b'#' | b'/' | b'*' | b')' | b']')
                && !t.starts_with(b"//")
            {
                // A statement or block opener less indented than the code
                // below: what we are in is inside it, not above it.
                need = ind;
            }
            if need == 0 {
                break;
            }
        }
        out.reverse();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(src: &str, f: Family, line: u64) -> Vec<String> {
        Index::from_bytes(src.as_bytes().to_vec(), f)
            .enclosing(line)
            .into_iter()
            .map(|s| format!("{}:{}", s.kind, s.name))
            .collect()
    }

    #[test]
    fn rust_nested() {
        let src = "struct S;\nimpl S {\n    pub fn a(&self) {\n        let x = 1;\n        if x {\n            foo();\n        }\n    }\n    fn b() {\n        bar();\n    }\n}\n";
        assert_eq!(names(src, Family::Rust, 6), ["impl:S", "fn:a"]);
        assert_eq!(names(src, Family::Rust, 10), ["impl:S", "fn:b"]);
        assert_eq!(names(src, Family::Rust, 3), ["impl:S", "fn:a"]);
        assert_eq!(names(src, Family::Rust, 1), ["struct:S"]);
    }

    #[test]
    fn rust_where_clause() {
        let src = "pub fn each<F>(p: &Path, mut f: F) -> io::Result<()>\nwhere\n    F: FnMut(),\n{\n    let a = 1;\n}\n";
        assert_eq!(names(src, Family::Rust, 5), ["fn:each"]);
    }

    #[test]
    fn python_nested() {
        let src = "class A:\n    def m(self):\n        for i in x:\n            y()\n\ndef f():\n    z()\n";
        assert_eq!(names(src, Family::Python, 4), ["class:A", "def:m"]);
        assert_eq!(names(src, Family::Python, 7), ["def:f"]);
        // A statement at top level is not inside the function above it.
        let src = "def a():\n    pass\nif x:\n    foo\n";
        assert!(names(src, Family::Python, 4).is_empty());
    }

    #[test]
    fn c_functions() {
        let src = "#include <x.h>\n\nstatic int foo(int a,\n                int b)\n{\n        if (a) {\n                return bar(a);\n        }\n        return b;\n}\n\nint main(void)\n{\n        return foo(1, 2);\n}\n";
        assert_eq!(names(src, Family::CLike, 7), ["function:foo"]);
        assert_eq!(names(src, Family::CLike, 14), ["function:main"]);
        assert!(names(src, Family::CLike, 2).is_empty());
        // A goto label and a macro loop do not hide the function.
        let src = "int f(int *a)\n{\n        FOREACH(x, a) {\n                g(x);\n        }\n\ndone:\n        return h();\n}\n";
        assert_eq!(names(src, Family::CLike, 4), ["function:f"]);
        assert_eq!(names(src, Family::CLike, 8), ["function:f"]);
    }

    #[test]
    fn js_and_go() {
        let src = "class K {\n  run(a) {\n    go();\n  }\n}\nconst f = (x) => {\n  return x;\n};\n";
        assert_eq!(names(src, Family::Js, 3), ["class:K", "method:run"]);
        assert_eq!(names(src, Family::Js, 7), ["function:f"]);
        let src = "package p\n\nfunc (s *S) Run() {\n\tdo()\n}\n";
        assert_eq!(names(src, Family::Go, 4), ["func:Run"]);
    }
}
