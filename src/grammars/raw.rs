use std::collections::HashMap;
use std::fs::File;
use std::ops::Deref;
use std::path::Path;

use serde::Deserialize;

use super::compiled::{CompileError, CompiledGrammar};

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

/// Transparent wrapper around `HashMap<usize, RawRule>` for TextMate grammar captures.
///
/// This type allows seamless deserialization from JSON objects with string keys (like "1", "2", "3")
/// while providing type-safe usize-indexed access in Rust code. According to the TextMate grammar
/// specification, capture keys must be numeric strings corresponding to regex capture groups.
///
/// # Examples
///
/// JSON input:
/// ```json
/// {
///   "captures": {
///     "0": {"name": "entire.match"},
///     "1": {"name": "first.group"},
///     "2": {"name": "second.group"}
///   }
/// }
/// ```
///
#[derive(Debug, Clone, Default)]
pub struct Captures(pub(crate) HashMap<usize, RawRule>);

/// Helper enum for deserializing captures in both object and array formats
#[derive(Deserialize)]
#[serde(untagged)]
enum CapturesFormat {
    Object(HashMap<String, RawRule>),
    Array(Vec<RawRule>),
}

impl<'de> Deserialize<'de> for Captures {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Try to deserialize as our supported formats, but handle the case where it might be empty/null
        match CapturesFormat::deserialize(deserializer) {
            Ok(captures_format) => {
                let mut usize_map = HashMap::new();

                match captures_format {
                    CapturesFormat::Object(string_map) => {
                        for (key, value) in string_map {
                            // anything not a number is a bug, just skip them
                            // currently only for XML syntax https://github.com/microsoft/vscode/pull/269766
                            if let Ok(idx) = key.parse::<usize>() {
                                usize_map.insert(idx, value);
                            }
                        }
                    }
                    CapturesFormat::Array(array) => {
                        for (idx, value) in array.into_iter().enumerate() {
                            usize_map.insert(idx, value);
                        }
                    }
                }

                Ok(Captures(usize_map))
            }
            Err(_) => {
                // If deserialization fails, just return an empty Captures
                // This handles cases like null, empty strings, or unexpected formats
                Ok(Captures(HashMap::new()))
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawRuleValue {
    Vec(Vec<RawRule>),
    Single(RawRule),
}

/// Custom deserializer for repository HashMap that handles values that might be single rules or arrays of rules
fn deserialize_repository_map<'de, D>(deserializer: D) -> Result<HashMap<String, RawRule>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw_map = HashMap::<String, RawRuleValue>::deserialize(deserializer)?;
    let mut result = HashMap::new();

    for (key, val) in raw_map {
        let rule = match val {
            RawRuleValue::Vec(rules) => RawRule {
                patterns: rules,
                ..Default::default()
            },
            RawRuleValue::Single(rule) => rule,
        };
        result.insert(key, rule);
    }

    Ok(result)
}

impl Deref for Captures {
    type Target = HashMap<usize, RawRule>;

    fn deref(&self) -> &Self::Target {
        &self.0
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
pub struct RawRule {
    // Include reference - for including other patterns by reference
    pub include: Option<String>,

    pub name: Option<String>,
    pub content_name: Option<String>,

    #[serde(rename = "match")]
    pub match_: Option<String>,
    pub captures: Captures,

    pub begin: Option<String>,
    pub begin_captures: Captures,

    pub end: Option<String>,
    pub end_captures: Captures,

    #[serde(rename = "while")]
    pub while_: Option<String>,
    pub while_captures: Captures,

    pub patterns: Vec<RawRule>,
    #[serde(deserialize_with = "deserialize_repository_map")]
    pub repository: HashMap<String, RawRule>,

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
    #[serde(default, deserialize_with = "deserialize_repository_map")]
    pub repository: HashMap<String, RawRule>,
    /// Root patterns that define the top-level structure
    /// These patterns are applied first when tokenizing
    #[serde(default)]
    pub patterns: Vec<RawRule>,
    /// Language injection patterns for embedding languages
    /// Maps selectors to patterns for injecting this grammar into others
    #[serde(default)]
    pub injections: HashMap<String, RawRule>,
    /// CSS selector defining where injections should occur
    /// Example: "source.js meta.embedded.block.sql"
    #[serde(default)]
    pub injection_selector: Option<String>,
}

impl RawGrammar {
    pub fn load_from_str(content: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let raw_grammar = serde_json::from_str(content)?;
        Ok(raw_grammar)
    }

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

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn can_parse_all_grammars() {
        let entries = fs::read_dir("grammars-themes/packages/tm-grammars/grammars")
            .expect("Failed to read grammars directory");

        for entry in entries {
            let entry = entry.expect("Failed to read directory entry");
            let path = entry.path();
            assert!(RawGrammar::load_from_file(&path).is_ok());
        }
    }
}
