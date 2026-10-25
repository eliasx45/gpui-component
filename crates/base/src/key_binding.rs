use gpui::{Action, App, KeyBinding, KeyBindingContextPredicate};

/// A key binding whose keystrokes may name `secondary-` (Cmd on macOS, Ctrl elsewhere).
///
/// Ghostex fork: GPUI's `KeyBinding::new` parses its keystrokes without an
/// operating system since Zed made `Modifiers::secondary` web-platform aware
/// (389c22e1c7), so `KeyBinding::new("secondary-enter", ..)` panics. This
/// resolves `secondary-` from `cx.platform_info().os`, as `KeyBinding::load`
/// does.
pub fn bind_secondary<A: Action>(
    keystrokes: &str,
    action: A,
    context: Option<&str>,
    cx: &App,
) -> KeyBinding {
    KeyBinding::load(
        keystrokes,
        Box::new(action),
        context.map(|context| KeyBindingContextPredicate::parse(context).unwrap().into()),
        false,
        None,
        cx,
    )
    .unwrap_or_else(|error| panic!("invalid key binding {keystrokes:?}: {error}"))
}
