use std::collections::HashMap;
use std::rc::Rc;

use crate::grammars::{
    CompiledGrammar, END_RULE_ID, PatternSet, PatternSetMatch, Regex, Rule, RuleId, WHILE_RULE_ID,
};

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// Start byte position in the input text (inclusive)
    pub start: usize,
    /// End byte position in the input text (exclusive)
    pub end: usize,
    /// Hierarchical scope names, ordered from outermost to innermost
    /// (e.g., source.js -> string.quoted.double -> punctuation.definition.string).
    pub scopes: Vec<String>,
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
    name_scopes: Vec<String>,
    /// "contentName" scopes - applied to content between delimiters
    /// These scopes are active for the rule's interior content
    content_scopes: Vec<String>,
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
    pub fn new(grammar_scope_name: String) -> Self {
        Self {
            parent: None,
            rule_id: RuleId(0), // Root rule (always ID 0)
            name_scopes: vec![grammar_scope_name.clone()],
            content_scopes: vec![grammar_scope_name],
            end_pattern: None,
            begin_captures: Vec::new(),
        }
    }

    /// Called when entering a nested context: when a BeginEnd or BeginWhile begin pattern matches
    fn push(self: &Rc<Self>, rule_id: RuleId) -> StateStack {
        StateStack {
            parent: Some(Rc::clone(self)),
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
struct TokenAccumulator {
    tokens: Vec<Token>,
    /// Position up to which tokens have been generated
    /// (start of next token to be produced)
    last_end_pos: usize,
}

impl TokenAccumulator {
    fn produce(&mut self, end_pos: usize, scopes: &[String]) {
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
    pub fn new(grammar: &'g CompiledGrammar) -> Self {
        Self {
            grammar,
            state: StateStack::new(grammar.scope_name.clone()),
            pattern_cache: HashMap::new(),
            end_regex_cache: HashMap::new(),
        }
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
                    Rule::BeginWhile(w) => {
                        p.push_front(WHILE_RULE_ID, end_pat);
                    }
                    _ => (),
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
            if rule.end_has_backrefs() {
                if rule.apply_end_pattern_last() {
                    p.update_pat_back(end_pat);
                } else {
                    p.update_pat_front(end_pat);
                }
            }
        }

        &self.pattern_cache[&rule_id]
    }

    // fn scan_next(&mut self, text: &str, pos: usize) -> Result<Option<PatternSetMatch>, TokenizeError> {
    //     let pattern_set = self.get_or_create_pattern_set();
    //     Ok(pattern_set.find_at(text, pos))
    // }

    fn resolve_captures(
        &self,
        line: &str,
        end_captures: &[Option<RuleId>],
        captures: &[(usize, usize)],
        accumulator: &mut TokenAccumulator,
    ) {
        if end_captures.is_empty() {
            return;
        }

        let mut local_stack: Vec<(Vec<String>, usize)> = vec![];

        let max = std::cmp::max(end_captures.len(), captures.len());

        for i in 0..max {
            let rule_id = if let Some(&Some(r)) = end_captures.get(i) {
                r
            } else {
                continue;
            };

            let (cap_start, cap_end) = captures[i];

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
                base.push(n);
                local_stack.push((base, cap_end));
            }
        }

        while let Some((scopes, end_pos)) = local_stack.pop() {
            accumulator.produce(end_pos, &scopes);
        }
    }

    fn handle_match(&mut self, line: &str, match_: PatternSetMatch, accumulator: &mut TokenAccumulator) {
        // Always generate a token for any text before this match
        accumulator.produce(match_.start, &self.state.content_scopes);

        if match_.rule_id == END_RULE_ID && let Rule::BeginEnd(b) = &self.grammar.rules[self.state.rule_id.id()] {
            accumulator.produce(match_.start, &self.state.content_scopes);
            self.resolve_captures(line, &b.end_captures, &match_.capture_pos, accumulator);
            accumulator.produce(match_.end, &self.state.content_scopes);
            // Pop back to the parent context
            if let Some(parent) = self.state.pop() {
                self.state = parent;
            }
            return;
        }

        // We got a match other than an end rule
        let rule = &self.grammar.rules[self.state.rule_id.id()];
        accumulator.produce(match_.start, &self.state.content_scopes);
        let name = rule.name(line, &match_.capture_pos);

    }

    pub fn tokenize_line(&mut self, line: &str) -> Result<TokenAccumulator, TokenizeError> {
        let mut accumulator = TokenAccumulator::default();
        let mut pos = 0;

        // 1. We check if the while pattern is still truthy
        self.check_while_conditions(line, &mut pos, &mut accumulator)?;

        // 2. We check for any matching patterns
        while pos < line.len() {
            let pattern_set = self.get_or_create_pattern_set();
            if let Some(m) = pattern_set.find_at(line, pos) {
                pos = m.end;
                // TODO: call handle_match here
            } else {
                accumulator.produce(line.len(), &self.state.content_scopes);
                return Ok(accumulator);
            }
        }

        Ok(accumulator)
    }

    pub fn tokenize_string(&mut self, text: &str) -> Result<Vec<Token>, TokenizeError> {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        if normalized.is_empty() {
            return Ok(vec![]);
        }

        // we do that twice... once in new and here again. not a huge deal
        self.state = StateStack::new(self.grammar.scope_name.clone());
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
        }
    }
}

impl std::error::Error for TokenizeError {}
