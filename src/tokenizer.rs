use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::ops::Range;

use crate::Registry;
use crate::grammars::{
    END_RULE_ID, GlobalRuleRef, GrammarId, InjectionPrecedence, PatternSet, PatternSetMatch,
    ROOT_RULE_ID, Regex, RegexId, Rule, resolve_backreferences,
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

/// Individual stack frame - represents a single parsing context
#[derive(Clone, Debug)]
struct StackFrame {
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

/// Keeps track of nested context as well as how to exit that context and the captures
/// strings used in backreferences.
#[derive(Clone)]
struct StateStack {
    /// Stack frames from root to current
    frames: Vec<StackFrame>,
}

impl StateStack {
    pub fn new(grammar_id: GrammarId, grammar_scope: Scope) -> Self {
        Self {
            frames: vec![StackFrame {
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
            }],
        }
    }

    /// Called when entering a nested context: when a BeginEnd or BeginWhile begin pattern matches
    fn push(
        &mut self,
        rule_ref: GlobalRuleRef,
        anchor_position: Option<usize>,
        begin_rule_has_captured_eol: bool,
        enter_position: Option<usize>,
    ) {
        let content_scopes = self.top().content_scopes.clone();

        self.frames.push(StackFrame {
            rule_ref,
            // Start with the same scope they will diverge later
            name_scopes: content_scopes.clone(),
            content_scopes,
            end_pattern: None,
            begin_rule_has_captured_eol,
            anchor_position,
            enter_position,
        });
    }

    /// Push with pre-computed scopes to avoid extra cloning
    fn push_with_scopes(
        &mut self,
        rule_ref: GlobalRuleRef,
        anchor_position: Option<usize>,
        begin_rule_has_captured_eol: bool,
        enter_position: Option<usize>,
        scopes: Vec<Scope>,
    ) {
        self.frames.push(StackFrame {
            rule_ref,
            name_scopes: scopes.clone(),
            content_scopes: scopes,
            end_pattern: None,
            begin_rule_has_captured_eol,
            anchor_position,
            enter_position,
        });
    }

    fn set_content_scopes(&mut self, content_scopes: Vec<Scope>) {
        self.top_mut().content_scopes = content_scopes;
    }

    fn set_end_pattern(&mut self, end_pattern: String) {
        self.top_mut().end_pattern = Some(end_pattern);
    }

    /// Exits the current context, getting back to the parent
    fn pop(&mut self) -> Option<StackFrame> {
        if self.frames.len() > 1 {
            self.frames.pop()
        } else {
            None
        }
    }

    /// Pop but never go below root state - used in infinite loop protection
    fn safe_pop(&mut self) {
        if self.frames.len() > 1 {
            self.frames.pop();
        }
    }

    /// Resets enter_position/anchor_position for all stack elements to None
    fn reset(&mut self) {
        for frame in &mut self.frames {
            frame.enter_position = None;
            frame.anchor_position = None;
        }
    }

    /// Access the top frame of the stack
    fn top(&self) -> &StackFrame {
        self.frames.last().expect("stack never empty")
    }

    /// Mutable access to the top frame of the stack
    fn top_mut(&mut self) -> &mut StackFrame {
        self.frames.last_mut().expect("stack never empty")
    }
}

impl fmt::Debug for StateStack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "StateStack:")?;

        for (depth, frame) in self.frames.iter().enumerate() {
            // Create indentation
            let indent = "  ".repeat(depth);

            // Format the basic info
            write!(
                f,
                "{}grammar={}, rule={}",
                indent, frame.rule_ref.grammar.0, frame.rule_ref.rule.0
            )?;

            // Add name scopes if not empty
            if !frame.name_scopes.is_empty() {
                write!(f, " name=[")?;
                for (i, scope) in frame.name_scopes.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", scope.build_string())?;
                }
                write!(f, "]")?;
            }

            // Add content scopes if not empty
            if !frame.content_scopes.is_empty() {
                write!(f, ", content=[")?;
                for (i, scope) in frame.content_scopes.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", scope.build_string())?;
                }
                write!(f, "]")?;
            }

            // Add end_pattern if present
            if let Some(pattern) = &frame.end_pattern {
                write!(f, ", end_pattern=\"{}\"", pattern)?;
            }

            write!(f, ", anchor_pos={:?}", frame.anchor_position)?;

            // Add enter_position if present and different from anchor_position
            if let Some(enter_pos) = frame.enter_position
                && frame.anchor_position != Some(enter_pos)
            {
                write!(f, ", enter_pos={}", enter_pos)?;
            }

            write!(
                f,
                ", begin_rule_has_captured_eol={}",
                frame.begin_rule_has_captured_eol
            )?;

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
        #[cfg(feature = "debug")]
        log::debug!(
            "[produce]: [{}..{end_pos}]\n{}",
            self.last_end_pos,
            scopes
                .iter()
                .map(|s| format!(" * {s}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
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
enum AnchorActive {
    // Only \A is active
    A,
    // Only \G is active
    G,
    // Both \A and \G are active
    AG,
    // Neither \A nor \G are active
    None,
}

impl AnchorActive {
    pub fn new(is_first_line: bool, anchor_position: Option<usize>, current_pos: usize) -> Self {
        let g_active = if let Some(a_pos) = anchor_position {
            a_pos == current_pos
        } else {
            false
        };

        if is_first_line {
            if g_active {
                AnchorActive::AG
            } else {
                AnchorActive::A
            }
        } else if g_active {
            AnchorActive::G
        } else {
            AnchorActive::None
        }
    }

    /// This follows vscode-textmate and replaces it with something that is very unlikely
    /// to match
    pub fn replace_anchors<'a>(&self, pat: &'a str) -> Cow<'a, str> {
        match self {
            AnchorActive::AG => {
                // No replacements needed
                Cow::Borrowed(pat)
            }
            AnchorActive::A => {
                if pat.contains("\\G") {
                    Cow::Owned(pat.replace("\\G", "\u{FFFF}"))
                } else {
                    Cow::Borrowed(pat)
                }
            }
            AnchorActive::G => {
                if pat.contains("\\A") {
                    Cow::Owned(pat.replace("\\A", "\u{FFFF}"))
                } else {
                    Cow::Borrowed(pat)
                }
            }
            AnchorActive::None => {
                if pat.contains("\\A") || pat.contains("\\G") {
                    Cow::Owned(pat.replace("\\A", "\u{FFFF}").replace("\\G", "\u{FFFF}"))
                } else {
                    Cow::Borrowed(pat)
                }
            }
        }
    }
}

impl fmt::Debug for AnchorActive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            AnchorActive::A => "allow_A=true, allow_G=false",
            AnchorActive::G => "allow_A=false, allow_G=true",
            AnchorActive::AG => "allow_A=true, allow_G=true",
            AnchorActive::None => "allow_A=false, allow_G=false",
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
    /// Runtime pattern cache by rule ID
    pattern_cache: HashMap<(GlobalRuleRef, AnchorActive), PatternSet>,
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
            .collect_injection_patterns(self.base_grammar_id, &stack.top().content_scopes);

        if injection_patterns.is_empty() {
            return Ok(None);
        }

        let mut best_match: Option<(InjectionPrecedence, PatternSetMatch)> = None;

        // Process injections in the order returned by registry (already sorted by precedence)
        for (precedence, rule) in injection_patterns {
            // Use injection override instead of cloning stack
            let pattern_set = self.get_or_create_pattern_set(
                stack,
                pos,
                is_first_line,
                anchor_position,
                Some(rule), // Override rule_ref for injection testing
            );

            if let Some(found) = pattern_set.find_at(line, pos)? {
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
            self.get_or_create_pattern_set(stack, pos, is_first_line, anchor_position, None);
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
    fn check_while_conditions(
        &mut self,
        stack: StateStack,
        line: &str,
        pos: &mut usize,
        acc: &mut TokenAccumulator,
        is_first_line: bool,
    ) -> Result<(StateStack, Option<usize>, bool), TokenizeError> {
        // Initialize anchor position: reset to 0 if previous rule captured EOL, otherwise use stack value
        let mut anchor_position: Option<usize> = if stack.top().begin_rule_has_captured_eol {
            Some(0)
        } else {
            None
        };
        let mut is_first_line = is_first_line;
        let mut stack = stack;

        let mut while_frame_indices = Vec::new();
        for i in 0..stack.frames.len() {
            let frame = &stack.frames[i];
            if let Some(Rule::BeginWhile(_)) = self.registry.grammars[frame.rule_ref.grammar]
                .rules
                .get(frame.rule_ref.rule.as_index())
            {
                while_frame_indices.push(i);
            }
        }

        if while_frame_indices.is_empty() {
            #[cfg(feature = "debug")]
            log::debug!(
                "[check_while_conditions] no while conditions active:\n  {:?}",
                stack
            );
            return Ok((stack, anchor_position, is_first_line));
        }

        let active_anchor = AnchorActive::new(is_first_line, anchor_position, *pos);
        #[cfg(feature = "debug")]
        log::debug!(
            "[check_while_conditions] going to check {} while rules at indices: {:?}, anchors: {active_anchor:?}",
            while_frame_indices.len(),
            while_frame_indices
        );

        for &frame_idx in while_frame_indices.iter() {
            let frame = &stack.frames[frame_idx];
            let initial_pat = if let Some(end_pat) = &frame.end_pattern {
                end_pat.as_str()
            } else if let Rule::BeginWhile(b) =
                &self.registry.grammars[frame.rule_ref.grammar].rules[frame.rule_ref.rule]
            {
                let re = &self.registry.grammars[frame.rule_ref.grammar].regexes[b.while_];
                re.pattern()
            } else {
                unreachable!()
            };
            let while_pat = active_anchor.replace_anchors(initial_pat);
            #[cfg(feature = "debug")]
            log::debug!(
                "[check_while_conditions] Testing while pattern: original={:?}, active_anchor={:?}, pos={}, after anchor replacement: {while_pat:?}",
                initial_pat,
                active_anchor,
                *pos
            );

            let re = if let Some(re) = self.end_regex_cache.get(&*while_pat) {
                re
            } else {
                let owned_pat = while_pat.to_string();
                self.end_regex_cache
                    .insert(owned_pat.clone(), Regex::new(owned_pat.clone()));
                self.end_regex_cache.get(&owned_pat).unwrap()
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

                acc.produce(absolute_start, &frame.content_scopes);
                // Handle while captures if they exist
                if let Some(Rule::BeginWhile(begin_while_rule)) = self.registry.grammars
                    [frame.rule_ref.grammar]
                    .rules
                    .get(frame.rule_ref.rule.as_index())
                    && !begin_while_rule.while_captures.is_empty()
                {
                    let captures_pos: Vec<Option<(usize, usize)>> = (0..cap.len())
                        .map(|i| cap.pos(i).map(|(s, e)| (*pos + s, *pos + e)))
                        .collect();

                    // Create temporary StateStack only for resolve_captures
                    let temp_while_stack = StateStack {
                        frames: stack.frames[0..=frame_idx].to_vec(),
                    };
                    self.resolve_captures(
                        &temp_while_stack,
                        line,
                        &begin_while_rule.while_captures,
                        &captures_pos,
                        acc,
                        is_first_line,
                    )?;
                }

                // Produce token for the while match itself
                acc.produce(absolute_end, &frame.content_scopes);

                // Advance position and update anchor - matches VSCode behavior
                if absolute_end > *pos {
                    *pos = absolute_end;
                    anchor_position = Some(*pos);
                    is_first_line = false;
                }
            } else {
                #[cfg(feature = "debug")]
                log::debug!(
                    "[check_while_conditions] No while match found, popping: {:?}",
                    self.registry.grammars[frame.rule_ref.grammar]
                        .rules
                        .get(frame.rule_ref.rule.as_index())
                        .unwrap()
                        .original_name()
                );

                // Create StateStack and pop the while frame
                let mut popped_stack = StateStack {
                    frames: stack.frames[0..=frame_idx].to_vec(),
                };
                popped_stack.pop();
                stack = popped_stack;
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
        injection_rule_override: Option<GlobalRuleRef>,
    ) -> &PatternSet {
        let rule_ref = injection_rule_override.unwrap_or(stack.top().rule_ref);
        let rule = &self.registry.grammars[rule_ref.grammar].rules[rule_ref.rule];
        let anchor_context = AnchorActive::new(is_first_line, anchor_position, pos);

        #[cfg(feature = "debug")]
        {
            log::debug!(
                "[get_or_create_pattern_set] Rule: {rule_ref:?} (grammar: {})",
                &self.registry.grammars[rule_ref.grammar].name
            );
            log::debug!(
                "[get_or_create_pattern_set] Scanning for: pos={pos}, anchor_position={anchor_position:?}"
            );
        }
        // Get end pattern from stack or rule definition when it has backref filled
        let mut end_pattern = stack.top().end_pattern.as_deref();
        // otherwise we get it from the rule directly
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

        let key = (rule_ref, anchor_context);
        if let Some(p) = self.pattern_cache.get_mut(&key) {
            if let Rule::BeginEnd(b) = rule
                && let Some(end_pat) = end_pattern
            {
                let pat = anchor_context.replace_anchors(end_pat);
                if b.apply_end_pattern_last {
                    p.update_last(pat.as_ref())
                } else {
                    p.update_front(pat.as_ref())
                };
            }
        } else {
            // Collect base patterns from grammar
            let raw_patterns = self
                .registry
                .collect_patterns(self.base_grammar_id, rule_ref);

            // Apply anchor replacements to all patterns
            let mut patterns: Vec<_> = raw_patterns
                .into_iter()
                .map(|(rule, pat)| (rule, anchor_context.replace_anchors(pat).into_owned()))
                .collect();

            // Insert end pattern at correct position if this is a BeginEnd rule
            if let Some(pat) = end_pattern
                && let Rule::BeginEnd(b) = rule
            {
                let end_pat_with_anchors = anchor_context.replace_anchors(pat);
                let end_rule_ref = GlobalRuleRef {
                    grammar: rule_ref.grammar,
                    rule: END_RULE_ID,
                };

                if b.apply_end_pattern_last {
                    patterns.push((end_rule_ref, end_pat_with_anchors.into_owned()));
                } else {
                    patterns.insert(0, (end_rule_ref, end_pat_with_anchors.into_owned()));
                }
            }

            let p = PatternSet::new(patterns);
            self.pattern_cache.insert(key, p);
        }

        let p = &self.pattern_cache[&key];

        #[cfg(feature = "debug")]
        {
            log::debug!(
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
        let mut local_stack: Vec<(Vec<Scope>, usize)> = Vec::with_capacity(2);

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
                accumulator.produce(cap_start, &stack.top().content_scopes);
            }

            //  Check if it has captures. If it does we need to call tokenize_string
            let rule = &self.registry.grammars[rule_ref.grammar].rules[rule_ref.rule];

            if rule.has_patterns() {
                let mut retokenization_stack = stack.clone();
                retokenization_stack.push(rule_ref, None, false, Some(cap_start));

                // Apply rule name scopes to the new state
                retokenization_stack
                    .top_mut()
                    .name_scopes
                    .extend(rule.get_name_scopes(line, captures));

                // Start with name + content scopes for content scopes
                retokenization_stack.top_mut().content_scopes =
                    retokenization_stack.top().name_scopes.clone();

                // Apply content scopes
                retokenization_stack
                    .top_mut()
                    .content_scopes
                    .extend(rule.get_content_scopes(line, captures));
                let substring = &line[0..cap_end];
                #[cfg(feature = "debug")]
                {
                    log::debug!(
                        "[resolve_captures] Retokenizing capture at [0..{cap_end}]: {:?}",
                        &line[0..cap_end]
                    );
                    log::debug!(
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
                    stack.top().content_scopes.clone()
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
            #[cfg(feature = "debug")]
            {
                log::trace!("");
                log::trace!("[tokenize_line] Scanning {pos}: |{:?}|", &line[pos..]);
            }

            if let Some(m) =
                self.match_rule_or_injections(&stack, line, pos, is_first_line, anchor_position)?
            {
                #[cfg(feature = "debug")]
                log::debug!(
                    "[tokenize_line] Matched rule: {:?} from pos {} to {} => {:?}",
                    m.rule_ref.rule,
                    m.start,
                    m.end,
                    &line[m.start..m.end]
                );

                // Track whether this match has advanced the position
                let has_advanced = m.end > pos;

                #[cfg(feature = "debug")]
                if m.rule_ref.rule == END_RULE_ID {
                    log::debug!(
                        "[tokenize_line] END RULE MATCHED at pos={}, m.start={}, m.end={}",
                        pos,
                        m.start,
                        m.end
                    );
                }
                // We matched the `end` for this rule, can only happen for BeginEnd rules
                if m.rule_ref.rule == END_RULE_ID
                    && let Rule::BeginEnd(b) = &self.registry.grammars[stack.top().rule_ref.grammar]
                        .rules[stack.top().rule_ref.rule]
                {
                    #[cfg(feature = "debug")]
                    {
                        log::debug!(
                            "[tokenize_line] End rule matched, popping '{}'",
                            b.name.clone().unwrap_or_default()
                        );
                        log::debug!(
                            "[BEFORE POP] Current anchor_position: {:?}",
                            anchor_position
                        );
                        log::debug!("[BEFORE POP] Stack: {:?}", stack);
                    }
                    accumulator.produce(m.start, &stack.top().content_scopes);
                    let popped_enter_position = stack.top().enter_position; // Save for infinite loop protection
                    let popped_anchor_position = stack.top().anchor_position;
                    #[cfg(feature = "debug")]
                    log::trace!("[POPPED RULE] Stack: {:?}", stack);
                    stack.set_content_scopes(stack.top().name_scopes.clone());
                    self.resolve_captures(
                        &stack,
                        line,
                        &b.end_captures,
                        &m.capture_pos,
                        &mut accumulator,
                        is_first_line,
                    )?;
                    accumulator.produce(m.end, &stack.top().content_scopes);

                    // Pop to parent state and update anchor position
                    let popped_frame = stack.pop().unwrap();
                    anchor_position = popped_anchor_position;
                    #[cfg(feature = "debug")]
                    {
                        log::debug!(
                            "[AFTER POP] Restored anchor_position: {:?}",
                            anchor_position
                        );
                        log::debug!("[AFTER POP] New stack: {:?}", stack);
                    }

                    // Grammar pushed & popped a rule without advancing - infinite loop protection
                    // It happens eg for astro grammar
                    if !has_advanced && popped_enter_position == Some(pos) {
                        // See https://github.com/Microsoft/vscode-textmate/issues/12
                        // Like vscode-textmate, restore the popped frame to keep the rule active
                        stack.frames.push(popped_frame);
                        #[cfg(feature = "debug")]
                        log::debug!(
                            "[INFINITE LOOP PROTECTION] Restored rule to stack: {:?}",
                            stack
                        );
                        accumulator.produce(line.len(), &stack.top().content_scopes);
                        break;
                    }
                } else {
                    let rule = &self.registry.grammars[m.rule_ref.grammar].rules[m.rule_ref.rule];
                    accumulator.produce(m.start, &stack.top().content_scopes);
                    let mut new_scopes = stack.top().content_scopes.clone();
                    new_scopes.extend(rule.get_name_scopes(line, &m.capture_pos));
                    // Use push_with_scopes to avoid double-cloning
                    stack.push_with_scopes(
                        m.rule_ref,
                        anchor_position,
                        m.end == line.len(),
                        Some(pos),
                        new_scopes,
                    );
                    stack.top_mut().end_pattern = None;

                    let mut handle_begin_rule = |re_id: RegexId,
                                                 end_has_backrefs: bool,
                                                 begin_captures: &[Option<GlobalRuleRef>]|
                     -> Result<(), TokenizeError> {
                        let re = &self.registry.grammars[m.rule_ref.grammar].regexes[re_id];
                        #[cfg(feature = "debug")]
                        {
                            let rule =
                                &self.registry.grammars[m.rule_ref.grammar].rules[m.rule_ref.rule];
                            log::debug!(
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
                        accumulator.produce(m.end, &stack.top().content_scopes);
                        anchor_position = Some(m.end);
                        let mut content_scopes = stack.top().name_scopes.clone();
                        content_scopes.extend(rule.get_content_scopes(line, &m.capture_pos));
                        stack.set_content_scopes(content_scopes);

                        if end_has_backrefs {
                            let resolved_end =
                                resolve_backreferences(re.pattern(), line, &m.capture_pos);
                            stack.set_end_pattern(resolved_end);
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
                            #[cfg(feature = "debug")]
                            log::debug!(
                                "[handle_match] Matched '{}'",
                                self.registry.grammars[m.rule_ref.grammar]
                                    .get_original_rule_name(m.rule_ref.rule)
                                    .unwrap_or_default()
                            );
                            self.resolve_captures(
                                &stack,
                                line,
                                &r.captures,
                                &m.capture_pos,
                                &mut accumulator,
                                is_first_line,
                            )?;
                            accumulator.produce(m.end, &stack.top().content_scopes);
                            // pop rule immediately since it is a MatchRule
                            stack.pop();

                            // Protection: grammar is not advancing, nor is it pushing/popping
                            // happens for some grammars eg astro
                            if !has_advanced {
                                #[cfg(feature = "debug")]
                                log::warn!("Match rule didn't advance, safe_pop and stop");
                                stack.safe_pop();
                                accumulator.produce(line.len(), &stack.top().content_scopes);
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
                #[cfg(feature = "debug")]
                log::debug!("[tokenize_line] no more matches");
                // No more matches - emit final token and stop
                accumulator.produce(line.len(), &stack.top().content_scopes);
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
            let (mut acc, mut new_state) =
                self.tokenize_line(stack, &line, 0, is_first_line, true)?;
            acc.finalize(line.len());
            lines_tokens.push(acc.tokens);
            new_state.reset();
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
    use crate::registry::normalize_string;

    fn get_registry() -> Registry {
        let mut registry = Registry::default();
        for entry in fs::read_dir("grammars-themes/packages/tm-grammars/grammars").unwrap() {
            let path = entry.unwrap().path();
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
                    out.push_str(&format!("  - {scope}\n"));
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
            let sample_content = normalize_string(&fs::read_to_string(sample_path).unwrap());
            let tokens = registry
                .tokenize(registry.grammar_id_by_name[&grammar], &sample_content)
                .unwrap();
            let out = format_tokens(&sample_content, tokens);
            assert_eq!(expected.trim(), out.trim());
        }
    }

    #[test]
    fn can_tokenize_specific_text() {
        env_logger::init();
        let registry = get_registry();

        let grammar = "stylus";
        // let sample_content = r#"<svg><rect x="0" /></svg>"#;
        let sample_content = normalize_string(
            &fs::read_to_string(format!("grammars-themes/samples/{grammar}.sample")).unwrap(),
        );
        let expected = fs::read_to_string(format!("src/fixtures/tokens/{grammar}.txt")).unwrap();

        let grammar_id = registry.grammar_id_by_name[grammar];

        let tokens = registry.tokenize(grammar_id, &sample_content).unwrap();
        let out = format_tokens(&sample_content, tokens);

        assert_eq!(out.trim(), expected.trim());
        // println!("{out}");
        // assert!(false);
    }
}
