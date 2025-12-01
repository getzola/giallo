use std::cell::RefCell;
use std::fmt::{Debug, Formatter};

use onig::{RegSet, RegexOptions};

use crate::grammars::GlobalRuleRef;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct PatternSetMatch {
    pub rule_ref: GlobalRuleRef,
    pub start: usize,
    pub end: usize,
    pub capture_pos: Vec<Option<(usize, usize)>>,
}

/// A lazily compiled pattern set for efficient batch regex matching using onig RegSet
/// Sadly oniguruma wants to own the regex so it does recompile them everytime...
pub struct PatternSet {
    rule_refs: Vec<GlobalRuleRef>,
    patterns: Vec<String>,
    regset: RefCell<Option<RegSet>>,
}

impl PartialEq for PatternSet {
    fn eq(&self, other: &Self) -> bool {
        self.patterns == other.patterns && self.rule_refs == other.rule_refs
    }
}

impl Eq for PatternSet {}

impl PatternSet {
    pub fn new(items: Vec<(GlobalRuleRef, String)>) -> Self {
        let (rule_refs, patterns): (Vec<_>, Vec<_>) = items.into_iter().unzip();

        Self {
            rule_refs,
            patterns,
            regset: RefCell::new(None),
        }
    }

    pub fn update(&mut self, index: usize, pat: &str) -> bool {
        debug_assert!(!self.patterns.is_empty());
        if self.patterns[index] == pat {
            false
        } else {
            self.patterns[index] = pat.to_owned();
            if let Some(regset) = self.regset.borrow_mut().as_mut() {
                regset.replace_pattern(index, pat).expect("no errors");
            }
            true
        }
    }
    pub fn update_front(&mut self, pat: &str) -> bool {
        self.update(0, pat)
    }

    pub fn update_last(&mut self, pat: &str) -> bool {
        self.update(self.patterns.len() - 1, pat)
    }

    pub(crate) fn find_at(
        &self,
        text: &str,
        pos: usize,
    ) -> Result<Option<PatternSetMatch>, String> {
        if self.patterns.is_empty() {
            return Ok(None);
        }

        if self.regset.borrow().is_none() {
            let pattern_strs: Vec<&str> = self.patterns.iter().map(|s| s.as_str()).collect();

            let regset =
                RegSet::with_options(&pattern_strs, RegexOptions::REGEX_OPTION_CAPTURE_GROUP)
                    .map_err(|e| {
                        eprintln!(
                            "RegSet compilation failed for pattern set with {} patterns",
                            pattern_strs.len()
                        );
                        eprintln!("Onig error: {:?}", e);
                        eprintln!("Rule IDs and patterns in this set:");
                        for (i, (rule_ref, pattern)) in
                            self.rule_refs.iter().zip(self.patterns.iter()).enumerate()
                        {
                            eprintln!(
                                "  [{}] Rule ID {} of grammar {}: {:?}",
                                i,
                                rule_ref.rule.as_index(),
                                rule_ref.grammar.as_index(),
                                pattern
                            );
                        }
                        format!(
                            "Failed to compile pattern set with {} patterns: {:?}",
                            pattern_strs.len(),
                            e
                        )
                    })?;
            *self.regset.borrow_mut() = Some(regset);
        }
        let regset_ref = self.regset.borrow();
        let regset = regset_ref.as_ref().unwrap();

        // We need to specify pos/text.len() because some regex might do lookbehind
        if let Some((pattern_index, captures)) = regset.captures_with_options(
            text,       // Full text (not sliced)
            pos,        // Start searching from this position
            text.len(), // Search to end of text
            onig::RegSetLead::Position,
            onig::SearchOptions::SEARCH_OPTION_NONE,
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
        let all: Vec<_> = self
            .patterns
            .iter()
            .zip(self.rule_refs.iter())
            .map(|(pat, rule_ref)| format!("  - {:?}: {pat}", rule_ref.rule))
            .collect();
        write!(f, "{}", all.join("\n"))
    }
}
