use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::path::Path;

use serde::Deserialize;

use super::compiled::{CompileError, CompiledGrammar};

/// A capture group that assigns a scope name to matched text
///
/// # Examples
/// ```json
/// {
///   "1": {
///     "name": "entity.name.function.js",
///     "patterns": []
///   },
///   "2": {
///     "name": "punctuation.definition.parameters.begin.js"
///   }
/// }
/// ```
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all(deserialize = "snake_case"))]
pub struct Capture {
    /// The scope name to assign to the captured text
    /// Example: "string.quoted.double.js", "entity.name.function"
    pub name: String,
    /// Optional nested patterns that can match within this capture
    /// Rarely used - most captures just assign a scope name
    #[serde(default)]
    pub patterns: Vec<Pattern>,
}

/// A pattern that matches a single line of text using a regular expression
///
/// # Examples
/// ```json
/// {
///   "match": "\\b(function)\\s+(\\w+)\\s*\\(",
///   "name": "meta.function.declaration.js",
///   "captures": {
///     "1": { "name": "storage.type.function.js" },
///     "2": { "name": "entity.name.function.js" }
///   }
/// }
/// ```
///
/// ```json
/// {
///   "match": "\"([^\"]*)\"",
///   "name": "string.quoted.double.js",
///   "captures": {
///     "1": { "name": "string.quoted.double.content.js" }
///   }
/// }
/// ```
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct MatchPattern {
    /// Optional scope name for the entire match
    /// Example: "meta.function.declaration.js"
    pub name: Option<String>,
    /// The regular expression to match against
    /// Example: "\\b(if|else|while|for)\\b" for keywords
    #[serde(rename(deserialize = "match"))]
    pub match_: String,
    /// Named capture groups that assign scopes to parts of the match
    /// Key is the capture group number ("1", "2", etc.)
    #[serde(default)]
    pub captures: BTreeMap<String, Capture>,
    /// Optional nested patterns (rarely used in match patterns)
    #[serde(default)]
    pub patterns: Vec<Pattern>,
}

/// A pattern that matches multi-line constructs with begin/end delimiters
///
/// # Examples
/// ```json
/// {
///   "name": "string.quoted.double.js",
///   "begin": "\"",
///   "end": "\"",
///   "beginCaptures": {
///     "0": { "name": "punctuation.definition.string.begin.js" }
///   },
///   "endCaptures": {
///     "0": { "name": "punctuation.definition.string.end.js" }
///   },
///   "patterns": [
///     {
///       "match": "\\\\.",
///       "name": "constant.character.escape.js"
///     }
///   ]
/// }
/// ```
///
/// ```json
/// {
///   "name": "comment.block.js",
///   "contentName": "comment.block.content.js",
///   "begin": "/\\*",
///   "end": "\\*/",
///   "captures": {
///     "0": { "name": "punctuation.definition.comment.js" }
///   }
/// }
/// ```
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all(deserialize = "camelCase"))]
pub struct BeginEndPattern {
    /// Optional scope name for the entire begin/end region
    /// Example: "string.quoted.double.js"
    #[serde(default)]
    pub name: Option<String>,
    /// Optional scope name for content between begin and end
    /// Example: "string.quoted.double.content.js"
    #[serde(default)]
    pub content_name: Option<String>,
    /// Regular expression that matches the beginning delimiter
    /// Example: "\"" for double-quoted strings, "/\\*" for block comments
    pub begin: String,
    /// Regular expression that matches the ending delimiter
    /// Can reference captures from begin pattern using \\1, \\2, etc.
    /// Example: "\"", "\\*/", or "\\1" to match the same quote type
    pub end: String,
    /// Capture groups for both begin and end patterns (fallback)
    /// Used when begin_captures or end_captures are not specified
    #[serde(default)]
    pub captures: BTreeMap<String, Capture>,
    /// Capture groups specifically for the begin pattern
    /// Example: {"0": {"name": "punctuation.definition.string.begin"}}
    #[serde(default)]
    pub begin_captures: BTreeMap<String, Capture>,
    /// Capture groups specifically for the end pattern
    /// Example: {"0": {"name": "punctuation.definition.string.end"}}
    #[serde(default)]
    pub end_captures: BTreeMap<String, Capture>,
    /// Nested patterns that can match within the begin/end region
    /// Example: escape sequences within strings
    #[serde(default)]
    pub patterns: Vec<Pattern>,
    /// Whether to apply the end pattern last (after nested patterns)
    /// Set to 1 (true) to allow nested patterns to override end matching
    #[serde(default)]
    pub apply_end_pattern_last: Option<u32>,
}

/// A pattern that matches multi-line constructs that continue while a condition holds
///
/// # Examples
/// ```json
/// {
///   "name": "markup.list.numbered.markdown",
///   "begin": "^\\s*(\\d+)\\.",
///   "while": "^\\s*(?=\\d+\\.)",
///   "beginCaptures": {
///     "1": { "name": "markup.list.numbered.bullet.markdown" }
///   },
///   "patterns": [
///     { "include": "#inline" }
///   ]
/// }
/// ```
///
/// ```json
/// {
///   "name": "meta.paragraph.markdown",
///   "begin": "^(?=\\S)",
///   "while": "^(?!\\s*$)",
///   "patterns": [
///     { "include": "#inline" }
///   ]
/// }
/// ```
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all(deserialize = "camelCase"))]
pub struct BeginWhilePattern {
    /// Optional scope name for the entire begin/while region
    /// Example: "markup.list.numbered.markdown"
    #[serde(default)]
    pub name: Option<String>,
    /// Optional scope name for content within the region
    /// Example: "markup.list.numbered.content.markdown"
    #[serde(default)]
    pub content_name: Option<String>,
    /// Regular expression that matches the beginning of the region
    /// Example: "^\\s*(\\d+)\\." for numbered lists
    pub begin: String,
    /// Regular expression that must match for the region to continue
    /// If this stops matching, the region ends
    /// Example: "^\\s*(?=\\d+\\.)" to continue while next line starts with number
    #[serde(rename(deserialize = "while"))]
    pub while_: String,
    /// Capture groups for both begin and while patterns (fallback)
    #[serde(default)]
    pub captures: BTreeMap<String, Capture>,
    /// Capture groups specifically for the begin pattern
    #[serde(default)]
    pub begin_captures: BTreeMap<String, Capture>,
    /// Capture groups specifically for the while pattern
    #[serde(default)]
    pub while_captures: BTreeMap<String, Capture>,
    /// Nested patterns that can match within the begin/while region
    #[serde(default)]
    pub patterns: Vec<Pattern>,
}

/// A pattern that includes other patterns by reference
///
/// # Examples
/// ```json
/// {
///   "include": "#statements"
/// }
/// ```
///
/// ```json
/// {
///   "include": "source.js#expressions"
/// }
/// ```
///
/// ```json
/// {
///   "include": "$self"
/// }
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct IncludePattern {
    /// Reference to patterns to include
    /// - "#name" - reference to repository entry in same grammar
    /// - "source.lang" - reference to another grammar's root patterns
    /// - "source.lang#name" - reference to repository entry in another grammar
    /// - "$self" - reference to current grammar's root patterns
    /// - "$base" - reference to base grammar (rarely used)
    pub include: String,
}

/// Union type representing all possible pattern types in a TextMate grammar
///
/// The order matters for serde deserialization - more specific patterns are tried first.
/// This ensures that patterns with required fields are matched before those with defaults.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Pattern {
    /// Multi-line pattern with begin/end delimiters
    /// Example: strings, comments, function bodies
    BeginEnd(BeginEndPattern),
    /// Multi-line pattern that continues while condition is true
    /// Example: markdown paragraphs, indented blocks
    BeginWhile(BeginWhilePattern),
    /// Reference to other patterns
    /// *Must come before Match in the enum to be correct*
    /// Example: including repository entries or other grammars
    Include(IncludePattern),
    /// Single-line pattern using regex matching
    /// Example: keywords, operators, literals
    Match(MatchPattern),
    /// Container for multiple patterns (most general, should be last)
    /// Example: top-level grammar patterns
    Repository(RepositoryPattern),
}

/// A simple container for multiple patterns
///
/// # Examples
/// ```json
/// {
///   "patterns": [
///     {
///       "match": "\\bif\\b",
///       "name": "keyword.control.if.js"
///     },
///     {
///       "match": "\\belse\\b",
///       "name": "keyword.control.else.js"
///     }
///   ]
/// }
/// ```
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct RepositoryPattern {
    /// List of patterns to match
    #[serde(default)]
    pub patterns: Vec<Pattern>,
}

/// Different ways to define reusable patterns in the repository
///
/// # Examples
/// ```json
/// {
///   "repository": {
///     "keywords": [
///       {
///         "match": "\\bif\\b",
///         "name": "keyword.control.if.js"
///       }
///     ],
///     "expressions": {
///       "patterns": [
///         { "include": "#literals" },
///         { "include": "#operators" }
///       ]
///     },
///     "string": {
///       "name": "string.quoted.double.js",
///       "begin": "\"",
///       "end": "\""
///     }
///   }
/// }
/// ```
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum RepositoryEntry {
    /// Direct array of patterns (like `"keywords": [...]`)
    /// Most compact form for simple pattern lists
    DirectArray(Vec<Pattern>),
    /// Object containing patterns array (like `"expressions": {"patterns": [...]}`)
    /// Standard form that allows future extension with additional properties
    PatternContainer {
        #[serde(default)]
        patterns: Vec<Pattern>,
    },
    /// Single pattern directly (like `"string": {"begin": "\"", "end": "\""}`)
    /// Convenient shorthand for repository entries that are single patterns
    DirectPattern(Pattern),
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
    pub repository: HashMap<String, RepositoryEntry>,
    /// Root patterns that define the top-level structure
    /// These patterns are applied first when tokenizing
    #[serde(default)]
    pub patterns: Vec<Pattern>,
    /// Optional regex to identify files by their first line content
    /// Example: "^#!.*\\bnode\\b" to detect Node.js scripts
    #[serde(default)]
    pub first_line_match: Option<String>,
    /// Regex to identify where foldable regions start
    /// Example: "\\{\\s*$" for opening braces
    #[serde(default)]
    pub folding_start_marker: Option<String>,
    /// Regex to identify where foldable regions end
    /// Example: "^\\s*\\}" for closing braces
    #[serde(default)]
    pub folding_stop_marker: Option<String>,
    /// Language injection patterns for embedding languages
    /// Maps selectors to patterns for injecting this grammar into others
    #[serde(default)]
    pub injections: HashMap<String, RepositoryEntry>,
    /// List of languages this grammar should be injected into
    /// Example: ["text.html.basic"] to inject into HTML
    #[serde(default)]
    pub inject_to: Vec<String>,
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
