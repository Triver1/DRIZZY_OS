//! Fuzzy subsequence scoring. Every query char must appear in order; the
//! best alignment (small DP over match positions) earns bonuses for the
//! first char, word starts and runs, and pays for gaps.

use super::apps::App;

const BASE: f32 = 4.0;
const FIRST: f32 = 14.0;
const WORD: f32 = 10.0;
const RUN: f32 = 9.0;
const GAP: f32 = 1.0;
const GAP_MAX: f32 = 7.0;
const LEAD: f32 = 0.4;
/// Matches scoring below this share of the best one are dropped.
const FLOOR: f32 = 0.45;

fn word_start(prev: Option<char>) -> bool {
    matches!(prev, None | Some(' ' | '-' | '_' | '.' | '/' | ':' | '(' ))
}

/// Score `q` (lower-case chars) against lower-case `hay`.
/// `buf` is scratch space reused across calls.
pub fn score(q: &[char], hay: &str, buf: &mut Vec<f32>, chars: &mut Vec<char>) -> Option<f32> {
    if q.is_empty() {
        return Some(0.0);
    }
    chars.clear();
    chars.extend(hay.chars());
    let n = chars.len();
    let m = q.len();
    if m > n {
        return None;
    }
    // Quick reject: is it a subsequence at all?
    let mut qi = 0;
    for &c in chars.iter() {
        if c == q[qi] {
            qi += 1;
            if qi == m {
                break;
            }
        }
    }
    if qi < m {
        return None;
    }
    // best[i * n + j]: best score with q[i] matched at hay[j]; anc[..]:
    // the best such alignment that is anchored (touches a word start or
    // has a run of two), so "fi" never matches "risk of rain" by scatter.
    const NEG: f32 = f32::NEG_INFINITY;
    buf.clear();
    buf.resize(2 * m * n, NEG);
    let (best, anc) = buf.split_at_mut(m * n);
    for i in 0..m {
        for j in i..n {
            if chars[j] != q[i] {
                continue;
            }
            let ws = j == 0 || word_start(Some(chars[j - 1]));
            let mut bonus = BASE;
            if j == 0 {
                bonus += FIRST;
            } else if ws {
                bonus += WORD;
            }
            if i == 0 {
                let v = bonus - (j as f32 * LEAD).min(6.0);
                best[j] = v;
                if ws {
                    anc[j] = v;
                }
                continue;
            }
            let (mut b, mut a) = (NEG, NEG);
            for k in (i - 1)..j {
                let p = best[(i - 1) * n + k];
                if p == NEG {
                    continue;
                }
                let run = k + 1 == j;
                let t = if run { RUN } else { -((j - k - 1) as f32 * GAP).min(GAP_MAX) };
                b = b.max(p + t);
                if run || ws {
                    a = a.max(p + t);
                } else {
                    let pa = anc[(i - 1) * n + k];
                    if pa != NEG {
                        a = a.max(pa + t);
                    }
                }
            }
            if b == NEG {
                continue;
            }
            best[i * n + j] = b + bonus;
            if a != NEG {
                anc[i * n + j] = a + bonus;
            }
        }
    }
    let last = if m >= 2 { &anc[(m - 1) * n..] } else { &best[(m - 1) * n..] };
    let mut s = last.iter().copied().fold(NEG, f32::max);
    if s == NEG {
        return None;
    }
    // Whole-string shape: exact > prefix > contiguous substring.
    let qs: String = q.iter().collect();
    if hay == qs {
        s += 40.0;
    } else if hay.starts_with(&qs) {
        s += 22.0;
    } else if hay.contains(&qs) {
        s += 8.0;
    }
    // Prefer shorter haystacks a little (tighter match).
    Some(s - (n as f32 * 0.08).min(4.0))
}

/// Rank `apps` for query `q` (already lower-cased); returns indices, best
/// first. `freq` is the frecency weight per app.
pub fn rank(apps: &[App], freq: &[f32], q: &str) -> Vec<u32> {
    let qc: Vec<char> = q.chars().filter(|c| !c.is_whitespace()).collect();
    let mut buf = Vec::new();
    let mut chars = Vec::new();
    // (score with frecency, match score, index)
    let mut v: Vec<(f32, f32, u32)> = Vec::new();
    for (i, a) in apps.iter().enumerate() {
        let mut best: Option<f32> = None;
        for (field, w) in a.hay.iter().zip([1.0, 0.72, 0.66, 0.6]) {
            if field.is_empty() {
                continue;
            }
            // Keywords/generic names are lists of words: match each word,
            // so "edit" finds "text editor" but never scatters across words.
            let s = if w < 1.0 && field.contains(' ') {
                field
                    .split(' ')
                    .filter_map(|word| score(&qc, word, &mut buf, &mut chars))
                    .fold(None, |a: Option<f32>, b| Some(a.map_or(b, |a| a.max(b))))
            } else {
                score(&qc, field, &mut buf, &mut chars)
            };
            if let Some(s) = s {
                let s = s * w;
                best = Some(best.map_or(s, |b| b.max(s)));
            }
        }
        if best.is_none() && qc.len() >= 3 {
            let c = a.comment.to_lowercase();
            let qs: String = qc.iter().collect();
            if c.contains(&qs) {
                best = Some(6.0 + qc.len() as f32);
            }
        }
        if let Some(s) = best {
            let f = freq.get(i).copied().unwrap_or(0.0);
            v.push((s + (f * 4.0).min(16.0), s, i as u32));
        }
    }
    // Relative floor: once there is a real hit, weak scatter matches go.
    if qc.len() >= 2 {
        let top = v.iter().map(|e| e.1).fold(0.0f32, f32::max);
        if top > 0.0 {
            v.retain(|e| e.1 >= top * FLOOR);
        }
    }
    v.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.2.cmp(&b.2)));
    v.into_iter().map(|(_, _, i)| i).collect()
}

/// Browse order: most used first, then by name (apps are name-sorted).
pub fn browse(freq: &[f32]) -> Vec<u32> {
    let mut v: Vec<u32> = (0..freq.len() as u32).collect();
    v.sort_by(|&a, &b| freq[b as usize].total_cmp(&freq[a as usize]).then(a.cmp(&b)));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(q: &str, h: &str) -> Option<f32> {
        let qc: Vec<char> = q.chars().collect();
        score(&qc, h, &mut vec![], &mut vec![])
    }

    #[test]
    fn ordering() {
        assert!(s("fi", "firefox").unwrap() > s("fi", "gnome files").unwrap());
        assert!(s("gf", "gnome files").unwrap() > s("gf", "pgfoo").unwrap());
        assert!(s("ff", "firefox").is_some());
        assert!(s("xz", "firefox").is_none());
        assert!(s("term", "terminal").unwrap() > s("term", "gnome system monitor").unwrap_or(0.0));
        // Scattered, unanchored matches are not matches.
        assert!(s("fi", "risk of rain 2").is_none());
        assert!(s("fi", "bambustudio").is_none());
        assert!(s("fi", "gnome files").is_some());
        assert!(s("f", "of").is_some());
    }
}
