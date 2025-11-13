use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Debug,
)]
pub struct FontStyle {
    bits: u8,
}

impl FontStyle {
    pub const BOLD: Self = Self { bits: 1 };
    pub const UNDERLINE: Self = Self { bits: 2 };
    pub const ITALIC: Self = Self { bits: 4 };
    pub const STRIKETHROUGH: Self = Self { bits: 8 };

    pub const fn empty() -> Self {
        Self { bits: 0 }
    }

    pub const fn is_empty(&self) -> bool {
        self.bits == 0
    }

    pub const fn contains(&self, other: Self) -> bool {
        (self.bits & other.bits) == other.bits
    }

    /// Returns the font style from a theme font style string
    pub fn from_str(font_style_str: &str) -> Self {
        let mut font_style = Self::empty();
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

    pub fn insert(&mut self, other: Self) {
        self.bits |= other.bits;
    }
}
