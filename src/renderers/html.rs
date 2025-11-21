use crate::registry::HighlightedCode;
use crate::renderers::Options;
use std::collections::BTreeMap;

#[derive(Debug, PartialEq, Clone, Default)]
pub struct HtmlRenderer {
    pub css_class_prefix: Option<&'static str>,
    pub other_metadata: BTreeMap<String, String>,
}

impl HtmlRenderer {
    pub fn render(&self, highlighted: &HighlightedCode, options: &Options) -> String {
        self.render_internal(highlighted, options, None)
    }

    pub fn render_with_classes(
        &self,
        highlighted: &HighlightedCode,
        options: &Options,
        prefix: &'static str,
    ) -> String {
        self.render_internal(highlighted, options, Some(prefix))
    }

    fn render_internal(
        &self,
        highlighted: &HighlightedCode,
        _options: &Options,
        prefix: Option<&'static str>,
    ) -> String {
        let mut lines = Vec::with_capacity(highlighted.tokens.len() + 4);
        for line_tokens in &highlighted.tokens {
            let mut line = Vec::with_capacity(line_tokens.len());
            for tok in line_tokens {
                line.push(tok.as_html(prefix, &highlighted.theme.default_style));
            }
            lines.push(line.join(""));
        }

        let lines = lines.join("\n");
        let lang = highlighted.language;
        let fg = highlighted
            .theme
            .default_style
            .foreground
            .as_css_color_property();
        let bg = highlighted
            .theme
            .default_style
            .background
            .as_css_bg_color_property();

        format!(r#"<pre style="{fg} {bg}"><code data-lang="{lang}">{lines}</code></pre>"#)
    }
}
