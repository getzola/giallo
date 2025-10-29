use serde::{Deserialize, Serialize};

// Removed scope ID dependencies - now using strings directly
use crate::themes::Color;
use crate::themes::font_style::FontStyle;
use crate::themes::raw::{RawTheme, TokenColorSettings};

/// A complete style with foreground, background colors and font styling
///
/// This is the runtime representation that always has concrete values.
/// Total size: 9 bytes (4 + 4 + 1)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
pub struct Style {
    pub foreground: Color,
    pub background: Color,
    pub font_style: FontStyle,
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
    pub foreground: Option<Color>,
    pub background: Option<Color>,
    pub font_style: Option<FontStyle>,
}

impl TryFrom<TokenColorSettings> for StyleModifier {
    type Error = Box<dyn std::error::Error>;

    fn try_from(settings: TokenColorSettings) -> Result<Self, Self::Error> {
        let foreground = if let Some(s) = settings.foreground() {
            Some(Color::from_hex(s)?)
        } else {
            None
        };
        let background = if let Some(s) = settings.background() {
            Some(Color::from_hex(s)?)
        } else {
            None
        };

        let font_style = settings.font_style.map(|s| FontStyle::from_str(&s));

        Ok(Self {
            foreground,
            background,
            font_style,
        })
    }
}

/// Theme type for determining fallback colors
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeType {
    Light,
    #[default]
    Dark,
}

impl ThemeType {
    // (fg, bg)
    pub fn default_colors(&self) -> (Color, Color) {
        match self {
            ThemeType::Light => (Color::LIGHT_FG_FALLBACK, Color::LIGHT_BG_FALLBACK),
            ThemeType::Dark => (Color::DARK_FG_FALLBACK, Color::DARK_BG_FALLBACK),
        }
    }

    pub fn from_str(s: &str) -> ThemeType {
        if s.eq_ignore_ascii_case("light") {
            ThemeType::Light
        } else {
            ThemeType::Dark
        }
    }
}

/// Compiled theme rule for efficient matching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledThemeRule {
    /// Compiled scope patterns - each pattern is a sequence of scope names
    pub scope_patterns: Vec<Vec<String>>,
    pub style_modifier: StyleModifier,
}

/// Compiled theme optimized for fast lookups
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledTheme {
    pub name: String,
    /// Theme type ("light" or "dark")
    pub theme_type: ThemeType,
    /// Default style for tokens with no specific rules
    pub default_style: Style,
    /// Theme rules sorted by specificity (most specific first)
    pub rules: Vec<CompiledThemeRule>,
}

impl CompiledTheme {
    pub fn from_raw_theme(raw_theme: RawTheme) -> Result<Self, Box<dyn std::error::Error>> {
        let theme_type = raw_theme
            .type_
            .map(|s| ThemeType::from_str(&s))
            .unwrap_or_default();

        let foreground = Color::from_hex(&raw_theme.colors.foreground)?;
        let background = Color::from_hex(&raw_theme.colors.background)?;

        let default_style = Style {
            foreground,
            background,
            font_style: FontStyle::empty(),
        };

        let mut rules = Vec::new();

        for token_rule in raw_theme.token_colors {
            // Should use the theme default style i think?
            if token_rule.scope.is_empty() {
                continue;
            }

            let mut scope_patterns = Vec::new();
            for scopes in token_rule.get_scope_patterns() {
                if scopes.is_empty() {
                    continue;
                }

                if !scopes.is_empty() {
                    scope_patterns.push(scopes);
                }
            }

            if !scope_patterns.is_empty() {
                let style_modifier = StyleModifier::try_from(token_rule.settings)?;
                rules.push(CompiledThemeRule {
                    scope_patterns,
                    style_modifier,
                });
            }
        }

        Ok(CompiledTheme {
            name: raw_theme.name,
            theme_type,
            default_style,
            rules,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn can_load_and_compile_all_shiki_themes() {
        let entries = fs::read_dir("grammars-themes/packages/tm-themes/themes")
            .expect("Failed to read grammars directory");

        for entry in entries {
            let entry = entry.expect("Failed to read directory entry");
            let path = entry.path();
            println!("{:?}", path);
            RawTheme::load_from_file(&path)
                .unwrap()
                .compile()
                .expect(&format!("Failed to compile theme: {path:?}"));
        }
    }
}
