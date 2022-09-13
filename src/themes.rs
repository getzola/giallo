use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use toml::Value;

// TODO: make serde not needed at runtime
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Color {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    Gray,
    LightRed,
    LightGreen,
    LightYellow,
    LightBlue,
    LightMagenta,
    LightCyan,
    LightGray,
    White,
    Hex(String),
}

impl Display for Color {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        // TODO: some colors don't exist in css, make they match whatever helix is using as rgb
        match self {
            Color::Black => write!(f, "black"),
            Color::Red => write!(f, "red"),
            Color::Green => write!(f, "green"),
            Color::Yellow => write!(f, "yellow"),
            Color::Blue => write!(f, "blue"),
            Color::Magenta => write!(f, "magenta"),
            Color::Cyan => write!(f, "cyan"),
            Color::Gray => write!(f, "gray"),
            Color::LightRed =>  write!(f, "salmon"),
            Color::LightGreen =>  write!(f, "lightgreen"),
            Color::LightYellow =>  write!(f, "lightyellow"),
            Color::LightBlue =>  write!(f, "lightblue"),
            Color::LightMagenta =>  write!(f, "violet"),
            Color::LightCyan =>  write!(f, "lightcyan"),
            Color::LightGray =>  write!(f, "lightgray"),
            Color::White => write!(f, "white"),
            Color::Hex(h) => write!(f, "{}", h),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Modifier {
    Italic,
    Bold,
    Underlined,
}

impl FromStr for Modifier {
    type Err = &'static str;

    fn from_str(modifier: &str) -> Result<Self, Self::Err> {
        match modifier {
            "bold" => Ok(Self::Bold),
            "italic" => Ok(Self::Italic),
            "underlined" => Ok(Self::Underlined),
            _ => Err("Invalid modifier"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub modifiers: Vec<Modifier>,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            fg: None,
            bg: None,
            modifiers: Vec::new(),
        }
    }
}


fn parse_value_as_str(value: &Value) -> Result<&str, String> {
    value
        .as_str()
        .ok_or(format!("Theme: unrecognized value: {}", value))
}

fn parse_color(value: &Value) -> Result<Color, String> {
    parse_value_as_str(value).and_then(|s| Ok(Color::Hex(s.to_string())))
}

#[derive(Debug)]
struct Palette {
    data: HashMap<String, Color>,
}

impl Default for Palette {
    fn default() -> Self {
        let mut data = HashMap::new();
        data.insert("black".to_string(), Color::Black);
        data.insert("red".to_string(), Color::Red);
        data.insert("green".to_string(), Color::Green);
        data.insert("yellow".to_string(), Color::Yellow);
        data.insert("blue".to_string(), Color::Blue);
        data.insert("magenta".to_string(), Color::Magenta);
        data.insert("cyan".to_string(), Color::Cyan);
        data.insert("gray".to_string(), Color::Gray);
        data.insert("light-red".to_string(), Color::LightRed);
        data.insert("light-green".to_string(), Color::LightGreen);
        data.insert("light-yellow".to_string(), Color::LightYellow);
        data.insert("light-blue".to_string(), Color::LightBlue);
        data.insert("light-magenta".to_string(), Color::LightMagenta);
        data.insert("light-cyan".to_string(), Color::LightCyan);
        data.insert("light-gray".to_string(), Color::LightGray);
        data.insert("white".to_string(), Color::White);

        Self { data }
    }
}

impl TryFrom<Value> for Palette {
    type Error = String;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        let map = match value {
            Value::Table(entries) => entries,
            _ => return Ok(Self::default()),
        };
        let mut palette = HashMap::with_capacity(map.len());
        for (name, value) in map {
            palette.insert(name, parse_color(&value)?);
        }

        Ok(Self::new(palette))
    }
}

impl Palette {
    pub fn new(palette: HashMap<String, Color>) -> Self {
        let mut default_palette = Self::default();
        default_palette.data.extend(palette);
        Self {
            data: default_palette.data,
        }
    }

    fn parse_color(&self, value: &Value) -> Result<Color, String> {
        let val = parse_value_as_str(value)?;
        self.data.get(val).cloned().ok_or("").or_else(|_| Ok(Color::Hex(val.to_string())))
    }

    pub fn parse_style(&self, value: Value) -> Result<Style, String> {
        let mut style = Style::default();

        if let Value::Table(entries) = value {
            for (name, val) in entries {
                match name.as_str() {
                    "fg" => style.fg = Some(self.parse_color(&val)?),
                    "bg" => style.bg = Some(self.parse_color(&val)?),
                    "modifiers" => {
                        let modifiers = val.as_array().ok_or("Modifiers should be an array")?;
                        for modifier in modifiers {
                            // We ignore all the other modifiers
                            if let Some(parsed_modifier) =
                                modifier.as_str().and_then(|s| s.parse::<Modifier>().ok())
                            {
                                style.modifiers.push(parsed_modifier);
                            }
                        }
                    }
                    _ => return Err(format!("Theme: invalid style attribute: {}", name)),
                }
            }
        } else {
            style.fg = Some(parse_color(&value)?);
        }

        Ok(style)
    }
}

// TODO: add a load method to load from filesystem
// TODO: try to highlight a file with a given theme from helix using the default query
// TODO: try to use helix Rust query to compare with the previous step.
#[derive(Debug)]
pub struct Theme {
    scopes: Vec<String>,
    highlights: Vec<Style>,
    // Some styles we will need outside of syntax scopes
    // TODO: those should be Color
    pub(crate) background: Style,
    pub(crate) foreground: Style,
    pub(crate) selection: Style,
    pub(crate) line_number: Style,
    pub(crate) line_number_selected: Style,
}

impl<'de> Deserialize<'de> for Theme {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut scopes = Vec::new();
        let mut highlights = Vec::new();
        let mut background = Style::default();
        let mut foreground = Style::default();
        let mut selection = Style::default();
        let mut line_number = Style::default();
        let mut line_number_selected = Style::default();

        if let Ok(mut colors) = HashMap::<String, Value>::deserialize(deserializer) {
            let palette = colors
                .remove("palette")
                .map(|value| Palette::try_from(value).unwrap_or_default())
                .unwrap_or_default();
            scopes.reserve(colors.len());
            highlights.reserve(colors.len());

            for (name, style_val) in colors {
                // TODO: handle errors
                let style = palette.parse_style(style_val).unwrap_or_default();
                if name.starts_with("ui") {
                    match name.as_str() {
                        "ui.background" => background = style,
                        "ui.text" => foreground = style,
                        "ui.selection" => selection = style,
                        "ui.linenr" => line_number = style,
                        "ui.linenr.selected" => line_number_selected = style,
                        _ => continue,
                    }
                } else {
                    scopes.push(name);
                    highlights.push(style);
                }
            }
        }

        Ok(Self { scopes, highlights, background, foreground, selection, line_number, line_number_selected })
    }
}

impl Theme {
    pub fn load(path: &str) -> Result<Self, String> {
        Ok(toml::from_str(&std::fs::read_to_string(path).expect("file exists")).expect("TODO"))
    }

    #[inline]
    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }

    #[inline]
    pub fn highlight(&self, index: usize) -> Style {
        // TODO: avoid the clone after benchmarking it
        self.highlights[index].clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_load_theme() {
        let theme = Theme2::load("onedark.toml");
        assert!(theme.is_ok());
    }
}