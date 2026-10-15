use aho_corasick::{
    AhoCorasick, AhoCorasickKind, Input, MatchKind as AcMatchKind,
};
use std::cell::Cell;

use grep_matcher::{LineTerminator, Match, Matcher, NoCaptures, NoError};

// Which engine is fastest depends on how many DISTINCT 4-byte prefixes the
// literals have, not on how many literals there are. The regex engine reduces
// the literals to their first four bytes and uses Teddy (SIMD, about 45-130 ms
// on a 239 MB file) while there are at most 64 distinct prefixes; one more and
// it falls to a much slower scan (240-315 ms). Measured: 80 words with 61
// distinct prefixes 114 ms, 85 words with 65 distinct prefixes 236 ms; 100
// short words with 61 distinct prefixes 129 ms, 130 with 80 distinct 315 ms.
// The hashed-bitap scan below runs at a steady 120-220 ms for 65-1,000
// literals, so it is used exactly when the regex engine would give up Teddy.
// Between 1,001 and 2,999 literals the regex engine's lazy DFA and the
// Aho-Corasick DFA are equally fast, so the regex engine is left alone. From
// 3,000 it falls to a slower Aho-Corasick NFA, and the DFA form used here is
// about 1.7x faster.
const MIN_LITERALS: usize = 65;
/// The regex engine keeps using Teddy up to this many distinct 4-byte prefixes.
const TEDDY_PREFIXES: usize = 64;
const MAX_BITAP: usize = 1_000;
const MIN_DFA: usize = 3_000;
const MAX_LITERALS: usize = 50_000;
/// Bitap table size (hashed byte pairs). 16 KB stays in the L1 cache.
const TAP_BITS: u32 = 14;
const TAP: usize = 1 << TAP_BITS;
/// A bitap table this full filters too little to pay off.
const MAX_TAP_FILL: usize = TAP * 6 / 10;

/// Largest number of positions each of the four scan streams covers in one
/// block. A stream records at most one candidate per position, so the
/// candidate buffers below can never overflow.
const MAX_SEG: usize = 512;

/// Two-tier filter (see `Dual`): bits per hash, and the most literals it is
/// used for (the table holds one entry per literal and tier-depth, and must
/// stay well under full).
const TAPD_BITS: u32 = 13;
const TAPD: usize = 1 << TAPD_BITS;
const MAX_DUAL: usize = 1000;
/// Candidate blocks of the two-tier scan are at most 4 * MAX_SEG2 positions;
/// a stream can record two candidates (one per tier) at a position.
const MAX_SEG2: usize = 256;
const STRIDE2: usize = 2 * MAX_SEG2 + 8;

#[inline(always)]
fn gram_hash(w: u32) -> usize {
    (w.wrapping_mul(0x9E37_79B1) >> (32 - TAP_BITS)) as usize
}

#[inline(always)]
fn gram_hash13(w: u32) -> usize {
    (w.wrapping_mul(0x9E37_79B1) >> (32 - TAPD_BITS)) as usize
}

#[inline(always)]
fn gram(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}


/// A matcher for a large set of plain (non-regex) literals.
///
/// The regex engine switches to a compact but slower Aho-Corasick automaton
/// once there are thousands of literals. For those sets this matcher uses the
/// faster, larger DFA form of Aho-Corasick instead. For sets of 100 to 1,000
/// literals it first runs a bitap scan over hashed byte pairs (the idea used
/// by ugrep) that rejects most positions with one table lookup, and confirms
/// the survivors with the Aho-Corasick DFA. Matches follow leftmost-first
/// semantics, the same as an alternation in a regex.
#[derive(Clone, Debug)]
pub struct LiteralSetMatcher {
    ac: AhoCorasick,
    /// For mid-sized sets: a shift-and bitap table over hashed byte pairs.
    /// Bit `j` of an entry says that pair hash can be the pair at offset `j`
    /// of some literal. `None` means scan with the Aho-Corasick DFA alone.
    bitap: Option<Bitap>,
    /// How densely matches occur in what has been searched so far; picks
    /// between the bitap scan and the plain Aho-Corasick DFA.
    density: Cell<Density>,
    line_term: u8,
}

/// The bitap scan beats the DFA only when matches are far apart: every call
/// pays a fixed cost to prime its four streams, and it rescans candidates
/// that a close-by match makes useless. Measured on a 239 MB file with 400
/// words: matches every ~1 KB tie, every ~240 bytes the DFA is 1.5x faster.
/// So each window of calls measures the average distance searched per call
/// and the next window uses the bitap scan only if that is long enough.
const DENSITY_WINDOW: u32 = 2048;
const MIN_BYTES_PER_CALL: u64 = 400;

#[derive(Clone, Copy, Debug)]
struct Density {
    calls: u32,
    bytes: u64,
    use_bitap: bool,
    /// Largest block (in `seg` units, see `find_bitap`) the scan may use. A
    /// call ends at its first match and drops the rest of its block, so with
    /// matches every few KB a block of a few hundred positions wastes far
    /// less than one of 2,048. Set from the average distance per call.
    seg_cap: usize,
}

#[derive(Clone, Debug)]
struct Bitap {
    tap: Vec<u8>,
    /// How many byte pairs (at most 8) every literal is checked on.
    pairs: usize,
    /// Open-addressing hash table from a literal's first four bytes to the
    /// literals that start with them. Each slot holds the four bytes in the
    /// low half and `group index + 1` in the high half (0 means empty).
    slots: Vec<u64>,
    /// For each group: the literals sharing a four-byte prefix, in the
    /// original order (earlier wins, as in a regex alternation).
    groups: Vec<Vec<Box<[u8]>>>,
    /// A 64 Kbit bitmap (8 KB) of hashed four-byte prefixes: a cheap first
    /// rejection before probing `slots`.
    seen: Vec<u64>,
    /// Set instead of `tap`/`pairs` for lists that mix 4-5 letter literals
    /// with longer ones.
    dual: Option<Dual>,
}

/// A two-tier filter. With one filter, the shortest literal sets how deep
/// every literal is checked: a single 4-letter word limits all the others to
/// their first 4 bytes, which lets through many false candidates. Here the
/// short literals (4-5 bytes) are checked on their first 4 bytes only, and
/// the long ones (6 or more) on `dl` groups, in separate bit ranges of the
/// same state word: bit 0 for the short tier, bits `1..=dl` for the long tier.
/// The scan is a shift-and: state = ((state << 1) | START) & entry, where START
/// sets the first bit of each tier.
#[derive(Clone, Debug)]
struct Dual {
    /// 16-bit entries, indexed by a hashed 4-byte group.
    tap: Vec<u16>,
    /// Depth of the short tier (always 1) and the long tier.
    ds: usize,
    dl: usize,
}

impl Dual {
    fn new<B: AsRef<[u8]>>(literals: &[B]) -> Option<Dual> {
        let long_min = literals
            .iter()
            .map(|l| l.as_ref().len())
            .filter(|&l| l >= 6)
            .min()?;
        let ds = 1;
        let dl = (long_min - 3).min(8);
        let mut tap = vec![0u16; TAPD];
        for lit in literals {
            let lit = lit.as_ref();
            if lit.len() <= 5 {
                tap[gram_hash13(gram(lit, 0))] |= 1;
            } else {
                for j in 0..dl {
                    tap[gram_hash13(gram(lit, j))] |= 1 << (ds + j);
                }
            }
        }
        if tap.iter().filter(|&&t| t != 0).count() > TAPD * 6 / 10 {
            return None;
        }
        Some(Dual { tap, ds, dl })
    }
}

impl Bitap {
    #[inline(always)]
    fn slot_of(key: u32, mask: usize) -> usize {
        (key.wrapping_mul(0x9E37_79B1) >> 15) as usize & mask
    }

    /// Returns the length of the first listed literal that occurs at the
    /// start of `s` (at least four bytes long), if any.
    #[inline(always)]
    fn literal_at(&self, s: &[u8]) -> Option<usize> {
        let key = u32::from_le_bytes([s[0], s[1], s[2], s[3]]);
        let bit = (key.wrapping_mul(0x85EB_CA6B) >> 16) as usize;
        if self.seen[bit >> 6] & (1 << (bit & 63)) == 0 {
            return None;
        }
        let mask = self.slots.len() - 1;
        let mut i = Bitap::slot_of(key, mask);
        loop {
            let slot = self.slots[i];
            if slot == 0 {
                return None;
            }
            if slot as u32 == key {
                let group = &self.groups[(slot >> 32) as usize - 1];
                for lit in group {
                    if s.starts_with(lit) {
                        return Some(lit.len());
                    }
                }
                return None;
            }
            i = (i + 1) & mask;
        }
    }
}

impl LiteralSetMatcher {
    /// The fewest literals for which this matcher is worth using. Below this,
    /// the regex engine's lazy DFA is as fast.
    pub const MIN_LITERALS: usize = MIN_LITERALS;

    /// True if the pattern contains no regex syntax, so it can be treated as
    /// a plain literal.
    pub fn is_plain(pattern: &str) -> bool {
        !pattern.chars().any(regex_syntax::is_meta_character)
    }

    /// Build a matcher from literals, or return `None` if this matcher does
    /// not apply: the number of literals is outside 3,000 to 50,000 (the DFA
    /// used here costs about 1.5 KB per literal, measured), a literal is empty, or a literal
    /// contains the line terminator or a NUL byte (so a match could span
    /// lines, or the regex path would need to reject it).
    pub fn new<B: AsRef<[u8]>>(
        literals: &[B],
        line_term: u8,
    ) -> Option<LiteralSetMatcher> {
        let n = literals.len();
        let use_bitap = (MIN_LITERALS..=MAX_BITAP).contains(&n)
            && distinct_prefixes(literals) > TEDDY_PREFIXES;
        let use_dfa = (MIN_DFA..=MAX_LITERALS).contains(&n);
        if !use_bitap && !use_dfa {
            return None;
        }
        for lit in literals {
            let lit = lit.as_ref();
            if lit.is_empty() || lit.contains(&line_term) || lit.contains(&0)
            {
                return None;
            }
        }
        let bitap = if use_bitap {
            // Needs at least 3 pairs of context to filter well.
            Some(Bitap::new(literals)?)
        } else {
            None
        };
        let ac = AhoCorasick::builder()
            .kind(Some(AhoCorasickKind::DFA))
            .match_kind(AcMatchKind::LeftmostFirst)
            .build(literals)
            .ok()?;
        Some(LiteralSetMatcher {
            ac,
            bitap,
            density: Cell::new(Density {
                calls: 0,
                bytes: 0,
                use_bitap: true,
                seg_cap: MAX_SEG,
            }),
            line_term,
        })
    }
}

/// The number of distinct leading-4-byte prefixes among the literals (whole
/// literal if shorter than 4 bytes).
fn distinct_prefixes<B: AsRef<[u8]>>(literals: &[B]) -> usize {
    let mut seen = std::collections::HashSet::new();
    for lit in literals {
        let lit = lit.as_ref();
        seen.insert(&lit[..lit.len().min(4)]);
    }
    seen.len()
}

impl Bitap {
    /// Returns `None` if the literals are too short or too alike for the
    /// table to filter well.
    fn new<B: AsRef<[u8]>>(literals: &[B]) -> Option<Bitap> {
        let min = literals.iter().map(|l| l.as_ref().len()).min()?;
        if min < 4 {
            return None;
        }
        let dual = if min == 4 && literals.len() <= MAX_DUAL {
            Dual::new(literals)
        } else {
            None
        };
        // Each step tests a hashed 4-byte group; step j is the group at
        // offset j of a literal. A literal of `min` bytes has `min - 3`.
        let pairs = (min - 3).min(8);
        let mut tap = vec![];
        if dual.is_none() {
            tap = vec![0u8; TAP];
            for lit in literals {
                let lit = lit.as_ref();
                for j in 0..pairs {
                    tap[gram_hash(gram(lit, j))] |= 1 << j;
                }
            }
            if tap.iter().filter(|&&t| t != 0).count() > MAX_TAP_FILL {
                return None;
            }
            // Store the complement: the scan is a shift-or, where a clear bit
            // means "this literal prefix can match here".
            for t in tap.iter_mut() {
                *t = !*t;
            }
        }
        // Group literals by their first four bytes.
        let size = (literals.len() * 4).next_power_of_two().max(1024);
        let mut slots = vec![0u64; size];
        let mut groups: Vec<Vec<Box<[u8]>>> = vec![];
        let mut seen = vec![0u64; 1 << 10];
        for lit in literals {
            let lit = lit.as_ref();
            let key = u32::from_le_bytes([lit[0], lit[1], lit[2], lit[3]]);
            let bit = (key.wrapping_mul(0x85EB_CA6B) >> 16) as usize;
            seen[bit >> 6] |= 1 << (bit & 63);
            let mut i = Bitap::slot_of(key, size - 1);
            loop {
                let slot = slots[i];
                if slot == 0 {
                    groups.push(vec![lit.into()]);
                    slots[i] = key as u64 | (groups.len() as u64) << 32;
                    break;
                }
                if slot as u32 == key {
                    groups[(slot >> 32) as usize - 1].push(lit.into());
                    break;
                }
                i = (i + 1) & (size - 1);
            }
        }
        Some(Bitap { tap, pairs, slots, groups, seen, dual })
    }
}

impl LiteralSetMatcher {
    #[inline]
    fn find_ac(&self, haystack: &[u8], at: usize) -> Option<Match> {
        let input = Input::new(haystack).span(at..haystack.len());
        self.ac.find(input).map(|m| Match::new(m.start(), m.end()))
    }

    /// Confirm a candidate window starting at `start` by looking up the
    /// literals that begin with the same four bytes.
    #[inline(always)]
    fn confirm(
        &self,
        bt: &Bitap,
        haystack: &[u8],
        start: usize,
    ) -> Option<Match> {
        let rest = haystack.get(start..)?;
        if rest.len() < 4 {
            return None;
        }
        bt.literal_at(rest).map(|len| Match::new(start, start + len))
    }

    /// Find the leftmost match at or after `at` using the bitap scan.
    ///
    /// A *window* ends at pair index `i` when the last `pairs` byte pairs
    /// (the pair at index `j` is `haystack[j], haystack[j+1]`) fit pairs
    /// `0..pairs` of some literal; it starts at `i + 1 - pairs`. Any match
    /// must begin at the start of such a window. The haystack is scanned in
    /// blocks by four interleaved streams, each over its own quarter of the
    /// block, so that the four dependency chains overlap in the processor.
    /// Candidates are recorded per stream and confirmed afterwards in
    /// ascending order, so the leftmost match is returned.
    fn find_bitap(
        &self,
        bt: &Bitap,
        haystack: &[u8],
        at: usize,
        seg_cap: usize,
    ) -> Option<Match> {
        use std::mem::MaybeUninit;

        let n = haystack.len();
        let pairs = bt.pairs;
        let goal = 1u32 << (pairs - 1);
        // Pair indices run over 0..=n-2, so windows can complete at indices
        // up to n-2. `lo` is the first completion index not yet examined.
        let last = n.saturating_sub(3);
        let mut lo = at + pairs - 1;
        let mut seg = 8usize;
        // Candidate window-completion indices, four streams of MAX_SEG each.
        // Offsets are relative to `lo`, so they fit in 16 bits (a block is at
        // most 4 * MAX_SEG positions). Keeping this buffer small matters: a
        // large stack frame costs a stack probe on every call.
        // The last stream also takes up to 3 leftover positions.
        let mut cands = [MaybeUninit::<u16>::uninit(); 4 * MAX_SEG + 4];
        while lo < last {
            let hi = (lo + 4 * seg).min(last);
            let quarter = (hi - lo) / 4;
            if quarter >= 8 {
                // Stream s covers completion indices lo+s*quarter ..
                // lo+(s+1)*quarter (the last one also takes the remainder).
                // Each starts `pairs - 1` pair indices early to prime its
                // state, without recording.
                let starts = [
                    lo,
                    lo + quarter,
                    lo + 2 * quarter,
                    lo + 3 * quarter,
                ];
                let hay = haystack.as_ptr();
                let tap = bt.tap.as_ptr();
                let counts: [usize; 4];
                // SAFETY: every pair index used is <= hi - 1 <= n - 2 so that
                // `+ 1` stays inside the haystack, and the first index used
                // is starts[s] - (pairs - 1) >= at. The hash is masked to the
                // table size.
                unsafe {
                    let mut i0 = starts[0] + 1 - pairs;
                    let mut i1 = starts[1] + 1 - pairs;
                    let mut i2 = starts[2] + 1 - pairs;
                    let mut i3 = starts[3] + 1 - pairs;
                    let (mut d0, mut d1, mut d2, mut d3) =
                        (!0u32, !0u32, !0u32, !0u32);
                    macro_rules! t {
                        ($i:expr) => {
                            *tap.add(gram_hash(
                                (hay.add($i) as *const u32).read_unaligned(),
                            )) as u32
                        };
                    }
                    for _ in 0..pairs - 1 {
                        d0 = (d0 << 1) | t!(i0);
                        d1 = (d1 << 1) | t!(i1);
                        d2 = (d2 << 1) | t!(i2);
                        d3 = (d3 << 1) | t!(i3);
                        i0 += 1;
                        i1 += 1;
                        i2 += 1;
                        i3 += 1;
                    }
                    let base = cands.as_mut_ptr() as *mut u16;
                    let (mut c0, mut c1, mut c2, mut c3) = (0, 0, 0, 0);
                    for _ in 0..quarter {
                        d0 = (d0 << 1) | t!(i0);
                        d1 = (d1 << 1) | t!(i1);
                        d2 = (d2 << 1) | t!(i2);
                        d3 = (d3 << 1) | t!(i3);
                        if d0 & d1 & d2 & d3 & goal != goal {
                            if d0 & goal == 0 {
                                *base.add(c0) = (i0 - lo) as u16;
                                c0 += 1;
                            }
                            if d1 & goal == 0 {
                                *base.add(MAX_SEG + c1) = (i1 - lo) as u16;
                                c1 += 1;
                            }
                            if d2 & goal == 0 {
                                *base.add(2 * MAX_SEG + c2) = (i2 - lo) as u16;
                                c2 += 1;
                            }
                            if d3 & goal == 0 {
                                *base.add(3 * MAX_SEG + c3) = (i3 - lo) as u16;
                                c3 += 1;
                            }
                        }
                        i0 += 1;
                        i1 += 1;
                        i2 += 1;
                        i3 += 1;
                    }
                    // The remainder belongs to the last stream.
                    while i3 < hi {
                        d3 = (d3 << 1) | t!(i3);
                        if d3 & goal == 0 {
                            *base.add(3 * MAX_SEG + c3) = (i3 - lo) as u16;
                            c3 += 1;
                        }
                        i3 += 1;
                    }
                    counts = [c0, c1, c2, c3];
                }
                for s in 0..4 {
                    for c in 0..counts[s] {
                        // SAFETY: the first counts[s] entries of stream s
                        // were written above.
                        let i = lo
                            + unsafe { cands[s * MAX_SEG + c].assume_init() }
                                as usize;
                        if let Some(m) = self.confirm(bt, haystack, i + 1 - pairs)
                        {
                            return Some(m);
                        }
                    }
                }
            } else {
                // Too little left for four streams: plain scan.
                let mut d = !0u32;
                for j in lo + 1 - pairs..hi {
                    let t = bt.tap[gram_hash(gram(haystack, j))];
                    d = (d << 1) | t as u32;
                    if j >= lo && d & goal == 0 {
                        if let Some(m) =
                            self.confirm(bt, haystack, j + 1 - pairs)
                        {
                            return Some(m);
                        }
                    }
                }
            }
            lo = hi;
            if seg < seg_cap {
                seg *= 2;
            }
        }
        None
    }
}

impl LiteralSetMatcher {
    /// The leftmost match at or after `at` using the two-tier filter.
    ///
    /// The two tiers complete a window at different offsets (a short literal
    /// starts at the completion index, a long one `dl - 1` earlier), so the
    /// candidates of a block are not in order of start. Each is confirmed
    /// unless it cannot beat the best match so far, and the scan goes on
    /// into the next block while a start earlier than the best could still
    /// show up there.
    fn find_bitap2(
        &self,
        bt: &Bitap,
        dual: &Dual,
        haystack: &[u8],
        at: usize,
        seg_cap: usize,
    ) -> Option<Match> {
        use std::mem::MaybeUninit;

        let n = haystack.len();
        let (ds, dl) = (dual.ds, dual.dl);
        let start_bits = 1u32 | (1u32 << ds);
        let goal_s = 1u32 << (ds - 1);
        let goal_l = 1u32 << (ds + dl - 1);
        let goals = goal_s | goal_l;
        // Group indices run over 0..=n-4.
        let last = n.saturating_sub(3);
        let mut lo = at + dl - 1;
        let mut seg = 8usize;
        let seg_cap = seg_cap.min(MAX_SEG2);
        let mut best: Option<Match> = None;
        // A long window cannot complete before `lo`, but a short one can:
        // the first few positions are checked on the short tier alone.
        for j in at..lo.min(last) {
            if dual.tap[gram_hash13(gram(haystack, j))] & 1 != 0 {
                if let Some(m) = self.confirm(bt, haystack, j) {
                    best = Some(m);
                    break;
                }
            }
        }
        // Entries are (offset from `lo`) << 1, with the low bit set for the
        // long tier.
        let mut cands = [MaybeUninit::<u16>::uninit(); 4 * STRIDE2 + 4];
        while lo < last {
            let hi = (lo + 4 * seg).min(last);
            let quarter = (hi - lo) / 4;
            if quarter >= 8 {
                let starts =
                    [lo, lo + quarter, lo + 2 * quarter, lo + 3 * quarter];
                let hay = haystack.as_ptr();
                let tap = dual.tap.as_ptr();
                let counts: [usize; 4];
                // SAFETY: every group index used is <= hi - 1 <= n - 4, so
                // the 4-byte load stays inside the haystack, and the first
                // index used is starts[s] + 1 - dl >= at. The hash is a
                // shift of a 32-bit product, so it is below the table size.
                // Each stream records at most two entries per position, so
                // at most 2 * (quarter + 3) < STRIDE2 entries.
                unsafe {
                    let mut i0 = starts[0] + 1 - dl;
                    let mut i1 = starts[1] + 1 - dl;
                    let mut i2 = starts[2] + 1 - dl;
                    let mut i3 = starts[3] + 1 - dl;
                    let (mut d0, mut d1, mut d2, mut d3) = (0u32, 0u32, 0u32, 0u32);
                    macro_rules! t {
                        ($i:expr) => {
                            *tap.add(gram_hash13(
                                (hay.add($i) as *const u32).read_unaligned(),
                            )) as u32
                        };
                    }
                    for _ in 0..dl - 1 {
                        d0 = ((d0 << 1) | start_bits) & t!(i0);
                        d1 = ((d1 << 1) | start_bits) & t!(i1);
                        d2 = ((d2 << 1) | start_bits) & t!(i2);
                        d3 = ((d3 << 1) | start_bits) & t!(i3);
                        i0 += 1;
                        i1 += 1;
                        i2 += 1;
                        i3 += 1;
                    }
                    let base = cands.as_mut_ptr() as *mut u16;
                    let (mut c0, mut c1, mut c2, mut c3) = (0, 0, 0, 0);
                    macro_rules! rec {
                        ($d:expr, $i:expr, $off:expr, $c:ident) => {
                            if $d & goal_l != 0 {
                                *base.add($off + $c) =
                                    ((($i - lo) << 1) | 1) as u16;
                                $c += 1;
                            }
                            if $d & goal_s != 0 {
                                *base.add($off + $c) = (($i - lo) << 1) as u16;
                                $c += 1;
                            }
                        };
                    }
                    for _ in 0..quarter {
                        d0 = ((d0 << 1) | start_bits) & t!(i0);
                        d1 = ((d1 << 1) | start_bits) & t!(i1);
                        d2 = ((d2 << 1) | start_bits) & t!(i2);
                        d3 = ((d3 << 1) | start_bits) & t!(i3);
                        if (d0 | d1 | d2 | d3) & goals != 0 {
                            rec!(d0, i0, 0, c0);
                            rec!(d1, i1, STRIDE2, c1);
                            rec!(d2, i2, 2 * STRIDE2, c2);
                            rec!(d3, i3, 3 * STRIDE2, c3);
                        }
                        i0 += 1;
                        i1 += 1;
                        i2 += 1;
                        i3 += 1;
                    }
                    // The remainder belongs to the last stream.
                    while i3 < hi {
                        d3 = ((d3 << 1) | start_bits) & t!(i3);
                        if d3 & goals != 0 {
                            rec!(d3, i3, 3 * STRIDE2, c3);
                        }
                        i3 += 1;
                    }
                    counts = [c0, c1, c2, c3];
                }
                'confirm: for s in 0..4 {
                    for c in 0..counts[s] {
                        // SAFETY: the first counts[s] entries of stream s
                        // were written above.
                        let v = unsafe {
                            cands[s * STRIDE2 + c].assume_init()
                        } as usize;
                        let i = lo + (v >> 1);
                        // Entries come in order of completion index, and no
                        // start is earlier than `i + 1 - dl`.
                        if let Some(b) = best {
                            if i + 1 - dl >= b.start() {
                                break 'confirm;
                            }
                        }
                        let start = i + 1 - if v & 1 == 1 { dl } else { ds };
                        if best.is_some_and(|b| start >= b.start()) {
                            continue;
                        }
                        if let Some(m) = self.confirm(bt, haystack, start) {
                            best = Some(m);
                        }
                    }
                }
            } else {
                // Too little left for four streams: plain scan.
                let mut d = 0u32;
                for j in lo + 1 - dl..hi {
                    let t = dual.tap[gram_hash13(gram(haystack, j))] as u32;
                    d = ((d << 1) | start_bits) & t;
                    if j < lo || d & goals == 0 {
                        continue;
                    }
                    if best.is_some_and(|b| j + 1 - dl >= b.start()) {
                        break;
                    }
                    if d & goal_l != 0 {
                        let start = j + 1 - dl;
                        if best.is_none_or(|b| start < b.start()) {
                            if let Some(m) = self.confirm(bt, haystack, start)
                            {
                                best = Some(m);
                            }
                        }
                    }
                    if d & goal_s != 0 {
                        let start = j + 1 - ds;
                        if best.is_none_or(|b| start < b.start()) {
                            if let Some(m) = self.confirm(bt, haystack, start)
                            {
                                best = Some(m);
                            }
                        }
                    }
                }
            }
            // A later block holds no start before `hi + 1 - dl`.
            if best.is_some_and(|b| b.start() + dl <= hi + 1) {
                return best;
            }
            lo = hi;
            if seg < seg_cap {
                seg *= 2;
            }
        }
        best
    }
}

impl Matcher for LiteralSetMatcher {
    type Captures = NoCaptures;
    type Error = NoError;

    #[inline]
    fn find_at(
        &self,
        haystack: &[u8],
        at: usize,
    ) -> Result<Option<Match>, NoError> {
        let found = match self.bitap {
            Some(ref bt) => {
                let mut d = self.density.get();
                let found = if d.use_bitap {
                    match bt.dual {
                        Some(ref dual) => self.find_bitap2(
                            bt, dual, haystack, at, d.seg_cap,
                        ),
                        None => self.find_bitap(bt, haystack, at, d.seg_cap),
                    }
                } else {
                    self.find_ac(haystack, at)
                };
                // Distance covered by this call, whichever scan ran.
                let end = found.map_or(haystack.len(), |m| m.start());
                d.bytes += end.saturating_sub(at) as u64;
                d.calls += 1;
                if d.calls == DENSITY_WINDOW {
                    d.use_bitap = d.bytes >= MIN_BYTES_PER_CALL * d.calls as u64;
                    // About one block per eighth of the average distance.
                    let avg = (d.bytes / d.calls as u64) as usize;
                    d.seg_cap = (avg / 32).clamp(16, MAX_SEG);
                    d.calls = 0;
                    d.bytes = 0;
                }
                self.density.set(d);
                found
            }
            None => self.find_ac(haystack, at),
        };
        Ok(found)
    }

    #[inline]
    fn new_captures(&self) -> Result<NoCaptures, NoError> {
        Ok(NoCaptures::new())
    }

    #[inline]
    fn line_terminator(&self) -> Option<LineTerminator> {
        Some(LineTerminator::byte(self.line_term))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn many(extra: &[&str]) -> Vec<Vec<u8>> {
        let mut v: Vec<Vec<u8>> =
            (0..MIN_DFA).map(|i| format!("w{i:05}x").into_bytes()).collect();
        v.extend(extra.iter().map(|s| s.as_bytes().to_vec()));
        v
    }

    // Words of 4+ letters over a small alphabet, so that many share prefixes
    // and hash collisions are likely.
    fn random_words(n: usize, seed: &mut u64) -> Vec<Vec<u8>> {
        let mut next = move |m: u64| {
            *seed ^= *seed << 13;
            *seed ^= *seed >> 7;
            *seed ^= *seed << 17;
            *seed % m
        };
        let mut out: Vec<Vec<u8>> = vec![];
        while out.len() < n {
            let len = 4 + next(5) as usize;
            let w: Vec<u8> = (0..len).map(|_| b'a' + next(6) as u8).collect();
            if !out.contains(&w) {
                out.push(w);
            }
        }
        out
    }

    // The obvious leftmost-first search: at each start, the first listed
    // literal that matches wins.
    fn naive(lits: &[Vec<u8>], hay: &[u8], at: usize) -> Option<Match> {
        for s in at..hay.len() {
            for l in lits {
                if hay[s..].starts_with(l) {
                    return Some(Match::new(s, s + l.len()));
                }
            }
        }
        None
    }

    #[test]
    fn bitap_agrees_with_naive_search() {
        let mut seed = 0x2545F4914F6CDD1Du64;
        for count in [100, 200, 1000] {
            let lits = random_words(count, &mut seed);
            let m = LiteralSetMatcher::new(&lits, b'\n').unwrap();
            assert!(m.bitap.is_some(), "{count} literals should use bitap");
            // Short haystacks, and long ones that cross many scan blocks.
            for len in [300usize, 300, 300, 5000, 5000, 40_000] {
                let mut hay = vec![];
                for _ in 0..len {
                    hay.push(b'a' + (seed % 7) as u8);
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                }
                let mut at = 0;
                loop {
                    let got = m.find_at(&hay, at).unwrap();
                    assert_eq!(got, naive(&lits, &hay, at), "at {at}");
                    match got {
                        Some(g) => at = g.end().max(at + 1),
                        None => break,
                    }
                }
            }
        }
    }

    #[test]
    fn two_tier_filter_agrees_with_naive_search() {
        // A few 4-5 letter words among longer ones (the case the two-tier
        // filter is for), over a small alphabet so that matches overlap and
        // sit close together.
        let mut seed = 0x9E3779B97F4A7C15u64;
        let mut next = |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        for (count, short_share, alphabet) in
            [(100, 8, 5u64), (300, 4, 6), (500, 20, 4), (200, 2, 26)]
        {
            let mut lits: Vec<Vec<u8>> = vec![];
            while lits.len() < count {
                let len = if next(100) < short_share {
                    4 + next(2)
                } else {
                    6 + next(12)
                } as usize;
                let w: Vec<u8> =
                    (0..len).map(|_| b'a' + next(alphabet) as u8).collect();
                if !lits.contains(&w) {
                    lits.push(w);
                }
            }
            let m = LiteralSetMatcher::new(&lits, b'\n').unwrap();
            let bt = m.bitap.as_ref().expect("bitap");
            assert!(bt.dual.is_some(), "{count} literals: two tiers");
            for len in [40usize, 300, 300, 5000, 5000, 40_000] {
                let hay: Vec<u8> = (0..len)
                    .map(|_| b'a' + next(alphabet + 1) as u8)
                    .collect();
                let mut at = 0;
                loop {
                    let got = m.find_at(&hay, at).unwrap();
                    assert_eq!(got, naive(&lits, &hay, at), "at {at}");
                    match got {
                        Some(g) => at = g.end().max(at + 1),
                        None => break,
                    }
                }
            }
        }
    }

    #[test]
    fn size_ranges() {
        let words = |n| random_words(n, &mut 88172645463325252u64);
        assert!(LiteralSetMatcher::new(&["a", "b"], b'\n').is_none());
        assert!(LiteralSetMatcher::new(&words(64), b'\n').is_none());
        assert!(LiteralSetMatcher::new(&words(100), b'\n').is_some());
        assert!(LiteralSetMatcher::new(&many(&[]), b'\n').is_some());
    }

    #[test]
    fn few_distinct_prefixes_stay_with_the_regex_engine() {
        // 300 words but only 30 distinct four-byte prefixes: the regex engine
        // handles these with Teddy, which is faster than the bitap scan.
        let lits: Vec<Vec<u8>> = (0..300)
            .map(|i| format!("p{:02}x_{i:03}", i % 30).into_bytes())
            .collect();
        assert!(LiteralSetMatcher::new(&lits, b'\n').is_none());
        // The same count with many distinct prefixes is accepted.
        let lits: Vec<Vec<u8>> = (0..300)
            .map(|i| format!("p{:03}x_{i:03}", i).into_bytes())
            .collect();
        assert!(LiteralSetMatcher::new(&lits, b'\n').is_some());
    }

    #[test]
    fn short_literals_skip_bitap() {
        // 3-letter words: too short for the bitap filter, and not enough
        // of them for the DFA, so the matcher declines.
        let lits: Vec<Vec<u8>> = (0..100u8)
            .map(|i| vec![b'a' + i % 26, b'a' + (i / 26) % 26, b'z'])
            .collect();
        assert!(LiteralSetMatcher::new(&lits, b'\n').is_none());
    }

    #[test]
    fn rejects_bad_literals() {
        assert!(LiteralSetMatcher::new(&many(&[""]), b'\n').is_none());
        assert!(LiteralSetMatcher::new(&many(&["a\nb"]), b'\n').is_none());
        assert!(LiteralSetMatcher::new(&many(&["a\0b"]), b'\n').is_none());
    }

    #[test]
    fn finds_leftmost_first() {
        // "ab" comes before "abc" in the list, so it wins at the same start.
        let m = LiteralSetMatcher::new(&many(&["ab", "abc", "zz"]), b'\n')
            .unwrap();
        assert_eq!(m.find_at(b"xxabcx", 0).unwrap(), Some(Match::new(2, 4)));
        assert_eq!(m.find_at(b"zz ab", 0).unwrap(), Some(Match::new(0, 2)));
        // The search starts at `at` and offsets stay absolute.
        assert_eq!(m.find_at(b"zz ab", 1).unwrap(), Some(Match::new(3, 5)));
        assert_eq!(m.find_at(b"nothing", 0).unwrap(), None);
    }
}
