use crate::color::Color;
use serde::{Deserialize, Serialize};
use std::ops;

/// Font styling flags using bitwise operations
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

    /// Returns the set containing all flags
    pub const fn all() -> Self {
        let bits = Self::BOLD.bits | Self::UNDERLINE.bits | Self::ITALIC.bits;
        Self { bits }
    }

    /// Returns the raw value of the flags currently stored
    pub const fn bits(&self) -> u8 {
        self.bits
    }

    /// Convert from underlying bit representation
    pub const fn from_bits(bits: u8) -> Option<Self> {
        if (bits & !Self::all().bits()) == 0 {
            Some(Self { bits })
        } else {
            None
        }
    }

    /// Convert from underlying bit representation, dropping invalid bits
    pub const fn from_bits_truncate(bits: u8) -> Self {
        let bits = bits & Self::all().bits;
        Self { bits }
    }

    /// Returns `true` if no flags are currently stored
    pub const fn is_empty(&self) -> bool {
        self.bits() == Self::empty().bits()
    }

    /// Returns `true` if all flags are currently set
    pub const fn is_all(&self) -> bool {
        self.bits() == Self::all().bits()
    }

    /// Returns `true` if there are flags common to both `self` and `other`
    pub const fn intersects(&self, other: Self) -> bool {
        let bits = self.bits & other.bits;
        !(Self { bits }).is_empty()
    }

    /// Returns `true` if all of the flags in `other` are contained within `self`
    pub const fn contains(&self, other: Self) -> bool {
        (self.bits & other.bits) == other.bits
    }

    /// Inserts the specified flags in-place
    pub fn insert(&mut self, other: Self) {
        self.bits |= other.bits;
    }

    /// Removes the specified flags in-place
    pub fn remove(&mut self, other: Self) {
        self.bits &= !other.bits;
    }

    /// Toggles the specified flags in-place
    pub fn toggle(&mut self, other: Self) {
        self.bits ^= other.bits;
    }

    /// Inserts or removes the specified flags depending on the passed value
    pub fn set(&mut self, other: Self, value: bool) {
        if value {
            self.insert(other);
        } else {
            self.remove(other);
        }
    }

    /// Returns the intersection between the flags in `self` and `other`
    #[must_use]
    pub const fn intersection(self, other: Self) -> Self {
        let bits = self.bits & other.bits;
        Self { bits }
    }

    /// Returns the union of the flags in `self` and `other`
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        let bits = self.bits | other.bits;
        Self { bits }
    }

    /// Returns the difference between the flags in `self` and `other`
    pub const fn difference(self, other: Self) -> Self {
        let bits = self.bits & !other.bits;
        Self { bits }
    }

    /// Returns the symmetric difference between the flags in `self` and `other`
    #[must_use]
    pub const fn symmetric_difference(self, other: Self) -> Self {
        let bits = self.bits ^ other.bits;
        Self { bits }
    }

    /// Returns the complement of this set of flags
    #[must_use]
    pub const fn complement(self) -> Self {
        Self::from_bits_truncate(!self.bits)
    }
}

// Implement bitwise operations for FontStyle
impl ops::BitOr for FontStyle {
    type Output = Self;
    fn bitor(self, other: FontStyle) -> Self {
        let bits = self.bits | other.bits;
        Self { bits }
    }
}

impl ops::BitOrAssign for FontStyle {
    fn bitor_assign(&mut self, other: Self) {
        self.bits |= other.bits;
    }
}

impl ops::BitXor for FontStyle {
    type Output = Self;
    fn bitxor(self, other: Self) -> Self {
        let bits = self.bits ^ other.bits;
        Self { bits }
    }
}

impl ops::BitXorAssign for FontStyle {
    fn bitxor_assign(&mut self, other: Self) {
        self.bits ^= other.bits;
    }
}

impl ops::BitAnd for FontStyle {
    type Output = Self;
    fn bitand(self, other: Self) -> Self {
        let bits = self.bits & other.bits;
        Self { bits }
    }
}

impl ops::BitAndAssign for FontStyle {
    fn bitand_assign(&mut self, other: Self) {
        self.bits &= other.bits;
    }
}

impl ops::Sub for FontStyle {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        let bits = self.bits & !other.bits;
        Self { bits }
    }
}

impl ops::SubAssign for FontStyle {
    fn sub_assign(&mut self, other: Self) {
        self.bits &= !other.bits;
    }
}

impl ops::Not for FontStyle {
    type Output = Self;
    fn not(self) -> Self {
        Self { bits: !self.bits } & Self::all()
    }
}

/// A complete style with foreground, background colors and font styling
///
/// This is the runtime representation that always has concrete values.
/// Total size: 9 bytes (4 + 4 + 1)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
pub struct Style {
    /// Foreground (text) color
    pub foreground: Color,
    /// Background color
    pub background: Color,
    /// Font styling flags
    pub font_style: FontStyle,
}

impl Style {
    /// Create a new style with the given colors and font style
    pub const fn new(foreground: Color, background: Color, font_style: FontStyle) -> Self {
        Self {
            foreground,
            background,
            font_style,
        }
    }

    /// Apply a style modifier to this style, returning a new style
    pub fn apply(&self, modifier: StyleModifier) -> Style {
        Style {
            foreground: modifier.foreground.unwrap_or(self.foreground),
            background: modifier.background.unwrap_or(self.background),
            font_style: modifier.font_style.unwrap_or(self.font_style),
        }
    }
}

impl Default for Style {
    fn default() -> Style {
        Style {
            foreground: Color::BLACK,
            background: Color::WHITE,
            font_style: FontStyle::empty(),
        }
    }
}

/// A style modifier with optional values for theme parsing
///
/// This represents theme entries where colors and font styles are optional.
/// Used during theme loading and then resolved to concrete Style values.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct StyleModifier {
    /// Optional foreground color
    pub foreground: Option<Color>,
    /// Optional background color
    pub background: Option<Color>,
    /// Optional font style
    pub font_style: Option<FontStyle>,
}

impl StyleModifier {
    /// Create a new empty style modifier
    pub const fn new() -> Self {
        Self {
            foreground: None,
            background: None,
            font_style: None,
        }
    }

    /// Create a style modifier with just a foreground color
    pub const fn with_foreground(color: Color) -> Self {
        Self {
            foreground: Some(color),
            background: None,
            font_style: None,
        }
    }

    /// Create a style modifier with just a background color
    pub const fn with_background(color: Color) -> Self {
        Self {
            foreground: None,
            background: Some(color),
            font_style: None,
        }
    }

    /// Create a style modifier with just font styling
    pub const fn with_font_style(font_style: FontStyle) -> Self {
        Self {
            foreground: None,
            background: None,
            font_style: Some(font_style),
        }
    }

    /// Apply another modifier to this one, with the other taking precedence
    pub fn apply(&self, other: StyleModifier) -> StyleModifier {
        StyleModifier {
            foreground: other.foreground.or(self.foreground),
            background: other.background.or(self.background),
            font_style: other.font_style.or(self.font_style),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_font_style_bitflags() {
        let empty = FontStyle::empty();
        assert!(empty.is_empty());
        assert!(!empty.contains(FontStyle::BOLD));

        let bold = FontStyle::BOLD;
        assert!(!bold.is_empty());
        assert!(bold.contains(FontStyle::BOLD));
        assert!(!bold.contains(FontStyle::ITALIC));

        let bold_italic = FontStyle::BOLD | FontStyle::ITALIC;
        assert!(bold_italic.contains(FontStyle::BOLD));
        assert!(bold_italic.contains(FontStyle::ITALIC));
        assert!(!bold_italic.contains(FontStyle::UNDERLINE));

        let all = FontStyle::all();
        assert!(all.contains(FontStyle::BOLD));
        assert!(all.contains(FontStyle::ITALIC));
        assert!(all.contains(FontStyle::UNDERLINE));
    }

    #[test]
    fn test_style_size() {
        // Ensure Style is exactly 9 bytes for cache efficiency
        assert_eq!(std::mem::size_of::<Style>(), 9);
        assert_eq!(std::mem::size_of::<Color>(), 4);
        assert_eq!(std::mem::size_of::<FontStyle>(), 1);
    }

    #[test]
    fn test_style_application() {
        let base = Style::default();
        let modifier = StyleModifier {
            foreground: Some(Color::from_hex("#ff0000").unwrap()),
            background: None,
            font_style: Some(FontStyle::BOLD),
        };

        let result = base.apply(modifier);
        assert_eq!(result.foreground, Color::from_hex("#ff0000").unwrap());
        assert_eq!(result.background, Color::WHITE); // Unchanged
        assert_eq!(result.font_style, FontStyle::BOLD);
    }

    #[test]
    fn test_style_modifier_chaining() {
        let base = StyleModifier::with_foreground(Color::BLACK);
        let overlay = StyleModifier::with_font_style(FontStyle::ITALIC);

        let combined = base.apply(overlay);
        assert_eq!(combined.foreground, Some(Color::BLACK));
        assert_eq!(combined.background, None);
        assert_eq!(combined.font_style, Some(FontStyle::ITALIC));
    }
}
