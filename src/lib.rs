//! Polling-style input state for Blinc sketches and canvases.
//!
//! Blinc's native event model is callback-based — `div().on_key_down(|e| …)`
//! fires whenever the focused widget sees a key. Sketches and games
//! typically want the *polling* shape: inside `draw()`, ask
//! `is_key_down(KeyCode::SPACE)` or `mouse_position()`. This crate
//! bridges the two.
//!
//! [`InputState`] owns the polling snapshot. Feed it from Blinc's event
//! stream and query it from `draw`. Call [`InputState::frame_end`] once
//! per frame to clear transient edge-trigger state
//! (`is_key_just_pressed` / `is_mouse_just_pressed`).
//!
//! # Recommended wiring — via `blinc_canvas_kit`
//!
//! If you're building on `Sketch` or `CanvasKit`, route events through
//! canvas-kit's hooks — they're already scoped to the canvas's own
//! bounds so you don't touch `Div` handles yourself:
//!
//! ```ignore
//! // Inside sketch(...):
//! use blinc_canvas_kit::prelude::*;   // brings in SketchEvents
//! use blinc_input::InputState;
//!
//! let input = InputState::new();
//! let i = input.clone();
//! let tree = sketch("demo", my_sketch).on_canvas_events(move |e| i.record(e));
//! ```
//!
//! ```ignore
//! // Inside CanvasKit:
//! let mut kit = CanvasKit::new("main");
//! let i = input.clone();
//! kit.on_any_event(move |e| i.record(e));
//! ```
//!
//! # Bare-Div escape hatch
//!
//! If you're not using canvas-kit, the [`DivInputExt::capture_input`]
//! helper attaches the same bundle of handlers to any `Div`:
//!
//! ```ignore
//! use blinc_input::{InputState, DivInputExt};
//!
//! let input = InputState::new();
//! let tree = div().w_full().h_full().capture_input(&input).child(/* … */);
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
    /// Per-gamepad state. Indexed by gamepad slot; lazily grown as
    /// new gamepads connect. Empty without the `gamepad` feature.
    gamepads: Vec<GamepadSnapshot>,
    /// Backend driving `gamepads`. `None` means "not initialised yet
    /// (or `gamepad` feature not compiled)". Behind a Mutex via the
    /// outer `Inner`.
    #[cfg(all(feature = "gamepad", not(target_arch = "wasm32")))]
    gilrs: Option<gilrs::Gilrs>,
    /// gilrs → slot mapping. A freshly connected gamepad claims the
    /// first free slot so indices stay compact as controllers come
    /// and go.
    #[cfg(all(feature = "gamepad", not(target_arch = "wasm32")))]
    gamepad_slots: std::collections::HashMap<gilrs::GamepadId, usize>,
}

/// Per-gamepad polling state. Populated by
/// [`InputState::poll_gamepads`] each frame from the active backend
/// (gilrs on desktop; Web Gamepad API planned for wasm).
#[derive(Debug, Default, Clone)]
pub struct GamepadSnapshot {
    /// Whether this slot currently has a connected controller. If
    /// `false`, all `is_gamepad_button_down` / `gamepad_axis` reads
    /// return their default (`false` / `0.0`).
    pub connected: bool,
    pub buttons_down: HashSet<GamepadButton>,
    pub buttons_just_pressed: HashSet<GamepadButton>,
    pub buttons_just_released: HashSet<GamepadButton>,
    pub axes: std::collections::HashMap<GamepadAxis, f32>,
}

/// Normalised gamepad button identifier. Mapping is middleware-provided
/// (gilrs on desktop) and follows the "south / east / west / north"
/// convention so the API stays stable across Xbox, PlayStation, and
/// Switch Pro layouts — callers write
/// `is_gamepad_button_down(0, GamepadButton::South)` instead of picking
/// between "A" and "Cross" per platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GamepadButton {
    South,          // A / Cross / B (Nintendo layout)
    East,           // B / Circle / A
    West,           // X / Square / Y
    North,          // Y / Triangle / X
    LeftShoulder,
    RightShoulder,
    LeftTrigger,    // Digital press — use `GamepadAxis::LeftTrigger` for analog
    RightTrigger,
    Select,         // Back / Share
    Start,          // Menu / Options
    LeftThumb,      // L3 — pressing the stick down
    RightThumb,     // R3
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
    Mode,           // Xbox / PS home button. Some platforms reserve it.
}

/// Normalised gamepad analog axis. Stick axes run `-1.0 ..= 1.0`
/// (up / right positive). Triggers run `0.0 ..= 1.0`. Deadzone
/// handling is the caller's job — we pass middleware values through
/// unfiltered so games can pick their own curves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GamepadAxis {
    LeftStickX,
    LeftStickY,
    RightStickX,
    RightStickY,
    LeftTrigger,
    RightTrigger,
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

    // ── Gamepad queries ───────────────────────────────────────────────
    //
    // All methods degrade gracefully when the `gamepad` feature isn't
    // compiled, or when no controller is connected to the requested
    // slot — `count` is `0`, buttons read as released, axes as `0.0`.
    // Call [`Self::poll_gamepads`] once per frame to drain middleware
    // events into the snapshot these queries read.

    /// Number of gamepads the backend has seen (including disconnected
    /// slots whose state is retained for stability of indices across
    /// reconnects). `is_gamepad_connected(i)` narrows to live slots.
    pub fn gamepad_count(&self) -> usize {
        self.inner.lock().unwrap().gamepads.len()
    }

    /// Whether gamepad `i` is currently plugged in.
    pub fn is_gamepad_connected(&self, gamepad: usize) -> bool {
        self.inner
            .lock()
            .unwrap()
            .gamepads
            .get(gamepad)
            .is_some_and(|g| g.connected)
    }

    /// Is `button` currently held on gamepad `gamepad`?
    pub fn is_gamepad_button_down(&self, gamepad: usize, button: GamepadButton) -> bool {
        self.inner
            .lock()
            .unwrap()
            .gamepads
            .get(gamepad)
            .is_some_and(|g| g.connected && g.buttons_down.contains(&button))
    }

    /// Did `button` transition down → up this frame?
    pub fn is_gamepad_button_just_pressed(&self, gamepad: usize, button: GamepadButton) -> bool {
        self.inner
            .lock()
            .unwrap()
            .gamepads
            .get(gamepad)
            .is_some_and(|g| g.connected && g.buttons_just_pressed.contains(&button))
    }

    /// Did `button` transition up → down this frame?
    pub fn is_gamepad_button_just_released(&self, gamepad: usize, button: GamepadButton) -> bool {
        self.inner
            .lock()
            .unwrap()
            .gamepads
            .get(gamepad)
            .is_some_and(|g| g.connected && g.buttons_just_released.contains(&button))
    }

    /// Current analog value for `axis` on gamepad `gamepad`. Sticks
    /// return `-1.0 ..= 1.0`, triggers `0.0 ..= 1.0`. No deadzone
    /// is applied — filter per-axis at the call site.
    pub fn gamepad_axis(&self, gamepad: usize, axis: GamepadAxis) -> f32 {
        self.inner
            .lock()
            .unwrap()
            .gamepads
            .get(gamepad)
            .and_then(|g| if g.connected { g.axes.get(&axis).copied() } else { None })
            .unwrap_or(0.0)
    }

    /// Drive the gamepad backend — drains pending controller events
    /// and updates the per-slot snapshot. Call once per frame before
    /// any gamepad queries (or not at all if you aren't using
    /// gamepads — cheap no-op when the feature's off).
    #[cfg(all(feature = "gamepad", not(target_arch = "wasm32")))]
    pub fn poll_gamepads(&self) {
        use gilrs::{Button, EventType};
        let mut inner = self.inner.lock().unwrap();
        // Lazy-init gilrs on the first poll so the HID subsystem
        // isn't touched by callers who never ask for gamepads.
        if inner.gilrs.is_none() {
            match gilrs::Gilrs::new() {
                Ok(g) => inner.gilrs = Some(g),
                Err(e) => {
                    tracing::warn!("gilrs init failed: {e:?}");
                    return;
                }
            }
        }
        // Edge state persists for exactly one frame — clear before
        // draining new events so `just_pressed` reflects only this
        // frame's transitions.
        for g in inner.gamepads.iter_mut() {
            g.buttons_just_pressed.clear();
            g.buttons_just_released.clear();
        }
        // Drain every pending controller event.
        while let Some(event) = inner.gilrs.as_mut().and_then(|g| g.next_event()) {
            // Assign a slot index the first time we see a gamepad id.
            // Two-step lookup to keep the borrow checker happy — we
            // can't touch `inner.gamepads` inside an `or_insert_with`
            // closure that itself borrows `inner.gamepad_slots`.
            let slot = if let Some(&s) = inner.gamepad_slots.get(&event.id) {
                s
            } else {
                let idx = inner.gamepads.len();
                inner.gamepads.push(GamepadSnapshot::default());
                inner.gamepad_slots.insert(event.id, idx);
                idx
            };
            if slot >= inner.gamepads.len() {
                // Shouldn't happen but be defensive.
                continue;
            }
            let snap = &mut inner.gamepads[slot];
            match event.event {
                EventType::Connected => snap.connected = true,
                EventType::Disconnected => {
                    snap.connected = false;
                    snap.buttons_down.clear();
                    snap.axes.clear();
                }
                EventType::ButtonPressed(btn, _) => {
                    if let Some(b) = map_gilrs_button(btn) {
                        if snap.buttons_down.insert(b) {
                            snap.buttons_just_pressed.insert(b);
                        }
                    }
                }
                EventType::ButtonReleased(btn, _) => {
                    if let Some(b) = map_gilrs_button(btn) {
                        if snap.buttons_down.remove(&b) {
                            snap.buttons_just_released.insert(b);
                        }
                    }
                }
                EventType::AxisChanged(axis, value, _) => {
                    if let Some(a) = map_gilrs_axis(axis) {
                        snap.axes.insert(a, value);
                    }
                }
                // ButtonChanged covers analog trigger values. gilrs
                // reports L2/R2 as buttons whose "value" ranges
                // 0..1 — record as the corresponding Trigger axis so
                // the analog read works alongside the digital flag.
                EventType::ButtonChanged(btn, value, _) => {
                    let axis = match btn {
                        Button::LeftTrigger2 => Some(GamepadAxis::LeftTrigger),
                        Button::RightTrigger2 => Some(GamepadAxis::RightTrigger),
                        _ => None,
                    };
                    if let Some(a) = axis {
                        snap.axes.insert(a, value);
                    }
                }
                _ => {}
            }
        }
    }

    /// Poll the browser's Web Gamepad API via `navigator.getGamepads()`.
    /// Works with DualShock, Xbox, Switch Pro, and any other
    /// controller the browser exposes — the spec normalises all of
    /// them to the "Standard Gamepad" button layout, so our enum
    /// mapping (see [`map_web_button_index`]) stays one-to-one with
    /// the array index regardless of controller brand.
    #[cfg(all(feature = "gamepad", target_arch = "wasm32"))]
    pub fn poll_gamepads(&self) {
        use wasm_bindgen::JsCast;
        let mut inner = self.inner.lock().unwrap();
        let Some(window) = web_sys::window() else { return; };
        let Ok(pads_js) = window.navigator().get_gamepads() else { return; };
        let pads = js_sys::Array::from(&pads_js);

        // Grow snapshot vec to match the browser's slot count. Empty
        // slots stay `connected = false`.
        while inner.gamepads.len() < pads.length() as usize {
            inner.gamepads.push(GamepadSnapshot::default());
        }

        for (slot, pad_val) in pads.iter().enumerate() {
            if slot >= inner.gamepads.len() {
                break;
            }
            // A missing gamepad is represented as `null`.
            let Ok(pad) = pad_val.dyn_into::<web_sys::Gamepad>() else {
                let snap = &mut inner.gamepads[slot];
                if snap.connected {
                    snap.connected = false;
                    snap.buttons_down.clear();
                    snap.axes.clear();
                }
                continue;
            };

            // Edge state is this-frame-only. Clear before repopulating.
            let snap = &mut inner.gamepads[slot];
            snap.buttons_just_pressed.clear();
            snap.buttons_just_released.clear();
            snap.connected = pad.connected();

            // Buttons: the Standard Gamepad layout defines index → role.
            let buttons = pad.buttons();
            for i in 0..buttons.length() {
                let Some(button) = buttons.get(i).dyn_ref::<web_sys::GamepadButton>().cloned()
                else {
                    continue;
                };
                let Some(mapped) = map_web_button_index(i) else { continue; };
                let pressed = button.pressed();
                let was_down = snap.buttons_down.contains(&mapped);
                if pressed && !was_down {
                    snap.buttons_down.insert(mapped);
                    snap.buttons_just_pressed.insert(mapped);
                } else if !pressed && was_down {
                    snap.buttons_down.remove(&mapped);
                    snap.buttons_just_released.insert(mapped);
                }
                // Analog trigger value: the browser exposes 0..1 via
                // `GamepadButton.value`; mirror into the axis sink so
                // the same accessor works for gilrs + web.
                let axis = match i {
                    6 => Some(GamepadAxis::LeftTrigger),
                    7 => Some(GamepadAxis::RightTrigger),
                    _ => None,
                };
                if let Some(a) = axis {
                    snap.axes.insert(a, button.value() as f32);
                }
            }

            // Axes: Standard Gamepad layout uses indices 0..4 for
            // LeftStickX/Y, RightStickX/Y. Browsers invert Y
            // relative to gilrs (web: +Y is down), but the `Standard`
            // mapping guarantee means indices are stable.
            let axes = pad.axes();
            let axis_map = [
                GamepadAxis::LeftStickX,
                GamepadAxis::LeftStickY,
                GamepadAxis::RightStickX,
                GamepadAxis::RightStickY,
            ];
            for (i, a) in axis_map.iter().enumerate() {
                let Some(v) = axes.get(i as u32).as_f64() else { continue; };
                // Web Y is down-positive; flip so consumers get the
                // same sign convention as gilrs (up-positive).
                let signed = if matches!(a, GamepadAxis::LeftStickY | GamepadAxis::RightStickY) {
                    -v as f32
                } else {
                    v as f32
                };
                snap.axes.insert(*a, signed);
            }
        }
    }

    /// No-op when the `gamepad` feature isn't enabled. Kept for API
    /// symmetry so callers don't need to cfg-guard the call site.
    #[cfg(not(feature = "gamepad"))]
    pub fn poll_gamepads(&self) {
        // no-op
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

// ─────────────────────────────────────────────────────────────────────────────
// gilrs ↔ Blinc enum mapping (desktop, behind the `gamepad` feature)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(all(feature = "gamepad", not(target_arch = "wasm32")))]
fn map_gilrs_button(btn: gilrs::Button) -> Option<GamepadButton> {
    use gilrs::Button::*;
    Some(match btn {
        South => GamepadButton::South,
        East => GamepadButton::East,
        West => GamepadButton::West,
        North => GamepadButton::North,
        LeftTrigger => GamepadButton::LeftShoulder,
        RightTrigger => GamepadButton::RightShoulder,
        LeftTrigger2 => GamepadButton::LeftTrigger,
        RightTrigger2 => GamepadButton::RightTrigger,
        Select => GamepadButton::Select,
        Start => GamepadButton::Start,
        LeftThumb => GamepadButton::LeftThumb,
        RightThumb => GamepadButton::RightThumb,
        DPadUp => GamepadButton::DPadUp,
        DPadDown => GamepadButton::DPadDown,
        DPadLeft => GamepadButton::DPadLeft,
        DPadRight => GamepadButton::DPadRight,
        Mode => GamepadButton::Mode,
        Unknown | C | Z => return None,
    })
}

/// Map a Web Gamepad API button-array index to our normalised enum.
/// Follows the "Standard Gamepad" mapping defined in the W3C Gamepad
/// spec, which every major browser implements identically for
/// DualShock / DualSense, Xbox, Switch Pro, and generic XInput pads.
#[cfg(all(feature = "gamepad", target_arch = "wasm32"))]
fn map_web_button_index(i: u32) -> Option<GamepadButton> {
    Some(match i {
        0 => GamepadButton::South,          // Cross / A
        1 => GamepadButton::East,           // Circle / B
        2 => GamepadButton::West,           // Square / X
        3 => GamepadButton::North,          // Triangle / Y
        4 => GamepadButton::LeftShoulder,   // L1 / LB
        5 => GamepadButton::RightShoulder,  // R1 / RB
        6 => GamepadButton::LeftTrigger,    // L2 / LT (digital press)
        7 => GamepadButton::RightTrigger,   // R2 / RT
        8 => GamepadButton::Select,         // Share / Back
        9 => GamepadButton::Start,          // Options / Menu
        10 => GamepadButton::LeftThumb,     // L3 (stick press)
        11 => GamepadButton::RightThumb,    // R3
        12 => GamepadButton::DPadUp,
        13 => GamepadButton::DPadDown,
        14 => GamepadButton::DPadLeft,
        15 => GamepadButton::DPadRight,
        16 => GamepadButton::Mode,          // PS button / Xbox home
        _ => return None,
    })
}

#[cfg(all(feature = "gamepad", not(target_arch = "wasm32")))]
fn map_gilrs_axis(axis: gilrs::Axis) -> Option<GamepadAxis> {
    use gilrs::Axis::*;
    Some(match axis {
        LeftStickX => GamepadAxis::LeftStickX,
        LeftStickY => GamepadAxis::LeftStickY,
        RightStickX => GamepadAxis::RightStickX,
        RightStickY => GamepadAxis::RightStickY,
        LeftZ => GamepadAxis::LeftTrigger,
        RightZ => GamepadAxis::RightTrigger,
        // DPad-as-axis (some drivers expose it this way) and
        // unknown axes are handled as button events elsewhere.
        _ => return None,
    })
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
