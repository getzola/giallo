use tree_sitter::LossyUtf8;
use tree_sitter_highlight::{util, Highlight, HighlightConfiguration, HighlightEvent, Highlighter};

use crate::languages::{SupportedLanguage, LANGUAGES};
use crate::options::HighlightStyle;
use crate::themes::Theme;

const BUFFER_HTML_RESERVE_CAPACITY: usize = 10 * 1024;

#[derive(Debug)]
pub struct HtmlRenderer<'t> {
    theme: &'t Theme,
    highlight_style: HighlightStyle,
    pub html: Vec<u8>,
    line_offsets: Vec<u32>,
}

impl<'t> HtmlRenderer<'t> {
    // TODO: pass a theme
    pub fn new(highlight_style: HighlightStyle, theme: &'t Theme) -> Self {
        let mut line_offsets = Vec::with_capacity(50);
        line_offsets.push(0);
        Self {
            theme,
            highlight_style,
            html: Vec::with_capacity(BUFFER_HTML_RESERVE_CAPACITY),
            line_offsets,
        }
    }

    fn start_highlight(&mut self, highlight: Highlight) {
        // TODO: pick class/css from the highlight
        //  pass a theme to the renderer + a class/css mode?
        self.html.extend(b"<span");
        self.html.extend_from_slice(format!(" style='color:{};'", self.theme.get_foreground(highlight.0)).as_bytes());
        // self.html
        //     .extend_from_slice(format!("class-{}", highlight.0).as_bytes());
        self.html.extend(b">");
    }

    fn add_text(&mut self, text: &[u8], highlights: &[Highlight]) {
        for c in LossyUtf8::new(text).flat_map(|p| p.bytes()) {
            if c == b'\n' {
                highlights.iter().for_each(|_| self.end_highlight());
                self.html.push(c);
                self.line_offsets.push(self.html.len() as u32);
                highlights.iter().for_each(|s| self.start_highlight(*s));
                continue;
            }

            if let Some(escaped) = util::html_escape(c) {
                self.html.extend_from_slice(escaped);
            } else {
                self.html.push(c);
            }
        }
    }

    fn end_highlight(&mut self) {
        self.html.extend(b"</span>");
    }

    pub fn render(&mut self, extension: &str, source: &[u8]) {
        let config = LANGUAGES
            .get(&SupportedLanguage::from_extension(extension).expect("fixme"))
            .expect("fixme2");
        let mut highlighter = Highlighter::new();
        let highlight_events = highlighter
            // TODO: figure out injection_callback
            .highlight(config, source, None, |_| None)
            .unwrap();

        let mut highlights = Vec::new();
        self.html.extend_from_slice(format!("<pre style='background-color:{};color:{};'><code>", self.theme.background, self.theme.foreground).as_bytes());

        for event in highlight_events {
            match event.expect("fixme3") {
                HighlightEvent::HighlightStart(s) => {
                    highlights.push(s);
                    self.start_highlight(s);
                }
                HighlightEvent::HighlightEnd => {
                    highlights.pop();
                    self.end_highlight();
                }
                HighlightEvent::Source { start, end } => {
                    self.add_text(&source[start..end], &highlights);
                }
            }
        }

        self.html.extend(b"</code></pre>");

        if self.html.last() != Some(&b'\n') {
            self.html.push(b'\n');
        }

        if self.line_offsets.last() == Some(&(self.html.len() as u32)) {
            self.line_offsets.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_render_rust() {
        let theme = Theme::new();
        let mut renderer = HtmlRenderer::new(HighlightStyle::None, &theme);
        renderer.render("rs", b"let mut renderer = HtmlRenderer::new();");
        assert_eq!(std::str::from_utf8(&renderer.html).unwrap(), "");
    }
}
