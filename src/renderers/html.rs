use crate::registry::HighlightedCode;
use crate::renderers::Options;
use crate::themes::{Color, ThemeVariant};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, PartialEq, Clone, Default)]
/// A renderer that will output proper HTML code
pub struct HtmlRenderer {
    /// Any metadata we want to add as `<code>` data-* attribute
    pub other_metadata: BTreeMap<String, String>,
}

impl HtmlRenderer {
    /// Renders the given highlighted code to an HTML string.
    /// This will also handle automatic light/dark theming and escaping characters.
    pub fn render(&self, highlighted: &HighlightedCode, options: &Options) -> String {
        let lang = highlighted.language;

        // Pre-compute highlight background CSS if available
        let highlight_bg_css = if !options.highlight_lines.is_empty() {
            match &highlighted.theme {
                ThemeVariant::Single(theme) => theme
                    .highlight_background_color
                    .as_ref()
                    .map(|c| c.as_css_bg_color_property()),
                ThemeVariant::Dual { light, dark } => {
                    match (
                        &light.highlight_background_color,
                        &dark.highlight_background_color,
                    ) {
                        (Some(l), Some(d)) => {
                            Some(Color::as_css_light_dark_bg_color_property(l, d))
                        }
                        _ => None,
                    }
                }
            }
        } else {
            None
        };

        let mut lines = Vec::with_capacity(highlighted.tokens.len() + 4);
        for (idx, line_tokens) in highlighted.tokens.iter().enumerate() {
            let line_num = idx + 1; // 1-indexed

            // Skip hidden lines
            if options.hide_lines.iter().any(|r| r.contains(&line_num)) {
                continue;
            }

            // Render tokens
            let mut line_content = Vec::with_capacity(line_tokens.len());
            for tok in line_tokens {
                line_content.push(tok.as_html(&highlighted.theme));
            }
            let line_content = line_content.join("");

            // Line number (uses original source line number)
            let display_line_num = options.line_number_start + (idx as isize);
            let line_number_html = if options.show_line_numbers {
                format!(r#"<span class="giallo-ln">{display_line_num}</span>"#)
            } else {
                String::new()
            };

            // Build line span, with highlight if applicable
            let is_highlighted = options
                .highlight_lines
                .iter()
                .any(|r| r.contains(&line_num));
            let line_html = match (is_highlighted, &highlight_bg_css) {
                (true, Some(bg_css)) => {
                    format!(
                        r#"<span class="giallo-l" style="{bg_css}">{line_number_html}{line_content}</span>"#
                    )
                }
                _ => format!(r#"<span class="giallo-l">{line_number_html}{line_content}</span>"#),
            };

            lines.push(line_html);
        }
        let lines = lines.join("");

        // Build data attributes from other_metadata
        let mut data_attrs = format!(r#"data-lang="{lang}""#);
        for (key, value) in &self.other_metadata {
            // lowercase and replace non-alphanumeric chars with hyphens
            let slugified_key: String = key
                .to_lowercase()
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == '-' {
                        c
                    } else {
                        '-'
                    }
                })
                .collect();
            data_attrs.push_str(&format!(r#" data-{slugified_key}="{value}""#));
        }

        match &highlighted.theme {
            ThemeVariant::Single(theme) => {
                let fg = theme.default_style.foreground.as_css_color_property();
                let bg = theme.default_style.background.as_css_bg_color_property();
                format!(
                    r#"<pre class="giallo" style="{fg} {bg}"><code {data_attrs}>{lines}</code></pre>"#
                )
            }
            ThemeVariant::Dual { light, dark } => {
                let fg = Color::as_css_light_dark_color_property(
                    &light.default_style.foreground,
                    &dark.default_style.foreground,
                );
                let bg = Color::as_css_light_dark_bg_color_property(
                    &light.default_style.background,
                    &dark.default_style.background,
                );
                format!(
                    r#"<pre class="giallo" style="color-scheme: light dark; {fg} {bg}"><code {data_attrs}>{lines}</code></pre>"#
                )
            }
        }
    }
}

// From syntect
pub(crate) struct HtmlEscaped<'a>(pub &'a str);
impl fmt::Display for HtmlEscaped<'_> {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Because the internet is always right, turns out there's not that many
        // characters to escape: http://stackoverflow.com/questions/7381974
        let Self(s) = *self;
        let pile_o_bits = s;
        let mut last = 0;
        for (i, ch) in s.bytes().enumerate() {
            match ch as char {
                '<' | '>' | '&' | '\'' | '"' => {
                    fmt.write_str(&pile_o_bits[last..i])?;
                    let s = match ch as char {
                        '>' => "&gt;",
                        '<' => "&lt;",
                        '&' => "&amp;",
                        '\'' => "&#39;",
                        '"' => "&quot;",
                        _ => unreachable!(),
                    };
                    fmt.write_str(s)?;
                    last = i + 1;
                }
                _ => {}
            }
        }

        if last < s.len() {
            fmt.write_str(&pile_o_bits[last..])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{HighlightOptions, Registry};
    use std::fs;

    fn get_registry() -> Registry {
        let mut registry = Registry::default();
        for entry in fs::read_dir("grammars-themes/packages/tm-grammars/grammars").unwrap() {
            let path = entry.unwrap().path();
            registry.add_grammar_from_path(path).unwrap();
        }
        registry.link_grammars();
        registry
            .add_theme_from_path(
                "vitesse-black",
                "grammars-themes/packages/tm-themes/themes/vitesse-black.json",
            )
            .unwrap();
        registry
    }

    #[test]
    fn test_highlight_and_hide_lines() {
        let registry = get_registry();
        let code = "let a = 1;\nlet b = 2;\nlet c = 3;\nlet d = 4;\nlet e = 5;";
        let options = HighlightOptions::new("javascript").single_theme("vitesse-black");
        let highlighted = registry.highlight(code, options).unwrap();

        let render_options = Options {
            show_line_numbers: true,
            line_number_start: 10,
            highlight_lines: vec![2..=2, 4..=4],
            hide_lines: vec![3..=3],
        };

        let mut other_metadata = BTreeMap::new();
        other_metadata.insert("copy".to_string(), "true".to_string());
        other_metadata.insert("name".to_string(), "Hello world".to_string());
        other_metadata.insert("name with space1".to_string(), "other".to_string());

        let html = HtmlRenderer { other_metadata }.render(&highlighted, &render_options);
        insta::assert_snapshot!(html);
    }
}
