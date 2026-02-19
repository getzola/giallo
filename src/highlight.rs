use std::collections::HashMap;
use std::fmt::Write;
use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::renderers::html::HtmlEscaped;
use crate::scope::Scope;
use crate::themes::compiled::ThemeType;
use crate::themes::css::ClassId;
use crate::themes::{Color, CompiledTheme, Style, ThemeVariant};
use crate::tokenizer::Token;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// A token with associated styling information
pub struct HighlightedText {
    /// The text from the input string for that specific token
    pub text: String,
    /// The assigned style. It can be a single theme or dual theme if light/dark
    /// support was requested.
    pub style: ThemeVariant<Style>,
    /// Stable CSS class IDs that contributed to the final token style.
    /// Only used when `HtmlRenderer::css_class_prefix` is set.
    pub(crate) class_ids: Vec<ClassId>,
}

impl HighlightedText {
    /// Renders this highlighted text using terminal ANSI escape codes
    pub(crate) fn as_ansi(
        &self,
        theme: &ThemeVariant<&CompiledTheme>,
        theme_type: Option<ThemeType>,
        bg_color: Option<Color>,
        f: &mut String,
    ) {
        let s = self.text.as_str();

        if self.class_ids.is_empty() {
            f.push_str(s);
            return;
        }

        let (style, theme) = match (self.style, theme) {
            (ThemeVariant::Single(style), ThemeVariant::Single(theme)) => (style, theme),
            (
                ThemeVariant::Dual {
                    dark: dark_style, ..
                },
                ThemeVariant::Dual {
                    dark: dark_theme, ..
                },
            ) if theme_type == Some(ThemeType::Dark) => (dark_style, dark_theme),
            (
                ThemeVariant::Dual {
                    light: light_style, ..
                },
                ThemeVariant::Dual {
                    light: light_theme, ..
                },
            ) if theme_type == Some(ThemeType::Light) => (light_style, light_theme),
            _ => unreachable!(),
        };

        let default = &theme.default_style;
        if style == *default {
            f.push_str(s);
            return;
        }

        f.push_str("\x1b[");
        if style.foreground != default.foreground {
            style.foreground.as_ansi_fg(f);
        }
        if style.background != default.background {
            style.background.as_ansi_bg(f);
        }
        style.font_style.ansi_escapes(f);
        f.push('m');

        // Highlight background color
        if let Some(bg) = bg_color {
            f.push_str("\x1b[");
            bg.as_ansi_bg(f);
            f.push('m');
        }

        f.push_str(s);
        // reset
        f.push_str("\x1b[0m");
    }

    /// Renders this highlighted text as an HTML span element with either classes or inline style.
    pub(crate) fn as_html(
        &self,
        theme: &ThemeVariant<&CompiledTheme>,
        css_class_prefix: Option<&str>,
    ) -> String {
        let escaped = HtmlEscaped(self.text.as_str());

        // CSS class mode
        if let Some(prefix) = css_class_prefix {
            if self.class_ids.is_empty() {
                return format!("<span>{escaped}</span>");
            }
            let mut css_classes = String::new();
            for (idx, class_id) in self.class_ids.iter().enumerate() {
                if idx > 0 {
                    css_classes.push(' ');
                }
                css_classes.push_str(prefix);
                write!(css_classes, "{class_id}").unwrap();
            }
            return format!(r#"<span class="{css_classes}">{escaped}</span>"#);
        }

        // Inline style mode
        match (&self.style, theme) {
            (ThemeVariant::Single(style), ThemeVariant::Single(t)) => {
                let default = &t.default_style;
                if *style == *default {
                    return format!("<span>{escaped}</span>");
                }

                let mut css = String::with_capacity(30);
                if style.foreground != default.foreground {
                    css.push_str(&style.foreground.as_css_color_property());
                }
                if style.background != default.background {
                    css.push_str(&style.background.as_css_bg_color_property());
                }
                for font_attr in style.font_style.css_attributes() {
                    css.push_str(font_attr);
                }
                format!(r#"<span style="{css}">{escaped}</span>"#)
            }
            (
                ThemeVariant::Dual { light, dark },
                ThemeVariant::Dual {
                    light: lt,
                    dark: dt,
                },
            ) => {
                let light_default = &lt.default_style;
                let dark_default = &dt.default_style;

                if *light == *light_default && *dark == *dark_default {
                    return format!("<span>{escaped}</span>");
                }

                let mut css = String::with_capacity(60);

                if light.foreground != light_default.foreground
                    || dark.foreground != dark_default.foreground
                {
                    css.push_str(&Color::as_css_light_dark_color_property(
                        &light.foreground,
                        &dark.foreground,
                    ));
                }

                if light.background != light_default.background
                    || dark.background != dark_default.background
                {
                    css.push_str(&Color::as_css_light_dark_bg_color_property(
                        &light.background,
                        &dark.background,
                    ));
                }

                for font_attr in light.font_style.css_attributes() {
                    css.push_str(font_attr);
                }

                if css.is_empty() {
                    format!("<span>{escaped}</span>")
                } else {
                    format!(r#"<span style="{css}">{escaped}</span>"#)
                }
            }
            _ => unreachable!(),
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
    #[allow(clippy::type_complexity)]
    // Separate cache per theme (max 2)
    // token stack -> (style, contributing CSS class IDs)
    cache: [HashMap<Vec<Scope>, (Style, Vec<ClassId>)>; 2],
}

impl<'r> Highlighter<'r> {
    /// Create a new highlighter from a compiled theme
    pub(crate) fn new(theme: &'r CompiledTheme) -> Self {
        Highlighter {
            themes: vec![theme],
            cache: [HashMap::new(), HashMap::new()],
        }
    }

    /// Create a new highlighter for dual themes (light and dark)
    pub(crate) fn new_dual(light_theme: &'r CompiledTheme, dark_theme: &'r CompiledTheme) -> Self {
        Highlighter {
            themes: vec![light_theme, dark_theme],
            cache: [HashMap::new(), HashMap::new()],
        }
    }

    /// Match a scope stack against theme rules, building styles hierarchically
    /// like vscode-textmate does.
    /// Returns (style_variant, class_ids) where class_ids is the
    /// union of property winner classes from all themes. Only used for CSS class output.
    fn match_scopes(&mut self, scopes: &[Scope]) -> (ThemeVariant<Style>, Vec<ClassId>) {
        match self.themes.len() {
            1 => {
                let (style, class_ids) = self.match_scopes_for_theme(scopes, 0);
                (ThemeVariant::Single(style), class_ids)
            }
            2 => {
                let (light_style, light_class_ids) = self.match_scopes_for_theme(scopes, 0);
                let (dark_style, dark_class_ids) = self.match_scopes_for_theme(scopes, 1);

                // Union of contributing classes from both themes
                let mut class_ids = light_class_ids;
                for class_id in dark_class_ids {
                    if !class_ids.contains(&class_id) {
                        class_ids.push(class_id);
                    }
                }

                (
                    ThemeVariant::Dual {
                        light: light_style,
                        dark: dark_style,
                    },
                    class_ids,
                )
            }
            _ => unreachable!("Highlighter supports only 1 or 2 themes"),
        }
    }

    /// Match scopes for a specific theme index with caching
    fn match_scopes_for_theme(
        &mut self,
        scopes: &[Scope],
        theme_index: usize,
    ) -> (Style, Vec<ClassId>) {
        // Check cache first
        if let Some(cached) = self.cache[theme_index].get(scopes) {
            return cached.clone();
        }

        // Cache miss - compute style and contributing class IDs
        let result = self.match_scopes_uncached_for_theme(scopes, theme_index);
        self.cache[theme_index].insert(scopes.to_vec(), result.clone());

        result
    }

    /// Match a scope stack against theme rules without caching for a specific theme
    fn match_scopes_uncached_for_theme(
        &self,
        scopes: &[Scope],
        theme_index: usize,
    ) -> (Style, Vec<ClassId>) {
        let theme = self.themes[theme_index];
        let mut current_style = theme.default_style;

        // Track winners per style property.
        let mut fg_rule_idx: Option<usize> = None;
        let mut bg_rule_idx: Option<usize> = None;
        let mut font_rule_idx: Option<usize> = None;

        // Build up scope path incrementally, simulating vscode-textmate's approach
        // Each scope level can override the accumulated style
        for i in 1..=scopes.len() {
            let current_scope_path = &scopes[0..i];

            for (rule_idx, rule) in theme.rules.iter().enumerate() {
                if rule.selector.matches(current_scope_path) {
                    current_style = rule.style_modifier.apply_to(&current_style);
                    // Track the last winner for each property.
                    if rule.style_modifier.foreground.is_some() {
                        fg_rule_idx = Some(rule_idx);
                    }
                    if rule.style_modifier.background.is_some() {
                        bg_rule_idx = Some(rule_idx);
                    }
                    if rule.style_modifier.font_style.is_some() {
                        font_rule_idx = Some(rule_idx);
                    }
                }
            }
            // If no match found, current_style remains unchanged (inheritance!)
        }

        // Collect winner classes for fg/bg/font.
        let mut class_ids = Vec::new();
        if let Some(rule_idx) = fg_rule_idx {
            class_ids.push(theme.class_ids[rule_idx].fg);
        }
        if let Some(rule_idx) = bg_rule_idx {
            class_ids.push(theme.class_ids[rule_idx].bg);
        }
        if let Some(rule_idx) = font_rule_idx {
            class_ids.push(theme.class_ids[rule_idx].ft);
        }

        (current_style, class_ids)
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

            // Keep class IDs alongside span and style: (span, style, class_ids)
            let mut line_result: Vec<(Range<usize>, ThemeVariant<Style>, Vec<ClassId>)> =
                line_tokens
                    .into_iter()
                    .map(|x| {
                        let (style, class_ids) = self.match_scopes(&x.scopes);
                        (x.span, style, class_ids)
                    })
                    .collect();

            // first merge all ws by prepending to the next non-ws token
            if options.merge_whitespaces {
                let num_tokens = line_result.len();
                let mut merged: Vec<(Range<usize>, ThemeVariant<Style>, Vec<ClassId>)> =
                    Vec::with_capacity(num_tokens);
                let mut carry_on_range: Option<Range<usize>> = None;

                for (idx, (span, theme_style, class_ids)) in line_result.into_iter().enumerate() {
                    let could_merge = !theme_style.has_decoration();
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
                                merged.push((carried_range.start..span.end, theme_style, class_ids))
                            } else {
                                // We need to push 2 tokens here, one for the carried WS and one
                                // for the current token
                                let ws_style = if self.themes.len() == 1 {
                                    ThemeVariant::Single(Style::default())
                                } else {
                                    ThemeVariant::Dual {
                                        light: Style::default(),
                                        dark: Style::default(),
                                    }
                                };
                                merged.push((carried_range.clone(), ws_style, Vec::new()));
                                merged.push((span, theme_style, class_ids));
                            }
                            carry_on_range = None;
                        } else {
                            merged.push((span, theme_style, class_ids));
                        }
                    }
                }

                line_result = merged;
            }

            // then merge same style tokens after we did the WS
            if options.merge_same_style_tokens && self.themes.len() == 1 {
                let num_tokens = line_result.len();
                let mut merged: Vec<(Range<usize>, ThemeVariant<Style>, Vec<ClassId>)> =
                    Vec::with_capacity(num_tokens);

                for (span, theme_style, class_ids) in line_result {
                    if let Some((prev_span, prev_theme_style, prev_class_ids)) = merged.last_mut() {
                        if &theme_style == prev_theme_style {
                            prev_span.end = span.end;
                            // Merge class IDs, avoiding duplicates.
                            for class_id in class_ids {
                                if !prev_class_ids.contains(&class_id) {
                                    prev_class_ids.push(class_id);
                                }
                            }
                        } else {
                            merged.push((span, theme_style, class_ids));
                        }
                    } else {
                        merged.push((span, theme_style, class_ids));
                    }
                }

                line_result = merged;
            }

            // then transform into HighlightedText
            result.push(
                line_result
                    .into_iter()
                    .map(|(span, style, class_ids)| HighlightedText {
                        style,
                        text: line[span].to_string(),
                        class_ids,
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
    use crate::themes::compiled::{CompiledThemeRule, StyleModifier, ThemeType};
    use crate::themes::css::RuleClassIds;
    use crate::themes::font_style::FontStyle;
    use crate::themes::raw::{Colors, TokenColorRule, TokenColorSettings};
    use crate::themes::selector::parse_selector;
    use crate::themes::{Color, CompiledTheme, RawTheme};
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
        let rules = vec![
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
        ];
        let class_ids = rules.iter().map(RuleClassIds::from).collect();

        CompiledTheme {
            name: "Test".to_string(),
            theme_type: ThemeType::Dark,
            default_style: Style {
                foreground: color("#D4D4D4"),
                background: color("#1E1E1E"),
                font_style: FontStyle::default(),
            },
            line_number_foreground: None,
            highlight_background_color: None,
            rules,
            class_ids,
        }
    }

    #[test]
    fn test_match_scopes() {
        let test_theme = test_theme();
        let mut highlighter = Highlighter::new(&test_theme);

        // Test matching scopes
        let (ThemeVariant::Single(comment_style), comment_scopes) =
            highlighter.match_scopes(&[scope("comment")])
        else {
            unreachable!()
        };
        assert_eq!(comment_style.foreground, color("#6A9955"));
        assert_eq!(comment_style.font_style, FontStyle::ITALIC);
        assert!(!comment_scopes.is_empty()); // Should have contributing classes

        let (ThemeVariant::Single(keyword_style), keyword_scopes) =
            highlighter.match_scopes(&[scope("keyword")])
        else {
            unreachable!()
        };
        assert_eq!(keyword_style.foreground, color("#569CD6"));
        assert_eq!(keyword_style.font_style, FontStyle::BOLD);
        assert!(!keyword_scopes.is_empty()); // Should have contributing classes

        // Test unmatched scope returns default with empty classes
        let (unknown_style, unknown_scopes) = highlighter.match_scopes(&[scope("unknown")]);
        assert_eq!(
            unknown_style,
            ThemeVariant::Single(highlighter.themes[0].default_style)
        );
        assert!(unknown_scopes.is_empty()); // No contributing classes for unknown
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
        let ThemeVariant::Single(s) = &highlighted[0][0].style else {
            unreachable!()
        };
        assert_eq!(s.foreground, color("#569CD6"));

        // Unknown token uses default
        assert_eq!(highlighted[0][1].text, "hello");
        let ThemeVariant::Single(s) = &highlighted[0][1].style else {
            unreachable!()
        };
        assert_eq!(s.foreground, color("#D4D4D4"));

        // Comment token
        assert_eq!(highlighted[1][0].text, "//");
        let ThemeVariant::Single(s) = &highlighted[1][0].style else {
            unreachable!()
        };
        assert_eq!(s.foreground, color("#6A9955"));
    }

    #[test]
    fn test_style_modifier_apply_to() {
        let base = Style {
            foreground: color("#FFFFFF"),
            background: color("#000000"),
            font_style: FontStyle::default(),
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
                line_number_foreground: None,
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
        let (ThemeVariant::Single(style), _scopes) = highlighter.match_scopes(&[scope("constant")])
        else {
            unreachable!()
        };
        assert_eq!(style.foreground, color("#300000"));
        assert_eq!(style.font_style, FontStyle::ITALIC);

        // Test: constant.numeric should inherit fontStyle from constant but override foreground
        let (ThemeVariant::Single(style), _scopes) =
            highlighter.match_scopes(&[scope("constant"), scope("constant.numeric")])
        else {
            unreachable!()
        };
        assert_eq!(style.foreground, color("#400000")); // Overridden
        assert_eq!(style.font_style, FontStyle::ITALIC);

        // Test: constant.numeric.hex should inherit foreground from constant.numeric but override fontStyle
        let (ThemeVariant::Single(style), _scopes) = highlighter.match_scopes(&[
            scope("constant"),
            scope("constant.numeric"),
            scope("constant.numeric.hex"),
        ]) else {
            unreachable!()
        };
        assert_eq!(style.foreground, color("#400000")); // Should inherit from constant.numeric
        assert_eq!(style.font_style, FontStyle::BOLD); // Overridden
    }

    #[test]
    fn test_class_ids_track_per_property_winners() {
        let parent_selector = parse_selector("source parent").unwrap();
        let child_selector = parse_selector("child").unwrap();
        let rules = vec![
            // Parent contributes font style but loses foreground.
            CompiledThemeRule {
                selector: parent_selector,
                style_modifier: StyleModifier {
                    foreground: Some(color("#00FF00")),
                    background: None,
                    font_style: Some(FontStyle::ITALIC),
                },
            },
            // Child wins foreground.
            CompiledThemeRule {
                selector: child_selector,
                style_modifier: StyleModifier {
                    foreground: Some(color("#0000FF")),
                    background: None,
                    font_style: None,
                },
            },
        ];
        let class_ids = rules.iter().map(RuleClassIds::from).collect();

        let theme = CompiledTheme {
            name: "Class IDs".to_string(),
            theme_type: ThemeType::Dark,
            default_style: Style {
                foreground: color("#D4D4D4"),
                background: color("#1E1E1E"),
                font_style: FontStyle::default(),
            },
            line_number_foreground: None,
            highlight_background_color: None,
            rules,
            class_ids,
        };

        let mut highlighter = Highlighter::new(&theme);
        let (ThemeVariant::Single(style), class_ids) =
            highlighter.match_scopes(&[scope("source"), scope("parent"), scope("child")])
        else {
            unreachable!()
        };

        assert_eq!(style.foreground, color("#0000FF"));
        assert_eq!(style.font_style, FontStyle::ITALIC);

        let child_fg = theme.class_ids[1].fg;
        let parent_ft = theme.class_ids[0].ft;
        let parent_fg = theme.class_ids[0].fg;

        assert!(class_ids.contains(&child_fg));
        assert!(class_ids.contains(&parent_ft));
        assert!(!class_ids.contains(&parent_fg));
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
        let (style1, _) = highlighter.match_scopes(&token1_scopes);

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
        let (style2, _) = highlighter.match_scopes(&token2_scopes);

        // Token 3: '>' - HTML tag end punctuation
        let token3_scopes = [
            scope("text.aspnetcorerazor"),
            scope("meta.element.structure.svg.html"),
            scope("meta.element.object.svg.foreignObject.html"),
            scope("meta.element.other.invalid.html"),
            scope("meta.tag.other.invalid.start.html"),
            scope("punctuation.definition.tag.end.html"),
        ];
        let (style3, _) = highlighter.match_scopes(&token3_scopes);

        // Verify that styles are not default (theme inheritance is working)
        assert_ne!(style1, ThemeVariant::Single(compiled_theme.default_style));
        assert_ne!(style2, ThemeVariant::Single(compiled_theme.default_style));
        assert_ne!(style3, ThemeVariant::Single(compiled_theme.default_style));

        // Token 2 should have distinct styling due to invalid.illegal scope
        // which typically gets error/warning colors in themes
        assert_ne!(style1, style2);
        assert_ne!(style2, style3);

        // Basic sanity checks - styles should have reasonable colors
        // (Not pure black/white which would indicate broken highlighting)
        let ThemeVariant::Single(s1) = &style1 else {
            unreachable!()
        };
        let ThemeVariant::Single(s2) = &style2 else {
            unreachable!()
        };
        let ThemeVariant::Single(s3) = &style3 else {
            unreachable!()
        };
        assert_ne!(s1.foreground, Color::BLACK);
        assert_ne!(s2.foreground, Color::BLACK);
        assert_ne!(s3.foreground, Color::BLACK);

        // Token 'p' should get pink color from invalid.illegal rule (#FDAEB7)
        let expected_pink = Color {
            r: 253,
            g: 174,
            b: 183,
            a: 255,
        };
        assert_eq!(
            s2.foreground, expected_pink,
            "Token 'p' should get pink color #FDAEB7 from invalid.illegal rule, got {:?}",
            s2.foreground
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
            style: ThemeVariant::Single(test_theme.default_style),
            class_ids: Vec::new(),
        };
        let res = ht.as_html(&ThemeVariant::Single(&test_theme), None);
        insta::assert_snapshot!(res, @"<span>hello</span>");
    }

    #[test]
    fn test_as_html_content_escape() {
        let test_theme = test_theme();
        let ht = HighlightedText {
            text: "<script></script>".to_string(),
            style: ThemeVariant::Single(test_theme.default_style),
            class_ids: Vec::new(),
        };
        let res = ht.as_html(&ThemeVariant::Single(&test_theme), None);
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
            style: ThemeVariant::Single(custom_style),
            class_ids: Vec::new(),
        };
        let res = ht.as_html(&ThemeVariant::Single(&test_theme), None);
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
            style: ThemeVariant::Single(custom_style),
            class_ids: Vec::new(),
        };
        let res = ht.as_html(&ThemeVariant::Single(&test_theme), None);
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
            style: ThemeVariant::Single(custom_style),
            class_ids: Vec::new(),
        };
        let res = ht.as_html(&ThemeVariant::Single(&test_theme), None);
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
            style: ThemeVariant::Single(custom_style),
            class_ids: Vec::new(),
        };
        let res = ht.as_html(&ThemeVariant::Single(&test_theme), None);
        insta::assert_snapshot!(res, @r#"<span style="color: #FFFF00;background-color: #FFFF00;font-style: italic;">hello</span>"#);
    }

    #[test]
    fn test_as_html_dual_both_default() {
        let light = test_theme();
        let dark = test_theme();
        let ht = HighlightedText {
            text: "hello".to_string(),
            style: ThemeVariant::Dual {
                light: light.default_style,
                dark: dark.default_style,
            },
            class_ids: Vec::new(),
        };
        let res = ht.as_html(
            &ThemeVariant::Dual {
                light: &light,
                dark: &dark,
            },
            None,
        );
        insta::assert_snapshot!(res, @"<span>hello</span>");
    }

    #[test]
    fn test_as_html_dual_fg_differs() {
        let light = test_theme();
        let dark = test_theme();
        let ht = HighlightedText {
            text: "hello".to_string(),
            style: ThemeVariant::Dual {
                light: Style {
                    foreground: color("#FF0000"),
                    ..light.default_style
                },
                dark: Style {
                    foreground: color("#00FF00"),
                    ..dark.default_style
                },
            },
            class_ids: Vec::new(),
        };
        let res = ht.as_html(
            &ThemeVariant::Dual {
                light: &light,
                dark: &dark,
            },
            None,
        );
        insta::assert_snapshot!(res, @r#"<span style="color: light-dark(#FF0000, #00FF00);">hello</span>"#);
    }

    #[test]
    fn test_as_html_dual_bg_differs() {
        let light = test_theme();
        let dark = test_theme();
        let ht = HighlightedText {
            text: "hello".to_string(),
            style: ThemeVariant::Dual {
                light: Style {
                    background: color("#FFFFFF"),
                    ..light.default_style
                },
                dark: Style {
                    background: color("#000000"),
                    ..dark.default_style
                },
            },
            class_ids: Vec::new(),
        };
        let res = ht.as_html(
            &ThemeVariant::Dual {
                light: &light,
                dark: &dark,
            },
            None,
        );
        insta::assert_snapshot!(res, @r#"<span style="background-color: light-dark(#FFFFFF, #000000);">hello</span>"#);
    }

    #[test]
    fn test_as_html_dual_both_differ() {
        let light = test_theme();
        let dark = test_theme();
        let ht = HighlightedText {
            text: "hello".to_string(),
            style: ThemeVariant::Dual {
                light: Style {
                    foreground: color("#FF0000"),
                    background: color("#FFFFFF"),
                    font_style: FontStyle::BOLD,
                },
                dark: Style {
                    foreground: color("#00FF00"),
                    background: color("#000000"),
                    font_style: FontStyle::BOLD,
                },
            },
            class_ids: Vec::new(),
        };
        let res = ht.as_html(
            &ThemeVariant::Dual {
                light: &light,
                dark: &dark,
            },
            None,
        );
        insta::assert_snapshot!(res, @r#"<span style="color: light-dark(#FF0000, #00FF00);background-color: light-dark(#FFFFFF, #000000);font-weight: bold;">hello</span>"#);
    }

    #[test]
    fn test_as_html_with_css_classes() {
        let test_theme = test_theme();
        let ht = HighlightedText {
            text: "hello".to_string(),
            style: ThemeVariant::Single(test_theme.default_style),
            class_ids: vec![
                ClassId {
                    property: crate::themes::css::Property::Fg,
                    hash: 0xdeadbeef,
                },
                ClassId {
                    property: crate::themes::css::Property::Ft,
                    hash: 0xcafebabe,
                },
            ],
        };
        let res = ht.as_html(&ThemeVariant::Single(&test_theme), Some("g-"));
        insta::assert_snapshot!(
            res,
            @r#"<span class="g-fg-00000000deadbeef g-ft-00000000cafebabe">hello</span>"#
        );
    }
}
