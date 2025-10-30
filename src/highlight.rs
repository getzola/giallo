use std::ops::Range;

use crate::themes::Style;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TokenStyle {
    pub range: Range<usize>,
    pub style: Style,
}

// struct Highlighter<'a> {
//
// }
