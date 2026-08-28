use std::sync::Arc;

use fancy_regex::RegexSet as FancyRegexSet;
use fancy_regex::{Captures, Regex as FancyRegex, RegexInput, RegexOptionsBuilder};

use crate::grammars::anchors::AnchorActive;

pub type CaptureSpans = Vec<Option<(usize, usize)>>;

pub(crate) fn fancy_options() -> RegexOptionsBuilder {
    let mut builder = RegexOptionsBuilder::new();
    builder
        .oniguruma_mode(true)
        .multi_line(true)
        .allow_input_assertion_overrides(true);
    builder
}

/// There are a couple of (not important) regexes that fancy doesn't compile so we replace them
fn never_matching_regex() -> fancy_regex::Regex {
    fancy_options().build(r"[^\s\S]".to_string()).unwrap()
}

pub(crate) fn compile_regex(pattern: &str) -> fancy_regex::Regex {
    let builder = fancy_options();
    builder.build(pattern.to_string()).unwrap_or_else(|_| {
        // https://github.com/fancy-regex/fancy-regex/issues/162#issuecomment-4788029548
        never_matching_regex()
    })
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

pub(crate) fn capture_spans(captures: &Captures<'_, str>) -> CaptureSpans {
    (0..captures.len())
        .map(|i| captures.get(i).map(|m| (m.start(), m.end())))
        .collect()
}

/// Individual regexes: either dynamic end/while pattern or regexes we can avoid feeding into
/// a regex set
#[derive(Debug)]
pub struct Regex(FancyRegex);

impl Regex {
    pub fn new(pattern: &str) -> Self {
        Self(compile_regex(pattern))
    }

    fn inner_search(
        &self,
        text: &str,
        pos: usize,
        anchors: AnchorActive,
        anchored: bool,
    ) -> Option<(usize, usize, CaptureSpans)> {
        let mut input = make_input(text, pos, anchors);
        if anchored {
            input = input.anchored(true);
        }
        let captures = self.0.captures_input(input).ok().flatten()?;
        let (start, end) = captures.get(0).map(|m| (m.start(), m.end()))?;
        let capture_pos = capture_spans(&captures);
        Some((start, end, capture_pos))
    }

    /// Unanchored search, used for dynamic end/while patterns only
    pub fn search(
        &self,
        text: &str,
        pos: usize,
        anchors: AnchorActive,
    ) -> Option<(usize, usize, CaptureSpans)> {
        self.inner_search(text, pos, anchors, false)
    }

    /// Anchored search, used when walking the regexes of the prefiler
    pub fn anchored_search(
        &self,
        text: &str,
        pos: usize,
        anchors: AnchorActive,
    ) -> Option<(usize, usize, CaptureSpans)> {
        self.inner_search(text, pos, anchors, true)
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Match {
    pub pattern_idx: usize,
    pub start: usize,
    pub end: usize,
    pub capture_pos: CaptureSpans,
}

/// Contains all the regexes we couldn't handle via the prefilter
#[derive(Debug)]
pub struct RegexSet(FancyRegexSet);

impl RegexSet {
    pub fn new(patterns: &[String]) -> Self {
        let regexes: Vec<_> = patterns
            .iter()
            .map(|x| Arc::new(compile_regex(x)))
            .collect();
        let regset = FancyRegexSet::from_regexes(regexes, Default::default())
            .unwrap_or_else(|e| unreachable!("fancy RegexSet build failed: {e}"));
        Self(regset)
    }

    pub fn search(
        &self,
        text: &str,
        pos: usize,
        anchors: AnchorActive,
    ) -> Result<Option<Match>, String> {
        let re_input = make_input(text, pos, anchors);
        if let Some(matches) = self.0.find_input(re_input).map_err(|e| e.to_string())?
            && let Some(m) = matches.flatten().next()
        {
            return Ok(Some(Match {
                pattern_idx: m.pattern(),
                start: m.start(),
                end: m.end(),
                capture_pos: capture_spans(m.captures()),
            }));
        }
        Ok(None)
    }
}
