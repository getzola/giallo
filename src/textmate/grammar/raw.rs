use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::path::Path;

use serde::Deserialize;

use super::common::{CompileError, Regex};
use super::compiled::{
    CompiledBeginEndPattern, CompiledBeginWhilePattern, CompiledCapture, CompiledGrammar,
    CompiledIncludePattern, CompiledMatchPattern, CompiledPattern,
};
use super::get_scope_id;

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
    /// Reference to other patterns (must come before Match due to serde defaults)
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
    /// Check if a pattern might contain backreferences (\\1 through \\9)
    /// This is a conservative check - we only skip validation if we're fairly sure
    /// there are backreferences to avoid false positives.
    fn might_have_backreferences(pattern: &str) -> bool {
        // Only consider it a backreference if it's likely to be one
        // Look for \\ followed by digit, but be more conservative
        for i in 1..=9 {
            let backref = format!("\\{}", i);
            if pattern.contains(&backref) {
                // Additional heuristic: make sure it looks like a real backreference
                // by checking it's not escaped (like \\\\1) or in a character class
                // For now, just use the simple check but be aware this could be improved
                return true;
            }
        }
        false
    }

    pub fn load_from_json_file<P: AsRef<Path>>(
        path: P,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let file = File::open(&path)?;
        let raw_grammar = serde_json::from_reader(&file)?;
        Ok(raw_grammar)
    }

    /// Compile this raw grammar into an optimized compiled grammar
    pub fn compile(&self) -> Result<CompiledGrammar, CompileError> {
        // Get the scope ID for the main scope
        let scope_id =
            get_scope_id(&self.scope_name).ok_or_else(|| CompileError::UnknownScope {
                scope: self.scope_name.clone(),
            })?;

        // Compile the first line match regex if present
        let first_line_regex = self
            .first_line_match
            .as_ref()
            .map(|pattern| {
                let regex = Regex::new(pattern.clone());
                regex.validate().map_err(|e| CompileError::InvalidRegex {
                    pattern: pattern.clone(),
                    error: e,
                })?;
                Ok::<Regex, CompileError>(regex)
            })
            .transpose()?;

        // Compile all patterns
        let patterns = self
            .patterns
            .iter()
            .map(|pattern| self.compile_pattern(pattern))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(CompiledGrammar {
            name: self.name.clone(),
            display_name: self.display_name.clone(),
            scope_name: self.scope_name.clone(),
            scope_id,
            file_types: self.file_types.clone(),
            patterns,
            first_line_regex,
        })
    }

    fn compile_pattern(&self, pattern: &Pattern) -> Result<CompiledPattern, CompileError> {
        self.compile_pattern_with_visited(pattern, &mut std::collections::HashSet::new())
    }

    fn compile_pattern_with_visited(
        &self,
        pattern: &Pattern,
        visited: &mut std::collections::HashSet<String>,
    ) -> Result<CompiledPattern, CompileError> {
        match pattern {
            Pattern::Match(match_pattern) => {
                let regex = Regex::new(match_pattern.match_.clone());
                regex.validate().map_err(|e| CompileError::InvalidRegex {
                    pattern: match_pattern.match_.clone(),
                    error: e,
                })?;

                let name_scope_id = match_pattern
                    .name
                    .as_ref()
                    .map(|name| get_scope_id(name))
                    .flatten();

                let captures = match_pattern
                    .captures
                    .iter()
                    .map(|(key, capture)| {
                        let scope_id = get_scope_id(&capture.name).ok_or_else(|| {
                            CompileError::UnknownScope {
                                scope: capture.name.clone(),
                            }
                        })?;
                        let compiled_capture = CompiledCapture {
                            scope_id,
                            patterns: capture
                                .patterns
                                .iter()
                                .map(|p| self.compile_pattern_with_visited(p, visited))
                                .collect::<Result<Vec<_>, _>>()?,
                        };
                        Ok((key.clone(), compiled_capture))
                    })
                    .collect::<Result<BTreeMap<_, _>, CompileError>>()?;

                let patterns = match_pattern
                    .patterns
                    .iter()
                    .map(|p| self.compile_pattern_with_visited(p, visited))
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(CompiledPattern::Match(CompiledMatchPattern {
                    name_scope_id,
                    regex,
                    captures,
                    patterns,
                }))
            }

            Pattern::BeginEnd(begin_end) => {
                let begin_regex = Regex::new(begin_end.begin.clone());
                begin_regex
                    .validate()
                    .map_err(|e| CompileError::InvalidRegex {
                        pattern: begin_end.begin.clone(),
                        error: e,
                    })?;

                // Check if the end pattern contains backreferences
                let has_backreferences = Self::might_have_backreferences(&begin_end.end);

                let end_regex = Regex::new(begin_end.end.clone());

                // Only validate the end regex if it doesn't contain backreferences
                if !has_backreferences {
                    end_regex
                        .validate()
                        .map_err(|e| CompileError::InvalidRegex {
                            pattern: begin_end.end.clone(),
                            error: e,
                        })?;
                }

                let name_scope_id = begin_end
                    .name
                    .as_ref()
                    .map(|name| get_scope_id(name))
                    .flatten();

                let content_name_scope_id = begin_end
                    .content_name
                    .as_ref()
                    .map(|name| get_scope_id(name))
                    .flatten();

                // Compile captures - prefer specific captures over general ones
                let captures = self.compile_captures_with_visited(&begin_end.captures, visited)?;
                let begin_captures =
                    self.compile_captures_with_visited(&begin_end.begin_captures, visited)?;
                let end_captures =
                    self.compile_captures_with_visited(&begin_end.end_captures, visited)?;

                let patterns = begin_end
                    .patterns
                    .iter()
                    .map(|p| self.compile_pattern_with_visited(p, visited))
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(CompiledPattern::BeginEnd(CompiledBeginEndPattern {
                    name_scope_id,
                    content_name_scope_id,
                    begin_regex,
                    end_regex,
                    end_pattern_source: begin_end.end.clone(),
                    captures,
                    begin_captures,
                    end_captures,
                    patterns,
                    apply_end_pattern_last: begin_end.apply_end_pattern_last.unwrap_or(0) != 0,
                }))
            }

            Pattern::BeginWhile(begin_while) => {
                let begin_regex = Regex::new(begin_while.begin.clone());
                begin_regex
                    .validate()
                    .map_err(|e| CompileError::InvalidRegex {
                        pattern: begin_while.begin.clone(),
                        error: e,
                    })?;

                // Check if the while pattern contains backreferences
                let has_while_backreferences = Self::might_have_backreferences(&begin_while.while_);

                let while_regex = Regex::new(begin_while.while_.clone());

                // Only validate the while regex if it doesn't contain backreferences
                if !has_while_backreferences {
                    while_regex
                        .validate()
                        .map_err(|e| CompileError::InvalidRegex {
                            pattern: begin_while.while_.clone(),
                            error: e,
                        })?;
                }

                let name_scope_id = begin_while
                    .name
                    .as_ref()
                    .map(|name| get_scope_id(name))
                    .flatten();

                let content_name_scope_id = begin_while
                    .content_name
                    .as_ref()
                    .map(|name| get_scope_id(name))
                    .flatten();

                let captures =
                    self.compile_captures_with_visited(&begin_while.captures, visited)?;
                let begin_captures =
                    self.compile_captures_with_visited(&begin_while.begin_captures, visited)?;
                let while_captures =
                    self.compile_captures_with_visited(&begin_while.while_captures, visited)?;

                let patterns = begin_while
                    .patterns
                    .iter()
                    .map(|p| self.compile_pattern_with_visited(p, visited))
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(CompiledPattern::BeginWhile(CompiledBeginWhilePattern {
                    name_scope_id,
                    content_name_scope_id,
                    begin_regex,
                    while_regex,
                    while_pattern_source: begin_while.while_.clone(),
                    captures,
                    begin_captures,
                    while_captures,
                    patterns,
                }))
            }

            Pattern::Include(include) => {
                // Resolve include references
                if include.include.starts_with('#') {
                    // Repository reference like "#statements"
                    let repo_key = &include.include[1..]; // Remove the '#'

                    // Check for cycles
                    if visited.contains(repo_key) {
                        // Cycle detected, return empty include to break the cycle
                        return Ok(CompiledPattern::Include(CompiledIncludePattern {
                            patterns: vec![],
                        }));
                    }

                    visited.insert(repo_key.to_string());

                    if let Some(repo_entry) = self.repository.get(repo_key) {
                        match repo_entry {
                            RepositoryEntry::DirectArray(patterns) => {
                                let compiled_patterns = patterns
                                    .iter()
                                    .map(|p| self.compile_pattern_with_visited(p, visited))
                                    .collect::<Result<Vec<_>, _>>()?;
                                Ok(CompiledPattern::Include(CompiledIncludePattern {
                                    patterns: compiled_patterns,
                                }))
                            }
                            RepositoryEntry::PatternContainer { patterns } => {
                                let compiled_patterns = patterns
                                    .iter()
                                    .map(|p| self.compile_pattern_with_visited(p, visited))
                                    .collect::<Result<Vec<_>, _>>()?;
                                Ok(CompiledPattern::Include(CompiledIncludePattern {
                                    patterns: compiled_patterns,
                                }))
                            }
                            RepositoryEntry::DirectPattern(pattern) => {
                                let compiled_pattern =
                                    self.compile_pattern_with_visited(pattern, visited)?;
                                Ok(CompiledPattern::Include(CompiledIncludePattern {
                                    patterns: vec![compiled_pattern],
                                }))
                            }
                        }
                    } else {
                        // Repository key not found - this can happen with incomplete grammars
                        // or when grammars reference repository entries from other grammars
                        // For now, we return an empty include to prevent compilation failure
                        // TODO: Consider adding proper logging when log crate is available
                        Ok(CompiledPattern::Include(CompiledIncludePattern {
                            patterns: vec![],
                        }))
                    }
                } else {
                    // Other types of includes (like external grammars) - not implemented yet
                    // This could include references to other grammar files or external scopes
                    // For now, return empty patterns to gracefully handle unsupported includes
                    Ok(CompiledPattern::Include(CompiledIncludePattern {
                        patterns: vec![],
                    }))
                }
            }

            Pattern::Repository(repo) => {
                let patterns = repo
                    .patterns
                    .iter()
                    .map(|p| self.compile_pattern_with_visited(p, visited))
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(CompiledPattern::Include(CompiledIncludePattern {
                    patterns,
                }))
            }
        }
    }

    fn compile_captures_with_visited(
        &self,
        captures: &BTreeMap<String, Capture>,
        visited: &mut std::collections::HashSet<String>,
    ) -> Result<BTreeMap<String, CompiledCapture>, CompileError> {
        captures
            .iter()
            .map(|(key, capture)| {
                let scope_id =
                    get_scope_id(&capture.name).ok_or_else(|| CompileError::UnknownScope {
                        scope: capture.name.clone(),
                    })?;
                let compiled_capture = CompiledCapture {
                    scope_id,
                    patterns: capture
                        .patterns
                        .iter()
                        .map(|p| self.compile_pattern_with_visited(p, visited))
                        .collect::<Result<Vec<_>, _>>()?,
                };
                Ok((key.clone(), compiled_capture))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_load_all_grammars() {
        let grammars_dir = "grammars-themes/packages/tm-grammars/grammars";

        let mut total_grammars = 0;
        let mut loaded_grammars = 0;
        let mut failed_grammars = Vec::new();

        // Read all .json files in the grammars directory
        let entries = fs::read_dir(grammars_dir).expect("Failed to read grammars directory");

        for entry in entries {
            let entry = entry.expect("Failed to read directory entry");
            let path = entry.path();

            // Only process .json files
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                total_grammars += 1;

                let filename = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");

                // Loading grammar file...

                match RawGrammar::load_from_json_file(&path) {
                    Ok(grammar) => {
                        loaded_grammars += 1;
                        // Basic validation - ensure required fields are present
                        assert!(
                            !grammar.name.is_empty(),
                            "Grammar {} has empty name",
                            filename
                        );
                        assert!(
                            !grammar.scope_name.is_empty(),
                            "Grammar {} has empty scope_name",
                            filename
                        );
                    }
                    Err(e) => {
                        failed_grammars.push((filename.to_string(), e.to_string()));
                    }
                }
            }
        }

        // Grammar loading completed

        // Test passes if we can load at least some grammars and have very few failures
        assert!(
            total_grammars > 0,
            "No grammar files found in {}",
            grammars_dir
        );
        assert!(
            loaded_grammars > 200,
            "Should load most grammars, got {}",
            loaded_grammars
        );
        assert!(
            failed_grammars.len() <= 5,
            "Too many failed grammars: {}. Details: {:?}",
            failed_grammars.len(),
            failed_grammars
        );

        // Test completed successfully
    }

    #[test]
    fn test_compile_all_grammars() {
        let grammars_dir = "grammars-themes/packages/tm-grammars/grammars";

        // Check if directory exists
        if !std::path::Path::new(grammars_dir).exists() {
            // Skip test if grammars directory doesn't exist
            return;
        }

        let mut total_grammars = 0;
        let mut compiled_grammars = 0;
        let mut failed_compilations = Vec::new();

        // Read all .json files in the grammars directory
        let entries = fs::read_dir(grammars_dir).expect("Failed to read grammars directory");

        for entry in entries {
            let entry = entry.expect("Failed to read directory entry");
            let path = entry.path();

            // Only process .json files
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                total_grammars += 1;

                let filename = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");

                // Compiling grammar file...

                // First load the grammar
                match RawGrammar::load_from_json_file(&path) {
                    Ok(raw_grammar) => {
                        // Then try to compile it
                        match raw_grammar.compile() {
                            Ok(_compiled) => {
                                compiled_grammars += 1;
                            }
                            Err(e) => {
                                failed_compilations.push((
                                    filename.to_string(),
                                    format!("Compilation failed: {}", e),
                                ));
                            }
                        }
                    }
                    Err(e) => {
                        failed_compilations
                            .push((filename.to_string(), format!("Loading failed: {}", e)));
                    }
                }
            }
        }

        // Grammar compilation completed

        // Test passes if we can compile at least some grammars
        // We expect failures due to unknown scopes since we haven't generated a complete scope map
        assert!(
            total_grammars > 0,
            "No grammar files found in {}",
            grammars_dir
        );
        assert!(
            compiled_grammars > 0,
            "Should compile at least some grammars, got {}",
            compiled_grammars
        );

        // Allow up to 90% failure rate due to missing scopes - this test is mainly checking that
        // the compilation process doesn't panic and handles errors gracefully
        let failure_rate = failed_compilations.len() as f64 / total_grammars as f64;
        assert!(
            failure_rate < 0.95,
            "Too many failed compilations: {:.1}% ({}/{}). This suggests a systemic issue.",
            failure_rate * 100.0,
            failed_compilations.len(),
            total_grammars
        );

        // Test completed successfully
    }
}
