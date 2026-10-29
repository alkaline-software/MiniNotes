//! The live-preview editor: a note's markdown drawn as it reads, with the
//! marks of whatever the caret is in shown as written, as Obsidian's
//! editor does. One element draws every line itself (`EditorElement`):
//! the text shaped by gpui, the bullets, boxes, bars, plates and rules
//! as quads, and the caret and selection over them. The toolkit's input
//! was not used: it lays every line out in one face at one size, and a
//! heading is larger than the words under it.
//!
//! The text is `lines::Doc`; what each line is comes from
//! `markdown::analyze`, read again whole after every change. A line is
//! laid out when its text, its hidden marks or the width change
//! (`Cache`), so a keystroke lays out the lines it touched and not the
//! note. Typing arrives through gpui's input handler, so an input method
//! composes in place; keys that move or change arrive as actions.

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::*;
use gpui_component::text::{TextView, TextViewStyle};
use gpui_component::{input, ActiveTheme as _};

use crate::lines::{list_line, next_marker, quote_line, Doc, ListKind, Pos, Sel};
use crate::markdown::{self, Analysis, LineKind, Shown, SpanKind};

actions!(
    editor,
    [Enter, NewLine, Tab, Outdent, Backspace, Delete, DeleteWordBack, DeleteLineBack, Left, Right, Up, Down, SelectLeft, SelectRight, SelectUp, SelectDown, WordLeft, WordRight, SelectWordLeft, SelectWordRight, LineStart, LineEnd, SelectLineStart, SelectLineEnd, DocStart, DocEnd, SelectDocStart, SelectDocEnd, PageUp, PageDown, Bold, Italic, ToggleTask]
);

/// The key context of a note being written in.
pub const EDITOR_CONTEXT: &str = "Editor";

/// Each key as the Mac has it, bound as `secondary-<key>` where ⌘ is
/// meant (Ctrl off a Mac). Copy, Cut, Paste, Select All, Undo and Redo
/// are the toolkit's own actions, so the Edit menu serves a field and
/// the editor alike.
pub fn key_bindings() -> Vec<KeyBinding> {
    let c = Some(EDITOR_CONTEXT);
    vec![
        KeyBinding::new("enter", Enter, c),
        KeyBinding::new("shift-enter", NewLine, c),
        KeyBinding::new("tab", Tab, c),
        KeyBinding::new("shift-tab", Outdent, c),
        KeyBinding::new("backspace", Backspace, c),
        KeyBinding::new("shift-backspace", Backspace, c),
        KeyBinding::new("delete", Delete, c),
        KeyBinding::new("alt-backspace", DeleteWordBack, c),
        KeyBinding::new("secondary-backspace", DeleteLineBack, c),
        KeyBinding::new("left", Left, c),
        KeyBinding::new("right", Right, c),
        KeyBinding::new("up", Up, c),
        KeyBinding::new("down", Down, c),
        KeyBinding::new("shift-left", SelectLeft, c),
        KeyBinding::new("shift-right", SelectRight, c),
        KeyBinding::new("shift-up", SelectUp, c),
        KeyBinding::new("shift-down", SelectDown, c),
        KeyBinding::new("alt-left", WordLeft, c),
        KeyBinding::new("alt-right", WordRight, c),
        KeyBinding::new("alt-shift-left", SelectWordLeft, c),
        KeyBinding::new("alt-shift-right", SelectWordRight, c),
        KeyBinding::new("secondary-left", LineStart, c),
        KeyBinding::new("secondary-right", LineEnd, c),
        KeyBinding::new("home", LineStart, c),
        KeyBinding::new("end", LineEnd, c),
        KeyBinding::new("secondary-shift-left", SelectLineStart, c),
        KeyBinding::new("secondary-shift-right", SelectLineEnd, c),
        KeyBinding::new("shift-home", SelectLineStart, c),
        KeyBinding::new("shift-end", SelectLineEnd, c),
        KeyBinding::new("secondary-up", DocStart, c),
        KeyBinding::new("secondary-down", DocEnd, c),
        KeyBinding::new("secondary-shift-up", SelectDocStart, c),
        KeyBinding::new("secondary-shift-down", SelectDocEnd, c),
        KeyBinding::new("pageup", PageUp, c),
        KeyBinding::new("pagedown", PageDown, c),
        KeyBinding::new("secondary-b", Bold, c),
        KeyBinding::new("secondary-i", Italic, c),
        KeyBinding::new("secondary-enter", ToggleTask, c),
        KeyBinding::new("secondary-a", input::SelectAll, c),
        KeyBinding::new("secondary-c", input::Copy, c),
        KeyBinding::new("secondary-x", input::Cut, c),
        KeyBinding::new("secondary-v", input::Paste, c),
        KeyBinding::new("secondary-z", input::Undo, c),
        KeyBinding::new("secondary-shift-z", input::Redo, c),
    ]
}

/// How a note is shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    /// As it reads, with the marks of what the caret is in shown.
    #[default]
    Live,
    /// As it is written, every mark shown, the styles kept.
    Source,
    /// As it reads only, with nothing to type in.
    Reading,
}

impl Mode {
    pub fn key(self) -> &'static str {
        match self {
            Mode::Live => "live",
            Mode::Source => "source",
            Mode::Reading => "reading",
        }
    }

    pub fn from_key(key: &str) -> Mode {
        match key {
            "source" => Mode::Source,
            "reading" => Mode::Reading,
            _ => Mode::Live,
        }
    }
}

/// What the editor tells the view around it.
pub enum EditorEvent {
    /// The text changed.
    Changed,
    /// A link was clicked: an address, or a path relative to the note.
    OpenLink(String),
    /// A `[[note]]` link was clicked: the name it gives.
    OpenNote(String),
}

/// The faces and size a note is set in, from the settings.
#[derive(Clone, PartialEq)]
pub struct Look {
    pub text_family: SharedString,
    pub mono_family: SharedString,
    pub text_px: f32,
}

impl Default for Look {
    fn default() -> Self {
        Self { text_family: "Inter".into(), mono_family: "JetBrains Mono".into(), text_px: 16. }
    }
}

/// The words' line, as a share of their size.
const LINE: f32 = 1.6;
/// A heading's size as a share of the words', its weight, and its line.
const HEADINGS: [(f32, FontWeight, f32); 6] = [(1.618, FontWeight::BOLD, 1.25), (1.462, FontWeight::SEMIBOLD, 1.25), (1.318, FontWeight::SEMIBOLD, 1.3), (1.188, FontWeight::SEMIBOLD, 1.35), (1.076, FontWeight::SEMIBOLD, 1.4), (1.0, FontWeight::SEMIBOLD, 1.5)];
/// Code's size as a share of the words'.
const CODE: f32 = 0.9;
/// Each level of a list, and each level of a quote, stands this far in.
const LIST_INDENT: f32 = 26.;
const QUOTE_INDENT: f32 = 18.;
/// The room at a code plate's top and foot, which its fence lines take
/// while they are hidden, and at its sides.
const PLATE_PAD: f32 = 10.;
const PLATE_X: f32 = 12.;
const CELL_X: f32 = 10.;
const CELL_Y: f32 = 5.;
const BLINK: Duration = Duration::from_millis(530);
/// The caret is kept this far from the top and foot of what shows.
const SCROLL_MARGIN: f32 = 28.;

/// `ONYX_TRACE=1` prints what each layout and paint costs, for a probe.
fn tracing() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("ONYX_TRACE").is_some())
}

fn trace(what: &str, since: Instant) {
    if tracing() {
        eprintln!("trace {what} {:.2}ms", since.elapsed().as_secs_f64() * 1000.);
    }
}

/// A stamp on the app's own clock, for the gap between an event and the paint after it.
pub(crate) fn stamp(what: &str) {
    if tracing() {
        static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        let t = EPOCH.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.;
        eprintln!("stamp {t:.1} {what}");
    }
}

pub struct Editor {
    doc: Doc,
    sel: Sel,
    analysis: Analysis,
    mode: Mode,
    look: Look,
    focus: FocusHandle,
    /// The scroller the view around the editor puts it in.
    pub scroll: ScrollHandle,
    cache: Rc<RefCell<Cache>>,
    /// An input method's composition: what it has put in so far.
    marked: Option<(Pos, Pos)>,
    blink_at: Instant,
    blinking: bool,
    focused: bool,
    /// A drag under way: how the press began (a caret, a word, a line),
    /// and the range that press selected.
    drag: Option<(Grain, Pos, Pos)>,
    want_scroll: bool,
    /// The column ↑ and ↓ keep to, in pixels from the words' left.
    preferred_x: Option<f32>,
    /// Where the element was last painted, for an input method's windows.
    origin: Point<Pixels>,
    over_link: bool,
    read_only: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Grain {
    Char,
    Word,
    Line,
}

impl EventEmitter<EditorEvent> for Editor {}

impl Focusable for Editor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

// -- what is laid out -------------------------------------------------------

/// A line laid out at a width: the words that show, their layout, and
/// the widgets beside them.
struct TextRow {
    shown: Shown,
    source_len: usize,
    line: WrappedLine,
    line_h: f32,
    /// Where the words begin, from the element's left.
    inset: f32,
    top_pad: f32,
    widgets: Vec<Widget>,
    /// Which spans' words are links, by their shown range, and where
    /// each leads.
    links: Vec<(Range<usize>, Target)>,
    /// Shown ranges with a plate behind them (code), and with a wash (a highlight).
    chips: Vec<(Range<usize>, Hsla)>,
}

#[derive(Clone)]
enum Target {
    Link(String),
    Note(String),
}

enum Widget {
    /// A bullet's disc, centered here.
    Bullet { x: f32, y: f32, r: f32 },
    /// A number before its item.
    Number { line: ShapedLine, x: f32, y: f32 },
    /// A task's box.
    Task { bounds: Bounds<Pixels>, done: bool },
    /// An indent guide: a hairline at `x` down the whole row.
    Guide { x: f32 },
    /// A quote's bar at `x`.
    Bar { x: f32 },
}

struct TableRow {
    cols: Vec<f32>,
    heights: Vec<f32>,
    cells: Vec<Vec<Option<WrappedLine>>>,
    line_h: f32,
    inset: f32,
}

enum Row {
    Text(TextRow),
    /// A hidden fence line: the plate's padding, and the language on the first.
    Pad { lang: Option<ShapedLine> },
    /// A line with no height: a table's row drawn in its first, a hidden underline.
    Hidden,
    Rule { inset: f32, line_h: f32 },
    Table(TableRow),
}

struct Entry {
    key: u64,
    height: f32,
    row: Rc<Row>,
}

#[derive(Default)]
struct Cache {
    width: f32,
    look_key: u64,
    entries: Vec<Entry>,
    tops: Vec<f32>,
    total: f32,
}

/// A caret's place on the page, from the element's top left.
#[derive(Clone, Copy)]
struct CaretAt {
    x: f32,
    y: f32,
    h: f32,
}

// -- the editor -------------------------------------------------------------

impl Editor {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            doc: Doc::default(),
            sel: Sel::default(),
            analysis: markdown::analyze(""),
            mode: Mode::Live,
            look: Look::default(),
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            cache: Rc::new(RefCell::new(Cache::default())),
            marked: None,
            blink_at: Instant::now(),
            blinking: false,
            focused: false,
            drag: None,
            want_scroll: false,
            preferred_x: None,
            origin: Point::default(),
            over_link: false,
            read_only: false,
        }
    }

    /// Another note's text: the history, the selection and the scroll start over.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.doc = Doc::from_text(text);
        self.sel = Sel::default();
        self.marked = None;
        self.drag = None;
        self.preferred_x = None;
        self.scroll.set_offset(point(px(0.), px(0.)));
        self.reanalyze();
        cx.notify();
    }

    /// The text as the disk has it, when nothing was typed: the caret and
    /// the history are kept where the text still has them.
    pub fn replace_text_quietly(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.doc.text() == text {
            return;
        }
        let keep = self.sel;
        self.doc = Doc::from_text(text);
        self.sel = Sel { anchor: self.doc.clamp(keep.anchor), head: self.doc.clamp(keep.head) };
        self.marked = None;
        self.reanalyze();
        cx.notify();
    }

    pub fn text(&self) -> String {
        self.doc.text()
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        if self.mode != mode {
            self.mode = mode;
            self.marked = None;
            self.want_scroll = true;
            cx.notify();
        }
    }

    pub fn set_look(&mut self, look: Look, cx: &mut Context<Self>) {
        if self.look != look {
            self.look = look;
            cx.notify();
        }
    }

    /// For a probe: words typed as the input method would type them.
    pub fn type_text(&mut self, text: &str, cx: &mut Context<Self>) {
        for ch in text.chars() {
            let (a, b) = self.sel.range();
            let end = self.doc.replace(a, b, &ch.to_string(), self.sel);
            self.sel = Sel::caret(end);
            self.after_edit(cx);
        }
    }

    /// For a probe: the caret at the end, and the view with it.
    pub fn move_to_end(&mut self, cx: &mut Context<Self>) {
        let end = self.doc.end();
        self.move_to(end, false, cx);
    }

    pub fn selection_text(&self) -> String {
        let (a, b) = self.sel.range();
        self.doc.slice(a, b)
    }

    fn reanalyze(&mut self) {
        self.analysis = markdown::analyze(&self.doc.text());
    }

    /// After every change: the text read again, numbered lists put
    /// right, the caret shown, the view around told.
    fn after_edit(&mut self, cx: &mut Context<Self>) {
        stamp("after_edit");
        let started = Instant::now();
        self.reanalyze();
        self.renumber();
        trace("after_edit(analyze+renumber)", started);
        self.want_scroll = true;
        self.preferred_x = None;
        self.kick_blink(cx);
        cx.emit(EditorEvent::Changed);
        cx.notify();
    }

    fn after_move(&mut self, cx: &mut Context<Self>) {
        self.want_scroll = true;
        self.kick_blink(cx);
        cx.notify();
    }

    /// Numbered lists count from their first item, as Obsidian keeps
    /// them, whatever was typed; one step of undo with the change before it.
    fn renumber(&mut self) {
        let mut changes = Vec::new();
        for list in self.analysis.lists.iter().filter(|l| l.ordered) {
            let mut expected: Option<u64> = None;
            for &line in &list.items {
                let Some(mark) = self.analysis.lines[line].list.as_ref() else { continue };
                let ListKind::Ordered(n) = mark.kind else { continue };
                let want = expected.unwrap_or(n);
                if n != want {
                    let text = self.doc.line(line);
                    let digits = text[mark.marker.start..].chars().take_while(|c| c.is_ascii_digit()).count();
                    changes.push((Pos::new(line, mark.marker.start), Pos::new(line, mark.marker.start + digits), want.to_string()));
                }
                expected = Some(want + 1);
            }
        }
        if changes.is_empty() {
            return;
        }
        // The caret stays with its line, moved by what its own number grew.
        let mut sel = self.sel;
        for (from, to, text) in &changes {
            for p in [&mut sel.anchor, &mut sel.head] {
                if p.line == from.line && p.col >= to.col {
                    p.col = p.col + text.len() - (to.col - from.col);
                }
            }
        }
        let before = self.sel;
        self.doc.replace_many(changes, before, sel);
        self.sel = sel;
        self.reanalyze();
    }

    fn kick_blink(&mut self, cx: &mut Context<Self>) {
        self.blink_at = Instant::now();
        if self.blinking {
            return;
        }
        self.blinking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(BLINK).await;
                let go_on = this
                    .update(cx, |e, cx| {
                        cx.notify();
                        e.focused && e.mode != Mode::Reading
                    })
                    .unwrap_or(false);
                if !go_on {
                    let _ = this.update(cx, |e, _| e.blinking = false);
                    break;
                }
            }
        })
        .detach();
    }

    fn caret_shown(&self) -> bool {
        self.focused && ((self.blink_at.elapsed().as_millis() / BLINK.as_millis()) % 2 == 0)
    }

    // -- edits --------------------------------------------------------------

    /// `text` in place of the selection.
    fn insert(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        let (a, b) = self.sel.range();
        let end = self.doc.replace(a, b, text, self.sel);
        self.sel = Sel::caret(end);
        self.after_edit(cx);
    }

    fn replace_range(&mut self, a: Pos, b: Pos, text: &str, cx: &mut Context<Self>) -> Pos {
        let end = self.doc.replace(a, b, text, self.sel);
        self.sel = Sel::caret(end);
        self.after_edit(cx);
        end
    }

    fn enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if !self.sel.is_caret() {
            self.insert("\n", cx);
            return;
        }
        let at = self.sel.head;
        let text = self.doc.line(at.line).to_string();
        let (quote_depth, body_start) = quote_line(&text).unwrap_or((0, 0));
        let quote_prefix = "> ".repeat(quote_depth);
        if let Some(item) = list_line(&text[body_start..]) {
            let marker_end = body_start + item.indent.len() + item.marker.len();
            if at.col >= marker_end {
                if item.body.trim().is_empty() && at.col == text.len() {
                    // An empty item ends the list: its marker goes.
                    self.replace_range(Pos::new(at.line, 0), Pos::new(at.line, text.len()), &quote_prefix, cx);
                } else {
                    let line = format!("\n{quote_prefix}{}{}", item.indent, next_marker(&item));
                    self.insert(&line, cx);
                }
                self.doc.end_step();
                return;
            }
        } else if quote_depth > 0 && at.col >= body_start {
            if text[body_start..].trim().is_empty() && at.col == text.len() {
                self.replace_range(Pos::new(at.line, 0), Pos::new(at.line, text.len()), "", cx);
            } else {
                self.insert(&format!("\n{quote_prefix}"), cx);
            }
            self.doc.end_step();
            return;
        }
        self.insert("\n", cx);
        self.doc.end_step();
    }

    fn new_line(&mut self, _: &NewLine, _: &mut Window, cx: &mut Context<Self>) {
        self.insert("\n", cx);
        self.doc.end_step();
    }

    /// The lines the selection touches.
    fn selected_lines(&self) -> Range<usize> {
        let (a, b) = self.sel.range();
        a.line..b.line + 1
    }

    fn tab(&mut self, _: &Tab, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        let lines: Vec<usize> = self.selected_lines().filter(|l| list_line(self.doc.line(*l)).is_some()).collect();
        if lines.is_empty() {
            self.insert("\t", cx);
            return;
        }
        let before = self.sel;
        let mut sel = self.sel;
        for p in [&mut sel.anchor, &mut sel.head] {
            if lines.contains(&p.line) {
                p.col += 1;
            }
        }
        let changes = lines.iter().map(|&l| (Pos::new(l, 0), Pos::new(l, 0), "\t".to_string())).collect();
        self.doc.replace_many(changes, before, sel);
        self.sel = sel;
        self.after_edit(cx);
    }

    fn outdent(&mut self, _: &Outdent, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        let mut changes = Vec::new();
        let mut sel = self.sel;
        for l in self.selected_lines() {
            let text = self.doc.line(l);
            if list_line(text).is_none() {
                continue;
            }
            let cut = if text.starts_with('\t') { 1 } else { text.len() - text.trim_start_matches(' ').len().max(text.len().saturating_sub(4)).min(text.len()) };
            let cut = if text.starts_with('\t') { cut } else { (text.len() - text.trim_start_matches(' ').len()).min(4) };
            if cut == 0 {
                continue;
            }
            changes.push((Pos::new(l, 0), Pos::new(l, cut), String::new()));
            for p in [&mut sel.anchor, &mut sel.head] {
                if p.line == l {
                    p.col = p.col.saturating_sub(cut);
                }
            }
        }
        if changes.is_empty() {
            return;
        }
        let before = self.sel;
        self.doc.replace_many(changes, before, sel);
        self.sel = sel;
        self.after_edit(cx);
    }

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if !self.sel.is_caret() {
            self.insert("", cx);
            return;
        }
        let at = self.sel.head;
        let text = self.doc.line(at.line);
        // Right after a list marker with nothing typed yet: the marker goes.
        if let Some(item) = list_line(text) {
            let marker_end = item.indent.len() + item.marker.len();
            if at.col == marker_end && item.body.is_empty() {
                self.replace_range(Pos::new(at.line, item.indent.len()), Pos::new(at.line, marker_end), "", cx);
                self.doc.end_step();
                return;
            }
        }
        let from = self.doc.prev_grapheme(at);
        if from != at {
            self.replace_range(from, at, "", cx);
        }
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if !self.sel.is_caret() {
            self.insert("", cx);
            return;
        }
        let at = self.sel.head;
        let to = self.doc.next_grapheme(at);
        if to != at {
            self.replace_range(at, to, "", cx);
        }
    }

    fn delete_word_back(&mut self, _: &DeleteWordBack, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if !self.sel.is_caret() {
            self.insert("", cx);
            return;
        }
        let at = self.sel.head;
        let from = self.doc.word_left(at);
        self.replace_range(from, at, "", cx);
        self.doc.end_step();
    }

    fn delete_line_back(&mut self, _: &DeleteLineBack, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        let at = self.sel.head;
        let from = if at.col > 0 { Pos::new(at.line, 0) } else { self.doc.prev_grapheme(at) };
        if from != at {
            self.replace_range(from, at, "", cx);
            self.doc.end_step();
        }
    }

    fn select_all(&mut self, _: &input::SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.sel = Sel { anchor: Pos::default(), head: self.doc.end() };
        cx.notify();
    }

    fn copy(&mut self, _: &input::Copy, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.selection_text();
        if !text.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn cut(&mut self, _: &input::Cut, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.selection_text();
        if !text.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            if !self.read_only {
                self.insert("", cx);
                self.doc.end_step();
            }
        }
    }

    fn paste(&mut self, _: &input::Paste, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let text = text.replace("\r\n", "\n").replace('\r', "\n");
            self.insert(&text, cx);
            self.doc.end_step();
        }
    }

    fn undo(&mut self, _: &input::Undo, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if let Some(sel) = self.doc.undo() {
            self.sel = Sel { anchor: self.doc.clamp(sel.anchor), head: self.doc.clamp(sel.head) };
            self.reanalyze();
            self.want_scroll = true;
            self.kick_blink(cx);
            cx.emit(EditorEvent::Changed);
            cx.notify();
        }
    }

    fn redo(&mut self, _: &input::Redo, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if let Some(sel) = self.doc.redo() {
            self.sel = Sel { anchor: self.doc.clamp(sel.anchor), head: self.doc.clamp(sel.head) };
            self.reanalyze();
            self.want_scroll = true;
            self.kick_blink(cx);
            cx.emit(EditorEvent::Changed);
            cx.notify();
        }
    }

    /// ⌘B and ⌘I: a span of the kind around or under the caret is
    /// unwrapped; a selection is wrapped; a caret on a word wraps the
    /// word; a caret on nothing opens an empty pair to type into, and in
    /// such a pair closes it again.
    fn toggle_wrap(&mut self, marker: &str, kind: SpanKind, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        let (a, b) = self.sel.range();
        let (ga, gb) = (self.doc.offset_of(a), self.doc.offset_of(b));
        let around = self.analysis.spans.iter().filter(|s| s.kind == kind && s.range.start <= ga && gb <= s.range.end).min_by_key(|s| s.range.len()).cloned();
        if let Some(s) = around {
            let (open, close) = (self.doc.pos_of(s.open.start), self.doc.pos_of(s.close.start));
            let (open_end, close_end) = (self.doc.pos_of(s.open.end), self.doc.pos_of(s.close.end));
            let before = self.sel;
            let shift = |p: Pos| -> Pos {
                let mut p = p;
                if p.line == close.line && p.col >= close_end.col {
                    p.col -= close_end.col - close.col;
                }
                if p.line == open.line && p.col >= open_end.col {
                    p.col -= open_end.col - open.col;
                } else if p.line == open.line && p.col > open.col {
                    p.col = open.col;
                }
                p
            };
            let after = Sel { anchor: shift(before.anchor), head: shift(before.head) };
            self.doc.replace_many(vec![(open, open_end, String::new()), (close, close_end, String::new())], before, after);
            self.sel = after;
            self.after_edit(cx);
            return;
        }
        if a != b {
            let text = self.doc.slice(a, b);
            let lead = text.len() - text.trim_start().len();
            let trail = text.len() - text.trim_end().len();
            let (ta, tb) = (self.doc.pos_of(ga + lead), self.doc.pos_of(gb - trail));
            if ta >= tb {
                return;
            }
            let before = self.sel;
            self.doc.replace_many(vec![(ta, ta, marker.to_string()), (tb, tb, marker.to_string())], before, before);
            let inner_a = Pos::new(ta.line, ta.col + marker.len());
            let inner_b = if tb.line == ta.line { Pos::new(tb.line, tb.col + marker.len()) } else { tb };
            self.sel = Sel { anchor: inner_a, head: inner_b };
            self.after_edit(cx);
            return;
        }
        let word = self.doc.word_at(a);
        if word.start < word.end {
            let (wa, wb) = (Pos::new(a.line, word.start), Pos::new(a.line, word.end));
            let before = self.sel;
            let caret = Pos::new(a.line, a.col + marker.len());
            self.doc.replace_many(vec![(wa, wa, marker.to_string()), (wb, wb, marker.to_string())], before, Sel::caret(caret));
            self.sel = Sel::caret(caret);
            self.after_edit(cx);
            return;
        }
        let line = self.doc.line(a.line);
        if line[..a.col].ends_with(marker) && line[a.col..].starts_with(marker) {
            let from = Pos::new(a.line, a.col - marker.len());
            let to = Pos::new(a.line, a.col + marker.len());
            self.replace_range(from, to, "", cx);
            return;
        }
        let caret = Pos::new(a.line, a.col + marker.len());
        self.doc.replace(a, a, &format!("{marker}{marker}"), self.sel);
        self.sel = Sel::caret(caret);
        self.after_edit(cx);
    }

    fn bold(&mut self, _: &Bold, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_wrap("**", SpanKind::Strong, cx);
    }

    fn italic(&mut self, _: &Italic, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_wrap("*", SpanKind::Em, cx);
    }

    /// ⌘↩: a task's box ticked or cleared; a bullet made a task; a plain
    /// line made a task.
    fn toggle_task(&mut self, _: &ToggleTask, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        let line = self.sel.head.line;
        self.toggle_task_on(line, cx);
    }

    fn toggle_task_on(&mut self, line: usize, cx: &mut Context<Self>) {
        let text = self.doc.line(line).to_string();
        let (_, body_start) = quote_line(&text).unwrap_or((0, 0));
        let before = self.sel;
        match list_line(&text[body_start..]) {
            Some(item) => {
                let start = body_start + item.indent.len();
                match item.kind {
                    ListKind::Task(done) => {
                        let bx = start + 2;
                        let new = if done { "[ ]" } else { "[x]" };
                        self.doc.replace_many(vec![(Pos::new(line, bx), Pos::new(line, bx + 3), new.to_string())], before, before);
                    }
                    ListKind::Bullet => {
                        let at = Pos::new(line, start + item.marker.len());
                        let mut after = before;
                        for p in [&mut after.anchor, &mut after.head] {
                            if p.line == line && p.col >= at.col {
                                p.col += 4;
                            }
                        }
                        self.doc.replace_many(vec![(at, at, "[ ] ".to_string())], before, after);
                        self.sel = after;
                    }
                    ListKind::Ordered(_) => return,
                }
            }
            None => {
                let at = Pos::new(line, body_start);
                let mut after = before;
                for p in [&mut after.anchor, &mut after.head] {
                    if p.line == line && p.col >= at.col {
                        p.col += 6;
                    }
                }
                self.doc.replace_many(vec![(at, at, "- [ ] ".to_string())], before, after);
                self.sel = after;
            }
        }
        self.after_edit(cx);
    }

    // -- moves ----------------------------------------------------------------

    fn move_to(&mut self, p: Pos, select: bool, cx: &mut Context<Self>) {
        let p = self.doc.clamp(p);
        self.sel = if select { Sel { anchor: self.sel.anchor, head: p } } else { Sel::caret(p) };
        self.after_move(cx);
    }

    fn left(&mut self, select: bool, cx: &mut Context<Self>) {
        stamp("key left");
        let p = if !select && !self.sel.is_caret() { self.sel.start() } else { self.doc.prev_grapheme(self.sel.head) };
        self.preferred_x = None;
        self.move_to(p, select, cx);
    }

    fn right(&mut self, select: bool, cx: &mut Context<Self>) {
        let p = if !select && !self.sel.is_caret() { self.sel.end() } else { self.doc.next_grapheme(self.sel.head) };
        self.preferred_x = None;
        self.move_to(p, select, cx);
    }

    fn word_left(&mut self, select: bool, cx: &mut Context<Self>) {
        let p = self.doc.word_left(self.sel.head);
        self.preferred_x = None;
        self.move_to(p, select, cx);
    }

    fn word_right(&mut self, select: bool, cx: &mut Context<Self>) {
        let p = self.doc.word_right(self.sel.head);
        self.preferred_x = None;
        self.move_to(p, select, cx);
    }

    /// Home: the start of the words, before the line's marks; pressed
    /// there, the line's very start.
    fn line_start(&mut self, select: bool, cx: &mut Context<Self>) {
        let at = self.sel.head;
        let text = self.doc.line(at.line);
        let (_, qs) = quote_line(text).unwrap_or((0, 0));
        let content = list_line(&text[qs..]).map_or(qs, |i| qs + i.indent.len() + i.marker.len());
        let p = if at.col > content { content } else { 0 };
        self.preferred_x = None;
        self.move_to(Pos::new(at.line, p), select, cx);
    }

    fn line_end(&mut self, select: bool, cx: &mut Context<Self>) {
        let at = self.sel.head;
        self.preferred_x = None;
        self.move_to(Pos::new(at.line, self.doc.line(at.line).len()), select, cx);
    }

    /// ↑ and ↓: a wrapped row up or down in the same line, else the line
    /// before or after, at the column the first press began in.
    fn vertical(&mut self, down: bool, select: bool, cx: &mut Context<Self>) {
        let head = self.sel.head;
        let Some(at) = self.caret_at(head) else { return };
        let x = *self.preferred_x.get_or_insert(at.x);
        let cache = self.cache.borrow();
        let Some(entry) = cache.entries.get(head.line) else { return };
        let target_y = if down { at.y + at.h + 1. } else { at.y - 1. };
        let in_row = target_y >= cache.tops[head.line] && target_y < cache.tops[head.line] + entry.height;
        let target = if in_row {
            self.pos_at(x, target_y, &cache)
        } else {
            let line = if down { head.line + 1 } else { head.line.checked_sub(1).unwrap_or(0) };
            if down && line >= self.doc.line_count() {
                Some(self.doc.end())
            } else if !down && head.line == 0 {
                Some(Pos::default())
            } else {
                let y = if down { cache.tops[line] + 1. } else { cache.tops[line] + cache.entries[line].height - 1. };
                self.pos_at(x, y.max(0.), &cache)
            }
        };
        drop(cache);
        if let Some(p) = target {
            self.move_to(p, select, cx);
        }
    }

    fn page(&mut self, down: bool, cx: &mut Context<Self>) {
        let head = self.sel.head;
        let Some(at) = self.caret_at(head) else { return };
        let view = f32::from(self.scroll.bounds().size.height).max(100.);
        let x = *self.preferred_x.get_or_insert(at.x);
        let y = if down { at.y + view } else { at.y - view };
        let target = { self.pos_at(x, y.max(0.), &self.cache.borrow()) };
        if let Some(p) = target {
            self.move_to(p, false, cx);
        }
    }

    // -- the layout ---------------------------------------------------------

    fn look_key(&self, theme: &gpui_component::Theme) -> u64 {
        let mut h = DefaultHasher::new();
        self.look.text_family.hash(&mut h);
        self.look.mono_family.hash(&mut h);
        self.look.text_px.to_bits().hash(&mut h);
        theme.mode.is_dark().hash(&mut h);
        (self.mode as u8).hash(&mut h);
        h.finish()
    }

    /// Whether the selection touches a line.
    fn on_line(&self, line: usize) -> bool {
        let (a, b) = self.sel.range();
        a.line <= line && line <= b.line
    }

    fn touches(&self, r: &Range<usize>, inclusive: bool) -> bool {
        let (a, b) = self.sel.range();
        let (ga, gb) = (self.doc.offset_of(a), self.doc.offset_of(b));
        if inclusive { ga <= r.end && gb >= r.start } else { ga < r.end && gb > r.start }
    }

    /// The marks hidden on a line, in the line's bytes, by the mode and
    /// where the caret is.
    fn hidden_on(&self, line: usize) -> Vec<Range<usize>> {
        let info = &self.analysis.lines[line];
        let on_line = self.on_line(line);
        let source = self.mode == Mode::Source;
        let start = self.analysis.line_starts[line];
        let mut hidden = Vec::new();
        if !source && !on_line {
            hidden.extend(info.marks.iter().cloned());
        }
        if !source {
            if let Some(m) = &info.list {
                hidden.push(0..m.marker.end);
            } else if info.quote_depth > 0 && !on_line {
                // The quote's marks are in `marks`; the room before them goes too.
                if let Some(first) = info.marks.first() {
                    hidden.push(0..first.start);
                }
            }
        }
        if !source {
            for &ix in &self.analysis.line_spans[line] {
                let s = &self.analysis.spans[ix];
                let inclusive = !matches!(s.kind, SpanKind::Link(_) | SpanKind::WikiLink(_) | SpanKind::Image(_));
                if self.touches(&s.range, inclusive) {
                    continue;
                }
                for r in [&s.open, &s.close] {
                    let (a, b) = (r.start.max(start), r.end.min(start + self.doc.line(line).len()));
                    if a < b {
                        hidden.push(a - start..b - start);
                    }
                }
            }
        }
        hidden
    }

    fn block_revealed(&self, first: usize, last: usize) -> bool {
        let (a, b) = self.sel.range();
        self.mode == Mode::Source || (a.line <= last && b.line >= first)
    }

    /// The whole note laid out at `width`, line by line, each only when
    /// its key changed. The tops and the total follow.
    fn relayout(&mut self, width: f32, window: &mut Window, cx: &mut App) {
        let theme = cx.theme().clone();
        let look_key = self.look_key(&theme);
        let n = self.doc.line_count();
        let mut cache = self.cache.borrow_mut();
        if cache.width != width || cache.look_key != look_key {
            cache.entries.clear();
            cache.width = width;
            cache.look_key = look_key;
        }
        cache.entries.truncate(n);
        let mut tops = Vec::with_capacity(n);
        let mut y = 0.;
        for line in 0..n {
            let hidden = self.hidden_on(line);
            let text = self.doc.line(line);
            let info = &self.analysis.lines[line];
            let mut h = DefaultHasher::new();
            text.hash(&mut h);
            hidden.hash(&mut h);
            info.kind.hash_key(&mut h);
            info.task_done.hash(&mut h);
            info.list_depth.hash(&mut h);
            info.quote_depth.hash(&mut h);
            let revealed = match &info.kind {
                LineKind::FenceOpen(_) | LineKind::FenceClose | LineKind::Code => self.analysis.code_block_at(line).map_or(true, |b| self.block_revealed(b.first, b.last)),
                LineKind::TableFirst(t) | LineKind::TableRow(t) => {
                    let t = &self.analysis.tables[*t];
                    t.rows.iter().for_each(|r| r.hash(&mut h));
                    self.block_revealed(t.first, t.last)
                }
                _ => self.on_line(line),
            };
            revealed.hash(&mut h);
            let prev_blank = line > 0 && matches!(self.analysis.lines[line - 1].kind, LineKind::Blank);
            prev_blank.hash(&mut h);
            let key = h.finish();
            let fresh = cache.entries.get(line).is_none_or(|e| e.key != key);
            if fresh {
                let row = self.lay_out(line, &hidden, revealed, prev_blank, width, &theme, window);
                let height = row_height(&row);
                let entry = Entry { key, height, row: Rc::new(row) };
                if line < cache.entries.len() {
                    cache.entries[line] = entry;
                } else {
                    cache.entries.push(entry);
                }
            }
            tops.push(y);
            y += cache.entries[line].height;
        }
        cache.tops = tops;
        cache.total = y;
    }

    fn font(&self, family: &SharedString, weight: FontWeight, italic: bool) -> Font {
        Font { family: family.clone(), features: FontFeatures::default(), fallbacks: None, weight, style: if italic { FontStyle::Italic } else { FontStyle::Normal } }
    }

    /// One line laid out: its words with the hidden marks taken out, in
    /// runs of the faces and inks its spans ask for, and its widgets.
    fn lay_out(&self, line: usize, hidden: &[Range<usize>], revealed: bool, prev_blank: bool, width: f32, theme: &gpui_component::Theme, window: &mut Window) -> Row {
        let info = &self.analysis.lines[line];
        let text = self.doc.line(line);
        let px_text = self.look.text_px;
        let dark = theme.mode.is_dark();
        let muted = theme.muted_foreground;
        let ink = theme.foreground;
        let link = theme.link;
        let source = self.mode == Mode::Source;

        // Block widgets, when the caret is away.
        match &info.kind {
            LineKind::FenceOpen(lang) if !revealed => {
                let lang = (!lang.is_empty()).then(|| {
                    let font = self.font(&self.look.mono_family, FontWeight::NORMAL, false);
                    let size = px(px_text * 0.75);
                    let run = TextRun { len: lang.len(), font, color: muted, background_color: None, underline: None, strikethrough: None };
                    window.text_system().shape_line(SharedString::from(lang.clone()), size, &[run], None)
                });
                return Row::Pad { lang };
            }
            LineKind::FenceClose if !revealed => return Row::Pad { lang: None },
            LineKind::SetextUnderline if !revealed => return Row::Hidden,
            LineKind::TableRow(_) if !revealed => return Row::Hidden,
            LineKind::TableFirst(t) if !revealed => return self.lay_out_table(*t, width, theme, window),
            LineKind::Rule if !revealed => {
                let inset = self.inset_of(info);
                return Row::Rule { inset, line_h: px_text * LINE };
            }
            _ => {}
        }

        // The size, weight and line of the words.
        let (font_px, base_weight, line_factor, top_pad) = match &info.kind {
            LineKind::Heading(level) => {
                let (scale, weight, lh) = HEADINGS[(*level as usize).clamp(1, 6) - 1];
                (px_text * scale, weight, lh, if prev_blank { 0. } else { px_text * 0.35 })
            }
            LineKind::Code | LineKind::FrontMatter | LineKind::FrontMatterEdge | LineKind::Html | LineKind::FenceOpen(_) | LineKind::FenceClose => (px_text * CODE, FontWeight::NORMAL, 1.5, 0.),
            LineKind::TableFirst(_) | LineKind::TableRow(_) => (px_text * CODE, FontWeight::NORMAL, 1.5, 0.),
            _ => (px_text, FontWeight::NORMAL, LINE, 0.),
        };
        let line_h = (font_px * line_factor).round();
        let mono_line = matches!(info.kind, LineKind::Code | LineKind::FrontMatter | LineKind::FrontMatterEdge | LineKind::Html | LineKind::FenceOpen(_) | LineKind::FenceClose | LineKind::TableFirst(_) | LineKind::TableRow(_));
        let muted_line = matches!(info.kind, LineKind::FrontMatter | LineKind::FrontMatterEdge | LineKind::Html | LineKind::FenceOpen(_) | LineKind::FenceClose | LineKind::SetextUnderline);

        // A style a byte of the source.
        #[derive(Clone, Copy, PartialEq)]
        struct St {
            weight: FontWeight,
            italic: bool,
            strike: bool,
            mono: bool,
            color: Hsla,
            chip: Option<Hsla>,
            underline: bool,
        }
        let base = St { weight: base_weight, italic: false, strike: info.task_done, mono: mono_line, color: if muted_line || info.task_done { muted } else { ink }, chip: None, underline: false };
        let mut styles = vec![base; text.len()];
        let paint = |styles: &mut Vec<St>, r: Range<usize>, f: &dyn Fn(&mut St)| {
            for s in styles.iter_mut().take(r.end.min(text.len())).skip(r.start.min(text.len())) {
                f(s);
            }
        };
        // Marks shown are quiet.
        for m in &info.marks {
            paint(&mut styles, m.clone(), &|s| {
                s.color = muted;
                s.weight = FontWeight::NORMAL;
            });
        }
        if let Some(m) = &info.list {
            paint(&mut styles, 0..m.marker.end, &|s| {
                s.color = muted;
                s.strike = false;
            });
        }
        let start = self.analysis.line_starts[line];
        let local = |r: &Range<usize>| r.start.saturating_sub(start).min(text.len())..r.end.saturating_sub(start).min(text.len());
        let chip_bg = theme.primary.opacity(if dark { 0.16 } else { 0.10 });
        let highlight_bg = theme.warning.opacity(if dark { 0.35 } else { 0.4 });
        let mut links: Vec<(Range<usize>, Target)> = Vec::new();
        let mut spans: Vec<&markdown::Span> = self.analysis.line_spans[line].iter().map(|&ix| &self.analysis.spans[ix]).collect();
        // The outer first, so an inner span paints over it.
        spans.sort_by_key(|s| std::cmp::Reverse(s.range.len()));
        for s in spans {
            let inner = local(&s.inner());
            let (open, close) = (local(&s.open), local(&s.close));
            let marks_muted = |st: &mut St| {
                st.color = muted;
                st.weight = base_weight;
                st.italic = false;
            };
            match &s.kind {
                SpanKind::Strong => paint(&mut styles, inner.clone(), &|st| st.weight = FontWeight::BOLD),
                SpanKind::Em => paint(&mut styles, inner.clone(), &|st| st.italic = true),
                SpanKind::Strike => paint(&mut styles, inner.clone(), &|st| {
                    st.strike = true;
                    st.color = muted;
                }),
                SpanKind::Code => paint(&mut styles, inner.clone(), &|st| {
                    st.mono = true;
                    st.color = link;
                    st.chip = Some(chip_bg);
                }),
                SpanKind::Math => paint(&mut styles, inner.clone(), &|st| {
                    st.mono = true;
                    st.color = link;
                }),
                SpanKind::Highlight => paint(&mut styles, inner.clone(), &|st| st.chip = Some(highlight_bg)),
                SpanKind::Link(url) => {
                    paint(&mut styles, inner.clone(), &|st| {
                        st.color = link;
                        st.underline = true;
                    });
                    links.push((inner.clone(), Target::Link(url.clone())));
                }
                SpanKind::WikiLink(name) => {
                    paint(&mut styles, inner.clone(), &|st| st.color = link);
                    links.push((inner.clone(), Target::Note(name.clone())));
                }
                SpanKind::Image(src) => {
                    paint(&mut styles, inner.clone(), &|st| {
                        st.color = link;
                        st.italic = true;
                    });
                    links.push((inner.clone(), Target::Link(src.clone())));
                }
                SpanKind::Footnote => paint(&mut styles, local(&s.range), &|st| st.color = link),
                SpanKind::Html => paint(&mut styles, local(&s.range), &|st| {
                    st.mono = true;
                    st.color = muted;
                }),
            }
            paint(&mut styles, open, &marks_muted);
            paint(&mut styles, close, &marks_muted);
        }

        let shown = Shown::new(text, if source { &[] } else { hidden });
        // The runs over the shown text, one a change of style.
        let mut runs: Vec<TextRun> = Vec::new();
        let mut chips: Vec<(Range<usize>, Hsla)> = Vec::new();
        let mut shown_links: Vec<(Range<usize>, Target)> = Vec::new();
        for &(d, s, len) in &shown.pieces {
            let mut at = s;
            while at < s + len {
                let st = styles[at];
                let mut end = at + 1;
                while end < s + len && styles[end] == st {
                    end += 1;
                }
                let family = if st.mono { &self.look.mono_family } else { &self.look.text_family };
                runs.push(TextRun {
                    len: end - at,
                    font: self.font(family, st.weight, st.italic),
                    color: st.color,
                    background_color: None,
                    underline: st.underline.then(|| UnderlineStyle { thickness: px(1.), color: Some(link.opacity(0.5)), wavy: false }),
                    strikethrough: st.strike.then(|| StrikethroughStyle { thickness: px(1.), color: Some(st.color) }),
                });
                if let Some(c) = st.chip {
                    let dr = d + (at - s)..d + (end - s);
                    match chips.last_mut() {
                        Some((r, col)) if r.end == dr.start && *col == c => r.end = dr.end,
                        _ => chips.push((dr, c)),
                    }
                }
                at = end;
            }
        }
        for (r, t) in links {
            let (a, b) = (shown.to_shown(r.start), shown.to_shown(r.end));
            if a < b {
                shown_links.push((a..b, t));
            }
        }
        let inset = self.inset_of(info);
        let wrap = (width - inset - if matches!(info.kind, LineKind::Code) { PLATE_X } else { 0. }).max(40.);
        let shaped = SharedString::from(shown.text.clone());
        let mut lines = window.text_system().shape_text(shaped, px(font_px), &runs, Some(px(wrap)), None).unwrap_or_default();
        let wrapped = lines.pop().unwrap_or_else(|| window.text_system().shape_text(" ".into(), px(font_px), &[TextRun { len: 1, font: self.font(&self.look.text_family, FontWeight::NORMAL, false), color: ink, background_color: None, underline: None, strikethrough: None }], None, None).unwrap().pop().unwrap());

        // The widgets beside the words.
        let mut widgets = Vec::new();
        if !source {
            if let Some(m) = &info.list {
                let depth = m.depth as f32;
                let x_mark = inset - LIST_INDENT * 0.5;
                match m.kind {
                    ListKind::Bullet => widgets.push(Widget::Bullet { x: x_mark, y: top_pad + line_h / 2., r: (px_text * 0.17).max(2.5) }),
                    ListKind::Ordered(_) => {
                        let number = text[m.marker.start..m.marker.end].trim_end().to_string();
                        let run = TextRun { len: number.len(), font: self.font(&self.look.text_family, FontWeight::NORMAL, false), color: muted, background_color: None, underline: None, strikethrough: None };
                        let shaped = window.text_system().shape_line(number.into(), px(px_text), &[run], None);
                        let x = inset - 6. - f32::from(shaped.width());
                        widgets.push(Widget::Number { line: shaped, x, y: top_pad });
                    }
                    ListKind::Task(done) => {
                        let size = (px_text * 0.95).round();
                        let bounds = Bounds::new(point(px(x_mark - size / 2.), px(top_pad + (line_h - size) / 2.)), size_px(size));
                        widgets.push(Widget::Task { bounds, done });
                    }
                }
                for d in 0..depth as usize {
                    widgets.push(Widget::Guide { x: self.base_inset(info) + LIST_INDENT * d as f32 + LIST_INDENT * 0.5 });
                }
            } else if info.list_depth > 0 && info.kind != LineKind::Blank {
                for d in 0..info.list_depth {
                    widgets.push(Widget::Guide { x: self.base_inset(info) + LIST_INDENT * d as f32 + LIST_INDENT * 0.5 });
                }
            }
            for d in 0..info.quote_depth {
                widgets.push(Widget::Bar { x: QUOTE_INDENT * d as f32 + 2. });
            }
        }
        Row::Text(TextRow { shown, source_len: text.len(), line: wrapped, line_h, inset, top_pad, widgets, links: shown_links, chips })
    }

    /// Where a line's words begin: in by its quotes and its list levels.
    fn base_inset(&self, info: &markdown::LineInfo) -> f32 {
        if self.mode == Mode::Source { 0. } else { QUOTE_INDENT * info.quote_depth as f32 }
    }

    fn inset_of(&self, info: &markdown::LineInfo) -> f32 {
        if self.mode == Mode::Source {
            return 0.;
        }
        let base = self.base_inset(info);
        match &info.list {
            Some(m) => base + LIST_INDENT * (m.depth as f32 + 1.),
            None if info.list_depth > 0 => base + LIST_INDENT * info.list_depth as f32,
            None => base + if matches!(info.kind, LineKind::Code) { PLATE_X } else { 0. },
        }
    }

    /// A table drawn as a grid: each column as wide as its longest cell
    /// wants, shared down when the table is wider than the room.
    fn lay_out_table(&self, t: usize, width: f32, theme: &gpui_component::Theme, window: &mut Window) -> Row {
        let table = &self.analysis.tables[t];
        let info = &self.analysis.lines[table.first];
        let inset = self.base_inset(info);
        let px_cell = self.look.text_px * 0.95;
        let line_h = (px_cell * 1.5).round();
        let ncols = table.rows.iter().map(Vec::len).max().unwrap_or(1).max(table.aligns.len()).max(1);
        let avail = (width - inset).max(80.);
        let runs_for = |text: &str, head: bool| vec![TextRun { len: text.len(), font: self.font(&self.look.text_family, if head { FontWeight::SEMIBOLD } else { FontWeight::NORMAL }, false), color: theme.foreground, background_color: None, underline: None, strikethrough: None }];
        let mut natural = vec![0f32; ncols];
        for (ri, row) in table.rows.iter().enumerate() {
            for (ci, cell) in row.iter().enumerate().take(ncols) {
                if cell.is_empty() {
                    continue;
                }
                let shaped = window.text_system().shape_line(SharedString::from(cell.clone()), px(px_cell), &runs_for(cell, ri == 0), None);
                natural[ci] = natural[ci].max(f32::from(shaped.width()) + CELL_X * 2.);
            }
        }
        let mut cols: Vec<f32> = natural.iter().map(|w| w.max(40.)).collect();
        let sum: f32 = cols.iter().sum();
        if sum > avail {
            let k = avail / sum;
            for c in cols.iter_mut() {
                *c = (*c * k).max(40.);
            }
        }
        let mut cells = Vec::new();
        let mut heights = Vec::new();
        for (ri, row) in table.rows.iter().enumerate() {
            let mut shaped_row = Vec::new();
            let mut tall = 1usize;
            for ci in 0..ncols {
                let cell = row.get(ci).map(String::as_str).unwrap_or("");
                if cell.is_empty() {
                    shaped_row.push(None);
                    continue;
                }
                let wrapped = window.text_system().shape_text(SharedString::from(cell.to_string()), px(px_cell), &runs_for(cell, ri == 0), Some(px((cols[ci] - CELL_X * 2.).max(10.))), None).ok().and_then(|mut v| v.pop());
                if let Some(w) = &wrapped {
                    tall = tall.max(w.wrap_boundaries().len() + 1);
                }
                shaped_row.push(wrapped);
            }
            cells.push(shaped_row);
            heights.push(tall as f32 * line_h + CELL_Y * 2.);
        }
        Row::Table(TableRow { cols, heights, cells, line_h, inset })
    }

    // -- places on the page -------------------------------------------------

    /// Where the caret at `p` stands, from the element's top left.
    fn caret_at(&self, p: Pos) -> Option<CaretAt> {
        let cache = self.cache.borrow();
        let entry = cache.entries.get(p.line)?;
        let top = cache.tops[p.line];
        Some(match &*entry.row {
            Row::Text(r) => {
                let shown_ix = r.shown.to_shown(p.col);
                let at = r.line.position_for_index(shown_ix, px(r.line_h)).unwrap_or_default();
                CaretAt { x: r.inset + f32::from(at.x), y: top + r.top_pad + f32::from(at.y), h: r.line_h }
            }
            Row::Rule { inset, line_h } => CaretAt { x: *inset, y: top, h: *line_h },
            _ => CaretAt { x: 0., y: top, h: entry.height.max(self.look.text_px * LINE) },
        })
    }

    /// The place in the text at a point of the page, from the element's
    /// top left: the row under `y`, the character nearest `x` in it.
    fn pos_at(&self, x: f32, y: f32, cache: &Cache) -> Option<Pos> {
        if cache.entries.is_empty() {
            return None;
        }
        let line = match cache.tops.binary_search_by(|t| t.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal)) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        // A row of no height is passed by for the one that has it.
        let mut line = line.min(cache.entries.len() - 1);
        while cache.entries[line].height <= 0. && line > 0 && cache.tops[line] > y {
            line -= 1;
        }
        let entry = &cache.entries[line];
        let text_len = self.doc.line(line).len();
        Some(match &*entry.row {
            Row::Text(r) => {
                let local = point(px(x - r.inset), px((y - cache.tops[line] - r.top_pad).max(0.)));
                let ix = match r.line.closest_index_for_position(local, px(r.line_h)) {
                    Ok(i) | Err(i) => i,
                };
                let ix = ix.min(r.shown.text.len());
                let mut col = r.shown.to_source(ix, text_len);
                // A click at a hidden span's right edge stands past the span.
                let start = self.analysis.line_starts[line];
                for &six in &self.analysis.line_spans[line] {
                    let s = &self.analysis.spans[six];
                    if s.close.start >= start && s.close.start - start == col && s.close.end > s.close.start && r.shown.is_hidden(col) {
                        col = s.close.end - start;
                    }
                }
                Pos::new(line, col.min(text_len))
            }
            _ => Pos::new(line, 0),
        })
    }

    /// What a press lands on besides words: a task's box, a link.
    fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        let cache = self.cache.borrow();
        let line = match cache.tops.binary_search_by(|t| t.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal)) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let entry = cache.entries.get(line)?;
        let top = cache.tops[line];
        let Row::Text(r) = &*entry.row else { return None };
        for w in &r.widgets {
            if let Widget::Task { bounds, .. } = w {
                let b = Bounds::new(point(bounds.origin.x - px(3.), bounds.origin.y + px(top) - px(3.)), bounds.size + size(px(6.), px(6.)));
                if b.contains(&point(px(x), px(y))) {
                    return Some(Hit::Task(line));
                }
            }
        }
        if !r.links.is_empty() {
            let local = point(px(x - r.inset), px((y - top - r.top_pad).max(0.)));
            if let Ok(ix) = r.line.index_for_position(local, px(r.line_h)) {
                for (range, target) in &r.links {
                    if range.contains(&ix) {
                        return Some(Hit::Link(target.clone()));
                    }
                }
            }
        }
        None
    }

    /// Whether the scroller was moved.
    fn scroll_to_caret(&mut self, bounds: Bounds<Pixels>, window: &Window) -> bool {
        self.want_scroll = false;
        let Some(at) = self.caret_at(self.sel.head) else { return false };
        let view = window.content_mask().bounds;
        let caret_top = f32::from(bounds.top()) + at.y;
        let caret_bottom = caret_top + at.h;
        let (vt, vb) = (f32::from(view.top()), f32::from(view.bottom()));
        let offset = self.scroll.offset();
        let mut dy = 0.;
        if caret_top < vt + SCROLL_MARGIN {
            dy = vt + SCROLL_MARGIN - caret_top;
        } else if caret_bottom > vb - SCROLL_MARGIN {
            dy = (vb - SCROLL_MARGIN) - caret_bottom;
        }
        if dy.abs() > 0.5 {
            let max = f32::from(self.scroll.max_offset().y);
            let y = (f32::from(offset.y) + dy).clamp(-max.max(0.), 0.);
            self.scroll.set_offset(point(offset.x, px(y)));
            return true;
        }
        false
    }
}

enum Hit {
    Task(usize),
    Link(Target),
}

fn size_px(s: f32) -> Size<Pixels> {
    size(px(s), px(s))
}

fn row_height(row: &Row) -> f32 {
    match row {
        Row::Text(r) => r.top_pad + (r.line.wrap_boundaries().len() as f32 + 1.) * r.line_h,
        Row::Pad { .. } => PLATE_PAD,
        Row::Hidden => 0.,
        Row::Rule { line_h, .. } => *line_h,
        Row::Table(t) => t.heights.iter().sum::<f32>() + 1.,
    }
}

trait HashKey {
    fn hash_key(&self, h: &mut DefaultHasher);
}

impl HashKey for LineKind {
    fn hash_key(&self, h: &mut DefaultHasher) {
        match self {
            LineKind::Text => 0u8.hash(h),
            LineKind::Blank => 1u8.hash(h),
            LineKind::Heading(l) => (2u8, l).hash(h),
            LineKind::SetextUnderline => 3u8.hash(h),
            LineKind::FenceOpen(l) => (4u8, l).hash(h),
            LineKind::FenceClose => 5u8.hash(h),
            LineKind::Code => 6u8.hash(h),
            LineKind::FrontMatterEdge => 7u8.hash(h),
            LineKind::FrontMatter => 8u8.hash(h),
            LineKind::Rule => 9u8.hash(h),
            LineKind::TableFirst(t) => (10u8, t).hash(h),
            LineKind::TableRow(t) => (11u8, t).hash(h),
            LineKind::Html => 12u8.hash(h),
        }
    }
}

// -- the input method -------------------------------------------------------

impl EntityInputHandler for Editor {
    fn text_for_range(&mut self, range_utf16: Range<usize>, adjusted_range: &mut Option<Range<usize>>, _: &mut Window, _: &mut Context<Self>) -> Option<String> {
        let a = self.doc.pos_of_utf16(range_utf16.start);
        let b = self.doc.pos_of_utf16(range_utf16.end);
        *adjusted_range = Some(self.doc.utf16_of(a)..self.doc.utf16_of(b));
        Some(self.doc.slice(a, b))
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        let (a, b) = self.sel.range();
        Some(UTF16Selection { range: self.doc.utf16_of(a)..self.doc.utf16_of(b), reversed: self.sel.head < self.sel.anchor })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.map(|(a, b)| self.doc.utf16_of(a)..self.doc.utf16_of(b))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(&mut self, range_utf16: Option<Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        stamp("key typed");
        if self.read_only || self.mode == Mode::Reading {
            return;
        }
        let (a, b) = match range_utf16 {
            Some(r) => (self.doc.pos_of_utf16(r.start), self.doc.pos_of_utf16(r.end)),
            None => match self.marked {
                Some((a, b)) => (a, b),
                None => self.sel.range(),
            },
        };
        self.marked = None;
        let end = self.doc.replace(a, b, text, self.sel);
        self.sel = Sel::caret(end);
        self.after_edit(cx);
    }

    fn replace_and_mark_text_in_range(&mut self, range_utf16: Option<Range<usize>>, new_text: &str, new_selected_range: Option<Range<usize>>, _: &mut Window, cx: &mut Context<Self>) {
        if self.read_only || self.mode == Mode::Reading {
            return;
        }
        let (a, b) = match range_utf16 {
            Some(r) => (self.doc.pos_of_utf16(r.start), self.doc.pos_of_utf16(r.end)),
            None => match self.marked {
                Some((a, b)) => (a, b),
                None => self.sel.range(),
            },
        };
        let end = self.doc.replace(a, b, new_text, self.sel);
        let start_u = self.doc.utf16_of(a);
        self.marked = (a != end).then_some((a, end));
        self.sel = match new_selected_range {
            Some(r) => Sel { anchor: self.doc.pos_of_utf16(start_u + r.start), head: self.doc.pos_of_utf16(start_u + r.end) },
            None => Sel::caret(end),
        };
        self.reanalyze();
        self.want_scroll = true;
        self.kick_blink(cx);
        cx.emit(EditorEvent::Changed);
        cx.notify();
    }

    fn bounds_for_range(&mut self, range_utf16: Range<usize>, _: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let at = self.caret_at(self.doc.pos_of_utf16(range_utf16.start))?;
        Some(Bounds::new(point(self.origin.x + px(at.x), self.origin.y + px(at.y)), size(px(2.), px(at.h))))
    }

    fn character_index_for_point(&mut self, point: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        let local = (f32::from(point.x - self.origin.x), f32::from(point.y - self.origin.y));
        let p = self.pos_at(local.0, local.1, &self.cache.borrow())?;
        Some(self.doc.utf16_of(p))
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.doc.utf16_len())
    }
}

// -- the view ---------------------------------------------------------------

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.mode == Mode::Reading {
            let text = self.doc.text();
            return div().id("reading").w_full().child(reading_view(without_front_matter(&text), &self.look, cx)).into_any_element();
        }
        div()
            .id("editor")
            .key_context(EDITOR_CONTEXT)
            .track_focus(&self.focus)
            .w_full()
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::new_line))
            .on_action(cx.listener(Self::tab))
            .on_action(cx.listener(Self::outdent))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_word_back))
            .on_action(cx.listener(Self::delete_line_back))
            .on_action(cx.listener(|this, _: &Left, _, cx| this.left(false, cx)))
            .on_action(cx.listener(|this, _: &Right, _, cx| this.right(false, cx)))
            .on_action(cx.listener(|this, _: &Up, _, cx| this.vertical(false, false, cx)))
            .on_action(cx.listener(|this, _: &Down, _, cx| this.vertical(true, false, cx)))
            .on_action(cx.listener(|this, _: &SelectLeft, _, cx| this.left(true, cx)))
            .on_action(cx.listener(|this, _: &SelectRight, _, cx| this.right(true, cx)))
            .on_action(cx.listener(|this, _: &SelectUp, _, cx| this.vertical(false, true, cx)))
            .on_action(cx.listener(|this, _: &SelectDown, _, cx| this.vertical(true, true, cx)))
            .on_action(cx.listener(|this, _: &WordLeft, _, cx| this.word_left(false, cx)))
            .on_action(cx.listener(|this, _: &WordRight, _, cx| this.word_right(false, cx)))
            .on_action(cx.listener(|this, _: &SelectWordLeft, _, cx| this.word_left(true, cx)))
            .on_action(cx.listener(|this, _: &SelectWordRight, _, cx| this.word_right(true, cx)))
            .on_action(cx.listener(|this, _: &LineStart, _, cx| this.line_start(false, cx)))
            .on_action(cx.listener(|this, _: &LineEnd, _, cx| this.line_end(false, cx)))
            .on_action(cx.listener(|this, _: &SelectLineStart, _, cx| this.line_start(true, cx)))
            .on_action(cx.listener(|this, _: &SelectLineEnd, _, cx| this.line_end(true, cx)))
            .on_action(cx.listener(|this, _: &DocStart, _, cx| this.move_to(Pos::default(), false, cx)))
            .on_action(cx.listener(|this, _: &DocEnd, _, cx| {
                let end = this.doc.end();
                this.move_to(end, false, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectDocStart, _, cx| this.move_to(Pos::default(), true, cx)))
            .on_action(cx.listener(|this, _: &SelectDocEnd, _, cx| {
                let end = this.doc.end();
                this.move_to(end, true, cx)
            }))
            .on_action(cx.listener(|this, _: &PageUp, _, cx| this.page(false, cx)))
            .on_action(cx.listener(|this, _: &PageDown, _, cx| this.page(true, cx)))
            .on_action(cx.listener(Self::bold))
            .on_action(cx.listener(Self::italic))
            .on_action(cx.listener(Self::toggle_task))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .child(EditorElement { editor: cx.entity().clone() })
            .into_any_element()
    }
}

/// A note's text without the YAML header at its top, which the reading
/// view would read as a rule and a heading.
fn without_front_matter(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("---\n") else { return text };
    match rest.find("\n---") {
        Some(at) if rest[at + 4..].is_empty() || rest[at + 4..].starts_with('\n') => rest[at + 4..].trim_start_matches('\n'),
        _ => text,
    }
}

/// A note as it reads, drawn by the toolkit's markdown view in the
/// studio's manner: the words' face and size, inline code on a plate of
/// the accent, a block of code on a muted plate.
fn reading_view(text: &str, look: &Look, cx: &App) -> impl IntoElement {
    let theme = cx.theme().clone();
    let dark = theme.mode.is_dark();
    let style = TextViewStyle {
        heading_base_font_size: Pixels::ZERO,
        heading_font_size: Some(std::sync::Arc::new(|level, base| {
            base * match level {
                1 => 1.618,
                2 => 1.462,
                3 => 1.318,
                4 => 1.188,
                5 => 1.076,
                _ => 1.,
            }
        })),
        paragraph_gap: rems(0.8),
        highlight_theme: theme.highlight_theme.clone(),
        strong_font_weight: Some(FontWeight::SEMIBOLD),
        inline_code_font_family: Some(look.mono_family.clone()),
        inline_code: HighlightStyle { color: Some(theme.link), ..Default::default() },
        inline_code_chip: Some((theme.primary.opacity(if dark { 0.16 } else { 0.10 }), theme.primary.opacity(if dark { 0.22 } else { 0.18 }))),
        ..Default::default()
    }
    .code_block(StyleRefinement::default().bg(theme.muted).border_1().border_color(theme.border).rounded(px(10.)).px(px(12.)).py(px(10.)));
    div()
        .font_family(look.text_family.clone())
        .text_size(px(look.text_px))
        .line_height(relative(LINE))
        .text_color(theme.foreground)
        .child(TextView::markdown(SharedString::from(format!("reading-{}", if dark { "d" } else { "l" })), SharedString::from(text.to_string())).selectable(true).style(style))
}

// -- the element ------------------------------------------------------------

/// The element that lays the note out and paints it, one child of the
/// scroller the view around it draws.
struct EditorElement {
    editor: Entity<Editor>,
}

struct Prepainted {
    hitbox: Hitbox,
}

/// What paint takes out of the editor before it draws, since drawing
/// needs the app the editor is read through.
struct PaintInfo {
    focus: FocusHandle,
    over_link: bool,
    cache: Rc<RefCell<Cache>>,
    /// Each code block's first and last line, and where its plate begins.
    blocks: Vec<(usize, usize, f32)>,
    sel: Sel,
    marked: Option<(Pos, Pos)>,
    caret_on: bool,
}

impl IntoElement for EditorElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepainted;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, _: &mut App) -> (LayoutId, ()) {
        stamp("editor request_layout");
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let editor = self.editor.clone();
        // The height is the note's at the width it is given.
        let id = window.request_measured_layout(style, move |known, available, window, cx| {
            let width = known.width.or(match available.width {
                AvailableSpace::Definite(w) => Some(w),
                _ => None,
            });
            let width = f32::from(width.unwrap_or(px(600.))).max(80.);
            let started = Instant::now();
            let total = editor.update(cx, |e, cx| {
                e.relayout(width, window, cx);
                e.cache.borrow().total
            });
            if tracing() {
                eprintln!("trace measure width={width} known={:?} avail={:?}", known.width, available.width);
            }
            trace("relayout", started);
            size(px(width), px(total.max(f32::from(window.line_height()))))
        });
        (id, ())
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), window: &mut Window, cx: &mut App) -> Prepainted {
        stamp("editor prepaint");
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        self.editor.update(cx, |e, cx| {
            e.origin = bounds.origin;
            // The scroller reads its offset before its child is laid out,
            // so a move made here shows at the next frame, which is asked for.
            if e.want_scroll && e.scroll_to_caret(bounds, window) {
                cx.notify();
            }
        });
        Prepainted { hitbox }
    }

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), prepaint: &mut Prepainted, window: &mut Window, cx: &mut App) {
        let theme = cx.theme().clone();
        let editor = self.editor.clone();
        stamp("paint begins");
        let paint_started = Instant::now();
        let info = editor.update(cx, |e, _| {
            e.focused = e.focus.is_focused(window);
            PaintInfo {
                focus: e.focus.clone(),
                over_link: e.over_link,
                cache: e.cache.clone(),
                blocks: e.analysis.code_blocks.iter().map(|b| (b.first, b.last, e.base_inset(&e.analysis.lines[b.first]))).collect(),
                sel: e.sel,
                marked: e.marked,
                caret_on: e.caret_shown() && e.mode != Mode::Reading,
            }
        });
        window.handle_input(&info.focus, ElementInputHandler::new(bounds, editor.clone()), cx);
        window.set_cursor_style(if info.over_link { CursorStyle::PointingHand } else { CursorStyle::IBeam }, &prepaint.hitbox);

        let cache = info.cache.borrow();
        let view = window.content_mask().bounds;
        let origin = bounds.origin;
        let (ox, oy) = (f32::from(origin.x), f32::from(origin.y));
        let visible = |top: f32, h: f32| oy + top + h >= f32::from(view.top()) - 1. && oy + top <= f32::from(view.bottom()) + 1.;
        let first = match cache.tops.binary_search_by(|t| t.partial_cmp(&(f32::from(view.top()) - oy)).unwrap_or(std::cmp::Ordering::Equal)) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };

        // The plates behind code blocks, before anything on them.
        for &(first_line, last_line, inset) in &info.blocks {
            let (Some(top), Some(last)) = (cache.tops.get(first_line), cache.entries.get(last_line)) else { continue };
            let bottom = cache.tops[last_line] + last.height;
            if !visible(*top, bottom - top) {
                continue;
            }
            let b = Bounds::new(point(px(ox + inset), px(oy + top)), size(bounds.size.width - px(inset), px(bottom - top)));
            window.paint_quad(quad(b, px(8.), theme.muted, px(1.), theme.border, BorderStyle::default()));
        }

        let (sa, sb) = info.sel.range();
        let is_caret = info.sel.is_caret();
        let mut caret_box: Option<Bounds<Pixels>> = None;
        for line in first..cache.entries.len() {
            let entry = &cache.entries[line];
            let top = cache.tops[line];
            if oy + top > f32::from(view.bottom()) + 1. {
                break;
            }
            if entry.height <= 0. || !visible(top, entry.height) {
                continue;
            }
            let row_origin = point(px(ox), px(oy + top));
            match &*entry.row {
                Row::Text(r) => {
                    let text_origin = point(px(ox + r.inset), px(oy + top + r.top_pad));
                    let lh = px(r.line_h);
                    // The chips: a plate behind inline code, a wash behind a highlight.
                    for (range, color) in &r.chips {
                        for b in text_boxes(&r.line, range.clone(), r.line_h, text_origin) {
                            let b = Bounds::new(point(b.origin.x - px(2.), b.origin.y + px(r.line_h * 0.08)), size(b.size.width + px(4.), px(r.line_h * 0.84)));
                            window.paint_quad(quad(b, px(4.), *color, px(0.), theme.transparent, BorderStyle::default()));
                        }
                    }
                    // The selection, as far as it lies on this line.
                    if sa != sb && sa.line <= line && line <= sb.line {
                        let from = if sa.line == line { r.shown.to_shown(sa.col) } else { 0 };
                        let to = if sb.line == line { r.shown.to_shown(sb.col) } else { r.shown.text.len() };
                        let past_end = sb.line > line;
                        if from < to || past_end {
                            let mut boxes = text_boxes(&r.line, from..to.max(from), r.line_h, text_origin);
                            if past_end {
                                // The line's end is selected too: a little past the last word.
                                if let Some(last) = boxes.last_mut() {
                                    last.size.width += px(6.);
                                } else {
                                    boxes.push(Bounds::new(text_origin, size(px(6.), lh)));
                                }
                            }
                            for b in boxes {
                                window.paint_quad(fill(b, theme.selection));
                            }
                        }
                    }
                    // The words.
                    let _ = r.line.paint(text_origin, lh, TextAlign::Left, None, window, cx);
                    // An input method's composition, underlined.
                    if let Some((ma, mb)) = info.marked {
                        if ma.line == line {
                            let (a, b) = (r.shown.to_shown(ma.col), r.shown.to_shown(if mb.line == line { mb.col } else { r.source_len }));
                            for bx in text_boxes(&r.line, a..b, r.line_h, text_origin) {
                                window.paint_quad(fill(Bounds::new(point(bx.origin.x, bx.origin.y + lh - px(2.)), size(bx.size.width, px(1.))), theme.foreground));
                            }
                        }
                    }
                    // The widgets.
                    for w in &r.widgets {
                        match w {
                            Widget::Bullet { x, y, r: radius } => {
                                let b = Bounds::new(point(px(ox + x - radius), px(oy + top + y - radius)), size_px(radius * 2.));
                                window.paint_quad(quad(b, px(*radius), theme.muted_foreground, px(0.), theme.transparent, BorderStyle::default()));
                            }
                            Widget::Number { line: shaped, x, y } => {
                                let _ = shaped.paint(point(px(ox + x), px(oy + top + y)), lh, TextAlign::Left, None, window, cx);
                            }
                            Widget::Task { bounds: b, done } => {
                                let b = Bounds::new(point(px(ox) + b.origin.x, px(oy + top) + b.origin.y), b.size);
                                if *done {
                                    window.paint_quad(quad(b, px(4.), theme.primary, px(0.), theme.transparent, BorderStyle::default()));
                                    let inner = Bounds::new(point(b.origin.x + b.size.width * 0.15, b.origin.y + b.size.height * 0.15), size(b.size.width * 0.7, b.size.height * 0.7));
                                    let _ = window.paint_svg(inner, "icons/check.svg".into(), None, TransformationMatrix::unit(), theme.primary_foreground, cx);
                                } else {
                                    window.paint_quad(quad(b, px(4.), theme.transparent, px(1.5), theme.muted_foreground.opacity(0.7), BorderStyle::default()));
                                }
                            }
                            Widget::Guide { x } => {
                                let b = Bounds::new(point(px(ox + x), row_origin.y), size(px(1.), px(entry.height)));
                                window.paint_quad(fill(b, theme.border));
                            }
                            Widget::Bar { x } => {
                                let b = Bounds::new(point(px(ox + x), row_origin.y), size(px(2.), px(entry.height)));
                                window.paint_quad(fill(b, theme.primary.opacity(0.7)));
                            }
                        }
                    }
                    // The caret.
                    if is_caret && sa.line == line {
                        let shown_ix = r.shown.to_shown(sa.col);
                        let at = r.line.position_for_index(shown_ix, lh).unwrap_or_default();
                        caret_box = Some(Bounds::new(point(text_origin.x + at.x, text_origin.y + at.y), size(px(1.5), lh)));
                    }
                }
                Row::Pad { lang } => {
                    if let Some(l) = lang {
                        let x = f32::from(bounds.size.width) - f32::from(l.width()) - PLATE_X - 2.;
                        let _ = l.paint(point(px(ox + x), px(oy + top + 2.)), px(PLATE_PAD + 6.), TextAlign::Left, None, window, cx);
                    }
                }
                Row::Hidden => {}
                Row::Rule { inset, line_h } => {
                    let b = Bounds::new(point(px(ox + inset), px(oy + top + line_h / 2. - 1.)), size(bounds.size.width - px(*inset), px(2.)));
                    window.paint_quad(fill(b, theme.border));
                    if is_caret && sa.line == line {
                        caret_box = Some(Bounds::new(point(px(ox + inset), px(oy + top)), size(px(1.5), px(*line_h))));
                    }
                }
                Row::Table(t) => {
                    let total_w: f32 = t.cols.iter().sum();
                    let left = ox + t.inset;
                    let mut y = oy + top;
                    for (ri, row) in t.cells.iter().enumerate() {
                        let h = t.heights[ri];
                        if ri == 0 {
                            window.paint_quad(fill(Bounds::new(point(px(left), px(y)), size(px(total_w), px(h))), theme.muted.opacity(0.6)));
                        }
                        let mut x = left;
                        for (ci, cell) in row.iter().enumerate() {
                            if let Some(c) = cell {
                                let _ = c.paint(point(px(x + CELL_X), px(y + CELL_Y)), px(t.line_h), TextAlign::Left, None, window, cx);
                            }
                            window.paint_quad(fill(Bounds::new(point(px(x), px(y)), size(px(1.), px(h))), theme.border));
                            x += t.cols[ci];
                        }
                        window.paint_quad(fill(Bounds::new(point(px(x), px(y)), size(px(1.), px(h))), theme.border));
                        window.paint_quad(fill(Bounds::new(point(px(left), px(y)), size(px(total_w + 1.), px(1.))), theme.border));
                        y += h;
                    }
                    window.paint_quad(fill(Bounds::new(point(px(left), px(y)), size(px(total_w + 1.), px(1.))), theme.border));
                }
            }
        }
        if let Some(b) = caret_box.filter(|_| info.caret_on) {
            window.paint_quad(fill(b, theme.caret));
        }
        drop(cache);
        trace("paint", paint_started);

        // The pointer: a press places the caret or takes a widget, a drag
        // selects, a press on a link follows it.
        let hitbox = prepaint.hitbox.clone();
        let ed = editor.clone();
        window.on_mouse_event(move |ev: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || ev.button != MouseButton::Left || !hitbox.is_hovered(window) {
                return;
            }
            let (x, y) = (f32::from(ev.position.x - bounds.origin.x), f32::from(ev.position.y - bounds.origin.y));
            stamp("mouse down");
            ed.update(cx, |e, cx| {
                if e.mode == Mode::Reading {
                    return;
                }
                if !e.focus.is_focused(window) {
                    window.focus(&e.focus, cx);
                }
                match e.hit(x, y) {
                    Some(Hit::Task(line)) if !e.read_only => {
                        e.toggle_task_on(line, cx);
                        e.doc.end_step();
                    }
                    Some(Hit::Link(target)) if ev.modifiers.secondary() || ev.click_count == 1 => {
                        match target {
                            Target::Link(url) => cx.emit(EditorEvent::OpenLink(url)),
                            Target::Note(name) => cx.emit(EditorEvent::OpenNote(name)),
                        }
                    }
                    _ => {
                        let p = match e.pos_at(x, y, &e.cache.borrow()) {
                            Some(p) => p,
                            None => return,
                        };
                        let grain = match ev.click_count {
                            1 => Grain::Char,
                            2 => Grain::Word,
                            _ => Grain::Line,
                        };
                        let (a, b) = e.grain_range(p, grain);
                        if ev.modifiers.shift && grain == Grain::Char {
                            e.sel.head = p;
                        } else {
                            e.sel = Sel { anchor: a, head: b };
                        }
                        e.drag = Some((grain, a, b));
                        e.preferred_x = None;
                        e.kick_blink(cx);
                        cx.notify();
                    }
                }
            });
            crate::widgets::swallow_click(window, cx);
        });
        let ed = editor.clone();
        window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let (x, y) = (f32::from(ev.position.x - bounds.origin.x), f32::from(ev.position.y - bounds.origin.y));
            ed.update(cx, |e, cx| {
                if let (Some((grain, a, b)), Some(MouseButton::Left)) = (e.drag, ev.pressed_button) {
                    if let Some(p) = e.pos_at(x, y.max(0.), &e.cache.borrow()) {
                        let (pa, pb) = e.grain_range(p, grain);
                        let (anchor, head) = if pa < a { (b, pa) } else { (a, pb.max(b)) };
                        if e.sel != (Sel { anchor, head }) {
                            e.sel = Sel { anchor, head };
                            e.want_scroll = true;
                            cx.notify();
                        }
                    }
                } else {
                    let over = window.mouse_position().x >= bounds.origin.x && matches!(e.hit(x, y), Some(Hit::Link(_)) | Some(Hit::Task(_)));
                    if over != e.over_link {
                        e.over_link = over;
                        cx.notify();
                    }
                }
            });
        });
        let ed = editor;
        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble {
                ed.update(cx, |e, _| e.drag = None);
            }
        });
    }
}

impl Editor {
    /// What a press selects at `p`: the place, the word, the line.
    fn grain_range(&self, p: Pos, grain: Grain) -> (Pos, Pos) {
        match grain {
            Grain::Char => (p, p),
            Grain::Word => {
                let w = self.doc.word_at(p);
                if w.start == w.end {
                    (p, self.doc.next_grapheme(p))
                } else {
                    (Pos::new(p.line, w.start), Pos::new(p.line, w.end))
                }
            }
            Grain::Line => (Pos::new(p.line, 0), if p.line + 1 < self.doc.line_count() { Pos::new(p.line + 1, 0) } else { self.doc.end() }),
        }
    }
}

/// The boxes a range of a wrapped line's text takes, one a wrapped row.
fn text_boxes(line: &WrappedLine, range: Range<usize>, line_h: f32, origin: Point<Pixels>) -> Vec<Bounds<Pixels>> {
    let mut out = Vec::new();
    if range.start > range.end {
        return out;
    }
    let lh = px(line_h);
    // The rows' starts, by the glyph each wrap begins at.
    let mut starts: Vec<usize> = vec![0];
    for wb in line.wrap_boundaries() {
        let run = &line.runs()[wb.run_ix];
        starts.push(run.glyphs[wb.glyph_ix].index);
    }
    let len = line.len();
    for (ri, &row_start) in starts.iter().enumerate() {
        let row_end = starts.get(ri + 1).copied().unwrap_or(len);
        let a = range.start.max(row_start);
        let b = range.end.min(row_end);
        if a > b || (a == b && !(range.start == range.end && a == row_start)) {
            continue;
        }
        let xa = line.position_for_index(a, lh).map(|p| p.x).unwrap_or_default();
        let xb = if b == row_end && ri + 1 < starts.len() {
            // To the row's end: the x of its last glyph plus its width.
            line.position_for_index(b, lh).map(|p| if (p.y / lh) as usize > ri { px(f32::from(line.width())) } else { p.x }).unwrap_or(line.width())
        } else {
            line.position_for_index(b, lh).map(|p| p.x).unwrap_or(line.width())
        };
        let w = (xb - xa).max(px(0.));
        out.push(Bounds::new(point(origin.x + xa, origin.y + lh * ri as f32), size(w, lh)));
    }
    out
}
