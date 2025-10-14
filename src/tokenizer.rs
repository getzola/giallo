use std::collections::HashMap;
use std::rc::Rc;

use crate::grammars::{
    CompiledGrammar, END_RULE_ID, PatternSet, PatternSetMatch, Regex, Rule, RuleId, WHILE_RULE_ID,
};
use crate::scope::{ParseScopeError, Scope};

/// Parse space-separated scope names into a vector of individual scopes
/// e.g., "string.json support.type.property-name.json" -> [Scope("string.json"), Scope("support.type.property-name.json")]
fn parse_scope_names(name: &str) -> Result<Vec<Scope>, TokenizeError> {
    name.split_whitespace()
        .map(|part| Scope::new(part).map_err(TokenizeError::from))
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// Start byte position in the input text (inclusive)
    pub start: usize,
    /// End byte position in the input text (exclusive)
    pub end: usize,
    /// Hierarchical scope names, ordered from outermost to innermost
    /// (e.g., source.js -> string.quoted.double -> punctuation.definition.string).
    pub scopes: Vec<Scope>,
}

/// Keeps track of nested context as well as how to exit that context and the captures
/// strings used in backreferences
///
/// For example, in a string rule:
/// - The quotes get name_scopes: ["string.quoted", "punctuation.definition.string"]
/// - The content gets content_scopes: ["string.quoted"]
#[derive(Debug, Clone)]
struct StateStack {
    /// Parent stack element (None for root)
    parent: Option<Rc<StateStack>>,
    /// Rule ID that created this stack element
    rule_id: RuleId,
    /// "name" scopes - applied to begin/end delimiters
    /// These scopes are active when matching the rule's boundaries
    name_scopes: Vec<Scope>,
    /// "contentName" scopes - applied to content between delimiters
    /// These scopes are active for the rule's interior content
    content_scopes: Vec<Scope>,
    /// Dynamic end/while pattern resolved with backreferences
    /// For BeginEnd rules: the end pattern with \1, \2, etc. resolved
    /// For BeginWhile rules: the while pattern with backreferences resolved
    end_pattern: Option<String>,
    /// Captured text from the begin pattern
    /// Used to resolve backreferences in end/while patterns
    /// Index 0 = full match, Index 1+ = capture groups
    begin_captures: Vec<String>,
}

impl StateStack {
    pub fn new(grammar_scope: Scope) -> Self {
        Self {
            parent: None,
            rule_id: RuleId(0), // Root rule (always ID 0)
            name_scopes: vec![grammar_scope],
            content_scopes: vec![grammar_scope],
            end_pattern: None,
            begin_captures: Vec::new(),
        }
    }

    /// Called when entering a nested context: when a BeginEnd or BeginWhile begin pattern matches
    fn push(&self, rule_id: RuleId) -> StateStack {
        StateStack {
            parent: Some(Rc::new(self.clone())),
            rule_id,
            // Start with the same scope they will diverge later
            name_scopes: self.content_scopes.clone(),
            content_scopes: self.content_scopes.clone(),
            end_pattern: None,
            begin_captures: Vec::new(),
        }
    }

    /// Exits the current context, getting back to the parent
    fn pop(&self) -> Option<StateStack> {
        self.parent.as_ref().map(|parent| (**parent).clone())
    }
}

/// Very small wrapper so we make we only produce valid tokens.
/// Called in the tokenizer a few times and easier to use a struct than pass
/// mutable vec and usize everywhere
#[derive(Debug, Clone, Default)]
pub struct TokenAccumulator {
    tokens: Vec<Token>,
    /// Position up to which tokens have been generated
    /// (start of next token to be produced)
    last_end_pos: usize,
}

impl TokenAccumulator {
    fn produce(&mut self, end_pos: usize, scopes: &[Scope]) {
        // Ensure we don't move backward (can happen with zero-width matches)
        if self.last_end_pos >= end_pos {
            return;
        }

        // Create and store the token with scope IDs directly
        self.tokens.push(Token {
            start: self.last_end_pos,
            end: end_pos,
            scopes: scopes.to_vec(),
        });

        // Advance to the end of this token
        self.last_end_pos = end_pos;
    }
}

#[derive(Debug)]
pub struct Tokenizer<'g> {
    /// Reference to the main compiled grammar in use
    grammar: &'g CompiledGrammar,
    state: StateStack,
    /// Runtime pattern cache by rule ID
    pattern_cache: HashMap<RuleId, PatternSet>,
    /// Used only end/while patterns
    end_regex_cache: HashMap<String, Regex>,
}

impl<'g> Tokenizer<'g> {
    /// The tokenizer starts in the initial state with only the grammar's
    /// root scope active. Use this for tokenizing the first line of a file.
    pub fn new(grammar: &'g CompiledGrammar) -> Result<Self, TokenizeError> {
        Ok(Self {
            grammar,
            state: StateStack::new(Scope::new(&grammar.scope_name)?),
            pattern_cache: HashMap::new(),
            end_regex_cache: HashMap::new(),
        })
    }

    /// Check if there is a while condition active and if it's still true
    fn check_while_conditions(
        &mut self,
        line: &str,
        pos: &mut usize,
        acc: &mut TokenAccumulator,
    ) -> Result<(), TokenizeError> {
        let mut while_stack = Vec::new();
        let mut current = Some(&self.state);

        while let Some(stack_elem) = current {
            if let Some(Rule::BeginWhile(_)) = self.grammar.rules.get(*stack_elem.rule_id as usize)
            {
                while_stack.push(stack_elem.clone());
            }
            current = stack_elem.parent.as_deref();
        }

        // Get all the while rules from last to first
        for w in while_stack
            .into_iter()
            .filter(|w| w.end_pattern.is_some())
            .rev()
        {
            let while_pat = w.end_pattern.as_ref().unwrap();
            // we cache the regex since it will be checked on every line that the pattern is active
            let re = if let Some(re) = self.end_regex_cache.get(while_pat) {
                re
            } else {
                self.end_regex_cache
                    .insert(while_pat.to_string(), Regex::new(while_pat.to_string()));
                self.end_regex_cache.get(while_pat).unwrap()
            };

            // TODO: should we raise an error?
            let search_text = line.get(*pos..).unwrap_or("");
            let compiled_re = re.compiled().ok_or_else(|| {
                TokenizeError::InvalidRegex(format!("While pattern {while_pat} was invalid"))
            })?;
            if let Some(cap) = compiled_re.captures(search_text)
                && let Some((start, end)) = cap.pos(0)
                && start == 0
            {
                let absolute_start = *pos;
                let absolute_end = *pos + end;
                // let capture_strings = cap.iter().filter(|s| s.is_some()).map((|s| s.unwrap().to_string())).collect::<Vec<String>>();

                // While condition still matches - the pattern continues

                // Token for text before the while match
                acc.produce(absolute_start, &self.state.content_scopes);
                // Token for the while match itself (with captures if any)
                acc.produce(absolute_end, &self.state.content_scopes);
                *pos = absolute_end;
            } else {
                // While condition failed
                // Pop everything up to and including this BeginWhile pattern
                while self.state.rule_id != w.rule_id {
                    if let Some(parent) = self.state.pop() {
                        self.state = parent;
                    } else {
                        // Reached root of stack - stop
                        break;
                    }
                }
                break;
            }
        }

        Ok(())
    }

    fn get_or_create_pattern_set(&mut self) -> &PatternSet {
        let rule_id = self.state.rule_id;

        let rule = &self.grammar.rules[rule_id.id()];

        if !self.pattern_cache.contains_key(&rule_id) {
            let patterns = self.grammar.collect_patterns(rule_id);
            let mut p = PatternSet::new(patterns);

            if let Some(end_pat) = self.state.end_pattern.as_deref() {
                match rule {
                    Rule::BeginEnd(b) => {
                        if b.apply_end_pattern_last {
                            p.push_back(END_RULE_ID, end_pat)
                        } else {
                            p.push_front(END_RULE_ID, end_pat)
                        }
                    }
                    Rule::BeginWhile(_) => {
                        p.push_front(WHILE_RULE_ID, end_pat);
                    }
                    _ => {}
                };
            }

            self.pattern_cache.insert(rule_id, p);
        }

        let end_pat = self
            .state
            .end_pattern
            .as_deref()
            .unwrap_or_else(|| "\u{FFFF}");

        if let Some(p) = self.pattern_cache.get_mut(&rule_id) {
            match rule {
                Rule::BeginEnd(_) | Rule::BeginWhile(_) => {
                    if rule.apply_end_pattern_last() {
                        p.update_pat_back(end_pat);
                    } else {
                        p.update_pat_front(end_pat);
                    }
                }
                _ => {}
            }
        }

        &self.pattern_cache[&rule_id]
    }

    fn resolve_captures(
        &self,
        line: &str,
        end_captures: &[Option<RuleId>],
        captures: &[(usize, usize)],
        accumulator: &mut TokenAccumulator,
    ) -> Result<(), TokenizeError> {
        if end_captures.is_empty() {
            return Ok(());
        }

        let mut local_stack: Vec<(Vec<Scope>, usize)> = vec![];

        let max = std::cmp::max(end_captures.len(), captures.len());

        for i in 0..max {
            let rule_id = if let Some(&Some(r)) = end_captures.get(i) {
                r
            } else {
                continue;
            };

            let (cap_start, cap_end) = if let Some(&capture) = captures.get(i) {
                capture
            } else {
                continue;
            };

            // Pop local stack not in use anymore
            while local_stack.len() > 0
                && let Some((scopes, end_pos)) = local_stack.last()
                && *end_pos <= cap_start
            {
                accumulator.produce(*end_pos, &scopes);
                local_stack.pop();
            }

            if let Some((scopes, _)) = local_stack.last() {
                accumulator.produce(cap_start, scopes);
            } else {
                accumulator.produce(cap_start, &self.state.content_scopes);
            }

            //  Check if it has captures. if it does we need to call tokenize string
            let rule = &self.grammar.rules[rule_id.id()];

            let name = rule.name(line, captures);
            if rule.has_patterns() {
                let content_name = rule.content_name(line, captures);
                let new_input = &line[0..cap_end];
                // We need to start a new tokenization
                // TODO!
                // export function _tokenizeString(
                //     grammar: Grammar,
                //     lineText: OnigString,
                //     isFirstLine: boolean,
                //     linePos: number,
                //     stack: StateStackImpl,
                //     lineTokens: LineTokens,
                //     checkWhileConditions: boolean,
                //     timeLimit: number
                // ): TokenizeStringResult {
                // if (captureRule.retokenizeCapturedWithRuleId) {
                //     // the capture requires additional matching
                //     const scopeName = captureRule.getName(lineTextContent, captureIndices);
                //     const nameScopesList = stack.contentNameScopesList!.pushAttributed(scopeName, grammar);
                //     const contentName = captureRule.getContentName(lineTextContent, captureIndices);
                //     const contentNameScopesList = nameScopesList.pushAttributed(contentName, grammar);
                //
                //     const stackClone = stack.push(captureRule.retokenizeCapturedWithRuleId, captureIndex.start, -1, false, null, nameScopesList, contentNameScopesList);
                //     const onigSubStr = grammar.createOnigString(lineTextContent.substring(0, captureIndex.end));
                //     _tokenizeString(grammar, onigSubStr, (isFirstLine && captureIndex.start === 0), captureIndex.start, stackClone, lineTokens, false, /* no time limit */0);
                //     disposeOnigString(onigSubStr);
                //     continue;
                // }
                continue;
            }

            if let Some(n) = name {
                let mut base = if let Some((scopes, _)) = local_stack.last() {
                    scopes.clone()
                } else {
                    self.state.content_scopes.clone()
                };
                base.extend(parse_scope_names(&n)?);
                local_stack.push((base, cap_end));
            }
        }

        while let Some((scopes, end_pos)) = local_stack.pop() {
            accumulator.produce(end_pos, &scopes);
        }

        Ok(())
    }

    fn handle_match(
        &mut self,
        line: &str,
        match_: PatternSetMatch,
        accumulator: &mut TokenAccumulator,
    ) -> Result<(), TokenizeError> {
        // Always generate a token for any text before this match
        accumulator.produce(match_.start, &self.state.content_scopes);

        if match_.rule_id == END_RULE_ID {
            // END RULE: Pop current rule from stack
            // Use name_scopes for end tokens
            let end_scopes = self.state.name_scopes.clone();

            // Handle end captures based on current rule type
            if let Rule::BeginEnd(b) = &self.grammar.rules[self.state.rule_id.id()] {
                self.resolve_captures(line, &b.end_captures, &match_.capture_pos, accumulator)?;
            }

            // Produce end token with name_scopes
            accumulator.produce(match_.end, &end_scopes);

            // Pop to parent state
            if let Some(parent) = self.state.pop() {
                self.state = parent;
            }
            return Ok(());
        }

        // ALL OTHER RULES: Get the actual rule and dispatch by type
        let rule = &self.grammar.rules[match_.rule_id.id()];

        // PUSH new rule onto stack (common for all rule types)
        let mut new_state = self.state.push(match_.rule_id);

        // Apply rule name to create name_scopes
        let rule_name = rule.name(line, &match_.capture_pos);
        if let Some(name) = rule_name.as_ref() {
            // name_scopes = current content_scopes + rule name
            new_state.name_scopes = new_state.content_scopes.clone();
            new_state.name_scopes.extend(parse_scope_names(name)?);
        }

        match rule {
            Rule::BeginEnd(r) => {
                // Set up content_scopes from content_name
                let content_name = rule.content_name(line, &match_.capture_pos);
                new_state.content_scopes = new_state.name_scopes.clone();
                if let Some(content) = content_name {
                    new_state
                        .content_scopes
                        .extend(parse_scope_names(&content)?);
                }

                // Always set up the end pattern
                let end_regex = &self.grammar.regexes[r.end.id()];
                if r.end_has_backrefs {
                    // Store captured text for backreference resolution
                    new_state.begin_captures = match_
                        .capture_pos
                        .iter()
                        .map(|(start, end)| line[*start..*end].to_string())
                        .collect();

                    // Resolve backreferences in end pattern
                    let mut resolved = end_regex.pattern().to_string();
                    for (i, capture) in new_state.begin_captures.iter().enumerate() {
                        resolved = resolved.replace(&format!("\\{}", i), capture);
                    }
                    new_state.end_pattern = Some(resolved);
                } else {
                    // No backreferences, use pattern as-is
                    new_state.end_pattern = Some(end_regex.pattern().to_string());
                }

                // Temporarily update state to use new_state for correct base scopes
                let old_state = std::mem::replace(&mut self.state, new_state.clone());

                // Handle begin captures with correct base scopes
                self.resolve_captures(line, &r.begin_captures, &match_.capture_pos, accumulator)?;

                // Restore and keep the new state
                self.state = new_state;
            }

            Rule::BeginWhile(r) => {
                // Set up content_scopes from content_name
                let content_name = rule.content_name(line, &match_.capture_pos);
                new_state.content_scopes = new_state.name_scopes.clone();
                if let Some(content) = content_name {
                    new_state
                        .content_scopes
                        .extend(parse_scope_names(&content)?);
                }

                // Always set up the while pattern
                let while_regex = &self.grammar.regexes[r.while_.id()];
                if r.while_has_backrefs {
                    // Store captured text for backreference resolution
                    new_state.begin_captures = match_
                        .capture_pos
                        .iter()
                        .map(|(start, end)| line[*start..*end].to_string())
                        .collect();

                    // Resolve backreferences in while pattern
                    let mut resolved = while_regex.pattern().to_string();
                    for (i, capture) in new_state.begin_captures.iter().enumerate() {
                        resolved = resolved.replace(&format!("\\{}", i), capture);
                    }
                    new_state.end_pattern = Some(resolved);
                } else {
                    // No backreferences, use pattern as-is
                    new_state.end_pattern = Some(while_regex.pattern().to_string());
                }

                // Temporarily update state to use new_state for correct base scopes
                let old_state = std::mem::replace(&mut self.state, new_state.clone());

                // Handle begin captures with correct base scopes
                self.resolve_captures(line, &r.begin_captures, &match_.capture_pos, accumulator)?;

                // Restore and keep the new state
                self.state = new_state;
            }

            Rule::Match(r) => {
                // Handle captures with proper scoping - convert Vec<RuleId> to Vec<Option<RuleId>>
                let captures: Vec<Option<RuleId>> = r.captures.iter().map(|&id| Some(id)).collect();
                self.resolve_captures(line, &captures, &match_.capture_pos, accumulator)?;

                // Produce match token with name_scopes
                accumulator.produce(match_.end, &new_state.name_scopes);

                // IMMEDIATE POP - don't keep the pushed state for match rules
                // (new_state is discarded, self.state remains unchanged)
            }

            _ => {
                // Handle other rule types if any
                accumulator.produce(match_.end, &new_state.name_scopes);
                self.state = new_state;
            }
        }

        Ok(())
    }

    pub fn tokenize_line(&mut self, line: &str) -> Result<TokenAccumulator, TokenizeError> {
        let mut accumulator = TokenAccumulator::default();
        let mut pos = 0;

        // 1. We check if the while pattern is still truthy
        self.check_while_conditions(line, &mut pos, &mut accumulator)?;

        // 2. We check for any matching patterns
        while pos < line.len() {
            let prev_pos = pos;

            let pattern_set = self.get_or_create_pattern_set();

            if let Some(m) = pattern_set.find_at(line, pos) {
                // Save match end and rule_id before moving m
                let match_end = m.end;
                let rule_id = m.rule_id;

                // Handle the match
                self.handle_match(line, m, &mut accumulator)?;

                // Update position using match end
                pos = match_end;

                // Infinite loop detection - but allow zero-width END_RULE_ID matches
                if pos <= prev_pos && rule_id != END_RULE_ID {
                    // Grammar didn't advance - prevent infinite loop
                    if pos == prev_pos {
                        // Zero-width match, force advance by one character
                        pos = (prev_pos + 1).min(line.len());
                    }
                    // If still no progress, break to avoid infinite loop
                    if pos <= prev_pos {
                        accumulator.produce(line.len(), &self.state.content_scopes);
                        break;
                    }
                }
            } else {
                // No more matches - emit final token and stop
                accumulator.produce(line.len(), &self.state.content_scopes);
                break;
            }
        }

        Ok(accumulator)
    }

    pub fn tokenize_string(&mut self, text: &str) -> Result<Vec<Token>, TokenizeError> {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        if normalized.is_empty() {
            return Ok(vec![]);
        }

        self.state = StateStack::new(Scope::new(&self.grammar.scope_name)?);
        let mut out = Vec::new();
        let mut pos = 0;

        // Use split to preserve information about trailing newlines
        for (i, line) in normalized.split('\n').enumerate() {
            if i > 0 {
                pos += 1; // Account for the \n we split on (except first line)
            }

            let acc = self.tokenize_line(line)?;

            for mut token in acc.tokens {
                token.start += pos;
                token.end += pos;
                out.push(token);
            }

            pos += line.len();
        }

        Ok(out)
    }
}

#[derive(Debug)]
pub enum TokenizeError {
    /// A regex pattern failed to compile or match.
    /// Contains the problematic pattern for debugging.
    InvalidRegex(String),
    GrammarError(String),
    /// A scope failed to parse or had too many atoms.
    ScopeParseError(ParseScopeError),
}

impl std::fmt::Display for TokenizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            TokenizeError::InvalidRegex(pattern) => {
                write!(f, "Invalid regex pattern: {}", pattern)
            }
            TokenizeError::GrammarError(msg) => {
                write!(f, "Grammar error: {}", msg)
            }
            TokenizeError::ScopeParseError(err) => {
                write!(f, "Scope parse error: {}", err)
            }
        }
    }
}

impl std::error::Error for TokenizeError {}

impl From<ParseScopeError> for TokenizeError {
    fn from(err: ParseScopeError) -> Self {
        TokenizeError::ScopeParseError(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammars::RawGrammar;

    #[test]
    fn test_json_tokenization_all_types() {
        // Load and compile the JSON grammar
        let json_path = "grammars-themes/packages/tm-grammars/grammars/json.json";
        let raw_grammar =
            RawGrammar::load_from_file(json_path).expect("Failed to load JSON grammar");
        let compiled_grammar = raw_grammar
            .compile()
            .expect("Failed to compile JSON grammar");

        // Create tokenizer
        let mut tokenizer = Tokenizer::new(&compiled_grammar).expect("Failed to create tokenizer");

        // Test JSON with all data types
        let json_input = r#"{
  "string": "hello world",
  "number": 42,
  "float": 3.14,
  "boolean_true": true,
  "boolean_false": false,
  "null_value": null,
  "array": [1, 2, "three"],
  "object": {
    "nested": "value"
  }
}"#;

        // Tokenize the JSON
        let tokens = tokenizer
            .tokenize_string(json_input)
            .expect("Failed to tokenize JSON");

        // Assert on number of tokens and print them
        assert!(
            tokens.len() > 20,
            "Should produce many tokens for complex JSON"
        );

        println!("JSON Tokenization Results ({} tokens):", tokens.len());
        for (i, token) in tokens.iter().enumerate() {
            let text = &json_input[token.start..token.end];
            println!(
                "  {}: '{}' [{}-{}] scopes: {:?}",
                i,
                text,
                token.start,
                token.end,
                token
                    .scopes
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn test_simple_json_object() {
        // Load and compile the JSON grammar
        let json_path = "grammars-themes/packages/tm-grammars/grammars/json.json";
        let raw_grammar =
            RawGrammar::load_from_file(json_path).expect("Failed to load JSON grammar");
        let compiled_grammar = raw_grammar
            .compile()
            .expect("Failed to compile JSON grammar");

        // Create tokenizer
        let mut tokenizer = Tokenizer::new(&compiled_grammar).expect("Failed to create tokenizer");

        // Test simple JSON
        let json_input = r#"{"key": "value"}"#;

        // Tokenize
        let tokens = tokenizer
            .tokenize_string(json_input)
            .expect("Failed to tokenize simple JSON");

        // Print tokens first
        println!("Simple JSON Tokenization ({} tokens):", tokens.len());
        for (i, token) in tokens.iter().enumerate() {
            let text = &json_input[token.start..token.end];
            println!(
                "  {}: '{}' [{}-{}] scopes: {:?}",
                i,
                text,
                token.start,
                token.end,
                token
                    .scopes
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
            );
        }

        // Assert on number of tokens
        assert_eq!(
            tokens.len(),
            10,
            "Should produce exactly 10 tokens for simple JSON"
        );

        // Check that we have separate tokens for key and value content
        let token_texts: Vec<&str> = tokens.iter().map(|t| &json_input[t.start..t.end]).collect();
        assert!(
            token_texts.contains(&"key"),
            "Should have separate token for key content"
        );
        assert!(
            token_texts.contains(&"value"),
            "Should have separate token for value content"
        );
        assert!(
            token_texts.contains(&"}"),
            "Should have closing brace token"
        );
        assert!(false);
    }
}
