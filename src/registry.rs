use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::grammars::{CompiledGrammar, GrammarId, RawGrammar};
use crate::themes::{CompiledTheme, RawTheme};
use crate::tokenizer::Tokenizer;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Registry {
    // Vector of compiled grammars for ID-based access
    grammars: Vec<CompiledGrammar>,
    // grammar name -> grammar ID lookup for string-based access
    grammar_names: HashMap<String, GrammarId>,
    // name given by user -> theme
    themes: HashMap<String, CompiledTheme>,
}

impl Registry {
    pub fn add_grammar_from_str(
        &mut self,
        grammar: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let raw_grammar = RawGrammar::load_from_str(grammar)?;
        let grammar_id = GrammarId(self.grammars.len() as u16);
        let grammar = CompiledGrammar::from_raw_grammar(raw_grammar, grammar_id)?;
        let grammar_name = grammar.name.clone();
        self.grammars.push(grammar);
        self.grammar_names.insert(grammar_name, grammar_id);
        Ok(())
    }

    pub fn add_grammar_from_path(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let raw_grammar = RawGrammar::load_from_file(path)?;
        let grammar_id = GrammarId(self.grammars.len() as u16);
        let grammar = CompiledGrammar::from_raw_grammar(raw_grammar, grammar_id)?;
        let grammar_name = grammar.name.clone();
        self.grammars.push(grammar);
        self.grammar_names.insert(grammar_name, grammar_id);
        Ok(())
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

    pub(crate) fn tokenize(&self, lang: &str, content: &str) {
        // 1. Get grammar by language name
        if let Some(grammar) = self.get_grammar_by_name(lang) {
            // 2. Create tokenizer with the grammar
            let mut tokenizer = Tokenizer::new(grammar);

            // 3. Tokenize the content
            match tokenizer.tokenize_string(content) {
                Ok(tokens) => {
                    // Process tokens (placeholder for actual implementation)
                    println!("Tokenized {} lines", tokens.len());
                }
                Err(e) => {
                    eprintln!("Tokenization error: {:?}", e);
                }
            }
        } else {
            eprintln!("Grammar not found: {}", lang);
        }
    }

    fn get_grammar_id(&self, name: &str) -> Option<GrammarId> {
        self.grammar_names.get(name).cloned()
    }

    fn get_grammar_by_name(&self, name: &str) -> Option<&CompiledGrammar> {
        let id = self.grammar_names.get(name)?;
        self.grammars.get(id.id())
    }

    fn get_grammar_by_id(&self, id: GrammarId) -> Option<&CompiledGrammar> {
        self.grammars.get(id.id())
    }

    /// Dump the current Registry to compressed MessagePack format at the given file path
    #[cfg(feature = "dump")]
    pub fn dump_to_file(&self, path: impl AsRef<Path>) -> Result<(), Box<dyn std::error::Error>> {
        use flate2::{Compression, write::GzEncoder};
        use std::io::Write;

        let msgpack_data = rmp_serde::to_vec(self)?;
        let file = fs::File::create(path)?;
        let mut encoder = GzEncoder::new(file, Compression::default());
        encoder.write_all(&msgpack_data)?;
        encoder.finish()?;

        Ok(())
    }

    /// Load a Registry from compressed MessagePack format at the given file path
    #[cfg(feature = "dump")]
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self, Box<dyn std::error::Error>> {
        use flate2::read::GzDecoder;
        use std::io::Read;

        let compressed_data = fs::read(path)?;
        let mut decoder = GzDecoder::new(&compressed_data[..]);
        let mut msgpack_data = Vec::new();
        decoder.read_to_end(&mut msgpack_data)?;

        let registry: Registry = rmp_serde::from_slice(&msgpack_data)?;
        Ok(registry)
    }
}
