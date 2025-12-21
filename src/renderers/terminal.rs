use crate::{HighlightedCode, RenderOptions};
use std::fmt::Write;

/// Terminal renderer via ANSI escape codes
#[derive(Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TerminalRenderer {
    /// If [`ThemeVariant::Dual`](crate::ThemeVariant::Dual) is provided, uses `dark` theme if this
    /// is set to `true`, and `light` if it is set to `false`, since terminals
    /// don't allow light or dark theme
    pub use_dark_theme: bool,
}

impl TerminalRenderer {
    /// Render to the terminal with ANSI escape codes
    pub fn render(&self, highlighted: &HighlightedCode, options: &RenderOptions) -> String {
        let mut output = String::new();

        // We want to calculate how many characters to give the
        // line numbers, so all line numbers fit
        let line_numbers_size = if options.show_line_numbers {
            // First line might be larger than the last line if it is negative, e.g. -100..90
            let first_line = options.line_number_start.to_string().chars().count();
            let last_line = highlighted
                .tokens
                .len()
                .saturating_add_signed(options.line_number_start)
                .to_string()
                .chars()
                .count();
            first_line.max(last_line)
        } else {
            // Won't be used
            0
        };

        for (idx, line_tokens) in highlighted.tokens.iter().enumerate() {
            let line_num = idx + 1; // 1-indexed

            // Skip hidden lines
            if options.hide_lines.iter().any(|r| r.contains(&line_num)) {
                continue;
            }

            // Render tokens
            if options.show_line_numbers {
                let line_num = options.line_number_start + (idx as isize);
                let line_num_s = line_num.to_string();
                let s = std::iter::repeat_n(' ', line_numbers_size - line_num_s.chars().count())
                    .chain(line_num_s.chars())
                    .collect::<String>();
                write!(output, " {s} │ ").expect("writing to `String` is infallible");
            }
            for token in line_tokens {
                token
                    .as_ansi(&highlighted.theme, self.use_dark_theme, &mut output)
                    .expect("writing to `String` is infallible");
            }
            writeln!(output).expect("writing to `String` is infallible");
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ThemeVariant;
    use crate::registry::HighlightOptions;
    use crate::test_utils::get_registry;

    #[test]
    fn test_highlight_and_hide_lines() {
        let registry = get_registry();
        let code = "let a = 1;\nlet b = 2;\nlet c = 3;\nlet d = 4;\nlet e = 5;";
        let options = HighlightOptions::new("javascript", ThemeVariant::Single("vitesse-black"));
        let highlighted = registry.highlight(code, options).unwrap();

        let render_options = RenderOptions {
            show_line_numbers: true,
            line_number_start: 10,
            highlight_lines: vec![2..=2, 4..=4],
            hide_lines: vec![3..=3],
        };

        let ansi = TerminalRenderer::default().render(&highlighted, &render_options);
        insta::assert_snapshot!(ansi);
    }
}
