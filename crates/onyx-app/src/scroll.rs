//! A scroll gesture belongs to the scroller it began in, its momentum
//! included, wherever the pointer is carried, and stops at that
//! scroller's edge.
//!
//! macOS goes on sending wheel events after the finger lifts, addressed
//! to wherever the pointer is by then, and gpui hands each to the
//! scroller under it. A momentum event is only `Moved`, so a flick in one
//! list followed by a move onto another scrolled the second. The router
//! runs in the capture phase, before any scroller: the scroller under the
//! pointer when a gesture begins (its `Started` phase, or the first event
//! after a short gap, since a mouse wheel sends no phases) owns it, and
//! every event of the gesture is applied to the owner, held to its ends,
//! and stopped.
//!
//! Every scroller that goes up and down names itself as it is drawn
//! (`name`). One that does not leaks: a flick in it scrolls whatever the
//! pointer reaches next. A sideways scroller is not named.

use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

use gpui::{point, px, ScrollHandle, ScrollWheelEvent, TouchPhase};

/// A wheel event this long after the previous one begins a new gesture.
const GAP: Duration = Duration::from_millis(150);

#[derive(Default)]
pub struct Scrollers {
    /// The scrollers drawn in the last frame, in the order drawn: the last
    /// is on top.
    live: RefCell<Vec<ScrollHandle>>,
    /// The scroller the gesture under way began in, if it began in one.
    owner: RefCell<Option<ScrollHandle>>,
    last: Cell<Option<Instant>>,
}

impl Scrollers {
    /// At the start of a view's draw: the scrollers it names are this
    /// draw's.
    pub fn begin(&self) {
        self.live.borrow_mut().clear();
    }

    /// A scroller drawn now.
    pub fn name(&self, handle: &ScrollHandle) {
        self.live.borrow_mut().push(handle.clone());
    }

    /// One wheel event, in the capture phase. True when it was applied
    /// here and must go no further.
    pub fn route(&self, e: &ScrollWheelEvent) -> bool {
        let now = Instant::now();
        let fresh = self.last.get().is_none_or(|at| now.duration_since(at) > GAP);
        self.last.set(Some(now));
        let begins = match e.touch_phase {
            TouchPhase::Started => true,
            TouchPhase::Moved => fresh,
            TouchPhase::Ended | TouchPhase::Cancelled => false,
        };
        if begins {
            // The top scroller under the pointer with something to scroll.
            let under = self.live.borrow().iter().rev().find(|h| h.bounds().contains(&e.position) && h.max_offset().y > px(0.)).cloned();
            *self.owner.borrow_mut() = under;
        }
        let Some(owner) = self.owner.borrow().clone() else { return false };
        let delta = e.delta.pixel_delta(px(20.));
        let y = (owner.offset().y + delta.y).clamp(-owner.max_offset().y, px(0.));
        owner.set_offset(point(owner.offset().x, y));
        true
    }
}
