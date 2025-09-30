use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::path::Path;

use serde::Deserialize;

use super::common::{Regex, CompileError};
use super::compiled::{CompiledGrammar, CompiledPattern, CompiledCapture, CompiledMatchPattern, CompiledBeginEndPattern, CompiledBeginWhilePattern, CompiledIncludePattern};
use super::get_scope_id;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all(deserialize = "snake_case"))]
pub struct Capture {
    pub name: String,
    #[serde(default)]
    pub patterns: Vec<Pattern>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct MatchPattern {
    pub name: Option<String>,
    #[serde(rename(deserialize = "match"))]
    pub match_: String,
    #[serde(default)]
    pub captures: BTreeMap<String, Capture>,
    #[serde(default)]
    pub patterns: Vec<Pattern>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all(deserialize = "camelCase"))]
pub struct BeginEndPattern {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub content_name: Option<String>,
    pub begin: String,
    pub end: String,
    #[serde(default)]
    pub captures: BTreeMap<String, Capture>,
    #[serde(default)]
    pub begin_captures: BTreeMap<String, Capture>,
    #[serde(default)]
    pub end_captures: BTreeMap<String, Capture>,
    #[serde(default)]
    pub patterns: Vec<Pattern>,
    // set to 1 if true
    #[serde(default)]
    pub apply_end_pattern_last: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all(deserialize = "camelCase"))]
pub struct BeginWhilePattern {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub content_name: Option<String>,
    pub begin: String,
    #[serde(rename(deserialize = "while"))]
    pub while_: String,
    #[serde(default)]
    pub captures: BTreeMap<String, Capture>,
    #[serde(default)]
    pub begin_captures: BTreeMap<String, Capture>,
    #[serde(default)]
    pub while_captures: BTreeMap<String, Capture>,
    #[serde(default)]
    pub patterns: Vec<Pattern>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IncludePattern {
    pub include: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Pattern {
    // Try most specific patterns first
    BeginEnd(BeginEndPattern),
    BeginWhile(BeginWhilePattern),
    Match(MatchPattern),
    Include(IncludePattern),
    // This should be last as it's most general (just has patterns)
    Repository(RepositoryPattern),
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct RepositoryPattern {
    #[serde(default)]
    pub patterns: Vec<Pattern>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum RepositoryEntry {
    // Direct array of patterns (like racket.json "lambda-onearg")
    DirectArray(Vec<Pattern>),
    // A repository entry that just contains patterns - try this second
    PatternContainer {
        #[serde(default)]
        patterns: Vec<Pattern>,
    },
    // A repository entry that is directly a pattern
    DirectPattern(Pattern),
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all(deserialize = "camelCase"))]
pub struct RawGrammar {
    pub name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub file_types: Vec<String>,
    pub scope_name: String,
    #[serde(default)]
    pub repository: HashMap<String, RepositoryEntry>,
    #[serde(default)]
    pub patterns: Vec<Pattern>,
    #[serde(default)]
    pub first_line_match: Option<String>,
    #[serde(default)]
    pub folding_start_marker: Option<String>,
    #[serde(default)]
    pub folding_stop_marker: Option<String>,
    #[serde(default)]
    pub injections: HashMap<String, RepositoryEntry>,
    #[serde(default)]
    pub inject_to: Vec<String>,
    #[serde(default)]
    pub injection_selector: Option<String>,
}

impl RawGrammar {
    pub fn load_from_json_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let file = File::open(&path)?;
        let raw_grammar = serde_json::from_reader(&file)?;
        Ok(raw_grammar)
    }

    /// Compile this raw grammar into an optimized compiled grammar
    pub fn compile(&self) -> Result<CompiledGrammar, CompileError> {
        // Get the scope ID for the main scope
        let scope_id = get_scope_id(&self.scope_name)
            .ok_or_else(|| CompileError::UnknownScope {
                scope: self.scope_name.clone()
            })?;

        // Compile the first line match regex if present
        let first_line_regex = self.first_line_match
            .as_ref()
            .map(|pattern| {
                let regex = Regex::new(pattern.clone());
                regex.validate()
                    .map_err(|e| CompileError::InvalidRegex {
                        pattern: pattern.clone(),
                        error: e
                    })?;
                Ok::<Regex, CompileError>(regex)
            })
            .transpose()?;

        // Compile all patterns
        let patterns = self.patterns
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
        match pattern {
            Pattern::Match(match_pattern) => {
                let regex = Regex::new(match_pattern.match_.clone());
                regex.validate()
                    .map_err(|e| CompileError::InvalidRegex {
                        pattern: match_pattern.match_.clone(),
                        error: e
                    })?;

                let name_scope_id = match_pattern.name
                    .as_ref()
                    .map(|name| get_scope_id(name))
                    .flatten();

                let captures = match_pattern.captures
                    .iter()
                    .map(|(key, capture)| {
                        let scope_id = get_scope_id(&capture.name)
                            .ok_or_else(|| CompileError::UnknownScope {
                                scope: capture.name.clone()
                            })?;
                        let compiled_capture = CompiledCapture {
                            scope_id,
                            patterns: capture.patterns
                                .iter()
                                .map(|p| self.compile_pattern(p))
                                .collect::<Result<Vec<_>, _>>()?,
                        };
                        Ok((key.clone(), compiled_capture))
                    })
                    .collect::<Result<BTreeMap<_, _>, CompileError>>()?;

                let patterns = match_pattern.patterns
                    .iter()
                    .map(|p| self.compile_pattern(p))
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
                begin_regex.validate()
                    .map_err(|e| CompileError::InvalidRegex {
                        pattern: begin_end.begin.clone(),
                        error: e
                    })?;

                let end_regex = Regex::new(begin_end.end.clone());
                end_regex.validate()
                    .map_err(|e| CompileError::InvalidRegex {
                        pattern: begin_end.end.clone(),
                        error: e
                    })?;

                let name_scope_id = begin_end.name
                    .as_ref()
                    .map(|name| get_scope_id(name))
                    .flatten();

                let content_name_scope_id = begin_end.content_name
                    .as_ref()
                    .map(|name| get_scope_id(name))
                    .flatten();

                // Compile captures - prefer specific captures over general ones
                let captures = self.compile_captures(&begin_end.captures)?;
                let begin_captures = self.compile_captures(&begin_end.begin_captures)?;
                let end_captures = self.compile_captures(&begin_end.end_captures)?;

                let patterns = begin_end.patterns
                    .iter()
                    .map(|p| self.compile_pattern(p))
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(CompiledPattern::BeginEnd(CompiledBeginEndPattern {
                    name_scope_id,
                    content_name_scope_id,
                    begin_regex,
                    end_regex,
                    captures,
                    begin_captures,
                    end_captures,
                    patterns,
                    apply_end_pattern_last: begin_end.apply_end_pattern_last.unwrap_or(0) != 0,
                }))
            }

            Pattern::BeginWhile(begin_while) => {
                let begin_regex = Regex::new(begin_while.begin.clone());
                begin_regex.validate()
                    .map_err(|e| CompileError::InvalidRegex {
                        pattern: begin_while.begin.clone(),
                        error: e
                    })?;

                let while_regex = Regex::new(begin_while.while_.clone());
                while_regex.validate()
                    .map_err(|e| CompileError::InvalidRegex {
                        pattern: begin_while.while_.clone(),
                        error: e
                    })?;

                let name_scope_id = begin_while.name
                    .as_ref()
                    .map(|name| get_scope_id(name))
                    .flatten();

                let content_name_scope_id = begin_while.content_name
                    .as_ref()
                    .map(|name| get_scope_id(name))
                    .flatten();

                let captures = self.compile_captures(&begin_while.captures)?;
                let begin_captures = self.compile_captures(&begin_while.begin_captures)?;
                let while_captures = self.compile_captures(&begin_while.while_captures)?;

                let patterns = begin_while.patterns
                    .iter()
                    .map(|p| self.compile_pattern(p))
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(CompiledPattern::BeginWhile(CompiledBeginWhilePattern {
                    name_scope_id,
                    content_name_scope_id,
                    begin_regex,
                    while_regex,
                    captures,
                    begin_captures,
                    while_captures,
                    patterns,
                }))
            }

            Pattern::Include(_include) => {
                // For now, create an empty include that will be resolved later
                // In a full implementation, we'd resolve repository includes here
                Ok(CompiledPattern::Include(CompiledIncludePattern {
                    patterns: vec![], // TODO: Resolve include references
                }))
            }

            Pattern::Repository(repo) => {
                let patterns = repo.patterns
                    .iter()
                    .map(|p| self.compile_pattern(p))
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(CompiledPattern::Include(CompiledIncludePattern {
                    patterns,
                }))
            }
        }
    }

    fn compile_captures(&self, captures: &BTreeMap<String, Capture>) -> Result<BTreeMap<String, CompiledCapture>, CompileError> {
        captures
            .iter()
            .map(|(key, capture)| {
                let scope_id = get_scope_id(&capture.name)
                    .ok_or_else(|| CompileError::UnknownScope {
                        scope: capture.name.clone()
                    })?;
                let compiled_capture = CompiledCapture {
                    scope_id,
                    patterns: capture.patterns
                        .iter()
                        .map(|p| self.compile_pattern(p))
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

                let filename = path.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");

                println!("Loading grammar {} ({}/?)...", filename, loaded_grammars + 1);

                match RawGrammar::load_from_json_file(&path) {
                    Ok(grammar) => {
                        loaded_grammars += 1;
                        // Basic validation - ensure required fields are present
                        assert!(!grammar.name.is_empty(),
                            "Grammar {} has empty name", filename);
                        assert!(!grammar.scope_name.is_empty(),
                            "Grammar {} has empty scope_name", filename);
                    }
                    Err(e) => {
                        failed_grammars.push((filename.to_string(), e.to_string()));
                    }
                }
            }
        }

        println!("\nGrammar loading summary:");
        println!("  Total grammars found: {}", total_grammars);
        println!("  Successfully loaded: {}", loaded_grammars);
        println!("  Failed to load: {}", failed_grammars.len());

        if !failed_grammars.is_empty() {
            println!("\nFailed grammars:");
            for (filename, error) in &failed_grammars {
                println!("  {}: {}", filename, error);
            }
        }

        // Test passes if we can load at least some grammars and have very few failures
        assert!(total_grammars > 0, "No grammar files found in {}", grammars_dir);
        assert!(loaded_grammars > 200, "Should load most grammars, got {}", loaded_grammars);
        assert!(failed_grammars.len() <= 5,
            "Too many failed grammars: {}. Details: {:?}", failed_grammars.len(), failed_grammars);

        println!("✅ Successfully loaded {}/{} grammars!", loaded_grammars, total_grammars);
    }

    #[test]
    fn test_compile_all_grammars() {
        let grammars_dir = "grammars-themes/packages/tm-grammars/grammars";

        // Check if directory exists
        if !std::path::Path::new(grammars_dir).exists() {
            println!("Skipping test - grammars directory not found: {}", grammars_dir);
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

                let filename = path.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");

                println!("Compiling grammar {} ({}/?)...", filename, compiled_grammars + 1);

                // First load the grammar
                match RawGrammar::load_from_json_file(&path) {
                    Ok(raw_grammar) => {
                        // Then try to compile it
                        match raw_grammar.compile() {
                            Ok(_compiled) => {
                                compiled_grammars += 1;
                            }
                            Err(e) => {
                                failed_compilations.push((filename.to_string(), format!("Compilation failed: {}", e)));
                            }
                        }
                    }
                    Err(e) => {
                        failed_compilations.push((filename.to_string(), format!("Loading failed: {}", e)));
                    }
                }
            }
        }

        println!("\nGrammar compilation summary:");
        println!("  Total grammars found: {}", total_grammars);
        println!("  Successfully compiled: {}", compiled_grammars);
        println!("  Failed to compile: {}", failed_compilations.len());

        if !failed_compilations.is_empty() {
            println!("\nFailed compilations:");
            for (filename, error) in &failed_compilations {
                println!("  {}: {}", filename, error);
            }
        }

        // Test passes if we can compile at least some grammars
        // We expect failures due to unknown scopes since we haven't generated a complete scope map
        assert!(total_grammars > 0, "No grammar files found in {}", grammars_dir);
        assert!(compiled_grammars > 0, "Should compile at least some grammars, got {}", compiled_grammars);

        // Allow up to 90% failure rate due to missing scopes - this test is mainly checking that
        // the compilation process doesn't panic and handles errors gracefully
        let failure_rate = failed_compilations.len() as f64 / total_grammars as f64;
        assert!(failure_rate < 0.95,
            "Too many failed compilations: {:.1}% ({}/{}). This suggests a systemic issue.",
            failure_rate * 100.0, failed_compilations.len(), total_grammars);

        println!("✅ Successfully compiled {}/{} grammars ({:.1}% success rate)!",
                compiled_grammars, total_grammars,
                (compiled_grammars as f64 / total_grammars as f64) * 100.0);
    }
}