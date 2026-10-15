use bstr::ByteSlice;
use grep_matcher::{LineTerminator, Match, Matcher, NoCaptures, NoError};

use crate::matcher::RegexMatcher;

/// A matcher that only reports a match on a line when that line contains a
/// match for every one of its inner matchers.
///
/// This is a conjunction ("AND") of patterns evaluated one line at a time. The
/// first inner matcher drives the scan over the haystack and so determines
/// the speed of the search. Only when it finds a candidate are the remaining
/// matchers consulted, and only on that single line. The reported match is
/// always a match of the first inner matcher.
#[derive(Clone, Debug)]
pub struct AllOfMatcher {
    first: RegexMatcher,
    rest: Vec<RegexMatcher>,
    line_term: u8,
}

impl AllOfMatcher {
    /// Build a matcher from the given inner matchers.
    ///
    /// The first matcher scans the haystack, so it should be the one that
    /// matches least often. Every matcher must have been built with the same
    /// single-byte line terminator, and none may match across lines.
    ///
    /// Returns `None` if `matchers` is empty or if the first matcher has no
    /// line terminator configured (for example, in multi-line mode).
    pub fn new(mut matchers: Vec<RegexMatcher>) -> Option<AllOfMatcher> {
        if matchers.is_empty() {
            return None;
        }
        let first = matchers.remove(0);
        let line_term = first.line_terminator()?.as_byte();
        Some(AllOfMatcher { first, rest: matchers, line_term })
    }
}

impl Matcher for AllOfMatcher {
    type Captures = NoCaptures;
    type Error = NoError;

    fn find_at(
        &self,
        haystack: &[u8],
        mut at: usize,
    ) -> Result<Option<Match>, NoError> {
        while at <= haystack.len() {
            let m = match self.first.find_at(haystack, at)? {
                None => return Ok(None),
                Some(m) => m,
            };
            if self.rest.is_empty() {
                return Ok(Some(m));
            }
            let start = haystack[..m.start()]
                .rfind_byte(self.line_term)
                .map_or(0, |i| i + 1);
            let end = haystack[m.end()..]
                .find_byte(self.line_term)
                .map_or(haystack.len(), |i| m.end() + i);
            let line = &haystack[start..end];
            let mut all = true;
            for r in &self.rest {
                if !r.is_match(line)? {
                    all = false;
                    break;
                }
            }
            if all {
                return Ok(Some(m));
            }
            // The line failed. Resume at the start of the next one.
            at = end + 1;
        }
        Ok(None)
    }

    fn new_captures(&self) -> Result<NoCaptures, NoError> {
        Ok(NoCaptures::new())
    }

    fn line_terminator(&self) -> Option<LineTerminator> {
        Some(LineTerminator::byte(self.line_term))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RegexMatcherBuilder;

    fn build(pats: &[&str]) -> AllOfMatcher {
        let mut b = RegexMatcherBuilder::new();
        b.multi_line(true).line_terminator(Some(b'\n'));
        let ms = pats.iter().map(|p| b.build(p).unwrap()).collect();
        AllOfMatcher::new(ms).unwrap()
    }

    fn lines(m: &AllOfMatcher, hay: &str) -> Vec<String> {
        // Emulate the searcher: report every line the matcher accepts.
        let hay = hay.as_bytes();
        let mut got = vec![];
        let mut at = 0;
        while let Some(mat) = m.find_at(hay, at).unwrap() {
            let s = hay[..mat.start()]
                .rfind_byte(b'\n')
                .map_or(0, |i| i + 1);
            let e = hay[mat.end()..]
                .find_byte(b'\n')
                .map_or(hay.len(), |i| mat.end() + i);
            got.push(String::from_utf8_lossy(&hay[s..e]).into_owned());
            at = e + 1;
        }
        got
    }

    #[test]
    fn both_terms_on_same_line() {
        let m = build(&["mutex", "unlock"]);
        let hay = "mutex only\nunlock only\nmutex_unlock both\nunlock then mutex\nnone\n";
        assert_eq!(
            lines(&m, hay),
            vec!["mutex_unlock both", "unlock then mutex"]
        );
    }

    #[test]
    fn terms_may_overlap() {
        // "ab" and "b" overlap on the same bytes; both must still count.
        let m = build(&["ab", "b"]);
        assert_eq!(lines(&m, "ab\nb\na\n"), vec!["ab"]);
    }

    #[test]
    fn single_term_behaves_like_plain_matcher() {
        let m = build(&["foo"]);
        assert_eq!(lines(&m, "foo\nbar\nxfoo\n"), vec!["foo", "xfoo"]);
    }

    #[test]
    fn no_final_newline_and_empty_haystack() {
        let m = build(&["a", "b"]);
        assert_eq!(lines(&m, "xa b"), vec!["xa b"]);
        assert!(lines(&m, "").is_empty());
    }

    #[test]
    fn terms_on_different_lines_do_not_match() {
        let m = build(&["a", "b"]);
        assert!(lines(&m, "a\nb\n").is_empty());
    }

    #[test]
    fn anchors_apply_per_term() {
        let m = build(&["^start", "end$"]);
        assert_eq!(
            lines(&m, "start middle end\nstart only\nonly end\n"),
            vec!["start middle end"]
        );
    }
}
