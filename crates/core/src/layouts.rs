//! The custom layout: sizes, spacing, fonts and colours of the user's own, kept as one JSON
//! file in the data directory.
//!
//! It describes how the app's parts are drawn, not which parts there are: the front end keeps
//! its structure and reads these values where it would read its own. Everything is checked
//! here before it is kept, so that neither a hand-edited file nor an agent can leave the app
//! unreadable: numbers have ranges, colours are `#rrggbb`, and text must stand out from what
//! it is written on. A file that fails the check is refused as a whole and nothing changes.
//!
//! A key the layout does not know is refused when it is saved, and left out and named when
//! the file is read: two Swarmeds of different versions can share one data directory, and a
//! key the newer one wrote must not cost the older one the rest of the layout.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use serde::de::{IgnoredAny, IntoDeserializer, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};

use crate::Core;
use crate::error::{Error, Result};
use crate::lenient::{Problems, read_if_there, unknown_said};

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
#[serde(default)]
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

/// The colour of a sidebar level's text and of its rows behind it, in one form of the app, each
/// as `#rrggbb`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LevelColours {
    /// The level's text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub colour: Option<String>,
    /// The level's rows, behind the text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
}

/// How one level of the sidebar is drawn. What it leaves out is drawn as in the layout the
/// custom one starts from.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SidebarLevel {
    /// The size of its text, in pixels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u16>,
    /// The weight of its text, from 400 for regular to 700 for bold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<u16>,
    /// Its colours in the dark form of the app.
    #[serde(skip_serializing_if = "is_default")]
    pub dark: LevelColours,
    /// Its colours in the light form of the app.
    #[serde(skip_serializing_if = "is_default")]
    pub light: LevelColours,
}

/// The four levels of the sidebar, each set apart from the others and from the rest of the app.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SidebarLevels {
    /// A repository's name.
    #[serde(skip_serializing_if = "is_default")]
    pub repository: SidebarLevel,
    /// A task's name, on the band its sessions hang from.
    #[serde(skip_serializing_if = "is_default")]
    pub task: SidebarLevel,
    /// A session's row.
    #[serde(skip_serializing_if = "is_default")]
    pub session: SidebarLevel,
    /// The row of a sub-agent at work, under its session.
    #[serde(skip_serializing_if = "is_default")]
    pub sub_agent: SidebarLevel,
}

impl SidebarLevels {
    /// Each level by its name in the file, with how far its text has to stand out.
    fn each(&self) -> [(&'static str, &SidebarLevel, u32); 4] {
        [
            ("repository", &self.repository, TEXT_CONTRAST),
            ("task", &self.task, TEXT_CONTRAST),
            ("session", &self.session, TEXT_CONTRAST),
            // Drawn quieter than the others, as muted text is.
            ("sub_agent", &self.sub_agent, QUIET_CONTRAST),
        ]
    }
}

/// Whether `value` is its type's default, which is left out when a layout is written.
fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// A layout of the user's own.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
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
    /// The sidebar's levels, each drawn on its own.
    #[serde(skip_serializing_if = "is_default")]
    pub sidebar: SidebarLevels,
    /// Colours for the dark form of the app.
    #[serde(
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "known_colours"
    )]
    pub dark: Colours,
    /// Colours for the light form of the app.
    #[serde(
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "known_colours"
    )]
    pub light: Colours,
}

/// Reads the colours of one form, passing over a name this version does not know as a struct
/// passes over a field it does not know, so that reading the layout can name it.
fn known_colours<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Colours, D::Error> {
    deserializer.deserialize_map(KnownColours)
}

struct KnownColours;

impl<'de> Visitor<'de> for KnownColours {
    type Value = Colours;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("colours by their names, each #rrggbb")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<Colours, A::Error> {
        let mut colours = Colours::new();
        while let Some(name) = map.next_key::<String>()? {
            take_colour(&mut map, &name, &mut colours)?;
        }
        Ok(colours)
    }
}

/// Takes the value under `name` into `colours` if a colour has that name, and passes over it
/// otherwise.
fn take_colour<'de, A: MapAccess<'de>>(
    map: &mut A,
    name: &str,
    colours: &mut Colours,
) -> std::result::Result<(), A::Error> {
    let known: std::result::Result<Colour, serde::de::value::Error> =
        Colour::deserialize(name.into_deserializer());
    match known {
        Ok(colour) => {
            colours.insert(colour, map.next_value()?);
        }
        Err(_) => {
            map.next_value::<IgnoredAny>()?;
        }
    }
    Ok(())
}

/// The least and the most a number of the layout may be.
type Range = (u16, u16);

const TEXT_PX: Range = (9, 40);
const LINE_HEIGHT: Range = (110, 220);
const SPACING: Range = (50, 220);
const ICONS: Range = (80, 220);
const ROW_HEIGHT: Range = (20, 88);
const CONTROL_HEIGHT: Range = (20, 72);
const WEIGHT: Range = (400, 700);
/// The longest name of a font, in characters.
pub(crate) const MAX_FONT_CHARS: usize = 60;
/// How far text has to stand out from its background, as WCAG measures it, times ten.
pub(crate) const TEXT_CONTRAST: u32 = 45;
/// The same for quieter text, which may stand out less.
const QUIET_CONTRAST: u32 = 30;
/// The largest file read as a layout, in bytes.
const MAX_LAYOUT_BYTES: usize = 20_000;

fn refused(why: impl Into<String>) -> Error {
    Error::Layout(why.into())
}

impl CustomLayout {
    /// Reads a layout from JSON text and checks it, as it is to be kept. A key it does not know
    /// is refused as well: one misspelt would otherwise be dropped without a word.
    pub fn parse(text: &str) -> Result<Self> {
        let (layout, unknown) = Self::read(text)?;
        match unknown.first() {
            Some(key) => Err(refused(format!("`{key}` is not something a layout has"))),
            None => Ok(layout),
        }
    }

    /// Reads a layout from JSON text and checks it, leaving out each key it does not know, at
    /// any depth. Those keys are returned too, by where they stand.
    pub(crate) fn read(text: &str) -> Result<(Self, Vec<String>)> {
        if text.len() > MAX_LAYOUT_BYTES {
            return Err(refused(format!(
                "a layout is at most {MAX_LAYOUT_BYTES} bytes"
            )));
        }
        let mut unknown = Vec::new();
        let mut json = serde_json::Deserializer::from_str(text);
        let layout: Self =
            serde_ignored::deserialize(&mut json, |key| unknown.push(key.to_string()))
                .map_err(|e| refused(e.to_string()))?;
        json.end().map_err(|e| refused(e.to_string()))?;
        Ok((layout.checked()?, unknown))
    }

    /// Every number of the layout that is set, with its name and range.
    fn numbers(&self) -> Vec<(String, Option<u16>, Range)> {
        let text = &self.text;
        let fixed = [
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
        ];
        let fixed = fixed
            .into_iter()
            .map(|(name, value, range)| (name.to_string(), value, range));
        let levels = self
            .sidebar
            .each()
            .into_iter()
            .flat_map(|(name, level, _)| {
                [
                    (format!("sidebar.{name}.size"), level.size, TEXT_PX),
                    (format!("sidebar.{name}.weight"), level.weight, WEIGHT),
                ]
            });
        fixed.chain(levels).collect()
    }

    /// The layout if all of it can be drawn and read; otherwise what is wrong with it.
    fn checked(self) -> Result<Self> {
        let out_of_range = |(_, value, (least, most)): &(String, Option<u16>, Range)| {
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
        check_levels(&self.sidebar, &self.dark, &self.light)?;
        Ok(self)
    }
}

pub(crate) fn is_font_name(name: &str) -> bool {
    let plain = |c: char| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.');
    !name.trim().is_empty() && name.chars().count() <= MAX_FONT_CHARS && name.chars().all(plain)
}

/// The red, green and blue of `#rrggbb`.
pub(crate) fn rgb(colour: &str) -> Option<[u8; 3]> {
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
pub(crate) fn stands_out(one: [u8; 3], other: [u8; 3], tenths: u32) -> bool {
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
    Err(refused(format!(
        "`{form}.{ink}` cannot be read on `{form}.{surface}`: their contrast has to be at least \
         {} to 1",
        ratio(tenths)
    )))
}

/// A contrast given in tenths, as it is said: 45 is 4.5.
fn ratio(tenths: u32) -> String {
    format!("{}.{}", tenths / 10, tenths % 10)
}

/// Checks the colours of the sidebar's levels in both forms of the app.
fn check_levels(levels: &SidebarLevels, dark: &Colours, light: &Colours) -> Result<()> {
    for (name, level, tenths) in levels.each() {
        let forms = [("dark", &level.dark, dark), ("light", &level.light, light)];
        for (form, colours, palette) in forms {
            let at = format!("sidebar.{name}.{form}");
            check_level(&at, colours, form, palette.get(&Colour::Panel), tenths)?;
        }
    }
    Ok(())
}

/// That each colour of a level is a colour, and that its text stands out from its own
/// background, or from the form's `panel`, which the sidebar is, where it gives none.
///
/// Against a palette's colours, which only the front end knows, nothing could be checked: a
/// level that gives a colour has to give what it is drawn on too, and one that gives a
/// background has to give the colour drawn on it.
fn check_level(
    at: &str,
    colours: &LevelColours,
    form: &str,
    panel: Option<&String>,
    tenths: u32,
) -> Result<()> {
    let read = |part: &str, value: Option<&str>| {
        value
            .map(|value| {
                rgb(value).ok_or_else(|| refused(format!("`{at}.{part}` is a colour like #1b1e26")))
            })
            .transpose()
    };
    let ink = read("colour", colours.colour.as_deref())?;
    let background = read("background", colours.background.as_deref())?;
    let Some(ink) = ink else {
        return match background {
            None => Ok(()),
            Some(_) => Err(refused(format!(
                "`{at}` gives a background, so it has to give its `colour` as well"
            ))),
        };
    };
    let under = match background {
        Some(background) => Some((background, format!("`{at}.background`"))),
        None => panel
            .and_then(|panel| rgb(panel))
            .map(|panel| (panel, format!("`{form}.panel`"))),
    };
    let Some((under, named)) = under else {
        return Err(refused(format!(
            "`{at}` gives a colour, so it has to give its `background` as well, or `{form}` its \
             `panel`"
        )));
    };
    if stands_out(ink, under, tenths) {
        return Ok(());
    }
    Err(refused(format!(
        "`{at}.colour` cannot be read on {named}: their contrast has to be at least {} to 1",
        ratio(tenths)
    )))
}

/// The JSON Schema of a custom layout, for editors and for agents.
#[must_use]
pub fn layout_schema() -> Value {
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
            "sidebar": sidebar_schema(),
            "dark": colours("dark"),
            "light": colours("light"),
        },
    })
}

/// The part of the schema that sets the sidebar's levels apart.
fn sidebar_schema() -> Value {
    let forms = |form: &str| {
        json!({
            "type": "object",
            "additionalProperties": false,
            "description": format!("The level's colours in the {form} form, each #rrggbb."),
            "properties": {
                "colour": colour("Its text."),
                "background": colour("Its rows, behind the text."),
            },
        })
    };
    let level = |what: &str| {
        json!({
            "type": "object",
            "additionalProperties": false,
            "description": what,
            "properties": {
                "size": number(TEXT_PX, "The size of its text, in pixels."),
                "weight": number(WEIGHT, "The weight of its text: 400 regular, 600 semibold, 700 bold."),
                "dark": forms("dark"),
                "light": forms("light"),
            },
        })
    };
    json!({
        "type": "object",
        "additionalProperties": false,
        "description": "The sidebar's four levels, each set apart from the others and from the \
            rest of the app. What a level leaves out is drawn as in the base layout. A colour \
            has to stand out from the level's background, or from the form's panel where it \
            gives none, and one of the two has to be given: 4.5 to 1, 3 to 1 for sub-agents. \
            A background needs a colour beside it.",
        "properties": {
            "repository": level("A repository's name. Classic 14px, senior 18px; weight 700; bright."),
            "task": level("A task's name, in the mono font on its band. Classic 11px, senior 14px; weight 600; text."),
            "session": level("A session's row. Classic 13px, senior 16px; weight 400; text, muted while idle. A colour given is used whatever the session is doing; one waiting on you or with an answer you have not read is still drawn heavier."),
            "sub_agent": level("A sub-agent at work, under its session. Classic 13px, senior 16px; weight 400; muted."),
        },
    })
}

fn number((least, most): Range, what: &str) -> Value {
    json!({ "type": "integer", "minimum": least, "maximum": most, "description": what })
}

fn colour(what: &str) -> Value {
    json!({ "type": "string", "pattern": "^#[0-9a-fA-F]{6}$", "description": what })
}

/// The layout kept in the file at `path`; the base layout and nothing else if there is none.
/// What it does not know is left out here and said by `problems_in`.
fn load(path: &Path) -> Result<CustomLayout> {
    let Some(text) = read_if_there(path)? else {
        return Ok(CustomLayout::default());
    };
    CustomLayout::read(&text).map(|(layout, _)| layout)
}

/// What of the layout in the file at `path` is not understood, said for the user. A layout
/// refused as a whole is said where it is read, by `Core::custom_layout`, and not again here.
pub(crate) fn problems_in(path: &Path) -> Problems {
    let Ok(Some(text)) = read_if_there(path) else {
        return Problems::new();
    };
    let unknown = CustomLayout::read(&text).map_or_else(|_| Vec::new(), |(_, unknown)| unknown);
    unknown.iter().map(|key| unknown_said(key)).collect()
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
        assert_eq!(
            why(r#"{ "sidebar_side": "right" }"#),
            "`sidebar_side` is not something a layout has"
        );
        assert_eq!(
            why(r##"{ "dark": { "glow": "#ffffff" } }"##),
            "`dark.glow` is not something a layout has"
        );
        assert!(why(r#"{ "icons": "big" }"#).contains("invalid type"));
        assert!(why(r#"{ "icons": 120 } trailing"#).contains("trailing characters"));
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
    fn reading_leaves_out_and_names_what_it_does_not_know_and_keeps_the_rest() {
        let text = r##"{ "icons": 120, "later": 1, "text": { "sm": 15, "xxl": 30 },
            "sidebar": { "task": { "size": 12, "italic": true }, "pinned": {} },
            "dark": { "glow": "#ffffff", "pollen": "#ffcc00" } }"##;
        let (layout, unknown) = CustomLayout::read(text).expect("reads");
        assert_eq!(
            unknown,
            [
                "later",
                "text.xxl",
                "sidebar.task.italic",
                "sidebar.pinned",
                "dark.glow"
            ]
        );
        assert_eq!(
            (layout.icons, layout.text.sm, layout.sidebar.task.size),
            (Some(120), Some(15), Some(12))
        );
        assert_eq!(
            layout.dark,
            Colours::from([(Colour::Pollen, "#ffcc00".to_string())])
        );
        // Kept, it is refused instead: a misspelt key would be dropped without a word.
        assert_eq!(why(text), "`later` is not something a layout has");
        // What is wrong with a value it knows still refuses the whole file.
        let wrong = r#"{ "later": 1, "icons": 900 }"#;
        assert!(CustomLayout::read(wrong).is_err());
    }

    #[test]
    fn a_layout_file_with_a_key_it_does_not_know_is_drawn_and_the_key_named() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("layout.json");
        assert_eq!(problems_in(&path), Problems::new());
        std::fs::write(&path, r#"{ "icons": 120, "later": { "a": 1 } }"#).expect("writes");
        assert_eq!(load(&path).expect("reads").icons, Some(120));
        let problems = problems_in(&path);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].starts_with("`later` is not something this version of Swarmed knows"),
            "{problems:?}"
        );
        // A layout refused as a whole is said where it is read.
        std::fs::write(&path, r#"{ "icons": 900, "later": 1 }"#).expect("writes");
        assert_eq!(problems_in(&path), Problems::new());
        assert!(load(&path).is_err());
    }

    #[test]
    fn each_level_of_the_sidebar_is_read_and_written_without_what_it_leaves_out() {
        let text = r##"{ "sidebar": {
            "repository": { "size": 16 },
            "session": { "size": 15, "weight": 600,
                "dark": { "colour": "#f0f0f0", "background": "#202020" } },
            "sub_agent": {} } }"##;
        let layout = CustomLayout::parse(text).expect("reads");
        assert_eq!(layout.sidebar.session.weight, Some(600));
        let written = serde_json::to_value(&layout).expect("writes");
        let expected = json!({ "base": "classic", "text": {}, "sidebar": {
            "repository": { "size": 16 },
            "session": { "size": 15, "weight": 600,
                "dark": { "colour": "#f0f0f0", "background": "#202020" } } } });
        assert_eq!(written, expected);
        let empty = r#"{ "sidebar": { "task": {} } }"#;
        let written = serde_json::to_value(CustomLayout::parse(empty).expect("reads"));
        assert_eq!(
            written.expect("writes"),
            json!({ "base": "classic", "text": {} })
        );
    }

    #[test]
    fn a_level_of_the_sidebar_is_refused_with_the_reason() {
        let level = |inner: &str| why(&format!("{{ \"sidebar\": {{ {inner} }} }}"));
        assert_eq!(
            level(r#""task": { "size": 8 }"#),
            "`sidebar.task.size` is from 9 to 40"
        );
        assert_eq!(
            level(r#""session": { "weight": 900 }"#),
            "`sidebar.session.weight` is from 400 to 700"
        );
        assert_eq!(
            level(r#""task": { "dark": { "colour": "white" } }"#),
            "`sidebar.task.dark.colour` is a colour like #1b1e26"
        );
        assert_eq!(
            level(r##""session": { "light": { "colour": "#909090", "background": "#808080" } }"##),
            "`sidebar.session.light.colour` cannot be read on `sidebar.session.light.background`: \
             their contrast has to be at least 4.5 to 1"
        );
        assert_eq!(
            level(r##""repository": { "dark": { "colour": "#ffcc00" } }"##),
            "`sidebar.repository.dark` gives a colour, so it has to give its `background` as \
             well, or `dark` its `panel`"
        );
        assert_eq!(
            level(r##""task": { "dark": { "background": "#000000" } }"##),
            "`sidebar.task.dark` gives a background, so it has to give its `colour` as well"
        );
        assert_eq!(
            level(r#""session": { "italic": true }"#),
            "`sidebar.session.italic` is not something a layout has"
        );
    }

    #[test]
    fn a_level_without_a_background_is_read_on_the_panel() {
        let on_panel = |colour: &str| {
            CustomLayout::parse(&format!(
                "{{ \"dark\": {{ \"dusk\": \"#101010\", \"text\": \"#f0f0f0\", \
                 \"panel\": \"#202020\" }}, \"sidebar\": {{ \"task\": {{ \"dark\": \
                 {{ \"colour\": \"{colour}\" }} }} }} }}"
            ))
        };
        assert!(on_panel("#e0e0e0").is_ok());
        let refused = on_panel("#303030").expect_err("too close").to_string();
        assert_eq!(
            refused,
            "`sidebar.task.dark.colour` cannot be read on `dark.panel`: their contrast has to be \
             at least 4.5 to 1"
        );
        // Sub-agents are drawn quieter, and may stand out less.
        let quiet = r##"{ "sidebar": { "sub_agent": { "light": { "colour": "#8a8a8a", "background": "#ffffff" } } } }"##;
        assert!(CustomLayout::parse(quiet).is_ok());
        let loud = quiet.replace("sub_agent", "session");
        assert!(why(&loud).contains("at least 4.5 to 1"));
    }

    #[test]
    fn the_schema_names_every_field_the_layout_has() {
        let schema = layout_schema();
        let full = r##"{ "base": "classic", "text": { "xs": 12, "sm": 14, "md": 16, "lg": 19, "xl": 24 },
            "line_height": 150, "spacing": 110, "icons": 120, "row_height": 36,
            "control_height": 34, "font": "Inter", "mono_font": "Menlo",
            "sidebar": { "session": { "size": 14, "weight": 500,
                "dark": { "colour": "#f0f0f0", "background": "#202020" } } },
            "dark": { "dusk": "#101010", "text": "#f0f0f0" },
            "light": { "dusk": "#ffffff", "text": "#101010" } }"##;
        let layout = serde_json::to_value(CustomLayout::parse(full).expect("reads")).expect("json");
        for field in layout.as_object().expect("an object").keys() {
            assert!(schema["properties"].get(field).is_some(), "{field}");
        }
        let sidebar = &schema["properties"]["sidebar"]["properties"];
        let mut levels: Vec<_> = sidebar.as_object().expect("levels").keys().collect();
        levels.sort();
        assert_eq!(levels, ["repository", "session", "sub_agent", "task"]);
        let session = &layout["sidebar"]["session"];
        for field in session.as_object().expect("a level").keys() {
            assert!(
                sidebar["session"]["properties"].get(field).is_some(),
                "{field}"
            );
        }
        for field in session["dark"].as_object().expect("colours").keys() {
            let forms = &sidebar["session"]["properties"]["dark"]["properties"];
            assert!(forms.get(field).is_some(), "{field}");
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
