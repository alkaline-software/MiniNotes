//! A search over the vault: every note read and looked through for the
//! words typed, as one phrase, whatever their case. A vault is a folder
//! of files and nothing is kept about them, so there is no index to keep
//! right; a thousand notes are read in a few milliseconds, and the window
//! runs a search off its own thread at each keystroke, the newest winning.

use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::vault;

/// One note the phrase is in.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Hit {
    pub path: PathBuf,
    pub title: String,
    /// The note's folder from the vault's root, empty at the root.
    pub folder: String,
    /// Whether the phrase is in the note's name.
    pub in_title: bool,
    /// How many times the phrase is in the note's text.
    pub hits: usize,
    /// The lines the phrase is on, the first few.
    pub snippets: Vec<Snippet>,
}

/// A line the phrase is on, cut to a width, and where the phrase is in it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snippet {
    pub text: String,
    pub marks: Vec<Range<usize>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Results {
    pub query: String,
    pub notes: Vec<Hit>,
    /// Every match in every note.
    pub total: usize,
}

/// How wide a snippet is cut, in characters.
const SNIPPET_W: usize = 120;

/// The notes the phrase is in, the ones named by it first, then by how
/// often it is in them; each with its first `per_note` lines.
pub fn search(root: &Path, query: &str, per_note: usize) -> Results {
    let q = query.trim();
    let mut out = Results { query: query.to_string(), ..Default::default() };
    if q.is_empty() {
        return out;
    }
    for path in vault::notes(root) {
        let title = vault::title(&path);
        let in_title = !find_all(&title, q).is_empty();
        let text = vault::read(&path).unwrap_or_default();
        let found = find_all(&text, q);
        if !in_title && found.is_empty() {
            continue;
        }
        let folder = path.parent().and_then(|d| d.strip_prefix(root).ok()).map(|d| d.to_string_lossy().to_string()).unwrap_or_default();
        let hits = found.len();
        out.total += hits;
        out.notes.push(Hit { path, title, folder, in_title, hits, snippets: snippets(&text, &found, per_note) });
    }
    out.notes.sort_by(|a, b| b.in_title.cmp(&a.in_title).then(b.hits.cmp(&a.hits)).then_with(|| a.title.cmp(&b.title)));
    out
}

/// Every place `q` is in `text`, whatever the case, as byte ranges of
/// `text`. The two are folded to lower case for the look; a fold that
/// changes a byte count, as some letters do, leaves the case as written.
pub fn find_all(text: &str, q: &str) -> Vec<Range<usize>> {
    let (hay, needle) = (text.to_lowercase(), q.to_lowercase());
    let (hay, needle) = if hay.len() == text.len() && needle.len() == q.len() { (hay, needle) } else { (text.to_string(), q.to_string()) };
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = hay[from..].find(&needle) {
        let start = from + at;
        out.push(start..start + needle.len());
        from = start + needle.len().max(1);
        if from >= hay.len() {
            break;
        }
    }
    out
}

/// The first `most` lines the matches are on, each cut around its first
/// match to `SNIPPET_W` characters, with every match on it marked.
fn snippets(text: &str, found: &[Range<usize>], most: usize) -> Vec<Snippet> {
    let mut out: Vec<Snippet> = Vec::new();
    let mut line_end_before = 0;
    for m in found {
        if out.len() >= most {
            break;
        }
        if m.start < line_end_before {
            continue;
        }
        let start = text[..m.start].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let end = text[m.start..].find('\n').map(|i| m.start + i).unwrap_or(text.len());
        line_end_before = end;
        let line = &text[start..end];
        let marks: Vec<Range<usize>> = found.iter().filter(|r| r.start >= start && r.end <= end).map(|r| r.start - start..r.end - start).collect();
        // The line cut to its width around the first match, on
        // characters' boundaries.
        let (cut_from, cut_to) = cut(line, marks[0].start, SNIPPET_W);
        let mut shown = line[cut_from..cut_to].trim_start().to_string();
        let trimmed = line[cut_from..cut_to].len() - shown.len();
        let mut marks: Vec<Range<usize>> = marks.into_iter().filter(|r| r.start >= cut_from + trimmed && r.end <= cut_to).map(|r| r.start - cut_from - trimmed..r.end - cut_from - trimmed).collect();
        if cut_from > 0 {
            shown.insert(0, '\u{2026}');
            for r in &mut marks {
                r.start += 3;
                r.end += 3;
            }
        }
        if cut_to < line.len() {
            shown.push('\u{2026}');
        }
        out.push(Snippet { text: shown, marks });
    }
    out
}

/// Where to cut `line` to `width` characters about `at`, in bytes on
/// characters' boundaries.
fn cut(line: &str, at: usize, width: usize) -> (usize, usize) {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    if chars.len() <= width {
        return (0, line.len());
    }
    let ix = chars.iter().position(|(i, _)| *i >= at).unwrap_or(0);
    let from = ix.saturating_sub(width / 4).min(chars.len() - width);
    let to = (from + width).min(chars.len());
    (chars[from].0, if to == chars.len() { line.len() } else { chars[to].0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_every_match_whatever_the_case() {
        assert_eq!(find_all("Onyx is an onyx, ONYX.", "onyx"), vec![0..4, 11..15, 17..21]);
        assert!(find_all("nothing", "x").is_empty());
    }

    #[test]
    fn a_snippet_is_the_line_with_its_marks() {
        let text = "first line\nthe stone and the Stone again\nlast";
        let found = find_all(text, "stone");
        let s = snippets(text, &found, 3);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].text, "the stone and the Stone again");
        assert_eq!(s[0].marks, vec![4..9, 18..23]);
    }

    #[test]
    fn a_long_line_is_cut_about_the_match() {
        let text = format!("{}needle{}", "a".repeat(300), "b".repeat(300));
        let found = find_all(&text, "needle");
        let s = snippets(&text, &found, 1);
        assert!(s[0].text.chars().count() <= SNIPPET_W + 2);
        let m = &s[0].marks[0];
        assert_eq!(&s[0].text[m.clone()], "needle");
    }
}
