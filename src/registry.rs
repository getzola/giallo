use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, GialloResult};
use crate::grammars::{
    BASE_GLOBAL_RULE_REF, CompiledGrammar, GlobalRuleRef, GrammarId, InjectionPrecedence, Match,
    NO_OP_GLOBAL_RULE_REF, ROOT_RULE_ID, RawGrammar, Rule,
};
use crate::highlight::{HighlightedText, Highlighter, MergingOptions};

use crate::scope::Scope;
#[cfg(feature = "dump")]
use crate::scope::ScopeRepository;
use crate::themes::{CompiledTheme, RawTheme, ThemeVariant};
use crate::tokenizer::{Token, Tokenizer};

#[cfg(feature = "dump")]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Dump {
    registry: Registry,
    scope_repo: ScopeRepository,
}

// We always include the data but if the `dump` feature is not enabled, we can't actually
// use it
#[allow(dead_code)]
const BUILTIN_DATA: &[u8] = include_bytes!("../builtin.msgpack");

/// The default grammar name, where nothing is highlighted
pub const PLAIN_GRAMMAR_NAME: &str = "plain";

/// Options for highlighting by the registry, NOT rendering.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HighlightOptions<'a> {
    pub(crate) lang: &'a str,
    pub(crate) theme: ThemeVariant<&'a str>,
    pub(crate) merge_whitespaces: bool,
    pub(crate) merge_same_style_tokens: bool,
    pub(crate) fallback_to_plain: bool,
}

impl<'a> HighlightOptions<'a> {
    /// Creates a new highlight options with the given language and theme.
    ///
    /// For dual themes (light/dark), `merge_same_style_tokens` is automatically
    /// disabled since tokens might get merged differently depending on theme.
    /// Even if you set it back to `true`, it will be ignored when rendering.
    pub fn new(lang: &'a str, theme: ThemeVariant<&'a str>) -> Self {
        let merge_same_style_tokens = matches!(theme, ThemeVariant::Single(_));
        Self {
            lang,
            theme,
            merge_same_style_tokens,
            merge_whitespaces: true,
            fallback_to_plain: false,
        }
    }

    /// Whitespace tokens are merged with the next non-ws tokens.
    pub fn merge_whitespace(mut self, value: bool) -> Self {
        self.merge_whitespaces = value;
        self
    }

    /// Merges tokens with the same style into a single token
    pub fn merge_same_style_tokens(mut self, value: bool) -> Self {
        self.merge_same_style_tokens = value;
        self
    }

    /// Whether to fallback to the plain grammar if the requested
    /// grammar is not found.
    pub fn fallback_to_plain(mut self, value: bool) -> Self {
        self.fallback_to_plain = value;
        self
    }
}

/// Highlighted code with language, theme, and tokens
#[derive(Debug, Clone)]
pub struct HighlightedCode<'a> {
    /// The requested language
    pub language: &'a str,
    /// The compiled theme(s) we got from the registry based on the requested themes
    pub theme: ThemeVariant<&'a CompiledTheme>,
    /// The generated tokens. Each line is a Vector
    pub tokens: Vec<Vec<HighlightedText>>,
}

#[inline]
pub(crate) fn normalize_string(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// The main struct in giallo.
///
/// Holds all the grammars and themes and is responsible for highlighting a text. It is not
/// responsible for actually rendering those highlighted texts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Registry {
    // Vector of compiled grammars for ID-based access
    pub(crate) grammars: Vec<CompiledGrammar>,
    // grammar scope name -> grammar ID lookup for string-based access
    // this is used internally only by grammars
    grammar_id_by_scope_name: HashMap<String, GrammarId>,
    // grammar name -> grammar ID lookup for string-based access
    // this is the name that end user will refer to
    pub(crate) grammar_id_by_name: HashMap<String, GrammarId>,
    // name given by user -> theme
    themes: HashMap<String, CompiledTheme>,
    // grammar ID quick lookup to find which external grammars can be loaded for each grammar
    // Most of the inner vecs will be empty since few grammars use injectTo
    injections_by_grammar: Vec<HashSet<GrammarId>>,
    // Once a registry has linked grammars, it's not possible to replace existing grammars.
    linked: bool,
}

impl Registry {
    fn add_grammar_from_raw(&mut self, raw_grammar: RawGrammar) -> GialloResult<()> {
        if self.linked && self.grammar_id_by_name.contains_key(&raw_grammar.name) {
            return Err(Error::ReplacingGrammarPostLinking(
                raw_grammar.name.to_owned(),
            ));
        }
        let grammar_id = GrammarId(self.grammars.len() as u16);
        let grammar = CompiledGrammar::from_raw_grammar(raw_grammar, grammar_id);
        let grammar_name = grammar.name.clone();
        let grammar_scope_name = grammar.scope_name.clone();
        self.grammars.push(grammar);
        self.grammar_id_by_scope_name
            .insert(grammar_scope_name, grammar_id);
        self.grammar_id_by_name.insert(grammar_name, grammar_id);
        self.injections_by_grammar.push(HashSet::new());
        Ok(())
    }

    /// Reads the file and add it as a grammar.
    pub fn add_grammar_from_path(&mut self, path: impl AsRef<Path>) -> GialloResult<()> {
        let raw_grammar = RawGrammar::load_from_file(path)?;
        self.add_grammar_from_raw(raw_grammar)
    }

    /// Adds an empty grammar that will not match any token. Useful as a fallback if the grammar is not found.
    ///
    /// It will get the `plain` grammar name.
    pub fn add_plain_grammar(&mut self, aliases: &[&str]) -> GialloResult<()> {
        let raw = RawGrammar {
            name: PLAIN_GRAMMAR_NAME.to_owned(),
            scope_name: PLAIN_GRAMMAR_NAME.to_owned(),
            ..Default::default()
        };
        self.add_grammar_from_raw(raw)?;
        for alias in aliases {
            self.add_alias(PLAIN_GRAMMAR_NAME, alias);
        }
        Ok(())
    }

    /// Adds an alias for the given grammar
    pub fn add_alias(&mut self, grammar_name: &str, alias: &str) {
        if let Some(grammar_id) = self.grammar_id_by_name.get(grammar_name) {
            self.grammar_id_by_name
                .insert(alias.to_string(), *grammar_id);
        }
    }

    /// Reads the file and add it as a theme.
    pub fn add_theme_from_path(&mut self, path: impl AsRef<Path>) -> GialloResult<()> {
        let raw_theme = RawTheme::load_from_file(path)?;
        let compiled_theme = raw_theme.compile()?;
        self.themes
            .insert(compiled_theme.name.to_string(), compiled_theme);
        Ok(())
    }

    /// Generates CSS stylesheet content for a theme.
    /// All classes will have the given prefix.
    ///
    /// Use this with `HtmlRenderer::css_class_prefix` to enable CSS-based theming,
    /// which allows JavaScript-based theme switching.
    pub fn generate_css(&self, theme_name: &str, prefix: &str) -> GialloResult<String> {
        let theme = self
            .themes
            .get(theme_name)
            .ok_or_else(|| Error::ThemeNotFound(theme_name.to_string()))?;
        Ok(crate::themes::css::generate_css(theme, prefix))
    }

    pub(crate) fn tokenize(
        &self,
        grammar_id: GrammarId,
        content: &str,
    ) -> GialloResult<Vec<Vec<Token>>> {
        let mut tokenizer = Tokenizer::new(grammar_id, self);
        let tokens = tokenizer
            .tokenize_string(content)
            .map_err(Error::TokenizeRegex)?;
        Ok(tokens)
    }

    /// Checks whether the given lang is available in the registry with its grammar name
    /// or aliases
    pub fn contains_grammar(&self, name: &str) -> bool {
        self.grammar_id_by_name.contains_key(name)
    }

    /// Checks whether the given theme is available in the registry
    pub fn contains_theme(&self, name: &str) -> bool {
        self.themes.contains_key(name)
    }

    /// The main entry point for the actual giallo usage.
    ///
    /// This returns the raw output of the tokenizer + theme matching. It's up to you to use
    /// a provided renderer or to use your own afterwards.
    ///
    /// Make sure `link_grammars` is called before calling `highlight`, this will error otherwise.
    pub fn highlight<'a>(
        &'a self,
        content: &str,
        options: HighlightOptions<'a>,
    ) -> GialloResult<HighlightedCode<'a>> {
        if !self.linked {
            return Err(Error::UnlinkedGrammars);
        }
        let grammar_id = *self
            .grammar_id_by_name
            .get(options.lang)
            .or_else(|| {
                if options.fallback_to_plain {
                    self.grammar_id_by_name.get(PLAIN_GRAMMAR_NAME)
                } else {
                    None
                }
            })
            .ok_or_else(|| Error::GrammarNotFound(options.lang.to_string()))?;

        let normalized_content = normalize_string(content);
        let tokens = self.tokenize(grammar_id, &normalized_content)?;

        let merging_options = MergingOptions {
            merge_whitespaces: options.merge_whitespaces,
            merge_same_style_tokens: options.merge_same_style_tokens,
        };

        match &options.theme {
            ThemeVariant::Single(theme_name) => {
                let theme = self
                    .themes
                    .get(*theme_name)
                    .ok_or_else(|| Error::ThemeNotFound((*theme_name).to_string()))?;

                let mut highlighter = Highlighter::new(theme);
                let highlighted_tokens =
                    highlighter.highlight_tokens(&normalized_content, tokens, merging_options);

                Ok(HighlightedCode {
                    language: &self.grammars[grammar_id].name,
                    theme: ThemeVariant::Single(theme),
                    tokens: highlighted_tokens,
                })
            }
            ThemeVariant::Dual { light, dark } => {
                let light_theme = self
                    .themes
                    .get(*light)
                    .ok_or_else(|| Error::ThemeNotFound((*light).to_string()))?;
                let dark_theme = self
                    .themes
                    .get(*dark)
                    .ok_or_else(|| Error::ThemeNotFound((*dark).to_string()))?;

                let mut highlighter = Highlighter::new_dual(light_theme, dark_theme);
                let highlighted_tokens =
                    highlighter.highlight_tokens(&normalized_content, tokens, merging_options);

                Ok(HighlightedCode {
                    language: &self.grammars[grammar_id].name,
                    theme: ThemeVariant::Dual {
                        light: light_theme,
                        dark: dark_theme,
                    },
                    tokens: highlighted_tokens,
                })
            }
        }
    }

    /// Will find all references to external grammars and use the correct target for them.
    /// Call that if you're not using the provided dump otherwise things will not work well.
    pub fn link_grammars(&mut self) {
        let grammar_names_ptr = &self.grammar_id_by_scope_name as *const HashMap<String, GrammarId>;
        let grammars_ptr = &self.grammars as *const Vec<CompiledGrammar>;
        for grammar in self.grammars.iter_mut() {
            // SAFETY: We only modify the content of the current grammar being iterated
            unsafe {
                grammar.resolve_external_references(&*grammar_names_ptr, &*grammars_ptr);
            }

            for inject_to in &grammar.inject_to {
                if let Some(g_id) = self.grammar_id_by_name.get(inject_to) {
                    self.injections_by_grammar[g_id.as_index()].insert(grammar.id);
                }
            }
        }

        self.linked = true;
    }

    fn get_rule_patterns(
        &self,
        base_grammar_id: GrammarId,
        mut rule_ref: GlobalRuleRef,
        visited: &mut HashSet<GlobalRuleRef>,
    ) -> Vec<(GlobalRuleRef, &str)> {
        let mut out = vec![];
        if visited.contains(&rule_ref) || rule_ref == NO_OP_GLOBAL_RULE_REF {
            return out;
        }
        if rule_ref == BASE_GLOBAL_RULE_REF {
            rule_ref = GlobalRuleRef {
                grammar: base_grammar_id,
                rule: ROOT_RULE_ID,
            };
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
                out.extend(self.get_pattern_set_data(base_grammar_id, &i.patterns, visited));
            }
            Rule::BeginEnd(b) => out.push((rule_ref, grammar.regexes[b.begin].pattern())),
            Rule::BeginWhile(b) => out.push((rule_ref, grammar.regexes[b.begin].pattern())),
            Rule::Noop => {}
        }
        out
    }

    fn get_pattern_set_data(
        &self,
        base_grammar_id: GrammarId,
        rule_refs: &[GlobalRuleRef],
        visited: &mut HashSet<GlobalRuleRef>,
    ) -> Vec<(GlobalRuleRef, &str)> {
        let mut out = Vec::new();

        for r in rule_refs {
            let rule_patterns = self.get_rule_patterns(base_grammar_id, *r, visited);
            out.extend(rule_patterns);
        }

        out
    }

    pub(crate) fn collect_patterns(
        &self,
        base_grammar_id: GrammarId,
        rule_ref: GlobalRuleRef,
    ) -> Vec<(GlobalRuleRef, &str)> {
        let grammar = &self.grammars[rule_ref.grammar];
        let base_patterns: &[GlobalRuleRef] = match &grammar.rules[rule_ref.rule] {
            Rule::IncludeOnly(a) => &a.patterns,
            Rule::BeginEnd(a) => &a.patterns,
            Rule::BeginWhile(a) => &a.patterns,
            Rule::Match(_) | Rule::Noop => &[],
        };
        let mut visited = HashSet::new();
        self.get_pattern_set_data(base_grammar_id, base_patterns, &mut visited)
    }

    pub(crate) fn collect_injection_patterns(
        &self,
        target_grammar_id: GrammarId,
        scope_stack: &[Scope],
    ) -> Vec<(InjectionPrecedence, GlobalRuleRef)> {
        let mut result = Vec::new();

        for (matchers, rule) in &self.grammars[target_grammar_id].injections {
            for matcher in matchers {
                if matcher.matches(scope_stack) {
                    if cfg!(feature = "debug") {
                        eprintln!(
                            "Scope stack {scope_stack:?} matched injection selector {matcher:?}"
                        );
                    }
                    result.push((matcher.precedence(), *rule));
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
                result.push((
                    matcher.precedence(),
                    GlobalRuleRef {
                        grammar: injector_id,
                        rule: ROOT_RULE_ID,
                    },
                ));
            }
        }

        result.sort_by_key(|(precedence, _)| match precedence {
            InjectionPrecedence::Left => -1,
            InjectionPrecedence::Right => 1,
        });

        result
    }

    #[cfg(feature = "dump")]
    /// Dump the registry + scope repository to a binary file that can be loaded later
    pub fn dump_to_file(&self, path: impl AsRef<Path>) -> GialloResult<()> {
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
    fn load_from_bytes(compressed_data: &[u8]) -> GialloResult<Self> {
        use crate::scope::replace_global_scope_repo;
        use flate2::read::GzDecoder;
        use std::io::Read;

        let mut decoder = GzDecoder::new(compressed_data);
        let mut msgpack_data = Vec::new();
        decoder.read_to_end(&mut msgpack_data)?;

        let dump: Dump = rmp_serde::from_slice(&msgpack_data)?;
        replace_global_scope_repo(dump.scope_repo);

        Ok(dump.registry)
    }

    #[cfg(feature = "dump")]
    /// Read a binary dump from giallo and load registry + scope repository from it
    pub fn load_from_file(path: impl AsRef<Path>) -> GialloResult<Self> {
        let compressed_data = std::fs::read(path)?;
        Self::load_from_bytes(&compressed_data)
    }

    #[cfg(feature = "dump")]
    /// Load the builtin registry containing all grammars and themes from grammars-themes
    pub fn builtin() -> GialloResult<Self> {
        Self::load_from_bytes(BUILTIN_DATA)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::highlight::HighlightedText;
    use crate::test_utils::get_registry;
    use crate::themes::font_style::FontStyle;

    fn format_highlighted_tokens(
        highlighted_tokens: &[Vec<HighlightedText>],
        content: &str,
    ) -> String {
        let lines: Vec<&str> = content.lines().collect();
        let mut result = String::new();

        for (line_idx, line_tokens) in highlighted_tokens.iter().enumerate() {
            if line_idx >= lines.len() {
                break;
            }

            for token in line_tokens {
                let ThemeVariant::Single(style) = &token.style else {
                    unreachable!()
                };
                // Use proper hex format that includes alpha channel when needed
                let hex_color = style.foreground.as_hex();

                // Create font style abbreviation to match JavaScript format
                // JavaScript bit mapping: bit 1=italic, bit 2=bold, bit 4=underline, bit 8=strikethrough
                let font_style_abbr = if style.font_style.is_empty() {
                    "      ".to_string() // 6 spaces for empty style
                } else {
                    let mut abbr = String::from("[");

                    // Check each style flag and add corresponding character
                    if style.font_style.contains(FontStyle::BOLD) {
                        abbr.push('b');
                    }
                    if style.font_style.contains(FontStyle::ITALIC) {
                        abbr.push('i');
                    }
                    if style.font_style.contains(FontStyle::UNDERLINE) {
                        abbr.push('u');
                    }
                    if style.font_style.contains(FontStyle::STRIKETHROUGH) {
                        abbr.push('s');
                    }

                    abbr.push(']');
                    format!("{:<6}", abbr) // Pad to 6 characters
                };

                // Format: {color_padded_to_10}{fontStyleAbbr_padded_to_6}{tokenText}
                result.push_str(&format!(
                    "{:<10}{}{}\n",
                    hex_color, font_style_abbr, token.text
                ));
            }
        }

        result
    }

    fn format_tokens(input: &str, lines_tokens: Vec<Vec<Token>>) -> String {
        let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
        let lines: Vec<&str> = normalized.split('\n').collect();

        let mut out = String::new();

        for (line_idx, line_tokens) in lines_tokens.iter().enumerate() {
            let line = lines.get(line_idx).unwrap_or(&"");

            for (token_idx, token) in line_tokens.iter().enumerate() {
                let text = &line[token.span.start..token.span.end];
                out.push_str(&format!(
                    "{}: '{}' (line {})\n", // Match fixture format: [start-end] (line N)
                    token_idx, text, line_idx
                ));
                for scope in &token.scopes {
                    out.push_str(&format!("  - {scope}\n"));
                }
                out.push('\n');
            }
        }

        out
    }

    fn get_output_folder_content(path: impl AsRef<Path>) -> Vec<(String, String)> {
        let mut out = Vec::new();

        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            let grammar_name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = fs::read_to_string(&path).unwrap();
            out.push((grammar_name, content));
        }

        out
    }

    #[test]
    fn cannot_replace_grammar_after_linking() {
        let mut registry = Registry::default();

        registry
            .add_grammar_from_path("grammars-themes/packages/tm-grammars/grammars/json.json")
            .unwrap();
        registry.link_grammars();
        let result = registry
            .add_grammar_from_path("grammars-themes/packages/tm-grammars/grammars/json.json");
        assert!(result.is_err());
    }

    #[test]
    fn can_tokenize_like_vscode_textmate() {
        let registry = get_registry();
        let expected_tokens = get_output_folder_content("src/fixtures/tokens");

        for (grammar, expected) in expected_tokens {
            let sample_path = format!("grammars-themes/samples/{grammar}.sample");
            println!("Checking {sample_path}");
            let sample_content = normalize_string(&fs::read_to_string(sample_path).unwrap());
            let tokens = registry
                .tokenize(registry.grammar_id_by_name[&grammar], &sample_content)
                .unwrap();
            let out = format_tokens(&sample_content, tokens);
            assert_eq!(expected.trim(), out.trim());
        }
    }

    #[test]
    fn can_highlight_plain_grammar() {
        let mut registry = Registry::default();
        registry.add_plain_grammar(&[]).unwrap();
        registry
            .add_theme_from_path("grammars-themes/packages/tm-themes/themes/vitesse-black.json")
            .unwrap();
        registry.link_grammars();
        let sample_content = normalize_string(
            &fs::read_to_string("grammars-themes/samples/javascript.sample").unwrap(),
        );
        let highlighted = registry
            .highlight(
                &sample_content,
                HighlightOptions::new(PLAIN_GRAMMAR_NAME, ThemeVariant::Single("vitesse-black"))
                    .merge_whitespace(false)
                    .merge_same_style_tokens(false),
            )
            .unwrap();
        let out = format_highlighted_tokens(&highlighted.tokens, &sample_content);
        insta::assert_snapshot!(out);

        // And then trying to render the same thing with plain fallback set and an unknown grammar
        // should give the same output
        let highlighted2 = registry
            .highlight(
                &sample_content,
                HighlightOptions::new("unknown", ThemeVariant::Single("vitesse-black"))
                    .merge_whitespace(false)
                    .merge_same_style_tokens(false)
                    .fallback_to_plain(true),
            )
            .unwrap();
        let out2 = format_highlighted_tokens(&highlighted2.tokens, &sample_content);
        assert_eq!(out, out2);
    }

    #[test]
    fn can_highlight_like_vscode_textmate() {
        let registry = get_registry();
        let expected_snapshots = get_output_folder_content("src/fixtures/snapshots");

        for (grammar, expected) in expected_snapshots {
            let sample_path = format!("grammars-themes/samples/{grammar}.sample");
            println!("Checking {sample_path}");
            let sample_content = normalize_string(&fs::read_to_string(sample_path).unwrap());
            let highlighted = registry
                .highlight(
                    &sample_content,
                    HighlightOptions::new(&grammar, ThemeVariant::Single("vitesse-black"))
                        .merge_whitespace(false)
                        .merge_same_style_tokens(false),
                )
                .unwrap();

            let out = format_highlighted_tokens(&highlighted.tokens, &sample_content);
            assert_eq!(expected.trim(), out.trim());
        }
    }
}
