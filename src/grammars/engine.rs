use std::sync::Arc;

use fancy_regex::RegexSet as FancyRegexSet;
use fancy_regex::{Captures, Regex as FancyRegex, RegexInput, RegexOptionsBuilder};

use crate::grammars::anchors::AnchorActive;
use crate::grammars::pattern::AnchorUsage;

pub type CaptureSpans = Vec<Option<(usize, usize)>>;

pub(crate) fn fancy_options() -> RegexOptionsBuilder {
    let mut builder = RegexOptionsBuilder::new();
    builder
        .oniguruma_mode(true)
        .multi_line(true)
        .build_delegate_prefilter(false)
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

/// We treat runtime errors as not maching.
fn ignore_regex_error<T>(res: Result<T, fancy_regex::Error>) -> Option<T> {
    res.inspect_err(|_e| {
        #[cfg(feature = "debug")]
        log::warn!("Regex runtime error {_e}. Treating it as a no match");
    })
    .ok()
}

/// A match from a single regex or from a regex set.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Match {
    /// Always 0 for a single regex
    pub pattern_idx: usize,
    pub start: usize,
    pub end: usize,
    pub capture_pos: CaptureSpans,
}

/// Individual regexes: either dynamic end/while pattern or regexes we can avoid feeding into
/// a regex set
#[derive(Debug)]
pub struct Regex {
    inner: FancyRegex,
    anchor_usage: AnchorUsage,
}

impl Regex {
    pub fn new(pattern: &str) -> Self {
        let anchor_usage = AnchorUsage::from_pattern(pattern);
        Self {
            inner: compile_regex(pattern),
            anchor_usage,
        }
    }

    fn inner_search(
        &self,
        text: &str,
        pos: usize,
        anchors: AnchorActive,
        anchored: bool,
    ) -> Option<Match> {
        let mut input = make_input(text, pos, anchors);
        if anchored {
            input = input.anchored(true);
        }

        let captures = ignore_regex_error(self.inner.captures_input(input))??;
        let cap = captures.get(0)?;
        Some(Match {
            pattern_idx: 0,
            start: cap.start(),
            end: cap.end(),
            capture_pos: capture_spans(&captures),
        })
    }

    /// Unanchored search, used for dynamic end/while patterns only
    pub fn search(&self, text: &str, pos: usize, anchors: AnchorActive) -> Option<Match> {
        self.inner_search(text, pos, anchors, false)
    }

    /// Anchored search, used when walking the regexes of the prefiler
    pub fn anchored_search(&self, text: &str, pos: usize, anchors: AnchorActive) -> Option<Match> {
        self.inner_search(text, pos, anchors, true)
    }

    pub fn anchor_usage(&self) -> AnchorUsage {
        self.anchor_usage
    }

    pub fn regex(&self) -> &FancyRegex {
        &self.inner
    }
}

/// Contains all the regexes we couldn't handle via the prefilter
#[derive(Debug)]
pub struct RegexSet {
    inner: FancyRegexSet,
    anchor_usage: AnchorUsage,
}

impl RegexSet {
    pub fn from_regexes(regexes: &[Arc<Regex>]) -> Self {
        let mut inner = Vec::with_capacity(regexes.len());
        let mut anchor_usage = AnchorUsage::default();

        for r in regexes {
            inner.push(Arc::new(r.regex().clone()));
            anchor_usage = anchor_usage.union(r.anchor_usage());
        }

        let regset = FancyRegexSet::from_regexes(inner, Default::default())
            .unwrap_or_else(|e| unreachable!("fancy RegexSet build failed: {e}"));

        Self {
            inner: regset,
            anchor_usage,
        }
    }

    pub fn anchor_usage(&self) -> AnchorUsage {
        self.anchor_usage
    }

    pub fn search(&self, text: &str, pos: usize, anchors: AnchorActive) -> Option<Match> {
        let re_input = make_input(text, pos, anchors);
        let mut matches = ignore_regex_error(self.inner.find_input(re_input))??;
        let m = ignore_regex_error(matches.next()?)?;
        Some(Match {
            pattern_idx: m.pattern(),
            start: m.start(),
            end: m.end(),
            capture_pos: capture_spans(m.captures()),
        })
    }
}
