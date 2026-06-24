use fancy_regex::{Regex, RegexInput, RegexOptionsBuilder};

use crate::tokenizer::anchors::AnchorActive;

pub(crate) fn fancy_options() -> RegexOptionsBuilder {
    let mut builder = RegexOptionsBuilder::new();
    builder
        .oniguruma_mode(true)
        .multi_line(true)
        .allow_input_assertion_overrides(true);
    builder
}

pub(crate) fn compile_regex(pattern: &str) -> Regex {
    let builder = fancy_options();
    builder.build(pattern.to_string()).unwrap_or_else(|_| {
        // https://github.com/fancy-regex/fancy-regex/issues/162#issuecomment-4788029548
        never_matching_regex()
    })
}

fn never_matching_regex() -> Regex {
    fancy_options().build(r"[^\s\S]".to_string()).unwrap()
}

/// Sets up the \A \G flags in fancy-regex
pub(crate) fn make_input(text: &str, pos: usize, anchors: AnchorActive) -> RegexInput<'_, str> {
    let mut input = RegexInput::new(text).from_pos(pos);
    if !anchors.allow_a() {
        input = input.start_text(false);
    }
    if !anchors.allow_g() {
        input = input.continue_from_previous_match_end(false);
    }
    input
}

type CaptureSpans = Vec<Option<(usize, usize)>>;

/// Anchored search in a single regex.
/// Only used with dynamic end regexes
pub(crate) fn search(
    re: &Regex,
    text: &str,
    pos: usize,
    anchors: AnchorActive,
) -> Option<(usize, usize, CaptureSpans)> {
    let captures = re
        .captures_input(make_input(text, pos, anchors))
        .ok()
        .flatten()?;
    let (start, end) = captures.get(0).map(|m| (m.start(), m.end()))?;
    let capture_pos = (0..captures.len())
        .map(|i| captures.get(i).map(|m| (m.start(), m.end())))
        .collect();
    Some((start, end, capture_pos))
}
