mod color;
mod compiled;
mod font_style;
mod raw;
mod selector;

use serde::{Deserialize, Serialize};

pub use color::Color;
pub use compiled::{CompiledTheme, CompiledThemeRule, Style, StyleModifier, ThemeType};
pub use font_style::FontStyle;
pub use raw::{Colors, RawTheme, TokenColorRule, TokenColorSettings};
pub use selector::{Parent, ThemeSelector, parse_selector};

/// Generic enum for single or dual (light/dark) theme values
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeVariant<T> {
    Single(T),
    Dual { light: T, dark: T },
}

impl ThemeVariant<Style> {
    pub(crate) fn has_decoration(&self) -> bool {
        match self {
            Self::Single(style) => style.has_decorations(),
            Self::Dual { light, dark } => light.has_decorations() || dark.has_decorations(),
        }
    }
}
