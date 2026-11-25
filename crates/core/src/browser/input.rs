//! What the user does on the picture of a page, as Chrome takes it: the mouse, the keys, and
//! what is typed into the address bar. Pure.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The keys held down with a click or a key, as the browser's own events say them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is a key of its own, held or not, as the browser's events have them"
)]
pub struct Modifiers {
    /// ⌥, or Alt.
    pub alt: bool,
    /// Control.
    pub ctrl: bool,
    /// ⌘, or the Windows key.
    pub meta: bool,
    /// Shift.
    pub shift: bool,
}

impl Modifiers {
    /// As the DevTools protocol counts them: a bit each.
    fn bits(self) -> u32 {
        u32::from(self.alt)
            | u32::from(self.ctrl) << 1
            | u32::from(self.meta) << 2
            | u32::from(self.shift) << 3
    }
}

/// What the mouse did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseKind {
    /// A button went down.
    Down,
    /// A button came up.
    Up,
    /// It moved.
    Move,
    /// The wheel or the trackpad scrolled.
    Wheel,
}

/// One thing the mouse did on the picture of a page, where on the page it was.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MouseInput {
    /// What it did.
    pub kind: MouseKind,
    /// Where, in the page's CSS pixels from its top left corner.
    pub x: f64,
    /// Where, in the page's CSS pixels from its top left corner.
    pub y: f64,
    /// Which button: `left`, `middle`, `right`, `back`, `forward`, or `none`.
    #[serde(default = "no_button")]
    pub button: String,
    /// The buttons held down, a bit each, as the browser's own events say them.
    #[serde(default)]
    pub buttons: u32,
    /// The how-manyth click this is in a quick row: 2 for a double click.
    #[serde(default)]
    pub click_count: u32,
    /// How far the wheel scrolled, in CSS pixels.
    #[serde(default)]
    pub delta_x: f64,
    /// How far the wheel scrolled, in CSS pixels.
    #[serde(default)]
    pub delta_y: f64,
    /// The keys held down with it.
    #[serde(default)]
    pub modifiers: Modifiers,
}

fn no_button() -> String {
    "none".to_string()
}

/// A key going down or coming up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyKind {
    /// It went down.
    Down,
    /// It came up.
    Up,
}

/// One key pressed or let go on the picture of a page, as the browser's own event says it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyInput {
    /// Down or up.
    pub kind: KeyKind,
    /// What the key means: `a`, `A`, `Enter`, `ArrowLeft`.
    pub key: String,
    /// Which key it is on the keyboard: `KeyA`, `Enter`.
    #[serde(default)]
    pub code: String,
    /// The key's number, as `keyCode` says it.
    #[serde(default)]
    pub key_code: u32,
    /// The keys held down with it.
    #[serde(default)]
    pub modifiers: Modifiers,
    /// Whether it is held down and repeating.
    #[serde(default)]
    pub repeat: bool,
}

/// `Input.dispatchMouseEvent`'s parameters for what the mouse did.
pub(crate) fn mouse(input: &MouseInput) -> Value {
    let kind = match input.kind {
        MouseKind::Down => "mousePressed",
        MouseKind::Up => "mouseReleased",
        MouseKind::Move => "mouseMoved",
        MouseKind::Wheel => "mouseWheel",
    };
    let button = match input.button.as_str() {
        known @ ("left" | "middle" | "right" | "back" | "forward") => known,
        _ => "none",
    };
    json!({
        "type": kind,
        "x": input.x,
        "y": input.y,
        "button": if input.kind == MouseKind::Wheel { "none" } else { button },
        "buttons": input.buttons,
        "clickCount": input.click_count,
        "deltaX": input.delta_x,
        "deltaY": input.delta_y,
        "modifiers": input.modifiers.bits(),
    })
}

/// `Input.dispatchKeyEvent`'s parameters for a key. A key that types something goes down
/// with its text; one that does not goes down raw, as Chrome's own keyboard sends it. On a
/// Mac, the shortcuts for editing text are sent as the commands they stand for, since a page
/// in Chrome without a window has no menu that would turn them into those.
pub(crate) fn key(input: &KeyInput) -> Value {
    let mut params = json!({
        "key": input.key,
        "code": input.code,
        "windowsVirtualKeyCode": input.key_code,
        "nativeVirtualKeyCode": input.key_code,
        "modifiers": input.modifiers.bits(),
        "autoRepeat": input.repeat,
    });
    if input.kind == KeyKind::Up {
        params["type"] = json!("keyUp");
        return params;
    }
    let commands = editing_commands(input);
    if !commands.is_empty() {
        params["commands"] = json!(commands);
    }
    match typed_text(input) {
        Some(text) => {
            params["type"] = json!("keyDown");
            params["text"] = json!(text);
            params["unmodifiedText"] = json!(text);
        }
        None => params["type"] = json!("rawKeyDown"),
    }
    params
}

/// What a key types, if it types anything.
fn typed_text(input: &KeyInput) -> Option<String> {
    let held = input.modifiers;
    if held.ctrl || held.meta {
        return None;
    }
    match input.key.as_str() {
        "Enter" => Some("\r".to_string()),
        key if key.chars().count() == 1 => Some(key.to_string()),
        _ => None,
    }
}

/// The editing commands a Mac's shortcut stands for. ⌘V is not among them: Chrome without a
/// window has a clipboard of its own, so the window pastes the user's text itself.
fn editing_commands(input: &KeyInput) -> Vec<&'static str> {
    let held = input.modifiers;
    if held.ctrl {
        return vec![];
    }
    let key = input.key.to_lowercase();
    let command = match (held.meta, held.alt, held.shift, key.as_str()) {
        (true, false, false, "a") => "selectAll",
        (true, false, false, "z") => "undo",
        (true, false, true, "z") => "redo",
        (true, false, false, "x") => "cut",
        (true, false, false, "c") => "copy",
        (true, false, false, "backspace") => "deleteToBeginningOfLine",
        (false, true, false, "backspace") => "deleteWordBackward",
        (true, false, shift, "arrowleft") => pick(shift, "moveToBeginningOfLine"),
        (true, false, shift, "arrowright") => pick(shift, "moveToEndOfLine"),
        (true, false, shift, "arrowup") => pick(shift, "moveToBeginningOfDocument"),
        (true, false, shift, "arrowdown") => pick(shift, "moveToEndOfDocument"),
        (false, true, shift, "arrowleft") => pick(shift, "moveWordLeft"),
        (false, true, shift, "arrowright") => pick(shift, "moveWordRight"),
        _ => return vec![],
    };
    vec![command]
}

/// The command, or the one that also widens the selection when Shift is held.
fn pick(shift: bool, command: &'static str) -> &'static str {
    if !shift {
        return command;
    }
    match command {
        "moveToBeginningOfLine" => "moveToBeginningOfLineAndModifySelection",
        "moveToEndOfLine" => "moveToEndOfLineAndModifySelection",
        "moveToBeginningOfDocument" => "moveToBeginningOfDocumentAndModifySelection",
        "moveToEndOfDocument" => "moveToEndOfDocumentAndModifySelection",
        "moveWordLeft" => "moveWordLeftAndModifySelection",
        "moveWordRight" => "moveWordRightAndModifySelection",
        other => other,
    }
}

/// The schemes an address typed with one is taken as it is.
const SCHEMES: &[&str] = &[
    "http:",
    "https:",
    "about:",
    "data:",
    "file:",
    "chrome:",
    "view-source:",
    "blob:",
];

/// The address to go to for what was typed into the address bar. One with a scheme is taken
/// as it is; one without gets `http://` if it is on this machine, where dev servers answer,
/// and `https://` if it is anywhere else.
pub(crate) fn address_of(typed: &str) -> String {
    let typed = typed.trim();
    let lower = typed.to_lowercase();
    if typed.contains("://") || SCHEMES.iter().any(|scheme| lower.starts_with(scheme)) {
        return typed.to_string();
    }
    let host = lower.split(['/', '?', '#']).next().unwrap_or_default();
    let name = if host.starts_with('[') {
        host.split(']').next().map(|name| format!("{name}]"))
    } else {
        host.split(':').next().map(String::from)
    }
    .unwrap_or_default();
    if is_this_machine(&name) {
        format!("http://{typed}")
    } else {
        format!("https://{typed}")
    }
}

fn is_this_machine(name: &str) -> bool {
    let numbers = name
        .split('.')
        .filter(|part| part.parse::<u8>().is_ok())
        .count();
    name == "localhost"
        || name.ends_with(".localhost")
        || name == "[::1]"
        || (numbers == 4 && name.split('.').count() == 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_down(key: &str, modifiers: Modifiers) -> KeyInput {
        KeyInput {
            kind: KeyKind::Down,
            key: key.into(),
            code: String::new(),
            key_code: 0,
            modifiers,
            repeat: false,
        }
    }

    const META: Modifiers = Modifiers {
        alt: false,
        ctrl: false,
        meta: true,
        shift: false,
    };

    #[test]
    fn a_click_and_a_scroll_are_said_as_chrome_takes_them() {
        let click = MouseInput {
            kind: MouseKind::Down,
            x: 10.5,
            y: 20.0,
            button: "left".into(),
            buttons: 1,
            click_count: 2,
            delta_x: 0.0,
            delta_y: 0.0,
            modifiers: Modifiers {
                shift: true,
                ..Modifiers::default()
            },
        };
        let said = mouse(&click);
        assert_eq!(said["type"], "mousePressed");
        assert_eq!(
            (said["x"].as_f64(), said["clickCount"].as_u64()),
            (Some(10.5), Some(2))
        );
        assert_eq!(
            (said["button"].as_str(), said["modifiers"].as_u64()),
            (Some("left"), Some(8))
        );
        let wheel = MouseInput {
            kind: MouseKind::Wheel,
            delta_y: 120.0,
            ..click
        };
        let said = mouse(&wheel);
        assert_eq!(
            (said["type"].as_str(), said["button"].as_str()),
            (Some("mouseWheel"), Some("none"))
        );
        assert_eq!(said["deltaY"].as_f64(), Some(120.0));
    }

    #[test]
    fn a_key_that_types_goes_down_with_its_text_and_one_that_does_not_goes_down_raw() {
        let typed = key(&key_down("é", Modifiers::default()));
        assert_eq!(
            (typed["type"].as_str(), typed["text"].as_str()),
            (Some("keyDown"), Some("é"))
        );
        let enter = key(&key_down("Enter", Modifiers::default()));
        assert_eq!(enter["text"], "\r");
        let arrow = key(&key_down("ArrowLeft", Modifiers::default()));
        assert_eq!(
            (arrow["type"].as_str(), arrow.get("text")),
            (Some("rawKeyDown"), None)
        );
        let up = key(&KeyInput {
            kind: KeyKind::Up,
            ..key_down("a", Modifiers::default())
        });
        assert_eq!(up["type"], "keyUp");
    }

    #[test]
    fn a_macs_editing_shortcuts_are_sent_as_their_commands_and_type_nothing() {
        let all = key(&key_down("a", META));
        assert_eq!(
            (all["type"].as_str(), all.get("text")),
            (Some("rawKeyDown"), None)
        );
        assert_eq!(all["commands"], json!(["selectAll"]));
        let redo = key(&key_down(
            "Z",
            Modifiers {
                shift: true,
                ..META
            },
        ));
        assert_eq!(redo["commands"], json!(["redo"]));
        let select = key(&key_down(
            "ArrowRight",
            Modifiers {
                shift: true,
                ..META
            },
        ));
        assert_eq!(
            select["commands"],
            json!(["moveToEndOfLineAndModifySelection"])
        );
        // Pasting is the window's: Chrome's own clipboard is not the user's.
        assert_eq!(key(&key_down("v", META)).get("commands"), None);
    }

    #[test]
    fn an_address_without_a_scheme_gets_http_on_this_machine_and_https_elsewhere() {
        assert_eq!(address_of(" localhost:3000/a "), "http://localhost:3000/a");
        assert_eq!(address_of("127.0.0.1:8080"), "http://127.0.0.1:8080");
        assert_eq!(address_of("[::1]:5173"), "http://[::1]:5173");
        assert_eq!(address_of("app.localhost"), "http://app.localhost");
        assert_eq!(address_of("example.com/docs"), "https://example.com/docs");
        assert_eq!(address_of("10.0.0.300"), "https://10.0.0.300");
        assert_eq!(address_of("http://example.com"), "http://example.com");
        assert_eq!(address_of("about:blank"), "about:blank");
        assert_eq!(address_of("view-source:http://x"), "view-source:http://x");
    }
}
