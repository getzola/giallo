use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::grammars::{
    CompiledGrammar, CompiledInjectionMatcher, GlobalRuleRef, GrammarId, InjectionPrecedence,
    Match, NO_OP_GLOBAL_RULE_REF, ROOT_RULE_ID, RawGrammar, Rule,
};
use crate::highlight::TokenWithStyle;
use crate::scope::Scope;
use crate::scope::ScopeRepository;
use crate::themes::{CompiledTheme, RawTheme};
use crate::tokenizer::{Token, Tokenizer};

// TODO: once theme matching works, we will create scopes in all rules + themes when compiling
// TODO: and add that to the dump. This means we will need to write only to the scope registry only
// TODO: for runtime scopes, eg capturing names
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Dump {
    registry: Registry,
    scope_repo: ScopeRepository,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct HighlightOptions<'a> {
    pub lang: &'a str,
    pub theme: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Registry {
    // Vector of compiled grammars for ID-based access
    pub(crate) grammars: Vec<CompiledGrammar>,
    // grammar scope name -> grammar ID lookup for string-based access
    // this is used internally only
    grammar_id_by_scope_name: HashMap<String, GrammarId>,
    // grammar name -> grammar ID lookup for string-based access
    // this is the name that end user will refer to
    grammar_id_by_name: HashMap<String, GrammarId>,
    // name given by user -> theme
    themes: HashMap<String, CompiledTheme>,
    // grammar ID quick lookup to find which external grammars can be loaded for each grammar
    // Most of the inner vecs will be empty since few grammars use injectTo
    injections_by_grammar: Vec<HashSet<GrammarId>>,
}

impl Registry {
    fn add_grammar_from_raw(
        &mut self,
        raw_grammar: RawGrammar,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let grammar_id = GrammarId(self.grammars.len() as u16);
        let grammar = CompiledGrammar::from_raw_grammar(raw_grammar, grammar_id)?;
        let grammar_name = grammar.name.clone();
        let grammar_scope_name = grammar.scope_name.clone();
        self.grammars.push(grammar);
        self.grammar_id_by_scope_name
            .insert(grammar_scope_name, grammar_id);
        self.grammar_id_by_name.insert(grammar_name, grammar_id);
        self.injections_by_grammar.push(HashSet::new());
        Ok(())
    }

    pub fn add_grammar_from_str(
        &mut self,
        grammar: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let raw_grammar = RawGrammar::load_from_str(grammar)?;
        self.add_grammar_from_raw(raw_grammar)
    }

    pub fn add_grammar_from_path(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let raw_grammar = RawGrammar::load_from_file(path)?;
        self.add_grammar_from_raw(raw_grammar)
    }

    pub fn add_alias(&mut self, grammar_name: &str, alias: &str) {
        if let Some(grammar_id) = self.grammar_id_by_name.get(grammar_name) {
            self.grammar_id_by_name
                .insert(alias.to_string(), *grammar_id);
        }
    }

    pub fn add_theme_from_str(
        &mut self,
        name: &str,
        content: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let raw_theme: RawTheme = serde_json::from_str(content)?;
        let compiled_theme = raw_theme.compile()?;
        self.themes.insert(name.to_string(), compiled_theme);
        Ok(())
    }

    pub fn add_theme_from_path(
        &mut self,
        name: &str,
        path: impl AsRef<Path>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let raw_theme = RawTheme::load_from_file(path)?;
        let compiled_theme = raw_theme.compile()?;
        self.themes.insert(name.to_string(), compiled_theme);
        Ok(())
    }

    pub(crate) fn tokenize(
        &self,
        grammar_id: GrammarId,
        content: &str,
    ) -> Result<Vec<Vec<Token>>, Box<dyn std::error::Error>> {
        let mut tokenizer = Tokenizer::new(grammar_id, &self);
        match tokenizer.tokenize_string(content) {
            Ok(tokens) => Ok(tokens),
            Err(e) => Err(Box::new(e)),
        }
    }

    pub fn highlight(
        &self,
        content: &str,
        options: HighlightOptions,
    ) -> Result<Vec<Vec<TokenWithStyle>>, Box<dyn std::error::Error>> {
        let grammar_id = *self
            .grammar_id_by_name
            .get(options.lang)
            .ok_or_else(|| format!("no grammar found for {}", options.lang))?;
        let theme_id = self
            .themes
            .get(options.theme)
            .ok_or_else(|| format!("no themes found for {}", options.theme))?;

        let tokens = self.tokenize(grammar_id, content)?;
        let mut out = Vec::with_capacity(tokens.len());

        for line_tokens in tokens {
            //TODO
        }

        Ok(out)
    }

    pub fn link_grammars(&mut self) {
        let grammar_names_ptr = &self.grammar_id_by_scope_name as *const HashMap<String, GrammarId>;
        let grammars_ptr = &self.grammars as *const Vec<CompiledGrammar>;
        for grammar in self.grammars.iter_mut() {
            // We only modify the content of the current grammar being iterated
            unsafe {
                grammar.resolve_external_references(&*grammar_names_ptr, &*grammars_ptr);
            }

            for inject_to in &grammar.inject_to {
                if let Some(g_id) = self.grammar_id_by_name.get(inject_to) {
                    self.injections_by_grammar[g_id.as_index()].insert(grammar.id);
                }
            }
        }
    }

    fn get_rule_patterns(
        &self,
        rule_ref: GlobalRuleRef,
        visited: &mut HashSet<GlobalRuleRef>,
    ) -> Vec<(GlobalRuleRef, &str)> {
        let mut out = vec![];
        if visited.contains(&rule_ref) || rule_ref == NO_OP_GLOBAL_RULE_REF {
            return out;
        }
        visited.insert(rule_ref);

        let grammar = &self.grammars[rule_ref.grammar];
        let rule = &grammar.rules[rule_ref.rule];
        match rule {
            Rule::Match(Match { regex_id, .. }) => {
                if let Some(regex_id) = regex_id {
                    let re = &grammar.regexes[*regex_id];
                    out.push((rule_ref, re.pattern()));
                }
            }
            Rule::IncludeOnly(i) => {
                out.extend(self.get_pattern_set_data(&i.patterns, visited));
            }
            Rule::BeginEnd(b) => out.push((rule_ref, grammar.regexes[b.begin].pattern())),
            Rule::BeginWhile(b) => out.push((rule_ref, grammar.regexes[b.begin].pattern())),
            Rule::Noop => {}
        }
        out
    }

    fn get_pattern_set_data(
        &self,
        rule_refs: &[GlobalRuleRef],
        visited: &mut HashSet<GlobalRuleRef>,
    ) -> Vec<(GlobalRuleRef, &str)> {
        let mut out = Vec::new();

        for r in rule_refs {
            let rule_patterns = self.get_rule_patterns(*r, visited);
            out.extend(rule_patterns);
        }

        out
    }

    pub(crate) fn collect_patterns(&self, rule_ref: GlobalRuleRef) -> Vec<(GlobalRuleRef, &str)> {
        let grammar = &self.grammars[rule_ref.grammar];
        let base_patterns: &[GlobalRuleRef] = match &grammar.rules[rule_ref.rule] {
            Rule::IncludeOnly(a) => &a.patterns,
            Rule::BeginEnd(a) => &a.patterns,
            Rule::BeginWhile(a) => &a.patterns,
            Rule::Match(_) | Rule::Noop => &[],
        };
        let mut visited = HashSet::new();
        self.get_pattern_set_data(&base_patterns, &mut visited)
    }

    pub(crate) fn collect_injection_patterns(
        &self,
        target_grammar_id: GrammarId,
        scope_stack: &[Scope],
    ) -> Vec<(InjectionPrecedence, Vec<(GlobalRuleRef, &str)>)> {
        let mut result = Vec::new();

        for (matchers, rule) in &self.grammars[target_grammar_id].injections {
            for matcher in matchers {
                if matcher.matches(scope_stack) {
                    let patterns = self.collect_patterns(*rule);
                    result.push((matcher.precedence(), patterns));
                }
            }
        }

        // Get external injection grammars for the target grammar
        for &injector_id in &self.injections_by_grammar[target_grammar_id.as_index()] {
            let injector = &self.grammars[injector_id];

            if let Some(matcher) = injector
                .injection_selector
                .iter()
                .find(|matcher| matcher.matches(scope_stack))
            {
                // in injector grammars, there should be just a root rule and we inject it all
                let patterns = self.collect_patterns(GlobalRuleRef {
                    grammar: injector_id,
                    rule: ROOT_RULE_ID,
                });
                result.push((matcher.precedence(), patterns));
            }
        }

        result.sort_by_key(|(precedence, _)| match precedence {
            InjectionPrecedence::Left => -1,
            InjectionPrecedence::Right => 1,
        });

        result
    }

    fn get_grammar_id(&self, name: &str) -> Option<GrammarId> {
        self.grammar_id_by_scope_name.get(name).cloned()
    }

    #[cfg(feature = "dump")]
    pub fn dump_to_file(&self, path: impl AsRef<Path>) -> Result<(), Box<dyn std::error::Error>> {
        use crate::scope::lock_global_scope_repo;
        use flate2::{Compression, write::GzEncoder};
        use std::io::Write;

        // Create a Dump containing both Registry and current ScopeRepository
        let scope_repo = lock_global_scope_repo().clone();
        let dump = Dump {
            registry: self.clone(),
            scope_repo,
        };

        let msgpack_data = rmp_serde::to_vec(&dump)?;
        let file = std::fs::File::create(path)?;
        let mut encoder = GzEncoder::new(file, Compression::default());
        encoder.write_all(&msgpack_data)?;
        encoder.finish()?;

        Ok(())
    }

    #[cfg(feature = "dump")]
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self, Box<dyn std::error::Error>> {
        use crate::scope::replace_global_scope_repo;
        use flate2::read::GzDecoder;
        use std::io::Read;

        let compressed_data = std::fs::read(path)?;
        let mut decoder = GzDecoder::new(&compressed_data[..]);
        let mut msgpack_data = Vec::new();
        decoder.read_to_end(&mut msgpack_data)?;

        let dump: Dump = rmp_serde::from_slice(&msgpack_data)?;
        replace_global_scope_repo(dump.scope_repo);

        Ok(dump.registry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::highlight::{Highlighter, TokenWithStyle};
    use std::fs;
    use std::path::PathBuf;

    /// Load a registry with all grammars from grammars-themes and the vitesse-black theme
    fn load_grammars_themes_registry() -> Result<(Registry, Highlighter), Box<dyn std::error::Error>>
    {
        let mut registry = Registry::default();

        // Load all grammars from grammars-themes
        let grammars_dir = PathBuf::from("grammars-themes/packages/tm-grammars/grammars");
        if !grammars_dir.exists() {
            return Err("grammars-themes directory not found".into());
        }

        for entry in fs::read_dir(&grammars_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Err(e) = registry.add_grammar_from_path(&path) {
                    eprintln!("Warning: Failed to load grammar {}: {}", path.display(), e);
                }
            }
        }

        // Link grammars to resolve includes
        registry.link_grammars();

        // Load vitesse-black theme
        let theme_path =
            PathBuf::from("grammars-themes/packages/tm-themes/themes/vitesse-black.json");
        if !theme_path.exists() {
            return Err("vitesse-black theme not found".into());
        }

        registry.add_theme_from_path("vitesse-black", &theme_path)?;

        let theme = registry
            .themes
            .get("vitesse-black")
            .ok_or("Failed to load vitesse-black theme")?;
        let highlighter = Highlighter::new(theme);

        Ok((registry, highlighter))
    }

    /// Format highlighted tokens as snapshot string matching grammars-themes format
    fn format_highlighted_tokens(
        highlighted_tokens: &[Vec<TokenWithStyle>],
        content: &str,
    ) -> String {
        let lines: Vec<&str> = content.lines().collect();
        let mut result = String::new();

        for (line_idx, line_tokens) in highlighted_tokens.iter().enumerate() {
            if line_idx >= lines.len() {
                break;
            }

            let line_content = lines[line_idx];

            for token in line_tokens {
                let token_content = &line_content[token.range.start..token.range.end];
                let hex_color = token.style.foreground.as_hex();
                // Format: {hex_color_15_chars}{content}
                result.push_str(&format!("{:<15}{}\n", hex_color, token_content));
            }
        }

        result
    }

    /// Get all sample files with their corresponding grammar names
    fn get_sample_files() -> Result<Vec<(String, PathBuf)>, Box<dyn std::error::Error>> {
        let samples_dir = PathBuf::from("grammars-themes/samples");
        if !samples_dir.exists() {
            return Err("samples directory not found".into());
        }

        let mut samples = Vec::new();
        for entry in fs::read_dir(samples_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("sample") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    samples.push((stem.to_string(), path));
                }
            }
        }

        samples.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(samples)
    }

    // #[test]
    // fn test_all_grammar_snapshots() {
    //     // Load registry and highlighter
    //     let (registry, highlighter) = match load_grammars_themes_registry() {
    //         Ok(result) => result,
    //         Err(e) => {
    //             panic!("Failed to load grammars-themes data: {}", e);
    //         }
    //     };
    //
    //     // Get all sample files
    //     let sample_files = match get_sample_files() {
    //         Ok(files) => files,
    //         Err(e) => {
    //             panic!("Failed to get sample files: {}", e);
    //         }
    //     };
    //
    //     let mut total_tested = 0;
    //     let mut passed = 0;
    //     let mut failed = Vec::new();
    //     let mut skipped = Vec::new();
    //
    //     println!("Running snapshot tests for {} samples", sample_files.len());
    //
    //     for (grammar_name, sample_path) in sample_files {
    //         // Read sample content
    //         let sample_content = match fs::read_to_string(&sample_path) {
    //             Ok(content) => content,
    //             Err(e) => {
    //                 eprintln!(
    //                     "Warning: Failed to read sample {}: {}",
    //                     sample_path.display(),
    //                     e
    //                 );
    //                 skipped.push(grammar_name);
    //                 continue;
    //             }
    //         };
    //
    //         // Check if snapshot file exists
    //         let snapshot_path = PathBuf::from(format!(
    //             "grammars-themes/test/__snapshots__/{}.txt",
    //             grammar_name
    //         ));
    //         let expected_snapshot = match fs::read_to_string(&snapshot_path) {
    //             Ok(content) => content,
    //             Err(_) => {
    //                 eprintln!("Warning: No snapshot file for grammar {}", grammar_name);
    //                 skipped.push(grammar_name);
    //                 continue;
    //             }
    //         };
    //
    //         println!("{grammar_name}");
    //         // Tokenize with giallo
    //         let tokens = match registry.tokenize(&grammar_name, &sample_content) {
    //             Ok(tokens) => tokens,
    //             Err(e) => {
    //                 eprintln!("Warning: Failed to tokenize {}: {}", grammar_name, e);
    //                 skipped.push(grammar_name);
    //                 continue;
    //             }
    //         };
    //
    //         // Apply theme
    //         let highlighted_tokens = highlighter.highlight_tokens(&tokens);
    //
    //         // Format as snapshot
    //         let actual_snapshot = format_highlighted_tokens(&highlighted_tokens, &sample_content);
    //
    //         total_tested += 1;
    //
    //         // Compare with expected snapshot
    //         if actual_snapshot.trim() == expected_snapshot.trim() {
    //             passed += 1;
    //         } else {
    //             // Print first few mismatches for debugging
    //             if failed.len() < 3 {
    //                 println!("\n❌ MISMATCH: {}", grammar_name);
    //                 let actual_lines: Vec<&str> = actual_snapshot.lines().collect();
    //                 let expected_lines: Vec<&str> = expected_snapshot.lines().collect();
    //
    //                 for (i, (actual, expected)) in
    //                     actual_lines.iter().zip(expected_lines.iter()).enumerate()
    //                 {
    //                     if actual != expected {
    //                         println!("  Line {}: Expected: {:?}", i + 1, expected);
    //                         println!("  Line {}: Actual:   {:?}", i + 1, actual);
    //                         break;
    //                     }
    //                 }
    //             }
    //
    //             failed.push((grammar_name.clone(), actual_snapshot, expected_snapshot));
    //         }
    //     }
    //
    //     // Print summary
    //     println!("\n📊 SNAPSHOT TEST SUMMARY:");
    //     println!("  Total tested: {}", total_tested);
    //     println!(
    //         "  Passed: {} ({}%)",
    //         passed,
    //         if total_tested > 0 {
    //             passed * 100 / total_tested
    //         } else {
    //             0
    //         }
    //     );
    //     println!(
    //         "  Failed: {} ({}%)",
    //         failed.len(),
    //         if total_tested > 0 {
    //             failed.len() * 100 / total_tested
    //         } else {
    //             0
    //         }
    //     );
    //     println!("  Skipped: {} (no sample or snapshot)", skipped.len());
    //
    //     if !failed.is_empty() {
    //         println!("\n❌ FAILED GRAMMARS:");
    //         for (grammar, _, _) in &failed {
    //             println!("  - {}", grammar);
    //         }
    //
    //         // Fail the test if there are mismatches
    //         panic!("{} grammar snapshot(s) failed validation", failed.len());
    //     } else if total_tested == 0 {
    //         panic!("No grammar samples were tested - check paths to grammars-themes");
    //     } else {
    //         println!("\n✅ All {} grammar snapshots passed!", total_tested);
    //     }
    // }
}
