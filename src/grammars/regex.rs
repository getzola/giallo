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

    /// Try to find a match starting at the given position
    pub fn find_at(&self, text: &str, start: usize) -> Option<(usize, usize)> {
        let regex = self.compiled()?;
        let search_text = text.get(start..)?;
        if let Some(pos) = regex.find(search_text) {
            // Adjust match positions to be relative to original text
            Some((pos.0 + start, pos.1 + start))
        } else {
            None
        }
    }

    /// Try to get captures starting at the given position
    pub fn captures_at(&self, text: &str, start: usize) -> Option<Vec<String>> {
        let regex = self.compiled()?;
        let search_text = text.get(start..)?;

        if let Some(captures) = regex.captures(search_text) {
            let mut result = Vec::new();
            for i in 0..captures.len() {
                if let Some(capture) = captures.at(i) {
                    result.push(capture.to_string());
                } else {
                    result.push(String::new());
                }
            }
            Some(result)
        } else {
            None
        }
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
