//! A menu of the app's own: at a right click, or dropped down from a
//! pill. A small card at the pointer, kept inside the window, over a
//! clear sheet any click or Escape puts away. Each choice has an icon
//! before its words, choices of one kind stand together with a line
//! between the groups, and the card is as wide as its longest choice.
//!
//! It comes in over `MENU_IN`, rising a few pixels into place as it fades
//! in, and goes out over `MENU_OUT`, fading where it was: what was closed
//! is kept for that moment and drawn once more, taking no click. Each
//! opening has a serial in its animation's name, so it plays once. The
//! toolkit's own popover has no way out but at once, which is why this
//! is the app's.

use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _};

use crate::widgets::{float_shadow, swallow_click, MENU_IN, MENU_OUT};

/// One line of a menu.
#[derive(Clone)]
pub enum Entry {
    /// A choice: what it is answered by, its icon, its words, and whether
    /// its icon is a picture in its own colors (a flag) rather than an
    /// icon in the ink.
    Choice { id: SharedString, icon: &'static str, label: SharedString, picture: bool, chosen: bool },
    /// The line between two groups.
    Rule,
}

impl Entry {
    pub fn choice(id: impl Into<SharedString>, icon: &'static str, label: impl Into<SharedString>) -> Self {
        Entry::Choice { id: id.into(), icon, label: label.into(), picture: false, chosen: false }
    }
}

#[derive(Clone)]
pub struct Menu {
    /// Which menu it is, for the view that answers its choices.
    pub what: &'static str,
    pub at: Point<Pixels>,
    pub entries: Vec<Entry>,
    pub serial: u32,
}

/// A view's menu: the one open, and the one going out.
#[derive(Default)]
pub struct Menus {
    pub open: Option<Menu>,
    pub gone: Option<(Menu, Instant)>,
    serial: u32,
}

impl Menus {
    pub fn show(&mut self, what: &'static str, at: Point<Pixels>, entries: Vec<Entry>) {
        self.serial += 1;
        self.open = Some(Menu { what, at, entries, serial: self.serial });
    }

    /// Put the open menu away, keeping it for its going.
    pub fn close<V: 'static>(&mut self, cx: &mut Context<V>) {
        if let Some(m) = self.open.take() {
            self.gone = Some((m, Instant::now()));
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(MENU_OUT + Duration::from_millis(20)).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
        }
    }

    /// What to draw over the view: the open menu, or the one going out.
    /// `pick` answers a choice with the menu's `what` and the choice's id.
    #[allow(clippy::type_complexity)]
    pub fn render<V: 'static>(&self, pick: std::rc::Rc<dyn Fn(&mut V, &'static str, SharedString, &mut Window, &mut Context<V>)>, close: std::rc::Rc<dyn Fn(&mut V, &mut Context<V>)>, cx: &mut Context<V>) -> Option<AnyElement> {
        if let Some(m) = &self.open {
            return Some(card(m, None, pick, close, cx));
        }
        let (m, at) = self.gone.as_ref()?;
        (at.elapsed() < MENU_OUT).then(|| card(m, Some(at.elapsed()), pick, close, cx))
    }
}

#[allow(clippy::type_complexity)]
fn card<V: 'static>(m: &Menu, going: Option<Duration>, pick: std::rc::Rc<dyn Fn(&mut V, &'static str, SharedString, &mut Window, &mut Context<V>)>, close: std::rc::Rc<dyn Fn(&mut V, &mut Context<V>)>, cx: &mut Context<V>) -> AnyElement {
    let theme = cx.theme().clone();
    let what = m.what;
    let rows = m.entries.iter().map(|e| match e.clone() {
        Entry::Rule => div().h(px(1.)).mx(px(8.)).my(px(4.)).bg(theme.border).into_any_element(),
        Entry::Choice { id, icon, label, picture, chosen } => {
            let pick = pick.clone();
            let id2 = id.clone();
            h_flex()
                .id(ElementId::Name(format!("menu-{what}-{id}").into()))
                .h(px(28.))
                .px(px(10.))
                .gap(px(9.))
                .items_center()
                .rounded(px(6.))
                .text_size(px(13.))
                .text_color(theme.popover_foreground)
                .cursor_pointer()
                // In the dark appearance the palette's hover is a shade off
                // the card and cannot be seen: a wash of the ink is.
                .hover(|s| s.bg(theme.foreground.opacity(0.07)))
                .map(|d| if picture { d.child(img(icon).w(px(17.)).h(px(12.)).flex_shrink_0()) } else { d.child(Icon::default().path(icon).with_size(px(15.)).text_color(theme.muted_foreground)) })
                .child(div().flex_1().child(label))
                .when(chosen, |d| d.child(Icon::default().path("icons/check.svg").with_size(px(14.)).text_color(theme.primary)))
                .when(going.is_none(), |d| {
                    d.on_click(cx.listener(move |this, _, window, cx| {
                        swallow_click(window, cx);
                        pick(this, what, id2.clone(), window, cx)
                    }))
                })
                .into_any_element()
        }
    });
    let body = v_flex()
        .min_w(px(160.))
        .p(px(5.))
        .rounded(px(10.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .shadow(float_shadow(&theme))
        .occlude()
        .children(rows);
    let body = match going {
        None => body.with_animation(ElementId::NamedInteger("menu-in".into(), m.serial as u64), Animation::new(MENU_IN).with_easing(ease_out_quint()), |d, t| d.opacity(t).mt(px(-4. * (1. - t)))).into_any_element(),
        Some(gone) => {
            let left = 1. - (gone.as_secs_f32() / MENU_OUT.as_secs_f32()).min(1.);
            body.opacity(ease_out_quint()(left)).into_any_element()
        }
    };
    let sheet = div().absolute().inset_0();
    let sheet = match going {
        // A press anywhere off the card puts it away.
        None => sheet
            .id("menu-sheet")
            .occlude()
            .on_mouse_down(MouseButton::Left, cx.listener({
                let close = close.clone();
                move |this, _, window, cx| {
                    swallow_click(window, cx);
                    close(this, cx)
                }
            }))
            .on_mouse_down(MouseButton::Right, cx.listener({
                let close = close.clone();
                move |this, _, window, cx| {
                    swallow_click(window, cx);
                    close(this, cx)
                }
            }))
            .into_any_element(),
        Some(_) => sheet.into_any_element(),
    };
    div()
        .absolute()
        .inset_0()
        .child(sheet)
        // Kept inside the window, so the room at its right is the room at its left.
        .child(anchored().position(m.at).snap_to_window_with_margin(px(8.)).child(body))
        .into_any_element()
}
