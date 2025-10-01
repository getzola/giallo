use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Debug,
)]
pub struct FontStyle {
    bits: u8,
}

impl FontStyle {
    /// Bold font style
    pub const BOLD: Self = Self { bits: 1 };
    /// Underline font style
    pub const UNDERLINE: Self = Self { bits: 2 };
    /// Italic font style
    pub const ITALIC: Self = Self { bits: 4 };

    /// Returns an empty set of flags
    pub const fn empty() -> Self {
        Self { bits: 0 }
    }

    /// Returns `true` if no flags are currently stored
    pub const fn is_empty(&self) -> bool {
        self.bits == 0
    }

    /// Returns `true` if all of the flags in `other` are contained within `self`
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
        font_style
    }

    /// Inserts the specified flags in-place
    pub fn insert(&mut self, other: Self) {
        self.bits |= other.bits;
    }
}
