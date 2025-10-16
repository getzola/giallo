use std::collections::HashMap;
use std::rc::Rc;

use crate::grammars::{
    CompiledGrammar, END_RULE_ID, PatternSet, PatternSetMatch, Regex, RegexId, Rule, RuleId,
    WHILE_RULE_ID,
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
    /// Start byte position within the line (inclusive, 0-based)
    pub start: usize,
    /// End byte position within the line (exclusive)
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
    /// None values represent capture groups that didn't match (optional groups)
    begin_captures: Vec<Option<String>>,
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
        &mut self,
        line: &str,
        end_captures: &[Option<RuleId>],
        captures: &[Option<(usize, usize)>],
        accumulator: &mut TokenAccumulator,
        base_scopes: &[Scope],
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

            let (cap_start, cap_end) = if let Some(&Some(capture)) = captures.get(i) {
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
                accumulator.produce(cap_start, base_scopes);
            }

            //  Check if it has captures. if it does we need to call tokenize string
            let rule = &self.grammar.rules[rule_id.id()];

            let name = rule.name(line, captures);
            if rule.has_patterns() {
                let content_name = rule.content_name(line, captures);

                // Save current state for restoration after retokenization
                let original_state = self.state.clone();

                // Create new state for retokenization (like stackClone in JS)
                self.state = self.state.push(rule_id);

                // Apply rule name scopes to the new state
                if let Some(name_str) = name.as_ref() {
                    self.state.name_scopes.extend(parse_scope_names(name_str)?);
                }

                // Apply contentName scopes to create proper scope hierarchy
                if let Some(content) = content_name {
                    self.state.content_scopes = self.state.name_scopes.clone();
                    self.state
                        .content_scopes
                        .extend(parse_scope_names(&content)?);
                } else {
                    self.state.content_scopes = self.state.name_scopes.clone();
                }

                // Tokenize substring with modified state (following JS: substring(0, captureIndex.end))
                let substring = &line[0..cap_end];
                let retokenized_acc = self.tokenize_line(substring)?;

                // Restore original state
                self.state = original_state;

                // Merge retokenized tokens back into accumulator with position adjustment
                for token in retokenized_acc.tokens {
                    // Adjust token positions to be relative to the capture start
                    let _adjusted_start = cap_start + token.start;
                    let adjusted_end = cap_start + token.end;

                    // Only produce tokens that are within the capture bounds
                    if adjusted_end <= cap_end {
                        accumulator.produce(adjusted_end, &token.scopes);
                    }
                }

                continue;
            }

            if let Some(n) = name {
                let mut scope_base = if let Some((scopes, _)) = local_stack.last() {
                    scopes.clone()
                } else {
                    base_scopes.to_vec()
                };

                scope_base.extend(parse_scope_names(&n)?);
                local_stack.push((scope_base, cap_end));
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
        let has_advanced = match_.end > match_.start;
        // Always generate a token for any text before this match
        accumulator.produce(match_.start, &self.state.content_scopes);

        if match_.rule_id == END_RULE_ID {
            // END RULE: Pop current rule from stack
            // Use name_scopes for end tokens
            let end_scopes = self.state.name_scopes.clone();

            // Handle end captures based on current rule type
            if let Rule::BeginEnd(b) = &self.grammar.rules[self.state.rule_id.id()] {
                let content_scopes = self.state.content_scopes.clone();
                let end_captures = b.end_captures.clone();
                self.resolve_captures(
                    line,
                    &end_captures,
                    &match_.capture_pos,
                    accumulator,
                    &content_scopes,
                )?;
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
            Rule::BeginEnd(_) | Rule::BeginWhile(_) => {
                // Closure to handle common begin rule logic
                let mut handle_begin_rule = |pattern_regex_id: RegexId,
                                             has_backrefs: bool,
                                             begin_captures: &[Option<RuleId>]|
                 -> Result<(), TokenizeError> {
                    // Set up content_scopes from content_name (including contentName scopes)
                    let content_name = rule.content_name(line, &match_.capture_pos);
                    new_state.content_scopes = new_state.name_scopes.clone();
                    if let Some(content) = content_name {
                        new_state
                            .content_scopes
                            .extend(parse_scope_names(&content)?);
                    }

                    // Always set up the pattern
                    let pattern_regex = &self.grammar.regexes[pattern_regex_id.id()];
                    if has_backrefs {
                        // Store captured text for backreference resolution
                        new_state.begin_captures = match_
                            .capture_pos
                            .iter()
                            .enumerate()
                            .map(|(_i, capture_opt)| match capture_opt {
                                Some((start, end)) => Some(line[*start..*end].to_string()),
                                None => None,
                            })
                            .collect();

                        // Resolve backreferences in pattern
                        let mut resolved = pattern_regex.pattern().to_string();
                        for (i, capture_opt) in new_state.begin_captures.iter().enumerate() {
                            let replacement = capture_opt.as_deref().unwrap_or("");
                            resolved = resolved.replace(&format!("\\{}", i), replacement);
                        }
                        new_state.end_pattern = Some(resolved);
                    } else {
                        // No backreferences, use pattern as-is
                        new_state.end_pattern = Some(pattern_regex.pattern().to_string());
                    }

                    // Handle begin captures with name scopes only (explicit base scopes)
                    let name_scopes = new_state.name_scopes.clone();
                    self.resolve_captures(
                        line,
                        begin_captures,
                        &match_.capture_pos,
                        accumulator,
                        &name_scopes,
                    )?;

                    Ok(())
                };

                // Call with specific parameters for each rule type
                match rule {
                    Rule::BeginEnd(r) => {
                        handle_begin_rule(r.end, r.end_has_backrefs, &r.begin_captures)?
                    }
                    Rule::BeginWhile(r) => {
                        handle_begin_rule(r.while_, r.while_has_backrefs, &r.begin_captures)?
                    }
                    _ => unreachable!(),
                }

                // Set the final state with contentName scopes for future content
                self.state = new_state;
            }

            Rule::Match(r) => {
                // For Match rules, content_scopes should be the same as name_scopes
                // This matches vscode-textmate behavior where both nameScopesList and contentNameScopesList
                // are set to the same value for Match rules
                new_state.content_scopes = new_state.name_scopes.clone();

                // Handle captures with name scopes (explicit base scopes)
                let name_scopes = new_state.name_scopes.clone();
                self.resolve_captures(
                    line,
                    &r.captures,
                    &match_.capture_pos,
                    accumulator,
                    &name_scopes,
                )?;

                // Produce match token with name_scopes
                accumulator.produce(match_.end, &new_state.name_scopes);

                // Match rules don't stay on the stack - keep original state

                // Infinite loop detection for Match rules (following vscode-textmate)
                if !has_advanced {
                    eprintln!("ERROR: Match rule didn't advance - breaking to avoid infinite loop");
                    // We should end tokenization here to prevent infinite loops
                    return Err(TokenizeError::GrammarError(
                        "Grammar is not advancing, nor is it pushing/popping".to_string(),
                    ));
                }
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

            if let Some(m) = pattern_set.find_at(line, pos)? {
                // Save match end and rule_id before moving m
                let match_end = m.end;
                let _rule_id = m.rule_id;

                // Removed debug output for cleaner test

                // Handle the match
                self.handle_match(line, m, &mut accumulator)?;

                // Update position using match end
                pos = match_end;

                // Follow vscode-textmate's approach: only treat specific cases as problematic
                // Zero-width matches are legitimate in TextMate grammars (lookaheads, boundaries, etc.)
                // We only break infinite loops for very specific problematic patterns

                // Note: The sophisticated infinite loop detection is handled within handle_match
                // for different rule types. Here we just need to ensure we make progress.
                if pos < prev_pos {
                    // This should never happen - position moved backward
                    eprintln!(
                        "ERROR: Position moved backward from {} to {}",
                        prev_pos, pos
                    );
                    accumulator.produce(line.len(), &self.state.content_scopes);
                    break;
                }
            } else {
                // No more matches - emit final token and stop
                accumulator.produce(line.len(), &self.state.content_scopes);
                break;
            }
        }

        Ok(accumulator)
    }

    pub fn tokenize_string(&mut self, text: &str) -> Result<Vec<Vec<Token>>, TokenizeError> {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        if normalized.is_empty() {
            return Ok(vec![]);
        }

        self.state = StateStack::new(Scope::new(&self.grammar.scope_name)?);
        let mut lines_tokens = Vec::new();

        // Split by lines and tokenize each line
        for line in normalized.split('\n') {
            let acc = self.tokenize_line(line)?;

            // Keep tokens with line-relative positions (no adjustment needed)
            lines_tokens.push(acc.tokens);
        }

        Ok(lines_tokens)
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
    use std::collections::HashMap;
    use std::fs;

    use super::*;
    use crate::grammars::RawGrammar;
    use pretty_assertions::assert_eq;

    fn format_tokens(input: &str, lines_tokens: Vec<Vec<Token>>) -> String {
        let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
        let lines: Vec<&str> = normalized.split('\n').collect();

        let mut out = String::new();

        for (line_idx, line_tokens) in lines_tokens.iter().enumerate() {
            let line = lines.get(line_idx).unwrap_or(&"");

            for (token_idx, token) in line_tokens.iter().enumerate() {
                let text = &line[token.start..token.end];
                out.push_str(&format!(
                    "{}: {:?} [{}-{}] (line {})\n", // Match fixture format: [start-end] (line N)
                    token_idx, text, token.start, token.end, line_idx
                ));
                for scope in &token.scopes {
                    out.push_str(&format!("  - {}\n", scope.to_string()));
                }
                out.push('\n');
            }
        }

        out
    }

    #[test]
    fn debug_ini_tokenization() {
        // Load INI grammar
        let ini_grammar_path = "grammars-themes/packages/tm-grammars/grammars/ini.json";
        let raw_grammar = RawGrammar::load_from_file(ini_grammar_path).unwrap();
        let compiled_grammar = raw_grammar.compile().unwrap();

        // Test with just the first few lines
        let test_input = "; last modified 1 April 2001 by John Doe\n[owner]\nname = John Doe";

        let mut tokenizer = Tokenizer::new(&compiled_grammar).unwrap();
        let tokens = tokenizer.tokenize_string(&test_input).unwrap();

        println!("Input: {:?}", test_input);
        println!("Lines: {:?}", test_input.split('\n').collect::<Vec<_>>());
        println!("Token results:");

        for (line_idx, line_tokens) in tokens.iter().enumerate() {
            println!("Line {}: {} tokens", line_idx, line_tokens.len());
            for (token_idx, token) in line_tokens.iter().enumerate() {
                let line = test_input.split('\n').nth(line_idx).unwrap_or("");
                let text = if token.end <= line.len() {
                    &line[token.start..token.end]
                } else {
                    "<INVALID RANGE>"
                };
                println!(
                    "  Token {}: {:?} [{}-{}] = {:?}",
                    token_idx,
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

        // Simple assertion - we should have 3 lines of tokens
        assert_eq!(tokens.len(), 3, "Should have exactly 3 lines of tokens");

        // Line 0 should have at least 2 tokens (semicolon + comment)
        assert!(tokens[0].len() >= 2, "Line 0 should have at least 2 tokens");

        // Line 1 should have at least 3 tokens ([, owner, ])
        assert!(tokens[1].len() >= 3, "Line 1 should have at least 3 tokens");
    }

    #[test]
    fn can_tokenize_like_vscode_textmate() {
        let mut all_grammars = HashMap::new();
        for entry in fs::read_dir("grammars-themes/packages/tm-grammars/grammars").unwrap() {
            let path = entry.unwrap().path();
            let grammar_name = path.file_stem().unwrap().to_str().unwrap();
            let raw_grammar = RawGrammar::load_from_file(&path).unwrap();
            let compiled_grammar = raw_grammar.compile().unwrap();
            all_grammars.insert(grammar_name.to_string(), compiled_grammar);
        }

        let mut fixtures = Vec::new();
        for entry in fs::read_dir("src/fixtures/tokens").unwrap() {
            let path = entry.unwrap().path();
            let grammar_name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = fs::read_to_string(&path).unwrap();
            fixtures.push((grammar_name, content));
        }

        for (grammar, expected) in fixtures {
            let sample_path = format!("grammars-themes/samples/{grammar}.sample");
            println!("Checking {sample_path}");
            let sample_content = fs::read_to_string(sample_path).unwrap();
            let mut tokenizer = Tokenizer::new(&all_grammars[&grammar]).unwrap();
            let tokens = tokenizer.tokenize_string(&sample_content).unwrap();
            let out = format_tokens(&sample_content, tokens);
            assert_eq!(expected.trim(), out.trim());
        }
    }
}
