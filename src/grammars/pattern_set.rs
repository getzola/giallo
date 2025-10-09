use onig::RegSet;
use std::cell::RefCell;

use crate::grammars::{END_RULE_ID, RuleId, WHILE_RULE_ID};

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct PatternSetMatch {
    pub rule_id: RuleId,
    pub start: usize,
    pub end: usize,
    pub capture_pos: Vec<(usize, usize)>,
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

    pub fn update_pat_front(&mut self, pat: &str) {
        debug_assert!(self.patterns.len() > 0);
        if &self.patterns[0] == pat {
            return;
        }
        self.patterns.insert(0, pat.to_string());
        self.clear_regset();
    }

    pub fn update_pat_back(&mut self, pat: &str) {
        debug_assert!(self.patterns.len() > 0);
        if let Some(last) = self.patterns.last_mut() {
            if last.as_str() == pat {
                return;
            }
            *last = pat.to_string();
            self.clear_regset();
        }
    }

    pub fn clear_regset(&mut self) {
        self.regset.borrow_mut().take();
    }

    // TODO: return an error there if we can't build the regset
    pub fn find_at(&self, text: &str, pos: usize) -> Option<PatternSetMatch> {
        if self.patterns.is_empty() {
            return None;
        }

        if self.regset.borrow().is_none() {
            let pattern_strs: Vec<&str> = self.patterns.iter().map(|s| s.as_str()).collect();
            let regset = RegSet::new(&pattern_strs).expect("Failed to create RegSet");
            *self.regset.borrow_mut() = Some(regset);
        }

        let search_text = text.get(pos..)?;

        let regset_ref = self.regset.borrow();
        let regset = regset_ref.as_ref().unwrap();

        if let Some((pattern_index, captures)) = regset.captures(search_text) {
            // Only accept matches that start exactly at current position
            if let Some((match_start, match_end)) = captures.pos(0)
                && match_start == 0
            {
                let absolute_start = pos;
                let absolute_end = pos + match_end;

                let capture_pos: Vec<(usize, usize)> =
                    captures.iter_pos().filter_map(|i| i).collect();

                return Some(PatternSetMatch {
                    rule_id: self.rule_ids[pattern_index],
                    start: absolute_start,
                    end: absolute_end,
                    capture_pos,
                });
            }
        }

        None
    }

    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

    pub fn rule_ids(&self) -> &[RuleId] {
        &self.rule_ids
    }
}
