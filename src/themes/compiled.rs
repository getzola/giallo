use serde::{Deserialize, Serialize};

// Removed scope ID dependencies - now using strings directly
use crate::themes::Color;
use crate::themes::font_style::FontStyle;
use crate::themes::raw::{RawTheme, TokenColorSettings};
use crate::themes::selector::{Parent, ThemeSelector, parse_selector};

/// Internal struct for calculating theme rule specificity during compilation.
///
/// Implements VSCode-TextMate specificity rules with 3-tier ordering:
/// 1. scope_depth: Number of scopes in the selector (parents + target)
/// 2. parent_length: Total characters in parent scope names
/// 3. parent_count: Number of parent scope requirements
///
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Specificity {
    scope_depth: u32,
    parent_length: u32,
    parent_count: u32,
}

impl Specificity {
    /// Calculate specificity for a theme selector.
    fn calculate(selector: &ThemeSelector) -> Self {
        let scope_depth = selector.parent_scopes.len() as u32 + 1;

        let parent_length = selector
            .parent_scopes
            .iter()
            .map(|parent| {
                let scope = match parent {
                    Parent::Anywhere(s) | Parent::Direct(s) => s,
                };
                scope.build_string().len() as u32
            })
            .sum();

        let parent_count = selector.parent_scopes.len() as u32;

        Specificity {
            scope_depth,
            parent_length,
            parent_count,
        }
    }
}

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
    pub selectors: Vec<ThemeSelector>,
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

        let mut default_style = Style {
            foreground,
            background,
            font_style: FontStyle::empty(),
        };

        let mut rules_with_specificity = Vec::new();

        // TODO: Implement VSCode-TextMate compatible deduplication
        // VSCode-TextMate merges rules with identical selectors during build-time,
        // with later rules overwriting earlier ones. Current implementation doesn't
        // handle this edge case, but it works correctly for most real themes.

        // Parse each token color rule
        for token_rule in raw_theme.token_colors {
            if token_rule.scope.is_empty() {
                if let Some(fg) = token_rule.settings.foreground() {
                    default_style.foreground = Color::from_hex(&fg)?;
                }
                if let Some(bg) = token_rule.settings.background() {
                    default_style.background = Color::from_hex(&bg)?;
                }
                continue;
            }

            let mut selectors = Vec::new();

            for scope_pattern in &token_rule.scope {
                if let Some(selector) = parse_selector(scope_pattern) {
                    selectors.push(selector);
                } else {
                    // Log warning for unparseable selectors but continue
                    eprintln!(
                        "Warning: Failed to parse theme selector: '{}'",
                        scope_pattern
                    );
                }
            }

            if !selectors.is_empty() {
                // Calculate max specificity among all selectors in this rule
                let max_specificity = selectors
                    .iter()
                    .map(|sel| Specificity::calculate(sel))
                    .max()
                    .unwrap();

                let style_modifier = StyleModifier::try_from(token_rule.settings.clone())?;

                rules_with_specificity.push((
                    CompiledThemeRule {
                        selectors,
                        style_modifier,
                    },
                    max_specificity,
                ));
            }
        }

        // Sort by specificity (highest first)
        rules_with_specificity.sort_by(|a, b| b.1.cmp(&a.1));

        // Extract rules (discard specificity)
        let rules = rules_with_specificity
            .into_iter()
            .map(|(rule, _)| rule)
            .collect();

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

    #[test]
    fn can_load_default_from_token_colors() {
        let theme = RawTheme::load_from_file("src/fixtures/themes/all_scope_styles.json").unwrap();
        let compiled = CompiledTheme::from_raw_theme(theme).unwrap();
        assert_eq!(compiled.default_style.background.as_hex(), "#23262E");
        assert_eq!(compiled.default_style.foreground.as_hex(), "#D5CED9");
    }
}
