use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use serde::Deserialize;

// use super::compiled::{CompileError, CompiledGrammar};

/// applyEndPatternLast is sometimes an integer or a bool
/// We only want them as bool
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum BoolOrNumber {
    Bool(bool),
    Number(u8),
}

/// Custom deserializer that handles both boolean and number (0/1) formats
/// This fixes compatibility with grammars that use numbers for boolean fields
fn bool_or_number<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match BoolOrNumber::deserialize(deserializer)? {
        BoolOrNumber::Bool(b) => Ok(b),
        BoolOrNumber::Number(0) => Ok(false),
        BoolOrNumber::Number(1) => Ok(true),
        BoolOrNumber::Number(x) => Err(serde::de::Error::custom(format!(
            "expected bool, 0, or 1, got {x}"
        ))),
    }
}

/// Unified rule structure that represents all possible TextMate grammar patterns
///
/// This structure replaces the previous separate Pattern enum variants with a single
/// flexible struct that can represent match patterns, begin/end patterns, begin/while
/// patterns, includes, and repository containers. The pattern type is determined by
/// which fields are present.
///
/// # Examples
///
/// Match pattern:
/// ```json
/// {
///   "match": "\\bfunction\\b",
///   "name": "storage.type.function"
/// }
/// ```
///
/// Begin/end pattern:
/// ```json
/// {
///   "begin": "\"",
///   "end": "\"",
///   "name": "string.quoted.double",
///   "patterns": [
///     {"match": "\\\\.", "name": "constant.character.escape"}
///   ]
/// }
/// ```
///
/// Include pattern:
/// ```json
/// {
///   "include": "#expressions"
/// }
/// ```
///
/// Container with nested repository:
/// ```json
/// {
///   "patterns": [
///     {"include": "#statements"}
///   ],
///   "repository": {
///     "statements": {
///       "patterns": [
///         {"match": "\\bif\\b", "name": "keyword.control"}
///       ]
///     }
///   }
/// }
/// ```
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Rule {
    // Include reference - for including other patterns by reference
    pub include: Option<String>,

    pub name: Option<String>,
    pub content_name: Option<String>,

    #[serde(rename = "match")]
    pub match_: Option<String>,
    pub captures: HashMap<String, Rule>,

    pub begin: Option<String>,
    pub begin_captures: HashMap<String, Rule>,

    pub end: Option<String>,
    pub end_captures: HashMap<String, Rule>,

    #[serde(rename = "while")]
    pub while_: Option<String>,
    pub while_captures: HashMap<String, Rule>,

    pub patterns: Vec<Rule>,
    pub repository: HashMap<String, Rule>,

    #[serde(deserialize_with = "bool_or_number")]
    pub apply_end_pattern_last: bool,
}

/// Top-level structure representing a complete TextMate grammar
///
/// # Examples
/// ```json
/// {
///   "name": "JavaScript",
///   "displayName": "JavaScript (ES6)",
///   "scopeName": "source.js",
///   "fileTypes": ["js", "jsx", "mjs"],
///   "firstLineMatch": "^#!.*\\bnode\\b",
///   "foldingStartMarker": "\\{\\s*$",
///   "foldingStopMarker": "^\\s*\\}",
///   "patterns": [
///     { "include": "#statements" },
///     { "include": "#expressions" }
///   ],
///   "repository": {
///     "statements": {
///       "patterns": [
///         { "include": "#keywords" },
///         { "include": "#functions" }
///       ]
///     }
///   }
/// }
/// ```
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all(deserialize = "camelCase"))]
pub struct RawGrammar {
    /// Human-readable name of the language
    /// Example: "JavaScript", "TypeScript", "Rust"
    pub name: String,
    /// Optional alternative display name
    /// Example: "JavaScript (ES6)", "TypeScript React"
    #[serde(default)]
    pub display_name: Option<String>,
    /// File extensions this grammar applies to
    /// Example: ["js", "jsx", "mjs"] for JavaScript
    #[serde(default)]
    pub file_types: Vec<String>,
    /// Unique identifier for this grammar's scope
    /// Example: "source.js", "text.html.markdown", "source.rust"
    pub scope_name: String,
    /// Named pattern definitions that can be referenced by includes
    /// Key is the repository name, value is the pattern(s)
    #[serde(default)]
    pub repository: HashMap<String, Rule>,
    /// Root patterns that define the top-level structure
    /// These patterns are applied first when tokenizing
    #[serde(default)]
    pub patterns: Vec<Rule>,
    /// Language injection patterns for embedding languages
    /// Maps selectors to patterns for injecting this grammar into others
    #[serde(default)]
    pub injections: HashMap<String, Rule>,
    /// CSS selector defining where injections should occur
    /// Example: "source.js meta.embedded.block.sql"
    #[serde(default)]
    pub injection_selector: Option<String>,
}

impl RawGrammar {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let file = File::open(&path)?;
        let raw_grammar = serde_json::from_reader(&file)?;
        Ok(raw_grammar)
    }

    /// Compile this raw grammar into an optimized compiled grammar
    pub fn compile(self) -> Result<CompiledGrammar, CompileError> {
        CompiledGrammar::from_raw_grammar(self)
    }
}
