use std::sync::{Arc, OnceLock};

use crate::grammars::anchors::AnchorActive;
use crate::grammars::caches::RegexCache;
use crate::grammars::engine::CaptureSpans;
use crate::grammars::prefilter::Prefilter;
use crate::grammars::{GlobalRuleRef, engine};

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct RuleMatch {
    pub rule_ref: GlobalRuleRef,
    pub start: usize,
    pub end: usize,
    pub capture_pos: CaptureSpans,
}

#[derive(Debug)]
struct Remainder {
    set: Arc<engine::RegexSet>,
    indices: Vec<usize>,
}

impl Remainder {
    fn find_at(
        &self,
        text: &str,
        pos: usize,
        anchors: AnchorActive,
    ) -> Result<Option<engine::Match>, String> {
        if let Some(mut m) = self.set.search(text, pos, anchors)? {
            // We want the idx at the rule level, not just the regset
            m.pattern_idx = self.indices[m.pattern_idx];
            return Ok(Some(m));
        }
        Ok(None)
    }
}

#[derive(Default, Debug)]
struct Finder {
    patterns: Vec<String>,
    regexes: Vec<OnceLock<Arc<engine::Regex>>>,
    cache: Arc<RegexCache>,
    prefilter: Prefilter,
    remainder: Option<Remainder>,
}

impl Finder {
    pub fn find_at(
        &self,
        text: &str,
        pos: usize,
        anchors: AnchorActive,
    ) -> Result<Option<engine::Match>, String> {
        // Regset first so we can get a starting pos to stop the walk early
        let set_hit = match &self.remainder {
            Some(r) => r.find_at(text, pos, anchors)?,
            None => None,
        };

        if self.regexes.is_empty() {
            return Ok(set_hit);
        }

        let set_hit_start = set_hit.as_ref().map(|m| m.start);
        let bytes = text.as_bytes();
        let mut walk_hit = None;
        let mut p = pos;

        'walk: while p < bytes.len() {
            if let Some(set_start) = set_hit_start
                && p > set_start
            {
                break;
            }

            let b = bytes[p];
            if !text.is_char_boundary(p) || !self.prefilter.may_match_at(b) {
                p += 1;
                continue;
            }
            // If we are after the `pos`, we need to disable `\G` as it can't match anymore
            let attempt_anchors = if p == pos {
                anchors
            } else {
                anchors.without_g()
            };

            for idx in self.prefilter.candidates(b) {
                let regex =
                    self.regexes[idx].get_or_init(|| self.cache.get_regex(&self.patterns[idx]));
                let Some((start, end, capture_pos)) =
                    regex.anchored_search(text, p, attempt_anchors)
                else {
                    continue;
                };

                walk_hit = Some(engine::Match {
                    pattern_idx: idx,
                    start,
                    end,
                    capture_pos,
                });
                break 'walk;
            }

            p += 1;
        }

        match (set_hit, walk_hit) {
            (None, None) => Ok(None),
            (Some(m), None) => Ok(Some(m)),
            (None, Some(m)) => Ok(Some(m)),
            (Some(set_m), Some(walk_m)) => {
                // Earliest win, otherwise by the idx in the list of patterns
                if set_m.start < walk_m.start
                    || (set_m.start == walk_m.start && set_m.pattern_idx < walk_m.pattern_idx)
                {
                    Ok(Some(set_m))
                } else {
                    Ok(Some(walk_m))
                }
            }
        }
    }
}

#[derive(Default, Debug)]
pub struct RuleMatcher {
    rule_refs: Vec<GlobalRuleRef>,
    finder: Finder,
}

impl RuleMatcher {
    pub fn new(items: Vec<(GlobalRuleRef, String)>, cache: Arc<RegexCache>) -> Self {
        if items.is_empty() {
            return RuleMatcher::default();
        }

        let (rule_refs, patterns): (Vec<_>, Vec<_>) = items.into_iter().unzip();
        let sets: Vec<_> = patterns.iter().map(|p| cache.get_first_bytes(p)).collect();

        let (prefilter, remaining) =
            if let Some((prefilter, remaining)) = Prefilter::from_byte_sets(&sets) {
                (prefilter, remaining)
            } else {
                (Prefilter::default(), (0..patterns.len()).collect())
            };

        let has_walk = remaining.len() < patterns.len();
        let remainder = (!remaining.is_empty()).then(|| {
            let sub: Vec<String> = remaining.iter().map(|&i| patterns[i].clone()).collect();
            Remainder {
                set: cache.get_set(&sub),
                indices: remaining,
            }
        });
        let regexes = if has_walk {
            (0..patterns.len()).map(|_| OnceLock::new()).collect()
        } else {
            Vec::new()
        };

        Self {
            rule_refs,
            finder: Finder {
                patterns,
                regexes,
                cache,
                prefilter,
                remainder,
            },
        }
    }

    pub fn find_at(
        &self,
        text: &str,
        pos: usize,
        anchors: AnchorActive,
    ) -> Result<Option<RuleMatch>, String> {
        let hit = self.finder.find_at(text, pos, anchors)?;

        Ok(hit.map(|m| RuleMatch {
            rule_ref: self.rule_refs[m.pattern_idx],
            start: m.start,
            end: m.end,
            capture_pos: m.capture_pos,
        }))
    }
}
