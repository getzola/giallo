use crate::scope::Scope;
use crate::themes::{CompiledTheme, Style, StyleModifier, ThemeSelector};
use crate::tokenizer::Token;
use serde::{Deserialize, Serialize};
use std::ops::Range;

/// A token with associated styling information
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HighlightedText {
    pub text: String,
    pub style: Style,
}

/// Options for token merging behavior
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MergingOptions {
    pub merge_whitespaces: bool,
    pub merge_same_style_tokens: bool,
}

impl Default for MergingOptions {
    fn default() -> Self {
        Self {
            merge_whitespaces: true,
            merge_same_style_tokens: true,
        }
    }
}

/// Internal rule for highlighting - one selector per rule
#[derive(Debug, Clone)]
struct HighlightRule {
    selector: ThemeSelector,
    style_modifier: StyleModifier,
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
                rules.push(HighlightRule {
                    selector: selector.clone(),
                    style_modifier: compiled_rule.style_modifier,
                });
            }
        }

        Highlighter {
            rules,
            default_style: theme.default_style.clone(),
        }
    }

    /// Match a scope stack against theme rules, building styles hierarchically
    /// like vscode-textmate does
    pub fn match_scopes(&self, scopes: &[Scope]) -> Style {
        let mut current_style = self.default_style.clone();

        // Build up scope path incrementally, simulating vscode-textmate's approach
        // Each scope level can override the accumulated style
        for i in 1..=scopes.len() {
            let current_scope_path = &scopes[0..i];

            // Apply all rules that match this current scope path (in specificity order)
            for rule in &self.rules {
                if rule.selector.matches(current_scope_path) {
                    // Apply style modifier to accumulated style (proper inheritance!)
                    current_style = rule.style_modifier.apply_to(&current_style);
                    // Continue to apply all matching rules (rules sorted by specificity)
                }
            }
            // If no match found, current_style remains unchanged (inheritance!)
        }

        current_style
    }

    /// Apply highlighting to tokenized lines, preserving line structure.
    pub fn highlight_tokens(
        &self,
        content: &str,
        tokens: Vec<Vec<Token>>,
        options: MergingOptions,
    ) -> Vec<Vec<HighlightedText>> {
        let mut result = Vec::with_capacity(tokens.len());

        let lines = content.split('\n').collect::<Vec<_>>();

        for (line_tokens, line) in tokens.into_iter().zip(lines) {
            if line_tokens.is_empty() {
                result.push(Vec::new());
                continue;
            }

            let mut line_result = line_tokens
                .into_iter()
                .map(|x| (x.span, self.match_scopes(&x.scopes)))
                .collect::<Vec<_>>();

            // for token in line_tokens {
            //     let style = self.match_scopes(&token.scopes);
            //     line_result.push((token.span, style));

            // // Try to merge with the last token in line_result
            // if let Some(last_token) = line_result.last_mut() {
            //     if last_token.style == style {
            //         // Same style - extend the range to include this token
            //         last_token.range.end = token.span.end;
            //         continue; // Skip creating a new token
            //     }
            // }
            // }

            // first merge all ws by prepending to the next non-ws token
            if options.merge_whitespaces {
                let num_tokens = line_result.len();
                let mut merged = Vec::with_capacity(num_tokens);
                let mut carry_on_range: Option<Range<usize>> = None;

                for (idx, (span, style)) in line_result.into_iter().enumerate() {
                    let could_merge = !style.has_decorations();
                    let token_content = &line[span.clone()];
                    let is_whitespace_with_next = could_merge
                        && token_content.chars().all(|c| c.is_whitespace())
                        && idx + 1 < num_tokens;

                    if is_whitespace_with_next {
                        // Extend or initialize the carried range
                        carry_on_range = Some(match carry_on_range {
                            Some(range) => range.start..span.end,
                            None => span.clone(),
                        });
                    } else {
                        // We've hit a non-whitespace token or the last token in the line
                        if let Some(carried_range) = &carry_on_range {
                            if could_merge {
                                // We can prepend all the WS to that token
                                merged.push((carried_range.start..span.end, style))
                            } else {
                                // We need to push 2 tokens here, one for the carried WS and one
                                // for the current token
                                merged.push((carried_range.clone(), Style::default()));
                                merged.push((span, style));
                            }
                            carry_on_range = None;
                        } else {
                            merged.push((span, style));
                        }
                    }
                }

                line_result = merged;
            }

            // then merge same style tokens after we did the WS
            if options.merge_same_style_tokens {
                let num_tokens = line_result.len();
                let mut merged: Vec<(Range<usize>, Style)> = Vec::with_capacity(num_tokens);

                for (span, style) in line_result {
                    if let Some((prev_span, prev_style)) = merged.last_mut() {
                        if style == *prev_style {
                            prev_span.end = span.end;
                        } else {
                            merged.push((span, style));
                        }
                    } else {
                        merged.push((span, style));
                    }
                }

                line_result = merged;
            }

            // then transform into HighlightedText
            result.push(
                line_result
                    .into_iter()
                    .map(|(span, style)| HighlightedText {
                        style,
                        text: line[span].to_string(),
                    })
                    .collect(),
            );
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope::Scope;
    use crate::themes::{
        Color, Colors, CompiledTheme, CompiledThemeRule, FontStyle, RawTheme, StyleModifier,
        ThemeType, TokenColorRule, TokenColorSettings, parse_selector,
    };
    use crate::tokenizer::Token;
    use std::ops::Range;
    use std::path::PathBuf;

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
    fn debug_test() {
        let theme_path =
            PathBuf::from("grammars-themes/packages/tm-themes/themes/vitesse-black.json");
        let theme =
            CompiledTheme::from_raw_theme(RawTheme::load_from_file(theme_path).unwrap()).unwrap();
        let highlighter = Highlighter::new(&theme);
        let keyword_style = highlighter.match_scopes(&[scope("markup.bold.asciidoc")]);
        assert_eq!(keyword_style.font_style, FontStyle::BOLD);
    }

    #[test]
    fn test_highlight_tokens() {
        let highlighter = Highlighter::new(&test_theme());
        let tokens = vec![
            vec![token(0, 2, "keyword"), token(3, 8, "unknown")],
            vec![token(0, 2, "comment")],
        ];
        let content = "if hello\n//";

        let highlighted = highlighter.highlight_tokens(content, tokens, MergingOptions::default());

        assert_eq!(highlighted.len(), 2);
        assert_eq!(highlighted[0].len(), 2);
        assert_eq!(highlighted[1].len(), 1);

        // Keyword token
        assert_eq!(highlighted[0][0].text, "if");
        assert_eq!(highlighted[0][0].style.foreground, color("#569CD6"));

        // Unknown token uses default
        assert_eq!(highlighted[0][1].text, "hello");
        assert_eq!(highlighted[0][1].style.foreground, color("#D4D4D4"));

        // Comment token
        assert_eq!(highlighted[1][0].text, "//");
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

    #[test]
    fn test_theme_inheritance() {
        // Create RawTheme using proper theme structure
        let raw_theme = RawTheme {
            name: "Inheritance Test".to_string(),
            type_: Some("dark".to_string()),
            colors: Colors {
                foreground: "#D4D4D4".to_string(),
                background: "#1E1E1E".to_string(),
            },
            token_colors: vec![
                // Parent: constant - has both foreground and fontStyle
                TokenColorRule {
                    scope: vec!["constant".to_string()],
                    settings: TokenColorSettings {
                        foreground: Some("#300000".to_string()),
                        background: None,
                        font_style: Some("italic".to_string()),
                    },
                },
                // Child: constant.numeric - only foreground (should inherit italic)
                TokenColorRule {
                    scope: vec!["constant.numeric".to_string()],
                    settings: TokenColorSettings {
                        foreground: Some("#400000".to_string()),
                        background: None,
                        font_style: None, // Should inherit italic from constant
                    },
                },
                // Grandchild: constant.numeric.hex - only fontStyle (should inherit foreground)
                TokenColorRule {
                    scope: vec!["constant.numeric.hex".to_string()],
                    settings: TokenColorSettings {
                        foreground: None, // Should inherit #400000 from constant.numeric
                        background: None,
                        font_style: Some("bold".to_string()),
                    },
                },
            ],
        };

        // Compile using proper pipeline (automatically sorts by specificity)
        let inheritance_theme = CompiledTheme::from_raw_theme(raw_theme).unwrap();
        let highlighter = Highlighter::new(&inheritance_theme);

        // Test: constant should get its own values
        let style = highlighter.match_scopes(&[scope("constant")]);
        assert_eq!(style.foreground, color("#300000"));
        assert_eq!(style.font_style, FontStyle::ITALIC);

        // Test: constant.numeric should inherit fontStyle from constant but override foreground
        let style = highlighter.match_scopes(&[scope("constant"), scope("constant.numeric")]);
        assert_eq!(style.foreground, color("#400000")); // Overridden
        assert_eq!(style.font_style, FontStyle::ITALIC);

        // Test: constant.numeric.hex should inherit foreground from constant.numeric but override fontStyle
        let style = highlighter.match_scopes(&[
            scope("constant"),
            scope("constant.numeric"),
            scope("constant.numeric.hex"),
        ]);
        assert_eq!(style.foreground, color("#400000")); // Should inherit from constant.numeric
        assert_eq!(style.font_style, FontStyle::BOLD); // Overridden
    }

    #[test]
    fn test_real_world_theme_inheritance() {
        // Load the Vitesse Black theme - a real production theme
        let theme_path =
            PathBuf::from("grammars-themes/packages/tm-themes/themes/vitesse-black.json");
        let raw_theme = RawTheme::load_from_file(theme_path).unwrap();
        let compiled_theme = CompiledTheme::from_raw_theme(raw_theme).unwrap();
        let highlighter = Highlighter::new(&compiled_theme);

        // Test real tokenizer output from ASP.NET Core Razor with invalid HTML tag
        // Token 1: '<' - HTML tag begin punctuation
        let token1_scopes = [
            scope("text.aspnetcorerazor"),
            scope("meta.element.structure.svg.html"),
            scope("meta.element.object.svg.foreignObject.html"),
            scope("meta.element.other.invalid.html"),
            scope("meta.tag.other.invalid.start.html"),
            scope("punctuation.definition.tag.begin.html"),
        ];
        let style1 = highlighter.match_scopes(&token1_scopes);

        // Token 2: 'p' - Invalid/unrecognized HTML tag name
        let token2_scopes = [
            scope("text.aspnetcorerazor"),
            scope("meta.element.structure.svg.html"),
            scope("meta.element.object.svg.foreignObject.html"),
            scope("meta.element.other.invalid.html"),
            scope("meta.tag.other.invalid.start.html"),
            scope("entity.name.tag.html"),
            scope("invalid.illegal.unrecognized-tag.html"),
        ];
        let style2 = highlighter.match_scopes(&token2_scopes);

        // Token 3: '>' - HTML tag end punctuation
        let token3_scopes = [
            scope("text.aspnetcorerazor"),
            scope("meta.element.structure.svg.html"),
            scope("meta.element.object.svg.foreignObject.html"),
            scope("meta.element.other.invalid.html"),
            scope("meta.tag.other.invalid.start.html"),
            scope("punctuation.definition.tag.end.html"),
        ];
        let style3 = highlighter.match_scopes(&token3_scopes);

        // Verify that styles are not default (theme inheritance is working)
        assert_ne!(style1, compiled_theme.default_style);
        assert_ne!(style2, compiled_theme.default_style);
        assert_ne!(style3, compiled_theme.default_style);

        // Token 2 should have distinct styling due to invalid.illegal scope
        // which typically gets error/warning colors in themes
        assert_ne!(style1, style2);
        assert_ne!(style2, style3);

        // Basic sanity checks - styles should have reasonable colors
        // (Not pure black/white which would indicate broken highlighting)
        assert_ne!(style1.foreground, Color::BLACK);
        assert_ne!(style2.foreground, Color::BLACK);
        assert_ne!(style3.foreground, Color::BLACK);

        // Token 'p' should get pink color from invalid.illegal rule (#FDAEB7)
        let expected_pink = Color {
            r: 253,
            g: 174,
            b: 183,
            a: 255,
        };
        assert_eq!(
            style2.foreground, expected_pink,
            "Token 'p' should get pink color #FDAEB7 from invalid.illegal rule, got {:?}",
            style2.foreground
        );

        // Print styles for manual verification during development
        println!("Token '<' style: {:?}", style1);
        println!("Token 'p' style: {:?}", style2);
        println!("Token '>' style: {:?}", style3);
    }
}
