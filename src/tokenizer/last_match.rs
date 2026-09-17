use std::collections::HashMap;

use crate::grammars::anchors::AnchorActive;
use crate::grammars::engine::Match;
use crate::grammars::pattern::AnchorUsage;

/// A cached result for a regex/regexset search
#[derive(Debug)]
struct Entry {
    /// Where the search started from
    pos: usize,
    result: Option<Match>,
}

/// A cache like the one vscode-oniguruma uses
/// (https://github.com/microsoft/vscode-oniguruma/blob/ce12600b7aa4d1314e24c306adba190b5aa8d7fc/src/onig.cc#L76-L79)
/// We store each regex/regexset result for the current line in case that allows us
/// to skip actually running the regex
#[derive(Debug, Default)]
pub(crate) struct LastMatchCache {
    /// (regex{set}+ ptr, entry)
    entries: HashMap<usize, Entry>,
}

impl LastMatchCache {
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn search(
        &mut self,
        key: usize,
        pos: usize,
        anchor_usage: AnchorUsage,
        active_anchors: AnchorActive,
        search: impl FnOnce() -> Option<Match>,
    ) -> Option<Match> {
        let cacheable = !(active_anchors.allow_a() && anchor_usage.uses_a)
            && !(active_anchors.allow_g() && anchor_usage.uses_g);

        if !cacheable {
            return search();
        }

        if let Some(entry) = self.entries.get(&key)
            && entry.pos <= pos
        {
            match &entry.result {
                // Still valid
                Some(m) if m.start >= pos => return Some(m.clone()),
                // Had nothing before, still not going to get anything now
                None => return None,
                // The match is before the pos, do a new search
                _ => {}
            }
        }

        let result = search();
        self.entries.insert(
            key,
            Entry {
                pos,
                result: result.clone(),
            },
        );
        result
    }
}
