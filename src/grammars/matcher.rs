use crate::grammars::anchors::AnchorActive;
use crate::grammars::caches::RegexCache;
use crate::grammars::engine::CaptureSpans;
use crate::grammars::prefilter::Prefilter;
use crate::grammars::{GlobalRuleRef, engine};
use crate::tokenizer::last_match::LastMatchCache;
use fancy_regex::ByteSet;
use std::sync::{Arc, OnceLock};

/// How will giallo find the next matching pattern
#[derive(Debug, PartialEq, Eq, Copy, Clone, Default)]
pub enum MatchStrategy {
    /// giallo will check every pattern individually when it can and fallback to a RegexSet when it cannot
    ///
    /// This is fast when giallo is cold and uses less memory.
    /// This should be used by CLIs highlighting a few things and exiting or if you are memory constrained.
    #[default]
    Walk,
    /// giallo will only use a RegexSet
    ///
    /// Compared to [MatchStrategy::Walk]:
    /// 1. Initial RegexSet compilation is _very_ slow: first highlight will likely be around 10x slower
    /// 2. It will use *much* more memory  (2-10x more) growing with the number of languages that have highlighted
    /// 3. Warm highlights will be between 15-60% faster, dependent on language
    ///
    /// This should only be used if you're using giallo in a long-lived process and you're okay with
    /// potentially 10GB+ of ram used just for giallo with a few dozen languages.
    Set,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct RuleMatch {
    pub rule_ref: GlobalRuleRef,
    pub start: usize,
    pub end: usize,
    pub capture_pos: CaptureSpans,
}

/// Whatever couldn't be prefiltered.
/// We do not want a one regex regexset, that'd be dumb and slow so we special case it.
/// It does happen in a few grammars.
#[derive(Debug)]
enum Remainder {
    Single {
        re: Arc<engine::Regex>,
        index: usize,
    },
    Set {
        set: Arc<engine::RegexSet>,
        indices: Vec<usize>,
    },
}

impl Remainder {
    fn find_at(
        &self,
        text: &str,
        pos: usize,
        anchors: AnchorActive,
        last_match_cache: &mut LastMatchCache,
    ) -> Option<engine::Match> {
        // We want the idx at the rule level, not the one inside the regex/regset
        match self {
            Remainder::Single { re, index } => last_match_cache
                .search(
                    Arc::as_ptr(re) as usize,
                    pos,
                    re.anchor_usage(),
                    anchors,
                    || re.search(text, pos, anchors),
                )
                .map(|mut m| {
                    m.pattern_idx = *index;
                    m
                }),
            Remainder::Set { set, indices } => last_match_cache
                .search(
                    Arc::as_ptr(set) as usize,
                    pos,
                    set.anchor_usage(),
                    anchors,
                    || set.search(text, pos, anchors),
                )
                .map(|mut m| {
                    m.pattern_idx = indices[m.pattern_idx];
                    m
                }),
        }
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
        end_start: Option<usize>,
        last_match_cache: &mut LastMatchCache,
    ) -> Option<engine::Match> {
        // Regset first so we can get a starting pos to stop the walk early
        let set_hit = self
            .remainder
            .as_ref()
            .and_then(|r| r.find_at(text, pos, anchors, last_match_cache))
            .filter(|r| end_start.is_none_or(|x| r.start < x));

        if self.regexes.is_empty() {
            return set_hit;
        }

        let set_hit_start = set_hit.as_ref().map(|m| m.start);
        let bytes = text.as_bytes();
        let mut walk_hit = None;
        let mut p = pos;
        let walk_end = end_start.unwrap_or(bytes.len()).min(bytes.len());

        'walk: while p < walk_end {
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
                let Some(mut m) = regex.anchored_search(text, p, attempt_anchors) else {
                    continue;
                };

                m.pattern_idx = idx;
                walk_hit = Some(m);
                break 'walk;
            }

            p += 1;
        }

        match (set_hit, walk_hit) {
            (None, None) => None,
            (Some(m), None) => Some(m),
            (None, Some(m)) => Some(m),
            (Some(set_m), Some(walk_m)) => {
                // Earliest win, otherwise by the idx in the list of patterns
                if set_m.start < walk_m.start
                    || (set_m.start == walk_m.start && set_m.pattern_idx < walk_m.pattern_idx)
                {
                    Some(set_m)
                } else {
                    Some(walk_m)
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
    pub fn new(
        items: Vec<(GlobalRuleRef, String, Option<ByteSet>)>,
        cache: Arc<RegexCache>,
        strategy: MatchStrategy,
    ) -> Self {
        if items.is_empty() {
            return RuleMatcher::default();
        }

        let mut rule_refs = Vec::with_capacity(items.len());
        let mut patterns = Vec::with_capacity(items.len());
        let mut byte_sets = Vec::with_capacity(items.len());
        for (rule_ref, pattern, byte_set) in items {
            rule_refs.push(rule_ref);
            patterns.push(pattern);
            byte_sets.push(byte_set);
        }

        let (prefilter, remaining) = if strategy == MatchStrategy::Walk {
            if let Some((prefilter, remaining)) = Prefilter::from_byte_sets(&byte_sets) {
                (prefilter, remaining)
            } else {
                (Prefilter::default(), (0..patterns.len()).collect())
            }
        } else {
            (Prefilter::default(), (0..patterns.len()).collect())
        };

        let has_walk = remaining.len() < patterns.len();
        let remainder = (!remaining.is_empty()).then(|| {
            let sub: Vec<String> = remaining.iter().map(|&i| patterns[i].clone()).collect();
            if sub.len() > 1 {
                Remainder::Set {
                    set: cache.get_set(&sub),
                    indices: remaining,
                }
            } else {
                Remainder::Single {
                    re: cache.get_regex(&sub[0]),
                    index: remaining[0],
                }
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
        end_start: Option<usize>,
        last_match_cache: &mut LastMatchCache,
    ) -> Option<RuleMatch> {
        self.finder
            .find_at(text, pos, anchors, end_start, last_match_cache)
            .map(|m| RuleMatch {
                rule_ref: self.rule_refs[m.pattern_idx],
                start: m.start,
                end: m.end,
                capture_pos: m.capture_pos,
            })
    }
}
