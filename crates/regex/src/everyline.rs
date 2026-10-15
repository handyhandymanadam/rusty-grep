use regex_syntax::hir::{Hir, HirKind, Look};

/// Returns true if the given regex is guaranteed to match every line.
///
/// A line is any (possibly empty) run of bytes between line terminators. This
/// is conservative: `false` means "not known to match every line", not "does
/// not". It is intended for patterns like the empty pattern, `^`, `$` and
/// `.*`, where the search loop can skip running the regex on every line.
///
/// The HIR must come from a regex compiled with multi-line mode on (so `^`
/// and `$` are line anchors) and with a single-byte line terminator.
pub(crate) fn matches_every_line(hir: &Hir) -> bool {
    analyze(hir).is_some()
}

/// Where a regex is certain to find an empty match inside any line.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Empty {
    /// At every position of the line.
    Anywhere,
    /// At the start of the line, or at the end of it, but not both required.
    AtLineEdge,
}

fn analyze(hir: &Hir) -> Option<Empty> {
    match *hir.kind() {
        HirKind::Empty => Some(Empty::Anywhere),
        HirKind::Literal(ref lit) if lit.0.is_empty() => {
            Some(Empty::Anywhere)
        }
        HirKind::Literal(_) | HirKind::Class(_) => None,
        HirKind::Look(Look::StartLF) | HirKind::Look(Look::EndLF) => {
            Some(Empty::AtLineEdge)
        }
        HirKind::Look(_) => None,
        HirKind::Repetition(ref rep) => {
            if rep.min == 0 {
                Some(Empty::Anywhere)
            } else {
                analyze(&rep.sub)
            }
        }
        HirKind::Capture(ref cap) => analyze(&cap.sub),
        HirKind::Concat(ref subs) => {
            // Every part must match empty at the same position, so at most
            // one part may be pinned to a line edge. (`^$` is not universal:
            // it only matches empty lines.)
            let mut edges = 0;
            for sub in subs {
                match analyze(sub)? {
                    Empty::Anywhere => {}
                    Empty::AtLineEdge => edges += 1,
                }
            }
            match edges {
                0 => Some(Empty::Anywhere),
                1 => Some(Empty::AtLineEdge),
                _ => None,
            }
        }
        HirKind::Alternation(ref subs) => {
            let mut best = None;
            for sub in subs {
                match analyze(sub) {
                    Some(Empty::Anywhere) => return Some(Empty::Anywhere),
                    Some(Empty::AtLineEdge) => best = Some(Empty::AtLineEdge),
                    None => {}
                }
            }
            best
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex_syntax::ParserBuilder;

    fn every(pat: &str) -> bool {
        let hir = ParserBuilder::new()
            .multi_line(true)
            .build()
            .parse(pat)
            .unwrap();
        matches_every_line(&hir)
    }

    #[test]
    fn universal() {
        for pat in [
            "", "^", "$", ".*", "a*", "(a*)", "(?:a|)", "a|^", "^a*", "a*$",
            "x*y*", "(^)", "(a*)+", "^|$", "(?:a)?", "a{0,3}",
        ] {
            assert!(every(pat), "{pat:?} should match every line");
        }
    }

    #[test]
    fn not_universal() {
        for pat in [
            "a", "^$", ".", ".+", "^a", "a$", "\\b", "\\B", "a+", "^a*$",
            "(?:a|b)", "x{2}", "^^", "$$", "\\A", "\\z", "[a-z]",
        ] {
            assert!(!every(pat), "{pat:?} must not be treated as universal");
        }
    }
}
