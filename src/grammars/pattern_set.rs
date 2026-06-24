use std::sync::Arc;

use fancy_regex::RegexSet;

use crate::caches::RegexCache;
use crate::grammars::GlobalRuleRef;
use crate::tokenizer::anchors::AnchorActive;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct PatternSetMatch {
    pub rule_ref: GlobalRuleRef,
    pub start: usize,
    pub end: usize,
    pub capture_pos: Vec<Option<(usize, usize)>>,
}

#[derive(Default, Debug)]
pub struct PatternSet {
    rule_refs: Vec<GlobalRuleRef>,
    set: Option<RegexSet>,
}

impl PatternSet {
    pub(crate) fn new(items: Vec<(GlobalRuleRef, String)>, cache: Arc<RegexCache>) -> Self {
        if items.is_empty() {
            return PatternSet::default();
        }

        let (rule_refs, patterns): (Vec<_>, Vec<_>) = items.into_iter().unzip();
        let set = cache.get_set(&patterns);

        Self {
            rule_refs,
            set: Some(set.1),
        }
    }

    pub(crate) fn find_at(
        &self,
        text: &str,
        pos: usize,
        anchors: AnchorActive,
    ) -> Result<Option<PatternSetMatch>, String> {
        if let Some(set) = self.set.as_ref() {
            let re_input = crate::grammars::regex::make_input(text, pos, anchors);
            if let Some(matches) = set.find_input(re_input).map_err(|e| e.to_string())? {
                for res in matches {
                    if let Ok(m) = res {
                        let captures = m.captures();
                        let capture_pos: Vec<Option<(usize, usize)>> = (0..captures.len())
                            .map(|i| captures.get(i).map(|c| (c.start(), c.end())))
                            .collect();
                        return Ok(Some(PatternSetMatch {
                            rule_ref: self.rule_refs[m.pattern()],
                            start: m.start(),
                            end: m.end(),
                            capture_pos,
                        }));
                    }
                }
                Ok(None)
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }
}
