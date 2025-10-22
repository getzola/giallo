use crate::grammars::{CompiledGrammar, RawGrammar};
use crate::themes::{CompiledTheme, RawTheme};
use crate::tokenizer::Tokenizer;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Registry {
    // compiled_grammar.name -> compiled_grammar
    grammars: HashMap<String, CompiledGrammar>,
    // name given by user -> theme
    themes: HashMap<String, CompiledTheme>,
}

impl Registry {
    pub fn add_grammar_from_str(
        &mut self,
        grammar: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let raw_grammar = RawGrammar::load_from_str(grammar)?;
        let compiled_grammar = raw_grammar.compile()?;
        self.grammars
            .insert(compiled_grammar.name.clone(), compiled_grammar);
        Ok(())
    }

    pub fn add_grammar_from_path(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let raw_grammar = RawGrammar::load_from_file(path)?;
        let compiled_grammar = raw_grammar.compile()?;
        self.grammars
            .insert(compiled_grammar.name.clone(), compiled_grammar);
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
        if let Some(grammar) = self.grammars.get(lang) {
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

    /// Dump the current Registry to compressed MessagePack format at the given file path
    #[cfg(feature = "dump")]
    pub fn dump_to_file(&self, path: impl AsRef<Path>) -> Result<(), Box<dyn std::error::Error>> {
        use flate2::{write::GzEncoder, Compression};
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
