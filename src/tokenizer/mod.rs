//! This file replicates the logic of <https://github.com/microsoft/vscode-textmate>

use std::collections::HashMap;
use std::ops::Range;

use onig::Region;

use serde::{Deserialize, Serialize};

use crate::Registry;
use crate::grammars::{
    END_RULE_ID, GlobalRuleRef, GrammarId, InjectionPrecedence, PatternSet, PatternSetMatch, Regex,
    RegexId, Rule, resolve_backreferences,
};
use crate::scope::Scope;
use crate::tokenizer::anchors::AnchorActive;
use crate::tokenizer::stack::StateStack;

mod anchors;
mod stack;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Token {
    /// Byte span within the line (start inclusive, end exclusive, 0-based)
    pub span: Range<usize>,
    /// Hierarchical scope names, ordered from outermost to innermost
    /// (e.g., source.js -> string.quoted.double -> punctuation.definition.string).
    pub scopes: Vec<Scope>,
}

/// Small wrapper so we make we only produce valid tokens.
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

#[derive(Debug)]
pub struct Tokenizer<'g> {
    /// The index in the grammars vec below we will use to start the process
    base_grammar_id: GrammarId,
    /// All the grammars in the registry
    registry: &'g Registry,
    /// Runtime pattern cache by rule ID
    pattern_cache: HashMap<GlobalRuleRef, PatternSet>,
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
    ) -> Result<Option<(InjectionPrecedence, PatternSetMatch)>, String> {
        let anchor_context = AnchorActive::new(is_first_line, anchor_position, pos);
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
                Some(rule), // Override rule_ref for injection testing
            );

            if let Some(found) =
                pattern_set.find_at(line, pos, anchor_context.to_search_options())?
            {
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
    ) -> Result<Option<PatternSetMatch>, String> {
        let anchor_context = AnchorActive::new(is_first_line, anchor_position, pos);
        // Get regular rule patterns
        let pattern_set = self.get_or_create_pattern_set(stack, None);
        let regular_match = pattern_set.find_at(line, pos, anchor_context.to_search_options())?;

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
    ) -> Result<(StateStack, Option<usize>, bool), String> {
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
            let while_pat = if let Some(end_pat) = &frame.end_pattern {
                end_pat.as_str()
            } else if let Rule::BeginWhile(b) =
                &self.registry.grammars[frame.rule_ref.grammar].rules[frame.rule_ref.rule]
            {
                let re = &self.registry.grammars[frame.rule_ref.grammar].regexes[b.while_];
                re.pattern()
            } else {
                unreachable!()
            };
            #[cfg(feature = "debug")]
            log::debug!(
                "[check_while_conditions] Testing while pattern: {:?}, active_anchor={:?}, pos={}",
                initial_pat,
                active_anchor,
                *pos
            );

            let re = if let Some(re) = self.end_regex_cache.get(while_pat) {
                re
            } else {
                self.end_regex_cache
                    .insert(while_pat.to_owned(), Regex::new(while_pat.to_owned()));
                self.end_regex_cache.get(while_pat).unwrap()
            };

            let search_text = line.get(*pos..).unwrap_or("");
            let compiled_re = re
                .compiled()
                .ok_or_else(|| format!("While pattern {while_pat} was invalid"))?;

            let mut region = Region::new();
            if compiled_re
                .search_with_options(
                    search_text,
                    0,
                    search_text.len(),
                    active_anchor.to_search_options(),
                    Some(&mut region),
                )
                .is_some()
                && let Some((start, end)) = region.pos(0)
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
                    let captures_pos: Vec<Option<(usize, usize)>> = (0..region.len())
                        .map(|i| region.pos(i).map(|(s, e)| (*pos + s, *pos + e)))
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
        injection_rule_override: Option<GlobalRuleRef>,
    ) -> &PatternSet {
        let rule_ref = injection_rule_override.unwrap_or(stack.top().rule_ref);
        let rule = &self.registry.grammars[rule_ref.grammar].rules[rule_ref.rule];

        #[cfg(feature = "debug")]
        {
            log::debug!(
                "[get_or_create_pattern_set] Rule: {rule_ref:?} (grammar: {})",
                &self.registry.grammars[rule_ref.grammar].name
            );
            log::debug!("[get_or_create_pattern_set] Scanning patterns");
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

        let key = rule_ref;
        if let Some(p) = self.pattern_cache.get_mut(&key) {
            if let Rule::BeginEnd(b) = rule
                && let Some(end_pat) = end_pattern
            {
                if b.apply_end_pattern_last {
                    p.update_last(end_pat)
                } else {
                    p.update_front(end_pat)
                };
            }
        } else {
            // Collect base patterns from grammar
            let raw_patterns = self
                .registry
                .collect_patterns(self.base_grammar_id, rule_ref);

            // Collect patterns as-is
            let mut patterns: Vec<_> = raw_patterns
                .into_iter()
                .map(|(rule, pat)| (rule, pat.to_owned()))
                .collect();

            // Insert end pattern at correct position if this is a BeginEnd rule
            if let Some(pat) = end_pattern
                && let Rule::BeginEnd(b) = rule
            {
                let end_rule_ref = GlobalRuleRef {
                    grammar: rule_ref.grammar,
                    rule: END_RULE_ID,
                };

                if b.apply_end_pattern_last {
                    patterns.push((end_rule_ref, pat.to_owned()));
                } else {
                    patterns.insert(0, (end_rule_ref, pat.to_owned()));
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
    ) -> Result<(), String> {
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
    ) -> Result<(TokenAccumulator, StateStack), String> {
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
                     -> Result<(), String> {
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

    pub(crate) fn tokenize_string(&mut self, text: &str) -> Result<Vec<Vec<Token>>, String> {
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
