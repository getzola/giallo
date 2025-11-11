use std::collections::HashMap;
use std::fmt;
use std::ops::Range;
use std::rc::Rc;

use crate::Registry;
use crate::grammars::{
    END_RULE_ID, GlobalRuleRef, GrammarId, InjectionPrecedence, PatternSet, PatternSetMatch,
    ROOT_RULE_ID, Regex, RegexId, Rule, RuleId,
};
use crate::scope::Scope;

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// Byte span within the line (start inclusive, end exclusive, 0-based)
    pub span: Range<usize>,
    /// Hierarchical scope names, ordered from outermost to innermost
    /// (e.g., source.js -> string.quoted.double -> punctuation.definition.string).
    pub scopes: Vec<Scope>,
}

/// Keeps track of nested context as well as how to exit that context and the captures
/// strings used in backreferences
#[derive(Clone)]
struct StateStack {
    /// Parent stack element (None for root)
    parent: Option<Rc<StateStack>>,
    /// Global rule ref that created this stack element
    rule_ref: GlobalRuleRef,
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
    /// The position where this rule was entered during current line (for infinite loop detection)
    /// None at beginning of a line
    enter_position: Option<usize>,
}

impl StateStack {
    pub fn new(grammar_id: GrammarId, grammar_scope: Scope) -> Self {
        Self {
            parent: None,
            rule_ref: GlobalRuleRef {
                grammar: grammar_id,
                rule: ROOT_RULE_ID,
            },
            name_scopes: vec![grammar_scope],
            content_scopes: vec![grammar_scope],
            end_pattern: None,
            begin_rule_has_captured_eol: false,
            anchor_position: None,
            enter_position: None,
        }
    }

    /// Called when entering a nested context: when a BeginEnd or BeginWhile begin pattern matches
    fn push(
        &self,
        rule_ref: GlobalRuleRef,
        anchor_position: Option<usize>,
        begin_rule_has_captured_eol: bool,
        enter_position: Option<usize>,
    ) -> StateStack {
        StateStack {
            parent: Some(Rc::new(self.clone())),
            rule_ref,
            // Start with the same scope they will diverge later
            name_scopes: self.content_scopes.clone(),
            content_scopes: self.content_scopes.clone(),
            end_pattern: None,
            begin_rule_has_captured_eol,
            anchor_position,
            enter_position,
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

    /// Pop but never go below root state - used in infinite loop protection
    fn safe_pop(&self) -> StateStack {
        if let Some(parent) = &self.parent {
            (**parent).clone()
        } else {
            self.clone()
        }
    }

    /// Resets enter_position for all stack elements to None
    fn reset_enter_position(&self) -> StateStack {
        StateStack {
            parent: self
                .parent
                .as_ref()
                .map(|parent| Rc::new(parent.reset_enter_position())),
            enter_position: None,
            ..self.clone()
        }
    }
}

impl std::fmt::Debug for StateStack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Collect the entire stack from root to current
        let mut stack_elements = Vec::new();
        let mut current = self;

        // Traverse up to root, collecting elements
        loop {
            stack_elements.push(current);
            if let Some(parent) = &current.parent {
                current = parent;
            } else {
                break;
            }
        }

        // Reverse to show root first
        stack_elements.reverse();

        writeln!(f, "StateStack:")?;

        for (depth, element) in stack_elements.iter().enumerate() {
            // Create indentation
            let indent = "  ".repeat(depth);

            // Format the basic info
            write!(
                f,
                "{}grammar={}, rule={}",
                indent, element.rule_ref.grammar.0, element.rule_ref.rule.0
            )?;

            // Add name scopes if not empty
            if !element.name_scopes.is_empty() {
                write!(f, " name=[")?;
                for (i, scope) in element.name_scopes.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", scope.build_string())?;
                }
                write!(f, "]")?;
            }

            // Add content scopes if not empty
            if !element.content_scopes.is_empty() {
                write!(f, ", content=[")?;
                for (i, scope) in element.content_scopes.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", scope.build_string())?;
                }
                write!(f, "]")?;
            }

            // Add end_pattern if present
            if let Some(pattern) = &element.end_pattern {
                write!(f, ", end_pattern=\"{}\"", pattern)?;
            }

            // Add anchor_position if present
            if let Some(pos) = element.anchor_position {
                write!(f, ", anchor_pos={}", pos)?;
            }

            // Add enter_position if present and different from anchor_position
            if let Some(enter_pos) = element.enter_position {
                if element.anchor_position != Some(enter_pos) {
                    write!(f, ", enter_pos={}", enter_pos)?;
                }
            }

            writeln!(f)?;
        }

        Ok(())
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
        // Skip empty tokens (can happen with zero-width matches)
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
            span: self.last_end_pos..end_pos,
            scopes: scopes.to_vec(),
        });

        // Advance to the end of this token
        self.last_end_pos = end_pos;
    }

    /// Similar to LineTokens.getResult in vscode-textmate except we don't push
    /// tokens for empty lines
    fn finalize(&mut self, line_len: usize) {
        // Pop the token for the added newline if there is one
        if let Some(tok) = self.tokens.last()
            && tok.span.start == line_len - 1
        {
            self.tokens.pop();
        }

        // If we have a token that includes the trailing newline,
        // decrement the end to not include it
        if let Some(t) = self.tokens.last_mut()
            && t.span.end == line_len
        {
            t.span.end -= 1;
        }
    }
}

/// We use that as a way to convey both the rule and which anchors should be active
/// in regexes. We don't want to enable \A or \G everywhere, it's context dependent.
#[derive(Copy, Clone, PartialEq, Hash, Eq)]
enum AnchorActiveRule {
    // Only \A is active
    A(GlobalRuleRef),
    // Only \G is active
    G(GlobalRuleRef),
    // Both \A and \G are active
    AG(GlobalRuleRef),
    // Neither \A nor \G are active
    None(GlobalRuleRef),
}

impl AnchorActiveRule {
    pub fn new(
        rule_ref: GlobalRuleRef,
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
                AnchorActiveRule::AG(rule_ref)
            } else {
                AnchorActiveRule::A(rule_ref)
            }
        } else if g_active {
            AnchorActiveRule::G(rule_ref)
        } else {
            AnchorActiveRule::None(rule_ref)
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
                AnchorActiveRule::G(*g),
                AnchorActiveRule::AG(*g),
                AnchorActiveRule::None(*g),
            ],
            AnchorActiveRule::G(g) => vec![
                AnchorActiveRule::A(*g),
                AnchorActiveRule::AG(*g),
                AnchorActiveRule::None(*g),
            ],
            AnchorActiveRule::AG(g) => vec![
                AnchorActiveRule::A(*g),
                AnchorActiveRule::G(*g),
                AnchorActiveRule::None(*g),
            ],
            AnchorActiveRule::None(g) => vec![
                AnchorActiveRule::A(*g),
                AnchorActiveRule::G(*g),
                AnchorActiveRule::AG(*g),
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
    /// The index in the grammars vec below we will use to start the process
    base_grammar_id: GrammarId,
    /// All the grammars in the registry
    registry: &'g Registry,
    /// Runtime pattern cache by rule ID + anchor state
    pattern_cache: HashMap<AnchorActiveRule, PatternSet>,
    /// Used only for end/while patterns
    /// Some end patterns will change depending on backrefs so we might have multiple
    /// versions of the same regex in there
    /// Some regex content use backref so they are essentially dynamic patterns
    end_regex_cache: HashMap<String, Regex>,
}

impl<'g> Tokenizer<'g> {
    pub fn new(base_grammar_id: GrammarId, registry: &'g Registry) -> Self {
        Self {
            base_grammar_id,
            registry,
            pattern_cache: HashMap::new(),
            end_regex_cache: HashMap::new(),
        }
    }

    /// Matches injection patterns at the current position
    /// Returns (is_left_precedence, PatternSetMatch) for the best match
    fn match_injections(
        &mut self,
        stack: &StateStack,
        line: &str,
        pos: usize,
        is_first_line: bool,
        anchor_position: Option<usize>,
    ) -> Result<Option<(InjectionPrecedence, PatternSetMatch)>, TokenizeError> {
        let injection_patterns = self
            .registry
            .collect_injection_patterns(self.base_grammar_id, &stack.content_scopes);

        if injection_patterns.is_empty() {
            return Ok(None);
        }

        let mut best_match: Option<(InjectionPrecedence, PatternSetMatch)> = None;

        // Process injections in the order returned by registry (already sorted by precedence)
        for (precedence, rule) in injection_patterns {
            let mut s = stack.clone();
            s.rule_ref = rule;
            s.end_pattern = None;
            if cfg!(feature = "debug") {
                println!("Building pattern set for injection");
            }

            let pattern_set =
                self.get_or_create_pattern_set(&s, pos, is_first_line, anchor_position);

            if let Some(found) = pattern_set.find_at(line, pos)? {
                if cfg!(feature = "debug") {
                    println!("found a match in injection: {:?}", found);
                }

                if let Some((_, current_best_match)) = &best_match {
                    if found.start >= current_best_match.start {
                        continue;
                    }
                    let is_done = found.start == pos;
                    best_match = Some((precedence, found));
                    if is_done {
                        break;
                    }
                } else {
                    best_match = Some((precedence, found));
                }
            }
        }

        Ok(best_match)
    }

    /// Matches both regular rule patterns and injections, returning the best match
    /// Follows vscode-textmate's comparison logic for rule vs injection precedence
    fn match_rule_or_injections(
        &mut self,
        stack: &StateStack,
        line: &str,
        pos: usize,
        is_first_line: bool,
        anchor_position: Option<usize>,
    ) -> Result<Option<PatternSetMatch>, TokenizeError> {
        // Get regular rule patterns
        let pattern_set =
            self.get_or_create_pattern_set(stack, pos, is_first_line, anchor_position);
        let regular_match = pattern_set.find_at(line, pos)?;

        // Get injection matches
        let injection_match =
            self.match_injections(stack, line, pos, is_first_line, anchor_position)?;

        // Compare and return the winner
        match (regular_match, injection_match) {
            (None, None) => Ok(None),
            (Some(regular), None) => Ok(Some(regular)),
            (None, Some((_, injection))) => Ok(Some(injection)),
            (Some(regular), Some((precedence, injection))) => {
                let match_score = regular.start;
                let injection_score = injection.start;
                if injection_score < match_score
                    || (injection_score == match_score && precedence == InjectionPrecedence::Left)
                {
                    Ok(Some(injection))
                } else {
                    Ok(Some(regular))
                }
            }
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
            if let Some(Rule::BeginWhile(_)) = self.registry.grammars[stack_elem.rule_ref.grammar]
                .rules
                .get(stack_elem.rule_ref.rule.as_index())
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
        // TODO: it should probably a standalone fn then
        let active_anchor = AnchorActiveRule::new(
            GlobalRuleRef {
                grammar: GrammarId(0),
                rule: RuleId(0),
            },
            is_first_line,
            anchor_position,
            *pos,
        );

        if cfg!(feature = "debug") {
            eprintln!("[check_while_conditions] Anchors: {active_anchor:?}");
        }

        for while_stack in while_stacks.into_iter().rev() {
            let initial_pat = if let Some(end_pat) = &while_stack.end_pattern {
                end_pat.as_str()
            } else if let Rule::BeginWhile(b) = &self.registry.grammars
                [while_stack.rule_ref.grammar]
                .rules[while_stack.rule_ref.rule]
            {
                let re = &self.registry.grammars[while_stack.rule_ref.grammar].regexes[b.while_];
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
                if let Some(Rule::BeginWhile(begin_while_rule)) = self.registry.grammars
                    [while_stack.rule_ref.grammar]
                    .rules
                    .get(while_stack.rule_ref.rule.as_index())
                    && !begin_while_rule.while_captures.is_empty()
                {
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
                        self.registry.grammars[while_stack.rule_ref.grammar]
                            .rules
                            .get(while_stack.rule_ref.rule.as_index())
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

    fn get_or_create_pattern_set(
        &mut self,
        stack: &StateStack,
        pos: usize,
        is_first_line: bool,
        anchor_position: Option<usize>,
    ) -> &PatternSet {
        let rule_ref = stack.rule_ref;

        let rule = &self.registry.grammars[rule_ref.grammar].rules[rule_ref.rule];
        let rule_w_anchor = AnchorActiveRule::new(rule_ref, is_first_line, anchor_position, pos);

        if cfg!(feature = "debug") {
            eprintln!(
                "[get_or_create_pattern_set] Rule: {rule_ref:?} (grammar: {}) Anchors: {rule_w_anchor:?}",
                &self.registry.grammars[rule_ref.grammar].name
            );
            eprintln!(
                "[get_or_create_pattern_set] Scanning for: pos={pos}, anchor_position={anchor_position:?}"
            );
        }

        let mut end_pattern = stack.end_pattern.as_deref();

        if end_pattern.is_none() {
            match rule {
                Rule::BeginEnd(b) => {
                    let re = &self.registry.grammars[rule_ref.grammar].regexes[b.end];
                    end_pattern = Some(re.pattern());
                }
                Rule::BeginWhile(b) => {
                    let re = &self.registry.grammars[rule_ref.grammar].regexes[b.while_];
                    end_pattern = Some(re.pattern());
                }
                _ => (),
            }
        }

        if !self.pattern_cache.contains_key(&rule_w_anchor) {
            if cfg!(feature = "debug") {
                eprintln!(
                    "[get_or_create_pattern_set] Creating new ps with stack: {stack:#?} for rule {rule_ref:?}"
                );
            }

            let raw_patterns = self
                .registry
                .collect_patterns(self.base_grammar_id, rule_ref);

            let patterns: Vec<_> = raw_patterns
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
                    p.push_back(
                        GlobalRuleRef {
                            grammar: rule_ref.grammar,
                            rule: END_RULE_ID,
                        },
                        pat,
                    )
                } else {
                    p.push_front(
                        GlobalRuleRef {
                            grammar: rule_ref.grammar,
                            rule: END_RULE_ID,
                        },
                        pat,
                    )
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
            let rule = &self.registry.grammars[rule_ref.grammar].rules[rule_ref.rule];
            eprintln!(
                "[get_or_create_pattern_set] Active patterns for rule {:?}.\n{p:?}",
                rule.original_name().unwrap_or("No name")
            );
        }

        p
    }

    fn resolve_captures(
        &mut self,
        stack: &StateStack,
        line: &str,
        rule_captures: &[Option<GlobalRuleRef>],
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
            let rule_ref = if let Some(&Some(r)) = rule_captures.get(i) {
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
            while !local_stack.is_empty()
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
            let rule = &self.registry.grammars[rule_ref.grammar].rules[rule_ref.rule];

            if rule.has_patterns() {
                let mut retokenization_stack = stack.push(rule_ref, None, false, Some(cap_start));

                // Apply rule name scopes to the new state
                retokenization_stack
                    .name_scopes
                    .extend(rule.get_name_scopes(line, captures));

                // Start with name + content scopes for content scopes
                retokenization_stack.content_scopes = retokenization_stack.name_scopes.clone();

                // Apply content scopes
                retokenization_stack
                    .content_scopes
                    .extend(rule.get_content_scopes(line, captures));
                let substring = &line[0..cap_end];
                if cfg!(feature = "debug") {
                    eprintln!(
                        "[resolve_captures] Retokenizing capture at [0..{cap_end}]: {:?}",
                        &line[0..cap_end]
                    );
                    eprintln!(
                        "[resolve_captures] Using substring [0..{cap_end}]: {:?}",
                        substring
                    );
                }
                let (retokenized_acc, _) = self.tokenize_line(
                    retokenization_stack,
                    substring,
                    cap_start,
                    is_first_line && cap_start == 0,
                    false,
                )?;

                for token in retokenized_acc.tokens {
                    // Only include tokens that are within the capture bounds (they should all be valid now)
                    accumulator.produce(token.span.end, &token.scopes);
                }
                continue;
            }

            // For rules without patterns, we still need to apply their scopes
            let rule_scopes = rule.get_name_scopes(line, captures);

            if !rule_scopes.is_empty() {
                let mut base = if let Some((scopes, _)) = local_stack.last() {
                    scopes.clone()
                } else {
                    stack.content_scopes.clone()
                };
                base.extend(rule_scopes);
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
        line_pos: usize,
        is_first_line: bool,
        check_while_conditions: bool,
    ) -> Result<(TokenAccumulator, StateStack), TokenizeError> {
        let mut accumulator = TokenAccumulator::default();
        let mut pos = line_pos;
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

            if let Some(m) =
                self.match_rule_or_injections(&stack, line, pos, is_first_line, anchor_position)?
            {
                if cfg!(feature = "debug") {
                    eprintln!(
                        "[tokenize_line] Matched rule: {:?} from pos {} to {} => {:?}",
                        m.rule_ref.rule,
                        m.start,
                        m.end,
                        &line[m.start..m.end]
                    );
                }

                // Track whether this match has advanced the position
                let has_advanced = m.end > pos;

                // We matched the `end` for this rule, can only happen for BeginEnd rules
                if m.rule_ref.rule == END_RULE_ID
                    && let Rule::BeginEnd(b) =
                        &self.registry.grammars[stack.rule_ref.grammar].rules[stack.rule_ref.rule]
                {
                    if cfg!(feature = "debug") {
                        eprintln!(
                            "[tokenize_line] End rule matched, popping '{}'",
                            b.name.clone().unwrap_or_default()
                        );
                    }
                    accumulator.produce(m.start, &stack.content_scopes);
                    let popped = stack.clone();
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

                    // Grammar pushed & popped a rule without advancing - infinite loop protection
                    // It happens eg for astro grammar
                    if !has_advanced && popped.enter_position == Some(pos) {
                        // See https://github.com/Microsoft/vscode-textmate/issues/12
                        // Let's assume this was a mistake by the grammar author and the intent was to continue in this state
                        stack = popped;
                        accumulator.produce(line.len(), &stack.content_scopes);
                        break;
                    }
                } else {
                    let rule = &self.registry.grammars[m.rule_ref.grammar].rules[m.rule_ref.rule];
                    accumulator.produce(m.start, &stack.content_scopes);
                    let mut new_scopes = stack.content_scopes.clone();
                    new_scopes.extend(rule.get_name_scopes(line, &m.capture_pos));
                    // TODO: improve that push?
                    stack = stack.push(m.rule_ref, anchor_position, m.end == line.len(), Some(pos));
                    stack.name_scopes = new_scopes.clone();
                    stack.content_scopes = new_scopes;
                    stack.end_pattern = None;

                    let mut handle_begin_rule = |re_id: RegexId,
                                                 end_has_backrefs: bool,
                                                 begin_captures: &[Option<GlobalRuleRef>]|
                     -> Result<(), TokenizeError> {
                        let re = &self.registry.grammars[m.rule_ref.grammar].regexes[re_id];
                        if cfg!(feature = "debug") {
                            let rule =
                                &self.registry.grammars[m.rule_ref.grammar].rules[m.rule_ref.rule];
                            eprintln!(
                                "[tokenize_line] Pushing begin rule={:?}",
                                rule.original_name().unwrap_or("No name")
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
                        let mut content_scopes = stack.name_scopes.clone();
                        content_scopes.extend(rule.get_content_scopes(line, &m.capture_pos));
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
                                    self.registry.grammars[m.rule_ref.grammar]
                                        .get_original_rule_name(m.rule_ref.rule)
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

                            // Protection: grammar is not advancing, nor is it pushing/popping
                            // happens for some grammars eg astro
                            if !has_advanced {
                                if cfg!(feature = "debug") {
                                    eprintln!("Match rule didn't advance, safe_pop and stop");
                                }
                                stack = stack.safe_pop();
                                accumulator.produce(line.len(), &stack.content_scopes);
                                break;
                            }
                        }
                        _ => unreachable!("matched something without a regex??"),
                    }
                }

                if has_advanced {
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

    pub(crate) fn tokenize_string(&mut self, text: &str) -> Result<Vec<Vec<Token>>, TokenizeError> {
        if text.is_empty() {
            return Ok(vec![]);
        }

        let mut stack = StateStack::new(
            self.base_grammar_id,
            self.registry.grammars[self.base_grammar_id].scope,
        );
        let mut lines_tokens = Vec::new();
        let mut is_first_line = true;

        // Split by lines and tokenize each line
        for line in text.split('\n') {
            // Always add a new line, some regex expect it
            let line = format!("{line}\n");
            let stack_for_line = stack.reset_enter_position();
            let (mut acc, new_state) =
                self.tokenize_line(stack_for_line, &line, 0, is_first_line, true)?;
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
        }
    }
}

impl std::error::Error for TokenizeError {}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use pretty_assertions::assert_eq;

    use crate::Registry;

    fn get_registry() -> Registry {
        let mut registry = Registry::default();
        for entry in fs::read_dir("grammars-themes/packages/tm-grammars/grammars").unwrap() {
            let path = entry.unwrap().path();
            let grammar_name = path.file_stem().unwrap().to_str().unwrap();
            registry.add_grammar_from_path(path).unwrap();
        }
        registry.link_grammars();
        registry
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
                    out.push_str(&format!("  - {}\n", scope.to_string()));
                }
                out.push('\n');
            }
        }

        out
    }

    #[test]
    fn can_tokenize_like_vscode_textmate() {
        let registry = get_registry();

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
            let tokens = registry
                .tokenize(registry.grammar_id_by_name[&grammar], &sample_content)
                .unwrap();
            let out = format_tokens(&sample_content, tokens);
            assert_eq!(expected.trim(), out.trim());
        }
    }

    #[test]
    fn can_tokenize_specific_text() {
        let registry = get_registry();

        let grammar = "razor";
        // let sample_content = r#"<svg><rect x="0" /></svg>"#;
        let sample_content =
            fs::read_to_string(format!("grammars-themes/samples/{grammar}.sample")).unwrap();
        let expected = fs::read_to_string(format!("src/fixtures/tokens/{grammar}.txt")).unwrap();

        let grammar_id = registry.grammar_id_by_name[grammar];
        let grammar = &registry.grammars[grammar_id];

        let tokens = registry.tokenize(grammar_id, &sample_content).unwrap();
        let out = format_tokens(&sample_content, tokens);

        assert_eq!(out.trim(), expected.trim());
        // println!("{out}");
        // assert!(false);
    }
}
