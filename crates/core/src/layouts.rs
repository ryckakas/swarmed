//! The custom layout: sizes, spacing, fonts and colours of the user's own, kept as one JSON
//! file in the data directory.
//!
//! It describes how the app's parts are drawn, not which parts there are: the front end keeps
//! its structure and reads these values where it would read its own. Everything is checked
//! here before it is kept, so that neither a hand-edited file nor an agent can leave the app
//! unreadable: numbers have ranges, colours are `#rrggbb`, and text must stand out from what
//! it is written on. A file that fails the check is refused as a whole and nothing changes.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::Core;
use crate::error::{Error, Result};

/// The layout a custom one starts from: whatever it does not set is drawn as there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutBase {
    /// The dense layout.
    #[default]
    Classic,
    /// The layout for easier reading.
    Senior,
}

/// The five sizes all of the app's text is set in, in pixels, from the smallest labels to the
/// largest heading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TextSizes {
    /// Labels, counts, hints.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xs: Option<u16>,
    /// The text of the app and of conversations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sm: Option<u16>,
    /// What stands out a little.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub md: Option<u16>,
    /// Titles.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lg: Option<u16>,
    /// The largest heading.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xl: Option<u16>,
}

/// One of the colours the app is drawn in, by the name its stylesheet knows it under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Colour {
    /// The window's background.
    Dusk,
    /// The sidebar and the task panel.
    Panel,
    /// Buttons, fields and what is selected.
    Raised,
    /// Rules and borders.
    Line,
    /// Text.
    Text,
    /// Quieter text.
    Muted,
    /// The quietest text.
    Faint,
    /// What text stresses.
    Bright,
    /// The ring around what the keyboard is on.
    Focus,
    /// Links.
    Sky,
    /// A session that needs the user.
    Pollen,
    /// What went well.
    Moss,
    /// Work going on.
    Busy,
    /// What failed.
    Clay,
    /// Names and headings in what agents write.
    Lilac,
    /// Code in what agents write.
    Teal,
    /// Numbers and strings in code.
    Sand,
}

/// The colours a custom layout sets, each as `#rrggbb`. What is not named stays the palette's.
pub type Colours = BTreeMap<Colour, String>;

/// A layout of the user's own.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CustomLayout {
    /// The layout this one starts from.
    pub base: LayoutBase,
    /// The sizes of text, in pixels.
    pub text: TextSizes,
    /// The height of a line of text, in percent of the text's size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_height: Option<u16>,
    /// The room between and inside things, in percent of the classic layout's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spacing: Option<u16>,
    /// The size of icons, status marks and small buttons, in percent of the classic layout's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icons: Option<u16>,
    /// The least height of a session's row in the sidebar, in pixels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row_height: Option<u16>,
    /// The least height of buttons and menus, in pixels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub control_height: Option<u16>,
    /// A font installed on this machine for the app's text, by its family name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// A font installed on this machine for code and the terminal, by its family name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mono_font: Option<String>,
    /// Colours for the dark form of the app.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub dark: Colours,
    /// Colours for the light form of the app.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub light: Colours,
}

/// The least and the most a number of the layout may be.
type Range = (u16, u16);

const TEXT_PX: Range = (9, 40);
const LINE_HEIGHT: Range = (110, 220);
const SPACING: Range = (50, 220);
const ICONS: Range = (80, 220);
const ROW_HEIGHT: Range = (20, 88);
const CONTROL_HEIGHT: Range = (20, 72);
/// The longest name of a font, in characters.
const MAX_FONT_CHARS: usize = 60;
/// How far text has to stand out from its background, as WCAG measures it, times ten.
const TEXT_CONTRAST: u32 = 45;
/// The same for quieter text, which may stand out less.
const QUIET_CONTRAST: u32 = 30;
/// The largest file read as a layout, in bytes.
const MAX_LAYOUT_BYTES: usize = 20_000;

fn refused(why: impl Into<String>) -> Error {
    Error::Layout(why.into())
}

impl CustomLayout {
    /// Reads a layout from JSON text and checks it.
    pub fn parse(text: &str) -> Result<Self> {
        if text.len() > MAX_LAYOUT_BYTES {
            return Err(refused(format!(
                "a layout is at most {MAX_LAYOUT_BYTES} bytes"
            )));
        }
        let layout: Self = serde_json::from_str(text).map_err(|e| refused(e.to_string()))?;
        layout.checked()
    }

    /// Every number of the layout that is set, with its name and range.
    fn numbers(&self) -> Vec<(&'static str, Option<u16>, Range)> {
        let text = &self.text;
        vec![
            ("text.xs", text.xs, TEXT_PX),
            ("text.sm", text.sm, TEXT_PX),
            ("text.md", text.md, TEXT_PX),
            ("text.lg", text.lg, TEXT_PX),
            ("text.xl", text.xl, TEXT_PX),
            ("line_height", self.line_height, LINE_HEIGHT),
            ("spacing", self.spacing, SPACING),
            ("icons", self.icons, ICONS),
            ("row_height", self.row_height, ROW_HEIGHT),
            ("control_height", self.control_height, CONTROL_HEIGHT),
        ]
    }

    /// The layout if all of it can be drawn and read; otherwise what is wrong with it.
    fn checked(self) -> Result<Self> {
        let out_of_range = |(_, value, (least, most)): &(&str, Option<u16>, Range)| {
            value.is_some_and(|value| value < *least || value > *most)
        };
        if let Some((name, _, (least, most))) = self.numbers().iter().find(|n| out_of_range(n)) {
            return Err(refused(format!("`{name}` is from {least} to {most}")));
        }
        let fonts = [("font", &self.font), ("mono_font", &self.mono_font)];
        let unusable = |font: &Option<String>| font.as_deref().is_some_and(|f| !is_font_name(f));
        if let Some((name, _)) = fonts.iter().find(|(_, font)| unusable(font)) {
            return Err(refused(format!(
                "`{name}` is a font's family name: letters, digits, spaces and hyphens, at most \
                 {MAX_FONT_CHARS} characters"
            )));
        }
        check_colours("dark", &self.dark)?;
        check_colours("light", &self.light)?;
        Ok(self)
    }
}

fn is_font_name(name: &str) -> bool {
    let plain = |c: char| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.');
    !name.trim().is_empty() && name.chars().count() <= MAX_FONT_CHARS && name.chars().all(plain)
}

/// The red, green and blue of `#rrggbb`.
fn rgb(colour: &str) -> Option<[u8; 3]> {
    let digits = colour
        .strip_prefix('#')
        .filter(|digits| digits.len() == 6)?;
    let part = |at: usize| u8::from_str_radix(digits.get(at..at + 2)?, 16).ok();
    Some([part(0)?, part(2)?, part(4)?])
}

/// How bright a colour is to the eye, from 0 to 1, as WCAG defines it.
fn luminance(colour: [u8; 3]) -> f64 {
    let linear = |part: u8| {
        let part = f64::from(part) / 255.0;
        if part <= 0.039_28 {
            part / 12.92
        } else {
            ((part + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(colour[0]) + 0.7152 * linear(colour[1]) + 0.0722 * linear(colour[2])
}

/// Whether two colours are at least `tenths` tenths apart in contrast: 45 is WCAG's 4.5 to 1.
fn stands_out(one: [u8; 3], other: [u8; 3], tenths: u32) -> bool {
    let (one, other) = (luminance(one), luminance(other));
    let ratio = (one.max(other) + 0.05) / (one.min(other) + 0.05);
    ratio * 10.0 >= f64::from(tenths)
}

/// The colours that text is written on, and the colours of text with how far each must stand
/// out from those.
const SURFACES: [Colour; 3] = [Colour::Dusk, Colour::Panel, Colour::Raised];
const INKS: [(Colour, u32); 4] = [
    (Colour::Text, TEXT_CONTRAST),
    (Colour::Bright, TEXT_CONTRAST),
    (Colour::Muted, QUIET_CONTRAST),
    (Colour::Faint, QUIET_CONTRAST),
];

fn name_of(colour: Colour) -> String {
    let named = serde_json::to_value(colour).unwrap_or_default();
    named.as_str().unwrap_or_default().to_string()
}

/// Checks the colours of one form of the app: that each is a colour, and that text can be read.
///
/// A layout that changes a background or the text has to give both `dusk` and `text`: against
/// a palette's colours, which only the front end knows, nothing could be checked.
fn check_colours(form: &str, colours: &Colours) -> Result<()> {
    let mut parts = BTreeMap::new();
    for (colour, value) in colours {
        let Some(read) = rgb(value) else {
            let name = name_of(*colour);
            return Err(refused(format!("`{form}.{name}` is a colour like #1b1e26")));
        };
        parts.insert(*colour, read);
    }
    let touched = |colour: &Colour| parts.contains_key(colour);
    let writes = SURFACES.iter().any(touched) || INKS.iter().any(|(ink, _)| touched(ink));
    if !writes {
        return Ok(());
    }
    if !touched(&Colour::Dusk) || !touched(&Colour::Text) {
        return Err(refused(format!(
            "`{form}` changes a background or the text, so it has to give both `dusk` and `text`"
        )));
    }
    for (ink, tenths) in INKS {
        check_ink(form, &parts, ink, tenths)?;
    }
    Ok(())
}

/// That `ink`, if the layout sets it, stands out from every background the layout sets.
fn check_ink(
    form: &str,
    parts: &BTreeMap<Colour, [u8; 3]>,
    ink: Colour,
    tenths: u32,
) -> Result<()> {
    let Some(written) = parts.get(&ink) else {
        return Ok(());
    };
    let unreadable = SURFACES.iter().find(|surface| {
        parts
            .get(surface)
            .is_some_and(|surface| !stands_out(*written, *surface, tenths))
    });
    let Some(surface) = unreadable else {
        return Ok(());
    };
    let (ink, surface) = (name_of(ink), name_of(*surface));
    let (whole, tenth) = (tenths / 10, tenths % 10);
    Err(refused(format!(
        "`{form}.{ink}` cannot be read on `{form}.{surface}`: their contrast has to be at least \
         {whole}.{tenth} to 1"
    )))
}

/// The JSON Schema of a custom layout, for editors and for agents.
#[must_use]
pub fn layout_schema() -> Value {
    let number = |(least, most): Range, what: &str| json!({ "type": "integer", "minimum": least, "maximum": most, "description": what });
    let font =
        |what: &str| json!({ "type": "string", "maxLength": MAX_FONT_CHARS, "description": what });
    let size = |what: &str| number(TEXT_PX, what);
    let colours = |form: &str| {
        json!({
            "type": "object",
            "additionalProperties": false,
            "description": format!(
                "Colours of the {form} form, each #rrggbb. What is left out stays the palette's. \
                 Changing a background or the text needs both dusk and text, and text has to \
                 stand out from the backgrounds given: 4.5 to 1 for text and bright, 3 to 1 for \
                 muted and faint."
            ),
            "properties": {
                "dusk": colour("The window's background."),
                "panel": colour("The sidebar and the task panel."),
                "raised": colour("Buttons, fields and what is selected."),
                "line": colour("Rules and borders."),
                "text": colour("Text."),
                "muted": colour("Quieter text."),
                "faint": colour("The quietest text."),
                "bright": colour("What text stresses."),
                "focus": colour("The ring around what the keyboard is on."),
                "sky": colour("Links."),
                "pollen": colour("A session that needs you."),
                "moss": colour("What went well."),
                "busy": colour("Work going on."),
                "clay": colour("What failed."),
                "lilac": colour("Names and headings in what agents write."),
                "teal": colour("Code in what agents write."),
                "sand": colour("Numbers and strings in code."),
            },
        })
    };
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "Swarmed custom layout",
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "base": {
                "enum": ["classic", "senior"],
                "description": "The layout this one starts from: what it leaves out is drawn as there.",
            },
            "text": {
                "type": "object",
                "additionalProperties": false,
                "description": "The five sizes all text is set in, in pixels.",
                "properties": {
                    "xs": size("Labels, counts, hints. Classic 11, senior 14."),
                    "sm": size("The text of the app and of conversations. Classic 13, senior 16."),
                    "md": size("What stands out a little. Classic 15, senior 18."),
                    "lg": size("Titles. Classic 18, senior 21."),
                    "xl": size("The largest heading. Classic 22, senior 26."),
                },
            },
            "line_height": number(LINE_HEIGHT, "The height of a line, in percent of the text's size. Classic 145."),
            "spacing": number(SPACING, "The room between and inside things, in percent of classic's."),
            "icons": number(ICONS, "The size of icons, status marks and small buttons, in percent of classic's."),
            "row_height": number(ROW_HEIGHT, "The least height of a session's row in the sidebar, in pixels."),
            "control_height": number(CONTROL_HEIGHT, "The least height of buttons and menus, in pixels."),
            "font": font("A font installed on this machine for the app's text, by family name."),
            "mono_font": font("A font installed on this machine for code and the terminal."),
            "dark": colours("dark"),
            "light": colours("light"),
        },
    })
}

fn colour(what: &str) -> Value {
    json!({ "type": "string", "pattern": "^#[0-9a-fA-F]{6}$", "description": what })
}

/// The layout kept in the file at `path`; the base layout and nothing else if there is none.
fn load(path: &Path) -> Result<CustomLayout> {
    match std::fs::read_to_string(path) {
        Ok(text) => CustomLayout::parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(CustomLayout::default()),
        Err(e) => Err(e.into()),
    }
}

impl Core {
    /// The user's custom layout. An error if the file was edited into something that cannot
    /// be drawn; the front end then stays with what it has.
    pub fn custom_layout(&self) -> Result<CustomLayout> {
        load(&self.paths.custom_layout())
    }

    /// Checks `text` as a custom layout and keeps it. Nothing is kept if anything is wrong.
    pub fn set_custom_layout(&self, text: &str) -> Result<CustomLayout> {
        let layout = CustomLayout::parse(text)?;
        let path = self.paths.custom_layout();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let pretty = serde_json::to_string_pretty(&layout).map_err(|e| refused(e.to_string()))?;
        std::fs::write(path, pretty + "\n")?;
        Ok(layout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn why(text: &str) -> String {
        CustomLayout::parse(text).expect_err("refused").to_string()
    }

    #[test]
    fn a_layout_is_read_and_written_without_what_it_leaves_out() {
        let text = r##"{ "base": "senior", "text": { "sm": 18 }, "icons": 150,
            "font": "Helvetica Neue", "dark": { "dusk": "#101010", "text": "#f0f0f0" } }"##;
        let layout = CustomLayout::parse(text).expect("reads");
        assert_eq!(
            (layout.base, layout.text.sm, layout.icons),
            (LayoutBase::Senior, Some(18), Some(150))
        );
        let written = serde_json::to_value(&layout).expect("writes");
        let expected = json!({ "base": "senior", "text": { "sm": 18 }, "icons": 150,
            "font": "Helvetica Neue", "dark": { "dusk": "#101010", "text": "#f0f0f0" } });
        assert_eq!(written, expected);
        assert_eq!(
            CustomLayout::parse("{}").expect("reads"),
            CustomLayout::default()
        );
    }

    #[test]
    fn what_cannot_be_drawn_is_refused_with_the_reason() {
        assert_eq!(
            why(r#"{ "text": { "xs": 4 } }"#),
            "`text.xs` is from 9 to 40"
        );
        assert_eq!(why(r#"{ "icons": 900 }"#), "`icons` is from 80 to 220");
        assert!(why(r#"{ "sidebar": "right" }"#).contains("unknown field `sidebar`"));
        assert!(why(r##"{ "dark": { "glow": "#ffffff" } }"##).contains("unknown variant `glow`"));
        assert!(why(r#"{ "font": "x; } body { display: none" }"#).contains("family name"));
        assert!(why(r#"{ "dark": { "sky": "blue" } }"#).contains("`dark.sky` is a colour"));
        assert!(why(&format!("{{ \"font\": \"{}\" }}", "a".repeat(30_000))).contains("at most"));
    }

    #[test]
    fn text_has_to_stand_out_from_its_background() {
        let grey_on_grey = r##"{ "light": { "dusk": "#808080", "text": "#909090" } }"##;
        assert_eq!(
            why(grey_on_grey),
            "`light.text` cannot be read on `light.dusk`: their contrast has to be at least 4.5 to 1"
        );
        let half = r##"{ "dark": { "panel": "#000000" } }"##;
        assert!(why(half).contains("has to give both `dusk` and `text`"));
        let quiet = r##"{ "dark": { "dusk": "#101010", "text": "#f0f0f0", "muted": "#222222" } }"##;
        assert!(why(quiet).contains("`dark.muted` cannot be read on `dark.dusk`"));
        // A colour that carries no text is taken as it is.
        let accent = r##"{ "dark": { "pollen": "#ffcc00", "focus": "#00aaff" } }"##;
        assert_eq!(CustomLayout::parse(accent).expect("reads").dark.len(), 2);
    }

    #[test]
    fn the_schema_names_every_field_the_layout_has() {
        let schema = layout_schema();
        let full = r##"{ "base": "classic", "text": { "xs": 12, "sm": 14, "md": 16, "lg": 19, "xl": 24 },
            "line_height": 150, "spacing": 110, "icons": 120, "row_height": 36,
            "control_height": 34, "font": "Inter", "mono_font": "Menlo",
            "dark": { "dusk": "#101010", "text": "#f0f0f0" },
            "light": { "dusk": "#ffffff", "text": "#101010" } }"##;
        let layout = serde_json::to_value(CustomLayout::parse(full).expect("reads")).expect("json");
        for field in layout.as_object().expect("an object").keys() {
            assert!(schema["properties"].get(field).is_some(), "{field}");
        }
        let colours = schema["properties"]["dark"]["properties"]
            .as_object()
            .expect("colours");
        assert_eq!(colours.len(), 17);
        for name in colours.keys() {
            let text = format!("{{ \"dark\": {{ \"{name}\": \"#12ab9f\" }} }}");
            // Named in the schema and known to the layout: refused, if at all, for its contrast.
            assert!(
                CustomLayout::parse(&text)
                    .map_or_else(|e| !e.to_string().contains("unknown"), |_| true)
            );
        }
    }
}
