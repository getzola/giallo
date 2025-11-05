use std::ops::Range;

use crate::scope::Scope;
use crate::themes::{CompiledTheme, Style, ThemeSelector};
use crate::tokenizer::Token;

/// A token with associated styling information
#[derive(Debug, Clone, PartialEq)]
pub struct TokenWithStyle {
    pub range: Range<usize>,
    pub style: Style,
}

/// Internal rule for highlighting - one selector per rule
#[derive(Debug, Clone)]
struct HighlightRule {
    selector: ThemeSelector,
    style: Style,
}

/// Highlighter that applies theme styles to tokenized code
#[derive(Debug, Clone)]
pub struct Highlighter {
    rules: Vec<HighlightRule>,
    default_style: Style,
}

impl Highlighter {
    /// Create a new highlighter from a compiled theme
    pub fn new(theme: &CompiledTheme) -> Self {
        let mut rules = Vec::new();

        // Flatten CompiledThemeRules into HighlightRules (one per selector)
        // Rules are already sorted by specificity in CompiledTheme
        for compiled_rule in &theme.rules {
            for selector in &compiled_rule.selectors {
                let style = compiled_rule.style_modifier.apply_to(&theme.default_style);
                rules.push(HighlightRule {
                    selector: selector.clone(),
                    style,
                });
            }
        }

        Highlighter {
            rules,
            default_style: theme.default_style.clone(),
        }
    }

    /// Match a scope stack against theme rules, returning the most specific style
    pub fn match_scopes(&self, scopes: &[Scope]) -> Style {
        // Linear scan through rules (already sorted by specificity)
        // Return first match since rules are ordered most-specific first
        for rule in &self.rules {
            if rule.selector.matches(scopes) {
                return rule.style.clone();
            }
        }

        // No match found, return default style
        self.default_style.clone()
    }

    /// Apply highlighting to tokenized lines, preserving line structure.
    /// Merges adjacent tokens with the same style for optimization.
    pub fn highlight_tokens(&self, tokens: Vec<Vec<Token>>) -> Vec<Vec<TokenWithStyle>> {
        let mut result = Vec::with_capacity(tokens.len());

        for line_tokens in tokens {
            if line_tokens.is_empty() {
                result.push(Vec::new());
                continue;
            }

            let mut line_result: Vec<TokenWithStyle> = Vec::with_capacity(line_tokens.len());

            for token in line_tokens {
                let style = self.match_scopes(&token.scopes);

                // Try to merge with the last token in line_result
                if let Some(last_token) = line_result.last_mut() {
                    if last_token.style == style {
                        // Same style - extend the range to include this token
                        last_token.range.end = token.span.end;
                        continue; // Skip creating a new token
                    }
                }

                // Different style or first token - create new TokenWithStyle
                line_result.push(TokenWithStyle {
                    range: token.span.clone(),
                    style,
                });
            }

            result.push(line_result);
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope::Scope;
    use crate::themes::{
        Color, CompiledTheme, CompiledThemeRule, FontStyle, StyleModifier, ThemeType,
        parse_selector,
    };
    use crate::tokenizer::Token;
    use std::ops::Range;

    // Helper functions
    fn scope(name: &str) -> Scope {
        Scope::new(name)[0]
    }

    fn color(hex: &str) -> Color {
        Color::from_hex(hex).unwrap()
    }

    fn token(start: usize, end: usize, scope_name: &str) -> Token {
        Token {
            span: Range { start, end },
            scopes: vec![scope(scope_name)],
        }
    }

    fn test_theme() -> CompiledTheme {
        CompiledTheme {
            name: "Test".to_string(),
            theme_type: ThemeType::Dark,
            default_style: Style {
                foreground: color("#D4D4D4"),
                background: color("#1E1E1E"),
                font_style: FontStyle::empty(),
            },
            rules: vec![
                CompiledThemeRule {
                    selectors: vec![parse_selector("comment").unwrap()],
                    style_modifier: StyleModifier {
                        foreground: Some(color("#6A9955")),
                        background: None,
                        font_style: Some(FontStyle::ITALIC),
                    },
                },
                CompiledThemeRule {
                    selectors: vec![parse_selector("keyword").unwrap()],
                    style_modifier: StyleModifier {
                        foreground: Some(color("#569CD6")),
                        background: None,
                        font_style: Some(FontStyle::BOLD),
                    },
                },
            ],
        }
    }

    #[test]
    fn test_highlighter_new() {
        let theme = test_theme();
        let highlighter = Highlighter::new(&theme);
        assert_eq!(highlighter.rules.len(), 2);
        assert_eq!(highlighter.default_style, theme.default_style);
    }

    #[test]
    fn test_match_scopes() {
        let highlighter = Highlighter::new(&test_theme());

        // Test matching scopes
        let comment_style = highlighter.match_scopes(&[scope("comment")]);
        assert_eq!(comment_style.foreground, color("#6A9955"));
        assert_eq!(comment_style.font_style, FontStyle::ITALIC);

        let keyword_style = highlighter.match_scopes(&[scope("keyword")]);
        assert_eq!(keyword_style.foreground, color("#569CD6"));
        assert_eq!(keyword_style.font_style, FontStyle::BOLD);

        // Test unmatched scope returns default
        let unknown_style = highlighter.match_scopes(&[scope("unknown")]);
        assert_eq!(unknown_style, highlighter.default_style);
    }

    #[test]
    fn test_highlight_tokens() {
        let highlighter = Highlighter::new(&test_theme());
        let tokens = vec![
            vec![token(0, 2, "keyword"), token(3, 8, "unknown")],
            vec![token(0, 2, "comment")],
        ];

        let highlighted = highlighter.highlight_tokens(tokens);

        assert_eq!(highlighted.len(), 2);
        assert_eq!(highlighted[0].len(), 2);
        assert_eq!(highlighted[1].len(), 1);

        // Keyword token
        assert_eq!(highlighted[0][0].range, Range { start: 0, end: 2 });
        assert_eq!(highlighted[0][0].style.foreground, color("#569CD6"));

        // Unknown token uses default
        assert_eq!(highlighted[0][1].range, Range { start: 3, end: 8 });
        assert_eq!(highlighted[0][1].style.foreground, color("#D4D4D4"));

        // Comment token
        assert_eq!(highlighted[1][0].range, Range { start: 0, end: 2 });
        assert_eq!(highlighted[1][0].style.foreground, color("#6A9955"));
    }

    #[test]
    fn test_style_modifier_apply_to() {
        let base = Style {
            foreground: color("#FFFFFF"),
            background: color("#000000"),
            font_style: FontStyle::empty(),
        };

        let modifier = StyleModifier {
            foreground: Some(color("#FF0000")),
            background: None,
            font_style: Some(FontStyle::BOLD),
        };

        let result = modifier.apply_to(&base);
        assert_eq!(result.foreground, color("#FF0000"));
        assert_eq!(result.background, color("#000000")); // unchanged
        assert_eq!(result.font_style, FontStyle::BOLD);
    }
}
