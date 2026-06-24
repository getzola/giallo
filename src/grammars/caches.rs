use std::sync::{Arc, OnceLock};

use crate::grammars::engine;

/// We want to compile only one of each regex/regexset as they are expensive.
/// We key by the patterns so they can be reused in multiple grammars/context.
#[derive(Default, Debug)]
pub(crate) struct RegexCache {
    /// We cache the fancy regexes across all grammars
    regexes: papaya::HashMap<String, OnceLock<Arc<engine::Regex>>>,
    /// And the sets are cached by the patterns
    sets: papaya::HashMap<Vec<String>, OnceLock<Arc<engine::RegexSet>>>,
}

impl RegexCache {
    #[doc(hidden)]
    pub(crate) fn clear(&self) {
        self.sets.pin().clear();
        self.regexes.pin().clear();
    }

    pub fn get_regex(&self, pattern: &str) -> Arc<engine::Regex> {
        self.regexes
            .pin()
            .get_or_insert_with(pattern.to_string(), OnceLock::new)
            .get_or_init(|| Arc::new(engine::Regex::new(pattern)))
            .clone()
    }

    pub fn get_set(&self, patterns: &[String]) -> Arc<engine::RegexSet> {
        self.sets
            .pin()
            .get_or_insert_with(patterns.to_vec(), OnceLock::new)
            .get_or_init(|| {
                let regexes: Vec<_> = patterns.iter().map(|p| self.get_regex(p)).collect();
                Arc::new(engine::RegexSet::from_regexes(&regexes))
            })
            .clone()
    }
}
