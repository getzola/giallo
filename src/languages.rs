use std::collections::HashMap;

use once_cell::sync::Lazy;
use tree_sitter_highlight::HighlightConfiguration;

pub(crate) const SCOPES: &[&str] = &[
    "constant",
    "type",
    "type.builtin",
    "property",
    "comment",
    "constructor",
    "function",
    "label",
    "keyword",
    "string",
    "variable",
    "variable.other.member",
    "operator",
    "attribute",
    "escape",
    "embedded",
    "symbol",
];

// TODO: feature flag each language
#[derive(Eq, PartialEq, Hash, Clone, Copy, Debug)]
pub(crate) enum SupportedLanguage {
    #[cfg(feature = "lang-rust")]
    Rust,
}

impl SupportedLanguage {
    pub fn from_extension(extension: &str) -> Option<Self> {
        // TODO: check whether this linear search makes a difference
        for options in BUILTIN_LANGUAGES {
            if options.extensions.contains(&extension) {
                return Some(options.id);
            }
        }
        None
    }
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub(crate) struct LanguageOptions {
    id: SupportedLanguage,
    /// The factory to return the language. We can't call it const context so we
    /// just keep the fn to call later
    language: fn() -> tree_sitter::Language,
    /// The tree-sitter
    highlight_query: &'static str,
    /// File name extensions that determine the language, eg `["rs"]` for Rust
    extensions: &'static [&'static str],
}

// TODO: we only need that for extensions really, check if a hashmap extension -> lang is better/faster
const BUILTIN_LANGUAGES: &[LanguageOptions] = &[
    #[cfg(feature = "lang-rust")]
    LanguageOptions {
        id: SupportedLanguage::Rust,
        language: tree_sitter_rust::language,
        highlight_query: tree_sitter_rust::HIGHLIGHT_QUERY,
        extensions: &["rs"],
    },
];

// TODO: Should we only build the languages we use instead of all of them upon first hit?
//  Eg make it a thread_local with refcell? In practice most people will only use <5 languages
pub(crate) static LANGUAGES: Lazy<HashMap<SupportedLanguage, HighlightConfiguration>> =
    Lazy::new(|| {
        let mut languages = HashMap::new();

        for lang in BUILTIN_LANGUAGES {
            let mut config =
                HighlightConfiguration::new((lang.language)(), lang.highlight_query, "", "")
                    .unwrap();
            config.configure(SCOPES);
            languages.insert(lang.id, config);
        }

        languages
    });

#[cfg(test)]
mod tests {
    use super::*;

    // TODO: add tests making sure we can load every language
}
