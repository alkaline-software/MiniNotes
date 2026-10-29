//! The clock that everything moving while something is at work runs on:
//! a spinner, a mark turning, dots breathing. Each is drawn where the
//! clock stands (`phase`), and while one was drawn the window is drawn
//! again a step later (`after_frame`): about 30 steps a second, 20 while
//! another app is in use.
//!
//! Never `Animation::repeat` for this. A repeating animation asks for
//! every frame the display has, and the window is drawn whole at each: a
//! mark of 14 points turning held a window at 60 frames a second for as
//! long as it turned, a fifth of a core and as much again of the window
//! server's. A transition (a fold, a card coming up) still runs on gpui's
//! own clock, and is over in a moment.

use std::cell::Cell;
use std::time::{Duration, Instant};

use gpui::{App, Context, Entity, IntoElement, Render, Window, div};

thread_local! {
    static EPOCH: Instant = Instant::now();
    static WANTED: Cell<bool> = const { Cell::new(false) };
}

/// The wait for the next step while the window is in use and while it is
/// not: the frame lands on the display's next refresh after it.
const STEP: Duration = Duration::from_millis(28);
const STEP_AWAY: Duration = Duration::from_millis(45);

/// Where a motion `period` long stands now, from 0 to 1, and a note that
/// something on the clock was drawn in this frame.
#[allow(dead_code)] // for a mark at work; nothing turns in Onyx yet
pub fn phase(period: Duration) -> f32 {
    WANTED.with(|w| w.set(true));
    let p = period.as_secs_f64();
    (EPOCH.with(|e| e.elapsed().as_secs_f64()) % p / p) as f32
}

/// A breath from `min` to `max` and back, at the clock's phase `t`.
#[allow(dead_code)] // for a mark at work; nothing breathes in Onyx yet
pub fn pulse(min: f32, max: f32, t: f32) -> f32 {
    let s = (t * std::f32::consts::TAU).sin();
    let breath = (s * s * s + s) / 2.;
    min + (breath + 1.) / 2. * (max - min)
}

/// The window's step on the clock: a view of no size, so its notify draws
/// the window again without telling the main view, and what the window
/// keeps between frames (`AnyView::cached`) is not drawn afresh with it.
#[derive(Default)]
pub struct Ambient {
    due: bool,
}

impl Render for Ambient {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// A frame that is not a change (a cursor's blink): asked of the clock's
/// view, which draws the window and tells the main view nothing.
#[allow(dead_code)] // for a product with a cursor or a terminal; the sample has none
pub fn redraw(ambient: &Entity<Ambient>, cx: &mut App) {
    ambient.update(cx, |_, cx| cx.notify());
}

/// At the end of a frame, from the last thing it paints: when something
/// on the clock was drawn in it, the next step is asked for.
pub fn after_frame(ambient: &Entity<Ambient>, window: &Window, cx: &mut App) {
    if !WANTED.with(|w| w.replace(false)) {
        return;
    }
    let step = if window.is_window_active() { STEP } else { STEP_AWAY };
    ambient.update(cx, |a, cx| {
        if a.due {
            return;
        }
        a.due = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(step).await;
            let _ = this.update(cx, |a, cx| {
                a.due = false;
                cx.notify();
            });
        })
        .detach();
    });
}
