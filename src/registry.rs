use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::grammars::{CompiledGrammar, GlobalRuleRef, GrammarId, Match, RawGrammar, Rule};
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
        self.grammar_id_by_scope_name
            .insert(grammar_name, grammar_id);
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

    pub fn tokenize(
        &self,
        lang: &str,
        content: &str,
    ) -> Result<Vec<Vec<Token>>, Box<dyn std::error::Error>> {
        // 1. Get grammar ID by language name
        if let Some(grammar_id) = self.get_grammar_id(lang) {
            // 2. Create tokenizer with the grammar ID and all grammars
            let mut tokenizer = Tokenizer::new(grammar_id, &self);
            Ok(tokenizer.tokenize_string(content).unwrap())
        } else {
            Err("Grammar not found".into())
        }
    }

    pub fn link_grammars(&mut self) {
        let grammar_names_ptr = &self.grammar_id_by_scope_name as *const HashMap<String, GrammarId>;
        let grammars_ptr = &self.grammars as *const Vec<CompiledGrammar>;
        for grammar in self.grammars.iter_mut() {
            // We only modify the content of the current grammar being iterated
            unsafe {
                grammar.resolve_external_references(&*grammar_names_ptr, &*grammars_ptr);
            }
        }
    }

    fn get_rule_patterns(
        &self,
        rule_ref: GlobalRuleRef,
        visited: &mut HashSet<GlobalRuleRef>,
    ) -> Vec<(GlobalRuleRef, &str)> {
        let mut out = vec![];
        if visited.contains(&rule_ref) {
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
