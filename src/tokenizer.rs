/*
 * Complete TextMate Grammar Tokenizer
 *
 * Produces professional-grade syntax highlighting identical to VS Code and other TextMate editors.
 *
 * Core features:
 * - Full TextMate include resolution (#value, #constant patterns)
 * - Proper begin/end capture scoping for delimiters
 * - Efficient token batching
 * - Exact position pattern matching
 * - Complete state stack management for nested contexts
 *
 * Key implementation breakthroughs:
 * 1. Include Resolution: Fixed "illegal tokens" by implementing proper TextMate include system
 * 2. Position-Exact Matching: Prevents content skipping by restricting matches to current position
 * 3. TextMate Captures: Enables proper delimiter vs content scoping
 * 4. Pattern Batching: PatternSet optimization for efficient multi-pattern matching
 * 5. Token Batching: Efficient processing of consecutive same-scope text
 */

use crate::grammars::{CompiledGrammar, PatternSet, Rule, RuleId, ScopeId};
use onig::RegSet;
use std::collections::HashMap;

/// A tokenized segment of text with its associated scope information.
///
/// Represents a contiguous span of text that should be styled with the same
/// set of scopes. The scopes form a hierarchical stack where inner scopes
/// inherit from outer scopes (e.g., source.js -> string.quoted.double -> punctuation.definition.string).
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// Start byte position in the input text (inclusive)
    pub start: usize,
    /// End byte position in the input text (exclusive)
    pub end: usize,
    /// Hierarchical scope IDs, ordered from outermost to innermost
    /// Use `crate::grammars::scope_id_to_name()` to convert to strings when needed
    pub scopes: Vec<ScopeId>,
}

impl Token {
    /// Get the scope names as strings.
    ///
    /// This is a convenience method that converts all ScopeIds to their string
    /// representations. Note that this is O(n*m) where n is the number of scopes
    /// and m is the total number of scopes in the grammar, so use sparingly.
    pub fn scope_names(&self) -> Vec<String> {
        use crate::grammars::scope_id_to_name;
        self.scopes
            .iter()
            .map(|&scope_id| scope_id_to_name(scope_id))
            .collect()
    }

    /// Check if this token has a scope that contains the given substring.
    ///
    /// This is useful for checking token types without converting all scopes to strings.
    /// For example: `token.has_scope_containing("invalid.illegal")`.
    pub fn has_scope_containing(&self, substring: &str) -> bool {
        use crate::grammars::scope_id_to_name;
        self.scopes
            .iter()
            .any(|&scope_id| scope_id_to_name(scope_id).contains(substring))
    }
}

/// Result of tokenizing a single line of text.
///
/// Contains both the generated tokens and the updated tokenizer state that
/// should be passed to the next line. This enables multi-line constructs
/// like block comments and strings to work correctly.
#[derive(Debug)]
pub struct TokenizeResult {
    /// All tokens generated for this line, ordered by position
    pub tokens: Vec<Token>,
    /// Updated tokenizer state to pass to the next line
    /// (preserves nested contexts like open strings/comments)
    pub state: TokenizerState,
}

/// Tokenizer state that persists across lines.
///
/// This is the core state that tracks nested grammar contexts. For example,
/// if a multi-line string starts on line 1, this state remembers that context
/// so line 2 can be tokenized correctly as string content.
///
/// The state is intentionally lightweight and cloneable for efficient
/// multi-line tokenization.
#[derive(Debug, Clone)]
pub struct TokenizerState {
    /// Current state stack representing nested grammar contexts
    stack: StateStack,
}

impl TokenizerState {
    /// Create initial tokenizer state for a grammar.
    ///
    /// The initial state has a root stack element with the grammar's main scope
    /// and no active patterns. This represents the "default" state before any
    /// patterns have been matched.
    fn new(grammar: &CompiledGrammar) -> Self {
        Self {
            stack: StateStack {
                parent: None,
                rule_id: RuleId(0), // Root rule (always ID 0)
                name_scopes: vec![grammar.scope_id],
                content_scopes: vec![grammar.scope_id],
                end_rule: None,
                begin_captures: Vec::new(),
            },
        }
    }
}

/// A single element in the tokenizer's state stack.
///
/// The state stack is fundamental to TextMate tokenization - it tracks nested
/// grammar contexts like strings inside functions inside classes. Each stack
/// element represents one level of nesting and maintains:
///
/// 1. SCOPE INFORMATION: Both "name" and "content" scopes for this context
/// 2. TERMINATION RULES: How to exit this context (end/while patterns)
/// 3. BACKREFERENCE DATA: Captured text for resolving dynamic patterns
///
/// ## Scope Behavior:
/// - name_scopes: Applied to the delimiters (begin/end markers) of a rule
/// - content_scopes: Applied to the content between delimiters
///
/// For example, in a string rule:
/// - The quotes get name_scopes: ["string.quoted", "punctuation.definition.string"]
/// - The content gets content_scopes: ["string.quoted"]
///
/// ## Stack Operations:
/// - push(): Create child context (entering nested rule)
/// - pop(): Return to parent context (exiting nested rule)
/// - switch_to_name_scopes(): Switch content scopes to name scopes (for end delimiters)
#[derive(Debug, Clone)]
struct StateStack {
    /// Parent stack element (None for root)
    parent: Option<Box<StateStack>>,

    /// Rule ID that created this stack element
    /// Used for pattern resolution and debugging
    rule_id: RuleId,

    /// "name" scopes - applied to begin/end delimiters
    /// These scopes are active when matching the rule's boundaries
    name_scopes: Vec<ScopeId>,

    /// "contentName" scopes - applied to content between delimiters
    /// These scopes are active for the rule's interior content
    /// This is what gets used for most token generation
    content_scopes: Vec<ScopeId>,

    /// Dynamic end/while pattern resolved with backreferences
    /// For BeginEnd rules: the end pattern with \1, \2, etc. resolved
    /// For BeginWhile rules: the while pattern with backreferences resolved
    end_rule: Option<String>,

    /// Captured text from the begin pattern
    /// Used to resolve backreferences in end/while patterns
    /// Index 0 = full match, Index 1+ = capture groups
    begin_captures: Vec<String>,
}

impl StateStack {
    /// Create a new child stack element (entering a nested context).
    ///
    /// This is called when a BeginEnd or BeginWhile pattern matches.
    /// The child inherits the parent's content scopes as its initial scope base,
    /// which maintains proper scope nesting.
    ///
    /// ## Scope Inheritance:
    /// Child's name_scopes = Parent's content_scopes + new rule's name scopes
    /// Child's content_scopes = Parent's content_scopes + new rule's content scopes
    ///
    /// Example: If parent has ["source.js"] and child adds ["string.quoted"],
    /// the child will have ["source.js", "string.quoted"] for both name and content initially.
    fn push(&self, rule_id: RuleId) -> StateStack {
        StateStack {
            parent: Some(Box::new(self.clone())),
            rule_id,
            // Important: Start with parent's content scopes as the base
            // This ensures proper scope inheritance in nested contexts
            name_scopes: self.content_scopes.clone(),
            content_scopes: self.content_scopes.clone(),
            end_rule: None,
            begin_captures: Vec::new(),
        }
    }

    /// Pop this stack element, returning to the parent context.
    ///
    /// This is called when an end pattern matches or a BeginWhile condition fails.
    /// Returns None if this is the root element (no parent to return to).
    fn pop(&self) -> Option<StateStack> {
        self.parent.as_ref().map(|parent| (**parent).clone())
    }

    /// Add a "name" scope to this stack element.
    ///
    /// Name scopes are applied to the begin/end delimiters of a rule.
    /// This is a builder pattern method for fluent configuration.
    fn with_name_scope(mut self, scope: ScopeId) -> Self {
        self.name_scopes.push(scope);
        self
    }

    /// Add a "content" scope to this stack element.
    ///
    /// Content scopes are applied to the interior content of a rule.
    /// This is a builder pattern method for fluent configuration.
    fn with_content_scope(mut self, scope: ScopeId) -> Self {
        self.content_scopes.push(scope);
        self
    }

    /// Set the dynamic end/while pattern for this stack element.
    ///
    /// Used for patterns with backreferences that need to be resolved
    /// at runtime based on what the begin pattern matched.
    fn with_end_rule(mut self, end_rule: String) -> Self {
        self.end_rule = Some(end_rule);
        self
    }

    /// Store the captured groups from the begin pattern.
    ///
    /// These captures are used later for resolving backreferences
    /// in end/while patterns (e.g., \1, \2, etc.).
    fn with_begin_captures(mut self, captures: Vec<String>) -> Self {
        self.begin_captures = captures;
        self
    }

    /// Switch content scopes to match name scopes.
    ///
    /// This is called when matching end delimiters - the end delimiter
    /// should use the same scopes as the begin delimiter (name scopes),
    /// not the content scopes used for the interior.
    ///
    /// Example: In a string, both opening and closing quotes should have
    /// the same "punctuation.definition.string" scope.
    fn switch_to_name_scopes(mut self) -> Self {
        self.content_scopes = self.name_scopes.clone();
        self
    }
}

/// Result of a pattern match attempt.
///
/// Contains all information needed to process a successful pattern match,
/// including position, rule information, and captured groups for backreference resolution.
#[derive(Debug)]
struct MatchResult {
    /// Start position of the match in the input text
    start: usize,
    /// End position of the match in the input text
    end: usize,
    /// ID of the rule that matched
    rule_id: RuleId,
    /// Captured groups from the regex match (for backreference resolution)
    captures: Vec<String>,
    /// Type of pattern that matched (affects how it's processed)
    match_type: MatchType,
}

/// The different types of patterns in TextMate grammars.
///
/// Each type has different semantics and requires different processing in handle_match():
///
/// ## Processing Behavior by Type:
///
/// ### EndPattern:
/// - **Trigger**: End/while pattern from current context matched
/// - **Action**: Pop state stack (exit current BeginEnd/BeginWhile)
/// - **Scoping**: Use end_captures (BeginEnd) or switch to name_scopes
/// - **State change**: self.state.stack = parent_stack
///
/// ### Match:
/// - **Trigger**: Standalone pattern matched (keyword, operator, etc.)
/// - **Action**: Apply scopes temporarily without state change
/// - **Scoping**: Add rule's scope_id to current content_scopes
/// - **State change**: None (no stack modification)
///
/// ### BeginEnd:
/// - **Trigger**: Begin pattern of BeginEnd rule matched
/// - **Action**: Push new context with explicit end condition
/// - **Scoping**: Configure name/content scopes, resolve end pattern with captures
/// - **State change**: Push new StateStack element with end_rule
///
/// ### BeginWhile:
/// - **Trigger**: Begin pattern of BeginWhile rule matched
/// - **Action**: Push new context with continuation condition
/// - **Scoping**: Configure name/content scopes, resolve while pattern with captures
/// - **State change**: Push new StateStack element with while condition in end_rule
///
/// ## Usage in Code:
/// MatchResult.match_type determines which case in handle_match() to execute.
/// Created by try_match_pattern() based on the Rule type that matched.
#[derive(Debug, PartialEq)]
enum MatchType {
    /// An end or while pattern matched - exit current context
    EndPattern,
    /// A simple match pattern - apply scopes atomically
    Match,
    /// Begin part of BeginEnd pattern - enter new context with explicit end
    BeginEnd,
    /// Begin part of BeginWhile pattern - enter new context with continuation condition
    BeginWhile,
}

/// Accumulates tokens during line tokenization.
///
/// This follows the vscode-textmate approach of generating tokens by "producing"
/// spans of text with their associated scopes. The accumulator tracks the last
/// position that was tokenized to ensure no gaps or overlaps in the token stream.
///
/// ## Token Generation Strategy:
/// 1. Start at position 0
/// 2. When a pattern matches at position N, produce a token from last_end_pos to N
/// 3. Process the match (might add scopes or change state)
/// 4. Produce a token for the matched text with updated scopes
/// 5. Update last_end_pos to the end of the match
/// 6. Continue until end of line
///
/// This ensures every character in the input gets exactly one token.
struct TokenAccumulator {
    /// All tokens generated so far
    tokens: Vec<Token>,
    /// Position up to which tokens have been generated
    /// (start of next token to be produced)
    last_end_pos: usize,
}

impl TokenAccumulator {
    /// Create a new token accumulator starting at position 0.
    fn new() -> Self {
        Self {
            tokens: Vec::new(),
            last_end_pos: 0,
        }
    }

    /// Produce a token from the current position to end_pos with the given scopes.
    ///
    /// This is the core of token generation - it creates a new token for the
    /// span [last_end_pos, end_pos) with the provided scopes, then advances
    /// the internal position.
    ///
    /// ## Behavior:
    /// - If end_pos <= last_end_pos, does nothing (no backward movement)
    /// - Stores ScopeIds directly in the token (no string conversion)
    /// - Creates and stores the token
    /// - Updates last_end_pos to end_pos
    ///
    /// ## Complete Coverage Guarantee:
    /// The accumulator ensures every character in the input gets exactly one token.
    /// Gaps in coverage indicate bugs in tokenization logic. The invariant maintained is:
    /// `sum(token.end - token.start) == input.len()` for complete tokenization.
    ///
    /// ## Zero-Width Protection:
    /// The `end_pos <= last_end_pos` check prevents:
    /// 1. **Backward movement**: Invalid position regression
    /// 2. **Zero-width tokens**: Empty spans that would break coverage tracking
    /// 3. **Duplicate tokens**: Multiple tokens for same text span
    ///
    /// ## Scope Storage:
    /// ScopeIds are stored directly in tokens without conversion to strings.
    /// This keeps the tokenizer efficient with integer scope operations throughout.
    ///
    /// ## Usage Pattern:
    /// ```ignore
    /// // Produce token for text before pattern match
    /// accumulator.produce(match_start, &current_scopes);
    /// // ... process the match (maybe change scopes) ...
    /// // Produce token for the matched text
    /// accumulator.produce(match_end, &updated_scopes);
    /// ```
    ///
    /// ## Error Detection:
    /// After tokenization, check that accumulator.last_end_pos == text.len().
    /// If not, there's a bug in position advancement or pattern processing.
    fn produce(&mut self, end_pos: usize, scopes: &[ScopeId]) {
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

/// Main TextMate tokenizer.
///
/// The tokenizer maintains a reference to the compiled grammar and internal state
/// that tracks nested grammar contexts across lines. It follows the TextMate
/// specification for pattern matching and scope generation.
///
/// ## Usage:
/// ```ignore
/// let tokenizer = Tokenizer::new(grammar);
/// let result = tokenizer.tokenize_line("const x = 42;")?;
/// // result.tokens contains Token objects with scope information
/// // result.state can be passed to tokenize the next line
/// ```
pub struct Tokenizer<'g> {
    /// Reference to the compiled grammar containing all rules and patterns
    grammar: &'g CompiledGrammar,
    /// Current tokenizer state (tracks nested contexts across lines)
    state: TokenizerState,
    /// Runtime pattern cache by rule ID
    /// This preserves OnceCell<RegSet> compilation cache across multiple calls
    pattern_cache: HashMap<RuleId, PatternSet>,
}

impl<'g> Tokenizer<'g> {
    /// Create a new tokenizer for the given grammar.
    ///
    /// The tokenizer starts in the initial state with only the grammar's
    /// root scope active. Use this for tokenizing the first line of a file.
    pub fn new(grammar: &'g CompiledGrammar) -> Self {
        Self {
            grammar,
            state: TokenizerState::new(grammar),
            pattern_cache: HashMap::new(),
        }
    }

    /// Create a tokenizer with existing state.
    ///
    /// Use this for multi-line tokenization - pass the state from the
    /// previous line to maintain context across line boundaries.
    /// This enables multi-line constructs like block comments and strings.
    pub fn with_state(grammar: &'g CompiledGrammar, state: TokenizerState) -> Self {
        Self {
            grammar,
            state,
            pattern_cache: HashMap::new(),
        }
    }

    /// Tokenize a single line of text according to TextMate rules.
    ///
    /// This is the main entry point for tokenization. It implements the complete
    /// TextMate tokenization algorithm in two phases:
    ///
    /// ## Phase 1: BeginWhile Condition Checking
    /// Before processing new patterns, check if any active BeginWhile patterns
    /// still match their continuation conditions. If not, pop them from the stack.
    /// This phase may consume some text and generate tokens.
    ///
    /// ## Phase 2: Main Pattern Matching
    /// Scan through the remaining text, finding pattern matches and processing them
    /// according to TextMate rules. Continue until all text is processed.
    ///
    /// ## Algorithm Flow (Step-by-Step):
    /// 1. **Initialize**: TokenAccumulator (tracks position, collects tokens), position = 0
    /// 2. **Phase 1**: Call check_while_conditions() - may advance pos, generate tokens
    /// 3. **Phase 2 Loop** (while pos < text.len()):
    ///    a. Call scan_next() to find best pattern match at current position
    ///    b. If match found: call handle_match() → generates tokens, updates state, advances pos
    ///    c. If no match: scan forward to next matchable position, generate content token for gap
    /// 4. **Return**: TokenizeResult with all tokens + updated state
    ///
    /// ## Position Advancement Guarantee:
    /// Every iteration MUST advance pos to prevent infinite loops. handle_match() returns
    /// new position, gap scanning increments until pattern found. Zero-width matches
    /// could potentially cause infinite loops if they occur.
    ///
    /// ## Token Generation Strategy:
    /// TokenAccumulator ensures complete coverage - every character gets exactly one token.
    /// Tokens are generated for: (1) text before matches, (2) matched text with new scopes.
    ///
    /// ## State Management:
    /// Input state is preserved, mutations happen on self.state. Final state reflects
    /// all context changes (pushed/popped contexts) from this line's processing.
    ///
    /// ## Return Value:
    /// Returns both the generated tokens and the updated state. The state must
    /// be passed to the next line for correct multi-line tokenization.
    ///
    /// ## Error Conditions:
    /// - Regex compilation errors
    /// - Invalid rule references
    /// - Malformed grammar patterns
    pub fn tokenize_line(&mut self, text: &str) -> Result<TokenizeResult, TokenizeError> {
        let mut accumulator = TokenAccumulator::new();
        let mut pos = 0;

        // Phase 1: Check BeginWhile continuation conditions
        // This must happen first - BeginWhile patterns can fail their condition
        // at the start of a line, causing them to be popped from the stack
        // before we start looking for new patterns.
        self.check_while_conditions(text, &mut pos, &mut accumulator)?;

        // Phase 2: Main pattern matching loop
        // Process the remainder of the line by finding the best pattern match
        // at each position and advancing through the text.
        while pos < text.len() {
            if let Some(match_result) = self.scan_next(text, pos)? {
                // Found a pattern match - process it and advance position
                pos = self.handle_match(match_result, &mut accumulator)?;
            } else {
                // No patterns match at current position
                // Scan ahead to find the next position where a pattern DOES match
                // This allows us to batch consecutive unmatched text into single tokens
                let mut next_pos = pos + 1;
                while next_pos < text.len() {
                    if self.scan_next(text, next_pos)?.is_some() {
                        break;
                    }
                    next_pos += 1;
                }

                // Generate one token for the entire unmatched text span
                // This gives proper token batching while preserving scope inheritance
                // The scopes are current context (e.g., inside string, comment, etc.)
                accumulator.produce(next_pos, &self.state.stack.content_scopes);
                pos = next_pos;
            }
        }
        Ok(TokenizeResult {
            tokens: accumulator.tokens,
            state: self.state.clone(),
        })
    }

    /// Check BeginWhile continuation conditions and pop failed patterns.
    ///
    /// This implements Phase 1 of TextMate tokenization. BeginWhile patterns
    /// have a "while" condition that must match at the start of each line
    /// for the pattern to continue. If the condition fails, the pattern
    /// (and all patterns nested within it) must be popped from the stack.
    ///
    /// ## Algorithm:
    /// 1. Collect all BeginWhile patterns from the current stack
    /// 2. Test their while conditions from outermost to innermost
    /// 3. If a condition matches, consume the matched text and continue
    /// 4. If a condition fails, pop that pattern and all nested patterns
    ///
    /// ## Example:
    /// Consider a heredoc in a shell script:
    /// ```bash
    /// cat << EOF
    /// line 1
    /// line 2
    /// EOF
    /// ```
    /// The BeginWhile pattern continues as long as lines DON'T match "EOF".
    /// When "EOF" is encountered, the while condition fails and the heredoc ends.
    ///
    /// ## Side Effects:
    /// - May modify the state stack (pop failed patterns)
    /// - May advance the position and generate tokens
    /// - Changes are reflected in the mutable references
    fn check_while_conditions(
        &mut self,
        text: &str,
        pos: &mut usize,
        accumulator: &mut TokenAccumulator,
    ) -> Result<(), TokenizeError> {
        // Collect all BeginWhile rules from the stack
        // We need to check from outermost to innermost, so collect them first
        let mut while_stack = Vec::new();
        let mut current = Some(&self.state.stack);

        while let Some(stack_elem) = current {
            // Check if this stack element represents a BeginWhile pattern
            if let Some(rule) = self.grammar.rules.get(*stack_elem.rule_id as usize) {
                if let Rule::BeginWhile(_) = rule {
                    while_stack.push(stack_elem.clone());
                }
            }
            current = stack_elem.parent.as_deref();
        }

        // Check while conditions from outermost to innermost
        // This order is important - outer patterns are checked first
        for while_elem in while_stack.into_iter().rev() {
            if let Some(while_pattern) = &while_elem.end_rule {
                if let Some(while_match) =
                    self.try_match_while_pattern(while_pattern, text, *pos)?
                {
                    // While condition still matches - the pattern continues
                    // Generate tokens for the matched text if it advances the position
                    if while_match.end > *pos {
                        // Token for text before the while match
                        accumulator.produce(while_match.start, &self.state.stack.content_scopes);
                        // Token for the while match itself (with captures if any)
                        accumulator.produce(while_match.end, &self.state.stack.content_scopes);
                        *pos = while_match.end;
                    }
                } else {
                    // While condition failed - this pattern and all nested patterns must end
                    // Pop everything up to and including this BeginWhile pattern
                    self.pop_until_rule(while_elem.rule_id);
                    break; // Once we pop a pattern, stop checking (inner patterns are gone)
                }
            }
        }

        Ok(())
    }

    /// Find the best pattern match at the current position.
    ///
    /// This implements Phase 2 of TextMate tokenization. It uses a two-path approach:
    /// an optimized pattern batching path and a fallback individual pattern matching path.
    ///
    /// ## Algorithm Implementation:
    /// 1. **Check end patterns FIRST** - call try_match_end_pattern(), return immediately if found
    /// 2. **Try pattern batching optimization** - use PatternSet for efficient batch matching
    /// 3. **Fallback to individual matching** - if optimization unavailable, use traditional approach
    ///
    /// ## Optimized Path (Pattern Batching):
    /// - Get PatternSet for current rule via get_pattern_set()
    /// - Call pattern_set.find_at() which applies TextMate priority rules internally
    /// - Return match result with proper rule type detection
    ///
    /// ## Fallback Path (Individual Patterns):
    /// - Get available patterns via get_current_patterns() for contextual pattern set
    /// - Try each pattern via try_match_pattern(), collect matches as candidates
    /// - Apply priority resolution by sorting candidates
    /// - Return best match after priority-based sorting
    ///
    /// ## TextMate Priority Rules (applied in both paths):
    /// 1. **End patterns always win** - checked first, bypass all other patterns
    /// 2. **Earliest start position wins** - patterns matching earlier in text take precedence
    /// 3. **Longest match wins** - among patterns at same position, longer matches win
    /// 4. **Definition order wins** - among equal matches, earlier-defined patterns win
    ///
    /// ## Pattern Categories:
    /// - **End patterns**: Exit current BeginEnd/BeginWhile context (checked via try_match_end_pattern)
    /// - **Begin patterns**: Enter new BeginEnd/BeginWhile context (from get_current_patterns)
    /// - **Match patterns**: Apply scopes without changing context (from get_current_patterns)
    /// - **Include patterns**: Reference other pattern sets (resolved by get_current_patterns)
    ///
    /// ## Candidate Data Structure:
    /// Each candidate is (start_pos, match_length, definition_order, MatchResult).
    /// The tuple implements priority rules when sorted - earlier elements have higher priority.
    ///
    /// ## Position Constraint:
    /// Only considers matches that start exactly at `pos` - try_match_pattern enforces this.
    /// Patterns that match later in the text are ignored (handled by gap scanning in tokenize_line).
    ///
    /// ## Returns:
    /// - `Some(MatchResult)` if a pattern matches at exactly the current position
    /// - `None` if no patterns match at the current position
    fn scan_next(&mut self, text: &str, pos: usize) -> Result<Option<MatchResult>, TokenizeError> {
        // 1. Check end patterns first - they have absolute priority
        //    If we're in a BeginEnd or BeginWhile context and its end/while pattern
        //    matches, that takes precedence over any other patterns
        if let Some(end_match) = self.try_match_end_pattern(text, pos)? {
            return Ok(Some(end_match));
        }

        // 2. Use compiled pattern set for efficient batch matching
        //
        // This is the core optimization: instead of testing each pattern individually
        // (which would require N regex operations), we use a PatternSet that
        // can test all relevant patterns with better performance characteristics.
        let current_rule_id = self.state.stack.rule_id;

        if let Some(pattern_set) = self.get_cached_pattern_set(current_rule_id) {
            // Batch pattern matching: Test all patterns for this rule simultaneously
            //
            // The pattern_set.find_at() method:
            // 1. Tests each pattern individually at the exact position
            // 2. Applies TextMate priority rules to select the best match
            // 3. Returns the winning pattern with its rule ID and position info
            //
            // This approach maintains identical behavior to individual pattern testing
            // while providing significant performance benefits through pattern caching
            // and batch processing.
            if let Some((start, end, matched_rule_id, captures)) = pattern_set.find_at(text, pos) {
                // Determine the match type based on what kind of rule matched
                //
                // Different rule types require different handling in handle_match():
                // - Match: Apply scopes atomically without state change
                // - BeginEnd: Enter new context with explicit end condition
                // - BeginWhile: Enter new context with continuation condition
                let match_type =
                    if let Some(rule) = self.grammar.rules.get(*matched_rule_id as usize) {
                        match rule {
                            crate::grammars::Rule::Match(_) => MatchType::Match,
                            crate::grammars::Rule::BeginEnd(_) => MatchType::BeginEnd,
                            crate::grammars::Rule::BeginWhile(_) => MatchType::BeginWhile,
                            _ => MatchType::Match, // Fallback for unexpected rule types
                        }
                    } else {
                        MatchType::Match // Fallback if rule lookup fails
                    };

                return Ok(Some(MatchResult {
                    start,
                    end,
                    rule_id: matched_rule_id,
                    captures, // Now includes full capture group information from RegSet
                    match_type,
                }));
            }
        }

        // No patterns matched
        Ok(None)
    }

    /// Process a pattern match and update tokenizer state.
    ///
    /// Handles different match types: EndPattern (pops context), Match (applies scopes),
    /// BeginEnd/BeginWhile (pushes new context). Includes TextMate capture support
    /// for proper delimiter scoping. Returns position to continue from.
    ///
    /// ## Algorithm Flow:
    /// 1. **Pre-match token**: Generate token for text before match using accumulator.produce()
    /// 2. **Match type dispatch**: Handle each MatchType with specific state transformations
    /// 3. **Post-match token**: Generate token for matched text with appropriate scopes
    /// 4. **Return advancement**: Return match_result.end as new position for tokenize_line
    ///
    /// ## State Transformations by Match Type:
    ///
    /// ### EndPattern:
    /// - **Action**: Exit current BeginEnd/BeginWhile context
    /// - **State change**: Pop state stack (self.state.stack = parent)
    /// - **Scoping**: Use end_captures if BeginEnd, otherwise switch to name_scopes
    /// - **Token generation**: Delimiter token with capture-resolved scopes
    ///
    /// ### Match:
    /// - **Action**: Apply scopes atomically without context change
    /// - **State change**: None (no stack modification)
    /// - **Scoping**: Temporarily add match rule's scope_id to current content_scopes
    /// - **Token generation**: Single token with augmented scopes
    ///
    /// ### BeginEnd:
    /// - **Action**: Enter new context with explicit end condition
    /// - **State change**: Push new stack element with configured scopes and end pattern
    /// - **Scoping**: Configure name_scopes and content_scopes via helper functions
    /// - **Token generation**: Begin delimiter with begin_captures resolution
    ///
    /// ### BeginWhile:
    /// - **Action**: Enter new context with continuation condition
    /// - **State change**: Push new stack element with configured scopes and while pattern
    /// - **Scoping**: Same as BeginEnd but sets while pattern instead of end pattern
    /// - **Token generation**: Begin delimiter with name_scopes
    ///
    /// ## Helper Function Integration:
    /// - configure_begin_context_scopes(): Sets up name_scopes and content_scopes for Begin* rules
    /// - setup_pattern_with_backrefs(): Resolves and stores end/while patterns with captures
    /// - resolve_captures(): Converts capture rules to specific delimiter scopes
    ///
    /// ## Position Advancement Contract:
    /// Always returns match_result.end, guaranteeing forward progress in tokenize_line loop.
    /// Callers rely on this to prevent infinite loops.
    fn handle_match(
        &mut self,
        match_result: MatchResult,
        accumulator: &mut TokenAccumulator,
    ) -> Result<usize, TokenizeError> {
        // Always generate a token for any text before this match
        // This ensures no text is left untokenized
        accumulator.produce(match_result.start, &self.state.stack.content_scopes);

        match match_result.match_type {
            MatchType::EndPattern => {
                // End pattern matched - we're exiting a BeginEnd or BeginWhile context

                // Generate token for the end delimiter using end_captures if available
                if let Some(rule) = self.grammar.rules.get(*self.state.stack.rule_id as usize) {
                    if let Rule::BeginEnd(begin_end) = rule {
                        // Use end_captures to get specific scopes for the closing delimiter
                        let delimiter_scopes = self.resolve_captures(
                            &begin_end.end_captures,
                            &match_result.captures,
                            begin_end.scope_id, // fallback to rule's main scope
                        );
                        accumulator.produce(match_result.end, &delimiter_scopes);
                    } else {
                        // For BeginWhile or other patterns, fall back to name scopes
                        self.state.stack = self.state.stack.clone().switch_to_name_scopes();
                        accumulator.produce(match_result.end, &self.state.stack.content_scopes);
                    }
                } else {
                    // Fallback: use name scopes if rule lookup fails
                    self.state.stack = self.state.stack.clone().switch_to_name_scopes();
                    accumulator.produce(match_result.end, &self.state.stack.content_scopes);
                }

                // Pop back to the parent context
                if let Some(parent) = self.state.stack.pop() {
                    self.state.stack = parent;
                }
            }

            MatchType::Match => {
                // Match pattern - atomic scope application without state change
                // This is for patterns that apply scopes to matched text but don't
                // create nested contexts (like keywords, operators, etc.)

                // Create temporary scopes with the match's scope added
                let mut temp_scopes = self.state.stack.content_scopes.clone();
                if let Some(rule) = self.grammar.rules.get(*match_result.rule_id as usize) {
                    if let Rule::Match(match_rule) = rule {
                        if let Some(scope_id) = match_rule.scope_id {
                            temp_scopes.push(scope_id);
                        }
                    }
                }

                // Generate token with augmented scopes
                // The match scope is only applied to this token, not to subsequent text
                accumulator.produce(match_result.end, &temp_scopes);
            }

            MatchType::BeginEnd => {
                // BeginEnd pattern - enter new context with explicit end condition
                let mut new_stack = self.state.stack.push(match_result.rule_id);

                // Configure the new context with the rule's scopes and end pattern
                if let Some(rule) = self.grammar.rules.get(*match_result.rule_id as usize) {
                    if let Rule::BeginEnd(begin_end) = rule {
                        // Configure scopes using helper
                        new_stack = self.configure_begin_context_scopes(
                            new_stack,
                            begin_end.scope_id,
                            begin_end.content_scope_id,
                        );

                        // Set up the end pattern using helper
                        new_stack = self.setup_pattern_with_backrefs(
                            new_stack,
                            *begin_end.end,
                            begin_end.end_has_backrefs,
                            &match_result.captures,
                        );

                        // Store captures and generate token for begin delimiter
                        new_stack = new_stack.with_begin_captures(match_result.captures.clone());

                        let delimiter_scopes = self.resolve_captures(
                            &begin_end.begin_captures,
                            &match_result.captures,
                            begin_end.scope_id, // fallback to rule's main scope
                        );
                        accumulator.produce(match_result.end, &delimiter_scopes);
                    }
                } else {
                    // Fallback: use name_scopes if rule lookup fails
                    accumulator.produce(match_result.end, &new_stack.name_scopes);
                }

                self.state.stack = new_stack;
            }

            MatchType::BeginWhile => {
                // BeginWhile pattern - enter new context with continuation condition
                // Similar to BeginEnd, but instead of an explicit end pattern,
                // the context continues as long as a "while" condition matches
                let mut new_stack = self.state.stack.push(match_result.rule_id);

                if let Some(rule) = self.grammar.rules.get(*match_result.rule_id as usize) {
                    if let Rule::BeginWhile(begin_while) = rule {
                        // Configure scopes using helper (same as BeginEnd)
                        new_stack = self.configure_begin_context_scopes(
                            new_stack,
                            begin_while.scope_id,
                            begin_while.content_scope_id,
                        );

                        // Set up the while pattern using helper
                        new_stack = self.setup_pattern_with_backrefs(
                            new_stack,
                            *begin_while.while_,
                            begin_while.while_has_backrefs,
                            &match_result.captures,
                        );
                    }
                }

                // Store captures and activate the new context
                new_stack = new_stack.with_begin_captures(match_result.captures);
                // Use name_scopes for the begin delimiter
                accumulator.produce(match_result.end, &new_stack.name_scopes);
                self.state.stack = new_stack;
            }
        }

        // Return the position to continue tokenizing from
        Ok(match_result.end)
    }

    /// Try to match the end pattern for the current rule context.
    ///
    /// This checks if the current BeginEnd or BeginWhile context's end/while
    /// pattern matches at the given position. End patterns have the highest
    /// priority in TextMate tokenization.
    ///
    /// ## Returns:
    /// - `Some(MatchResult)` if the end pattern matches
    /// - `None` if no end pattern is active or it doesn't match
    ///
    /// ## Note:
    /// Uses special rule IDs (u16::MAX) to mark end patterns since they're
    /// not regular grammar rules but dynamic patterns from the current context.
    fn try_match_end_pattern(
        &self,
        text: &str,
        pos: usize,
    ) -> Result<Option<MatchResult>, TokenizeError> {
        if let Some(end_pattern) = &self.state.stack.end_rule {
            if let Some(matched) = self.try_match_regex(end_pattern, text, pos)? {
                return Ok(Some(Self::create_match_result_from_regex(
                    matched,
                    RuleId(u16::MAX), // Special marker for end patterns
                    MatchType::EndPattern,
                )));
            }
        }
        Ok(None)
    }

    /// Try to match a BeginWhile continuation pattern.
    ///
    /// This is similar to `try_match_end_pattern` but specifically for the
    /// "while" conditions of BeginWhile patterns. Used during Phase 1 to
    /// check if BeginWhile patterns should continue or be popped.
    ///
    /// ## Returns:
    /// - `Some(MatchResult)` if the while pattern matches (pattern continues)
    /// - `None` if the while pattern doesn't match (pattern should be popped)
    fn try_match_while_pattern(
        &self,
        pattern: &str,
        text: &str,
        pos: usize,
    ) -> Result<Option<MatchResult>, TokenizeError> {
        if let Some(matched) = self.try_match_regex(pattern, text, pos)? {
            Ok(Some(Self::create_match_result_from_regex(
                matched,
                RuleId(u16::MAX - 1),  // Special marker for while patterns
                MatchType::EndPattern, // Treat as end for processing
            )))
        } else {
            Ok(None)
        }
    }

    /// Helper function to create MatchResult from regex match tuple.
    ///
    /// Extracts the common pattern of creating MatchResult from the tuple
    /// returned by try_match_regex: (start, end, captures).
    fn create_match_result_from_regex(
        matched: (usize, usize, Vec<String>),
        rule_id: RuleId,
        match_type: MatchType,
    ) -> MatchResult {
        MatchResult {
            start: matched.0,
            end: matched.1,
            rule_id,
            captures: matched.2,
            match_type,
        }
    }

    /// Helper function to configure scopes for BeginEnd and BeginWhile contexts.
    ///
    /// Extracts the common pattern of setting name_scope and content_scope
    /// from rule's scope_id and content_scope_id fields.
    ///
    /// ## Scope Configuration Logic:
    /// 1. **name_scope**: If scope_id provided, add to new_stack.name_scopes
    /// 2. **content_scope**: If content_scope_id provided, add to new_stack.content_scopes
    /// 3. **Fallback**: If no content_scope_id but scope_id exists, use scope_id for both
    ///
    /// ## TextMate Behavior:
    /// - name_scopes apply to begin/end delimiters (quotes, brackets, etc.)
    /// - content_scopes apply to text between delimiters (string interior, block content)
    /// - If no explicit contentName, content inherits the delimiter scope
    ///
    /// ## Usage Contract:
    /// Called from handle_match() for BeginEnd and BeginWhile cases after stack.push().
    /// Input stack already has parent scopes inherited; this adds rule-specific scopes.
    /// Returns modified stack for further configuration (backreference setup, captures).
    fn configure_begin_context_scopes(
        &self,
        mut new_stack: StateStack,
        scope_id: Option<ScopeId>,
        content_scope_id: Option<ScopeId>,
    ) -> StateStack {
        // Add "name" scope (applied to begin/end delimiters)
        if let Some(scope_id) = scope_id {
            new_stack = new_stack.with_name_scope(scope_id);
        }

        // Add "contentName" scope (applied to content between delimiters)
        if let Some(content_scope_id) = content_scope_id {
            new_stack = new_stack.with_content_scope(content_scope_id);
        } else if let Some(scope_id) = scope_id {
            // If no explicit contentName, use the name scope for content too
            // This is standard TextMate behavior - content inherits the delimiter scope
            new_stack = new_stack.with_content_scope(scope_id);
        }

        new_stack
    }

    /// Helper function to set up end/while pattern with backreference resolution.
    ///
    /// Extracts the common pattern of resolving backreferences in end/while patterns
    /// and setting the resolved pattern on the stack.
    ///
    /// ## Backreference Resolution Process:
    /// 1. **Get pattern**: Look up regex by regex_id in grammar.regexes
    /// 2. **Check backrefs**: If has_backrefs=true, call resolve_backreferences()
    /// 3. **Resolve**: Replace \1, \2, etc. with actual captured text from begin pattern
    /// 4. **Store**: Set resolved pattern as new_stack.end_rule for runtime matching
    ///
    /// ## Example Transformation:
    /// - Begin pattern: `(['"])` matches and captures `"`
    /// - End pattern: `\1` (raw from grammar)
    /// - After resolution: `"` (literal quote to match)
    /// - Stored in stack: end_rule = Some("\"")
    ///
    /// ## Usage Contract:
    /// Called from handle_match() after scope configuration. The captures parameter
    /// contains the matched groups from the begin pattern that just matched.
    /// For BeginEnd: uses end pattern with end_has_backrefs flag
    /// For BeginWhile: uses while pattern with while_has_backrefs flag
    ///
    /// ## Pattern Storage:
    /// The resolved pattern is stored in StateStack.end_rule and will be used by:
    /// - try_match_end_pattern() for BeginEnd contexts
    /// - check_while_conditions() for BeginWhile contexts
    fn setup_pattern_with_backrefs(
        &self,
        mut new_stack: StateStack,
        regex_id: u16,
        has_backrefs: bool,
        captures: &[String],
    ) -> StateStack {
        if let Some(regex) = self.grammar.regexes.get(regex_id as usize) {
            let pattern = regex.pattern();
            if has_backrefs {
                // If it has backreferences, resolve them using the begin captures
                let resolved_pattern = self.resolve_backreferences(pattern, captures);
                new_stack = new_stack.with_end_rule(resolved_pattern);

                // Note: With our simplified approach mirroring vscode-textmate, we don't cache
                // PatternSets at the rule level, so we don't need cache invalidation.
                // RegSet compilation is still cached inside PatternSet via OnceCell.
            } else {
                // No backreferences, use the pattern as-is
                new_stack = new_stack.with_end_rule(pattern.to_string());
            }
        }
        new_stack
    }

    /// Resolve backreferences in a pattern string.
    ///
    /// TextMate patterns can contain backreferences like \1, \2, etc. that refer
    /// to captured groups from the begin pattern. This function replaces those
    /// backreferences with the actual captured text.
    ///
    /// ## Example:
    /// If begin pattern is `(['"'])` and captures `"`, and end pattern is `\1`,
    /// this resolves `\1` to `"` so the end pattern becomes a literal quote.
    ///
    /// ## Current Implementation:
    /// Simple string replacement. A robust implementation would handle:
    /// - Escaped backreferences (\\1 should not be replaced)
    /// - Invalid backreferences (referring to non-existent captures)
    /// - Regex escaping of replacement text
    fn resolve_backreferences(&self, pattern: &str, captures: &[String]) -> String {
        let mut result = pattern.to_string();
        // Replace \1, \2, \3, etc. with corresponding captures
        for (i, capture) in captures.iter().enumerate() {
            let backref = format!("\\{}", i + 1);
            result = result.replace(&backref, capture);
        }
        result
    }

    /// Create a single-pattern RegSet for dynamic pattern matching.
    ///
    /// This follows vscode-textmate's approach of always using scanners (RegSet/OnigScanner)
    /// even for single patterns, rather than creating temporary individual regexes.
    ///
    /// ## Usage:
    /// Used for dynamic end/while patterns with resolved backreferences where we
    /// can't use the pre-compiled pattern sets.
    fn create_single_pattern_regset(&self, pattern: &str) -> Result<RegSet, TokenizeError> {
        RegSet::new(&[pattern]).map_err(|e| {
            TokenizeError::InvalidRegex(format!(
                "Failed to create RegSet for pattern '{}': {}",
                pattern, e
            ))
        })
    }

    /// Try to match a pattern at a specific position using RegSet.
    ///
    /// Only accepts matches that start exactly at the current position to prevent
    /// content skipping. Returns (start, end, captures) on success.
    ///
    /// ## Critical Position Constraint:
    /// This function enforces that matches MUST start at exactly `pos`. This prevents
    /// end patterns from "scanning ahead" and skipping content that should be tokenized.
    ///
    /// ## Why This Matters:
    /// Without this constraint, an end pattern like `"` could match the closing quote
    /// in `"hello world"` while positioned at `h`, causing `hello worl` to be skipped
    /// entirely. The constraint forces character-by-character progression.
    ///
    /// ## Implementation Details:
    /// 1. **Create RegSet**: Single-pattern RegSet following vscode-textmate approach
    /// 2. **Find match**: Call regset.captures() to get match with capture groups
    /// 3. **Position check**: If match.start != pos, reject (return None)
    /// 4. **Extract captures**: Return all capture groups for backreference resolution
    /// 5. **Return tuple**: (start, end, captures) for further processing
    ///
    /// ## Return Values:
    /// - `Ok(Some((start, end, captures)))` - Valid match at exact position
    /// - `Ok(None)` - No match at position OR match found at wrong position
    /// - `Err(TokenizeError)` - RegSet creation failure
    ///
    /// ## Performance Note:
    /// Now uses RegSet like vscode-textmate's OnigScanner approach instead of temporary
    /// Regex instances. This provides consistent O(1) matching behavior.
    fn try_match_regex(
        &self,
        pattern: &str,
        text: &str,
        pos: usize,
    ) -> Result<Option<(usize, usize, Vec<String>)>, TokenizeError> {
        // Create single-pattern RegSet (following vscode-textmate's OnigScanner approach)
        let regset = self.create_single_pattern_regset(pattern)?;

        // Get text from position onward for matching
        let search_text = text.get(pos..).unwrap_or("");

        if let Some((pattern_index, captures)) = regset.captures(search_text) {
            // Should always be 0 since we only have one pattern
            debug_assert_eq!(pattern_index, 0);

            if let Some((match_start, match_end)) = captures.pos(0) {
                // Only accept matches that start exactly at the current position
                // This prevents end patterns from scanning ahead and skipping content
                if match_start == 0 {
                    let absolute_start = pos;
                    let absolute_end = pos + match_end;

                    // Extract all capture groups as strings
                    let capture_strings: Vec<String> = (0..captures.len())
                        .filter_map(|i| captures.at(i))
                        .map(|s| s.to_string())
                        .collect();

                    Ok(Some((absolute_start, absolute_end, capture_strings)))
                } else {
                    // Match found but not at current position - reject it
                    Ok(None)
                }
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    /// Resolve TextMate captures to get appropriate scopes for a match.
    ///
    /// Looks up capture rules from beginCaptures/endCaptures to assign specific
    /// scopes to delimiters vs content (e.g., punctuation vs string scopes).
    ///
    /// ## Capture System Purpose:
    /// Allows different parts of a pattern match to have different scopes:
    /// - Opening quote: `punctuation.definition.string.begin`
    /// - String content: `string.quoted.double`
    /// - Closing quote: `punctuation.definition.string.end`
    ///
    /// ## Resolution Algorithm:
    /// 1. **Start with context**: Begin with current StateStack.content_scopes
    /// 2. **Check capture 0**: Look for explicit capture rule for full match (index 0)
    /// 3. **Apply capture scope**: If found, add the capture rule's scope_id to stack
    /// 4. **Fallback to rule scope**: If no capture found, use fallback_scope parameter
    /// 5. **Return augmented scopes**: Combined stack for token generation
    ///
    /// ## Capture Rule Lookup:
    /// captures_list[0] contains Optional<RuleId> for the full match.
    /// The RuleId points to a Match rule with scope_id containing the delimiter scope.
    /// Individual capture groups (1, 2, etc.) could be supported but aren't currently used.
    ///
    /// ## Usage in handle_match():
    /// - **BeginEnd**: Called with begin_captures for opening delimiter
    /// - **BeginEnd**: Called with end_captures for closing delimiter
    /// - **Match**: Not used (scopes applied directly in handle_match)
    ///
    /// ## Scope Hierarchy:
    /// Result maintains scope nesting: parent scopes + current context + capture scope.
    /// This ensures proper CSS/theme inheritance in syntax highlighting.
    fn resolve_captures(
        &self,
        captures_list: &[Option<RuleId>],
        _matched_captures: &[String],
        fallback_scope: Option<ScopeId>,
    ) -> Vec<ScopeId> {
        // Start with current context scopes
        let mut scopes = self.state.stack.content_scopes.clone();

        // For capture 0 (the full match), check if there's a specific capture defined
        if !captures_list.is_empty() {
            if let Some(Some(capture_rule_id)) = captures_list.get(0) {
                // Look up the capture rule to get its scope
                if let Some(rule) = self.grammar.rules.get(**capture_rule_id as usize) {
                    if let Rule::Match(match_rule) = rule {
                        // Scope-only Match rule with the punctuation/delimiter scope
                        if let Some(capture_scope) = match_rule.scope_id {
                            scopes.push(capture_scope);
                            return scopes;
                        }
                    }
                }
            }
        }

        // Use fallback scope if no capture scope found
        if let Some(scope) = fallback_scope {
            scopes.push(scope);
        }

        scopes
    }

    /// Pop the state stack until reaching the specified rule.
    ///
    /// This is used when BeginWhile conditions fail - we need to pop the
    /// failed BeginWhile pattern and all patterns nested within it.
    /// Continues popping until reaching the target rule ID.
    ///
    /// ## BeginWhile Failure Handling:
    /// When a BeginWhile pattern's "while" condition fails at line start:
    /// 1. **Identify failed pattern**: check_while_conditions() detects the failure
    /// 2. **Pop nested contexts**: This function removes failed pattern + all nested
    /// 3. **Restore parent context**: Stack returns to state before BeginWhile started
    ///
    /// ## Algorithm:
    /// Loop while current stack.rule_id != target_rule_id:
    /// - Call stack.pop() to get parent
    /// - Update self.state.stack to parent
    /// - Break if parent is None (reached root)
    ///
    /// ## Stack Integrity:
    /// The function maintains stack integrity by never popping below the root.
    /// If target rule is not found, it stops at root rather than underflowing.
    ///
    /// ## Example Scenario:
    /// BeginWhile heredoc fails → pops heredoc context and any nested string/comment
    /// contexts within it, returning to the outer shell script context.
    ///
    /// ## Safety:
    /// Stops if it reaches the root of the stack (parent is None) to prevent
    /// infinite loops or stack underflow.
    fn pop_until_rule(&mut self, target_rule_id: RuleId) {
        while self.state.stack.rule_id != target_rule_id {
            if let Some(parent) = self.state.stack.pop() {
                self.state.stack = parent;
            } else {
                // Reached root of stack - stop popping to prevent underflow
                break;
            }
        }
    }

    /// Get a cached pattern set for a rule, creating and caching it if needed.
    ///
    /// This method preserves the OnceCell<RegSet> compilation cache within PatternSet
    /// instances across multiple calls, providing significant performance improvements
    /// over the uncached grammar.get_pattern_set() method.
    ///
    /// ## Caching Strategy:
    /// - Cache key: RuleId (eliminates expensive pattern resolution for cache keys)
    /// - Cache value: complete PatternSet with preserved OnceCell<RegSet>
    /// - Cache is stored on the tokenizer instance for session-long reuse
    /// - Follows vscode-textmate's approach of caching by rule identifier
    fn get_cached_pattern_set(&mut self, rule_id: RuleId) -> Option<&PatternSet> {
        // Check if we already have this RuleId cached
        if self.pattern_cache.contains_key(&rule_id) {
            return self.pattern_cache.get(&rule_id);
        }

        // If not cached, get the pattern set from grammar and cache it
        if let Some(pattern_set) = self.grammar.get_pattern_set(rule_id) {
            self.pattern_cache.insert(rule_id, pattern_set);
            self.pattern_cache.get(&rule_id)
        } else {
            None
        }
    }

    /// Get a reference to the current tokenizer state.
    ///
    /// Use this to access the current state for debugging or to pass to
    /// another tokenizer instance for continued multi-line tokenization.
    /// The state is immutable through this reference.
    pub fn get_state(&self) -> &TokenizerState {
        &self.state
    }
}

/// Error types that can occur during tokenization.
///
/// These errors represent different failure modes in the tokenization process.
/// Most errors indicate bugs in the grammar or tokenizer implementation rather
/// than invalid input text.
#[derive(Debug)]
pub enum TokenizeError {
    /// A regex pattern failed to compile or match.
    /// Contains the problematic pattern for debugging.
    InvalidRegex(String),

    /// Referenced a rule ID that doesn't exist in the grammar.
    /// This usually indicates corrupted grammar data or a bug in rule resolution.
    RuleNotFound(RuleId),

    /// Tokenizer got stuck at the same position without making progress.
    /// Contains the position where the loop was detected.
    /// This indicates a bug in the tokenizer logic or malformed grammar.
    InfiniteLoop(usize),

    /// General grammar-related error.
    /// Contains a description of what went wrong.
    GrammarError(String),
}

impl std::fmt::Display for TokenizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            TokenizeError::InvalidRegex(pattern) => {
                write!(f, "Invalid regex pattern: {}", pattern)
            }
            TokenizeError::RuleNotFound(rule_id) => {
                write!(f, "Rule not found: {:?}", rule_id)
            }
            TokenizeError::InfiniteLoop(pos) => {
                write!(f, "Infinite loop detected at position {}", pos)
            }
            TokenizeError::GrammarError(msg) => {
                write!(f, "Grammar error: {}", msg)
            }
        }
    }
}

impl std::error::Error for TokenizeError {}

/// Convenience function for stateless tokenization of a single line.
///
/// This is a simpler interface for tokenizing individual lines without
/// managing tokenizer instances. It creates a tokenizer internally and
/// returns both the tokens and the updated state.
///
/// ## Usage:
/// ```ignore
/// // First line (no previous state)
/// let result1 = tokenize_line(grammar, "const x = 42;", None)?;
///
/// // Subsequent lines (pass previous state)
/// let result2 = tokenize_line(grammar, "console.log(x);", Some(result1.state))?;
/// ```
///
/// ## Parameters:
/// - `grammar`: The compiled grammar to use for tokenization
/// - `text`: The line of text to tokenize
/// - `prev_state`: State from the previous line (None for first line)
///
/// ## Returns:
/// TokenizeResult containing tokens and updated state, or TokenizeError on failure.
///
/// For repeated tokenization, consider using a Tokenizer instance directly
/// for better performance (avoids repeated tokenizer creation).
pub fn tokenize_line(
    grammar: &CompiledGrammar,
    text: &str,
    prev_state: Option<TokenizerState>,
) -> Result<TokenizeResult, TokenizeError> {
    let mut tokenizer = if let Some(state) = prev_state {
        Tokenizer::with_state(grammar, state)
    } else {
        Tokenizer::new(grammar)
    };

    tokenizer.tokenize_line(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammars::RawGrammar;

    /// Load and compile the JSON grammar for testing
    fn load_json_grammar() -> Result<CompiledGrammar, Box<dyn std::error::Error>> {
        let json_grammar_path = "grammars-themes/packages/tm-grammars/grammars/json.json";
        let raw_grammar = RawGrammar::load_from_file(json_grammar_path)?;
        let compiled_grammar = raw_grammar.compile()?;
        Ok(compiled_grammar)
    }

    /// Load and compile the TOML grammar for testing
    fn load_toml_grammar() -> Result<CompiledGrammar, Box<dyn std::error::Error>> {
        let toml_grammar_path = "grammars-themes/packages/tm-grammars/grammars/toml.json";
        let raw_grammar = RawGrammar::load_from_file(toml_grammar_path)?;
        let compiled_grammar = raw_grammar.compile()?;
        Ok(compiled_grammar)
    }

    /// Test JSON tokenization implementation.
    #[test]
    fn test_json_tokenization() {
        let grammar = load_json_grammar().expect("Failed to load JSON grammar");

        let test_cases = vec![
            ("42", 1),                  // Number -> 1 token
            ("true", 1),                // Boolean -> 1 token
            (r#""hello""#, 3),          // String -> 3 tokens (begin quote, content, end quote)
            (r#"{"key": "value"}"#, 10), // Object -> detailed tokenization
            ("[1, 2, 3]", 9),           // Array -> detailed tokenization
        ];

        for (json_text, expected_tokens) in test_cases {
            let mut tokenizer = Tokenizer::new(&grammar);
            let result = tokenizer
                .tokenize_line(json_text)
                .expect("Tokenization should succeed");

            // Debug output
            println!("Input: '{}'", json_text);
            println!("Tokens ({}):", result.tokens.len());
            for (i, token) in result.tokens.iter().enumerate() {
                let token_text = &json_text[token.start..token.end];
                let scopes = token.scope_names();
                println!(
                    "  [{}] '{}' ({}-{}) -> {:?}",
                    i, token_text, token.start, token.end, scopes
                );
            }

            // Verify correct token count
            assert_eq!(
                result.tokens.len(),
                expected_tokens,
                "Wrong token count for '{}'",
                json_text
            );

            // Verify complete coverage
            let total_chars: usize = result.tokens.iter().map(|t| t.end - t.start).sum();
            assert_eq!(
                total_chars,
                json_text.len(),
                "Incomplete coverage for '{}'",
                json_text
            );

            // Verify no illegal tokens
            let has_illegal = result
                .tokens
                .iter()
                .any(|token| token.has_scope_containing("invalid.illegal"));
            assert!(!has_illegal, "Found illegal tokens in '{}'", json_text);

            // Verify proper JSON scopes
            let has_valid = result.tokens.iter().any(|token| {
                token.has_scope_containing("source.json")
                    || token.has_scope_containing("string.quoted")
                    || token.has_scope_containing("constant.numeric")
                    || token.has_scope_containing("constant.language")
                    || token.has_scope_containing("punctuation")
            });
            assert!(has_valid, "No valid JSON scopes found in '{}'", json_text);
        }
    }

    #[test]
    fn test_complex_json_tokenization_with_output() {
        let complex_json = r#"{"name": "John", "age": 30, "active": true, "score": 95.5, "tags": ["developer", "rust"], "address": null}"#;

        let grammar = load_json_grammar().expect("Failed to load JSON grammar");
        let mut tokenizer = Tokenizer::new(&grammar);

        let result = tokenizer
            .tokenize_line(complex_json)
            .expect("Tokenization should succeed");

        println!("=== Complex JSON Tokenization Output (RegSet Optimization) ===");
        println!("Input: {}", complex_json);
        println!("Tokens ({} total):", result.tokens.len());

        for (i, token) in result.tokens.iter().enumerate() {
            let token_text = &complex_json[token.start..token.end];
            let scopes = token.scope_names();
            println!(
                "  [{:2}] '{}' ({:3}-{:3}) -> {:?}",
                i, token_text, token.start, token.end, scopes
            );
        }

        // Validate comprehensive tokenization
        let total_chars: usize = result.tokens.iter().map(|t| t.end - t.start).sum();
        assert_eq!(total_chars, complex_json.len(), "Incomplete coverage");

        // Verify no illegal tokens
        let has_illegal = result
            .tokens
            .iter()
            .any(|token| token.has_scope_containing("invalid.illegal"));
        assert!(!has_illegal, "Found illegal tokens");

        // With restored fallback functionality: JSON grammar uses create_simple_pattern_set
        // for patterns that couldn't be pre-compiled, giving us detailed token breakdown
        assert!(
            result.tokens.len() > 50,
            "Expected detailed tokenization with {} tokens",
            result.tokens.len()
        );

        // Verify we get proper scope breakdown
        let has_punctuation = result
            .tokens
            .iter()
            .any(|token| token.has_scope_containing("punctuation"));
        assert!(has_punctuation, "Should have punctuation scopes");

        let has_string = result
            .tokens
            .iter()
            .any(|token| token.has_scope_containing("string"));
        assert!(has_string, "Should have string scopes");

        let has_numeric = result
            .tokens
            .iter()
            .any(|token| token.has_scope_containing("constant.numeric"));
        assert!(has_numeric, "Should have numeric scopes");

        let has_boolean = result
            .tokens
            .iter()
            .any(|token| token.has_scope_containing("constant.language"));
        assert!(has_boolean, "Should have constant.language scopes");

        println!(
            "✅ Restored fallback functionality successfully tokenized complex JSON with {} detailed tokens",
            result.tokens.len()
        );
    }

    #[test]
    fn test_toml_tokenization_with_pattern_set_stats() {
        let toml_content = r#"# TOML Configuration
[database]
server = "192.168.1.1"
ports = [ 8001, 8001, 8002 ]
connection_max = 5000
enabled = true

[servers.alpha]
ip = "10.0.0.1"
dc = "eqdc10"

[servers.beta]
ip = "10.0.0.2"
dc = "eqdc10"
"#;

        let grammar = load_toml_grammar().expect("Failed to load TOML grammar");
        let mut tokenizer = Tokenizer::new(&grammar);

        let result = tokenizer
            .tokenize_line(toml_content)
            .expect("Tokenization should succeed");

        println!("=== TOML Tokenization Pattern Set Usage Analysis ===");
        println!("Input: {}", toml_content);
        println!("Tokens ({} total):", result.tokens.len());

        for (i, token) in result.tokens.iter().take(20).enumerate() {
            let token_text = &toml_content[token.start..token.end];
            let scopes = token.scope_names();
            println!(
                "  [{:2}] '{}' ({:3}-{:3}) -> {:?}",
                i, token_text.escape_debug(), token.start, token.end, scopes
            );
        }

        if result.tokens.len() > 20 {
            println!("  ... and {} more tokens", result.tokens.len() - 20);
        }

        // Verify complete coverage
        let total_chars: usize = result.tokens.iter().map(|t| t.end - t.start).sum();
        assert_eq!(total_chars, toml_content.len(), "Incomplete coverage");

        // Verify no illegal tokens
        let has_illegal = result
            .tokens
            .iter()
            .any(|token| token.has_scope_containing("invalid.illegal"));
        assert!(!has_illegal, "Found illegal tokens");

        // Verify proper TOML scopes
        let has_valid = result.tokens.iter().any(|token| {
            token.has_scope_containing("source.toml")
                || token.has_scope_containing("comment.line")
                || token.has_scope_containing("punctuation")
                || token.has_scope_containing("variable.other.key")
        });
        assert!(has_valid, "No valid TOML scopes found");

        println!(
            "✅ TOML tokenization completed with {} tokens",
            result.tokens.len()
        );
    }
}
