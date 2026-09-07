//! A faithful port of the subset of `CPython` 3.14's `Lib/difflib.py` that
//! style50's character diff consumes. Byte-parity of `-o character` is
//! defined by this module: style50 calls `difflib.ndiff(old, new)` with the
//! full normalized source and its styled content as *character sequences*,
//! and walks the delta as `(d[0], d[2])` (tag char, value char) units.
//!
//! Ported exactly (reference line numbers cite `Lib/difflib.py` as shipped
//! with Python 3.14, `/usr/lib/python3.14/difflib.py`):
//!
//! - [`SequenceMatcher`]: `__init__`/`chain_b` with the `autojunk=True`
//!   heuristic (elements occurring more than `n/100 + 1` times in a
//!   sequence of >= 200 elements become `bpopular` and stop anchoring
//!   matches; difflib.py:293-301), `find_longest_match` (the `j2len` walk
//!   with junk/popular elements extending but never anchoring a match and
//!   `CPython`'s first-wins tie-breaking; difflib.py:305-437),
//!   `get_matching_blocks` (the LIFO queue, tuple sort, adjacent merge and
//!   the `(len(a), len(b), 0)` sentinel; difflib.py:440-519),
//!   `get_opcodes` (difflib.py:522-562) and the three ratio bounds
//!   (difflib.py:39-42, 597-669).
//! - [`Differ`]: `compare` (difflib.py:875-905), `_dump`,
//!   `_plain_replace`, `_fancy_replace` (the 3.14 windowed scan with
//!   `cutoff = 0.74999` and `WINDOW = 10`; difflib.py:917-1007),
//!   `_fancy_helper`, `_qformat` with `_keep_original_ws`
//!   (difflib.py:715-721, 997-1030), and `IS_CHARACTER_JUNK`
//!   (difflib.py:1062).
//! - [`ndiff_lines`]: the module-level `ndiff(a, b)` entry point
//!   (difflib.py:1310 = `Differ(None, IS_CHARACTER_JUNK).compare(a, b)`).
//!
//! Element type is `char`: `ndiff(old, new)` iterates the two strings, so
//! every delta line is the 3-character string `"<tag> <value>"` (the
//! `_qformat` quad is unreachable for one-character elements — a synch
//! pair needs `ratio() > 0.74999`, i.e. identical characters) but is kept
//! for faithfulness.

use std::collections::HashMap;
use std::collections::HashSet;

/// Runs the ported [`Differ`] over the two texts as character sequences
/// (`ndiff(old, new)`, difflib.py:1310) and returns the delta as
/// `(tag, value)` pairs — the `(d[0], d[2])` units style50's `_char_diff`
/// walk consumes. Tags are `' '` (common), `'-'` (only in `a`), `'+'`
/// (only in `b`) and `'?'` (guide line; unreachable in character mode,
/// see the module docs).
pub(crate) fn ndiff_lines(a: &str, b: &str) -> Vec<(char, char)> {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let differ = Differ::new(None, Some(IS_CHARACTER_JUNK));
    differ
        .compare(&a_chars, &b_chars)
        .into_iter()
        .map(|delta| {
            // Delta lines are `"<tag> <value>"`: d[0] is the tag, d[2] the
            // diffed character.
            let mut it = delta.chars();
            let tag = it.next().unwrap_or(' ');
            let value = it.nth(1).unwrap_or(' ');
            (tag, value)
        })
        .collect()
}

/// `IS_CHARACTER_JUNK` (difflib.py:1062): iff `ch` is a space or tab.
// `CPython` name preserved for port fidelity.
#[allow(non_snake_case)]
fn IS_CHARACTER_JUNK(ch: char) -> bool {
    ch == ' ' || ch == '\t'
}

/// `_calculate_ratio` (difflib.py:39-42).
// The usize->f64 casts are CPython's arithmetic, ported exactly.
#[allow(clippy::cast_precision_loss)]
fn calculate_ratio(matches: usize, length: usize) -> f64 {
    if length != 0 {
        2.0 * matches as f64 / length as f64
    } else {
        1.0
    }
}

/// Python `str.isspace()`: Rust's `char::is_whitespace` plus the C0 file
/// separators `\x1c`-`\x1f` Python also classifies as space.
fn py_isspace(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// Python `str.rstrip()` (used by `_qformat`).
fn py_rstrip(s: &str) -> &str {
    s.trim_end_matches(py_isspace)
}

/// `_keep_original_ws` (difflib.py:715-721): replace whitespace tags with
/// the original whitespace characters in `s`.
fn keep_original_ws(s: &str, tag_s: &str) -> String {
    s.chars()
        .zip(tag_s.chars())
        .map(|(c, tag_c)| {
            if tag_c == ' ' && py_isspace(c) {
                c
            } else {
                tag_c
            }
        })
        .collect()
}

/// The opcode tags of `get_opcodes` (difflib.py:522-562).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tag {
    Replace,
    Delete,
    Insert,
    Equal,
}

/// One `(tag, i1, i2, j1, j2)` tuple of `get_opcodes`.
#[derive(Clone, Copy, Debug)]
struct Opcode {
    tag: Tag,
    i1: usize,
    i2: usize,
    j1: usize,
    j2: usize,
}

/// The optional junk predicate (`isjunk`/`linejunk`/`charjunk`).
type IsJunk = Option<fn(char) -> bool>;

/// `CPython`'s `SequenceMatcher` (difflib.py:44-669) over `char` sequences.
/// Both style50 call sites keep the `autojunk=True` default, so the flag
/// is fixed to `true` here.
struct SequenceMatcher {
    isjunk: IsJunk,
    a: Vec<char>,
    b: Vec<char>,
    autojunk: bool,
    /// For x in b, indices (ascending) at which x appears; junk and
    /// popular elements do not appear.
    b2j: HashMap<char, Vec<usize>>,
    /// The items in b for which `isjunk` is true.
    bjunk: HashSet<char>,
    /// Non-junk items treated as junk by the autojunk heuristic. Unused
    /// by the ported subset (as in `CPython`, where only inspecting code
    /// reads it) but kept as part of the faithful state.
    #[allow(dead_code)]
    bpopular: HashSet<char>,
    /// Materialized on demand by [`SequenceMatcher::quick_ratio`].
    fullbcount: Option<HashMap<char, usize>>,
    matching_blocks: Option<Vec<(usize, usize, usize)>>,
    opcodes: Option<Vec<Opcode>>,
}

impl SequenceMatcher {
    /// `__init__(isjunk=None, a='', b='', autojunk=True)`
    /// (difflib.py:120-181).
    fn new(isjunk: IsJunk, a: &[char], b: &[char]) -> Self {
        let mut sm = Self {
            isjunk,
            a: Vec::new(),
            b: Vec::new(),
            autojunk: true,
            b2j: HashMap::new(),
            bjunk: HashSet::new(),
            bpopular: HashSet::new(),
            fullbcount: None,
            matching_blocks: None,
            opcodes: None,
        };
        sm.set_seqs(a, b);
        sm
    }

    /// `set_seqs` (difflib.py:184-190).
    fn set_seqs(&mut self, a: &[char], b: &[char]) {
        self.set_seq1(a);
        self.set_seq2(b);
    }

    /// `set_seq1` (difflib.py:193-220): caches keyed on b stay valid.
    fn set_seq1(&mut self, a: &[char]) {
        if self.a.as_slice() == a {
            return;
        }
        self.a = a.to_vec();
        self.matching_blocks = None;
        self.opcodes = None;
    }

    /// `set_seq2` (difflib.py:222-253): b is the expensive side.
    fn set_seq2(&mut self, b: &[char]) {
        if self.b.as_slice() == b {
            return;
        }
        self.b = b.to_vec();
        self.matching_blocks = None;
        self.opcodes = None;
        self.fullbcount = None;
        self.chain_b();
    }

    /// `__chain_b` (difflib.py:273-302): b2j for every element, then purge
    /// junk, then purge popular elements (autojunk heuristic).
    fn chain_b(&mut self) {
        self.b2j.clear();
        for (i, elt) in self.b.iter().enumerate() {
            self.b2j.entry(*elt).or_default().push(i);
        }

        self.bjunk.clear();
        if let Some(isjunk) = self.isjunk {
            let junk: Vec<char> = self.b2j.keys().copied().filter(|&e| isjunk(e)).collect();
            self.bjunk.extend(junk.iter().copied());
            for elt in junk {
                self.b2j.remove(&elt);
            }
        }

        self.bpopular.clear();
        let n = self.b.len();
        if self.autojunk && n >= 200 {
            let ntest = n / 100 + 1;
            let popular: Vec<char> = self
                .b2j
                .iter()
                .filter(|(_, idxs)| idxs.len() > ntest)
                .map(|(elt, _)| *elt)
                .collect();
            self.bpopular.extend(popular.iter().copied());
            for elt in popular {
                self.b2j.remove(&elt);
            }
        }
    }

    /// `find_longest_match(alo, ahi, blo, bhi)` (difflib.py:305-437).
    /// Returns `(i, j, k)` such that `a[i:i+k] == b[j:j+k]`, maximal and
    /// leftmost-earliest per `CPython`'s tie-breaking (`k > bestsize` is
    /// strict, so the first candidate in a-position, then in j-order,
    /// wins).
    fn find_longest_match(
        &self,
        alo: usize,
        ahi: usize,
        blo: usize,
        bhi: usize,
    ) -> (usize, usize, usize) {
        let (a, b) = (&self.a, &self.b);
        let isbjunk = |c: char| self.bjunk.contains(&c);
        let (mut best_ai, mut bestj, mut bestsize) = (alo, blo, 0usize);
        // During an iteration of the loop, j2len[j] = length of longest
        // junk-free match ending with a[i-1] and b[j].
        let mut j2len: HashMap<usize, usize> = HashMap::new();
        // The index `i` IS the algorithm (the j2len walk over a[i-1]);
        // renaming to iterators would obscure the CPython parity.
        #[allow(clippy::needless_range_loop)]
        for i in alo..ahi {
            // b2j has no junk/popular keys, so the inner loop is skipped
            // when a[i] is junk or popular.
            let mut newj2len: HashMap<usize, usize> = HashMap::new();
            if let Some(indices) = self.b2j.get(&a[i]) {
                for &j in indices {
                    if j < blo {
                        continue;
                    }
                    if j >= bhi {
                        break;
                    }
                    // `CPython`'s `j2lenget(j-1, 0)`: j == 0 has no j-1 key.
                    let k = if j == 0 {
                        1
                    } else {
                        j2len.get(&(j - 1)).copied().unwrap_or(0) + 1
                    };
                    newj2len.insert(j, k);
                    if k > bestsize {
                        best_ai = i + 1 - k;
                        bestj = j + 1 - k;
                        bestsize = k;
                    }
                }
            }
            j2len = newj2len;
        }

        // Extend the best by non-junk elements on each end (popular
        // elements are not in b2j but match freely here).
        while best_ai > alo
            && bestj > blo
            && !isbjunk(b[bestj - 1])
            && a[best_ai - 1] == b[bestj - 1]
        {
            best_ai -= 1;
            bestj -= 1;
            bestsize += 1;
        }
        while best_ai + bestsize < ahi
            && bestj + bestsize < bhi
            && !isbjunk(b[bestj + bestsize])
            && a[best_ai + bestsize] == b[bestj + bestsize]
        {
            bestsize += 1;
        }

        // Suck up the matching junk on each side of the best match.
        while best_ai > alo
            && bestj > blo
            && isbjunk(b[bestj - 1])
            && a[best_ai - 1] == b[bestj - 1]
        {
            best_ai -= 1;
            bestj -= 1;
            bestsize += 1;
        }
        while best_ai + bestsize < ahi
            && bestj + bestsize < bhi
            && isbjunk(b[bestj + bestsize])
            && a[best_ai + bestsize] == b[bestj + bestsize]
        {
            bestsize += 1;
        }

        (best_ai, bestj, bestsize)
    }

    /// `get_matching_blocks` (difflib.py:440-519): queue-based recursion
    /// (LIFO `pop`), tuple sort, adjacent merge, `(len(a), len(b), 0)`
    /// sentinel.
    fn get_matching_blocks(&mut self) -> &[(usize, usize, usize)] {
        if self.matching_blocks.is_none() {
            let (la, lb) = (self.a.len(), self.b.len());
            let mut queue = vec![(0, la, 0, lb)];
            let mut blocks: Vec<(usize, usize, usize)> = Vec::new();
            while let Some((alo, ahi, blo, bhi)) = queue.pop() {
                let (i, j, k) = self.find_longest_match(alo, ahi, blo, bhi);
                if k != 0 {
                    blocks.push((i, j, k));
                    if alo < i && blo < j {
                        queue.push((alo, i, blo, j));
                    }
                    if i + k < ahi && j + k < bhi {
                        queue.push((i + k, ahi, j + k, bhi));
                    }
                }
            }
            blocks.sort_unstable();

            // Collapse adjacent equal blocks.
            let mut non_adjacent: Vec<(usize, usize, usize)> = Vec::new();
            let (mut i1, mut j1, mut k1) = (0usize, 0usize, 0usize);
            for &(i2, j2, k2) in &blocks {
                if i1 + k1 == i2 && j1 + k1 == j2 {
                    k1 += k2;
                } else {
                    if k1 != 0 {
                        non_adjacent.push((i1, j1, k1));
                    }
                    i1 = i2;
                    j1 = j2;
                    k1 = k2;
                }
            }
            if k1 != 0 {
                non_adjacent.push((i1, j1, k1));
            }
            non_adjacent.push((la, lb, 0));
            self.matching_blocks = Some(non_adjacent);
        }
        self.matching_blocks.as_deref().expect("computed above")
    }

    /// `get_opcodes` (difflib.py:522-562).
    fn get_opcodes(&mut self) -> &[Opcode] {
        if self.opcodes.is_none() {
            let blocks = self.get_matching_blocks().to_vec();
            let mut answer: Vec<Opcode> = Vec::new();
            let (mut i, mut j) = (0usize, 0usize);
            for &(ai, bj, size) in &blocks {
                let tag = if i < ai && j < bj {
                    Some(Tag::Replace)
                } else if i < ai {
                    Some(Tag::Delete)
                } else if j < bj {
                    Some(Tag::Insert)
                } else {
                    None // `CPython`'s empty `tag = ''`
                };
                if let Some(tag) = tag {
                    answer.push(Opcode {
                        tag,
                        i1: i,
                        i2: ai,
                        j1: j,
                        j2: bj,
                    });
                }
                i = ai + size;
                j = bj + size;
                if size != 0 {
                    answer.push(Opcode {
                        tag: Tag::Equal,
                        i1: ai,
                        i2: i,
                        j1: bj,
                        j2: j,
                    });
                }
            }
            self.opcodes = Some(answer);
        }
        self.opcodes.as_deref().expect("computed above")
    }

    /// `ratio` (difflib.py:597-617).
    fn ratio(&mut self) -> f64 {
        let matches = self
            .get_matching_blocks()
            .iter()
            .map(|&(_, _, k)| k)
            .sum::<usize>();
        calculate_ratio(matches, self.a.len() + self.b.len())
    }

    /// `quick_ratio` (difflib.py:620-649): multiset upper bound.
    fn quick_ratio(&mut self) -> f64 {
        if self.fullbcount.is_none() {
            let mut fullbcount: HashMap<char, usize> = HashMap::new();
            for elt in &self.b {
                *fullbcount.entry(*elt).or_insert(0) += 1;
            }
            self.fullbcount = Some(fullbcount);
        }
        let fullbcount = self.fullbcount.as_ref().expect("set above");
        let mut avail: HashMap<char, isize> = HashMap::new();
        let mut matches: usize = 0;
        for elt in &self.a {
            let numb = match avail.get(elt) {
                Some(numb) => *numb,
                None => fullbcount.get(elt).copied().unwrap_or(0).cast_signed(),
            };
            avail.insert(*elt, numb - 1);
            if numb > 0 {
                matches += 1;
            }
        }
        calculate_ratio(matches, self.a.len() + self.b.len())
    }

    /// `real_quick_ratio` (difflib.py:651-669): length upper bound.
    fn real_quick_ratio(&self) -> f64 {
        let (la, lb) = (self.a.len(), self.b.len());
        calculate_ratio(la.min(lb), la + lb)
    }
}

/// `CPython`'s `Differ` (difflib.py:745-1030).
struct Differ {
    linejunk: IsJunk,
    charjunk: IsJunk,
}

impl Differ {
    /// `__init__` (difflib.py:857-874).
    fn new(linejunk: IsJunk, charjunk: IsJunk) -> Self {
        Self { linejunk, charjunk }
    }

    /// `compare` (difflib.py:875-905).
    fn compare(&self, a: &[char], b: &[char]) -> Vec<String> {
        let mut out = Vec::new();
        let mut cruncher = SequenceMatcher::new(self.linejunk, a, b);
        for opcode in cruncher.get_opcodes().to_vec() {
            match opcode.tag {
                Tag::Replace => {
                    out.extend(
                        self.fancy_replace(a, opcode.i1, opcode.i2, b, opcode.j1, opcode.j2),
                    );
                }
                Tag::Delete => out.extend(dump('-', a, opcode.i1, opcode.i2)),
                Tag::Insert => out.extend(dump('+', b, opcode.j1, opcode.j2)),
                Tag::Equal => out.extend(dump(' ', a, opcode.i1, opcode.i2)),
            }
        }
        out
    }

    /// `_fancy_replace` (difflib.py:917-1007): the 3.14 windowed scan —
    /// for each j, search the corresponding i's within `WINDOW` for the
    /// highest ratio greater than `cutoff`, synch on the best pair, and
    /// pump out straight replaces around it. The pre-3.14 all-pairs
    /// recursion was removed upstream (gh-119105).
    #[allow(clippy::too_many_lines)]
    fn fancy_replace(
        &self,
        a: &[char],
        alo: usize,
        ahi: usize,
        b: &[char],
        blo: usize,
        bhi: usize,
    ) -> Vec<String> {
        // "Don't synch up unless the lines have a similarity score above
        // cutoff."
        const CUTOFF: f64 = 0.74999;
        const WINDOW: usize = 10;
        let mut out = Vec::new();
        let mut cruncher = SequenceMatcher::new(self.charjunk, &[], &[]);
        // Smallest indices not yet resolved.
        let mut dump_i = alo;
        let mut dump_j = blo;
        for j in blo..bhi {
            cruncher.set_seq2(&[b[j]]);
            let aequiv = alo + (j - blo);
            let start = usize::max(aequiv.saturating_sub(WINDOW), dump_i);
            let end = usize::min(aequiv + WINDOW + 1, ahi);
            if start >= end {
                // Empty range: likely exit if `a` is shorter than `b`.
                break;
            }
            let mut best_ratio = CUTOFF;
            let mut best_i: Option<usize> = None;
            // The index `i` IS the algorithm (the 3.14 windowed scan
            // indexing a[i]); CPython parity.
            #[allow(clippy::needless_range_loop)]
            for i in start..end {
                cruncher.set_seq1(&[a[i]]);
                // Ordering by cheapest to most expensive ratio.
                if cruncher.real_quick_ratio() > best_ratio && cruncher.quick_ratio() > best_ratio {
                    let r = cruncher.ratio();
                    if r > best_ratio {
                        best_i = Some(i);
                        best_ratio = r;
                    }
                }
            }
            let Some(best_i) = best_i else {
                // Found nothing to synch on yet - move to next j.
                continue;
            };
            // Pump out straight replace from before this synch pair.
            out.extend(Self::fancy_helper(a, dump_i, best_i, b, dump_j, j));
            let aelt = a[best_i];
            let belt = b[j];
            if aelt == belt {
                // The synch pair is identical.
                out.push(format!("  {aelt}"));
            } else {
                // Pump out a '-', '?', '+', '?' quad for the synched
                // lines. Unreachable for one-character elements (the synch
                // pair only passes `cutoff` when equal) — kept faithful.
                let mut atags = String::new();
                let mut btags = String::new();
                cruncher.set_seqs(&[aelt], &[belt]);
                for opcode in cruncher.get_opcodes().to_vec() {
                    let (la, lb) = (opcode.i2 - opcode.i1, opcode.j2 - opcode.j1);
                    match opcode.tag {
                        Tag::Replace => {
                            atags.extend(std::iter::repeat_n('^', la));
                            btags.extend(std::iter::repeat_n('^', lb));
                        }
                        Tag::Delete => atags.extend(std::iter::repeat_n('-', la)),
                        Tag::Insert => btags.extend(std::iter::repeat_n('+', lb)),
                        Tag::Equal => {
                            atags.extend(std::iter::repeat_n(' ', la));
                            btags.extend(std::iter::repeat_n(' ', lb));
                        }
                    }
                }
                out.extend(qformat(aelt, belt, &atags, &btags));
            }
            dump_i = best_i + 1;
            dump_j = j + 1;
        }
        // Pump out straight replace from after the last synch pair.
        out.extend(Self::fancy_helper(a, dump_i, ahi, b, dump_j, bhi));
        out
    }

    /// `_fancy_helper` (difflib.py:1009-1020).
    fn fancy_helper(
        a: &[char],
        alo: usize,
        ahi: usize,
        b: &[char],
        blo: usize,
        bhi: usize,
    ) -> Vec<String> {
        if alo < ahi {
            if blo < bhi {
                return Self::plain_replace(a, alo, ahi, b, blo, bhi);
            }
            return dump('-', a, alo, ahi);
        }
        if blo < bhi {
            return dump('+', b, blo, bhi);
        }
        Vec::new()
    }

    /// `_plain_replace` (difflib.py:867-884): dump the shorter block
    /// first — reduces the burden on short-term memory.
    fn plain_replace(
        a: &[char],
        alo: usize,
        ahi: usize,
        b: &[char],
        blo: usize,
        bhi: usize,
    ) -> Vec<String> {
        debug_assert!(alo < ahi && blo < bhi);
        if bhi - blo < ahi - alo {
            let mut out = dump('+', b, blo, bhi);
            out.extend(dump('-', a, alo, ahi));
            out
        } else {
            let mut out = dump('-', a, alo, ahi);
            out.extend(dump('+', b, blo, bhi));
            out
        }
    }
}

/// `_dump` (difflib.py:907-910): generate comparison results for a
/// same-tagged range (`'%s %s' % (tag, x[i])`).
fn dump(tag: char, x: &[char], lo: usize, hi: usize) -> Vec<String> {
    (lo..hi).map(|i| format!("{tag} {}", x[i])).collect()
}

/// `_qformat` (difflib.py:997-1030): format "?" output and deal with tabs.
fn qformat(aline: char, bline: char, atags: &str, btags: &str) -> Vec<String> {
    let aline = aline.to_string();
    let bline = bline.to_string();
    let a_keep = keep_original_ws(&aline, atags);
    let b_keep = keep_original_ws(&bline, btags);
    let atags = py_rstrip(&a_keep);
    let btags = py_rstrip(&b_keep);
    let mut out = vec![format!("- {aline}")];
    if !atags.is_empty() {
        out.push(format!("? {atags}\n"));
    }
    out.push(format!("+ {bline}"));
    if !btags.is_empty() {
        out.push(format!("? {btags}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `CPython` doctest (difflib.py:396-401).
    #[test]
    fn find_longest_match_matches_doctest() {
        let sm = SequenceMatcher::new(
            None,
            &" abcd".chars().collect::<Vec<_>>(),
            &"abcd abcd".chars().collect::<Vec<_>>(),
        );
        assert_eq!(sm.find_longest_match(0, 5, 0, 9), (0, 4, 5));
    }

    /// `CPython` doctest (difflib.py:468-472).
    #[test]
    fn matching_blocks_match_doctest() {
        let mut sm = SequenceMatcher::new(
            None,
            &"abxcd".chars().collect::<Vec<_>>(),
            &"abcd".chars().collect::<Vec<_>>(),
        );
        assert_eq!(
            sm.get_matching_blocks().to_vec(),
            vec![(0, 0, 2), (3, 2, 2), (5, 4, 0)]
        );
    }

    /// `CPython` doctest (difflib.py:537-552).
    #[test]
    fn opcodes_match_doctest() {
        let mut sm = SequenceMatcher::new(
            None,
            &"qabxcd".chars().collect::<Vec<_>>(),
            &"abycdf".chars().collect::<Vec<_>>(),
        );
        let tags: Vec<String> = sm
            .get_opcodes()
            .iter()
            .map(|op| format!("{:?} {}:{} {}:{}", op.tag, op.i1, op.i2, op.j1, op.j2))
            .collect();
        assert_eq!(
            tags,
            [
                "Delete 0:1 0:0",
                "Equal 1:3 0:2",
                "Replace 3:4 2:3",
                "Equal 4:6 3:5",
                "Insert 6:6 5:6"
            ]
        );
    }

    /// The character-mode delta for the newline-merge case, verified
    /// against `list(difflib.ndiff("a\nb\n", "ab\n"))` (Python 3.14.7):
    /// common 'a', deleted '\n', common 'b', common '\n'.
    #[test]
    fn ndiff_lines_newline_merge_matches_python() {
        assert_eq!(
            ndiff_lines("a\nb\n", "ab\n"),
            [(' ', 'a'), ('-', '\n'), (' ', 'b'), (' ', '\n')]
        );
    }

    /// Verified against `list(difflib.ndiff("abc", "abd"))`: equal prefix,
    /// then a replace block rendered as delete/insert (no '?' guides —
    /// unreachable for one-character synch pairs).
    #[test]
    fn ndiff_lines_replace_matches_python() {
        assert_eq!(
            ndiff_lines("abc", "abd"),
            [(' ', 'a'), (' ', 'b'), ('-', 'c'), ('+', 'd')]
        );
    }

    /// Verified against `list(difflib.ndiff("x\n", "\tx\n"))`: the junk
    /// tab cannot anchor the match, so it is inserted before the common
    /// "x\n" (`find_longest_match`'s junk-extension rule).
    #[test]
    fn ndiff_lines_junk_insert_matches_python() {
        assert_eq!(
            ndiff_lines("x\n", "\tx\n"),
            [('+', '\t'), (' ', 'x'), (' ', '\n')]
        );
    }
}
