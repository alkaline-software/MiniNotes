//! The main window: one view (`Shell`) that draws the frame, the sidebar
//! on it, and the content as a card with rounded corners. The sidebar is
//! the vault's tree: its folders and notes, as the disk has them. The
//! card is the note open in the editor (`editor.rs`) under a strip with
//! its name and how it is shown, or, with no vault open, the landing
//! page. It also holds what the settings window draws (`settings.rs`),
//! so a change shows in both windows at once.
//!
//! Everything on the disk goes through `onyx_core::vault`: a note is
//! written whole a moment after the last keystroke and before it is left,
//! and the vault's folder is watched so the tree and an untouched note
//! follow what another app does there.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::tooltip::ManagedTooltipExt as _;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, IconName, Sizable as _};

use onyx_core::config::Config;
use onyx_core::{update, vault};

use crate::ambient::Ambient;
use crate::editor::{Editor, EditorEvent, Look, Mode};
use crate::i18n::{t, tf};
use crate::menu::{Entry, Menus};
use crate::scroll::Scrollers;
use crate::ui_state::{Rect, UiState};
use crate::widgets::{choice_pill, float_shadow, key_text, page_in, small_pill, swallow_click, tip, Motion, Press, FOLD_ANIM, SLIDE};

actions!(onyx, [ToggleSidebar, OpenSettings, Escape, NewNote, NewFolder, OpenVault, CloseVault, Save, ShowLive, ShowSource, ShowReading, ToggleReading]);

/// The key context of the main window.
pub const KEY_CONTEXT: &str = "Shell";
/// The key context of a sheet over the window.
const SHEET_CONTEXT: &str = "Sheet";

/// The window's ground shows this far around the card.
pub const FRAME: Pixels = px(4.);
/// The card's corner: the window's less the frame's band, so the two
/// curves run side by side. Off a Mac the window's corner is small.
pub const CARD_R: Pixels = px(if cfg!(target_os = "macos") { 12. } else { 4. });
/// The card's top strip, where the traffic lights and the strip's buttons
/// stand on one line.
pub const TITLEBAR_H: Pixels = px(48.);
/// The room the traffic lights take at the window's left.
const TRAFFIC_W: Pixels = px(85.);
const STRIP_BTN: Pixels = px(26.);
pub const SIDEBAR_W: Pixels = px(240.);
/// A note's words run this wide at most, as a comfortable line of
/// reading, when the settings keep them readable.
const READABLE_W: Pixels = px(700.);
/// The least the window is dragged to.
pub const MIN_W: Pixels = px(720.);
/// A note is written this long after the last keystroke.
const SAVE_AFTER: Duration = Duration::from_millis(600);
/// How often the vault's changes are looked for.
const WATCH_POLL: Duration = Duration::from_millis(300);

/// The vault open: its folder, its tree as last read, the folders folded
/// shut, and the watcher that says when to read it again.
struct Vault {
    root: PathBuf,
    tree: vault::Entry,
    folded: HashSet<PathBuf>,
    _watcher: Option<vault::Watcher>,
    changes: Rc<std::sync::mpsc::Receiver<Vec<PathBuf>>>,
}

/// The note open: its file, whether the editor has what the disk has
/// not, and the text as last written, to see a change on the disk.
struct Note {
    path: PathBuf,
    dirty: bool,
    saved: String,
    /// Counts up at each change, so a save that was waiting on an older
    /// one does nothing.
    serial: u64,
}

pub struct Shell {
    pub(crate) cfg: Config,
    pub(crate) motion: Motion,
    scrollers: Scrollers,
    pub(crate) menus: Menus,
    focus: FocusHandle,
    ambient: Entity<Ambient>,
    sidebar_open: bool,
    /// The sidebar's coming or going: which, and when it began.
    side_anim: Option<(bool, Instant)>,
    side_scroll: ScrollHandle,
    page_scroll: ScrollHandle,
    vault: Option<Vault>,
    note: Option<Note>,
    pub(crate) editor: Entity<Editor>,
    /// The measured height of each folder's children, for its fold.
    fold_h: RefCell<HashMap<PathBuf, Rc<Cell<f32>>>>,
    /// The row a right click was on.
    pressed: Option<PathBuf>,
    /// A rename under way: the entry, and the field for its name.
    rename: Option<PathBuf>,
    rename_input: Entity<InputState>,
    /// Something to say under the strip, and when it was said.
    notice: Option<(String, Instant)>,
    /// When the window's bounds were last written.
    bounds_saved: Option<Instant>,
    pub(crate) settings: Option<AnyWindowHandle>,
    pub(crate) settings_section: &'static str,
    pub(crate) settings_scroll: ScrollHandle,
    pub(crate) lang_serial: u32,
    pub(crate) update: UpdateRow,
}

/// What the Updates row says and does.
#[derive(Default, Clone)]
pub(crate) struct UpdateRow {
    pub checking: bool,
    pub found: Option<String>,
    /// While a download runs: how far, from 0 to 100.
    pub downloading: Option<u32>,
    pub said: Option<String>,
}

impl Shell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let ui = UiState::load();
        // The bounds are written as they change, at most every two seconds.
        cx.observe_window_bounds(window, |this, window, _| {
            if this.bounds_saved.is_none_or(|at| at.elapsed() > Duration::from_secs(2)) {
                this.bounds_saved = Some(Instant::now());
                this.save_ui(window);
            }
        })
        .detach();
        let editor = cx.new(Editor::new);
        cx.subscribe(&editor, |this, _, ev: &EditorEvent, cx| match ev {
            EditorEvent::Changed => this.note_changed(cx),
            EditorEvent::OpenLink(url) => this.open_link(url, cx),
            EditorEvent::OpenNote(name) => this.open_wikilink(name, cx),
        })
        .detach();
        let rename_input = cx.new(|cx| InputState::new(window, cx));
        cx.subscribe_in(&rename_input, window, |this, _, ev: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = ev {
                this.finish_rename(window, cx);
            }
        })
        .detach();
        // A quit writes the note first.
        cx.on_app_quit(|this, cx| {
            this.save_now(cx);
            async {}
        })
        .detach();
        let cfg = Config::load();
        let mut shell = Self {
            cfg,
            motion: Motion::default(),
            scrollers: Scrollers::default(),
            menus: Menus::default(),
            focus: cx.focus_handle(),
            ambient: cx.new(|_| Ambient::default()),
            sidebar_open: ui.sidebar_open.unwrap_or(true),
            side_anim: None,
            side_scroll: ScrollHandle::new(),
            page_scroll: ScrollHandle::new(),
            vault: None,
            note: None,
            editor,
            fold_h: RefCell::new(HashMap::new()),
            pressed: None,
            rename: None,
            rename_input,
            notice: None,
            bounds_saved: None,
            settings: None,
            settings_section: crate::settings::section_named(ui.settings_section.as_deref().unwrap_or("appearance")),
            settings_scroll: ScrollHandle::new(),
            lang_serial: 0,
            update: UpdateRow::default(),
        };
        shell.push_look(cx);
        let mode = Mode::from_key(ui.mode.as_deref().unwrap_or("live"));
        shell.editor.update(cx, |e, cx| e.set_mode(mode, cx));
        // A probe's vault stands over the remembered one.
        let vault = std::env::var_os("ONYX_VAULT").map(PathBuf::from).or_else(|| ui.vault.clone());
        if let Some(root) = vault.filter(|p| p.is_dir()) {
            shell.open_vault(root, window, cx);
            // A probe names a note to open (`ONYX_NOTE`); otherwise the one
            // that was open is opened again.
            let probe_note = std::env::var_os("ONYX_NOTE").map(PathBuf::from).filter(|p| p.is_file());
            let note = probe_note.or_else(|| ui.note.filter(|p| p.is_file() && std::env::var_os("ONYX_VAULT").is_none()));
            if let Some(note) = note {
                shell.open_note(note, window, cx);
            }
        }
        if shell.cfg.app.check_updates {
            shell.check_updates_later(cx);
        }
        shell
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// What the window remembers, written now.
    pub fn save_ui(&self, window: &Window) {
        let mut ui = UiState::load();
        // `window_bounds()`, never `bounds()` (`ui_state.rs`).
        if let WindowBounds::Windowed(b) = window.window_bounds() {
            ui.window = Some(Rect { x: f32::from(b.origin.x), y: f32::from(b.origin.y), w: f32::from(b.size.width), h: f32::from(b.size.height) });
        }
        ui.sidebar_open = Some(self.sidebar_open);
        ui.settings_section = Some(self.settings_section.to_string());
        if std::env::var_os("ONYX_VAULT").is_none() {
            ui.vault = self.vault.as_ref().map(|v| v.root.clone());
            ui.note = self.note.as_ref().map(|n| n.path.clone());
            ui.folded = self.vault.as_ref().map(|v| v.folded.iter().cloned().collect()).unwrap_or_default();
        }
        ui.save();
    }

    /// The faces and size the editor is set in, from the settings.
    pub(crate) fn push_look(&mut self, cx: &mut Context<Self>) {
        let e = &self.cfg.editor;
        let look = Look {
            text_family: crate::fonts::text_family(&e.text_face),
            mono_family: crate::fonts::mono_family(&e.mono_face),
            text_px: match e.text_size.as_str() {
                "small" => 14.,
                "large" => 18.,
                _ => 16.,
            },
        };
        self.editor.update(cx, |ed, cx| ed.set_look(look, cx));
    }

    // -- the vault ------------------------------------------------------------

    pub(crate) fn vault_root(&self) -> Option<&Path> {
        self.vault.as_ref().map(|v| v.root.as_path())
    }

    fn open_vault(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.save_now(cx);
        self.note = None;
        self.editor.update(cx, |e, cx| e.set_text("", cx));
        let ui = UiState::load();
        let folded: HashSet<PathBuf> = ui.folded.iter().filter(|p| p.starts_with(&root)).cloned().collect();
        let (tx, rx) = std::sync::mpsc::channel::<Vec<PathBuf>>();
        let watcher = match vault::watch(&root, move |paths| {
            let _ = tx.send(paths);
        }) {
            Ok(w) => Some(w),
            Err(e) => {
                self.say(tf("The vault is not being watched: {why}", &[("why", &e)]), cx);
                None
            }
        };
        let tree = vault::tree(&root);
        self.vault = Some(Vault { root: root.clone(), tree, folded, _watcher: watcher, changes: Rc::new(rx) });
        let mut ui = ui;
        ui.remember_vault(&root);
        ui.save();
        self.watch_changes(cx);
        self.save_ui(window);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The vault's changes, looked for every so often; a burst of them is
    /// one reading of the tree.
    fn watch_changes(&mut self, cx: &mut Context<Self>) {
        let Some(rx) = self.vault.as_ref().map(|v| v.changes.clone()) else { return };
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(WATCH_POLL).await;
                let mut paths: Vec<PathBuf> = Vec::new();
                while let Ok(p) = rx.try_recv() {
                    paths.extend(p);
                }
                let alive = this
                    .update(cx, |s, cx| {
                        let same = s.vault.as_ref().is_some_and(|v| Rc::ptr_eq(&v.changes, &rx));
                        if same && !paths.is_empty() {
                            s.disk_changed(&paths, cx);
                        }
                        same
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }

    /// Something in the vault changed: the tree is read again off the
    /// main thread, and an untouched open note takes what the disk has.
    fn disk_changed(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let Some(root) = self.vault_root().map(Path::to_path_buf) else { return };
        let note_path = self.note.as_ref().map(|n| n.path.clone());
        let note_touched = note_path.as_ref().is_some_and(|n| paths.iter().any(|p| p == n));
        let task = cx.background_executor().spawn(async move {
            let tree = vault::tree(&root);
            let text = note_path.filter(|_| note_touched).and_then(|p| vault::read(&p).ok());
            (tree, text)
        });
        cx.spawn(async move |this, cx| {
            let (tree, text) = task.await;
            let _ = this.update(cx, |s, cx| {
                if let Some(v) = s.vault.as_mut() {
                    v.tree = tree;
                }
                if let (Some(text), Some(note)) = (text, s.note.as_mut()) {
                    if !note.dirty && note.saved != text {
                        note.saved = text.clone();
                        s.editor.update(cx, |e, cx| e.replace_text_quietly(&text, cx));
                    }
                }
                // A note gone from the disk is closed.
                if let Some(n) = &s.note {
                    if !n.path.exists() {
                        s.note = None;
                        s.editor.update(cx, |e, cx| e.set_text("", cx));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn reread_tree(&mut self, cx: &mut Context<Self>) {
        if let Some(v) = self.vault.as_mut() {
            v.tree = vault::tree(&v.root);
        }
        cx.notify();
    }

    /// Ask the system for a folder to be the vault.
    pub(crate) fn choose_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some(t("Open").into()) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(dir) = paths.into_iter().next() else { return };
            let _ = cx.update(|window, cx| {
                let _ = this.update(cx, |s, cx| s.open_vault(dir, window, cx));
            });
        })
        .detach();
    }

    fn close_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_now(cx);
        self.note = None;
        self.vault = None;
        self.editor.update(cx, |e, cx| e.set_text("", cx));
        let mut ui = UiState::load();
        ui.vault = None;
        ui.note = None;
        ui.save();
        self.save_ui(window);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    // -- the note -------------------------------------------------------------

    fn open_note(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.note.as_ref().is_some_and(|n| n.path == path) {
            window.focus(&self.editor.read(cx).focus_handle(cx), cx);
            return;
        }
        self.save_now(cx);
        match vault::read(&path) {
            Ok(text) => {
                self.editor.update(cx, |e, cx| e.set_text(&text, cx));
                self.note = Some(Note { path, dirty: false, saved: text, serial: 0 });
                window.focus(&self.editor.read(cx).focus_handle(cx), cx);
                self.save_ui(window);
            }
            Err(e) => self.say(tf("The note could not be read: {why}", &[("why", &e)]), cx),
        }
        cx.notify();
    }

    /// The editor's text changed: it is written a moment after the last change.
    fn note_changed(&mut self, cx: &mut Context<Self>) {
        let Some(note) = self.note.as_mut() else { return };
        note.dirty = true;
        note.serial += 1;
        let serial = note.serial;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_AFTER).await;
            let _ = this.update(cx, |s, cx| {
                if s.note.as_ref().is_some_and(|n| n.serial == serial && n.dirty) {
                    s.save_now(cx);
                }
            });
        })
        .detach();
    }

    /// The note written now, when it has changes.
    pub(crate) fn save_now(&mut self, cx: &mut Context<Self>) {
        let Some(note) = self.note.as_mut() else { return };
        if !note.dirty {
            return;
        }
        let text = self.editor.read(cx).text();
        match vault::write(&note.path, &text) {
            Ok(()) => {
                note.dirty = false;
                note.saved = text;
            }
            Err(e) => {
                let why = e.to_string();
                self.say(tf("The note could not be written: {why}", &[("why", &why)]), cx);
            }
        }
        cx.notify();
    }

    fn new_note_in(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        match vault::new_note(&dir, &t("Untitled")) {
            Ok(path) => {
                self.reread_tree(cx);
                if let Some(v) = self.vault.as_mut() {
                    let mut up = path.parent();
                    while let Some(d) = up {
                        v.folded.remove(d);
                        up = d.parent();
                    }
                }
                self.open_note(path.clone(), window, cx);
                self.start_rename(path, window, cx);
            }
            Err(e) => self.say(tf("The note could not be made: {why}", &[("why", &e)]), cx),
        }
    }

    fn new_folder_in(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        match vault::new_folder(&dir, &t("New folder")) {
            Ok(path) => {
                self.reread_tree(cx);
                self.start_rename(path, window, cx);
            }
            Err(e) => self.say(tf("The folder could not be made: {why}", &[("why", &e)]), cx),
        }
    }

    /// Where a new note goes: the open note's folder, else the vault.
    fn new_note_dir(&self) -> Option<PathBuf> {
        let root = self.vault_root()?;
        Some(self.note.as_ref().and_then(|n| n.path.parent()).map(Path::to_path_buf).unwrap_or_else(|| root.to_path_buf()))
    }

    fn open_link(&mut self, url: &str, cx: &mut Context<Self>) {
        if url.contains("://") || url.starts_with("mailto:") {
            crate::sys::open(url);
            return;
        }
        // A path relative to the note: a note of the vault opens here,
        // anything else with the system's app.
        let Some(base) = self.note.as_ref().and_then(|n| n.path.parent()).map(Path::to_path_buf) else { return };
        let path = base.join(url.split('#').next().unwrap_or(url));
        if vault::is_note(&path) && path.is_file() {
            let weak = cx.entity().downgrade();
            cx.defer(move |cx| {
                let _ = cx.active_window().map(|w| {
                    w.update(cx, |_, window, cx| {
                        let _ = weak.update(cx, |s, cx| s.open_note(path, window, cx));
                    })
                });
            });
        } else if path.exists() {
            crate::sys::open(&path.to_string_lossy());
        }
    }

    /// A `[[note]]` link: the note it names, or a new note of that name
    /// in the vault's folder, as Obsidian makes one.
    fn open_wikilink(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(root) = self.vault_root().map(Path::to_path_buf) else { return };
        let from = self.note.as_ref().map(|n| n.path.clone()).unwrap_or_else(|| root.clone());
        let target = vault::resolve_link(&root, &from, name).or_else(|| {
            let stem = name.split(['#', '|']).next().unwrap_or(name).trim();
            vault::new_note(&root, stem).ok()
        });
        let Some(path) = target else { return };
        let weak = cx.entity().downgrade();
        cx.defer(move |cx| {
            let _ = cx.active_window().map(|w| {
                w.update(cx, |_, window, cx| {
                    let _ = weak.update(cx, |s, cx| {
                        s.reread_tree(cx);
                        s.open_note(path, window, cx)
                    });
                })
            });
        });
    }

    fn say(&mut self, words: String, cx: &mut Context<Self>) {
        self.notice = Some((words, Instant::now()));
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(6)).await;
            let _ = this.update(cx, |s, cx| {
                if s.notice.as_ref().is_some_and(|(_, at)| at.elapsed() >= Duration::from_secs(6)) {
                    s.notice = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    // -- renaming and the menus -------------------------------------------------

    fn start_rename(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let name = if path.is_dir() { path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default() } else { vault::title(&path) };
        self.rename = Some(path);
        self.rename_input.update(cx, |i, cx| {
            i.set_value(name.clone(), window, cx);
            i.select_all(window, cx);
        });
        window.focus(&self.rename_input.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    fn finish_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.rename.take() else { return };
        let name = self.rename_input.read(cx).value().to_string();
        let was_open = self.note.as_ref().is_some_and(|n| n.path == path);
        if was_open {
            self.save_now(cx);
        }
        match vault::rename(&path, &name) {
            Ok(new_path) => {
                if was_open {
                    if let Some(n) = self.note.as_mut() {
                        n.path = new_path.clone();
                    }
                } else if let Some(n) = self.note.as_mut() {
                    // A note inside a renamed folder moved with it.
                    if let Ok(rest) = n.path.strip_prefix(&path) {
                        n.path = new_path.join(rest);
                    }
                }
                if let Some(v) = self.vault.as_mut() {
                    if v.folded.remove(&path) {
                        v.folded.insert(new_path.clone());
                    }
                }
                self.reread_tree(cx);
                self.save_ui(window);
            }
            Err(e) => self.say(tf("The name was not changed: {why}", &[("why", &e)]), cx),
        }
        self.focus_editor_or_shell(window, cx);
        cx.notify();
    }

    fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.rename = None;
        self.focus_editor_or_shell(window, cx);
        cx.notify();
    }

    fn focus_editor_or_shell(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.note.is_some() {
            window.focus(&self.editor.read(cx).focus_handle(cx), cx);
        } else {
            window.focus(&self.focus, cx);
        }
    }

    fn trash(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.note.as_ref().is_some_and(|n| n.path == path || n.path.starts_with(&path)) {
            self.save_now(cx);
            self.note = None;
            self.editor.update(cx, |e, cx| e.set_text("", cx));
            window.focus(&self.focus, cx);
        }
        if let Err(e) = vault::trash(&path) {
            self.say(tf("It was not moved to the trash: {why}", &[("why", &e)]), cx);
        }
        self.reread_tree(cx);
        self.save_ui(window);
    }

    fn entry_menu(&self, path: &Path) -> Vec<Entry> {
        let reveal = crate::sys::REVEAL_LABEL;
        if path.is_dir() {
            vec![
                Entry::choice("new-note", "icons/file-plus.svg", t("New Note")),
                Entry::choice("new-folder", "icons/folder-plus.svg", t("New Folder")),
                Entry::Rule,
                Entry::choice("reveal", "icons/folder.svg", t(reveal)),
                Entry::Rule,
                Entry::choice("rename", "icons/pencil-simple.svg", t("Rename…")),
                Entry::choice("trash", "icons/trash.svg", t("Move to Trash")),
            ]
        } else {
            vec![
                Entry::choice("open", "icons/external-link.svg", t("Open")),
                Entry::choice("reveal", "icons/folder.svg", t(reveal)),
                Entry::Rule,
                Entry::choice("copy-path", "icons/copy.svg", t("Copy Path")),
                Entry::Rule,
                Entry::choice("rename", "icons/pencil-simple.svg", t("Rename…")),
                Entry::choice("trash", "icons/trash.svg", t("Move to Trash")),
            ]
        }
    }

    fn menu_pick(&mut self, what: &'static str, id: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        let pressed = self.pressed.take();
        self.menus.close(cx);
        match (what, id.as_ref()) {
            ("entry", "open") => {
                if let Some(p) = pressed {
                    self.open_note(p, window, cx);
                }
            }
            ("entry", "reveal") => {
                if let Some(p) = pressed {
                    crate::sys::reveal(&p);
                }
            }
            ("entry", "copy-path") => {
                if let Some(p) = pressed {
                    cx.write_to_clipboard(ClipboardItem::new_string(p.to_string_lossy().to_string()));
                }
            }
            ("entry", "rename") => {
                if let Some(p) = pressed {
                    self.start_rename(p, window, cx);
                }
            }
            ("entry", "trash") => {
                if let Some(p) = pressed {
                    self.trash(p, window, cx);
                }
            }
            ("entry", "new-note") => {
                if let Some(p) = pressed {
                    self.new_note_in(p, window, cx);
                }
            }
            ("entry", "new-folder") => {
                if let Some(p) = pressed {
                    self.new_folder_in(p, window, cx);
                }
            }
            ("vault", "open-other") => self.choose_vault(window, cx),
            ("vault", "reveal") => {
                if let Some(r) = self.vault_root() {
                    crate::sys::reveal(r);
                }
            }
            ("vault", "close") => self.close_vault(window, cx),
            _ => {}
        }
        cx.notify();
    }

    // -- the sidebar ----------------------------------------------------------

    pub fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // From as far as the move before had got, so a second press while
        // it moves turns it back without a jump.
        let begun = match self.side_anim {
            Some((_, at)) if at.elapsed() < SLIDE => Instant::now() - (SLIDE - at.elapsed()),
            _ => Instant::now(),
        };
        self.sidebar_open = !self.sidebar_open;
        self.side_anim = Some((self.sidebar_open, begun));
        self.save_ui(window);
        cx.notify();
    }

    /// How much of the sidebar is beside the content, from 0 to 1, on one
    /// eased clock that every term of the row's layout goes by.
    fn side_t(&self) -> f32 {
        match self.side_anim {
            Some((coming, at)) if at.elapsed() < SLIDE => {
                let t = ease_out_quint()(at.elapsed().as_secs_f32() / SLIDE.as_secs_f32());
                if coming { t } else { 1. - t }
            }
            _ => {
                if self.sidebar_open {
                    1.
                } else {
                    0.
                }
            }
        }
    }

    fn sidebar_moving(&self) -> bool {
        self.side_anim.is_some_and(|(_, at)| at.elapsed() < SLIDE)
    }

    /// The frame's theme: the sidebar stands on the frame, which is a dark
    /// graphite in both appearances, so it draws with a dark theme's inks
    /// whatever the page is.
    fn frame_theme(&self, cx: &App) -> gpui_component::Theme {
        let mut theme = cx.theme().clone();
        if !theme.mode.is_dark() {
            theme.foreground = rgb(0xF3F1EA).into();
            theme.muted_foreground = rgb(0xA09D94).into();
            theme.mode = gpui_component::theme::ThemeMode::Dark;
            theme.primary = Rgba::try_from(crate::look::on_frame(&self.cfg.app.accent)).map(Hsla::from).unwrap_or(theme.primary);
        }
        theme
    }

    /// The tree's rows, a folder's children under it in a fold that opens
    /// and closes in motion.
    fn render_entries(&self, entries: &[vault::Entry], depth: usize, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let dark = true;
        let open_note = self.note.as_ref().map(|n| n.path.clone());
        let folded = self.vault.as_ref().map(|v| &v.folded);
        let mut out = Vec::new();
        for e in entries {
            let path = e.path.clone();
            let is_open_folder = e.dir && !folded.is_some_and(|f| f.contains(&e.path));
            let chosen = open_note.as_ref() == Some(&e.path);
            let icon = crate::file_icons::path(&if e.dir { e.name.clone() } else { format!("{}.md", e.name) }, e.dir, is_open_folder, dark);
            let id = ElementId::Name(SharedString::from(format!("entry-{}", e.path.display())));
            let menu_path = path.clone();
            let row = h_flex()
                .id(id)
                .h(px(27.))
                .pl(px(8. + 14. * depth as f32))
                .pr(px(8.))
                .gap(px(6.))
                .items_center()
                .rounded(px(7.))
                .text_size(px(13.))
                .text_color(theme.foreground.opacity(if chosen { 1. } else { 0.88 }))
                .cursor_pointer()
                .when(chosen, |d| d.bg(theme.foreground.opacity(0.13)))
                .when(!chosen, |d| d.hover(|s| s.bg(theme.foreground.opacity(0.07))))
                .when(self.pressed.as_ref() == Some(&e.path), |d| d.bg(theme.foreground.opacity(0.1)))
                .child(div().w(px(12.)).flex_shrink_0().flex().justify_center().when(e.dir, |d| d.child(Icon::default().path(if is_open_folder { "icons/chevron-down.svg" } else { "icons/chevron-right.svg" }).with_size(px(12.)).text_color(theme.muted_foreground))))
                .child(img(icon).size(px(16.)).flex_shrink_0())
                .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().child(e.name.clone()))
                .on_click(cx.listener({
                    let path = path.clone();
                    let dir = e.dir;
                    move |this, _, window, cx| {
                        swallow_click(window, cx);
                        if dir {
                            if let Some(v) = this.vault.as_mut() {
                                if !v.folded.remove(&path) {
                                    v.folded.insert(path.clone());
                                }
                            }
                            this.save_ui(window);
                            cx.notify();
                        } else {
                            this.open_note(path.clone(), window, cx);
                        }
                    }
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                        swallow_click(window, cx);
                        this.pressed = Some(menu_path.clone());
                        let entries = this.entry_menu(&menu_path);
                        this.menus.show("entry", ev.position, entries);
                        cx.notify();
                    }),
                );
            out.push(row.into_any_element());
            if e.dir {
                if let Some(fold) = self.render_fold(e, depth, theme, is_open_folder, cx) {
                    out.push(fold);
                }
            }
        }
        out
    }

    /// A folder's children under its row: the box's height runs up from
    /// nothing and back, the content fading with it. The first sight
    /// plays nothing.
    fn render_fold(&self, e: &vault::Entry, depth: usize, theme: &gpui_component::Theme, open: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        let key = format!("fold-{}", e.path.display());
        let moving = self.motion.presence(&key, open, cx);
        if !open && moving.is_none() {
            return None;
        }
        let measured = self.fold_h.borrow_mut().entry(e.path.clone()).or_insert_with(|| Rc::new(Cell::new(0.))).clone();
        let entity = cx.entity().downgrade();
        let children = self.render_entries(&e.children, depth + 1, theme, cx);
        let content = v_flex()
            .relative()
            .flex_shrink_0()
            .children(children)
            .child(
                canvas(
                    {
                        let measured = measured.clone();
                        move |b, _, cx| {
                            let h = f32::from(b.size.height);
                            if (measured.get() - h).abs() > 0.5 {
                                measured.set(h);
                                let _ = entity.update(cx, |_, cx| cx.notify());
                            }
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            );
        let wrap = v_flex().overflow_hidden().child(content);
        Some(match moving {
            Some((opening, n)) => wrap
                .with_animation(ElementId::NamedInteger(SharedString::from(key), n as u64), Animation::new(FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| {
                    let t = if opening { t } else { 1. - t };
                    d.h(px(measured.get() * t)).opacity(t)
                })
                .into_any_element(),
            None => wrap.into_any_element(),
        })
    }

    fn render_sidebar(&self, width: Pixels, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.frame_theme(cx);
        let frame = cx.theme().title_bar;
        self.scrollers.name(&self.side_scroll);
        let head_button = |id: &'static str, icon: &'static str, label: SharedString, theme: &gpui_component::Theme| {
            let hover = theme.foreground.opacity(0.1);
            div()
                .id(id)
                .size(px(24.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .managed_tooltip(move |window, cx| tip(label.clone(), window, cx))
                .child(Icon::default().path(icon).with_size(px(15.)).text_color(theme.muted_foreground))
        };
        let vault_name: SharedString = self.vault.as_ref().map(|v| v.tree.name.clone()).unwrap_or_else(|| t("No vault").to_string()).into();
        let head = h_flex()
            .id("vault-head")
            .h(px(28.))
            .mx(px(8.))
            .px(px(8.))
            .gap(px(6.))
            .items_center()
            .rounded(px(7.))
            .cursor_pointer()
            .hover(|s| s.bg(theme.foreground.opacity(0.07)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, ev: &ClickEvent, window, cx| {
                swallow_click(window, cx);
                let at = ev.position() + point(px(-8.), px(14.));
                let mut entries = vec![Entry::choice("open-other", "icons/folder-open.svg", t("Open Vault…"))];
                if this.vault.is_some() {
                    entries.push(Entry::choice("reveal", "icons/folder.svg", t(crate::sys::REVEAL_LABEL)));
                    entries.push(Entry::Rule);
                    entries.push(Entry::choice("close", "icons/arrow-left.svg", t("Close Vault")));
                }
                this.menus.show("vault", at, entries);
                cx.notify();
            }))
            .child(Icon::default().path("icons/folder.svg").with_size(px(15.)).text_color(theme.muted_foreground))
            .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(theme.foreground).child(vault_name))
            .child(Icon::default().path("icons/chevrons-up-down.svg").with_size(px(12.)).text_color(theme.muted_foreground));
        let actions = h_flex()
            .px(px(14.))
            .pb(px(6.))
            .gap(px(2.))
            .items_center()
            .child(head_button("side-new-note", "icons/file-plus.svg", t("New Note (⌘N)").into(), &theme).on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.new_note(&NewNote, window, cx);
            })))
            .child(head_button("side-new-folder", "icons/folder-plus.svg", t("New Folder").into(), &theme).on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.new_folder(&NewFolder, window, cx);
            })));
        let rows = match &self.vault {
            Some(v) => self.render_entries(&v.tree.children, 0, &theme, cx),
            None => Vec::new(),
        };
        let empty = self.vault.as_ref().is_some_and(|v| v.tree.children.is_empty());
        // The room it takes of the row is `width`; the sidebar keeps its
        // own width inside it and slides in from the window's edge.
        div()
            .w(width)
            .h_full()
            .flex_shrink_0()
            .overflow_hidden()
            .child(
                v_flex()
                    .w(SIDEBAR_W)
                    .h_full()
                    .ml(width - SIDEBAR_W)
                    .pt(TITLEBAR_H + FRAME)
                    .child(div().px(px(16.)).pb(px(8.)).child(crate::widgets::wordmark("Onyx", 20., theme.primary, cx)))
                    .child(head)
                    .when(self.vault.is_some(), |d| d.child(actions))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .child(
                                v_flex()
                                    .id("side-list")
                                    .size_full()
                                    .px(px(8.))
                                    .pb(px(12.))
                                    .overflow_y_scroll()
                                    .track_scroll(&self.side_scroll)
                                    .children(rows)
                                    .when(empty, |d| d.child(div().px(px(10.)).py(px(8.)).text_size(px(12.)).text_color(theme.muted_foreground).child(t("No notes yet. Make one with the button above.")))),
                            )
                            // What is cut says so: the list fades into the
                            // frame at an edge with more beyond it.
                            .children(edge_fade(&self.side_scroll, frame, true))
                            .children(edge_fade(&self.side_scroll, frame, false))
                            .vertical_scrollbar(&self.side_scroll),
                    )
                    .child(h_flex().h(px(40.)).px(px(16.)).gap(px(8.)).items_center().text_size(px(12.)).text_color(theme.muted_foreground).child(t("Sidebar")).child(div().flex_1()).child(crate::widgets::kbd_hint("⌘⇧B", &theme))),
            )
            .into_any_element()
    }

    // -- the strip --------------------------------------------------------------

    /// The buttons beside the traffic lights, drawn once over the window's
    /// corner, so none moves whether the sidebar is there or not. Their
    /// centers are on the traffic lights'.
    fn render_strip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let over_side = self.side_t() > 0.5;
        let theme = if over_side { self.frame_theme(cx) } else { cx.theme().clone() };
        let button = |id: &'static str, icon: IconName, label: SharedString| {
            let hover = theme.foreground.opacity(0.07);
            div()
                .id(id)
                .size(STRIP_BTN)
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .managed_tooltip(move |window, cx| tip(label.clone(), window, cx))
                .child(Icon::new(icon).with_size(px(16.)).text_color(theme.muted_foreground))
        };
        let left = if cfg!(target_os = "macos") { TRAFFIC_W } else { px(10.) };
        h_flex()
            .absolute()
            .top(px(1.) + FRAME / 2.)
            .left(left)
            .h(TITLEBAR_H)
            .items_center()
            .child(button("strip-sidebar", IconName::PanelLeft, t("Sidebar (⌘⇧B)").into()).on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.toggle_sidebar(window, cx)
            })))
    }

    /// The window's top is the app's to drag by, and a button on it takes
    /// its press first (`on_click` above stops it).
    fn drag_region(el: Div) -> Div {
        el.on_mouse_down(MouseButton::Left, |ev, window, _| {
            if ev.click_count == 2 {
                window.titlebar_double_click();
            }
        })
        .on_mouse_move(|ev, window, _| {
            if ev.pressed_button == Some(MouseButton::Left) {
                window.start_window_move();
            }
        })
    }

    /// The right end of the strip: how the note is shown, and the settings.
    fn render_strip_right(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let mode = self.editor.read(cx).mode();
        let modes = self.motion.segmented(
            "mode",
            vec![("live", "icons/pencil-simple.svg", t("Edit").into()), ("source", "icons/file-code.svg", t("Source").into()), ("reading", "icons/book-open.svg", t("Read").into())],
            mode.key(),
            false,
            Rc::new(|this: &mut Self, key, window, cx| this.set_mode(Mode::from_key(key), window, cx)),
            cx,
        );
        let settings = div()
            .id("strip-settings")
            .size(STRIP_BTN)
            .rounded(px(6.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|s| s.bg(theme.foreground.opacity(0.07)))
            .managed_tooltip(|window, cx| tip(t("Settings (⌘,)"), window, cx))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.open_settings(None, window, cx)
            }))
            .child(Icon::new(IconName::Settings).with_size(px(16.)).text_color(theme.muted_foreground));
        h_flex().gap(px(10.)).items_center().when(self.note.is_some(), |d| d.child(div().on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).child(modes))).child(settings)
    }

    pub(crate) fn set_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |e, cx| e.set_mode(mode, cx));
        let mut ui = UiState::load();
        ui.mode = Some(mode.key().to_string());
        ui.save();
        if mode != Mode::Reading && self.note.is_some() {
            window.focus(&self.editor.read(cx).focus_handle(cx), cx);
        }
        cx.notify();
    }

    // -- the page ---------------------------------------------------------------

    /// With no vault: the way in. A vault is a folder of notes.
    fn render_landing(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let recent: Vec<PathBuf> = UiState::load().recent_vaults.into_iter().filter(|p| p.is_dir()).collect();
        let rows = recent.into_iter().enumerate().map(|(i, p)| {
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.display().to_string());
            let where_ = p.parent().map(|d| d.display().to_string()).unwrap_or_default();
            h_flex()
                .id(("recent", i))
                .h(px(44.))
                .px(px(14.))
                .gap(px(10.))
                .items_center()
                .cursor_pointer()
                .when(i > 0, |d| d.border_t_1().border_color(theme.border))
                .hover(|s| s.bg(theme.foreground.opacity(0.04)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    swallow_click(window, cx);
                    this.open_vault(p.clone(), window, cx)
                }))
                .child(Icon::default().path("icons/folder.svg").with_size(px(16.)).text_color(theme.muted_foreground))
                .child(v_flex().flex_1().min_w_0().child(div().text_size(px(13.)).child(name)).child(div().text_size(px(11.)).text_color(theme.muted_foreground).overflow_hidden().whitespace_nowrap().child(where_)))
        });
        let has_recent = !UiState::load().recent_vaults.iter().any(|p| p.is_dir());
        v_flex()
            .w(px(420.))
            .max_w_full()
            .mx_auto()
            .pt(px(80.))
            .px(px(24.))
            .gap(px(22.))
            .items_center()
            .child(crate::widgets::wordmark("Onyx", 34., theme.primary, cx))
            .child(div().text_size(px(13.5)).text_color(theme.muted_foreground).text_center().child(t("Notes live in a folder of your own, a vault, as plain markdown files.")))
            .child(choice_pill("open-vault", "icons/folder-open.svg", t("Open a vault…"), Press::Primary, false, &theme).on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.choose_vault(window, cx)
            })))
            .when(!has_recent, |d| {
                d.child(
                    v_flex()
                        .w_full()
                        .gap(px(6.))
                        .child(div().px(px(14.)).text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).child(t("Recent")))
                        .child(v_flex().w_full().rounded(px(12.)).border_1().border_color(theme.border).bg(if theme.mode.is_dark() { theme.sidebar } else { theme.muted.opacity(0.4) }).overflow_hidden().children(rows)),
                )
            })
            .into_any_element()
    }

    /// A vault with no note open.
    fn render_no_note(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        v_flex()
            .w_full()
            .pt(px(120.))
            .gap(px(16.))
            .items_center()
            .child(div().text_size(px(13.5)).text_color(theme.muted_foreground).child(t("No note is open.")))
            .child(choice_pill("new-note-page", "icons/file-plus.svg", t("New Note"), Press::Quiet, false, &theme).on_click(cx.listener(|this, _, window, cx| {
                swallow_click(window, cx);
                this.new_note(&NewNote, window, cx)
            })))
            .into_any_element()
    }

    fn render_note(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let readable = self.cfg.editor.readable_width;
        let scroll = self.editor.read(cx).scroll.clone();
        self.scrollers.name(&scroll);
        let theme = cx.theme().clone();
        let title: SharedString = self.note.as_ref().map(|n| vault::title(&n.path)).unwrap_or_default().into();
        let text_px = match self.cfg.editor.text_size.as_str() {
            "small" => 14.,
            "large" => 18.,
            _ => 16.,
        };
        let family = crate::fonts::text_family(&self.cfg.editor.text_face);
        div()
            .relative()
            .size_full()
            .child(
                div().id("note-scroll").size_full().overflow_y_scroll().track_scroll(&scroll).child(
                    v_flex()
                        .when(readable, |d| d.w(READABLE_W).max_w_full())
                        .when(!readable, |d| d.w_full())
                        .mx_auto()
                        .px(px(32.))
                        .pt(px(18.))
                        .pb(px(120.))
                        .gap(px(10.))
                        // The note's name over its words, in the words' face.
                        .child(div().font_family(family).text_size(px(text_px * 1.8)).font_weight(FontWeight::BOLD).line_height(relative(1.25)).text_color(theme.foreground).child(title))
                        .child(self.editor.clone()),
                ),
            )
            .children(edge_fade(&scroll, theme.background, true))
            .vertical_scrollbar(&scroll)
            .into_any_element()
    }

    fn render_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.vault.is_none() {
            self.scrollers.name(&self.page_scroll);
            let theme = cx.theme().clone();
            return div()
                .relative()
                .size_full()
                .child(div().id("page").size_full().overflow_y_scroll().track_scroll(&self.page_scroll).child(page_in("landing", div().child(self.render_landing(cx)))))
                .children(edge_fade(&self.page_scroll, theme.background, false))
                .vertical_scrollbar(&self.page_scroll)
                .into_any_element();
        }
        if self.note.is_none() {
            return page_in("no-note", div().size_full().child(self.render_no_note(cx)));
        }
        let key = self.note.as_ref().map(|n| format!("note-{}", n.path.display())).unwrap_or_default();
        page_in(key, div().size_full().child(self.render_note(cx)))
    }

    /// A rename: a card over the window with the name in a field. ↩ keeps
    /// it, Escape or a press off the card lets it go.
    fn render_rename(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let path = self.rename.as_ref()?;
        let theme = cx.theme().clone();
        let focus = self.rename_input.read(cx).focus_handle(cx);
        let title = if path.is_dir() { t("Rename Folder") } else { t("Rename Note") };
        Some(
            crate::widgets::fade_in(
                "rename-sheet",
                div()
                    .id("rename-scrim")
                    .absolute()
                    .inset_0()
                    .occlude()
                    .bg(theme.overlay)
                    .flex()
                    .flex_col()
                    .items_center()
                    .pt(px(140.))
                    .on_click(cx.listener(|this, _, window, cx| this.cancel_rename(window, cx)))
                    .child(
                        v_flex()
                            .id("rename-card")
                            .occlude()
                            .key_context(SHEET_CONTEXT)
                            .on_action(cx.listener(|this, _: &Escape, window, cx| this.cancel_rename(window, cx)))
                            .on_click(|_, window, cx| swallow_click(window, cx))
                            .w(px(420.))
                            .max_w(relative(0.9))
                            .p(px(16.))
                            .gap(px(10.))
                            .rounded(px(18.))
                            .bg(theme.popover)
                            .border_1()
                            .border_color(theme.border)
                            .shadow(float_shadow(&theme))
                            .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(title))
                            .child(div().id("rename-field").track_focus(&focus).px(px(10.)).h(px(36.)).flex().items_center().rounded(px(8.)).bg(theme.muted).text_size(px(14.)).child(Input::new(&self.rename_input).appearance(false)))
                            .child(h_flex().gap(px(8.)).justify_end().child(small_pill("rename-cancel", "icons/close.svg", t("Cancel"), Press::Quiet, false, &theme).on_click(cx.listener(|this, _, window, cx| this.cancel_rename(window, cx)))).child(small_pill("rename-ok", "icons/check.svg", t("Rename"), Press::Primary, false, &theme).on_click(cx.listener(|this, _, window, cx| this.finish_rename(window, cx)))))
                            .child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child(key_text(&t("↩ to rename, Esc to cancel")))),
                    ),
            )
            .into_any_element(),
        )
    }

    // -- the actions ------------------------------------------------------------

    pub(crate) fn new_note(&mut self, _: &NewNote, window: &mut Window, cx: &mut Context<Self>) {
        match self.new_note_dir() {
            Some(dir) => self.new_note_in(dir, window, cx),
            None => self.choose_vault(window, cx),
        }
    }

    fn new_folder(&mut self, _: &NewFolder, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(dir) = self.new_note_dir() {
            self.new_folder_in(dir, window, cx);
        }
    }

    // -- the settings and updates ------------------------------------------------

    pub fn open_settings(&mut self, section: Option<&'static str>, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(s) = section {
            self.settings_section = s;
        }
        // After this update: opening reads the view this call is inside.
        let weak = cx.entity().downgrade();
        cx.defer(move |cx| crate::settings::open(weak, cx));
        cx.notify();
    }

    fn check_updates_later(&self, cx: &mut Context<Self>) {
        if update::from_store() {
            return;
        }
        let state = update::UpdateState::load();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.);
        if now - state.last_check < 86_400. {
            return;
        }
        cx.spawn(async move |this, cx| {
            let _ = this.update(cx, |this, cx| this.check_updates(cx));
        })
        .detach();
    }

    /// Ask for the newest version, off the main thread.
    pub(crate) fn check_updates(&mut self, cx: &mut Context<Self>) {
        self.update.checking = true;
        self.update.said = None;
        cx.notify();
        let task = cx.background_executor().spawn(async { update::latest() });
        cx.spawn(async move |this, cx| {
            let found = task.await;
            let _ = this.update(cx, |this, cx| {
                this.update.checking = false;
                match found {
                    Ok(latest) => {
                        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.);
                        let _ = update::UpdateState { last_check: now, latest: latest.clone() }.save();
                        if update::is_newer(&latest, update::current_version()) {
                            this.update.found = Some(latest);
                        } else {
                            this.update.found = None;
                            this.update.said = Some(t("Up to date.").into());
                        }
                    }
                    Err(e) => this.update.said = Some(tf("The check did not go through: {why}", &[("why", &e)])),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Fetch the newest version and put it in place, then quit: the new
    /// app opens once this one has gone.
    pub(crate) fn install_update(&mut self, cx: &mut Context<Self>) {
        let Some(version) = self.update.found.clone() else { return };
        self.update.downloading = Some(0);
        cx.notify();
        let (tx, rx) = std::sync::mpsc::channel::<u32>();
        let task = cx.background_executor().spawn(async move {
            update::download(&version, &|done, total| {
                if let Some(total) = total.filter(|t| *t > 0) {
                    let _ = tx.send((done * 100 / total) as u32);
                }
            })
            .and_then(|file| update::install(&file))
        });
        cx.spawn(async move |this, cx| {
            let ticker = async {
                loop {
                    cx.background_executor().timer(Duration::from_millis(200)).await;
                    let p = rx.try_iter().last();
                    if this.update(cx, |this, cx| {
                        if let Some(p) = p {
                            this.update.downloading = Some(p);
                            cx.notify();
                        }
                    }).is_err() {
                        break;
                    }
                }
            };
            let done = futures_lite_select(task, ticker).await;
            let _ = this.update(cx, |this, cx| {
                this.update.downloading = None;
                match done {
                    Ok(()) => {
                        this.save_now(cx);
                        cx.quit()
                    }
                    Err(e) => this.update.said = Some(tf("The update did not go through: {e}", &[("e", &e)])),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

/// A fade at a scroller's top or foot, there while there is more that way.
/// It comes and goes with the scroll, which draws the view at each step.
fn edge_fade(handle: &ScrollHandle, ground: Hsla, top: bool) -> Option<AnyElement> {
    let (at, most) = (-handle.offset().y, handle.max_offset().y);
    let more = if top { at > px(1.) } else { at < most - px(1.) };
    more.then(|| {
        let (from, to) = if top { (ground, ground.opacity(0.)) } else { (ground.opacity(0.), ground) };
        div()
            .absolute()
            .left_0()
            .right_0()
            .h(px(28.))
            .map(|d| if top { d.top_0() } else { d.bottom_0() })
            .bg(linear_gradient(180., linear_color_stop(from, 0.), linear_color_stop(to, 1.)))
            .into_any_element()
    })
}

/// The first of a task and a loop that never ends: the task's answer.
async fn futures_lite_select<T: 'static>(task: Task<T>, ticker: impl std::future::Future<Output = ()>) -> T {
    use std::future::Future as _;
    use std::pin::pin;
    let mut task = pin!(task);
    let mut ticker = pin!(ticker);
    std::future::poll_fn(move |cx| {
        if let std::task::Poll::Ready(v) = task.as_mut().poll(cx) {
            return std::task::Poll::Ready(v);
        }
        let _ = ticker.as_mut().poll(cx);
        std::task::Poll::Pending
    })
    .await
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::editor::stamp("shell render begins");
        self.scrollers.begin();
        let theme = cx.theme().clone();
        let frame = theme.title_bar;
        crate::sys::set_window_ground(window, frame.to_rgb());
        if self.sidebar_moving() {
            window.request_animation_frame();
        }
        let t_side = self.side_t();
        let side_w = SIDEBAR_W * t_side;
        // The strip leaves room for the traffic lights and its button while
        // the sidebar is away, in proportion to the same clock.
        let strip_room = (if cfg!(target_os = "macos") { TRAFFIC_W } else { px(10.) } + STRIP_BTN + px(8.)) * (1. - t_side);
        let title: SharedString = self.note.as_ref().map(|n| vault::title(&n.path)).unwrap_or_default().into();
        let dirty = self.note.as_ref().is_some_and(|n| n.dirty);
        let notice = self.notice.as_ref().map(|(w, _)| w.clone());
        let right = self.render_strip_right(cx);
        let page = self.render_page(cx);
        let card = v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .rounded(CARD_R)
            .bg(theme.background)
            .text_color(theme.foreground)
            .overflow_hidden()
            .child(
                Self::drag_region(h_flex().h(TITLEBAR_H - FRAME).flex_shrink_0().pl(strip_room + px(12.)).pr(px(12.)).items_center().gap(px(8.)))
                    .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_size(px(12.5)).text_color(theme.muted_foreground).text_center().child(title))
                    .when(dirty, |d| d.child(div().size(px(6.)).rounded_full().bg(theme.muted_foreground.opacity(0.6))))
                    .child(right),
            )
            .children(notice.map(|words| div().px(px(16.)).py(px(6.)).text_size(px(12.5)).text_color(theme.danger).child(words)))
            .child(div().relative().flex_1().min_h_0().child(page));
        let pick = Rc::new(|this: &mut Self, what: &'static str, id: SharedString, window: &mut Window, cx: &mut Context<Self>| this.menu_pick(what, id, window, cx));
        let close = Rc::new(|this: &mut Self, cx: &mut Context<Self>| {
            this.pressed = None;
            this.menus.close(cx);
            cx.notify();
        });
        let menu = if self.menus.open.as_ref().is_some_and(|m| m.what == "entry" || m.what == "vault") || self.menus.gone.as_ref().is_some_and(|(m, _)| m.what == "entry" || m.what == "vault") { self.menus.render(pick, close, cx) } else { None };
        let rename = self.render_rename(cx);
        let this = cx.entity().downgrade();
        let ambient = self.ambient.clone();
        crate::editor::stamp("shell render ends");
        div()
            .id("shell")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .bg(frame)
            .on_action(cx.listener(|this, _: &ToggleSidebar, window, cx| this.toggle_sidebar(window, cx)))
            .on_action(cx.listener(Self::new_note))
            .on_action(cx.listener(Self::new_folder))
            .on_action(cx.listener(|this, _: &OpenVault, window, cx| this.choose_vault(window, cx)))
            .on_action(cx.listener(|this, _: &CloseVault, window, cx| this.close_vault(window, cx)))
            .on_action(cx.listener(|this, _: &Save, _, cx| this.save_now(cx)))
            .on_action(cx.listener(|this, _: &ShowLive, window, cx| this.set_mode(Mode::Live, window, cx)))
            .on_action(cx.listener(|this, _: &ShowSource, window, cx| this.set_mode(Mode::Source, window, cx)))
            .on_action(cx.listener(|this, _: &ShowReading, window, cx| this.set_mode(Mode::Reading, window, cx)))
            .on_action(cx.listener(|this, _: &ToggleReading, window, cx| {
                let mode = if this.editor.read(cx).mode() == Mode::Reading { Mode::Live } else { Mode::Reading };
                this.set_mode(mode, window, cx)
            }))
            .on_action(cx.listener(|this, _: &Escape, window, cx| {
                if this.rename.is_some() {
                    this.cancel_rename(window, cx);
                }
                this.pressed = None;
                this.menus.close(cx);
                cx.notify();
            }))
            // The scroll router, a raw listener in the capture phase,
            // registered at paint, before any scroller hears the wheel; and
            // the clock's step, asked for at the end of the frame.
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, _, window, cx| {
                        let router = this.clone();
                        window.on_mouse_event(move |e: &ScrollWheelEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture {
                                let taken = router.update(cx, |s, _| s.scrollers.route(e)).unwrap_or(false);
                                if taken {
                                    cx.stop_propagation();
                                    let _ = router.update(cx, |_, cx| cx.notify());
                                }
                            }
                        });
                        crate::ambient::after_frame(&ambient, window, cx);
                    },
                )
                .absolute()
                .size_0(),
            )
            .child(h_flex().size_full().items_stretch().child(self.render_sidebar(side_w, cx)).child(div().flex_1().min_w_0().h_full().p(FRAME).pl(if side_w > FRAME { px(0.) } else { FRAME - side_w }).child(card)))
            .child(self.render_strip(cx))
            .children(crate::csd::chrome(window, px(1.) + FRAME / 2. + TITLEBAR_H / 2. - px(12.), |window, _| window.remove_window(), cx))
            .children(menu)
            .children(rename)
            .child(self.ambient.clone())
    }
}

impl Shell {
    /// For a script: a picture of the window once it is drawn, then quit.
    /// The settings window when it is open, else the main one.
    pub fn shoot_later(&self, path: std::path::PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(1100)).await;
            let settings = this.update(cx, |s, _| s.settings).ok().flatten();
            let _ = cx.update(|window, cx| {
                let shot = match settings {
                    Some(h) => h.update(cx, |_, w, _| crate::sys::shoot_window(w, &path)).unwrap_or_else(|e| Err(e.to_string())),
                    None => crate::sys::shoot_window(window, &path),
                };
                if let Err(e) = shot {
                    eprintln!("onyx: ONYX_SHOT: {e}");
                }
                cx.quit();
            });
        })
        .detach();
    }

    /// For a script: steps by name (`ONYX_GO=note;sidebar;source`): the
    /// vault's first note opened, the sidebar folded, a mode, a row's menu.
    pub fn go(&mut self, steps: &str, window: &mut Window, cx: &mut Context<Self>) {
        for step in steps.split(';') {
            match step {
                "note" => {
                    if let Some(root) = self.vault_root().map(Path::to_path_buf) {
                        if let Some(first) = vault::notes(&root).into_iter().next() {
                            self.open_note(first, window, cx);
                        }
                    }
                }
                "sidebar" => self.toggle_sidebar(window, cx),
                "end" => self.editor.update(cx, |e, cx| e.move_to_end(cx)),
                "type" => self.editor.update(cx, |e, cx| e.type_text("Hello, probe. ", cx)),
                "live" => self.set_mode(Mode::Live, window, cx),
                "source" => self.set_mode(Mode::Source, window, cx),
                "reading" => self.set_mode(Mode::Reading, window, cx),
                "menu" => {
                    let first = self.vault.as_ref().and_then(|v| v.tree.children.iter().find(|e| !e.dir).map(|e| e.path.clone()));
                    if let Some(p) = first {
                        self.pressed = Some(p.clone());
                        let entries = self.entry_menu(&p);
                        self.menus.show("entry", point(px(120.), px(200.)), entries);
                    }
                }
                "rename" => {
                    let first = self.vault.as_ref().and_then(|v| v.tree.children.iter().find(|e| !e.dir).map(|e| e.path.clone()));
                    if let Some(p) = first {
                        self.start_rename(p, window, cx);
                    }
                }
                _ => {}
            }
        }
        cx.notify();
    }
}

impl Focusable for Shell {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
