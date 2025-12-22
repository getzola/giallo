use crate::{HighlightedCode, RenderOptions, themes::compiled::ThemeType};

/// Terminal renderer via ANSI escape codes. Requires a terminal that supports truecolor
#[derive(Default, Copy, Clone, PartialEq, Eq)]
pub struct TerminalRenderer {
    /// The theme type to use if [`ThemeVariant::Dual`](crate::ThemeVariant::Dual) is provided, since terminals don't allow light or dark theme
    pub theme_type: ThemeType,
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

        // Color of line numbers
        let line_number_foreground = match highlighted.theme {
            crate::ThemeVariant::Single(theme) => theme.line_number_foreground,
            crate::ThemeVariant::Dual { light, .. } if self.theme_type == ThemeType::Light => {
                light.line_number_foreground
            }
            crate::ThemeVariant::Dual { dark, .. } if self.theme_type == ThemeType::Dark => {
                dark.line_number_foreground
            }
            _ => unreachable!(),
        };

        let line_count = highlighted.tokens.len();
        for (idx, line_tokens) in highlighted.tokens.iter().enumerate() {
            let line_num = idx + 1; // 1-indexed
            let is_last_line = line_count == line_num;

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
                if !is_last_line {
                    if let Some(line_number_foreground) = line_number_foreground {
                        output.push_str("\x1b[");
                        line_number_foreground.as_ansi_fg(&mut output);
                        output.push('m');
                    }
                    output.push_str(&format!("  {s} "));
                    if line_number_foreground.is_some() {
                        // reset
                        output.push_str("\x1b[0m");
                    }
                }
            }
            for token in line_tokens {
                token.as_ansi(&highlighted.theme, self.theme_type, &mut output)
            }

            // Don't add a newline after the last line to match bat's output
            let is_second_to_last_line = idx + 2 == line_count;

            if !is_last_line && !is_second_to_last_line {
                output.push('\n');
            }
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
