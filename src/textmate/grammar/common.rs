use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};

/// A regex wrapper that serializes as a string but compiles lazily at runtime
#[derive(Debug)]
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

impl Regex {
    pub fn new(pattern: String) -> Self {
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

/// Errors that can occur during grammar compilation
#[derive(Debug)]
pub enum CompileError {
    InvalidRegex { pattern: String, error: onig::Error },
    UnknownScope { scope: String },
    UnresolvedInclude { include: String },
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompileError::InvalidRegex { pattern, error } => {
                write!(f, "Invalid regex pattern '{}': {}", pattern, error)
            }
            CompileError::UnknownScope { scope } => {
                write!(f, "Unknown scope '{}'", scope)
            }
            CompileError::UnresolvedInclude { include } => {
                write!(f, "Unresolved include '{}'", include)
            }
        }
    }
}

impl std::error::Error for CompileError {}
