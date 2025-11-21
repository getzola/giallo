use std::ops::RangeInclusive;

pub mod html;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    show_line_numbers: bool,
    line_number_start: isize,
    highlight_lines: Vec<RangeInclusive<usize>>,
    hide_lines: Vec<RangeInclusive<usize>>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            show_line_numbers: false,
            line_number_start: 1,
            highlight_lines: Vec::new(),
            hide_lines: Vec::new(),
        }
    }
}
