# blinc_input — Backlog

Outstanding work, ordered by demand.

---

## Gamepad support

- [x] **Core gamepad polling.** `gamepad` feature gates both desktop
  (gilrs 0.11) and wasm (Web Gamepad API via web-sys) backends.
  `InputState::poll_gamepads()` drains controller events into
  per-slot `GamepadSnapshot`s; query with
  `is_gamepad_button_down(slot, GamepadButton::South)`,
  `gamepad_axis(slot, GamepadAxis::LeftStickX)`, etc. Button enum
  uses the South/East/West/North convention so Xbox / PlayStation
  / Switch Pro all share one API. Web path uses the Standard
  Gamepad layout (covers DualShock / DualSense / Xbox / generic
  XInput — browsers remap them identically).

- [ ] **Connection change events.** Currently connection state is
  exposed via the `connected` flag on `GamepadSnapshot` and
  `is_gamepad_connected(i)`. Dispatching a one-shot event on
  transitions would let sketches react (re-bind UI, update HUD)
  without a per-frame equality check against last frame's state.

---

## Touch and gesture

- [ ] **Multi-touch tracking**
  - **Why:** Tablet / mobile sketches need pinch / drag-with-two-fingers
    independent of POINTER_DOWN.
  - **How:** Extend `InputState` with `touches()` returning a map of
    `TouchId -> TouchPoint { x, y, pressure }`. Wire via
    `event_types::PINCH`, `ROTATE`, and per-touch `POINTER_*` once
    Blinc exposes per-touch IDs.

- [ ] **Pinch / rotate** accumulators exposed alongside scroll delta.

---

## Action / axis binding layer

- [x] **Named actions over raw keys/buttons.** `ActionMap` with
  `Binding` (Key / Mouse / GamepadButton / GamepadAxisThreshold)
  and `AxisBinding` (KeyPair / GamepadAxis / GamepadButtonPair).
  `InputState::{action_down, action_just_pressed,
  action_just_released, axis}`. Actions resolve via OR across
  bindings; axes resolve via largest-magnitude. Install via
  `InputState::set_actions`; swap to rebind. Introspection via
  `ActionMap::{actions, axes}` iterators for rebind UIs.

- [ ] **Binding persistence** to TOML/JSON for user-facing rebind UIs.
  Needs `serde` derives on `Binding` / `AxisBinding` / `ActionMap`
  behind an optional feature. Not started — the in-memory
  `ActionMap` is the foundation; serialization is mechanical once
  an app wants it.

---

## Text / IME

- [ ] **Pending text input queue**
  - **Why:** Forms / in-game chat want the `TEXT_INPUT` stream, not
    raw key codes.
  - **How:** Record `event_types::TEXT_INPUT` into a FIFO; drain via
    `input.take_text_input()` in the sketch.

---

## Ergonomics

- [x] **Automated `capture_input` + `frame_end` wiring for 3D scenes**
  — resolved on the canvas-kit side as
  `SceneKit3D::with_input(&InputState)` (Blinc commit 922cb316).
  Caller keeps owning the `InputState` and accessing it via
  closure capture; the kit handles `capture_input` on the outer
  `Div` and `frame_end()` per paint. The original framing
  (`ctx.input() -> &InputState` + a `Sketch` wrapper) turned out
  to be the wrong shape: readers already have the state through
  closure capture, so rerouting access through a new context
  type would trade one ergonomic problem for another. Implemented
  without a feature gate because `blinc_input` is small and
  canvas_kit's 3D scene use-case always wants it.

---

## Non-goals

- **Rebinding UI widgets** — belongs to `blinc_cn` or downstream
  apps, not the input layer.
- **Input recording / replay** — belongs to a separate `blinc_replay`
  crate if ever needed.
