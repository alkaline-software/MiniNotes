//! The settings window (⌘,): a window of its own, as the Mac's own
//! settings are. The rail of sections at its left under the traffic
//! lights; at its right the chosen section's name in the window's top,
//! level with the traffic lights, over its groups, each a card of rows
//! with a name at the left and a control at the right.
//!
//! The window draws the main view's own state (`SettingsView` renders
//! `Shell::render_settings`), so every listener on it is the main view's
//! and a change shows in both windows at once. Every section fits the
//! window as it opens; the page scrolls only for a window made smaller.

use std::rc::Rc;
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _};

use onyx_core::config::Config;
use onyx_core::update;

use crate::i18n::{phrase, t, tf, weave};
use crate::menu::Entry;
use crate::shell::{Escape, OpenSettings, Shell};
use crate::widgets::{choice_pill, page_in, small_pill, wordmark, Press};

/// The window's size as it opens, and the least it is dragged to. It is
/// never narrower than it opens: the rows are laid out for that width,
/// and narrower, names broke mid-word. Shorter, the page scrolls.
pub const SETTINGS_W: Pixels = px(820.);
pub const SETTINGS_H: Pixels = px(600.);
const SETTINGS_MIN: Size<Pixels> = Size { width: SETTINGS_W, height: px(460.) };
const RAIL_W: Pixels = px(190.);
const PAGE_X: Pixels = px(36.);
/// The window's top, clear for the traffic lights and dragged by.
const TOP_H: Pixels = px(52.);
/// What the widest controls at a row's right take, so those on a page
/// start and end together.
const CONTROL_W: Pixels = px(336.);
const CONTROL_H: Pixels = px(28.);
const CARD_R: Pixels = px(12.);
pub const SETTINGS_CONTEXT: &str = "Settings";

/// The sections in the rail's order: key, name, icon. About is last.
pub const SECTIONS: &[(&str, &str, &str)] = &[
    ("appearance", phrase("Appearance"), "icons/palette.svg"),
    ("editor", phrase("Editor"), "icons/typography.svg"),
    ("about", phrase("About"), "icons/info.svg"),
];

/// A section by its key, the first when there is none of that name.
pub fn section_named(key: &str) -> &'static str {
    SECTIONS.iter().find(|(k, _, _)| *k == key).map_or(SECTIONS[0].0, |(k, _, _)| k)
}

/// The settings window's root: the main view's settings, drawn in a
/// window of their own.
pub struct SettingsView {
    shell: WeakEntity<Shell>,
    focus: FocusHandle,
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = self.focus.clone();
        match self.shell.upgrade() {
            Some(s) => s.update(cx, |s, cx| s.render_settings(focus, window, cx)),
            None => div().into_any_element(),
        }
    }
}

/// Show the settings: the window comes forward, or opens, centered as a
/// Mac's settings window opens.
pub fn open(shell: WeakEntity<Shell>, cx: &mut App) {
    let Some(s) = shell.upgrade() else { return };
    if let Some(handle) = s.read(cx).settings {
        if handle.update(cx, |_, window, _| window.activate_window()).is_ok() {
            return;
        }
    }
    let bounds = Bounds::centered(None, size(SETTINGS_W, SETTINGS_H), cx);
    let titlebar = if cfg!(target_os = "macos") {
        TitlebarOptions { title: Some(t("Settings").into()), appears_transparent: true, traffic_light_position: Some(point(px(17.), px(19.))) }
    } else {
        TitlebarOptions { title: Some(t("Settings").into()), appears_transparent: false, traffic_light_position: None }
    };
    let weak = shell.clone();
    let opened = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(titlebar),
            window_min_size: Some(SETTINGS_MIN),
            app_id: Some("Onyx".into()),
            // The top is the app's to drag by: left to AppKit, a press on
            // a control there moved the window.
            app_owns_titlebar_drag: true,
            ..Default::default()
        },
        move |window, cx| {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            let view = cx.new(|cx| {
                if let Some(s) = weak.upgrade() {
                    cx.observe(&s, |_, _, cx| cx.notify()).detach();
                }
                SettingsView { shell: weak.clone(), focus }
            });
            cx.new(|cx| gpui_component::Root::new(view, window, cx))
        },
    );
    match opened {
        Ok(handle) => {
            s.update(cx, |s, _| s.settings = Some(handle.into()));
            let _ = handle.update(cx, |_, window, _| {
                if std::env::var_os("ONYX_GO").is_some() {
                    crate::sys::keep_on_top(window);
                }
                window.activate_window();
            });
        }
        Err(e) => eprintln!("onyx: could not open the settings: {e}"),
    }
}

/// A group of the settings: its name over it, what belongs to the whole
/// group at the name's right, and its rows on one card.
fn group(title: Option<SharedString>, aside: Option<AnyElement>, rows: Vec<AnyElement>, theme: &gpui_component::Theme) -> AnyElement {
    let head = (title.is_some() || aside.is_some()).then(|| {
        h_flex()
            .min_h(px(20.))
            // In line with the rows' names and controls under it.
            .px(px(14.))
            .gap(px(12.))
            .items_center()
            .justify_between()
            .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.muted_foreground).children(title))
            .children(aside)
    });
    v_flex().gap(px(6.)).children(head).child(card(rows, theme)).into_any_element()
}

/// The ground of a group: a card a shade off the page, with a hairline
/// between its rows. A row that paints its own ground rounds it where it
/// meets the card's corners, by the card's radius less its border: a clip
/// in gpui is a rectangle.
fn card(rows: Vec<AnyElement>, theme: &gpui_component::Theme) -> Div {
    v_flex().w_full().rounded(CARD_R).border_1().border_color(theme.border).bg(card_ground(theme)).children(rows.into_iter().enumerate().map(|(i, row)| div().when(i > 0, |d| d.border_t_1().border_color(theme.border)).child(row)))
}

/// A card's ground: the muted shade thinned in the light appearance, the
/// sidebar's in the dark.
fn card_ground(theme: &gpui_component::Theme) -> Hsla {
    if theme.mode.is_dark() { theme.sidebar } else { theme.muted.opacity(0.4) }
}

/// A row of a card: its name, under it a line only where the name does
/// not say it all, and its control at the right. Every row is made here,
/// never laid out by hand.
fn line(title: SharedString, detail: Option<SharedString>, control: AnyElement, theme: &gpui_component::Theme) -> AnyElement {
    h_flex()
        .min_h(px(46.))
        .px(px(14.))
        .py(px(8.))
        .gap(px(16.))
        .items_center()
        .child(v_flex().flex_1().min_w_0().gap(px(1.)).child(div().text_size(px(13.)).child(title)).children(detail.map(|d| div().text_size(px(11.5)).line_height(px(15.)).text_color(theme.muted_foreground).child(d))))
        .child(div().flex_shrink_0().child(control))
        .into_any_element()
}

/// A card of tiles: each face drawn in itself, the chosen one on a plate
/// with a ring of the accent. The tiles share the card's width.
fn tiles(items: Vec<(&'static str, SharedString, SharedString, bool, bool)>, pick: Rc<dyn Fn(&mut Shell, &'static str, &mut Window, &mut Context<Shell>)>, theme: &gpui_component::Theme, cx: &mut Context<Shell>) -> AnyElement {
    h_flex()
        .p(px(14.))
        .gap(px(10.))
        .flex_wrap()
        .children(items.into_iter().map(|(key, label, family, mono, chosen)| {
            let pick = pick.clone();
            v_flex()
                .id(SharedString::from(format!("tile-{key}")))
                .w(px(120.))
                .h(px(76.))
                .items_center()
                .justify_center()
                .gap(px(4.))
                .rounded(px(8.))
                .border_2()
                .border_color(if chosen { theme.primary } else { theme.border })
                .bg(if chosen { theme.primary.opacity(0.08) } else { theme.background.opacity(0.5) })
                .cursor_pointer()
                .hover(|s| s.bg(theme.foreground.opacity(0.04)))
                .on_click(cx.listener(move |this, _, window, cx| pick(this, key, window, cx)))
                .child(div().font_family(family).text_size(px(if mono { 20. } else { 24. })).line_height(px(28.)).child(if mono { "{ }" } else { "Aa" }))
                .child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child(label))
        }))
        .into_any_element()
}

impl Shell {
    pub(crate) fn render_settings(&mut self, focus: FocusHandle, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let section = self.settings_section;
        let rows = match section {
            "editor" => self.editor_rows(cx),
            "about" => self.about_rows(cx),
            _ => self.appearance_rows(window, cx),
        };
        let title = SECTIONS.iter().find(|(k, _, _)| *k == section).map(|(_, name, _)| t(name)).unwrap_or(t("Appearance"));
        let language = (section == "appearance").then(|| self.language_pill(cx));
        let pick = Rc::new(|this: &mut Self, what: &'static str, id: SharedString, window: &mut Window, cx: &mut Context<Self>| this.settings_pick(what, id, window, cx));
        let close = Rc::new(|this: &mut Self, cx: &mut Context<Self>| {
            this.menus.close(cx);
            cx.notify();
        });
        let menu = if self.menus.open.as_ref().is_some_and(|m| m.what == "language") || self.menus.gone.as_ref().is_some_and(|(m, _)| m.what == "language") { self.menus.render(pick, close, cx) } else { None };
        div()
            .id("settings-window")
            .key_context(SETTINGS_CONTEXT)
            .track_focus(&focus)
            .size_full()
            .relative()
            .bg(theme.popover)
            .text_color(theme.foreground)
            .text_size(px(13.))
            // Escape puts away a list open here, and with none open closes
            // the window, as ⌘W does.
            .on_action(cx.listener(|this, _: &Escape, window, cx| {
                if this.menus.open.is_some() {
                    this.menus.close(cx);
                    cx.notify();
                } else {
                    window.remove_window();
                }
            }))
            .on_action(cx.listener(|_, _: &crate::CloseWindow, window, _| window.remove_window()))
            .on_action(cx.listener(|_, _: &OpenSettings, window, _| window.activate_window()))
            .child(
                h_flex()
                    .size_full()
                    .items_stretch()
                    // A new language is faded into, rail and page alike.
                    .with_animation(ElementId::NamedInteger("settings-lang".into(), self.lang_serial as u64), Animation::new(Duration::from_millis(240)).with_easing(ease_out_quint()), |d, t| d.opacity(t))
                    .child(self.settings_rail(cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .child(
                                drag_region(h_flex().h(TOP_H).flex_shrink_0().w_full().pl(PAGE_X).pr(PAGE_X + crate::csd::room(window, cx).1).items_center())
                                    .child(page_in(format!("title-{section}"), div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(title)))
                                    .when_some(language, |d, l| d.child(div().flex_1()).child(l)),
                            )
                            // One pane: the settings have no gesture to route.
                            .child({
                                v_flex()
                                    .relative()
                                    .flex_1()
                                    .min_h_0()
                                    .child(v_flex().id("settings-body").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.settings_scroll).px(PAGE_X).pt(px(2.)).pb(px(24.)).child(page_in(format!("page-{section}"), v_flex().gap(px(18.)).children(rows))))
                                    .vertical_scrollbar(&self.settings_scroll)
                            }),
                    ),
            )
            .children(menu)
            .children(crate::csd::chrome(window, TOP_H / 2. - px(12.), |window, _| window.remove_window(), cx))
            .into_any_element()
    }

    /// The rail of sections, under the traffic lights: an icon and a name
    /// a section, the chosen one on a plate. A section with something
    /// waiting wears a dot in the accent. Its empty room drags the window.
    fn settings_rail(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let current = self.settings_section;
        let waiting = self.update.found.is_some();
        drag_region(v_flex().w(RAIL_W).h_full().flex_shrink_0().pt(TOP_H).px(px(10.)).gap(px(2.)).bg(if theme.mode.is_dark() { theme.sidebar } else { theme.muted.opacity(0.5) }).border_r_1().border_color(theme.border))
            .children(SECTIONS.iter().map(|(key, label, icon)| {
                let active = *key == current;
                let key: &'static str = key;
                h_flex()
                    .id(SharedString::from(format!("settings-nav-{key}")))
                    .h(px(30.))
                    .px(px(10.))
                    .gap(px(9.))
                    .items_center()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .when(active, |d| d.bg(theme.sidebar_accent))
                    .when(!active, |d| d.hover(|s| s.bg(theme.sidebar_accent.opacity(0.6))))
                    // A press on a section is not a drag of the window.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if this.settings_section != key {
                            this.settings_section = key;
                            this.settings_scroll.set_offset(point(px(0.), px(0.)));
                            this.menus.close(cx);
                            this.save_ui_now(window, cx);
                        }
                        cx.notify();
                    }))
                    .child(Icon::default().path(*icon).with_size(px(15.)).text_color(if active { theme.foreground } else { theme.muted_foreground }))
                    .child(div().flex_1().text_size(px(13.)).when(active, |d| d.font_weight(FontWeight::MEDIUM)).text_color(if active { theme.foreground } else { theme.sidebar_foreground }).child(t(label)))
                    .when(key == "about" && waiting, |d| d.child(div().size(px(7.)).rounded_full().bg(theme.primary)))
            }))
            .into_any_element()
    }

    fn appearance_rows(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let looks = div().w(px(CONTROL_W.into())).child(self.motion.segmented(
            "appearance",
            vec![("system", "icons/device-desktop.svg", t("System").into()), ("light", "icons/sun.svg", t("Light").into()), ("dark", "icons/moon.svg", t("Dark").into())],
            match self.cfg.app.appearance.as_str() {
                "light" => "light",
                "dark" => "dark",
                _ => "system",
            },
            true,
            Rc::new(|this: &mut Self, key, window, cx| {
                this.cfg.app.appearance = key.to_string();
                let _ = Config::edit(|c| c.app.appearance = key.to_string());
                crate::look::apply(key, Some(window), cx);
                cx.notify();
            }),
            cx,
        ));
        let dark = theme.mode.is_dark();
        let chosen = crate::look::accent(&self.cfg.app.accent).key;
        let ground = card_ground(&theme);
        let swatches = h_flex().gap(px(10.)).children(crate::look::ACCENTS.iter().map(|a| {
            let key = a.key;
            let color = Rgba::try_from(crate::look::accent_hex(key, dark)).map(Hsla::from).unwrap_or(theme.primary);
            div()
                .id(SharedString::from(format!("accent-{key}")))
                .size(px(22.))
                .rounded_full()
                .bg(color)
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                // The chosen one is marked by a ring inside it, so every
                // disc is one size and the row's ends are its first and last.
                .when(key == chosen, |d| d.child(div().size(px(14.)).rounded_full().border_2().border_color(ground)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.cfg.app.accent = key.to_string();
                    let _ = Config::edit(|c| c.app.accent = key.to_string());
                    crate::look::install(key, cx);
                    let appearance = this.cfg.app.appearance.clone();
                    crate::look::apply(&appearance, Some(window), cx);
                    cx.notify();
                }))
        }));
        let accent_name = t(crate::look::accent(&self.cfg.app.accent).name);
        vec![group(
            None,
            None,
            vec![line(t("Appearance").into(), None, looks.into_any_element(), &theme), line(t("Accent").into(), Some(accent_name.into()), swatches.into_any_element(), &theme)],
            &theme,
        )]
    }

    /// Editor: the faces a note is set in, each a tile drawn in itself;
    /// the words' size; and how wide a line runs.
    fn editor_rows(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let text_face = self.cfg.editor.text_face.clone();
        let text_tiles = tiles(
            crate::fonts::TEXT_FACES.iter().map(|f| (f.key, t(f.name).to_string().into(), SharedString::from(f.family), false, f.key == text_face)).collect(),
            Rc::new(|this: &mut Self, key, _, cx| {
                this.cfg.editor.text_face = key.to_string();
                let _ = Config::edit(|c| c.editor.text_face = key.to_string());
                this.push_look(cx);
                cx.notify();
            }),
            &theme,
            cx,
        );
        let mono_face = self.cfg.editor.mono_face.clone();
        let mono_tiles = tiles(
            crate::fonts::MONO_FACES.iter().map(|f| (f.key, t(f.name).to_string().into(), SharedString::from(f.family), true, f.key == mono_face)).collect(),
            Rc::new(|this: &mut Self, key, _, cx| {
                this.cfg.editor.mono_face = key.to_string();
                let _ = Config::edit(|c| c.editor.mono_face = key.to_string());
                this.push_look(cx);
                cx.notify();
            }),
            &theme,
            cx,
        );
        let sizes = div().w(px(CONTROL_W.into())).child(self.motion.segmented(
            "text-size",
            vec![("small", "", t("Small").into()), ("medium", "", t("Medium").into()), ("large", "", t("Large").into())],
            match self.cfg.editor.text_size.as_str() {
                "small" => "small",
                "large" => "large",
                _ => "medium",
            },
            true,
            Rc::new(|this: &mut Self, key, _, cx| {
                this.cfg.editor.text_size = key.to_string();
                let _ = Config::edit(|c| c.editor.text_size = key.to_string());
                this.push_look(cx);
                cx.notify();
            }),
            cx,
        ));
        let readable = self.motion.switch(
            "readable",
            self.cfg.editor.readable_width,
            Rc::new(|this: &mut Self, on, _, cx| {
                this.cfg.editor.readable_width = on;
                let _ = Config::edit(|c| c.editor.readable_width = on);
                cx.notify();
            }),
            cx,
        );
        vec![
            group(Some(t("Text").into()), None, vec![text_tiles], &theme),
            group(Some(t("Code").into()), None, vec![mono_tiles], &theme),
            group(
                Some(t("Layout").into()),
                None,
                vec![line(t("Text size").into(), None, sizes.into_any_element(), &theme), line(t("Readable line length").into(), Some(t("Lines no wider than a comfortable line of reading, centered.").into()), readable, &theme)],
                &theme,
            ),
        ]
    }

    /// About: the title card, set as a film's first one is, the credits,
    /// and the updates under them.
    fn about_rows(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let link = |id: &'static str, words: &'static str, url: &'static str| {
            div().id(id).text_color(theme.link).cursor_pointer().hover(|s| s.underline()).on_click(move |_, _, _| crate::sys::open(url)).child(t(words)).into_any_element()
        };
        let title = v_flex()
            .items_center()
            .gap(px(8.))
            .py(px(26.))
            .child(div().text_size(px(11.)).text_color(theme.muted_foreground).child(tf("{studio} presents", &[("studio", &"Alkaline Software")])))
            .child(img("icon/icon-512.png").size(px(84.)))
            .child(wordmark("Onyx", 26., theme.primary, cx))
            .child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child(tf("Version {version}", &[("version", &update::current_version())])))
            .child(weave(&t("Made by {studio}."), vec![("studio", link("studio-link", "Alkaline Software", "https://github.com/alkaline-software"))], h_flex().pt(px(6.)).text_size(px(12.5)).gap(px(0.))))
            .child(
                h_flex()
                    .pt(px(6.))
                    .gap(px(8.))
                    .child(small_pill("about-source", "icons/github.svg", t("Source"), Press::Quiet, false, &theme).on_click(|_, _, _| crate::sys::open(&update::repo_page())))
                    .child(small_pill("about-notes", "icons/book-open.svg", t("Release Notes"), Press::Quiet, false, &theme).on_click(|_, _, _| crate::sys::open(&update::release_page(update::current_version())))),
            );
        let u = self.update.clone();
        let (icon, words, kind) = match (&u.found, u.downloading) {
            (_, Some(p)) => ("icons/download.svg", tf("Downloading… {percent}%", &[("percent", &p)]), Press::Primary),
            (Some(v), None) => ("icons/download.svg", tf("Update to {version}", &[("version", v)]), Press::Primary),
            (None, None) => ("icons/refresh.svg", t("Check for updates").to_string(), Press::Quiet),
        };
        let busy = u.checking || u.downloading.is_some();
        let button = choice_pill("update", icon, words, kind, busy, &theme).min_w(px(190.)).justify_center().when(!busy, |d| {
            d.on_click(cx.listener(|this, _, _, cx| {
                if this.update.found.is_some() {
                    this.install_update(cx);
                } else {
                    this.check_updates(cx);
                }
            }))
        });
        let said = u.said.clone().map(SharedString::from);
        let check = self.motion.switch(
            "check-updates",
            self.cfg.app.check_updates,
            Rc::new(|this: &mut Self, on, _, cx| {
                this.cfg.app.check_updates = on;
                let _ = Config::edit(|c| c.app.check_updates = on);
                cx.notify();
            }),
            cx,
        );
        let mut rows = vec![group(None, None, vec![title.into_any_element()], &theme)];
        if !update::from_store() {
            rows.push(group(None, None, vec![line(t("Updates").into(), said, button.into_any_element(), &theme), line(t("Check automatically").into(), None, check, &theme)], &theme));
        }
        rows
    }

    /// The window's language, at the right of Appearance's name: the
    /// system's, or a language by its flag and letters, in a list that
    /// drops down.
    fn language_pill(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let chosen = crate::i18n::chosen(&self.cfg.app.language);
        let (icon, label, picture) = match crate::i18n::CHOICES.iter().find(|(c, _, _)| *c == chosen) {
            Some((_, flag, letters)) => (*flag, letters.to_string(), true),
            None => ("icons/globe.svg", t("System").to_string(), false),
        };
        h_flex()
            .id("language-pill")
            .h(CONTROL_H)
            .px(px(12.))
            .gap(px(7.))
            .items_center()
            .rounded_full()
            .border_1()
            .border_color(theme.foreground.opacity(0.16))
            .text_size(px(12.5))
            .cursor_pointer()
            .hover(|s| s.bg(theme.foreground.opacity(0.06)))
            // A press on it is not a drag of the window's top.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
                let at = ev.position() + point(px(-60.), px(16.));
                let chosen = crate::i18n::chosen(&this.cfg.app.language);
                let mut entries = vec![Entry::Choice { id: "system".into(), icon: "icons/globe.svg", label: t("System").into(), picture: false, chosen: chosen == "system" }, Entry::Rule];
                entries.extend(crate::i18n::CHOICES.iter().map(|(code, flag, letters)| Entry::Choice { id: (*code).into(), icon: flag, label: (*letters).into(), picture: true, chosen: chosen == *code }));
                this.menus.show("language", at, entries);
                cx.notify();
            }))
            .map(|d| if picture { d.child(img(icon).w(px(17.)).h(px(12.))) } else { d.child(Icon::default().path(icon).with_size(px(14.))) })
            .child(label)
            .child(Icon::default().path("icons/chevron-down.svg").with_size(px(12.)).text_color(theme.muted_foreground))
            .into_any_element()
    }

    fn settings_pick(&mut self, what: &'static str, id: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        if what == "language" {
            let code = id.to_string();
            self.cfg.app.language = code.clone();
            let _ = Config::edit(|c| c.app.language = code.clone());
            // On a Mac the app's own `AppleLanguages` is the setting, which
            // System Settings writes too.
            crate::sys::set_own_language(if code == "system" { None } else { crate::i18n::LANGS.iter().find(|(c, _, _)| *c == code).map(|(_, _, bundle)| *bundle).or(Some("en")) });
            crate::i18n::set(&code);
            cx.set_menus(crate::app_menus());
            self.lang_serial += 1;
            let _ = window;
        }
        self.menus.close(cx);
        cx.refresh_windows();
        cx.notify();
    }

    fn save_ui_now(&self, _window: &mut Window, cx: &mut Context<Self>) {
        let mut ui = crate::ui_state::UiState::load();
        ui.settings_section = Some(self.settings_section.to_string());
        ui.save();
        let _ = cx;
    }
}

/// The window's top and the rail's empty room drag the window.
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
