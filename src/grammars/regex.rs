use std::fmt;
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};

/// A regex wrapper that serializes as a string but compiles lazily at runtime
pub struct Regex {
    pattern: String,
    compiled: OnceLock<Option<Arc<onig::Regex>>>,
}

impl Clone for Regex {
    fn clone(&self) -> Self {
        // Create a new regex with the same pattern but fresh lazy compilation
        Regex::new(self.pattern.clone())
    }
}

impl fmt::Debug for Regex {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.pattern)
    }
}

impl Regex {
    pub fn new(pattern: String) -> Self {
        // TODO: validate and look for backreference

        Self {
            pattern,
            compiled: OnceLock::new(),
        }
    }

    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    pub fn compiled(&self) -> Option<&Arc<onig::Regex>> {
        self.compiled
            .get_or_init(|| onig::Regex::new(&self.pattern).ok().map(Arc::new))
            .as_ref()
    }

    /// Validate that this regex pattern compiles successfully
    pub fn validate(&self) -> Result<(), onig::Error> {
        onig::Regex::new(&self.pattern).map(|_| ())
    }

    pub fn has_backreferences(&self) -> bool {
        for i in 1..=9 {
            let backref = format!("\\{}", i);
            if self.pattern.contains(&backref) {
                return true;
            }
        }
        false
    }
}

impl Serialize for Regex {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.pattern)
    }
}

impl<'de> Deserialize<'de> for Regex {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let pattern = String::deserialize(deserializer)?;
        Ok(Regex::new(pattern))
    }
}
