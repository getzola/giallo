use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use crate::grammars::{CompiledGrammar, END_RULE_ID, PatternSet, Regex, RegexId, Rule, RuleId};
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
    /// The state has entered and captured \n.
    /// This means that the next line should start with an anchor_position of 0.
    begin_rule_has_captured_eol: bool,
    /// Where we currently are in a line
    anchor_position: Option<usize>,
}

impl StateStack {
    pub fn new(grammar_scope: Scope) -> Self {
        Self {
            parent: None,
            rule_id: RuleId(0), // Root rule (always ID 0)
            name_scopes: vec![grammar_scope],
            content_scopes: vec![grammar_scope],
            end_pattern: None,
            begin_rule_has_captured_eol: false,
            anchor_position: None,
        }
    }

    /// Called when entering a nested context: when a BeginEnd or BeginWhile begin pattern matches
    fn push(
        &self,
        rule_id: RuleId,
        anchor_position: Option<usize>,
        begin_rule_has_captured_eol: bool,
    ) -> StateStack {
        StateStack {
            parent: Some(Rc::new(self.clone())),
            rule_id,
            // Start with the same scope they will diverge later
            name_scopes: self.content_scopes.clone(),
            content_scopes: self.content_scopes.clone(),
            end_pattern: None,
            begin_rule_has_captured_eol,
            anchor_position,
        }
    }

    fn with_content_scopes(&self, content_scopes: Vec<Scope>) -> Self {
        let mut new = self.clone();
        new.content_scopes = content_scopes;
        new
    }

    fn with_end_pattern(&self, end_pattern: String) -> Self {
        let mut new = self.clone();
        new.end_pattern = Some(end_pattern);
        new
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
        if cfg!(feature = "debug") {
            eprintln!(
                "[produce]: [{}..{end_pos}]\n{}",
                self.last_end_pos,
                scopes
                    .iter()
                    .map(|s| format!(" * {s}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        }
        self.tokens.push(Token {
            start: self.last_end_pos,
            end: end_pos,
            scopes: scopes.to_vec(),
        });

        // Advance to the end of this token
        self.last_end_pos = end_pos;
    }

    /// Similar to LineTokens.getResult in vscode-textmate except we don't push
    /// tokens for empty lines
    fn finalize(&mut self, line_len: usize) {
        // Pop the token for the added newline if there is one
        if let Some(tok) = self.tokens.last() {
            if tok.start == line_len - 1 {
                self.tokens.pop();
            }
        }

        // If we have a token that includes the trailing newline,
        // decrement the end to not include it
        if let Some(t) = self.tokens.last_mut() {
            if t.end == line_len {
                t.end -= 1;
            }
        }
    }
}

/// We use that as a way to convey both the rule and which anchors should be active
/// in regexes. We don't want to enable \A or \G everywhere, it's context dependent.
#[derive(Copy, Clone, PartialEq, Hash, Eq)]
enum AnchorActiveRule {
    // Only \A is active
    A(RuleId),
    // Only \G is active
    G(RuleId),
    // Both \A and \G are active
    AG(RuleId),
    // Neither \A nor \G are active
    None(RuleId),
}

impl AnchorActiveRule {
    pub fn new(
        rule_id: RuleId,
        is_first_line: bool,
        anchor_position: Option<usize>,
        current_pos: usize,
    ) -> AnchorActiveRule {
        let g_active = if let Some(a_pos) = anchor_position {
            a_pos == current_pos
        } else {
            false
        };

        if is_first_line {
            if g_active {
                AnchorActiveRule::AG(rule_id)
            } else {
                AnchorActiveRule::A(rule_id)
            }
        } else {
            if g_active {
                AnchorActiveRule::G(rule_id)
            } else {
                AnchorActiveRule::None(rule_id)
            }
        }
    }

    /// This follows vscode-textmate and replaces it with something that is very unlikely
    /// to match
    pub fn replace_anchors(&self, pat: &str) -> String {
        let replacement = match self {
            AnchorActiveRule::A(_) => vec![("\\G", "\u{FFFF}")],
            AnchorActiveRule::G(_) => vec![("\\A", "\u{FFFF}")],
            AnchorActiveRule::AG(_) => vec![],
            AnchorActiveRule::None(_) => vec![("\\A", "\u{FFFF}"), ("\\G", "\u{FFFF}")],
        };

        let mut pat = pat.to_string();
        for (a, b) in replacement {
            pat = pat.replace(a, b);
        }
        pat
    }

    /// Return the other members of the enum for the given anchor.
    /// We need it later to invalidate caches
    pub fn others(&self) -> Vec<AnchorActiveRule> {
        match self {
            AnchorActiveRule::A(g) => vec![
                AnchorActiveRule::G(g.clone()),
                AnchorActiveRule::AG(g.clone()),
                AnchorActiveRule::None(g.clone()),
            ],
            AnchorActiveRule::G(g) => vec![
                AnchorActiveRule::A(g.clone()),
                AnchorActiveRule::AG(g.clone()),
                AnchorActiveRule::None(g.clone()),
            ],
            AnchorActiveRule::AG(g) => vec![
                AnchorActiveRule::A(g.clone()),
                AnchorActiveRule::G(g.clone()),
                AnchorActiveRule::None(g.clone()),
            ],
            AnchorActiveRule::None(g) => vec![
                AnchorActiveRule::A(g.clone()),
                AnchorActiveRule::G(g.clone()),
                AnchorActiveRule::AG(g.clone()),
            ],
        }
    }
}

impl fmt::Debug for AnchorActiveRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            AnchorActiveRule::A(_) => "allow_A=true, allow_G=false",
            AnchorActiveRule::G(_) => "allow_A=false, allow_G=true",
            AnchorActiveRule::AG(_) => "allow_A=true, allow_G=true",
            AnchorActiveRule::None(_) => "allow_A=false, allow_G=false",
        };
        f.write_str(s)
    }
}

#[derive(Debug)]
pub struct Tokenizer<'g> {
    /// Reference to the main compiled grammar in use
    grammar: &'g CompiledGrammar,
    /// Runtime pattern cache by rule ID + anchor state
    pattern_cache: HashMap<AnchorActiveRule, PatternSet>,
    /// Used only for end/while patterns
    /// Some end patterns will change depending on backrefs so we might have multiple
    /// versions of the same regex in there
    end_regex_cache: HashMap<String, Regex>,
}

impl<'g> Tokenizer<'g> {
    pub fn new(grammar: &'g CompiledGrammar) -> Self {
        Self {
            grammar,
            pattern_cache: HashMap::new(),
            end_regex_cache: HashMap::new(),
        }
    }

    /// Check if there is a while condition active and if it's still true
    /// This follows VSCode TextMate's behavior: bottom-up processing from outermost to innermost
    /// Functional version of check_while_conditions that takes and returns state
    fn check_while_conditions(
        &mut self,
        stack: StateStack,
        line: &str,
        pos: &mut usize,
        acc: &mut TokenAccumulator,
        is_first_line: bool,
    ) -> Result<(StateStack, Option<usize>, bool), TokenizeError> {
        // Initialize anchor position: reset to 0 if previous rule captured EOL, otherwise use stack value
        let mut anchor_position: Option<usize> = if stack.begin_rule_has_captured_eol {
            Some(0)
        } else {
            None
        };
        let mut is_first_line = is_first_line;
        let mut stack = stack;

        // Collect all BeginWhile rules from bottom to top of stack
        let mut while_stacks = Vec::new();
        let mut current = Some(&stack);

        while let Some(stack_elem) = current {
            if let Some(Rule::BeginWhile(_)) = self.grammar.rules.get(*stack_elem.rule_id as usize)
            {
                while_stacks.push(stack_elem.clone());
            }
            current = stack_elem.parent.as_deref();
        }

        if cfg!(feature = "debug") {
            if while_stacks.is_empty() {
                eprintln!(
                    "[check_while_conditions] no while conditions active:\n  {:?}",
                    stack
                );
            } else {
                eprintln!(
                    "[check_while_conditions] going to check:\n  {:?}",
                    while_stacks
                );
            }
        }

        // We don't care about the rule here, we just want to compute which anchors should be active
        let active_anchor = AnchorActiveRule::new(RuleId(0), is_first_line, anchor_position, *pos);

        if cfg!(feature = "debug") {
            eprintln!("[check_while_conditions] Anchors: {active_anchor:?}");
        }

        for while_stack in while_stacks.into_iter().rev() {
            let initial_pat = if let Some(end_pat) = &while_stack.end_pattern {
                end_pat.as_str()
            } else if let Rule::BeginWhile(b) = &self.grammar.rules[while_stack.rule_id.id()] {
                let re = &self.grammar.regexes[b.while_.id()];
                re.pattern()
            } else {
                unreachable!()
            };
            let while_pat = active_anchor.replace_anchors(initial_pat);

            let re = if let Some(re) = self.end_regex_cache.get(&while_pat) {
                re
            } else {
                self.end_regex_cache
                    .insert(while_pat.clone(), Regex::new(while_pat.clone()));
                self.end_regex_cache.get(&while_pat).unwrap()
            };

            let search_text = line.get(*pos..).unwrap_or("");
            let compiled_re = re.compiled().ok_or_else(|| {
                TokenizeError::InvalidRegex(format!("While pattern {while_pat} was invalid"))
            })?;

            if let Some(cap) = compiled_re.captures(search_text)
                && let Some((start, end)) = cap.pos(0)
                && start == 0
            // Must match at current position
            {
                // While condition matches - handle captures and advance position
                let absolute_start = *pos;
                let absolute_end = *pos + end;

                acc.produce(absolute_start, &while_stack.content_scopes);
                // Handle while captures if they exist
                if let Some(Rule::BeginWhile(begin_while_rule)) =
                    self.grammar.rules.get(*while_stack.rule_id as usize)
                {
                    if !begin_while_rule.while_captures.is_empty() {
                        let captures_pos: Vec<Option<(usize, usize)>> = (0..cap.len())
                            .map(|i| cap.pos(i).map(|(s, e)| (*pos + s, *pos + e)))
                            .collect();

                        self.resolve_captures(
                            &while_stack,
                            line,
                            &begin_while_rule.while_captures,
                            &captures_pos,
                            acc,
                            is_first_line,
                        )?;
                    }
                }

                // Produce token for the while match itself
                acc.produce(absolute_end, &while_stack.content_scopes);

                // Advance position and update anchor - matches VSCode behavior
                if absolute_end > *pos {
                    *pos = absolute_end;
                    anchor_position = Some(*pos);
                    is_first_line = false;
                }
            } else {
                if cfg!(feature = "debug") {
                    eprintln!(
                        "[check_while_conditions] No while match found, popping: {:?}",
                        self.grammar
                            .rules
                            .get(*while_stack.rule_id as usize)
                            .unwrap()
                            .original_name()
                    );
                }

                stack = while_stack.pop().expect("to have a parent");
                break; // Stop checking further while conditions
            }
        }

        Ok((stack, anchor_position, is_first_line))
    }

    /// Functional version of get_or_create_pattern_set that takes a stack reference and session state
    fn get_or_create_pattern_set(
        &mut self,
        stack: &StateStack,
        pos: usize,
        is_first_line: bool,
        anchor_position: Option<usize>,
    ) -> &PatternSet {
        let rule_id = stack.rule_id;

        let rule = &self.grammar.rules[rule_id.id()];
        let rule_w_anchor = AnchorActiveRule::new(rule_id, is_first_line, anchor_position, pos);

        if cfg!(feature = "debug") {
            eprintln!(
                "[get_or_create_pattern_set] Rule ID: {} Anchors: {rule_w_anchor:?}",
                rule_id.id()
            );
            eprintln!(
                "[get_or_create_pattern_set] Scanning for: pos={pos}, anchor_position={:?}",
                stack.anchor_position
            );
        }

        let mut end_pattern = stack.end_pattern.as_ref().map(|s| s.as_str());

        if end_pattern.is_none() {
            match rule {
                Rule::BeginEnd(b) => {
                    let re = &self.grammar.regexes[b.end.id()];
                    end_pattern = Some(re.pattern());
                }
                Rule::BeginWhile(b) => {
                    let re = &self.grammar.regexes[b.while_.id()];
                    end_pattern = Some(re.pattern());
                }
                _ => (),
            }
        }

        if !self.pattern_cache.contains_key(&rule_w_anchor) {
            let raw_patterns = self.grammar.collect_patterns(rule_id);

            if cfg!(feature = "debug") {
                eprintln!("[get_or_create_pattern_set] Creating new ps with stack: {stack:#?}");
            }

            let patterns: Vec<(RuleId, String)> = raw_patterns
                .into_iter()
                .map(|(rule, pat)| {
                    let replaced = rule_w_anchor.replace_anchors(&pat);
                    (rule, replaced)
                })
                .collect();

            let mut p = PatternSet::new(patterns);

            // end pattern is pushed separately since some rules can apply it at
            // the end of the pattern set instead of beginning
            if let Some(pat) = end_pattern
                && let Rule::BeginEnd(_) = rule
            {
                if rule.apply_end_pattern_last() {
                    p.push_back(END_RULE_ID, &pat)
                } else {
                    p.push_front(END_RULE_ID, &pat)
                }
            }

            self.pattern_cache.insert(rule_w_anchor, p);
        }

        // And then once we have the pattern set created/retrieved, we update the end pattern
        // This might invalidate the internal regset if the pattern is different from what
        // was there before
        let mut updated = false;
        if let Rule::BeginEnd(b) = rule
            && let Some(p) = self.pattern_cache.get_mut(&rule_w_anchor)
            && let Some(end_pat) = end_pattern
        {
            let pat = rule_w_anchor.replace_anchors(end_pat);
            if b.apply_end_pattern_last {
                updated = p.update_pat_back(&pat);
            } else {
                updated = p.update_pat_front(&pat);
            }
        }

        // If we updated an end pattern we need to invalidate all other cached versions of this rule
        if updated {
            for r in rule_w_anchor.others() {
                self.pattern_cache.remove(&r);
            }
        }
        let p = &self.pattern_cache[&rule_w_anchor];

        if cfg!(feature = "debug") {
            let rule = &self.grammar.rules[rule_id.id()];
            eprintln!(
                "[get_or_create_pattern_set] Active patterns for rule {:?}.\n{p:?}",
                rule.original_name()
            );
        }

        p
    }

    fn resolve_captures(
        &mut self,
        stack: &StateStack,
        line: &str,
        rule_captures: &[Option<RuleId>],
        captures: &[Option<(usize, usize)>],
        accumulator: &mut TokenAccumulator,
        is_first_line: bool,
    ) -> Result<(), TokenizeError> {
        if rule_captures.is_empty() {
            return Ok(());
        }

        // (scopes, end_pos)[]
        let mut local_stack: Vec<(Vec<Scope>, usize)> = vec![];

        let min = std::cmp::min(rule_captures.len(), captures.len());

        for i in 0..min {
            let rule_id = if let Some(&Some(r)) = rule_captures.get(i) {
                r
            } else {
                continue;
            };
            if captures.get(i).unwrap().is_none() {
                continue;
            }
            let (cap_start, cap_end) = captures.get(i).unwrap().unwrap();
            // Nothing captured
            if cap_start == cap_end {
                continue;
            }

            // pop captures while needed
            while local_stack.len() > 0
                && let Some((scopes, end_pos)) = local_stack.last()
                && *end_pos <= cap_start
            {
                accumulator.produce(*end_pos, scopes);
                local_stack.pop();
            }

            if let Some((scopes, _)) = local_stack.last() {
                accumulator.produce(cap_start, scopes);
            } else {
                accumulator.produce(cap_start, &stack.content_scopes);
            }

            //  Check if it has captures. If it does we need to call tokenize_string
            let rule = &self.grammar.rules[rule_id.id()];
            let name = rule.name(line, captures);

            if rule.has_patterns() {
                let content_name = rule.content_name(line, captures);
                let mut retokenization_stack = stack.push(rule_id, None, false);
                // Apply rule name scopes to the new state
                if let Some(name_str) = name.as_ref() {
                    retokenization_stack
                        .name_scopes
                        .extend(parse_scope_names(name_str)?);
                }
                // Start with name + content scopes for content scpoes
                retokenization_stack.content_scopes = retokenization_stack.name_scopes.clone();
                if let Some(content) = content_name {
                    retokenization_stack
                        .content_scopes
                        .extend(parse_scope_names(&content)?);
                }
                let substring = &line[0..cap_end];
                if cfg!(feature = "debug") {
                    eprintln!(
                        "[resolve_captures] Retokenizing capture at [{cap_start}..{cap_end}]: {:?}",
                        &line[cap_start..cap_end]
                    );
                    eprintln!(
                        "[resolve_captures] Using substring [0..{cap_end}]: {:?}",
                        substring
                    );
                }
                let (retokenized_acc, _) = self.tokenize_line(
                    retokenization_stack,
                    substring,
                    is_first_line && cap_start == 0,
                    false,
                )?;

                // Merge retokenized tokens back into accumulator with position adjustment
                for token in retokenized_acc.tokens {
                    // Only include tokens that are within the capture bounds
                    if token.start >= cap_start && token.end <= cap_end {
                        accumulator.produce(token.end, &token.scopes);
                    }
                }
                continue;
            }

            if let Some(n) = name {
                let mut base = if let Some((scopes, _)) = local_stack.last() {
                    scopes.clone()
                } else {
                    stack.content_scopes.clone()
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

    fn tokenize_line(
        &mut self,
        stack: StateStack,
        line: &str,
        is_first_line: bool,
        check_while_conditions: bool,
    ) -> Result<(TokenAccumulator, StateStack), TokenizeError> {
        let mut accumulator = TokenAccumulator::default();
        let mut pos = 0;
        let mut anchor_position = None;
        let mut is_first_line = is_first_line;
        let mut stack = stack;

        // 1. We check if the while pattern is still truthy
        if check_while_conditions {
            let while_res = self.check_while_conditions(
                stack,
                line,
                &mut pos,
                &mut accumulator,
                is_first_line,
            )?;
            stack = while_res.0;
            anchor_position = while_res.1;
            is_first_line = while_res.2;
        }

        // 2. We check for any matching patterns
        loop {
            if cfg!(feature = "debug") {
                eprintln!();
                eprintln!("[tokenize_line] Scanning {pos}: |{:?}|", &line[pos..]);
            }

            let pattern_set =
                self.get_or_create_pattern_set(&stack, pos, is_first_line, anchor_position);

            if let Some(m) = pattern_set.find_at(line, pos)? {
                if cfg!(feature = "debug") {
                    eprintln!(
                        "[tokenize_line] Matched rule: {} from pos {} to {}",
                        m.rule_id.id(),
                        m.start,
                        m.end
                    );
                }

                // We matched the `end` for this rule, can only happen for BeginEnd rules
                if m.rule_id == END_RULE_ID
                    && let Rule::BeginEnd(b) = &self.grammar.rules[stack.rule_id.id()]
                {
                    if cfg!(feature = "debug") {
                        eprintln!(
                            "[tokenize_line] End rule matched, popping '{}'",
                            b.name.clone().unwrap_or_default()
                        );
                    }
                    accumulator.produce(m.start, &stack.content_scopes);
                    stack = stack.with_content_scopes(stack.name_scopes.clone());
                    self.resolve_captures(
                        &stack,
                        line,
                        &b.end_captures,
                        &m.capture_pos,
                        &mut accumulator,
                        is_first_line,
                    )?;
                    accumulator.produce(m.end, &stack.content_scopes);

                    // Pop to parent state and update anchor position
                    anchor_position = stack.anchor_position;
                    stack = stack.pop().expect("to have a parent stack");
                } else {
                    let rule = &self.grammar.rules[m.rule_id.id()];
                    accumulator.produce(m.start, &stack.content_scopes);
                    let new_scopes = if let Some(n) = rule.name(line, &m.capture_pos) {
                        let mut s = stack.content_scopes.clone();
                        s.extend(parse_scope_names(&n)?);
                        s
                    } else {
                        stack.content_scopes.clone()
                    };
                    // TODO: improve that push?
                    stack = stack.push(m.rule_id, anchor_position, m.end == line.len());
                    stack.name_scopes = new_scopes.clone();
                    stack.content_scopes = new_scopes;
                    stack.end_pattern = None;

                    let mut handle_begin_rule = |re_id: RegexId,
                                                 end_has_backrefs: bool,
                                                 begin_captures: &[Option<RuleId>]|
                     -> Result<(), TokenizeError> {
                        let re = &self.grammar.regexes[re_id.id()];
                        if cfg!(feature = "debug") {
                            eprintln!(
                                "[tokenize_line] Pushing '{}' - '{}'",
                                self.grammar
                                    .get_original_rule_name(m.rule_id)
                                    .unwrap_or_default(),
                                re.pattern()
                            );
                        }

                        self.resolve_captures(
                            &stack,
                            line,
                            begin_captures,
                            &m.capture_pos,
                            &mut accumulator,
                            is_first_line,
                        )?;
                        accumulator.produce(m.end, &stack.content_scopes);
                        anchor_position = Some(m.end);
                        let content_scopes =
                            if let Some(s) = rule.content_name(line, &m.capture_pos) {
                                let mut r = stack.name_scopes.clone();
                                r.extend(parse_scope_names(&s)?);
                                r
                            } else {
                                stack.name_scopes.clone()
                            };
                        stack = stack.with_content_scopes(content_scopes);

                        if end_has_backrefs {
                            let resolved_end = re.resolve_backreferences(line, &m.capture_pos);
                            stack = stack.with_end_pattern(resolved_end);
                        }

                        Ok(())
                    };

                    match rule {
                        Rule::BeginEnd(r) => {
                            handle_begin_rule(r.end, r.end_has_backrefs, &r.begin_captures)?;
                        }
                        Rule::BeginWhile(r) => {
                            handle_begin_rule(r.while_, r.while_has_backrefs, &r.begin_captures)?;
                        }
                        Rule::Match(r) => {
                            if cfg!(feature = "debug") {
                                eprintln!(
                                    "[handle_match] Matched '{}'",
                                    self.grammar
                                        .get_original_rule_name(m.rule_id)
                                        .unwrap_or_default()
                                );
                            }
                            self.resolve_captures(
                                &stack,
                                line,
                                &r.captures,
                                &m.capture_pos,
                                &mut accumulator,
                                is_first_line,
                            )?;
                            accumulator.produce(m.end, &stack.content_scopes);
                            // pop rule immediately since it is a MatchRule
                            stack = stack.pop().expect("to have a parent stack");
                        }
                        _ => unreachable!("matched something without a regex??"),
                    }
                }

                if m.end > pos {
                    // advance
                    pos = m.end;
                    is_first_line = false;
                }
            } else {
                if cfg!(feature = "debug") {
                    eprintln!("[tokenize_line] no more matches");
                }
                // No more matches - emit final token and stop
                accumulator.produce(line.len() - 1, &stack.content_scopes);
                break;
            }
        }

        Ok((accumulator, stack))
    }

    pub fn tokenize_string(&mut self, text: &str) -> Result<Vec<Vec<Token>>, TokenizeError> {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        if normalized.is_empty() {
            return Ok(vec![]);
        }

        let mut stack = StateStack::new(Scope::new(&self.grammar.scope_name)?);
        let mut lines_tokens = Vec::new();
        let mut is_first_line = true;

        // Split by lines and tokenize each line
        for line in normalized.split('\n') {
            // Always add a new line, some regex expect it
            let line = format!("{line}\n");
            if cfg!(feature = "debug") {
                eprintln!("Tokenizing {line:?}");
                eprintln!();
            }
            let (mut acc, new_state) = self.tokenize_line(stack, &line, is_first_line, true)?;
            acc.finalize(line.len());
            lines_tokens.push(acc.tokens);
            stack = new_state;
            is_first_line = false;
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

impl fmt::Display for TokenizeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
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

                // Convert byte positions to character positions
                let char_start = line[..token.start].chars().count();
                let char_end = line[..token.end].chars().count();

                // We use char index because the JS fixtures output use that
                out.push_str(&format!(
                    "{}: {:?} [{}-{}] (line {})\n", // Match fixture format: [start-end] (line N)
                    token_idx, text, char_start, char_end, line_idx
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
    fn test_md_tokenization() {
        let mut all_grammars = HashMap::new();
        for entry in fs::read_dir("grammars-themes/packages/tm-grammars/grammars").unwrap() {
            let path = entry.unwrap().path();
            let grammar_name = path.file_stem().unwrap().to_str().unwrap();
            let raw_grammar = RawGrammar::load_from_file(&path).unwrap();
            let compiled_grammar = raw_grammar.compile().unwrap();
            all_grammars.insert(grammar_name.to_string(), compiled_grammar);
        }

        let input = r#"
~~~python
import time
~~~
        "#;
        println!("{input}");
        let mut tokenizer = Tokenizer::new(&all_grammars["markdown"]);
        let tokens = tokenizer.tokenize_string(&input).unwrap();
        let out = format_tokens(&input, tokens);
        println!("{out}");
        assert!(false);
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
            let mut tokenizer = Tokenizer::new(&all_grammars[&grammar]);
            let tokens = tokenizer.tokenize_string(&sample_content).unwrap();
            let out = format_tokens(&sample_content, tokens);
            assert_eq!(expected.trim(), out.trim());
        }
    }
}
