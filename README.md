# blinc_input

Polling-style input state for [Blinc](https://github.com/project-blinc/Blinc)
sketches and canvases.

Blinc's event model is callback-based — `div().on_key_down(|e| …)`.
Sketches and games typically want the *polling* shape: inside `draw()`,
ask `is_key_down(KeyCode::SPACE)` or `mouse_position()`. `blinc_input`
bridges the two.

```rust
use blinc_input::{InputState, DivInputExt, MouseButton};
use blinc_core::events::KeyCode;

let input = InputState::new();

// Wire event handlers on the root of your tree:
let tree = div().w_full().h_full().capture_input(&input)
    .child(/* … */);

// Inside your Sketch::draw:
if input.is_key_down(KeyCode::SPACE)        { /* jump */ }
if input.is_mouse_just_pressed(MouseButton::Left) { /* fire */ }
let (mx, my) = input.mouse_position();
let (sx, sy) = input.scroll_delta();

input.frame_end();  // clear edge-triggered state for next frame
```

## What's tracked

- **Keys:** held / just-pressed / just-released, with `KeyCode` from
  `blinc_core::events`.
- **Mouse buttons:** `MouseButton::{Left, Middle, Right, Other(n)}`,
  same held / just-pressed / just-released surface.
- **Mouse position:** `(local_x, local_y)` in the capturing `Div`'s
  coordinate space.
- **Scroll delta:** accumulated per frame, cleared on `frame_end`.
- **Modifiers:** shift / ctrl / alt / meta snapshot from the most
  recent event.

See [BACKLOG.md](./BACKLOG.md) for planned additions — gamepad,
virtual axis mapping, touch tracking, action-binding layer.

## Event routing — what's reliable and what isn't

Built on top of Blinc's event router, so behavior inherits Blinc's
rules:

- **Pointer + scroll** bubble through every ancestor of the hit / hovered
  element. `capture_input(&root)` reliably sees every pointer-down,
  pointer-up, pointer-move, and scroll — no caveat.
- **Keys** bubble *leaf-to-root and stop at the first handler*. Focus
  is set implicitly on pointer-down (the clicked node *and* its full
  ancestor chain become focused); clicking outside clears it. So:
  - In a sketch whose subtree has no child key handlers, keys reach
    `capture_input` after the first pointer-down anywhere inside.
  - If the subtree contains a widget that handles keys itself
    (`text_input`, `code_editor`, custom `on_key_down` on a child),
    that descendant absorbs the key and `capture_input` never sees it.
    Keep key-capturing widgets outside the region you want to drive
    with `blinc_input`, or read from them directly.

## License

Apache-2.0.
