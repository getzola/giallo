use crate::registry::HighlightedCode;
use crate::renderers::RenderOptions;
use crate::themes::{Color, ThemeVariant};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, PartialEq, Clone, Default)]
/// A renderer that will output proper HTML code
pub struct HtmlRenderer {
    /// Any metadata we want to add as `<code>` data-* attribute
    pub other_metadata: BTreeMap<String, String>,
    /// If set, output CSS classes instead of inline styles.
    /// The value is the class prefix (e.g., "g-" produces classes like "g-keyword").
    /// Generate corresponding CSS stylesheets using `Registry::generate_css`.
    pub css_class_prefix: Option<String>,
}

impl HtmlRenderer {
    /// Renders the given highlighted code to an HTML string.
    /// This will also handle automatic light/dark theming and escaping characters.
    pub fn render(&self, highlighted: &HighlightedCode, options: &RenderOptions) -> String {
        let lang = highlighted.language;
        let css_prefix = self.css_class_prefix.as_deref();

        // Pre-compute highlight background CSS/class if available
        let highlight_attr = if !options.highlight_lines.is_empty() {
            if let Some(prefix) = css_prefix {
                // CSS class mode: use hl class
                Some(format!(r#" class="{prefix}hl""#))
            } else {
                // Inline style mode
                match &highlighted.theme {
                    ThemeVariant::Single(theme) => theme
                        .highlight_background_color
                        .as_ref()
                        .map(|c| format!(r#" style="{}""#, c.as_css_bg_color_property())),
                    ThemeVariant::Dual { light, dark } => {
                        match (
                            &light.highlight_background_color,
                            &dark.highlight_background_color,
                        ) {
                            (Some(l), Some(d)) => Some(format!(
                                r#" style="{}""#,
                                Color::as_css_light_dark_bg_color_property(l, d)
                            )),
                            _ => None,
                        }
                    }
                }
            }
        } else {
            None
        };

        // Pre-compute line number color style if available (inline style mode only)
        let line_number_style = if options.show_line_numbers && css_prefix.is_none() {
            match &highlighted.theme {
                ThemeVariant::Single(theme) => theme
                    .line_number_foreground
                    .as_ref()
                    .map(|c| format!(r#" style="{}""#, c.as_css_color_property())),
                ThemeVariant::Dual { light, dark } => {
                    match (&light.line_number_foreground, &dark.line_number_foreground) {
                        (Some(l), Some(d)) => Some(format!(
                            r#" style="{}""#,
                            Color::as_css_light_dark_color_property(l, d)
                        )),
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
                line_content.push(tok.as_html(&highlighted.theme, css_prefix));
            }
            let line_content = line_content.join("");

            // Line number (uses original source line number)
            let display_line_num = options.line_number_start + (idx as isize);
            let line_number_html = if options.show_line_numbers {
                format!(
                    r#"<span class="giallo-ln"{}>{display_line_num}</span>"#,
                    line_number_style.as_deref().unwrap_or_default()
                )
            } else {
                String::new()
            };

            // Build line span, with highlight if applicable
            let is_highlighted = options
                .highlight_lines
                .iter()
                .any(|r| r.contains(&line_num));
            let line_html = match (is_highlighted, &highlight_attr) {
                (true, Some(hl_class_or_style)) => {
                    format!(
                        r#"<span class="giallo-l{hl_class_or_style}"{hl_style}>{line_number_html}{line_content}</span>"#,
                        hl_class_or_style = if let Some(p) = css_prefix {
                            format!(" {p}hl")
                        } else {
                            String::new()
                        },
                        hl_style = if css_prefix.is_none() {
                            hl_class_or_style
                        } else {
                            ""
                        }
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

        // CSS class mode: output class instead of inline styles on <pre>
        if let Some(prefix) = css_prefix {
            return format!(
                r#"<pre class="giallo {prefix}code"><code {data_attrs}>{lines}</code></pre>"#
            );
        }

        // Inline style mode
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
    use crate::registry::HighlightOptions;
    use crate::test_utils::get_registry;

    #[test]
    fn test_highlight_and_hide_lines() {
        let registry = get_registry();
        let code = "let a = 1;\n\nlet b = 2;\nlet c = 3;\nlet d = 4;\nlet e = 5;";
        let options = HighlightOptions::new("javascript", ThemeVariant::Single("vitesse-black"));
        let highlighted = registry.highlight(code, &options).unwrap();

        let render_options = RenderOptions {
            show_line_numbers: true,
            line_number_start: 10,
            highlight_lines: vec![3..=3, 5..=5],
            hide_lines: vec![4..=4],
        };

        let mut other_metadata = BTreeMap::new();
        other_metadata.insert("copy".to_string(), "true".to_string());
        other_metadata.insert("name".to_string(), "Hello world".to_string());
        other_metadata.insert("name with space1".to_string(), "other".to_string());

        let html = HtmlRenderer {
            other_metadata,
            css_class_prefix: None,
        }
        .render(&highlighted, &render_options);
        insta::assert_snapshot!(html);
    }
}
