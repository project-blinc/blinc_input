//! Polling-style input state for Blinc sketches and canvases.
//!
//! Blinc's native event model is callback-based — `div().on_key_down(|e| …)`
//! fires whenever the focused widget sees a key. Sketches and games
//! typically want the *polling* shape: inside `draw()`, ask
//! `is_key_down(KeyCode::SPACE)` or `mouse_position()`. This crate
//! bridges the two.
//!
//! [`InputState`] owns the polling snapshot. Hand it to the event
//! system via [`DivInputExt::capture_input`] (or register handlers
//! manually and call [`InputState::record`] from each one), then
//! query it from `draw`. Call [`InputState::frame_end`] once per
//! frame to clear transient edge-trigger state
//! (`is_key_just_pressed` / `is_mouse_just_pressed`).
//!
//! # Example
//!
//! ```ignore
//! use blinc_input::{InputState, DivInputExt, MouseButton};
//! use blinc_core::events::KeyCode;
//!
//! let input = InputState::new();
//!
//! let tree = div()
//!     .w_full().h_full()
//!     .capture_input(&input)
//!     .child(/* … */);
//!
//! // Inside your sketch's draw():
//! if input.is_key_down(KeyCode::SPACE) { /* jump! */ }
//! if input.is_mouse_just_pressed(MouseButton::Left) { /* fire */ }
//! let (mx, my) = input.mouse_position();
//! input.frame_end();  // clear just_pressed / scroll_delta for next frame
//! ```
//!
//! # Event routing
//!
//! Inherits Blinc's dispatch rules — not invented here:
//!
//! - **Pointer + scroll** bubble through every ancestor of the hit /
//!   hovered element, so `capture_input(&root)` reliably sees every
//!   pointer-down / up / move / scroll regardless of subtree shape.
//! - **Keys** bubble leaf-to-root and stop at the first handler found.
//!   Focus is set implicitly on pointer-down — the clicked node *and*
//!   its full ancestor chain become focused, so keys reach your `Div`
//!   after the first click inside its subtree. But any descendant that
//!   handles keys itself (`text_input`, `code_editor`, or a child with
//!   its own `on_key_down`) will absorb the event and `capture_input`
//!   will never see it. Don't nest key-capturing widgets inside a
//!   region you want `blinc_input` to drive.
//!
//! [`DivInputExt::capture_input`]: DivInputExt::capture_input

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use blinc_core::events::{event_types, KeyCode, Modifiers};
use blinc_layout::div::Div;
use blinc_layout::event_handler::EventContext;

/// Mouse button identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    /// Extra buttons (4 and above on multi-button mice). `index` starts
    /// at `3` for the first extra button to match the underlying
    /// Blinc `mouse_button: u8` scheme.
    Other(u8),
}

impl MouseButton {
    fn from_index(idx: u8) -> Self {
        match idx {
            0 => MouseButton::Left,
            1 => MouseButton::Middle,
            2 => MouseButton::Right,
            n => MouseButton::Other(n),
        }
    }
}

/// Polling snapshot of input state. Cheap to clone (shared `Arc`).
///
/// Event sources (handlers registered via [`DivInputExt::capture_input`]
/// or manually) feed this by calling [`InputState::record`]. Sketches
/// read it via the query methods below and then call
/// [`InputState::frame_end`] once per frame to cycle transient state.
#[derive(Clone, Default)]
pub struct InputState {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    keys_down: HashSet<KeyCode>,
    keys_just_pressed: HashSet<KeyCode>,
    keys_just_released: HashSet<KeyCode>,
    buttons_down: HashSet<MouseButton>,
    buttons_just_pressed: HashSet<MouseButton>,
    buttons_just_released: HashSet<MouseButton>,
    mouse_x: f32,
    mouse_y: f32,
    /// Accumulated scroll delta since the last `frame_end`. Cleared
    /// each frame so `scroll_delta()` returns "this frame's scroll"
    /// rather than a running total.
    scroll_delta_x: f32,
    scroll_delta_y: f32,
    modifiers: Modifiers,
}

impl InputState {
    /// Fresh empty state.
    pub fn new() -> Self {
        Self::default()
    }

    // ── Queries ───────────────────────────────────────────────────────

    /// Is `key` currently held?
    pub fn is_key_down(&self, key: KeyCode) -> bool {
        self.inner.lock().unwrap().keys_down.contains(&key)
    }

    /// Did `key` transition from up to down since the previous
    /// [`frame_end`](Self::frame_end)?
    pub fn is_key_just_pressed(&self, key: KeyCode) -> bool {
        self.inner.lock().unwrap().keys_just_pressed.contains(&key)
    }

    /// Did `key` transition from down to up since the previous
    /// [`frame_end`](Self::frame_end)?
    pub fn is_key_just_released(&self, key: KeyCode) -> bool {
        self.inner.lock().unwrap().keys_just_released.contains(&key)
    }

    /// Is `button` currently held?
    pub fn is_mouse_down(&self, button: MouseButton) -> bool {
        self.inner.lock().unwrap().buttons_down.contains(&button)
    }

    /// Did `button` transition from up to down since the previous
    /// [`frame_end`](Self::frame_end)?
    pub fn is_mouse_just_pressed(&self, button: MouseButton) -> bool {
        self.inner
            .lock()
            .unwrap()
            .buttons_just_pressed
            .contains(&button)
    }

    /// Did `button` transition from down to up since the previous
    /// [`frame_end`](Self::frame_end)?
    pub fn is_mouse_just_released(&self, button: MouseButton) -> bool {
        self.inner
            .lock()
            .unwrap()
            .buttons_just_released
            .contains(&button)
    }

    /// Last observed mouse position in the event source's coordinate
    /// space (typically the capturing `Div`'s local coordinates —
    /// `(local_x, local_y)` from the event).
    pub fn mouse_position(&self) -> (f32, f32) {
        let s = self.inner.lock().unwrap();
        (s.mouse_x, s.mouse_y)
    }

    /// Accumulated scroll delta since the previous
    /// [`frame_end`](Self::frame_end). Cleared each frame.
    pub fn scroll_delta(&self) -> (f32, f32) {
        let s = self.inner.lock().unwrap();
        (s.scroll_delta_x, s.scroll_delta_y)
    }

    /// Current keyboard modifier flags (shift / ctrl / alt / meta).
    pub fn modifiers(&self) -> Modifiers {
        self.inner.lock().unwrap().modifiers
    }

    // ── Lifecycle ─────────────────────────────────────────────────────

    /// Record a Blinc [`EventContext`] into the state. Safe to call
    /// from any handler; unknown event types are ignored.
    pub fn record(&self, evt: &EventContext) {
        let mut s = self.inner.lock().unwrap();
        s.modifiers = Modifiers::new(evt.shift, evt.ctrl, evt.alt, evt.meta);

        match evt.event_type {
            event_types::KEY_DOWN => {
                let key = KeyCode(evt.key_code);
                if s.keys_down.insert(key) {
                    s.keys_just_pressed.insert(key);
                }
            }
            event_types::KEY_UP => {
                let key = KeyCode(evt.key_code);
                if s.keys_down.remove(&key) {
                    s.keys_just_released.insert(key);
                }
            }
            event_types::POINTER_DOWN => {
                let btn = MouseButton::from_index(evt.mouse_button);
                s.mouse_x = evt.local_x;
                s.mouse_y = evt.local_y;
                if s.buttons_down.insert(btn) {
                    s.buttons_just_pressed.insert(btn);
                }
            }
            event_types::POINTER_UP => {
                let btn = MouseButton::from_index(evt.mouse_button);
                s.mouse_x = evt.local_x;
                s.mouse_y = evt.local_y;
                if s.buttons_down.remove(&btn) {
                    s.buttons_just_released.insert(btn);
                }
            }
            event_types::POINTER_MOVE => {
                s.mouse_x = evt.local_x;
                s.mouse_y = evt.local_y;
            }
            event_types::SCROLL => {
                s.scroll_delta_x += evt.scroll_delta_x;
                s.scroll_delta_y += evt.scroll_delta_y;
            }
            _ => {}
        }
    }

    /// Call once per frame, after reading inputs, to clear
    /// edge-triggered state (`*_just_pressed`, `*_just_released`,
    /// `scroll_delta`). Positions and held keys / buttons persist.
    pub fn frame_end(&self) {
        let mut s = self.inner.lock().unwrap();
        s.keys_just_pressed.clear();
        s.keys_just_released.clear();
        s.buttons_just_pressed.clear();
        s.buttons_just_released.clear();
        s.scroll_delta_x = 0.0;
        s.scroll_delta_y = 0.0;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Div helper
// ─────────────────────────────────────────────────────────────────────────────

/// Extension trait that wires up the usual event handlers on a `Div`
/// to feed an [`InputState`] without the caller having to register each
/// one by hand.
pub trait DivInputExt: Sized {
    /// Register key-up/down, pointer-up/down/move, and scroll
    /// handlers that all route into `input`. Returns the modified `Div`.
    ///
    /// Key events still require focus on the target — see the
    /// crate-level docs.
    fn capture_input(self, input: &InputState) -> Self;
}

impl DivInputExt for Div {
    fn capture_input(self, input: &InputState) -> Self {
        let i_kd = input.clone();
        let i_ku = input.clone();
        let i_pd = input.clone();
        let i_pu = input.clone();
        let i_pm = input.clone();
        let i_sc = input.clone();
        self.on_key_down(move |e| i_kd.record(e))
            .on_key_up(move |e| i_ku.record(e))
            .on_event(event_types::POINTER_DOWN, move |e| i_pd.record(e))
            .on_event(event_types::POINTER_UP, move |e| i_pu.record(e))
            .on_event(event_types::POINTER_MOVE, move |e| i_pm.record(e))
            .on_scroll(move |e| i_sc.record(e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blinc_core::events::KeyCode;
    use blinc_layout::event_handler::EventContext;
    use blinc_layout::tree::LayoutNodeId;

    fn blank_evt(event_type: u32) -> EventContext {
        let mut e = EventContext::new(event_type, LayoutNodeId::default());
        // EventContext::new fills in defaults; we tweak per test.
        e.key_code = 0;
        e.mouse_button = 0;
        e
    }

    #[test]
    fn key_down_sets_held_and_just_pressed() {
        let input = InputState::new();
        let mut e = blank_evt(event_types::KEY_DOWN);
        e.key_code = KeyCode::SPACE.0;
        input.record(&e);

        assert!(input.is_key_down(KeyCode::SPACE));
        assert!(input.is_key_just_pressed(KeyCode::SPACE));

        // frame_end clears the edge flag but keeps `held`.
        input.frame_end();
        assert!(input.is_key_down(KeyCode::SPACE));
        assert!(!input.is_key_just_pressed(KeyCode::SPACE));
    }

    #[test]
    fn key_up_clears_held_and_sets_just_released() {
        let input = InputState::new();
        let mut e = blank_evt(event_types::KEY_DOWN);
        e.key_code = KeyCode::A.0;
        input.record(&e);
        input.frame_end();

        let mut e = blank_evt(event_types::KEY_UP);
        e.key_code = KeyCode::A.0;
        input.record(&e);

        assert!(!input.is_key_down(KeyCode::A));
        assert!(input.is_key_just_released(KeyCode::A));
    }

    #[test]
    fn repeat_key_down_does_not_re_fire_just_pressed_within_frame() {
        // Blinc emits KEY_DOWN with `repeat: true` while a key is
        // held. We want is_just_pressed to fire only on the initial
        // down -> up -> down transition.
        let input = InputState::new();
        let mut e = blank_evt(event_types::KEY_DOWN);
        e.key_code = KeyCode::B.0;
        input.record(&e);
        input.frame_end();

        // Second KEY_DOWN (auto-repeat) should not re-trigger
        // just_pressed because the key is already in keys_down.
        input.record(&e);
        assert!(input.is_key_down(KeyCode::B));
        assert!(!input.is_key_just_pressed(KeyCode::B));
    }

    #[test]
    fn pointer_tracks_position_and_buttons() {
        let input = InputState::new();
        let mut e = blank_evt(event_types::POINTER_MOVE);
        e.local_x = 42.0;
        e.local_y = 17.0;
        input.record(&e);
        assert_eq!(input.mouse_position(), (42.0, 17.0));

        let mut down = blank_evt(event_types::POINTER_DOWN);
        down.mouse_button = 0;
        down.local_x = 10.0;
        down.local_y = 20.0;
        input.record(&down);
        assert!(input.is_mouse_down(MouseButton::Left));
        assert!(input.is_mouse_just_pressed(MouseButton::Left));

        input.frame_end();
        let mut up = blank_evt(event_types::POINTER_UP);
        up.mouse_button = 0;
        input.record(&up);
        assert!(!input.is_mouse_down(MouseButton::Left));
        assert!(input.is_mouse_just_released(MouseButton::Left));
    }

    #[test]
    fn scroll_accumulates_then_clears_on_frame_end() {
        let input = InputState::new();
        let mut e = blank_evt(event_types::SCROLL);
        e.scroll_delta_x = 1.0;
        e.scroll_delta_y = -2.0;
        input.record(&e);
        input.record(&e);
        assert_eq!(input.scroll_delta(), (2.0, -4.0));

        input.frame_end();
        assert_eq!(input.scroll_delta(), (0.0, 0.0));
    }

    #[test]
    fn modifiers_reflect_latest_event() {
        let input = InputState::new();
        let mut e = blank_evt(event_types::KEY_DOWN);
        e.shift = true;
        e.ctrl = false;
        e.key_code = KeyCode::Z.0;
        input.record(&e);
        let m = input.modifiers();
        assert!(m.shift());
        assert!(!m.ctrl());
    }
}
