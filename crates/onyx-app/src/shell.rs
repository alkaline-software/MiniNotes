//! The main window: one view (`Shell`) that draws the frame, the sidebar
//! on it, and the content as a card with rounded corners. The sidebar is
//! the vault's tree: its folders and notes, as the disk has them. The
//! card is the note open in the editor (`editor.rs`) under a strip with
//! its name and how it is shown, or, with no vault open, the landing
//! page. It also holds what the settings window draws (`settings.rs`),
//! so a change shows in both windows at once.
//!
//! Everything on the disk goes through `onyx_core::vault`: a note is
//! written whole when it is left (another note opens, the vault closes,
//! the window closes, the app quits) and at ⌘S, never while it is being
//! typed, and the vault's folder is watched so the tree and an untouched
//! note follow what another app does there.

use std::cell::Cell;
use std::collections::HashSet;
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
use crate::tabs::{Item as TabItem, Tabs};
use crate::ui_state::{Rect, UiState};
use crate::widgets::{choice_pill, float_shadow, key_text, page_in, small_pill, swallow_click, tip, Motion, Press, FOLD_ANIM, SLIDE};

actions!(onyx, [ToggleSidebar, OpenSettings, Escape, NewNote, NewFolder, NewVault, OpenVault, CloseVault, Save, ShowLive, ShowSource, ShowReading, ToggleReading, CloseTab, ReopenTab, Tab1, Tab2, Tab3, Tab4, Tab5, Tab6, Tab7, Tab8, Tab9]);

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

/// An open note, a tab: its file, its own editor (the text, the caret,
/// the scroll and the history stay with the tab while another shows),
/// whether the editor has what the disk has not, and the text as last
/// written, to see a change on the disk.
struct Tab {
    path: PathBuf,
    editor: Entity<Editor>,
    dirty: bool,
    saved: String,
    _sub: Subscription,
}

/// Where a new vault goes: a folder named for it under iCloud Drive's
/// `Onyx` folder, synced by the system, or a folder of the person's own.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    Cloud,
    Folder,
}

/// The new-vault sheet while it is up: the place chosen, and the folder
/// the vault goes in when the place is a folder of the person's own.
struct NewVaultSheet {
    place: Place,
    folder: PathBuf,
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
    /// The open notes, in the row's order: a folder's together.
    tabs: Vec<Tab>,
    /// The tab showing.
    shown: Option<PathBuf>,
    /// The tabs closed, the last closed last, for ⌘⇧T.
    closed_tabs: Vec<PathBuf>,
    /// The row's drawing and motion.
    pub(crate) tabs_row: Tabs,
    /// A press on the strip that a tab or a button took first, so the
    /// window is not moved by it; and the press that is the window's.
    pub(crate) press_taken: bool,
    win_move: bool,
    /// What every editor is set in, and how every note is shown.
    pub(crate) look: Look,
    mode: Mode,
    /// The color the window's own ground was last set to, so it is set
    /// once and not at every frame.
    ground: Cell<Option<Hsla>>,
    /// The row a right click was on.
    pressed: Option<PathBuf>,
    /// A rename under way: the entry, and the field for its name.
    rename: Option<PathBuf>,
    rename_input: Entity<InputState>,
    /// A new vault under way: the sheet, and the field for its name.
    new_vault: Option<NewVaultSheet>,
    vault_input: Entity<InputState>,
    /// Something to say under the strip, and when it was said.
    notice: Option<(String, Instant)>,
    /// When the window's bounds were last written.
    bounds_saved: Option<Instant>,
    pub(crate) settings: Option<AnyWindowHandle>,
    pub(crate) settings_section: &'static str,
    /// Text's page: `fonts` or `size`.
    pub(crate) settings_page: &'static str,
    /// The fields an installed font is named in, made with the settings
    /// window: the words', code's and Chinese's.
    pub(crate) font_inputs: Option<[Entity<InputState>; 3]>,
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

/// Every row of the tree is this tall.
const TREE_ROW_H: f32 = 27.;
/// Rows drawn beyond the list's box at either end, so a scroll of a few
/// rows shows no blank before the next frame.
const TREE_OVER: f32 = TREE_ROW_H * 6.;
/// Before the list's box has been painted, this much of the tree is
/// drawn from its top.
const TREE_FIRST: f32 = 4000.;

/// One piece of the tree as drawn (`Shell::render_tree`).
enum Piece<'a> {
    Row(&'a vault::Entry, usize),
    /// A folder's children on their way in or out: whether in, the
    /// change's number, and how far along, as a share of their height.
    Fold { folder: &'a vault::Entry, inside: Vec<Piece<'a>>, opening: bool, serial: u32, at: f32 },
}

impl Piece<'_> {
    /// The rows in it, folds and all.
    fn rows(&self) -> usize {
        match self {
            Piece::Row(..) => 1,
            Piece::Fold { inside, .. } => inside.iter().map(Piece::rows).sum(),
        }
    }

    /// What it stands for in the list now.
    fn height(&self) -> f32 {
        match self {
            Piece::Row(..) => TREE_ROW_H,
            Piece::Fold { at, .. } => self.rows() as f32 * TREE_ROW_H * at,
        }
    }
}

/// The key a folder's fold has with `Motion::presence`.
fn fold_key(path: &Path) -> String {
    format!("fold-{}", path.display())
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
        let rename_input = cx.new(|cx| InputState::new(window, cx));
        cx.subscribe_in(&rename_input, window, |this, _, ev: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = ev {
                this.finish_rename(window, cx);
            }
        })
        .detach();
        let vault_input = cx.new(|cx| InputState::new(window, cx));
        cx.subscribe_in(&vault_input, window, |this, _, ev: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = ev {
                this.finish_new_vault(window, cx);
            }
        })
        .detach();
        // A quit writes every note with changes first, and waits for
        // what the window remembers to be on the disk.
        cx.on_app_quit(|this, cx| {
            this.save_all(cx);
            UiState::flush();
            async {}
        })
        .detach();
        let mut cfg = Config::load();
        // The faces' keys before v0.5.0's Text page: each goes to the face
        // that took its place.
        match cfg.editor.text_face.as_str() {
            "system" => cfg.editor.text_face = "inter".into(),
            _ => {}
        }
        match cfg.editor.mono_face.as_str() {
            "jetbrains-mono" => cfg.editor.mono_face = "jetbrains".into(),
            "system-mono" => cfg.editor.mono_face = crate::fonts::MONOS[1].key.into(),
            _ => {}
        }
        let mut shell = Self {
            look: Self::look_of(&cfg.editor),
            mode: Mode::from_key(ui.mode.as_deref().unwrap_or("live")),
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
            tabs: Vec::new(),
            shown: None,
            closed_tabs: Vec::new(),
            tabs_row: Tabs::default(),
            press_taken: false,
            win_move: false,
            ground: Cell::new(None),
            pressed: None,
            rename: None,
            rename_input,
            new_vault: None,
            vault_input,
            notice: None,
            bounds_saved: None,
            settings: None,
            settings_section: crate::settings::section_named(ui.settings_section.as_deref().unwrap_or("appearance")),
            settings_page: "fonts",
            font_inputs: None,
            settings_scroll: ScrollHandle::new(),
            lang_serial: 0,
            update: UpdateRow::default(),
        };
        // A probe's vault stands over the remembered one.
        let probing = std::env::var_os("ONYX_VAULT").is_some();
        let vault = std::env::var_os("ONYX_VAULT").map(PathBuf::from).or_else(|| ui.vault.clone());
        if let Some(root) = vault.filter(|p| p.is_dir()) {
            shell.open_vault(root, window, cx);
            // A probe names a note to open (`ONYX_NOTE`); otherwise the
            // tabs that were open are opened again, and the one that
            // showed shows.
            let probe_note = std::env::var_os("ONYX_NOTE").map(PathBuf::from).filter(|p| p.is_file());
            match probe_note {
                Some(note) => shell.open_note(note, window, cx),
                None if !probing => {
                    for path in ui.tabs.iter().filter(|p| p.is_file()) {
                        shell.open_note(path.clone(), window, cx);
                    }
                    if let Some(note) = ui.note.filter(|p| p.is_file()) {
                        shell.open_note(note, window, cx);
                    }
                }
                None => {}
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

    /// What the window remembers, as it is now.
    fn ui_state(&self, window: &Window) -> UiState {
        let mut ui = UiState::load();
        // `window_bounds()`, never `bounds()` (`ui_state.rs`).
        if let WindowBounds::Windowed(b) = window.window_bounds() {
            ui.window = Some(Rect { x: f32::from(b.origin.x), y: f32::from(b.origin.y), w: f32::from(b.size.width), h: f32::from(b.size.height) });
        }
        ui.sidebar_open = Some(self.sidebar_open);
        ui.settings_section = Some(self.settings_section.to_string());
        if std::env::var_os("ONYX_VAULT").is_none() {
            ui.vault = self.vault.as_ref().map(|v| v.root.clone());
            ui.note = self.shown.clone();
            ui.tabs = self.tabs.iter().map(|t| t.path.clone()).collect();
            ui.folded = self.vault.as_ref().map(|v| v.folded.iter().cloned().collect()).unwrap_or_default();
        }
        ui
    }

    /// What the window remembers, handed to its writer
    /// (`UiState::save_later`): a frame never waits on the disk for a
    /// tab moved or a folder folded.
    pub fn save_ui(&self, window: &Window) {
        self.ui_state(window).save_later();
    }

    /// Written before this returns, for the window's close, which on
    /// Windows and Linux ends the app.
    pub fn save_ui_now(&self, window: &Window) {
        self.ui_state(window).save();
    }

    /// The faces and size the editors are set in, from the settings.
    fn look_of(e: &onyx_core::config::EditorConfig) -> Look {
        let sizes = e.text_sizes();
        let (text_family, mono_family) = (crate::fonts::text_family(&e.text_face, &e.text_custom), crate::fonts::mono_family(&e.mono_face, &e.mono_custom));
        let (text_fallbacks, mono_fallbacks) = (Some(crate::fonts::cjk_fallbacks(&text_family, &e.cjk_face, &e.cjk_custom)), Some(crate::fonts::cjk_fallbacks(&mono_family, &e.cjk_face, &e.cjk_custom)));
        Look { text_family, mono_family, text_px: sizes.note, mono_px: sizes.code, text_fallbacks, mono_fallbacks }
    }

    /// The settings changed: every editor takes the faces and size.
    pub(crate) fn push_look(&mut self, cx: &mut Context<Self>) {
        self.look = Self::look_of(&self.cfg.editor);
        let look = self.look.clone();
        for tab in &self.tabs {
            tab.editor.update(cx, |ed, cx| ed.set_look(look.clone(), cx));
        }
    }

    // -- the vault ------------------------------------------------------------

    pub(crate) fn vault_root(&self) -> Option<&Path> {
        self.vault.as_ref().map(|v| v.root.as_path())
    }

    fn open_vault(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.save_all(cx);
        self.tabs.clear();
        self.shown = None;
        self.closed_tabs.clear();
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
        ui.save_later();
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
        if !paths.iter().any(|p| Self::change_matters(&root, p)) {
            return;
        }
        // The open notes the change touched, read again with the tree.
        let touched: Vec<PathBuf> = self.tabs.iter().map(|t| t.path.clone()).filter(|t| paths.iter().any(|p| p == t)).collect();
        let task = cx.background_executor().spawn(async move {
            let tree = vault::tree(&root);
            let texts: Vec<(PathBuf, Option<String>)> = touched.into_iter().map(|p| (p.clone(), vault::read(&p).ok())).collect();
            (tree, texts)
        });
        cx.spawn(async move |this, cx| {
            let (tree, texts) = task.await;
            let _ = this.update(cx, |s, cx| {
                if let Some(v) = s.vault.as_mut() {
                    v.tree = tree;
                }
                for (path, text) in texts {
                    match text {
                        // An untouched note takes what the disk has.
                        Some(text) => {
                            if let Some(tab) = s.tabs.iter_mut().find(|t| t.path == path) {
                                if !tab.dirty && tab.saved != text {
                                    tab.saved = text.clone();
                                    tab.editor.update(cx, |e, cx| e.replace_text_quietly(&text, cx));
                                }
                            }
                        }
                        // A note gone from the disk is closed.
                        None if !path.exists() => s.drop_tab(&path, cx),
                        None => {}
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Whether a change on the disk can show in the window: a note, a
    /// folder, or something gone whose name does not say it was neither.
    /// A picture copied in, a sync's bookkeeping, Obsidian's own folder
    /// and a temp file cost nothing: no reading, no frame.
    fn change_matters(root: &Path, path: &Path) -> bool {
        let hidden = path.strip_prefix(root).ok().is_some_and(|rest| rest.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.')));
        if hidden {
            return false;
        }
        vault::is_note(path) || path.is_dir() || (!path.exists() && path.extension().is_none())
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
        self.save_all(cx);
        self.tabs.clear();
        self.shown = None;
        self.closed_tabs.clear();
        self.vault = None;
        let mut ui = UiState::load();
        ui.vault = None;
        ui.note = None;
        ui.tabs.clear();
        ui.save_later();
        self.save_ui(window);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    // -- a new vault ------------------------------------------------------------

    /// The new-vault sheet: a name, and where it goes. iCloud Drive is the
    /// first choice where the machine has it, else the documents folder.
    fn start_new_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menus.close(cx);
        let place = if Self::cloud_place().is_some() { Place::Cloud } else { Place::Folder };
        self.new_vault = Some(NewVaultSheet { place, folder: onyx_core::paths::documents() });
        self.vault_input.update(cx, |i, cx| {
            i.set_value(t("Notes"), window, cx);
            i.select_all(window, cx);
        });
        window.focus(&self.vault_input.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    /// Where a vault goes in iCloud: Onyx's own app library when the
    /// bundle is signed for one (`sys::icloud_container`), else an `Onyx`
    /// folder at iCloud Drive's root; nothing where the machine has no
    /// iCloud Drive.
    fn cloud_place() -> Option<PathBuf> {
        crate::sys::icloud_container().or_else(|| onyx_core::paths::icloud_drive().map(|d| d.join("Onyx")))
    }

    /// Where a vault named `name` would go for the sheet's choice.
    fn new_vault_parent(sheet: &NewVaultSheet) -> PathBuf {
        match sheet.place {
            Place::Cloud => Self::cloud_place().unwrap_or_else(onyx_core::paths::documents),
            Place::Folder => sheet.folder.clone(),
        }
    }

    /// Ask the system for the folder a new vault goes in.
    fn pick_vault_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some(t("Choose").into()) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(dir) = paths.into_iter().next() else { return };
            let _ = cx.update(|window, cx| {
                let _ = this.update(cx, |s, cx| {
                    if let Some(sheet) = s.new_vault.as_mut() {
                        sheet.folder = dir;
                        sheet.place = Place::Folder;
                    }
                    window.focus(&s.vault_input.read(cx).focus_handle(cx), cx);
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn finish_new_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(sheet) = self.new_vault.take() else { return };
        let name = self.vault_input.read(cx).value().to_string();
        match vault::new_vault(&Self::new_vault_parent(&sheet), &name) {
            Ok(root) => self.open_vault(root, window, cx),
            Err(e) => {
                self.say(tf("The vault was not made: {why}", &[("why", &e)]), cx);
                self.focus_editor_or_shell(window, cx);
            }
        }
        cx.notify();
    }

    fn cancel_new_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.new_vault = None;
        self.focus_editor_or_shell(window, cx);
        cx.notify();
    }

    // -- the notes: the tabs ----------------------------------------------------

    /// The tab showing, and its editor.
    fn shown_tab(&self) -> Option<&Tab> {
        let shown = self.shown.as_ref()?;
        self.tabs.iter().find(|t| t.path == *shown)
    }

    pub(crate) fn shown_editor(&self) -> Option<Entity<Editor>> {
        self.shown_tab().map(|t| t.editor.clone())
    }

    /// An editor for a note, set as every editor is, and listened to.
    fn make_editor(&self, path: &Path, text: &str, cx: &mut Context<Self>) -> (Entity<Editor>, Subscription) {
        let editor = cx.new(Editor::new);
        let look = self.look.clone();
        let mode = self.mode;
        editor.update(cx, |e, cx| {
            e.set_look(look, cx);
            e.set_mode(mode, cx);
            e.set_text(text, cx);
        });
        let path = path.to_path_buf();
        let sub = cx.subscribe(&editor, move |this, _, ev: &EditorEvent, cx| match ev {
            EditorEvent::Changed => this.note_changed(&path, cx),
            EditorEvent::OpenLink(url) => this.open_link(url, cx),
            EditorEvent::OpenNote(name) => this.open_wikilink(name, cx),
        });
        (editor, sub)
    }

    /// A note opened: its tab shows, made if it was not open, at the end
    /// of its folder's pill.
    fn open_note(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.iter().any(|t| t.path == path) {
            self.show_tab(&path, window, cx);
            return;
        }
        match vault::read(&path) {
            Ok(text) => {
                let (editor, sub) = self.make_editor(&path, &text, cx);
                self.tabs.push(Tab { path: path.clone(), editor, dirty: false, saved: text, _sub: sub });
                self.closed_tabs.retain(|c| *c != path);
                self.sort_tabs();
                self.show_tab(&path, window, cx);
            }
            Err(e) => {
                self.say(tf("The note could not be read: {why}", &[("why", &e)]), cx);
                cx.notify();
            }
        }
    }

    /// The row's order: a folder's tabs together (`Tabs::group`).
    fn sort_tabs(&mut self) {
        let order = Tabs::order(&Tabs::group(&self.tabs.iter().map(|t| t.path.clone()).collect::<Vec<_>>()));
        self.tabs.sort_by_key(|t| order.iter().position(|p| *p == t.path).unwrap_or(usize::MAX));
    }

    /// The order a drag gave the row (`Tabs::drag_tab_to`,
    /// `Tabs::drag_pill_to`), kept and remembered.
    pub(crate) fn reorder_tabs(&mut self, order: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.sort_by_key(|t| order.iter().position(|p| *p == t.path).unwrap_or(usize::MAX));
        self.save_ui(window);
        cx.notify();
    }

    /// The tab shows, and the keyboard goes to its editor.
    pub(crate) fn show_tab(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter().find(|t| t.path == path) else { return };
        self.shown = Some(path.to_path_buf());
        // The note comes up with no caret: the keyboard is the window's
        // until a press in the words asks for it.
        let _ = tab;
        window.focus(&self.focus, cx);
        self.save_ui(window);
        cx.notify();
    }

    /// A tab closed: its note is written when it has changes, and the
    /// next tab shows in its place, or with none left the vault's page.
    pub(crate) fn close_tab(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.tabs.iter().position(|t| t.path == path) else { return };
        self.save_tab(ix, cx);
        self.tabs.remove(ix);
        self.closed_tabs.retain(|c| c != path);
        self.closed_tabs.push(path.to_path_buf());
        if self.shown.as_deref() == Some(path) {
            self.shown = None;
            match self.tabs.get(ix.min(self.tabs.len().saturating_sub(1))).map(|t| t.path.clone()) {
                Some(next) => self.show_tab(&next, window, cx),
                None => window.focus(&self.focus, cx),
            }
        }
        self.save_ui(window);
        cx.notify();
    }

    /// A tab let go without a write: its file is gone, or on its way to
    /// the trash with what was written already.
    fn drop_tab(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(ix) = self.tabs.iter().position(|t| t.path == path) else { return };
        self.tabs.remove(ix);
        if self.shown.as_deref() == Some(path) {
            self.shown = self.tabs.get(ix.min(self.tabs.len().saturating_sub(1))).map(|t| t.path.clone());
        }
        cx.notify();
    }

    /// ⌘W: the tab showing closes; with none, the window.
    fn close_nearest(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.shown.clone() {
            Some(path) => self.close_tab(&path, window, cx),
            None => {
                self.save_all(cx);
                self.save_ui(window);
                window.remove_window();
            }
        }
    }

    /// ⌘⇧T: the tab closed last is open again and showing, and at each
    /// further press the one closed before it. One whose note is gone
    /// since, or that is open again, is passed over.
    fn reopen_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while let Some(path) = self.closed_tabs.pop() {
            if !self.tabs.iter().any(|t| t.path == path) && path.is_file() {
                self.open_note(path, window, cx);
                return;
            }
        }
    }

    /// ⌘1 to ⌘9: that tab in the row's order, or the last when there
    /// are fewer.
    fn go_tab(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(n.saturating_sub(1).min(self.tabs.len().saturating_sub(1))) else { return };
        let path = tab.path.clone();
        self.show_tab(&path, window, cx);
    }

    /// An editor's text changed: its note has what the disk has not, and
    /// its tab shows the dot. Nothing is written until the note is closed
    /// or ⌘S: a note typed for an hour is one write, not a thousand.
    fn note_changed(&mut self, path: &Path, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.path == path) {
            if !tab.dirty {
                tab.dirty = true;
                cx.notify();
            }
        }
    }

    /// One tab's note written now, when it has changes.
    fn save_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(ix) else { return };
        if !tab.dirty {
            return;
        }
        let text = tab.editor.read(cx).text();
        match vault::write(&tab.path, &text) {
            Ok(()) => {
                tab.dirty = false;
                tab.saved = text;
            }
            Err(e) => {
                let why = e.to_string();
                self.say(tf("The note could not be written: {why}", &[("why", &why)]), cx);
            }
        }
        cx.notify();
    }

    /// ⌘S: the note showing, written now.
    pub(crate) fn save_now(&mut self, cx: &mut Context<Self>) {
        if let Some(ix) = self.shown.as_ref().and_then(|s| self.tabs.iter().position(|t| t.path == *s)) {
            self.save_tab(ix, cx);
        }
    }

    /// Every note with changes written: before the vault closes or
    /// changes, when the window closes, at quit.
    pub(crate) fn save_all(&mut self, cx: &mut Context<Self>) {
        for ix in 0..self.tabs.len() {
            self.save_tab(ix, cx);
        }
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
        Some(self.shown.as_ref().and_then(|n| n.parent()).map(Path::to_path_buf).unwrap_or_else(|| root.to_path_buf()))
    }

    fn open_link(&mut self, url: &str, cx: &mut Context<Self>) {
        if url.contains("://") || url.starts_with("mailto:") {
            crate::sys::open(url);
            return;
        }
        // A path relative to the note: a note of the vault opens here,
        // anything else with the system's app.
        let Some(base) = self.shown.as_ref().and_then(|n| n.parent()).map(Path::to_path_buf) else { return };
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
        let from = self.shown.clone().unwrap_or_else(|| root.clone());
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
        match vault::rename(&path, &name) {
            Ok(new_path) => {
                // An open note follows its new path, and so does one
                // inside a renamed folder. The tab's editor listens by
                // the old path, so it is made to listen by the new one.
                let moved: Vec<(usize, PathBuf)> = self.tabs.iter().enumerate().filter_map(|(ix, t)| if t.path == path { Some((ix, new_path.clone())) } else { t.path.strip_prefix(&path).ok().map(|rest| (ix, new_path.join(rest))) }).collect();
                for (ix, to) in moved {
                    let editor = self.tabs[ix].editor.clone();
                    let at = to.clone();
                    let sub = cx.subscribe(&editor, move |this, _, ev: &EditorEvent, cx| match ev {
                        EditorEvent::Changed => this.note_changed(&at, cx),
                        EditorEvent::OpenLink(url) => this.open_link(url, cx),
                        EditorEvent::OpenNote(name) => this.open_wikilink(name, cx),
                    });
                    if self.shown.as_ref() == Some(&self.tabs[ix].path) {
                        self.shown = Some(to.clone());
                    }
                    self.tabs[ix].path = to;
                    self.tabs[ix]._sub = sub;
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
        match self.shown_editor().filter(|_| self.mode != Mode::Reading) {
            Some(editor) => window.focus(&editor.read(cx).focus_handle(cx), cx),
            None => window.focus(&self.focus, cx),
        }
    }

    fn trash(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        // An open note going to the trash is written first, so what was
        // typed goes with it, and its tab closes.
        let going: Vec<PathBuf> = self.tabs.iter().map(|t| t.path.clone()).filter(|p| *p == path || p.starts_with(&path)).collect();
        for p in going {
            self.close_tab(&p, window, cx);
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
            ("vault", "new") => self.start_new_vault(window, cx),
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

    /// The tree as drawn: a row for each entry in view, a folder's
    /// children under its row, and the rows out of view as one empty box
    /// above and one below, of their height, so the list keeps its length
    /// and the scroll its place. Every row is `TREE_ROW_H` tall, so where
    /// each stands is a sum and nothing is measured; a vault of a thousand
    /// notes costs the frame no more than the rows in view.
    fn render_tree(&self, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(v) = &self.vault else { return Vec::new() };
        let mut pieces = Vec::new();
        self.tree_pieces(&v.tree.children, 0, cx, &mut pieces);
        // The list's box as last painted; before the first paint, more
        // rows than any sidebar holds.
        let view = f32::from(self.side_scroll.bounds().size.height);
        let top = -f32::from(self.side_scroll.offset().y);
        let (from, to) = if view > 0. { (top - TREE_OVER, top + view + TREE_OVER) } else { (0., TREE_FIRST) };
        let mut out = Vec::new();
        let (mut y, mut gap) = (0., 0.);
        for p in &pieces {
            let h = p.height();
            if y + h > from && y < to {
                if gap > 0. {
                    out.push(div().h(px(gap)).flex_shrink_0().into_any_element());
                    gap = 0.;
                }
                out.push(self.render_piece(p, theme, cx));
            } else {
                gap += h;
            }
            y += h;
        }
        if gap > 0. {
            out.push(div().h(px(gap)).flex_shrink_0().into_any_element());
        }
        out
    }

    /// The tree flattened: a row for each entry, an open folder's
    /// children after its row, and a folder's on their way in or out as
    /// one piece, so the fold is drawn as a box wherever it stands.
    fn tree_pieces<'a>(&self, entries: &'a [vault::Entry], depth: usize, cx: &mut Context<Self>, out: &mut Vec<Piece<'a>>) {
        let folded = self.vault.as_ref().map(|v| &v.folded);
        for e in entries {
            out.push(Piece::Row(e, depth));
            if !e.dir {
                continue;
            }
            let open = !folded.is_some_and(|f| f.contains(&e.path));
            let key = fold_key(&e.path);
            match self.motion.presence(&key, open, cx) {
                Some((opening, serial)) => {
                    let mut inside = Vec::new();
                    self.tree_pieces(&e.children, depth + 1, cx, &mut inside);
                    let t = self.motion.progress(&key);
                    out.push(Piece::Fold { folder: e, inside, opening, serial, at: if opening { t } else { 1. - t } });
                }
                None if open => self.tree_pieces(&e.children, depth + 1, cx, out),
                None => {}
            }
        }
    }

    fn render_piece(&self, piece: &Piece, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> AnyElement {
        match piece {
            Piece::Row(e, depth) => self.render_row(e, *depth, theme, cx),
            // A folder's children under its row, in a box that runs up to
            // their height and back, the content fading with it, keyed on
            // the folder's path so each change plays once and the first
            // sight plays nothing (`Motion::presence`).
            Piece::Fold { folder, inside, opening, serial, .. } => {
                let full = inside.iter().map(Piece::rows).sum::<usize>() as f32 * TREE_ROW_H;
                let opening = *opening;
                let children: Vec<AnyElement> = inside.iter().map(|p| self.render_piece(p, theme, cx)).collect();
                v_flex()
                    .flex_shrink_0()
                    .overflow_hidden()
                    .child(v_flex().flex_shrink_0().children(children))
                    .with_animation(ElementId::NamedInteger(SharedString::from(fold_key(&folder.path)), *serial as u64), Animation::new(FOLD_ANIM).with_easing(ease_out_quint()), move |d, t| {
                        let t = if opening { t } else { 1. - t };
                        d.h(px(full * t)).opacity(t)
                    })
                    .into_any_element()
            }
        }
    }

    /// A row: the chevron (a folder), Catppuccin's icon, the name, on a
    /// plate while it is the open note. Every row and every fold is
    /// `flex_shrink_0`: a scroll box's children shrink to fit it
    /// otherwise, and a tree taller than the sidebar drew its folders
    /// empty and its rows squeezed.
    fn render_row(&self, e: &vault::Entry, depth: usize, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> AnyElement {
        let dark = true;
        let folded = self.vault.as_ref().map(|v| &v.folded);
        let path = e.path.clone();
        let is_open_folder = e.dir && !folded.is_some_and(|f| f.contains(&e.path));
        let chosen = self.shown.as_ref() == Some(&e.path);
        let icon = crate::file_icons::path(&if e.dir { e.name.clone() } else { format!("{}.md", e.name) }, e.dir, is_open_folder, dark);
        let id = ElementId::Name(SharedString::from(format!("entry-{}", e.path.display())));
        let menu_path = path.clone();
            let row = h_flex()
                .id(id)
                .h(px(TREE_ROW_H))
                .flex_shrink_0()
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
        row.into_any_element()
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
                let mut entries = vec![Entry::choice("new", "icons/plus.svg", t("New Vault…")), Entry::choice("open-other", "icons/folder-open.svg", t("Open Vault…"))];
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
        let rows = self.render_tree(&theme, cx);
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

    /// The window's top is the app's to drag by: a press on it moves the
    /// window once the pointer moves with the button down, unless a tab
    /// or a button on the strip took the press first (`press_taken`: a
    /// child's listener runs before its parent's, and a tab's press is
    /// the start of its own drag), and a double click is the title bar's.
    fn drag_region(el: Div, cx: &mut Context<Self>) -> Div {
        el.on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, ev: &MouseDownEvent, window, _| {
                this.win_move = !std::mem::take(&mut this.press_taken);
                if this.win_move && ev.click_count == 2 {
                    this.win_move = false;
                    window.titlebar_double_click();
                }
            }),
        )
        .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, window, _| {
            if !this.win_move {
                return;
            }
            this.win_move = false;
            if ev.pressed_button == Some(MouseButton::Left) {
                window.start_window_move();
            }
        }))
    }

    /// The right end of the strip: how the note is shown, and the settings.
    fn render_strip_right(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let mode = self.mode;
        let modes = self.motion.segmented(
            "mode",
            vec![("live", "icons/pencil-simple.svg", t("Edit (⌥1)").into()), ("source", "icons/file-code.svg", t("Source (⌥2)").into()), ("reading", "icons/book-open.svg", t("Read (⌥3)").into())],
            mode.key(),
            false,
            true,
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
        h_flex().gap(px(10.)).items_center().when(self.shown.is_some(), |d| d.child(div().on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).child(modes))).child(settings)
    }

    pub(crate) fn set_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = mode;
        for tab in &self.tabs {
            tab.editor.update(cx, |e, cx| e.set_mode(mode, cx));
        }
        let mut ui = UiState::load();
        ui.mode = Some(mode.key().to_string());
        ui.save_later();
        self.focus_editor_or_shell(window, cx);
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
            .child(
                h_flex()
                    .gap(px(10.))
                    .child(choice_pill("new-vault", "icons/plus.svg", t("New Vault…"), Press::Primary, false, &theme).on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.start_new_vault(window, cx)
                    })))
                    .child(choice_pill("open-vault", "icons/folder-open.svg", t("Open Vault…"), Press::Quiet, false, &theme).on_click(cx.listener(|this, _, window, cx| {
                        swallow_click(window, cx);
                        this.choose_vault(window, cx)
                    }))),
            )
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

    fn render_note(&mut self, editor: Entity<Editor>, path: &Path, cx: &mut Context<Self>) -> AnyElement {
        let readable = self.cfg.editor.readable_width;
        let scroll = editor.read(cx).scroll.clone();
        self.scrollers.name(&scroll);
        let theme = cx.theme().clone();
        let title: SharedString = vault::title(path).into();
        let text_px = self.cfg.editor.text_sizes().note;
        let family = self.look.text_family.clone();
        let title_font = Font { family: family.clone(), features: FontFeatures::default(), fallbacks: self.look.text_fallbacks.clone(), weight: FontWeight::BOLD, style: FontStyle::Normal };
        div()
            .relative()
            .size_full()
            .child(
                // A press in the margin, off the words, takes the keyboard
                // out of the editor: the note reads whole, with no caret.
                div().id("note-scroll").size_full().overflow_y_scroll().track_scroll(&scroll).on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| {
                    crate::editor::stamp("margin press");
                    window.focus(&this.focus, cx);
                    cx.notify();
                })).child(
                    v_flex()
                        .when(readable, |d| d.w(READABLE_W).max_w_full())
                        .when(!readable, |d| d.w_full())
                        .mx_auto()
                        .px(px(32.))
                        .pt(px(18.))
                        .pb(px(120.))
                        .gap(px(10.))
                        // The note's name over its words, in the words' face.
                        .child(div().font(title_font).text_size(px(text_px * 1.8)).line_height(relative(1.25)).text_color(theme.foreground).child(title))
                        .child(editor),
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
        let Some((editor, path)) = self.shown_tab().map(|t| (t.editor.clone(), t.path.clone())) else {
            return page_in("no-note", div().size_full().child(self.render_no_note(cx)));
        };
        let key = format!("note-{}", path.display());
        page_in(key, div().size_full().child(self.render_note(editor, &path, cx)))
    }

    /// A sheet: a card over a scrim, with a title, what the sheet asks,
    /// and a line of keys under it. Esc or a press off the card lets it go.
    /// The card occludes, so a press in it let go outside does not close it.
    fn render_sheet(&self, id: &'static str, title: SharedString, body: Vec<AnyElement>, hint: SharedString, cancel: fn(&mut Self, &mut Window, &mut Context<Self>), cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        crate::widgets::fade_in(
            id,
            div()
                .id("sheet-scrim")
                .absolute()
                .inset_0()
                .occlude()
                .bg(theme.overlay)
                .flex()
                .flex_col()
                .items_center()
                .pt(px(140.))
                .on_click(cx.listener(move |this, _, window, cx| cancel(this, window, cx)))
                .child(
                    v_flex()
                        .id("sheet-card")
                        .occlude()
                        .key_context(SHEET_CONTEXT)
                        .on_action(cx.listener(move |this, _: &Escape, window, cx| cancel(this, window, cx)))
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
                        .children(body)
                        .child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child(key_text(&hint))),
                ),
        )
        .into_any_element()
    }

    /// A field on a sheet: the toolkit's input on a plate, keyed to the
    /// window it was made in.
    fn sheet_field(id: &'static str, input: &Entity<InputState>, theme: &gpui_component::Theme, cx: &mut Context<Self>) -> AnyElement {
        let focus = input.read(cx).focus_handle(cx);
        div().id(id).track_focus(&focus).px(px(10.)).h(px(36.)).flex().items_center().rounded(px(8.)).bg(theme.muted).text_size(px(14.)).child(Input::new(input).appearance(false)).into_any_element()
    }

    /// A rename: the name in a field. ↩ keeps it.
    fn render_rename(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let path = self.rename.as_ref()?;
        let theme = cx.theme().clone();
        let title = if path.is_dir() { t("Rename Folder") } else { t("Rename Note") };
        let body = vec![
            Self::sheet_field("rename-field", &self.rename_input, &theme, cx),
            h_flex()
                .gap(px(8.))
                .justify_end()
                .child(small_pill("rename-cancel", "icons/close.svg", t("Cancel"), Press::Quiet, false, &theme).on_click(cx.listener(|this, _, window, cx| this.cancel_rename(window, cx))))
                .child(small_pill("rename-ok", "icons/check.svg", t("Rename"), Press::Primary, false, &theme).on_click(cx.listener(|this, _, window, cx| this.finish_rename(window, cx))))
                .into_any_element(),
        ];
        Some(self.render_sheet("rename-sheet", title.into(), body, t("↩ to rename, Esc to cancel").into(), Self::cancel_rename, cx))
    }

    /// A new vault: its name in a field, and where it goes as two rows,
    /// iCloud Drive (where the machine has it) and a folder of one's own
    /// with a pill to choose it. ↩ makes it and opens it.
    fn render_new_vault(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let sheet = self.new_vault.as_ref()?;
        let theme = cx.theme().clone();
        let cloud = Self::cloud_place();
        let row = |id: &'static str, icon: &'static str, name: SharedString, where_: SharedString, place: Place, on: bool, theme: &gpui_component::Theme, cx: &mut Context<Self>| {
            h_flex()
                .id(id)
                .h(px(48.))
                .px(px(12.))
                .gap(px(10.))
                .items_center()
                .cursor_pointer()
                .when(on, |d| d.bg(theme.primary.opacity(0.1)))
                .when(!on, |d| d.hover(|s| s.bg(theme.foreground.opacity(0.04))))
                .on_click(cx.listener(move |this, _, window, cx| {
                    swallow_click(window, cx);
                    if let Some(sheet) = this.new_vault.as_mut() {
                        sheet.place = place;
                    }
                    window.focus(&this.vault_input.read(cx).focus_handle(cx), cx);
                    cx.notify();
                }))
                .child(Icon::default().path(icon).with_size(px(16.)).text_color(if on { theme.primary } else { theme.muted_foreground }))
                .child(v_flex().flex_1().min_w_0().child(div().text_size(px(13.)).child(name)).child(div().text_size(px(11.)).text_color(theme.muted_foreground).overflow_hidden().whitespace_nowrap().child(where_)))
        };
        let folder_where: SharedString = crate::sys::short_path(&sheet.folder).into();
        let mut rows: Vec<AnyElement> = Vec::new();
        if cloud.is_some() {
            rows.push(row("place-cloud", "icons/cloud.svg", t("iCloud Drive").into(), format!("{} › Onyx", t("iCloud Drive")).into(), Place::Cloud, sheet.place == Place::Cloud, &theme, cx).into_any_element());
        }
        rows.push(
            row("place-folder", "icons/device-laptop.svg", t("On this computer").into(), folder_where, Place::Folder, sheet.place == Place::Folder, &theme, cx)
                .when(cloud.is_some(), |d| d.border_t_1().border_color(theme.border))
                .child(small_pill("place-choose", "icons/folder-open.svg", t("Choose…"), Press::Quiet, false, &theme).on_click(cx.listener(|this, _, window, cx| {
                    swallow_click(window, cx);
                    this.pick_vault_folder(window, cx)
                })))
                .into_any_element(),
        );
        let body = vec![
            Self::sheet_field("vault-field", &self.vault_input, &theme, cx),
            v_flex().w_full().rounded(px(12.)).border_1().border_color(theme.border).overflow_hidden().children(rows).into_any_element(),
            h_flex()
                .gap(px(8.))
                .justify_end()
                .child(small_pill("vault-cancel", "icons/close.svg", t("Cancel"), Press::Quiet, false, &theme).on_click(cx.listener(|this, _, window, cx| this.cancel_new_vault(window, cx))))
                .child(small_pill("vault-ok", "icons/check.svg", t("Create"), Press::Primary, false, &theme).on_click(cx.listener(|this, _, window, cx| this.finish_new_vault(window, cx))))
                .into_any_element(),
        ];
        Some(self.render_sheet("vault-sheet", t("New Vault").into(), body, t("↩ to create, Esc to cancel").into(), Self::cancel_new_vault, cx))
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
        if self.ground.get() != Some(frame) {
            self.ground.set(Some(frame));
            crate::sys::set_window_ground(window, frame.to_rgb());
        }
        if self.sidebar_moving() {
            window.request_animation_frame();
        }
        let t_side = self.side_t();
        let side_w = SIDEBAR_W * t_side;
        // The strip leaves room for the traffic lights and its button while
        // the sidebar is away, in proportion to the same clock.
        let strip_room = (if cfg!(target_os = "macos") { TRAFFIC_W } else { px(10.) } + STRIP_BTN + px(8.)) * (1. - t_side);
        // The tabs, their widths set for this draw; a frame more while
        // any of them moves.
        let items: Vec<TabItem> = self.tabs.iter().map(|t| TabItem { path: t.path.clone(), title: vault::title(&t.path), dirty: t.dirty }).collect();
        self.tabs_row.sync(&items, self.shown.as_deref(), window, cx);
        if self.tabs_row.moving() {
            window.request_animation_frame();
        }
        let root = self.vault_root().map(Path::to_path_buf);
        let center = if items.is_empty() { div().flex_1().min_w_0().into_any_element() } else { self.tabs_row.render(&items, self.shown.as_deref(), root.as_deref(), cx) };
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
                Self::drag_region(h_flex().h(TITLEBAR_H - FRAME).flex_shrink_0().pl(strip_room + px(12.)).pr(px(12.)).items_center().gap(px(8.)), cx)
                    .child(center)
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
        let sheet = self.render_new_vault(cx).or_else(|| self.render_rename(cx));
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
            .on_modifiers_changed(cx.listener(|this, e: &ModifiersChangedEvent, _, cx| this.tabs_row.modifiers_changed(&e.modifiers, cx)))
            .on_action(cx.listener(|this, _: &ToggleSidebar, window, cx| this.toggle_sidebar(window, cx)))
            .on_action(cx.listener(Self::new_note))
            .on_action(cx.listener(Self::new_folder))
            .on_action(cx.listener(|this, _: &NewVault, window, cx| this.start_new_vault(window, cx)))
            .on_action(cx.listener(|this, _: &OpenVault, window, cx| this.choose_vault(window, cx)))
            .on_action(cx.listener(|this, _: &CloseVault, window, cx| this.close_vault(window, cx)))
            .on_action(cx.listener(|this, _: &Save, _, cx| this.save_now(cx)))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| this.close_nearest(window, cx)))
            .on_action(cx.listener(|this, _: &ReopenTab, window, cx| this.reopen_tab(window, cx)))
            .on_action(cx.listener(|this, _: &Tab1, window, cx| this.go_tab(1, window, cx)))
            .on_action(cx.listener(|this, _: &Tab2, window, cx| this.go_tab(2, window, cx)))
            .on_action(cx.listener(|this, _: &Tab3, window, cx| this.go_tab(3, window, cx)))
            .on_action(cx.listener(|this, _: &Tab4, window, cx| this.go_tab(4, window, cx)))
            .on_action(cx.listener(|this, _: &Tab5, window, cx| this.go_tab(5, window, cx)))
            .on_action(cx.listener(|this, _: &Tab6, window, cx| this.go_tab(6, window, cx)))
            .on_action(cx.listener(|this, _: &Tab7, window, cx| this.go_tab(7, window, cx)))
            .on_action(cx.listener(|this, _: &Tab8, window, cx| this.go_tab(8, window, cx)))
            .on_action(cx.listener(|this, _: &Tab9, window, cx| this.go_tab(9, window, cx)))
            .on_action(cx.listener(|this, _: &ShowLive, window, cx| this.set_mode(Mode::Live, window, cx)))
            .on_action(cx.listener(|this, _: &ShowSource, window, cx| this.set_mode(Mode::Source, window, cx)))
            .on_action(cx.listener(|this, _: &ShowReading, window, cx| this.set_mode(Mode::Reading, window, cx)))
            .on_action(cx.listener(|this, _: &ToggleReading, window, cx| {
                let mode = if this.mode == Mode::Reading { Mode::Live } else { Mode::Reading };
                this.set_mode(mode, window, cx)
            }))
            .on_action(cx.listener(|this, _: &Escape, window, cx| {
                if this.rename.is_some() {
                    this.cancel_rename(window, cx);
                }
                if this.new_vault.is_some() {
                    this.cancel_new_vault(window, cx);
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
            .children(sheet)
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
    /// vault's first note opened, the sidebar folded, a mode, a row's menu,
    /// the tabs' numbers.
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
                "vault" => self.start_new_vault(window, cx),
                "notes" => {
                    // The vault's first three notes, as tabs.
                    if let Some(root) = self.vault_root().map(Path::to_path_buf) {
                        for note in vault::notes(&root).into_iter().take(3) {
                            self.open_note(note, window, cx);
                        }
                    }
                }
                "close" => self.close_nearest(window, cx),
                "hints" => self.tabs_row.hints = true,
                "end" => {
                    if let Some(e) = self.shown_editor() {
                        e.update(cx, |e, cx| e.move_to_end(cx));
                    }
                }
                "type" => {
                    if let Some(e) = self.shown_editor() {
                        e.update(cx, |e, cx| e.type_text("Hello, probe. ", cx));
                    }
                }
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
