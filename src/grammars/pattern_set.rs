use onig::RegSet;
use std::cell::RefCell;

use crate::grammars::{END_RULE_ID, RuleId, WHILE_RULE_ID};
use crate::tokenizer::TokenizeError;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct PatternSetMatch {
    pub rule_id: RuleId,
    pub start: usize,
    pub end: usize,
    pub capture_pos: Vec<Option<(usize, usize)>>,
}

impl PatternSetMatch {
    pub fn is_end_rule(&self) -> bool {
        self.rule_id == END_RULE_ID
    }

    pub fn is_while_rule(&self) -> bool {
        self.rule_id == WHILE_RULE_ID
    }

    pub fn has_advanced(&self) -> bool {
        self.end > self.start
    }
}

/// A compiled pattern set for efficient batch regex matching using onig RegSet
///
/// RegSet compilation is lazy - it's only created when first needed.
#[derive(Debug)]
pub struct PatternSet {
    rule_ids: Vec<RuleId>,
    patterns: Vec<String>,
    regset: RefCell<Option<RegSet>>,
}

impl PartialEq for PatternSet {
    fn eq(&self, other: &Self) -> bool {
        self.patterns == other.patterns && self.rule_ids == other.rule_ids
    }
}

impl Eq for PatternSet {}

impl PatternSet {
    pub fn new(items: Vec<(RuleId, String)>) -> Self {
        let (rule_ids, patterns): (Vec<_>, Vec<_>) = items.into_iter().unzip();

        Self {
            rule_ids,
            patterns,
            regset: RefCell::new(None),
        }
    }

    pub fn push_back(&mut self, rule_id: RuleId, pat: &str) {
        self.rule_ids.push(rule_id);
        self.patterns.push(pat.to_string());
        self.clear_regset();
    }

    pub fn push_front(&mut self, rule_id: RuleId, pat: &str) {
        self.rule_ids.insert(0, rule_id);
        self.patterns.insert(0, pat.to_string());
        self.clear_regset();
    }

    /// Updates the pattern at the front.
    /// Returns true if the pattern was different and the regset invalidated
    pub fn update_pat_front(&mut self, pat: &str) -> bool {
        debug_assert!(self.patterns.len() > 0);
        if &self.patterns[0] == pat {
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
        debug_assert!(self.patterns.len() > 0);
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
            let regset = RegSet::new(&pattern_strs).map_err(|e| {
                eprintln!(
                    "ERROR: RegSet compilation failed with {} patterns",
                    pattern_strs.len()
                );
                eprintln!("ERROR: Onig error: {:?}", e);
                for (i, pattern) in pattern_strs.iter().enumerate() {
                    eprintln!("ERROR: Pattern {}: {:?}", i, pattern);
                }
                TokenizeError::InvalidRegex(format!(
                    "Failed to compile pattern set with {} patterns: {:?}",
                    pattern_strs.len(),
                    e
                ))
            })?;
            *self.regset.borrow_mut() = Some(regset);
        }
        // println!("Searching {pos}: |{}|", &text[pos..]);
        // println!("Patterns:");
        // for p in &self.patterns {
        //     println!("  - {}", p);
        // }

        let regset_ref = self.regset.borrow();
        let regset = regset_ref.as_ref().unwrap();

        // We need to specify pos/text.len() because some regex might do lookbehind
        if let Some((pattern_index, captures)) = regset.captures_with_encoding(
            text,       // Full text (not sliced)
            pos,        // Start searching from this position
            text.len(), // Search to end of text
            onig::RegSetLead::Position,
            onig::SearchOptions::SEARCH_OPTION_NONE,
        ) {
            if let Some((match_start, match_end)) = captures.pos(0) {
                // Convert all capture positions (they're already absolute from captures_with_encoding)
                let capture_pos: Vec<Option<(usize, usize)>> =
                    (0..captures.len()).map(|i| captures.pos(i)).collect();

                return Ok(Some(PatternSetMatch {
                    rule_id: self.rule_ids[pattern_index],
                    start: match_start,
                    end: match_end,
                    capture_pos,
                }));
            }
        }

        Ok(None)
    }

    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

    pub fn rule_ids(&self) -> &[RuleId] {
        &self.rule_ids
    }
}
