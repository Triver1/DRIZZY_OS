//! An fzf-like ranker for file names and paths.
//!
//! The launcher's matcher (`launcher::fuzzy`) walks chars over short app
//! names; here the haystack is raw path bytes, tens of thousands of them,
//! on every key press, so this one is built to reject fast: a 32-bit
//! "which letters occur" mask throws out most entries in a nanosecond, a
//! subsequence check the next batch, and only the survivors pay for the
//! alignment. That is a Smith-Waterman style DP with affine gaps: bonuses
//! for word starts, camelCase humps and consecutive runs, as in fzf v2.
//!
//! Whitespace separates terms and every term has to match (`tax 2024`).
//! Case folds for ASCII; non-ASCII bytes compare as they are, so accents
//! match themselves but `É` does not find `é`.

/// Longest haystack the DP looks at (bytes): names longer than this match
/// on their first `MAX_HAY` bytes.
const MAX_HAY: usize = 512;

const MATCH: i32 = 16;
const GAP_START: i32 = -3;
const GAP_EXT: i32 = -1;
const B_SEP: i32 = 10;
const B_CAMEL: i32 = 8;
const B_CONSEC: i32 = 5;
const FIRST_MULT: i32 = 2;
const NEG: i32 = i32::MIN / 2;

/// A matched byte range `[start, end)`.
pub type Span = (u16, u16);

#[derive(Clone, Debug)]
pub struct Term {
    bytes: Vec<u8>,
    /// Spans directories: it can only match the whole path.
    slash: bool,
}

/// A parsed query: whitespace-separated terms, lower-cased.
#[derive(Clone, Debug, Default)]
pub struct Query {
    terms: Vec<Term>,
    mask: u32,
}

impl Query {
    pub fn new(s: &str) -> Self {
        let mut q = Query::default();
        for t in s.to_lowercase().split_whitespace().take(8) {
            let bytes: Vec<u8> = t.bytes().take(64).collect();
            q.mask |= mask_of(&bytes);
            q.terms.push(Term { slash: bytes.contains(&b'/'), bytes });
        }
        q
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    pub fn mask(&self) -> u32 {
        self.mask
    }
}

fn bit(b: u8) -> u32 {
    match b {
        b'a'..=b'z' => 1 << (b - b'a'),
        b'A'..=b'Z' => 1 << (b - b'A'),
        b'0'..=b'9' => 1 << 26,
        b'.' => 1 << 27,
        b'-' | b'_' | b' ' => 1 << 28,
        0x80.. => 1 << 29,
        _ => 1 << 30,
    }
}

/// One bit per letter class present in `s`.
pub fn mask_of(s: &[u8]) -> u32 {
    s.iter().fold(0, |m, &b| m | bit(b))
}

fn is_subseq(q: &[u8], h: &[u8]) -> bool {
    let mut i = 0;
    for &b in h {
        if b.to_ascii_lowercase() == q[i] {
            i += 1;
            if i == q.len() {
                return true;
            }
        }
    }
    false
}

fn bonus(prev: Option<u8>, c: u8) -> i32 {
    match prev {
        None => B_SEP + 2,
        Some(b'/') => B_SEP + 2,
        Some(b' ' | b'-' | b'_' | b'.' | b'(' | b'[' | b',' | b'+') => B_SEP,
        Some(p) if p.is_ascii_lowercase() && c.is_ascii_uppercase() => B_CAMEL,
        Some(p) if !p.is_ascii_digit() && c.is_ascii_digit() => B_CAMEL,
        _ => 0,
    }
}

/// Reused between calls so a search allocates once.
#[derive(Default)]
pub struct Scratch {
    prev: Vec<i32>,
    cur: Vec<i32>,
    run: Vec<i32>,
    trace: Vec<u16>,
    bonus: Vec<i32>,
}

/// Best alignment of `q` in `hay`, or None if it is no subsequence. With
/// `out`, its matched positions are appended (ascending, offset by `at`).
fn align(q: &[u8], hay: &[u8], sc: &mut Scratch, out: Option<(&mut Vec<u16>, u16)>) -> Option<i32> {
    let hay = &hay[..hay.len().min(MAX_HAY)];
    let (m, n) = (q.len(), hay.len());
    if m == 0 {
        return Some(0);
    }
    if m > n || !is_subseq(q, hay) {
        return None;
    }
    sc.bonus.clear();
    sc.bonus.extend((0..n).map(|j| bonus(j.checked_sub(1).map(|p| hay[p]), hay[j])));
    sc.prev.clear();
    sc.prev.resize(n, NEG);
    sc.cur.clear();
    sc.cur.resize(n, NEG);
    sc.run.clear();
    sc.run.resize(n, 0);
    let trace = out.is_some();
    if trace {
        sc.trace.clear();
        sc.trace.resize(m * n, u16::MAX);
    }
    for (i, &qc) in q.iter().enumerate() {
        let mut gap = NEG;
        // `run` holds the bonus a run ending at j-1 (previous row) carries;
        // updating it right-to-left would need a copy, so keep one value.
        let mut run_prev = 0;
        for j in 0..n {
            if j >= 2 {
                gap = (gap + GAP_EXT).max(sc.prev[j - 2] + GAP_START);
            }
            let run_here = run_prev;
            run_prev = sc.run[j];
            sc.cur[j] = NEG;
            sc.run[j] = 0;
            if j < i || hay[j].to_ascii_lowercase() != qc {
                continue;
            }
            let b = sc.bonus[j];
            if i == 0 {
                // Earlier is a little better, the way a gap would count.
                sc.cur[j] = MATCH + b * FIRST_MULT - (j as i32 / 2).min(8);
                sc.run[j] = b;
                continue;
            }
            let jump = if gap > NEG / 2 { gap + MATCH + b } else { NEG };
            let (mut v, mut from, mut r) = (jump, u16::MAX, b);
            let p = if j > 0 { sc.prev[j - 1] } else { NEG };
            if p > NEG / 2 {
                let cb = b.max(B_CONSEC).max(run_here);
                let c = p + MATCH + cb;
                if c >= v {
                    v = c;
                    from = (j - 1) as u16;
                    r = cb;
                }
            }
            if v <= NEG / 2 {
                continue;
            }
            sc.cur[j] = v;
            sc.run[j] = r;
            if trace {
                sc.trace[i * n + j] = from;
            }
        }
        std::mem::swap(&mut sc.prev, &mut sc.cur);
    }
    let (end, best) = sc.prev.iter().copied().enumerate().max_by_key(|&(j, v)| (v, std::cmp::Reverse(j)))?;
    if best <= NEG / 2 {
        return None;
    }
    if let Some((pos, at)) = out {
        // Walk back. A jump stores no predecessor (the gap's source was
        // not kept): recover it greedily, the nearest earlier match of the
        // previous query byte; the score is already settled.
        let start = pos.len();
        let mut j = end;
        for i in (0..m).rev() {
            pos.push(at + j as u16);
            if i == 0 {
                break;
            }
            let f = sc.trace[i * n + j];
            j = if f != u16::MAX {
                f as usize
            } else {
                match (0..j.saturating_sub(1)).rev().find(|&k| hay[k].to_ascii_lowercase() == q[i - 1]) {
                    Some(k) => k,
                    None => j.saturating_sub(1),
                }
            };
        }
        pos[start..].reverse();
    }
    Some(best)
}

/// Exact > prefix > substring > word-start shape, for one term in a name.
fn shape(q: &[u8], name: &[u8]) -> i32 {
    let lower = |b: &u8| b.to_ascii_lowercase();
    let n = q.len();
    if name.len() < n {
        return 0;
    }
    let stem = name.iter().rposition(|&b| b == b'.').filter(|&p| p > 0).unwrap_or(name.len());
    if name.len() == n && name.iter().map(lower).eq(q.iter().copied()) {
        70
    } else if stem == n && name[..n].iter().map(lower).eq(q.iter().copied()) {
        60
    } else if name[..n].iter().map(lower).eq(q.iter().copied()) {
        32
    } else {
        0
    }
}

/// Score a bare name (browser filter). None when a term does not match.
pub fn score_name(q: &Query, name: &[u8], sc: &mut Scratch) -> Option<i32> {
    let mut total = 0;
    for t in &q.terms {
        total += align(&t.bytes, name, sc, None)? + shape(&t.bytes, name);
    }
    Some(total - (name.len() as i32 / 6).min(8))
}

/// Merge sorted positions into spans.
fn spans(pos: &mut [u16]) -> Vec<Span> {
    pos.sort_unstable();
    let mut v: Vec<Span> = Vec::new();
    for &p in pos.iter() {
        match v.last_mut() {
            Some(l) if l.1 >= p => l.1 = l.1.max(p + 1),
            _ => v.push((p, p + 1)),
        }
    }
    v
}

/// Matched spans in a bare name, for the browser's highlighting.
pub fn name_spans(q: &Query, name: &[u8], sc: &mut Scratch) -> Vec<Span> {
    let mut pos = vec![];
    for t in &q.terms {
        let _ = align(&t.bytes, name, sc, Some((&mut pos, 0)));
    }
    spans(&mut pos)
}

/// How much a basename match outweighs one in the directories.
const BASE_BONUS: i32 = 48;
/// What a term that only matches across the path has to score, or nearly
/// any long path would "match" by scatter: a clean run anywhere passes, a
/// lone byte has to sit on a word start.
fn path_min(m: usize) -> i32 {
    if m == 1 { MATCH + B_SEP * 2 - 8 } else { (MATCH + B_CONSEC) * m as i32 - 13 }
}

fn path_term(t: &Term, path: &[u8], base: usize, sc: &mut Scratch) -> Option<i32> {
    if !t.slash
        && let Some(s) = align(&t.bytes, &path[base..], sc, None)
    {
        return Some(s + BASE_BONUS + shape(&t.bytes, &path[base..]));
    }
    let s = align(&t.bytes, path, sc, None)?;
    (s >= path_min(t.bytes.len())).then_some(s)
}

/// Score a path relative to the search root; `base` is where its basename
/// starts. `age_days` nudges recently touched files up. Higher is better.
pub fn score_path(q: &Query, path: &[u8], base: usize, age_days: u32, sc: &mut Scratch) -> Option<i32> {
    let mut total = 0;
    for t in &q.terms {
        total += path_term(t, path, base, sc)?;
    }
    let depth = path.iter().filter(|&&b| b == b'/').count() as i32;
    let recent = match age_days {
        0..=1 => 10,
        2..=7 => 7,
        8..=30 => 4,
        31..=120 => 2,
        _ => 0,
    };
    Some(total - depth * 4 - (path.len() as i32 / 5).min(16) + recent)
}

/// Matched spans of a path: (in the basename, in the directories), as the
/// path scorer aligned it. Offsets are relative to each part.
pub fn path_spans(q: &Query, path: &[u8], base: usize, sc: &mut Scratch) -> (Vec<Span>, Vec<Span>) {
    let (mut name, mut dir) = (vec![], vec![]);
    for t in &q.terms {
        if !t.slash && align(&t.bytes, &path[base..], sc, Some((&mut name, 0))).is_some() {
            continue;
        }
        let mut pos = vec![];
        if align(&t.bytes, path, sc, Some((&mut pos, 0))).is_some() {
            for p in pos {
                if p as usize >= base {
                    name.push(p - base as u16);
                } else {
                    dir.push(p);
                }
            }
        }
    }
    (spans(&mut name), spans(&mut dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(q: &str, h: &str) -> Option<i32> {
        score_name(&Query::new(q), h.as_bytes(), &mut Scratch::default())
    }

    fn path(q: &str, p: &str) -> Option<i32> {
        let base = p.rfind('/').map_or(0, |i| i + 1);
        score_path(&Query::new(q), p.as_bytes(), base, 999, &mut Scratch::default())
    }

    fn best<'a>(q: &str, ps: &[&'a str]) -> &'a str {
        ps.iter().copied().filter(|p| path(q, p).is_some()).max_by_key(|p| path(q, p).unwrap()).unwrap_or("")
    }

    #[test]
    fn subsequence_required() {
        assert!(name("rpt", "report.txt").is_some());
        assert!(name("xyz", "report.txt").is_none());
        assert!(name("reprot", "report.txt").is_none());
        assert!(name("", "report.txt").is_some());
    }

    #[test]
    fn exact_beats_prefix_beats_scatter() {
        let exact = name("notes", "notes").unwrap();
        let stem = name("notes", "notes.md").unwrap();
        let prefix = name("notes", "notes-old.md").unwrap();
        let inner = name("notes", "my-notes.md").unwrap();
        let scatter = name("notes", "no-trees-here.md").unwrap();
        assert!(exact > stem, "{exact} {stem}");
        assert!(stem > prefix, "{stem} {prefix}");
        assert!(prefix > inner, "{prefix} {inner}");
        assert!(inner > scatter, "{inner} {scatter}");
    }

    #[test]
    fn consecutive_runs_and_word_starts() {
        assert!(name("rep", "report").unwrap() > name("rep", "r_e_p").unwrap());
        // Word starts count: b-a-r in "big-apple-row" beats a buried one.
        assert!(name("bar", "big-apple-row").unwrap() > name("bar", "xbxaxrx").unwrap());
        // camelCase humps are word starts too.
        assert!(name("fb", "FileBrowser").unwrap() > name("fb", "fabulous").unwrap());
    }

    #[test]
    fn basename_beats_path() {
        assert_eq!(best("invoice", &["invoice/2019/scan.pdf", "work/invoice.pdf"]), "work/invoice.pdf");
        assert_eq!(best("kernel", &["kernel/readme.txt", "src/deep/kernel.c"]), "src/deep/kernel.c");
    }

    #[test]
    fn shorter_paths_win_ties() {
        assert_eq!(best("todo", &["a/b/c/todo.txt", "a/todo.txt", "a/b/todo.txt"]), "a/todo.txt");
    }

    #[test]
    fn recent_files_are_nudged_not_promoted() {
        let q = Query::new("plan");
        let sc = &mut Scratch::default();
        let (p, b) = (b"docs/plan.md", 5);
        assert!(score_path(&q, p, b, 0, sc).unwrap() > score_path(&q, p, b, 900, sc).unwrap());
        // A week-old exact name still beats a fresh weak match.
        let weak = score_path(&q, b"docs/poland-map.txt", 5, 0, sc).unwrap();
        assert!(score_path(&q, p, b, 400, sc).unwrap() > weak);
    }

    #[test]
    fn path_scatter_is_rejected() {
        // Letters strewn over a long path are not a match…
        assert!(path("xyq", "axb/some/cyd/folder/eqf/file.txt").is_none());
        assert!(path("q", "axb/cqd/file.txt").is_none());
        // …but a clean run in a folder name is, even mid-word.
        assert!(path("photos", "photos/2021/img_0001.jpg").is_some());
        assert!(path("ports", "reports/2021/img_0001.jpg").is_some());
        // Terms with a slash always mean the path.
        assert!(path("doc/rep", "docs/report.txt").is_some());
    }

    #[test]
    fn terms_are_anded() {
        assert!(path("tax 2024", "finance/tax-2024.pdf").is_some());
        assert!(path("tax 2024", "finance/tax-2023.pdf").is_none());
        assert!(path("2024 tax", "2024/tax.pdf").is_some());
    }

    #[test]
    fn case_folds_and_bytes_are_safe() {
        assert!(name("readme", "README.md").is_some());
        // Invalid UTF-8 and non-ASCII never panic or misalign.
        assert!(score_name(&Query::new("a"), b"\xff\xfea\xc3", &mut Scratch::default()).is_some());
        assert!(name("日本", "日本語.txt").is_some());
    }

    #[test]
    fn spans_cover_the_match() {
        let (q, sc) = (Query::new("rep"), &mut Scratch::default());
        assert_eq!(name_spans(&q, b"my-report.txt", sc), vec![(3, 6)]);
        let q = Query::new("mrt");
        let s = name_spans(&q, b"my-report.txt", sc);
        assert_eq!(s.iter().map(|s| s.1 - s.0).sum::<u16>(), 3);
        let (q, p) = (Query::new("docs/re"), b"docs/report.txt");
        let (n, d) = path_spans(&q, p, 5, sc);
        assert_eq!(n, vec![(0, 2)]);
        assert_eq!(d, vec![(0, 5)]);
    }

    #[test]
    fn traced_score_matches_plain_score() {
        // The trace is the same DP: positions never change the result.
        let sc = &mut Scratch::default();
        for (q, h) in [("abc", "a-b-c"), ("fbr", "FileBrowser.rs"), ("aa", "aXaXa")] {
            let plain = align(q.as_bytes(), h.as_bytes(), sc, None);
            let mut pos = vec![];
            let traced = align(q.as_bytes(), h.as_bytes(), sc, Some((&mut pos, 0)));
            assert_eq!(plain, traced);
            assert_eq!(pos.len(), q.len());
            assert!(pos.windows(2).all(|w| w[0] < w[1]), "{pos:?}");
        }
    }

    #[test]
    fn long_haystacks_are_bounded() {
        let long = "a".repeat(5000);
        assert!(score_name(&Query::new("aaa"), long.as_bytes(), &mut Scratch::default()).is_some());
    }
}
