use super::common::Regex;
use crate::grammars::raw::{Capture, Pattern, RepositoryEntry, Rule};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

use super::{RawGrammar, ScopeId, get_scope_id};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledCapture {
    pub scope_id: ScopeId,
    #[serde(default)]
    pub patterns: Vec<CompiledPattern>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledMatchPattern {
    pub name_scope_id: Option<ScopeId>,
    pub regex: Regex,
    #[serde(default)]
    pub captures: BTreeMap<String, CompiledCapture>,
    #[serde(default)]
    pub patterns: Vec<CompiledPattern>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledBeginEndPattern {
    pub name_scope_id: Option<ScopeId>,
    pub content_name_scope_id: Option<ScopeId>,
    pub begin_regex: Regex,
    pub end_regex: Regex,
    /// The original end pattern string (may contain unresolved backreferences)
    pub end_pattern_source: String,
    #[serde(default)]
    pub captures: BTreeMap<String, CompiledCapture>,
    #[serde(default)]
    pub begin_captures: BTreeMap<String, CompiledCapture>,
    #[serde(default)]
    pub end_captures: BTreeMap<String, CompiledCapture>,
    #[serde(default)]
    pub patterns: Vec<CompiledPattern>,
    #[serde(default)]
    pub apply_end_pattern_last: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledBeginWhilePattern {
    pub name_scope_id: Option<ScopeId>,
    pub content_name_scope_id: Option<ScopeId>,
    pub begin_regex: Regex,
    pub while_regex: Regex,
    /// The original while pattern string (may contain unresolved backreferences)
    pub while_pattern_source: String,
    #[serde(default)]
    pub captures: BTreeMap<String, CompiledCapture>,
    #[serde(default)]
    pub begin_captures: BTreeMap<String, CompiledCapture>,
    #[serde(default)]
    pub while_captures: BTreeMap<String, CompiledCapture>,
    #[serde(default)]
    pub patterns: Vec<CompiledPattern>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledIncludePattern {
    /// The resolved patterns from the include reference
    pub patterns: Vec<CompiledPattern>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CompiledPattern {
    BeginEnd(CompiledBeginEndPattern),
    BeginWhile(CompiledBeginWhilePattern),
    Match(CompiledMatchPattern),
    Include(CompiledIncludePattern),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledGrammar {
    pub name: String,
    pub display_name: Option<String>,
    pub scope_name: String,
    pub scope_id: ScopeId,
    pub file_types: Vec<String>,
    pub patterns: Vec<CompiledPattern>,
    pub first_line_regex: Option<Regex>,
}

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

/// Compile and validate a regex pattern, optionally allowing backreferences
fn compile_and_validate_regex(
    pattern: &str,
    allow_backreferences: bool,
) -> Result<Regex, CompileError> {
    let regex = Regex::new(pattern.to_string());

    // Only validate if backreferences aren't expected
    if !allow_backreferences || !might_have_backreferences(pattern) {
        regex.validate().map_err(|e| CompileError::InvalidRegex {
            pattern: pattern.to_string(),
            error: e,
        })?;
    }

    Ok(regex)
}

fn get_optional_scope_id(name: &Option<String>) -> Option<ScopeId> {
    name.as_ref().and_then(|n| get_scope_id(n))
}

/// Compile a list of patterns recursively
fn compile_nested_patterns(
    raw_grammar: &RawGrammar,
    patterns: &[Pattern],
    visited: &mut HashSet<String>,
) -> Result<Vec<CompiledPattern>, CompileError> {
    patterns
        .iter()
        .map(|p| compile_pattern_with_visited(raw_grammar, p, visited))
        .collect()
}

/// Compile capture groups
fn compile_captures(
    raw_grammar: &RawGrammar,
    captures: &BTreeMap<String, Capture>,
    visited: &mut HashSet<String>,
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
                patterns: compile_nested_patterns(raw_grammar, &capture.patterns, visited)?,
            };
            Ok((key.clone(), compiled_capture))
        })
        .collect()
}

/// Main pattern compilation function with visited tracking
fn compile_pattern_with_visited(
    raw_grammar: &RawGrammar,
    pattern: &Pattern,
    visited: &mut HashSet<String>,
) -> Result<CompiledPattern, CompileError> {
    match pattern {
        Pattern::Match(match_pattern) => {
            let regex = compile_and_validate_regex(&match_pattern.match_, false)?;
            let name_scope_id = get_optional_scope_id(&match_pattern.name);
            let captures = compile_captures(raw_grammar, &match_pattern.captures, visited)?;
            let patterns = compile_nested_patterns(raw_grammar, &match_pattern.patterns, visited)?;

            let compiled = CompiledPattern::Match(CompiledMatchPattern {
                name_scope_id,
                regex,
                captures,
                patterns,
            });
            Ok(compiled)
        }
        Pattern::BeginEnd(begin_end) => {
            let begin_regex = compile_and_validate_regex(&begin_end.begin, false)?;
            let end_regex = compile_and_validate_regex(&begin_end.end, true)?; // Allow backrefs
            let name_scope_id = get_optional_scope_id(&begin_end.name);
            let content_name_scope_id = get_optional_scope_id(&begin_end.content_name);
            let captures = compile_captures(raw_grammar, &begin_end.captures, visited)?;
            let begin_captures = compile_captures(raw_grammar, &begin_end.begin_captures, visited)?;
            let patterns = compile_nested_patterns(raw_grammar, &begin_end.patterns, visited)?;
            let end_captures = compile_captures(raw_grammar, &begin_end.end_captures, visited)?;

            Ok(CompiledPattern::BeginEnd(CompiledBeginEndPattern {
                name_scope_id,
                content_name_scope_id,
                begin_regex,
                end_regex,
                captures,
                begin_captures,
                end_captures,
                patterns,
                end_pattern_source: begin_end.end.clone(),
                apply_end_pattern_last: begin_end.apply_end_pattern_last.unwrap_or(0) != 0,
            }))
        }
        Pattern::BeginWhile(begin_while) => {
            let begin_regex = compile_and_validate_regex(&begin_while.begin, false)?;
            let while_regex = compile_and_validate_regex(&begin_while.while_, true)?; // Allow backrefs
            let name_scope_id = get_optional_scope_id(&begin_while.name);
            let content_name_scope_id = get_optional_scope_id(&begin_while.content_name);
            let captures = compile_captures(raw_grammar, &begin_while.captures, visited)?;
            let begin_captures =
                compile_captures(raw_grammar, &begin_while.begin_captures, visited)?;
            let patterns = compile_nested_patterns(raw_grammar, &begin_while.patterns, visited)?;
            let while_captures =
                compile_captures(raw_grammar, &begin_while.while_captures, visited)?;

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
            if include.include.starts_with('#') {
                let repo_key = &include.include[1..];

                // Check for cycles
                if visited.contains(repo_key) {
                    return Ok(CompiledPattern::Include(CompiledIncludePattern {
                        patterns: vec![],
                    }));
                }

                // Check if repository entry exists
                if let Some(repo_entry) = raw_grammar.repository.get(repo_key) {
                    // Add to visited set BEFORE processing (for true cycle detection)
                    visited.insert(repo_key.to_string());

                    let patterns = match repo_entry {
                        RepositoryEntry::DirectArray(patterns) => {
                            let compiled = compile_nested_patterns(raw_grammar, patterns, visited)?;
                            compiled
                        }
                        RepositoryEntry::PatternContainer { patterns } => {
                            let compiled = compile_nested_patterns(raw_grammar, patterns, visited)?;
                            compiled
                        }
                        RepositoryEntry::DirectPattern(pattern) => {
                            let compiled_pattern = compile_pattern_with_visited(raw_grammar, pattern, visited)?;
                            vec![compiled_pattern]
                        }
                    };

                    // Remove from visited set AFTER processing (allow reuse in different branches)
                    visited.remove(repo_key);

                    Ok(CompiledPattern::Include(CompiledIncludePattern {
                        patterns,
                    }))
                } else {
                    // Repository key not found - gracefully handle by returning empty patterns
                    Ok(CompiledPattern::Include(CompiledIncludePattern {
                        patterns: vec![],
                    }))
                }
            } else {
                // TODO: External includes not implemented yet - return empty patterns
                Ok(CompiledPattern::Include(CompiledIncludePattern {
                    patterns: vec![],
                }))
            }
        }

        Pattern::Repository(repo) => Ok(CompiledPattern::Include(CompiledIncludePattern {
            patterns: compile_nested_patterns(raw_grammar, &repo.patterns, visited)?,
        })),
    }
}

/// Main pattern compilation function
fn compile_pattern(
    raw_grammar: &RawGrammar,
    pattern: &Pattern,
) -> Result<CompiledPattern, CompileError> {
    compile_pattern_with_visited(raw_grammar, pattern, &mut HashSet::new())
}

impl CompiledGrammar {
    pub fn from_raw_grammar(raw: RawGrammar) -> Result<Self, CompileError> {
        let scope_id = get_scope_id(&raw.scope_name).ok_or_else(|| CompileError::UnknownScope {
            scope: raw.scope_name.clone(),
        })?;

        // Compile the first line match regex if present
        let first_line_regex = raw
            .first_line_match
            .as_ref()
            .map(|pattern| {
                compile_and_validate_regex(pattern, false).map_err(|e| match e {
                    CompileError::InvalidRegex { pattern, error } => {
                        CompileError::InvalidRegex { pattern, error }
                    }
                    other => other,
                })
            })
            .transpose()?;

        // Compile all patterns
        let patterns = raw
            .patterns
            .iter()
            .map(|pattern| compile_pattern(&raw, pattern))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(CompiledGrammar {
            name: raw.name,
            display_name: raw.display_name,
            scope_name: raw.scope_name,
            file_types: raw.file_types,
            scope_id,
            patterns,
            first_line_regex,
        })
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

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn can_load_and_compile_all_shiki_grammars() {
        let entries = fs::read_dir("grammars-themes/packages/tm-grammars/grammars")
            .expect("Failed to read grammars directory");

        for entry in entries {
            let entry = entry.expect("Failed to read directory entry");
            let path = entry.path();
            RawGrammar::load_from_file(&path)
                .unwrap()
                .compile()
                .expect(&format!("Failed to compile grammar: {path:?}"));
        }
    }

    #[test]
    fn test_json_grammar_compilation() {
        let raw_grammar = RawGrammar::load_from_file("grammars-themes/packages/tm-grammars/grammars/json.json")
            .expect("Failed to load JSON grammar");

        let compiled_grammar = raw_grammar.compile()
            .expect("Failed to compile JSON grammar");

        // Snapshot the compiled grammar structure to ensure correctness
        insta::assert_debug_snapshot!(compiled_grammar);
    }

    #[test]
    fn test_markdown_grammar_compilation() {
        let raw_grammar = RawGrammar::load_from_file("grammars-themes/packages/tm-grammars/grammars/markdown.json")
            .expect("Failed to load Markdown grammar");

        let compiled_grammar = raw_grammar.compile()
            .expect("Failed to compile Markdown grammar");

        // Snapshot the compiled grammar structure to ensure correctness
        insta::assert_debug_snapshot!(compiled_grammar);
    }
}
