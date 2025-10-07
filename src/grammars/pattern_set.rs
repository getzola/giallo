use std::cell::OnceCell;

use onig::RegSet;
use serde::{Deserialize, Serialize};

use crate::grammars::RuleId;

/// A compiled pattern set for efficient batch regex matching.
///
/// Uses onig RegSet internally for batch matching instead of testing patterns individually.
#[derive(Debug, Serialize, Deserialize)]
pub struct PatternSet {
    rule_ids: Vec<RuleId>,
    patterns: Vec<String>,
    /// Lazily compiled RegSet
    #[serde(skip)]
    regset: OnceCell<RegSet>,
}

// Manual Clone implementation since RegSet doesn't implement Clone
// We only need it to make the Rule structs clonable but we don't actually use it in the code
impl Clone for PatternSet {
    fn clone(&self) -> Self {
        Self {
            rule_ids: self.rule_ids.clone(),
            patterns: self.patterns.clone(),
            regset: OnceCell::new(), // Don't clone the regset, let it be recompiled
        }
    }
}

impl PartialEq for PatternSet {
    fn eq(&self, other: &Self) -> bool {
        // Compare based on patterns and rule_ids, ignore the cached regset
        self.patterns == other.patterns && self.rule_ids == other.rule_ids
    }
}

impl Eq for PatternSet {}

impl PatternSet {
    pub fn new(items: Vec<(RuleId, String)>) -> Self {
        let (rule_ids, patterns): (Vec<_>, Vec<_>) = items.into_iter().unzip();
        assert_eq!(patterns.len(), rule_ids.len());

        Self {
            rule_ids,
            patterns,
            regset: OnceCell::new(),
        }
    }

    pub fn find_at(&self, text: &str, pos: usize) -> Option<(usize, usize, RuleId, Vec<String>)> {
        if self.patterns.is_empty() {
            return None;
        }

        let regset = self.regset.get_or_init(|| {
            // Convert Vec<String> to Vec<&str> for RegSet::new
            let pattern_strs: Vec<&str> = self.patterns.iter().map(|s| s.as_str()).collect();
            RegSet::new(&pattern_strs).expect("Failed to create RegSet")
        });

        let search_text = text.get(pos..)?;

        if let Some((pattern_index, captures)) = regset.captures(search_text) {
            // Only accept matches that start exactly at current position
            if let Some((match_start, match_end)) = captures.pos(0)
                && match_start == 0
            {
                let absolute_start = pos;
                let absolute_end = pos + match_end;

                let capture_strings: Vec<String> = captures
                    .iter()
                    .filter_map(|i| i)
                    .map(|s| s.to_string())
                    .collect();

                return Some((
                    absolute_start,
                    absolute_end,
                    self.rule_ids[pattern_index],
                    capture_strings,
                ));
            }
        }

        None
    }

    /// Get the patterns in this set
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

    /// Get the rule IDs in this set
    pub fn rule_ids(&self) -> &[RuleId] {
        &self.rule_ids
    }
}
