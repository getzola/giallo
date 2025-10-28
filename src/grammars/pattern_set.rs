use crate::grammars::{END_RULE_ID, GlobalRuleRef};
use crate::tokenizer::TokenizeError;
use onig::{RegSet, RegexOptions};
use std::cell::RefCell;
use std::fmt::{Debug, Formatter};

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct PatternSetMatch {
    pub rule_ref: GlobalRuleRef,
    pub start: usize,
    pub end: usize,
    pub capture_pos: Vec<Option<(usize, usize)>>,
}

impl PatternSetMatch {
    pub fn is_end_rule(&self) -> bool {
        self.rule_ref.rule == END_RULE_ID
    }

    pub fn has_advanced(&self) -> bool {
        self.end > self.start
    }
}

/// A compiled pattern set for efficient batch regex matching using onig RegSet
///
/// RegSet compilation is lazy - it's only created when first needed.
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

    pub fn push_back(&mut self, rule_ref: GlobalRuleRef, pat: &str) {
        self.rule_refs.push(rule_ref);
        self.patterns.push(pat.to_string());
        self.clear_regset();
    }

    pub fn push_front(&mut self, rule_ref: GlobalRuleRef, pat: &str) {
        self.rule_refs.insert(0, rule_ref);
        self.patterns.insert(0, pat.to_string());
        self.clear_regset();
    }

    /// Updates the pattern at the front.
    /// Returns true if the pattern was different and the regset invalidated
    pub fn update_pat_front(&mut self, pat: &str) -> bool {
        debug_assert!(!self.patterns.is_empty());
        if self.patterns[0] == pat {
            false
        } else {
            self.patterns[0] = pat.to_string();
            self.clear_regset();
            true
        }
    }

    /// Updates the pattern at the back.
    /// Returns true if the pattern was different and the regset invalidated
    pub fn update_pat_back(&mut self, pat: &str) -> bool {
        debug_assert!(!self.patterns.is_empty());
        if let Some(last) = self.patterns.last_mut() {
            if last.as_str() == pat {
                return false;
            }
            *last = pat.to_string();
            self.clear_regset();
            return true;
        }

        unreachable!()
    }

    pub fn clear_regset(&mut self) {
        self.regset.borrow_mut().take();
    }

    pub fn find_at(
        &self,
        text: &str,
        pos: usize,
    ) -> Result<Option<PatternSetMatch>, TokenizeError> {
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
                        TokenizeError::InvalidRegex(format!(
                            "Failed to compile pattern set with {} patterns: {:?}",
                            pattern_strs.len(),
                            e
                        ))
                    })?;
            *self.regset.borrow_mut() = Some(regset);
        }
        let regset_ref = self.regset.borrow();
        let regset = regset_ref.as_ref().unwrap();

        // We need to specify pos/text.len() because some regex might do lookbehind
        if let Some((pattern_index, captures)) = regset.captures_with_encoding(
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
