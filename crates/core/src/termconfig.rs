//! How the terminal looks and behaves: one JSON file of the user's own in the data directory.
//!
//! Like the custom layout, it is checked here before it is kept, so that neither a hand-edited
//! file nor an agent can leave a terminal nobody can read: numbers have ranges, colours are
//! `#rrggbb`, and the text has to stand out from the background. Whatever is left out is as
//! Swarmed draws a terminal by itself, in the colours of the palette.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::Core;
use crate::error::{Error, Result};
use crate::layouts::{MAX_FONT_CHARS, TEXT_CONTRAST, is_font_name, rgb, stands_out};

/// The shape of the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cursor {
    /// A filled cell.
    Block,
    /// A line before the character.
    Bar,
    /// A line under the character.
    Underline,
}

/// One of the colours a terminal is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalColour {
    /// What everything is written on.
    Background,
    /// Text that names no colour.
    Foreground,
    /// The cursor.
    Cursor,
    /// Behind what is selected.
    Selection,
    /// What programs call black.
    Black,
    /// What programs call red.
    Red,
    /// What programs call green.
    Green,
    /// What programs call yellow.
    Yellow,
    /// What programs call blue.
    Blue,
    /// What programs call magenta.
    Magenta,
    /// What programs call cyan.
    Cyan,
    /// What programs call white.
    White,
    /// The bright form of black.
    BrightBlack,
    /// The bright form of red.
    BrightRed,
    /// The bright form of green.
    BrightGreen,
    /// The bright form of yellow.
    BrightYellow,
    /// The bright form of blue.
    BrightBlue,
    /// The bright form of magenta.
    BrightMagenta,
    /// The bright form of cyan.
    BrightCyan,
    /// The bright form of white.
    BrightWhite,
}

/// The colours a terminal's configuration sets, each as `#rrggbb`.
pub type TerminalColours = BTreeMap<TerminalColour, String>;

/// The user's own configuration of the terminal.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalConfig {
    /// A font installed on this machine, by its family name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// The size of the text, in pixels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<u16>,
    /// The height of a line, in percent of the text's size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_height: Option<u16>,
    /// The shape of the cursor.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
    /// Whether the cursor blinks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor_blink: Option<bool>,
    /// How many lines of output are kept to scroll back through.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scrollback: Option<u32>,
    /// Whether bold text is also drawn in the bright form of its colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bold_is_bright: Option<bool>,
    /// Whether the Option key is Meta, as Emacs and readline want it, instead of typing
    /// the characters a keyboard has on it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub option_is_meta: Option<bool>,
    /// Whether selecting text copies it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copy_on_select: Option<bool>,
    /// Whether programs may put text on the clipboard, as tmux and vim do over ssh. They are
    /// never given what is on it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clipboard: Option<bool>,
    /// Whether the terminal is drawn by the graphics card, which is faster for much output.
    /// Off unless asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<bool>,
    /// How far any text is kept apart from what it is written on, in tenths of a contrast
    /// ratio: a colour a program asks for that would be harder to read is adjusted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_contrast: Option<u16>,
    /// Colours for the dark form of the app.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub dark: TerminalColours,
    /// Colours for the light form of the app.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub light: TerminalColours,
}

type Range = (u32, u32);

const FONT_SIZE: Range = (8, 32);
const LINE_HEIGHT: Range = (100, 200);
const SCROLLBACK: Range = (100, 100_000);
const MINIMUM_CONTRAST: Range = (10, 210);
/// The largest file read as a terminal's configuration, in bytes.
const MAX_BYTES: usize = 20_000;

fn refused(why: impl Into<String>) -> Error {
    Error::TerminalConfig(why.into())
}

impl TerminalConfig {
    /// Reads a configuration from JSON text and checks it.
    pub fn parse(text: &str) -> Result<Self> {
        if text.len() > MAX_BYTES {
            return Err(refused(format!(
                "a terminal's configuration is at most {MAX_BYTES} bytes"
            )));
        }
        let config: Self = serde_json::from_str(text).map_err(|e| refused(e.to_string()))?;
        config.checked()
    }

    fn numbers(&self) -> [(&'static str, Option<u32>, Range); 4] {
        [
            ("font_size", self.font_size.map(u32::from), FONT_SIZE),
            ("line_height", self.line_height.map(u32::from), LINE_HEIGHT),
            ("scrollback", self.scrollback, SCROLLBACK),
            (
                "minimum_contrast",
                self.minimum_contrast.map(u32::from),
                MINIMUM_CONTRAST,
            ),
        ]
    }

    fn checked(self) -> Result<Self> {
        let out_of_range = |(_, value, (least, most)): &(&str, Option<u32>, Range)| {
            value.is_some_and(|value| value < *least || value > *most)
        };
        if let Some((name, _, (least, most))) = self.numbers().iter().find(|n| out_of_range(n)) {
            return Err(refused(format!("`{name}` is from {least} to {most}")));
        }
        if self.font.as_deref().is_some_and(|font| !is_font_name(font)) {
            return Err(refused(format!(
                "`font` is a font's family name: letters, digits, spaces and hyphens, at most \
                 {MAX_FONT_CHARS} characters"
            )));
        }
        check_colours("dark", &self.dark)?;
        check_colours("light", &self.light)?;
        Ok(self)
    }
}

/// Checks the colours of one form: that each is a colour, and that text can be read. A form
/// that sets the background or the text has to set both: the other would be the palette's,
/// which only the front end knows.
fn check_colours(form: &str, colours: &TerminalColours) -> Result<()> {
    let unreadable = |(_, value): &(&TerminalColour, &String)| rgb(value).is_none();
    if let Some((colour, _)) = colours.iter().find(unreadable) {
        let name = serde_json::to_value(colour).unwrap_or_default();
        let name = name.as_str().unwrap_or_default();
        return Err(refused(format!("`{form}.{name}` is a colour like #1b1e26")));
    }
    let part = |colour: TerminalColour| colours.get(&colour).and_then(|value| rgb(value));
    match (
        part(TerminalColour::Background),
        part(TerminalColour::Foreground),
    ) {
        (None, None) => Ok(()),
        (Some(background), Some(foreground))
            if stands_out(foreground, background, TEXT_CONTRAST) =>
        {
            Ok(())
        }
        (Some(_), Some(_)) => Err(refused(format!(
            "`{form}.foreground` cannot be read on `{form}.background`: their contrast has to \
             be at least 4.5 to 1"
        ))),
        _ => Err(refused(format!(
            "`{form}` has to give both `background` and `foreground`, or neither"
        ))),
    }
}

/// The JSON Schema of a terminal's configuration, for editors and for agents.
#[must_use]
pub fn terminal_schema() -> Value {
    let number = |(least, most): Range, what: &str| json!({ "type": "integer", "minimum": least, "maximum": most, "description": what });
    let switch = |what: &str| json!({ "type": "boolean", "description": what });
    let colour = json!({ "type": "string", "pattern": "^#[0-9a-fA-F]{6}$" });
    let names = [
        "background",
        "foreground",
        "cursor",
        "selection",
        "black",
        "red",
        "green",
        "yellow",
        "blue",
        "magenta",
        "cyan",
        "white",
        "bright_black",
        "bright_red",
        "bright_green",
        "bright_yellow",
        "bright_blue",
        "bright_magenta",
        "bright_cyan",
        "bright_white",
    ];
    let properties: serde_json::Map<String, Value> = names
        .iter()
        .map(|name| ((*name).to_string(), colour.clone()))
        .collect();
    let colours = |form: &str| {
        json!({
            "type": "object",
            "additionalProperties": false,
            "description": format!(
                "Colours of the {form} form, each #rrggbb. What is left out follows the app's \
                 palette. `background` and `foreground` are given together or not at all, and \
                 at least 4.5 to 1 apart."
            ),
            "properties": properties,
        })
    };
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "Swarmed terminal",
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "font": {
                "type": "string", "maxLength": MAX_FONT_CHARS,
                "description": "A font installed on this machine, by family name. Default: the app's own.",
            },
            "font_size": number(FONT_SIZE, "The size of the text, in pixels. Default: the app's text size."),
            "line_height": number(LINE_HEIGHT, "The height of a line, in percent. Default 100."),
            "cursor": { "enum": ["block", "bar", "underline"], "description": "Default block." },
            "cursor_blink": switch("Default true."),
            "scrollback": number(SCROLLBACK, "Lines kept to scroll back through. Default 10000."),
            "bold_is_bright": switch("Draw bold text in the bright form of its colour. Default true."),
            "option_is_meta": switch("Option is Meta instead of typing a keyboard's own characters. Default false."),
            "copy_on_select": switch("Selecting text copies it. Default false."),
            "clipboard": switch("Programs may put text on the clipboard; they never read it. Default true."),
            "gpu": switch("Drawn by the graphics card: faster for much output. Default false; switch it off again if text comes out the wrong size."),
            "minimum_contrast": number(
                MINIMUM_CONTRAST,
                "In tenths of a contrast ratio: text a program colours harder to read than this is adjusted. Default 45; 10 leaves every colour as asked for.",
            ),
            "dark": colours("dark"),
            "light": colours("light"),
        },
    })
}

fn load(path: &Path) -> Result<TerminalConfig> {
    match std::fs::read_to_string(path) {
        Ok(text) => TerminalConfig::parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(TerminalConfig::default()),
        Err(e) => Err(e.into()),
    }
}

impl Core {
    /// The user's configuration of the terminal. An error if the file was edited into
    /// something unusable; the front end then stays with what it has.
    pub fn terminal_config(&self) -> Result<TerminalConfig> {
        load(&self.paths.terminal_config())
    }

    /// Checks `text` as a terminal's configuration and keeps it. Nothing is kept if anything
    /// is wrong.
    pub fn set_terminal_config(&self, text: &str) -> Result<TerminalConfig> {
        let config = TerminalConfig::parse(text)?;
        let path = self.paths.terminal_config();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let pretty = serde_json::to_string_pretty(&config).map_err(|e| refused(e.to_string()))?;
        std::fs::write(path, pretty + "\n")?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn why(text: &str) -> String {
        TerminalConfig::parse(text)
            .expect_err("refused")
            .to_string()
    }

    #[test]
    fn a_configuration_is_read_and_written_without_what_it_leaves_out() {
        let text = r##"{ "font": "JetBrains Mono", "font_size": 14, "cursor": "bar",
            "copy_on_select": true, "dark": { "blue": "#7aa2f7" } }"##;
        let config = TerminalConfig::parse(text).expect("reads");
        assert_eq!(
            (config.font_size, config.cursor),
            (Some(14), Some(Cursor::Bar))
        );
        let expected = json!({ "font": "JetBrains Mono", "font_size": 14, "cursor": "bar",
            "copy_on_select": true, "dark": { "blue": "#7aa2f7" } });
        assert_eq!(serde_json::to_value(&config).expect("writes"), expected);
        assert_eq!(
            serde_json::to_value(TerminalConfig::parse("{}").expect("reads")).expect("writes"),
            json!({})
        );
    }

    #[test]
    fn what_cannot_be_used_is_refused_with_the_reason() {
        assert_eq!(why(r#"{ "font_size": 3 }"#), "`font_size` is from 8 to 32");
        assert_eq!(
            why(r#"{ "scrollback": 9000000 }"#),
            "`scrollback` is from 100 to 100000"
        );
        assert!(why(r#"{ "shell": "/bin/evil" }"#).contains("unknown field `shell`"));
        assert!(why(r#"{ "cursor": "beam" }"#).contains("unknown variant `beam`"));
        assert!(why(r#"{ "font": "x; } body { display: none" }"#).contains("family name"));
        assert!(why(r#"{ "dark": { "red": "red" } }"#).contains("`dark.red` is a colour"));
        let one = r##"{ "light": { "background": "#ffffff" } }"##;
        assert!(why(one).contains("both `background` and `foreground`"));
        let grey = r##"{ "light": { "background": "#808080", "foreground": "#8a8a8a" } }"##;
        assert!(why(grey).contains("`light.foreground` cannot be read"));
    }

    #[test]
    fn the_schema_names_every_field_and_every_colour() {
        let schema = terminal_schema();
        let full = r##"{ "font": "Menlo", "font_size": 13, "line_height": 120, "cursor": "block",
            "cursor_blink": false, "scrollback": 5000, "bold_is_bright": false,
            "option_is_meta": true, "copy_on_select": true, "clipboard": false, "gpu": false,
            "minimum_contrast": 70,
            "dark": { "background": "#101010", "foreground": "#f0f0f0" },
            "light": { "background": "#ffffff", "foreground": "#101010" } }"##;
        let config =
            serde_json::to_value(TerminalConfig::parse(full).expect("reads")).expect("json");
        for field in config.as_object().expect("an object").keys() {
            assert!(schema["properties"].get(field).is_some(), "{field}");
        }
        let colours = schema["properties"]["dark"]["properties"]
            .as_object()
            .expect("colours");
        assert_eq!(colours.len(), 20);
        for name in colours.keys().filter(|name| !name.ends_with("ground")) {
            let text = format!("{{ \"dark\": {{ \"{name}\": \"#12ab9f\" }} }}");
            assert!(TerminalConfig::parse(&text).is_ok(), "{name}");
        }
    }
}
