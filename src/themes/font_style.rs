use core::fmt;
use std::fmt::Write;

use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Debug,
)]
/// A compressed representation of all available textmate font styles
pub struct FontStyle {
    bits: u8,
}

impl FontStyle {
    /// Bold bits
    pub const BOLD: Self = Self { bits: 1 };
    /// Underline bits
    pub const UNDERLINE: Self = Self { bits: 2 };
    /// Italic bits
    pub const ITALIC: Self = Self { bits: 4 };
    /// Strikethrough bits
    pub const STRIKETHROUGH: Self = Self { bits: 8 };

    /// Whether this font style is empty
    pub const fn is_empty(&self) -> bool {
        self.bits == 0
    }

    /// Whether this font style contains the other font style
    pub const fn contains(&self, other: Self) -> bool {
        (self.bits & other.bits) == other.bits
    }

    /// Returns the font style from a theme font style string
    pub fn from_theme_str(font_style_str: &str) -> Self {
        let mut font_style = Self::default();
        if font_style_str.contains("bold") {
            font_style.insert(FontStyle::BOLD);
        }
        if font_style_str.contains("italic") {
            font_style.insert(FontStyle::ITALIC);
        }
        if font_style_str.contains("underline") {
            font_style.insert(FontStyle::UNDERLINE);
        }
        if font_style_str.contains("strikethrough") {
            font_style.insert(FontStyle::STRIKETHROUGH);
        }
        font_style
    }

    pub(crate) fn insert(&mut self, other: Self) {
        self.bits |= other.bits;
    }

    /// Render the ANSI escape codes for the terminal
    pub(crate) fn ansi_escapes(self, f: &mut String) -> fmt::Result {
        if self.contains(FontStyle::BOLD) {
            write!(f, ";1")?;
        }
        if self.contains(FontStyle::ITALIC) {
            write!(f, ";3")?;
        }
        if self.contains(FontStyle::UNDERLINE) {
            write!(f, ";4")?;
        }
        if self.contains(FontStyle::STRIKETHROUGH) {
            write!(f, ";9")?;
        }
        Ok(())
    }

    pub(crate) fn css_attributes(&self) -> Vec<&'static str> {
        let mut out = Vec::new();

        if self.contains(FontStyle::BOLD) {
            out.push("font-weight: bold;");
        }
        if self.contains(FontStyle::ITALIC) {
            out.push("font-style: italic;");
        }
        if self.contains(FontStyle::UNDERLINE) && self.contains(FontStyle::STRIKETHROUGH) {
            out.push("text-decoration: underline line-through;");
        } else if self.contains(FontStyle::UNDERLINE) {
            out.push("text-decoration: underline;");
        } else if self.contains(FontStyle::STRIKETHROUGH) {
            out.push("text-decoration: line-through;");
        }

        out
    }
}
