use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::grammars::{
    BASE_GLOBAL_RULE_REF, CompiledGrammar, GlobalRuleRef, GrammarId, InjectionPrecedence, Match,
    NO_OP_GLOBAL_RULE_REF, ROOT_RULE_ID, RawGrammar, Rule,
};
use crate::highlight::{HighlightedText, Highlighter, MergingOptions};
use crate::renderers::{Renderer, html};
use crate::scope::Scope;
#[cfg(feature = "dump")]
use crate::scope::ScopeRepository;
use crate::themes::{CompiledTheme, RawTheme, Style};
use crate::tokenizer::{Token, Tokenizer};

#[cfg(feature = "dump")]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Dump {
    registry: Registry,
    scope_repo: ScopeRepository,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HighlightOptions<'a> {
    pub lang: &'a str,
    pub theme: &'a str,
    pub merge_whitespaces: bool,
    pub merge_same_style_tokens: bool,
}

impl<'a> Default for HighlightOptions<'a> {
    fn default() -> Self {
        Self {
            lang: "",
            theme: "",
            merge_whitespaces: true,
            merge_same_style_tokens: true,
        }
    }
}

#[inline]
pub(crate) fn normalize_string(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
        let mut tokenizer = Tokenizer::new(grammar_id, self);
        match tokenizer.tokenize_string(content) {
            Ok(tokens) => Ok(tokens),
            Err(e) => Err(Box::new(e)),
        }
    }

    #[allow(clippy::type_complexity)]
    pub fn highlight(
        &self,
        content: &str,
        options: HighlightOptions,
    ) -> Result<(Style, Vec<Vec<HighlightedText>>), Box<dyn std::error::Error>> {
        let grammar_id = *self
            .grammar_id_by_name
            .get(options.lang)
            .ok_or_else(|| format!("no grammar found for {}", options.lang))?;
        let theme = self
            .themes
            .get(options.theme)
            .ok_or_else(|| format!("no themes found for {}", options.theme))?;

        let normalized_content = normalize_string(content);

        let tokens = self.tokenize(grammar_id, &normalized_content)?;

        let merging_options = MergingOptions {
            merge_whitespaces: options.merge_whitespaces,
            merge_same_style_tokens: options.merge_same_style_tokens,
        };

        // Create highlighter from theme and apply highlighting (with merging)
        let mut highlighter = Highlighter::new(theme);
        let highlighted_tokens =
            highlighter.highlight_tokens(&normalized_content, tokens, merging_options);

        Ok((theme.default_style, highlighted_tokens))
    }

    pub fn render(
        &self,
        default_style: Style,
        tokens: Vec<Vec<HighlightedText>>,
        renderer: Renderer,
    ) -> String {
        match renderer {
            Renderer::Html => html::render(default_style, tokens),
        }
    }

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
    use pretty_assertions::assert_eq;
    use std::fs;

    use super::*;
    use crate::highlight::HighlightedText;

    fn get_registry() -> Registry {
        let mut registry = Registry::default();
        for entry in fs::read_dir("grammars-themes/packages/tm-grammars/grammars").unwrap() {
            let path = entry.unwrap().path();
            registry.add_grammar_from_path(path).unwrap();
        }
        registry.link_grammars();
        registry
            .add_theme_from_path(
                "vitesse-black",
                "grammars-themes/packages/tm-themes/themes/vitesse-black.json",
            )
            .unwrap();
        registry
    }

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
                // Use proper hex format that includes alpha channel when needed
                let hex_color = token.style.foreground.as_hex();

                // Create font style abbreviation to match JavaScript format
                // JavaScript bit mapping: bit 1=italic, bit 2=bold, bit 4=underline, bit 8=strikethrough
                let font_style_abbr = if token.style.font_style.is_empty() {
                    "      ".to_string() // 6 spaces for empty style
                } else {
                    let mut abbr = String::from("[");

                    // Check each style flag and add corresponding character
                    if token
                        .style
                        .font_style
                        .contains(crate::themes::FontStyle::BOLD)
                    {
                        abbr.push('b');
                    }
                    if token
                        .style
                        .font_style
                        .contains(crate::themes::FontStyle::ITALIC)
                    {
                        abbr.push('i');
                    }
                    if token
                        .style
                        .font_style
                        .contains(crate::themes::FontStyle::UNDERLINE)
                    {
                        abbr.push('u');
                    }
                    if token
                        .style
                        .font_style
                        .contains(crate::themes::FontStyle::STRIKETHROUGH)
                    {
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
    fn can_highlight_like_vscode_textmate() {
        let registry = get_registry();
        let expected_snapshots = get_output_folder_content("src/fixtures/snapshots");

        for (grammar, expected) in expected_snapshots {
            let sample_path = format!("grammars-themes/samples/{grammar}.sample");
            println!("Checking {sample_path}");
            let sample_content = normalize_string(&fs::read_to_string(sample_path).unwrap());
            let (_, highlighted_tokens) = registry
                .highlight(
                    &sample_content,
                    HighlightOptions {
                        lang: &grammar,
                        theme: "vitesse-black",
                        merge_whitespaces: false,
                        merge_same_style_tokens: false,
                    },
                )
                .unwrap();

            let out = format_highlighted_tokens(&highlighted_tokens, &sample_content);
            assert_eq!(expected.trim(), out.trim());
        }
    }
}
