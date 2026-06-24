use std::sync::{Arc, OnceLock};

use fancy_regex::{Regex, RegexSet};

use crate::grammars::regex::compile_regex;

/// We keep the Regex around for the prefilter
pub(crate) type SharedSet = (Vec<Arc<Regex>>, RegexSet);

/// We want to compile only of each regex/regexset as they are expensive.
/// We key by the patterns so they can be reused in multiple grammars/context.
#[derive(Debug, Default)]
pub(crate) struct RegexCache {
    /// We cache the fancy regexes across all grammars
    regexes: papaya::HashMap<String, OnceLock<Arc<Regex>>>,
    /// And the sets are cached by the patterns
    sets: papaya::HashMap<Vec<String>, OnceLock<SharedSet>>,
}

impl RegexCache {
    #[doc(hidden)]
    pub(crate) fn clear(&self) {
        self.sets.pin().clear();
        self.regexes.pin().clear();
    }

    pub fn get_regex(&self, pattern: &str) -> Arc<Regex> {
        self.regexes
            .pin()
            .get_or_insert_with(pattern.to_string(), OnceLock::new)
            .get_or_init(|| Arc::new(compile_regex(pattern)))
            .clone()
    }

    pub fn get_set(&self, patterns: &[String]) -> SharedSet {
        self.sets
            .pin()
            .get_or_insert_with(patterns.to_vec(), OnceLock::new)
            .get_or_init(|| {
                let regexes: Vec<_> = patterns.iter().map(|x| self.get_regex(x)).collect();
                let set = match RegexSet::from_regexes(regexes.clone(), Default::default()) {
                    Ok(set) => set,
                    Err(e) => {
                        unreachable!("WTF: {}", e);
                    }
                };
                (regexes, set)
            })
            .clone()
    }
}
