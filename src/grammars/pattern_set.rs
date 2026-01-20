use std::fmt::{Debug, Formatter};

use onig::{RegSet, RegexOptions, SearchOptions};

use crate::grammars::GlobalRuleRef;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct PatternSetMatch {
    pub rule_ref: GlobalRuleRef,
    pub start: usize,
    pub end: usize,
    pub capture_pos: Vec<Option<(usize, usize)>>,
}

/// An eagerly compiled pattern set for efficient batch regex matching using onig RegSet
pub struct PatternSet {
    rule_refs: Vec<GlobalRuleRef>,
    regset: Option<RegSet>,
}

impl PatternSet {
    pub fn new(items: Vec<(GlobalRuleRef, String)>) -> Result<Self, String> {
        if items.is_empty() {
            return Ok(Self {
                rule_refs: Vec::new(),
                regset: None,
            });
        }

        let (rule_refs, patterns): (Vec<_>, Vec<_>) = items.into_iter().unzip();
        let pattern_strs: Vec<&str> = patterns.iter().map(|s| s.as_str()).collect();

        let regset = RegSet::with_options(&pattern_strs, RegexOptions::REGEX_OPTION_CAPTURE_GROUP)
            .map_err(|e| {
                format!(
                    "Failed to compile pattern set with {} patterns: {:?}",
                    pattern_strs.len(),
                    e
                )
            })?;

        Ok(Self {
            rule_refs,
            regset: Some(regset),
        })
    }

    pub(crate) fn find_at(
        &self,
        text: &str,
        pos: usize,
        options: SearchOptions,
    ) -> Result<Option<PatternSetMatch>, String> {
        let Some(regset) = &self.regset else {
            return Ok(None);
        };

        // We need to specify pos/text.len() because some regex might do lookbehind
        if let Some((pattern_index, captures)) = regset.captures_with_options(
            text,       // Full text (not sliced)
            pos,        // Start searching from this position
            text.len(), // Search to end of text
            onig::RegSetLead::Position,
            options,
        ) && let Some((match_start, match_end)) = captures.pos(0)
        {
            // Convert all capture positions (they're already absolute from captures_with_encoding)
            let capture_pos: Vec<Option<(usize, usize)>> =
                (0..captures.len()).map(|i| captures.pos(i)).collect();

            return Ok(Some(PatternSetMatch {
                rule_ref: self.rule_refs[pattern_index],
                start: match_start,
                end: match_end,
                capture_pos,
            }));
        }

        Ok(None)
    }
}

impl Debug for PatternSet {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "PatternSet({} rules)", self.rule_refs.len())
    }
}
