//! A note's markdown read for the live preview: what each line is (a
//! heading, a list item, a line of a fence, a row of a table), and every
//! inline span with where its marks stand, so the editor can hide the
//! marks of a span the caret is not in and draw the words styled.
//!
//! The reading is pulldown-cmark's, with offsets, over the whole text;
//! what is kept is by the line, since the editor draws by the line.
//! Nothing here knows the caret: the editor decides what to hide.

use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd};

use crate::lines::{list_line, quote_line, ListKind};

/// What a line is, by its block.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum LineKind {
    #[default]
    Text,
    Blank,
    Heading(u8),
    /// A setext heading's underline (`===` or `---`), under its words.
    SetextUnderline,
    /// The opening fence of a code block, with its language.
    FenceOpen(String),
    FenceClose,
    /// A line of code, in a fence or indented.
    Code,
    /// The `---` lines around a YAML header.
    FrontMatterEdge,
    FrontMatter,
    Rule,
    /// The first line of a table; the table is drawn here.
    TableFirst(usize),
    /// Any other line of a table, drawn as part of the first.
    TableRow(usize),
    Html,
}

/// A list item's mark on its first line, replaced by a widget: a disc, a
/// number, a box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListMark {
    /// The marker as written, with its trailing space (`- `, `1. `, `- [ ] `).
    pub marker: Range<usize>,
    pub kind: ListKind,
    /// How deep the item is nested, from 0.
    pub depth: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct LineInfo {
    pub kind: LineKind,
    /// Block syntax on the line, shown muted when the caret is on it and
    /// hidden otherwise: a heading's `#`s, a quote's `>`s.
    pub marks: Vec<Range<usize>>,
    pub quote_depth: usize,
    pub list: Option<ListMark>,
    /// How many list levels the line stands inside, for indent guides:
    /// a nested item's line, or a continuation line of one.
    pub list_depth: usize,
    /// Where a task line's words begin, for striking a done task through.
    pub task_done: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpanKind {
    Strong,
    Em,
    Strike,
    Code,
    Link(String),
    /// A `[[note]]` link: the note it names.
    WikiLink(String),
    /// A picture: its source.
    Image(String),
    Highlight,
    Math,
    Footnote,
    Html,
}

/// An inline span in the whole text: its range, what it is, and its
/// opening and closing marks, which are hidden when the caret is away.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub kind: SpanKind,
    pub open: Range<usize>,
    pub close: Range<usize>,
}

impl Span {
    /// The words between the marks.
    pub fn inner(&self) -> Range<usize> {
        self.open.end..self.close.start
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Align {
    None,
    Left,
    Center,
    Right,
}

/// A table, read from its lines: the header row first, the separator
/// left out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    pub first: usize,
    pub last: usize,
    pub aligns: Vec<Align>,
    pub rows: Vec<Vec<String>>,
}

/// A fenced block of code: its fence lines and its language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeBlock {
    pub first: usize,
    pub last: usize,
    pub lang: String,
    /// Whether the closing fence is there; without it the block runs to the end.
    pub closed: bool,
}

/// A list as the parser found it: whether it is numbered, and the first
/// line of each of its items, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListRange {
    pub ordered: bool,
    pub items: Vec<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct Analysis {
    pub lines: Vec<LineInfo>,
    pub lists: Vec<ListRange>,
    /// Every span, in order of its start, the outer before the inner.
    pub spans: Vec<Span>,
    /// For each line, the spans that touch it, as indices into `spans`.
    pub line_spans: Vec<Vec<usize>>,
    pub tables: Vec<Table>,
    pub code_blocks: Vec<CodeBlock>,
    pub line_starts: Vec<usize>,
}

impl Analysis {
    /// The line a byte of the whole text is on.
    #[allow(dead_code)]
    pub fn line_of(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
    }

    /// The code block a line is in, fences included.
    pub fn code_block_at(&self, line: usize) -> Option<&CodeBlock> {
        self.code_blocks.iter().find(|b| b.first <= line && line <= b.last)
    }
}

/// The text read whole.
pub fn analyze(text: &str) -> Analysis {
    let line_starts: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
    let n = line_starts.len();
    let line_text = |i: usize| -> &str {
        let start = line_starts[i];
        let end = line_starts.get(i + 1).map_or(text.len(), |e| e - 1);
        &text[start..end]
    };
    let mut lines: Vec<LineInfo> = (0..n).map(|i| LineInfo { kind: if line_text(i).trim().is_empty() { LineKind::Blank } else { LineKind::Text }, ..Default::default() }).collect();
    let mut a = Analysis { line_starts: line_starts.clone(), ..Default::default() };
    let line_of = |offset: usize| match line_starts.binary_search(&offset) {
        Ok(i) => i,
        Err(i) => i.saturating_sub(1),
    };
    // The last line a range touches: its end is past the range's last byte.
    let last_line_of = |r: &Range<usize>| if r.end > r.start { line_of(r.end - 1) } else { line_of(r.start) };

    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_MATH | Options::ENABLE_WIKILINKS | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS | Options::ENABLE_FOOTNOTES);
    let parser = Parser::new_ext(text, opts);

    // The lists open around the parser's place, as indices into `a.lists`.
    let mut lists: Vec<usize> = Vec::new();
    // A link or a picture open: its range, where its words have got to.
    struct OpenLink {
        range: Range<usize>,
        words_end: usize,
        kind: SpanKind,
        open_len: usize,
    }
    let mut links: Vec<OpenLink> = Vec::new();
    let mut item_line: Option<usize> = None;

    for (ev, r) in parser.into_offset_iter() {
        match ev {
            Event::Start(Tag::Heading { level, .. }) => {
                let line = line_of(r.start);
                let t = line_text(line);
                let lead = t.len() - t.trim_start().len();
                let hashes = t[lead..].chars().take_while(|c| *c == '#').count();
                if hashes > 0 && hashes <= 6 && t[lead + hashes..].starts_with([' ', '\t']) || t[lead..].trim_end() == "#".repeat(hashes) {
                    let mut end = lead + hashes;
                    while t[end..].starts_with(' ') {
                        end += 1;
                    }
                    let mut marks = vec![lead..end];
                    // A closing run of `#`s, which markdown drops.
                    let trimmed = t.trim_end();
                    let tail = trimmed.len() - trimmed.trim_end_matches('#').len();
                    if tail > 0 && trimmed[..trimmed.len() - tail].ends_with(' ') && trimmed.len() - tail > end {
                        let from = trimmed[..trimmed.len() - tail].trim_end().len();
                        marks.push(from..t.len());
                    }
                    lines[line].kind = LineKind::Heading(level as u8);
                    lines[line].marks = marks;
                } else {
                    lines[line].kind = LineKind::Heading(level as u8);
                    let under = last_line_of(&r);
                    if under > line && under < n {
                        lines[under].kind = LineKind::SetextUnderline;
                        lines[under].marks = vec![0..line_text(under).len()];
                    }
                }
            }
            Event::Start(Tag::BlockQuote(_)) => {
                for line in line_of(r.start)..=last_line_of(&r).min(n - 1) {
                    lines[line].quote_depth += 1;
                }
            }
            Event::Start(Tag::List(start)) => {
                lists.push(a.lists.len());
                a.lists.push(ListRange { ordered: start.is_some(), items: Vec::new() });
            }
            Event::End(TagEnd::List(_)) => {
                lists.pop();
            }
            Event::Start(Tag::Item) => {
                let line = line_of(r.start);
                let depth = lists.len().saturating_sub(1);
                let t = line_text(line);
                let body_start = quote_line(t).map_or(0, |(_, at)| at);
                if let Some(item) = list_line(&t[body_start..]) {
                    let from = body_start + item.indent.len();
                    lines[line].list = Some(ListMark { marker: from..from + item.marker.len(), kind: item.kind, depth });
                    lines[line].task_done = matches!(item.kind, ListKind::Task(true));
                }
                for l in line..=last_line_of(&r).min(n - 1) {
                    lines[l].list_depth = lines[l].list_depth.max(depth + 1);
                }
                if let Some(&ix) = lists.last() {
                    a.lists[ix].items.push(line);
                }
                item_line = Some(line);
            }
            Event::TaskListMarker(done) => {
                if let Some(line) = item_line {
                    if let Some(m) = lines[line].list.as_mut() {
                        m.kind = ListKind::Task(done);
                        let t = line_text(line);
                        let mut end = r.end - line_starts[line];
                        if t[end..].starts_with(' ') {
                            end += 1;
                        }
                        m.marker.end = end;
                        lines[line].task_done = done;
                    }
                }
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let first = line_of(r.start);
                let last = last_line_of(&r).min(n - 1);
                match kind {
                    CodeBlockKind::Fenced(lang) => {
                        let fence_of = |t: &str| t.trim_start().starts_with("```") || t.trim_start().starts_with("~~~");
                        let closed = last > first && fence_of(line_text(last));
                        lines[first].kind = LineKind::FenceOpen(lang.trim().to_string());
                        let body_end = if closed { last } else { last + 1 };
                        for l in first + 1..body_end.min(n) {
                            lines[l].kind = LineKind::Code;
                        }
                        if closed {
                            lines[last].kind = LineKind::FenceClose;
                        }
                        a.code_blocks.push(CodeBlock { first, last, lang: lang.trim().to_string(), closed });
                    }
                    CodeBlockKind::Indented => {
                        for l in first..=last {
                            lines[l].kind = LineKind::Code;
                        }
                    }
                }
            }
            Event::Start(Tag::MetadataBlock(_)) => {
                let first = line_of(r.start);
                let last = last_line_of(&r).min(n - 1);
                for l in first..=last {
                    lines[l].kind = if l == first || l == last { LineKind::FrontMatterEdge } else { LineKind::FrontMatter };
                }
            }
            Event::Start(Tag::HtmlBlock) => {
                for l in line_of(r.start)..=last_line_of(&r).min(n - 1) {
                    if lines[l].kind == LineKind::Text {
                        lines[l].kind = LineKind::Html;
                    }
                }
            }
            Event::Rule => {
                lines[line_of(r.start)].kind = LineKind::Rule;
            }
            Event::Start(Tag::Table(aligns)) => {
                let first = line_of(r.start);
                let mut last = last_line_of(&r).min(n - 1);
                // The parser's range may end at the row's newline; a row
                // is a line that starts with a bar.
                while last + 1 < n && line_text(last + 1).trim_start().starts_with('|') {
                    last += 1;
                }
                while last > first && line_text(last).trim().is_empty() {
                    last -= 1;
                }
                let ix = a.tables.len();
                let mut rows = Vec::new();
                for l in first..=last {
                    if l != first + 1 {
                        rows.push(cells(line_text(l)));
                    }
                    lines[l].kind = if l == first { LineKind::TableFirst(ix) } else { LineKind::TableRow(ix) };
                }
                let aligns = aligns
                    .iter()
                    .map(|al| match al {
                        pulldown_cmark::Alignment::Left => Align::Left,
                        pulldown_cmark::Alignment::Center => Align::Center,
                        pulldown_cmark::Alignment::Right => Align::Right,
                        pulldown_cmark::Alignment::None => Align::None,
                    })
                    .collect();
                a.tables.push(Table { first, last, aligns, rows });
            }
            Event::Start(Tag::Emphasis) => a.spans.push(Span { range: r.clone(), kind: SpanKind::Em, open: r.start..r.start + 1, close: r.end - 1..r.end }),
            Event::Start(Tag::Strong) => a.spans.push(Span { range: r.clone(), kind: SpanKind::Strong, open: r.start..r.start + 2, close: r.end - 2..r.end }),
            Event::Start(Tag::Strikethrough) => {
                let m = if text[r.start..].starts_with("~~") { 2 } else { 1 };
                a.spans.push(Span { range: r.clone(), kind: SpanKind::Strike, open: r.start..r.start + m, close: r.end - m..r.end });
            }
            Event::Code(_) => {
                let m = text[r.start..].chars().take_while(|c| *c == '`').count();
                a.spans.push(Span { range: r.clone(), kind: SpanKind::Code, open: r.start..r.start + m, close: r.end - m..r.end });
            }
            Event::InlineMath(_) => a.spans.push(Span { range: r.clone(), kind: SpanKind::Math, open: r.start..r.start + 1, close: r.end - 1..r.end }),
            Event::DisplayMath(_) => {
                let m = 2.min(r.len() / 2);
                a.spans.push(Span { range: r.clone(), kind: SpanKind::Math, open: r.start..r.start + m, close: r.end - m..r.end });
            }
            Event::FootnoteReference(_) => a.spans.push(Span { range: r.clone(), kind: SpanKind::Footnote, open: r.start..r.start, close: r.end..r.end }),
            Event::InlineHtml(_) => a.spans.push(Span { range: r.clone(), kind: SpanKind::Html, open: r.start..r.start, close: r.end..r.end }),
            Event::Start(Tag::Link { link_type, dest_url, .. }) => {
                let (kind, open_len) = match link_type {
                    LinkType::WikiLink { .. } => (SpanKind::WikiLink(dest_url.to_string()), 2),
                    LinkType::Autolink | LinkType::Email => (SpanKind::Link(dest_url.to_string()), 1),
                    _ => (SpanKind::Link(dest_url.to_string()), 1),
                };
                links.push(OpenLink { range: r.clone(), words_end: r.start + open_len, kind, open_len });
            }
            Event::Start(Tag::Image { dest_url, .. }) => links.push(OpenLink { range: r.clone(), words_end: r.start + 2, kind: SpanKind::Image(dest_url.to_string()), open_len: 2 }),
            Event::End(TagEnd::Link) | Event::End(TagEnd::Image) => {
                if let Some(l) = links.pop() {
                    let words_end = l.words_end.max(l.range.start + l.open_len).min(l.range.end);
                    let mut open = l.range.start..l.range.start + l.open_len;
                    // A wikilink's words may be an alias after `|`: the
                    // target and the bar are part of the opening mark.
                    if let SpanKind::WikiLink(_) = l.kind {
                        let inner = &text[l.range.start + 2..l.range.end.saturating_sub(2).max(l.range.start + 2)];
                        if let Some(bar) = inner.find('|') {
                            open = l.range.start..l.range.start + 2 + bar + 1;
                        }
                    }
                    let words_end = words_end.max(open.end);
                    a.spans.push(Span { range: l.range.clone(), kind: l.kind, open, close: words_end..l.range.end });
                }
            }
            Event::Text(_) => {
                if let Some(l) = links.last_mut() {
                    l.words_end = r.end;
                }
                // `==words==`, which the parser does not know.
                let s = &text[r.clone()];
                let mut from = 0;
                while let Some(open) = s[from..].find("==").map(|i| i + from) {
                    let after = open + 2;
                    let Some(close) = s[after..].find("==").map(|i| i + after) else { break };
                    if close > after && !s[after..close].starts_with(' ') && !s[after..close].ends_with(' ') {
                        a.spans.push(Span { range: r.start + open..r.start + close + 2, kind: SpanKind::Highlight, open: r.start + open..r.start + after, close: r.start + close..r.start + close + 2 });
                    }
                    from = close + 2;
                }
            }
            Event::Start(_) | Event::End(_) | Event::Html(_) | Event::SoftBreak | Event::HardBreak => {
                if let (Some(l), Event::End(_)) = (links.last_mut(), &ev) {
                    l.words_end = l.words_end.max(r.end.min(l.range.end));
                }
                if let Some(l) = links.last_mut() {
                    if matches!(ev, Event::SoftBreak | Event::HardBreak) {
                        l.words_end = r.end;
                    }
                }
            }
        }
    }

    // A quote's marks on each of its lines, by the depth the parser gave it.
    for (i, info) in lines.iter_mut().enumerate() {
        if info.quote_depth > 0 {
            let t = line_text(i);
            let mut at = 0;
            let mut seen = 0;
            let bytes = t.as_bytes();
            while seen < info.quote_depth && at < bytes.len() {
                let mark_start = at;
                while at < bytes.len() && bytes[at] == b' ' {
                    at += 1;
                }
                if at < bytes.len() && bytes[at] == b'>' {
                    at += 1;
                    if at < bytes.len() && bytes[at] == b' ' {
                        at += 1;
                    }
                    info.marks.push(mark_start..at);
                    seen += 1;
                } else {
                    break;
                }
            }
        }
    }

    a.spans.sort_by(|x, y| x.range.start.cmp(&y.range.start).then(y.range.end.cmp(&x.range.end)));
    a.line_spans = vec![Vec::new(); n];
    for (ix, s) in a.spans.iter().enumerate() {
        let first = line_of(s.range.start);
        let last = last_line_of(&s.range).min(n - 1);
        for l in first..=last {
            a.line_spans[l].push(ix);
        }
    }
    a.lines = lines;
    a
}

/// A table row's cells, as written between its bars, trimmed. A bar
/// escaped with a backslash is part of its cell.
pub fn cells(row: &str) -> Vec<String> {
    let t = row.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    let mut out = Vec::new();
    let mut cell = String::new();
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'|') {
            cell.push('|');
            chars.next();
        } else if c == '|' {
            out.push(cell.trim().to_string());
            cell.clear();
        } else {
            cell.push(c);
        }
    }
    out.push(cell.trim().to_string());
    out
}

/// A line shown with some of its bytes hidden: the words that show, and
/// for each shown piece where it came from, so a place in the words maps
/// back to the text and the other way.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shown {
    pub text: String,
    /// (start in `text`, start in the source, length), in order.
    pub pieces: Vec<(usize, usize, usize)>,
}

impl Shown {
    /// `source` with `hidden` taken out. The ranges are in the line's bytes,
    /// in any order, and may overlap.
    pub fn new(source: &str, hidden: &[Range<usize>]) -> Shown {
        let mut hidden: Vec<Range<usize>> = hidden.iter().filter(|r| r.end > r.start).cloned().collect();
        hidden.sort_by_key(|r| r.start);
        let mut merged: Vec<Range<usize>> = Vec::new();
        for r in hidden {
            match merged.last_mut() {
                Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
                _ => merged.push(r),
            }
        }
        let mut shown = Shown::default();
        let mut at = 0;
        for r in merged.iter().chain(std::iter::once(&(source.len()..source.len()))) {
            let start = r.start.min(source.len());
            if start > at {
                shown.pieces.push((shown.text.len(), at, start - at));
                shown.text.push_str(&source[at..start]);
            }
            at = at.max(r.end.min(source.len()));
        }
        shown
    }

    /// Where a byte of the source stands in the words; a hidden byte
    /// stands where its hidden run begins.
    pub fn to_shown(&self, source_col: usize) -> usize {
        for &(d, s, len) in &self.pieces {
            if source_col < s {
                return d;
            }
            if source_col <= s + len {
                return d + (source_col - s);
            }
        }
        self.text.len()
    }

    /// The byte of the source a place in the words stands for. At the
    /// seam between two pieces it is the earlier piece's end, before the
    /// hidden bytes: a caret put at a span's left edge stands before the
    /// whole span, and the editor snaps one at the right edge past it.
    pub fn to_source(&self, shown_col: usize, source_len: usize) -> usize {
        for &(d, s, len) in &self.pieces {
            if shown_col <= d + len {
                return s + shown_col.saturating_sub(d);
            }
        }
        source_len
    }

    /// Whether a byte of the source is among the hidden.
    pub fn is_hidden(&self, source_col: usize) -> bool {
        !self.pieces.iter().any(|&(_, s, len)| source_col >= s && source_col < s + len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_are_told_by_the_line() {
        let text = "---\ntitle: x\n---\n# Title ##\n\n> quoted\n> > deep\n\n- one\n  - [x] two\n3. three\n\n```rust\nlet a = 1;\n```\n\n| a | b |\n|---|:-:|\n| 1 | 2 |\n\n***\nSetext\n======\n";
        let a = analyze(text);
        let kinds: Vec<&LineKind> = a.lines.iter().map(|l| &l.kind).collect();
        assert_eq!(kinds[0], &LineKind::FrontMatterEdge);
        assert_eq!(kinds[1], &LineKind::FrontMatter);
        assert_eq!(kinds[2], &LineKind::FrontMatterEdge);
        assert_eq!(kinds[3], &LineKind::Heading(1));
        assert_eq!(a.lines[3].marks, vec![0..2, 7..10]);
        assert_eq!(kinds[4], &LineKind::Blank);
        assert_eq!(a.lines[5].quote_depth, 1);
        assert_eq!(a.lines[5].marks, vec![0..2]);
        assert_eq!(a.lines[6].quote_depth, 2);
        assert_eq!(a.lines[6].marks, vec![0..2, 2..4]);
        let one = a.lines[8].list.clone().unwrap();
        assert_eq!((one.marker, one.kind, one.depth), (0..2, ListKind::Bullet, 0));
        let two = a.lines[9].list.clone().unwrap();
        assert_eq!((two.marker, two.kind, two.depth), (2..8, ListKind::Task(true), 1));
        assert!(a.lines[9].task_done);
        assert_eq!(a.lines[9].list_depth, 2);
        let three = a.lines[10].list.clone().unwrap();
        assert_eq!((three.marker, three.kind), (0..3, ListKind::Ordered(3)));
        assert_eq!(kinds[12], &LineKind::FenceOpen("rust".into()));
        assert_eq!(kinds[13], &LineKind::Code);
        assert_eq!(kinds[14], &LineKind::FenceClose);
        assert_eq!(a.code_blocks[0], CodeBlock { first: 12, last: 14, lang: "rust".into(), closed: true });
        assert_eq!(kinds[16], &LineKind::TableFirst(0));
        assert_eq!(kinds[17], &LineKind::TableRow(0));
        assert_eq!(kinds[18], &LineKind::TableRow(0));
        assert_eq!(a.tables[0].rows, vec![vec!["a".to_string(), "b".to_string()], vec!["1".to_string(), "2".to_string()]]);
        assert_eq!(a.tables[0].aligns, vec![Align::None, Align::Center]);
        assert_eq!(kinds[20], &LineKind::Rule);
        assert_eq!(kinds[21], &LineKind::Heading(1));
        assert_eq!(kinds[22], &LineKind::SetextUnderline);
    }

    #[test]
    fn lists_know_their_items() {
        let a = analyze("1. a\n3. b\n   - x\n   - y\n9. c\n\n- z");
        assert_eq!(a.lists.len(), 3);
        assert_eq!(a.lists[0], ListRange { ordered: true, items: vec![0, 1, 4] });
        assert_eq!(a.lists[1], ListRange { ordered: false, items: vec![2, 3] });
        assert_eq!(a.lists[2], ListRange { ordered: false, items: vec![6] });
    }

    #[test]
    fn an_open_fence_runs_to_the_end() {
        let a = analyze("```\ncode\nmore");
        assert_eq!(a.lines[0].kind, LineKind::FenceOpen(String::new()));
        assert_eq!(a.lines[1].kind, LineKind::Code);
        assert_eq!(a.lines[2].kind, LineKind::Code);
        assert!(!a.code_blocks[0].closed);
    }

    #[test]
    fn spans_have_their_marks() {
        let text = "a **bold** and *em* and ~~gone~~ and `code` and [link](http://x) and [[Note|alias]] and ![pic](p.png) and ==hi== and $x^2$ and [[Plain]]";
        let a = analyze(text);
        let find = |k: &SpanKind| a.spans.iter().find(|s| &s.kind == k).unwrap().clone();
        let s = find(&SpanKind::Strong);
        assert_eq!((&text[s.open.clone()], &text[s.inner()], &text[s.close.clone()]), ("**", "bold", "**"));
        let s = find(&SpanKind::Em);
        assert_eq!(&text[s.inner()], "em");
        let s = find(&SpanKind::Strike);
        assert_eq!((&text[s.open.clone()], &text[s.inner()]), ("~~", "gone"));
        let s = find(&SpanKind::Code);
        assert_eq!(&text[s.inner()], "code");
        let s = find(&SpanKind::Link("http://x".into()));
        assert_eq!((&text[s.open.clone()], &text[s.inner()], &text[s.close.clone()]), ("[", "link", "](http://x)"));
        let s = find(&SpanKind::WikiLink("Note".into()));
        assert_eq!((&text[s.open.clone()], &text[s.inner()], &text[s.close.clone()]), ("[[Note|", "alias", "]]"));
        let s = find(&SpanKind::WikiLink("Plain".into()));
        assert_eq!((&text[s.open.clone()], &text[s.inner()], &text[s.close.clone()]), ("[[", "Plain", "]]"));
        let s = find(&SpanKind::Image("p.png".into()));
        assert_eq!((&text[s.open.clone()], &text[s.inner()], &text[s.close.clone()]), ("![", "pic", "](p.png)"));
        let s = find(&SpanKind::Highlight);
        assert_eq!(&text[s.inner()], "hi");
        let s = find(&SpanKind::Math);
        assert_eq!(&text[s.inner()], "x^2");
    }

    #[test]
    fn nested_emphasis_keeps_each_layers_marks() {
        let text = "***both*** and **a *b* c**";
        let a = analyze(text);
        let em = a.spans.iter().filter(|s| s.kind == SpanKind::Em).count();
        let strong = a.spans.iter().filter(|s| s.kind == SpanKind::Strong).count();
        assert_eq!((em, strong), (2, 2));
        let outer = &a.spans[0];
        assert_eq!(&text[outer.range.clone()], "***both***");
        assert_eq!(a.line_spans[0].len(), 4);
    }

    #[test]
    fn spans_crossing_lines_are_on_each() {
        let a = analyze("some **bold\nwords** here\nplain");
        assert_eq!(a.line_spans[0].len(), 1);
        assert_eq!(a.line_spans[1].len(), 1);
        assert!(a.line_spans[2].is_empty());
    }

    #[test]
    fn shown_text_maps_both_ways() {
        let src = "**bold** and `x`";
        let s = Shown::new(src, &[0..2, 6..8, 13..14, 15..16]);
        assert_eq!(s.text, "bold and x");
        assert_eq!(s.to_shown(2), 0);
        assert_eq!(s.to_shown(1), 0);
        assert_eq!(s.to_shown(6), 4);
        assert_eq!(s.to_shown(9), 5);
        assert_eq!(s.to_shown(16), 10);
        assert_eq!(s.to_source(0, src.len()), 2);
        assert_eq!(s.to_source(4, src.len()), 6);
        assert_eq!(s.to_source(9, src.len()), 13);
        assert_eq!(s.to_source(10, src.len()), 15);
        assert!(s.is_hidden(0));
        assert!(!s.is_hidden(3));
        let none = Shown::new("abc", &[]);
        assert_eq!(none.text, "abc");
        assert_eq!(none.to_source(3, 3), 3);
    }

    #[test]
    fn table_cells_are_split_on_unescaped_bars() {
        assert_eq!(cells("| a \\| b | c |"), vec!["a | b", "c"]);
        assert_eq!(cells("x|y"), vec!["x", "y"]);
    }
}
