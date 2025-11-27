use std::collections::HashMap;
use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::renderers::html::HtmlEscaped;
use crate::scope::Scope;
use crate::themes::{CompiledTheme, Style};
use crate::tokenizer::Token;

/// Theme-aware styling information for syntax highlighting
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ThemeStyle {
    Single(Style),
    Dual { light: Style, dark: Style },
}

impl ThemeStyle {
    /// Get the primary style (single style or light style for dual themes)
    pub fn primary_style(&self) -> &Style {
        match self {
            ThemeStyle::Single(style) => style,
            ThemeStyle::Dual { light, dark: _ } => light,
        }
    }

    /// Get the foreground color of the primary style
    pub fn foreground(&self) -> crate::themes::Color {
        self.primary_style().foreground
    }

    /// Get the font style of the primary style
    pub fn font_style(&self) -> crate::themes::FontStyle {
        self.primary_style().font_style
    }

    /// Create a single ThemeStyle from a Style
    pub fn single(style: Style) -> Self {
        ThemeStyle::Single(style)
    }
}

/// A token with associated styling information
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HighlightedText {
    pub text: String,
    pub style: ThemeStyle,
}

impl HighlightedText {
    /// Renders this highlighted text as an HTML span element for a single theme.
    pub fn as_html(&self, default_style: &Style) -> String {
        let escaped = HtmlEscaped(self.text.as_str());
        let style_to_use = match &self.style {
            ThemeStyle::Single(style) => style,
            ThemeStyle::Dual { light, dark: _ } => light, // Default to light theme for single rendering
        };

        if *style_to_use == *default_style {
            format!("<span>{escaped}</span>")
        } else {
            let mut css_style = String::with_capacity(30);
            if style_to_use.foreground != default_style.foreground {
                css_style.push_str(&style_to_use.foreground.as_css_color_property());
            }
            if style_to_use.background != default_style.background {
                css_style.push_str(&style_to_use.background.as_css_bg_color_property());
            }
            for font_attr in style_to_use.font_style.css_attributes() {
                css_style.push_str(font_attr);
            }
            format!(r#"<span style="{}">{escaped}</span>"#, css_style)
        }
    }

    /// Renders this highlighted text as HTML span element for dual themes (light/dark).
    pub fn as_html_dual(&self, light_default: &Style, dark_default: &Style) -> (String, String) {
        let escaped = HtmlEscaped(self.text.as_str());
        match &self.style {
            ThemeStyle::Single(style) => {
                // For single style, use the same style for both themes
                let light_html = self.render_html_with_style(&escaped, style, light_default);
                let dark_html = self.render_html_with_style(&escaped, style, dark_default);
                (light_html, dark_html)
            }
            ThemeStyle::Dual { light, dark } => {
                let light_html = self.render_html_with_style(&escaped, light, light_default);
                let dark_html = self.render_html_with_style(&escaped, dark, dark_default);
                (light_html, dark_html)
            }
        }
    }

    fn render_html_with_style(
        &self,
        escaped: &HtmlEscaped,
        style: &Style,
        default_style: &Style,
    ) -> String {
        if *style == *default_style {
            format!("<span>{escaped}</span>")
        } else {
            let mut css_style = String::with_capacity(30);
            if style.foreground != default_style.foreground {
                css_style.push_str(&style.foreground.as_css_color_property());
            }
            if style.background != default_style.background {
                css_style.push_str(&style.background.as_css_bg_color_property());
            }
            for font_attr in style.font_style.css_attributes() {
                css_style.push_str(font_attr);
            }
            format!(r#"<span style="{}">{escaped}</span>"#, css_style)
        }
    }
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

/// Highlighter that applies theme styles to tokenized code
#[derive(Debug, Clone)]
pub(crate) struct Highlighter<'r> {
    themes: Vec<&'r CompiledTheme>, // 1 theme for Single, 2 for Dual
    cache: [HashMap<Vec<Scope>, Style>; 2], // Separate cache per theme (max 2)
}

/// Helper function to check if a ThemeStyle has decorations
fn theme_style_has_decorations(theme_style: &ThemeStyle) -> bool {
    match theme_style {
        ThemeStyle::Single(style) => style.has_decorations(),
        ThemeStyle::Dual { light, dark } => light.has_decorations() || dark.has_decorations(),
    }
}

/// Helper function to check if two ThemeStyles are equal (for merging)
fn theme_styles_equal(style1: &ThemeStyle, style2: &ThemeStyle) -> bool {
    style1 == style2
}

/// Helper function to create a default ThemeStyle based on number of themes
fn default_theme_style_for_count(theme_count: usize) -> ThemeStyle {
    match theme_count {
        1 => ThemeStyle::Single(Style::default()),
        2 => ThemeStyle::Dual {
            light: Style::default(),
            dark: Style::default(),
        },
        _ => panic!("Unsupported theme count"),
    }
}

impl<'r> Highlighter<'r> {
    /// Create a new highlighter from a compiled theme
    pub fn new(theme: &'r CompiledTheme) -> Self {
        Highlighter {
            themes: vec![theme],
            cache: [HashMap::new(), HashMap::new()],
        }
    }

    /// Create a new highlighter for dual themes (light and dark)
    pub fn new_dual(light_theme: &'r CompiledTheme, dark_theme: &'r CompiledTheme) -> Self {
        Highlighter {
            themes: vec![light_theme, dark_theme],
            cache: [HashMap::new(), HashMap::new()],
        }
    }

    /// Match a scope stack against theme rules, building styles hierarchically
    /// like vscode-textmate does
    pub fn match_scopes(&mut self, scopes: &[Scope]) -> ThemeStyle {
        match self.themes.len() {
            1 => {
                // Single theme case
                let style = self.match_scopes_for_theme(scopes, 0);
                ThemeStyle::Single(style)
            }
            2 => {
                // Dual theme case
                let light_style = self.match_scopes_for_theme(scopes, 0);
                let dark_style = self.match_scopes_for_theme(scopes, 1);
                ThemeStyle::Dual {
                    light: light_style,
                    dark: dark_style,
                }
            }
            _ => panic!("Highlighter supports only 1 or 2 themes"),
        }
    }

    /// Match scopes for a specific theme index with caching
    fn match_scopes_for_theme(&mut self, scopes: &[Scope], theme_index: usize) -> Style {
        // Check cache first
        if let Some(&cached_style) = self.cache[theme_index].get(scopes) {
            return cached_style;
        }

        // Cache miss - compute style
        let style = self.match_scopes_uncached_for_theme(scopes, theme_index);

        // Cache the result
        self.cache[theme_index].insert(scopes.to_vec(), style);

        style
    }

    /// Match a scope stack against theme rules without caching for a specific theme
    fn match_scopes_uncached_for_theme(&self, scopes: &[Scope], theme_index: usize) -> Style {
        let theme = self.themes[theme_index];
        let mut current_style = theme.default_style;

        // Build up scope path incrementally, simulating vscode-textmate's approach
        // Each scope level can override the accumulated style
        for i in 1..=scopes.len() {
            let current_scope_path = &scopes[0..i];

            for rule in &theme.rules {
                if rule.selector.matches(current_scope_path) {
                    current_style = rule.style_modifier.apply_to(&current_style);
                }
            }
            // If no match found, current_style remains unchanged (inheritance!)
        }

        current_style
    }

    /// Apply highlighting to tokenized lines, preserving line structure.
    pub fn highlight_tokens(
        &mut self,
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

            // first merge all ws by prepending to the next non-ws token
            if options.merge_whitespaces {
                let num_tokens = line_result.len();
                let mut merged = Vec::with_capacity(num_tokens);
                let mut carry_on_range: Option<Range<usize>> = None;

                for (idx, (span, theme_style)) in line_result.into_iter().enumerate() {
                    let could_merge = !theme_style_has_decorations(&theme_style);
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
                                merged.push((carried_range.start..span.end, theme_style))
                            } else {
                                // We need to push 2 tokens here, one for the carried WS and one
                                // for the current token
                                merged.push((
                                    carried_range.clone(),
                                    default_theme_style_for_count(self.themes.len()),
                                ));
                                merged.push((span, theme_style));
                            }
                            carry_on_range = None;
                        } else {
                            merged.push((span, theme_style));
                        }
                    }
                }

                line_result = merged;
            }

            // then merge same style tokens after we did the WS
            if options.merge_same_style_tokens {
                let num_tokens = line_result.len();
                let mut merged: Vec<(Range<usize>, ThemeStyle)> = Vec::with_capacity(num_tokens);

                for (span, theme_style) in line_result {
                    if let Some((prev_span, prev_theme_style)) = merged.last_mut() {
                        if theme_styles_equal(&theme_style, prev_theme_style) {
                            prev_span.end = span.end;
                        } else {
                            merged.push((span, theme_style));
                        }
                    } else {
                        merged.push((span, theme_style));
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
            highlight_background_color: None,
            rules: vec![
                CompiledThemeRule {
                    selector: parse_selector("comment").unwrap(),
                    style_modifier: StyleModifier {
                        foreground: Some(color("#6A9955")),
                        background: None,
                        font_style: Some(FontStyle::ITALIC),
                    },
                },
                CompiledThemeRule {
                    selector: parse_selector("keyword").unwrap(),
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
    fn test_match_scopes() {
        let test_theme = test_theme();
        let mut highlighter = Highlighter::new(&test_theme);

        // Test matching scopes
        let comment_style = highlighter.match_scopes(&[scope("comment")]);
        assert_eq!(comment_style.foreground(), color("#6A9955"));
        assert_eq!(comment_style.font_style(), FontStyle::ITALIC);

        let keyword_style = highlighter.match_scopes(&[scope("keyword")]);
        assert_eq!(keyword_style.foreground(), color("#569CD6"));
        assert_eq!(keyword_style.font_style(), FontStyle::BOLD);

        // Test unmatched scope returns default
        let unknown_style = highlighter.match_scopes(&[scope("unknown")]);
        assert_eq!(
            unknown_style,
            ThemeStyle::single(highlighter.themes[0].default_style)
        );
    }

    #[test]
    fn test_highlight_tokens() {
        let test_theme = test_theme();
        let mut highlighter = Highlighter::new(&test_theme);
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
        assert_eq!(highlighted[0][0].style.foreground(), color("#569CD6"));

        // Unknown token uses default
        assert_eq!(highlighted[0][1].text, "hello");
        assert_eq!(highlighted[0][1].style.foreground(), color("#D4D4D4"));

        // Comment token
        assert_eq!(highlighted[1][0].text, "//");
        assert_eq!(highlighted[1][0].style.foreground(), color("#6A9955"));
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
            kind: Some("dark".to_string()),
            colors: Colors {
                foreground: "#D4D4D4".to_string(),
                background: "#1E1E1E".to_string(),
                highlight_background: None,
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
        let mut highlighter = Highlighter::new(&inheritance_theme);

        // Test: constant should get its own values
        let style = highlighter.match_scopes(&[scope("constant")]);
        assert_eq!(style.foreground(), color("#300000"));
        assert_eq!(style.font_style(), FontStyle::ITALIC);

        // Test: constant.numeric should inherit fontStyle from constant but override foreground
        let style = highlighter.match_scopes(&[scope("constant"), scope("constant.numeric")]);
        assert_eq!(style.foreground(), color("#400000")); // Overridden
        assert_eq!(style.font_style(), FontStyle::ITALIC);

        // Test: constant.numeric.hex should inherit foreground from constant.numeric but override fontStyle
        let style = highlighter.match_scopes(&[
            scope("constant"),
            scope("constant.numeric"),
            scope("constant.numeric.hex"),
        ]);
        assert_eq!(style.foreground(), color("#400000")); // Should inherit from constant.numeric
        assert_eq!(style.font_style(), FontStyle::BOLD); // Overridden
    }

    #[test]
    fn test_real_world_theme_inheritance() {
        // Load the Vitesse Black theme - a real production theme
        let theme_path =
            PathBuf::from("grammars-themes/packages/tm-themes/themes/vitesse-black.json");
        let raw_theme = RawTheme::load_from_file(theme_path).unwrap();
        let compiled_theme = CompiledTheme::from_raw_theme(raw_theme).unwrap();
        let mut highlighter = Highlighter::new(&compiled_theme);

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
        assert_ne!(style1, ThemeStyle::single(compiled_theme.default_style));
        assert_ne!(style2, ThemeStyle::single(compiled_theme.default_style));
        assert_ne!(style3, ThemeStyle::single(compiled_theme.default_style));

        // Token 2 should have distinct styling due to invalid.illegal scope
        // which typically gets error/warning colors in themes
        assert_ne!(style1, style2);
        assert_ne!(style2, style3);

        // Basic sanity checks - styles should have reasonable colors
        // (Not pure black/white which would indicate broken highlighting)
        assert_ne!(style1.foreground(), Color::BLACK);
        assert_ne!(style2.foreground(), Color::BLACK);
        assert_ne!(style3.foreground(), Color::BLACK);

        // Token 'p' should get pink color from invalid.illegal rule (#FDAEB7)
        let expected_pink = Color {
            r: 253,
            g: 174,
            b: 183,
            a: 255,
        };
        assert_eq!(
            style2.foreground(),
            expected_pink,
            "Token 'p' should get pink color #FDAEB7 from invalid.illegal rule, got {:?}",
            style2.foreground()
        );

        // Print styles for manual verification during development
        println!("Token '<' style: {:?}", style1);
        println!("Token 'p' style: {:?}", style2);
        println!("Token '>' style: {:?}", style3);
    }

    #[test]
    fn test_as_html_empty() {
        let test_theme = test_theme();
        let ht = HighlightedText {
            text: "hello".to_string(),
            style: ThemeStyle::single(test_theme.default_style),
        };
        let res = ht.as_html(&test_theme.default_style);
        insta::assert_snapshot!(res, @"<span>hello</span>");
    }

    #[test]
    fn test_as_html_content_escape() {
        let test_theme = test_theme();
        let ht = HighlightedText {
            text: "<script></script>".to_string(),
            style: ThemeStyle::single(test_theme.default_style),
        };
        let res = ht.as_html(&test_theme.default_style);
        insta::assert_snapshot!(res, @"<span>&lt;script&gt;&lt;/script&gt;</span>");
    }

    #[test]
    fn test_as_html_hex_fg_diff() {
        let test_theme = test_theme();
        let custom_style = Style {
            foreground: color("#FFFF00"),
            background: test_theme.default_style.background,
            font_style: test_theme.default_style.font_style,
        };
        let ht = HighlightedText {
            text: "hello".to_string(),
            style: ThemeStyle::single(custom_style),
        };
        let res = ht.as_html(&test_theme.default_style);
        insta::assert_snapshot!(res, @r#"<span style="color: #FFFF00;">hello</span>"#);
    }

    #[test]
    fn test_as_html_hex_bg_diff() {
        let test_theme = test_theme();
        let custom_style = Style {
            foreground: test_theme.default_style.foreground,
            background: color("#FFFF00"),
            font_style: test_theme.default_style.font_style,
        };
        let ht = HighlightedText {
            text: "hello".to_string(),
            style: ThemeStyle::single(custom_style),
        };
        let res = ht.as_html(&test_theme.default_style);
        insta::assert_snapshot!(res, @r#"<span style="background-color: #FFFF00;">hello</span>"#);
    }

    #[test]
    fn test_as_html_hex_fontstyle_diff() {
        let test_theme = test_theme();
        let custom_style = Style {
            foreground: test_theme.default_style.foreground,
            background: test_theme.default_style.background,
            font_style: FontStyle::ITALIC,
        };
        let ht = HighlightedText {
            text: "hello".to_string(),
            style: ThemeStyle::single(custom_style),
        };
        let res = ht.as_html(&test_theme.default_style);
        insta::assert_snapshot!(res, @r#"<span style="font-style: italic;">hello</span>"#);
    }

    #[test]
    fn test_as_html_hex_completely_different() {
        let test_theme = test_theme();
        let custom_style = Style {
            foreground: color("#FFFF00"),
            background: color("#FFFF00"),
            font_style: FontStyle::ITALIC,
        };
        let ht = HighlightedText {
            text: "hello".to_string(),
            style: ThemeStyle::single(custom_style),
        };
        let res = ht.as_html(&test_theme.default_style);
        insta::assert_snapshot!(res, @r#"<span style="color: #FFFF00;background-color: #FFFF00;font-style: italic;">hello</span>"#);
    }
}
