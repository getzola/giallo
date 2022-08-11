use std::ops::RangeInclusive;

/// The different ways we can highlight code
#[derive(Default, Copy, Clone, Eq, PartialEq, Debug)]
pub enum HighlightStyle {
    /// Prefix the scope with the given string. Useful for avoiding name collisions
    Classes(&'static str),
    /// Sets the color directly in style attributes
    Inline,
    /// Does nothing. We might want that if we only care about line numbers for example.
    #[default]
    None,
}

/// The options we can set for highlighting a code snippet
#[derive(Default, Clone, Eq, PartialEq, Debug)]
pub struct Options {
    highlight_style: HighlightStyle,
    /// Whether to show line numbers in the HTML output
    line_numbers: bool,
    /// At which line to start the snippet at, not used if `line_numbers` is `false`
    line_number_start: usize,
    /// Which lines to highlight. 1-indexed.
    highlight_lines: Vec<RangeInclusive<usize>>,
    /// Which lines to hide. 1-indexed.
    hide_lines: Vec<RangeInclusive<usize>>,
}
