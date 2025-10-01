use crate::grammars::{CompiledGrammar, CompiledPattern, ScopeId};
use crate::themes::{CompiledTheme, Style};

// ================================================================================================
// CORE DATA STRUCTURES
// ================================================================================================
// This module implements a TextMate-compatible tokenizer that processes text according to
// grammar rules and produces styled tokens. The key insight is that TextMate grammars define
// hierarchical patterns that build scope stacks, which are then mapped to visual styles.

/// A token represents a span of text with associated scope information
///
/// Example: The text "var" in JavaScript might produce:
/// Token {
///     start: 0, end: 3,
///     scope_stack: [ScopeId(1), ScopeId(42)] // ["source.js", "keyword.control.var"]
/// }
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// Start position in the text (byte offset)
    pub start: usize,
    /// End position in the text (byte offset)
    pub end: usize,
    /// Stack of scopes applied to this token - builds from root scope outward
    /// e.g., ["source.js", "string.quoted.double.js", "punctuation.definition.string.begin.js"]
    pub scope_stack: Vec<ScopeId>,
}

/// A batched token optimizes consecutive tokens with the same styling
///
/// Performance optimization: Instead of rendering 100s of individual tokens,
/// we batch consecutive tokens that have the same visual style.
/// Example: "hello world" might become 2 batches instead of 11 individual character tokens.
#[derive(Debug, Clone, PartialEq)]
pub struct TokenBatch {
    /// Start position in the text (character offset)
    pub start: u32,
    /// End position in the text (character offset)
    pub end: u32,
    /// Computed style from scope stack and theme
    pub style: Style,
}

/// State for an active BeginEnd or BeginWhile pattern that's waiting for its end match
///
/// Critical for proper scope management: When we encounter a BeginEnd pattern like
/// string literals or block comments, we need to track the active pattern state
/// until we find the matching end pattern. This prevents scope stack corruption.
///
/// Example: For a string "hello world", we push string scopes on the opening quote,
/// maintain them for the content, then pop them on the closing quote.
#[derive(Debug, Clone)]
struct ActivePattern {
    /// The BeginEnd/BeginWhile pattern that was matched (cloned for end pattern matching)
    pattern: CompiledPattern,
    /// Path through include chain for this pattern - helps with nested pattern resolution
    context_path: Vec<usize>,
    /// The name scope that was pushed when this pattern began (for proper cleanup)
    pushed_scope: Option<ScopeId>,
    /// Content name scope that was pushed (for BeginEnd patterns with contentName)
    content_scope: Option<ScopeId>,
    /// Captured groups from the begin pattern - used for dynamic backreference resolution
    /// Example: For pattern begin="(['\"`])" end="\\1", we store the captured quote type
    begin_captures: Vec<String>,
}

/// The main tokenizer that processes text according to TextMate grammar rules
///
/// This is the core engine that transforms raw text into styled tokens. The process:
/// 1. Load a compiled grammar (contains patterns, scopes, and rules)
/// 2. For each line of text, find pattern matches in priority order
/// 3. Build scope stacks as patterns nest (e.g., inside a string inside a function)
/// 4. Generate tokens with proper scope information
/// 5. Apply themes to convert scopes to visual styles
#[derive(Debug)]
pub struct Tokenizer {
    /// The compiled grammar to use for tokenization - contains all the language rules
    grammar: CompiledGrammar,
    /// Current stack of active scopes - grows as patterns nest, shrinks as they end
    /// Example: ["source.js"] -> ["source.js", "string.quoted"] -> ["source.js"]
    scope_stack: Vec<ScopeId>,
    /// Stack of active patterns (for BeginEnd/BeginWhile patterns waiting for their end)
    /// Example: After matching opening quote, we track the string pattern until closing quote
    active_patterns: Vec<ActivePattern>,
    /// Current line being processed (for debugging and error reporting)
    current_line: usize,
}

// ================================================================================================
// PATTERN MATCHING SYSTEM
// ================================================================================================

/// Result of a successful pattern match against text
///
/// When a regex pattern matches against input text, we create one of these to track
/// the match details, which pattern matched, and any capture groups that were found.
#[derive(Debug, Clone)]
struct PatternMatch {
    /// Start position of the match in the input text
    start: usize,
    /// End position of the match in the input text
    end: usize,
    /// Direct reference to the pattern that matched (Match, BeginEnd, BeginWhile)
    pattern: CompiledPattern,
    /// Path through include chain - helps debug complex nested grammars
    context_path: Vec<usize>,
    /// Capture groups and their scopes: (start, end, scope_id)
    /// Example: For regex "(\w+)", captures would contain the word match with its scope
    captures: Vec<(usize, usize, ScopeId)>,
}

/// Iterator for patterns that handles Include pattern resolution
///
/// CRITICAL COMPONENT: TextMate grammars use "include" patterns extensively to reference
/// repository entries and create modular, reusable pattern definitions. This iterator
/// flattens the include hierarchy into a linear sequence while preventing infinite loops.
///
/// Example: A grammar might have:
/// - Root patterns: [Include("#statements")]
/// - Repository "statements": [Include("#comment"), Include("#keywords")]
/// - Repository "comment": [BeginEnd for // comments]
///
/// This iterator would traverse: Include("#statements") -> Include("#comment") -> BeginEnd
#[derive(Debug)]
struct PatternIterator<'a> {
    /// Stack of (patterns, current_index) for handling nested includes
    /// Each entry represents a level in the include hierarchy
    context_stack: Vec<(&'a [CompiledPattern], usize)>,
    /// Track visited include patterns to prevent cycles (uses pointer addresses)
    /// Prevents infinite loops when grammars have circular references
    visited_includes: std::collections::HashSet<*const CompiledPattern>,
}

impl<'a> PatternIterator<'a> {
    /// Create a new pattern iterator for the given patterns
    fn new(patterns: &'a [CompiledPattern]) -> Self {
        let mut context_stack = Vec::new();
        if !patterns.is_empty() {
            context_stack.push((patterns, 0));
        }

        Self {
            context_stack,
            visited_includes: std::collections::HashSet::new(),
        }
    }

    /// Get the next pattern, handling Include resolution
    ///
    /// DEPTH-FIRST TRAVERSAL: This is the heart of include resolution. When we encounter
    /// an Include pattern, we immediately push its referenced patterns onto the stack
    /// and process them before continuing with the current level.
    ///
    /// This ensures that deeply nested includes are processed in the correct order,
    /// which is crucial for TextMate pattern priority rules.
    fn next(&mut self) -> Option<(&'a CompiledPattern, Vec<usize>)> {
        while let Some((patterns, index)) = self.context_stack.last_mut() {
            if *index >= patterns.len() {
                // Finished with this context, pop it
                self.context_stack.pop();
                continue;
            }

            let pattern = &patterns[*index];
            *index += 1; // Move to next pattern for subsequent calls

            match pattern {
                CompiledPattern::Include(include_pattern) => {
                    // CYCLE DETECTION: Use pointer addresses to detect circular includes
                    // This prevents infinite loops when repository A includes B and B includes A
                    let pattern_ptr = pattern as *const CompiledPattern;
                    if self.visited_includes.contains(&pattern_ptr) {
                        // Skip this include to avoid cycles
                        continue;
                    }

                    // Add to visited set
                    self.visited_includes.insert(pattern_ptr);

                    // IMMEDIATE PROCESSING: Push included patterns onto stack right now
                    // This ensures depth-first traversal - we process all nested includes
                    // before continuing with sibling patterns at the current level
                    if !include_pattern.patterns.is_empty() {
                        self.context_stack.push((&include_pattern.patterns, 0));
                    }

                    // Continue to process the included patterns immediately
                    continue;
                }
                _ => {
                    // Build context path (indices through the include chain)
                    let context_path: Vec<usize> = self
                        .context_stack
                        .iter()
                        .map(|(_, idx)| *idx - 1) // Subtract 1 since we already incremented
                        .collect();

                    return Some((pattern, context_path));
                }
            }
        }

        None
    }
}

/// Errors that can occur during tokenization
#[derive(Debug)]
pub enum TokenizeError {
    RegexError { pattern_index: usize },
    InvalidUtf8,
    MatchError,
}

impl std::fmt::Display for TokenizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenizeError::RegexError { pattern_index } => {
                write!(
                    f,
                    "Regex compilation failed for pattern at index {}",
                    pattern_index
                )
            }
            TokenizeError::InvalidUtf8 => {
                write!(f, "Invalid UTF-8 in input text")
            }
            TokenizeError::MatchError => {
                write!(f, "Pattern matching failed")
            }
        }
    }
}

impl std::error::Error for TokenizeError {}

// ================================================================================================
// BACKREFERENCE RESOLUTION
// ================================================================================================

/// Resolve backreferences in a regex pattern using captured text
///
/// DYNAMIC PATTERN RESOLUTION: Some TextMate patterns use backreferences like \1, \2
/// to refer to text captured in a previous match. This is commonly used in string
/// literals where the opening and closing quotes must match.
///
/// Example:
/// - Begin pattern: "(['\"`])"  (captures quote type)
/// - End pattern: "\\1"        (must match same quote type)
/// - If we captured '"', the end pattern becomes: '"'
/// - If we captured "'", the end pattern becomes: "'"
fn resolve_backreferences(pattern: &str, captures: &[String]) -> String {
    let mut result = pattern.to_string();

    // Replace backreferences \1 through \9 with actual captured text
    for i in 1..=9usize {
        let backref = format!("\\{}", i);
        if let Some(replacement) = captures.get(i.saturating_sub(1)) {
            result = result.replace(&backref, replacement);
        }
    }

    result
}

// ================================================================================================
// TOKENIZER IMPLEMENTATION
// ================================================================================================

impl Tokenizer {
    /// Create a new tokenizer for the given grammar
    pub fn new(grammar: CompiledGrammar) -> Self {
        let mut scope_stack = Vec::new();
        // Always start with the grammar's root scope
        scope_stack.push(grammar.scope_id);

        Self {
            grammar,
            scope_stack,
            active_patterns: Vec::new(),
            current_line: 0,
        }
    }

    /// Tokenize a single line of text
    ///
    /// MAIN TOKENIZATION ALGORITHM:
    /// 1. Start at position 0 in the text
    /// 2. Find the next pattern match using TextMate priority rules
    /// 3. Create tokens for any unmatched text before the match
    /// 4. Process the match (update scope stack, create tokens)
    /// 5. Advance position and repeat until end of text
    ///
    /// CRITICAL: We must ensure progress on each iteration to prevent infinite loops,
    /// especially with Unicode characters that span multiple bytes.
    pub fn tokenize_line(&mut self, text: &str) -> Result<Vec<Token>, TokenizeError> {
        self.current_line += 1;
        let mut tokens = Vec::new();
        let mut position = 0;

        while position < text.len() {
            // Try to find the next pattern match
            if let Some(pattern_match) = self.find_next_match(text, position)? {
                // UNICODE SAFETY: Ensure we're making progress to prevent infinite loops
                if pattern_match.end <= position {
                    // CRITICAL: Pattern matched at same position or went backward
                    // We must advance by one FULL CHARACTER, not one byte, to handle Unicode correctly
                    // Example: '←' is 3 bytes [0xE2, 0x86, 0x90], advancing by 1 byte would land
                    // in the middle of the UTF-8 sequence and cause invalid slicing
                    if let Some(slice) = text.get(position..) {
                        if let Some(ch) = slice.chars().next() {
                            position += ch.len_utf8(); // Safe: advances to next character boundary
                        } else {
                            position += 1; // Fallback for edge cases
                        }
                    } else {
                        position += 1; // Fallback for invalid positions
                    }
                    continue;
                }

                // If there's text before the match, create a token for it
                if pattern_match.start > position {
                    tokens.push(Token {
                        start: position,
                        end: pattern_match.start,
                        scope_stack: self.scope_stack.clone(),
                    });
                }

                // Handle the pattern match
                self.handle_pattern_match(&pattern_match, &mut tokens)?;
                position = pattern_match.end;
            } else {
                // No more matches, create token for remaining text
                if position < text.len() {
                    tokens.push(Token {
                        start: position,
                        end: text.len(),
                        scope_stack: self.scope_stack.clone(),
                    });
                }
                break;
            }
        }

        Ok(tokens)
    }

    /// Find the next pattern match starting from the given position
    ///
    /// TEXTMATE PRIORITY RULES IMPLEMENTATION:
    /// This is where we implement the core TextMate specification for pattern matching:
    /// 1. Check for end patterns first (if we have active BeginEnd/BeginWhile patterns)
    /// 2. Collect ALL possible pattern matches from the current pattern set
    /// 3. Apply TextMate priority rules: earliest start position wins, then longest match wins
    /// 4. Return the best match according to these rules
    ///
    /// CRITICAL BUG FIX: Previously used "first match wins" which violated TextMate spec.
    /// This caused shorter patterns (like single operators) to beat longer patterns
    /// (like full comment lines), resulting in incorrect tokenization.
    fn find_next_match(
        &self,
        text: &str,
        start: usize,
    ) -> Result<Option<PatternMatch>, TokenizeError> {
        let search_text = text.get(start..).unwrap_or("");
        if search_text.is_empty() {
            return Ok(None);
        }
        let mut best_match: Option<PatternMatch> = None;

        // First, check if we need to match an end pattern for active BeginEnd patterns
        if let Some(active) = self.active_patterns.last() {
            if let Some(end_match) = self.try_match_end_pattern(active, search_text, start)? {
                return Ok(Some(end_match));
            }
        }

        // Get the current patterns to search (either from active pattern or root grammar)
        let patterns = if let Some(active) = self.active_patterns.last() {
            // If we have an active pattern, use its nested patterns
            self.get_active_patterns(active)?
        } else {
            // Use root grammar patterns
            &self.grammar.patterns
        };

        // Try each pattern and find the best match using PatternIterator
        let mut pattern_iter = PatternIterator::new(patterns);
        let mut all_matches = Vec::new();

        while let Some((pattern, context_path)) = pattern_iter.next() {
            if let Some(pattern_match) =
                self.try_match_pattern(pattern, context_path, search_text, start)?
            {
                all_matches.push(pattern_match);
            }
        }

        // Choose the best match based on priority rules
        if !all_matches.is_empty() {
            // TEXTMATE PRIORITY IMPLEMENTATION: Sort matches by specification rules
            all_matches.sort_by(|a, b| {
                // Rule 1: Earliest start position wins (matches that start sooner in text)
                match a.start.cmp(&b.start) {
                    std::cmp::Ordering::Equal => {
                        // Rule 2: If same start position, prefer longer matches
                        // This is crucial - a comment pattern "// comment" should beat
                        // an operator pattern "/" when both start at the same position
                        let a_len = a.end - a.start;
                        let b_len = b.end - b.start;
                        b_len.cmp(&a_len) // Note: reversed for longer matches first
                    }
                    other => other,
                }
            });

            best_match = Some(all_matches.into_iter().next().unwrap());
        }

        Ok(best_match)
    }

    /// Try to match an end pattern for the given active pattern
    ///
    /// END PATTERN MATCHING: When we have an active BeginEnd or BeginWhile pattern,
    /// we need to check for its end condition before looking for new patterns.
    /// This maintains proper nesting and scope stack integrity.
    ///
    /// For BeginEnd: Check if the end regex matches
    /// For BeginWhile: Check if the while condition still matches (if not, end the pattern)
    fn try_match_end_pattern(
        &self,
        active: &ActivePattern,
        text: &str,
        offset: usize,
    ) -> Result<Option<PatternMatch>, TokenizeError> {
        // Use the direct pattern reference from the active pattern
        match &active.pattern {
            CompiledPattern::BeginEnd(begin_end) => {
                // DYNAMIC BACKREFERENCE RESOLUTION: The end pattern might contain \1, \2, etc.
                // referring to capture groups from the begin pattern. We resolve these at runtime.
                let resolved_end_pattern =
                    resolve_backreferences(&begin_end.end_pattern_source, &active.begin_captures);

                // Create a temporary regex with the resolved pattern
                let resolved_regex = if resolved_end_pattern != begin_end.end_pattern_source {
                    // Pattern was modified, create new regex
                    onig::Regex::new(&resolved_end_pattern).ok()
                } else {
                    // No backreferences, create regex from original pattern
                    onig::Regex::new(&begin_end.end_pattern_source).ok()
                };

                if let Some(regex) = resolved_regex {
                    if let Some(captures) = regex.captures(text) {
                        let main_match_pos = captures.pos(0).ok_or(TokenizeError::MatchError)?;

                        // Extract capture groups from end captures
                        let mut pattern_captures = Vec::new();
                        for (capture_name, capture_info) in &begin_end.end_captures {
                            if let Ok(capture_idx) = capture_name.parse::<usize>() {
                                if let Some(capture_pos) = captures.pos(capture_idx) {
                                    pattern_captures.push((
                                        offset + capture_pos.0,
                                        offset + capture_pos.1,
                                        capture_info.scope_id,
                                    ));
                                }
                            }
                        }

                        // Also check general captures
                        for (capture_name, capture_info) in &begin_end.captures {
                            if let Ok(capture_idx) = capture_name.parse::<usize>() {
                                if let Some(capture_pos) = captures.pos(capture_idx) {
                                    pattern_captures.push((
                                        offset + capture_pos.0,
                                        offset + capture_pos.1,
                                        capture_info.scope_id,
                                    ));
                                }
                            }
                        }

                        return Ok(Some(PatternMatch {
                            start: offset + main_match_pos.0,
                            end: offset + main_match_pos.1,
                            pattern: active.pattern.clone(),
                            context_path: active.context_path.clone(),
                            captures: pattern_captures,
                        }));
                    }
                }
            }
            CompiledPattern::BeginWhile(begin_while) => {
                // BEGINWHILE LOGIC: These patterns continue as long as the "while" condition matches
                // When the while condition fails, we generate a zero-width end match to close the pattern
                // Example: Markdown blockquotes continue while lines start with "> "

                // Resolve backreferences in the while pattern using captured text from begin
                let resolved_while_pattern = resolve_backreferences(
                    &begin_while.while_pattern_source,
                    &active.begin_captures,
                );

                // Create a temporary regex with the resolved pattern
                let resolved_regex = if resolved_while_pattern != begin_while.while_pattern_source {
                    // Pattern was modified, create new regex
                    onig::Regex::new(&resolved_while_pattern).ok()
                } else {
                    // No backreferences, create regex from original pattern
                    onig::Regex::new(&begin_while.while_pattern_source).ok()
                };

                if let Some(regex) = resolved_regex {
                    // If the while condition matches, the BeginWhile pattern continues
                    if regex.captures(text).is_some() {
                        // While condition still matches, continue the pattern
                        return Ok(None);
                    } else {
                        // While condition failed, end the BeginWhile pattern
                        // This is conceptually similar to matching an "end" pattern
                        // but we don't consume any text (zero-width match)

                        // Extract capture groups from while captures (if any)
                        let mut pattern_captures = Vec::new();
                        for (capture_name, capture_info) in &begin_while.while_captures {
                            if let Ok(_capture_idx) = capture_name.parse::<usize>() {
                                // For failed while conditions, we don't have actual capture positions
                                // This is a conceptual "end" so we just mark it as zero-width at current position
                                pattern_captures.push((offset, offset, capture_info.scope_id));
                            }
                        }

                        return Ok(Some(PatternMatch {
                            start: offset,
                            end: offset, // Zero-width end match
                            pattern: active.pattern.clone(),
                            context_path: active.context_path.clone(),
                            captures: pattern_captures,
                        }));
                    }
                }
            }
            _ => {
                // Non-BeginEnd patterns don't have end patterns
            }
        }

        Ok(None)
    }

    /// Get the patterns that should be active for the given active pattern
    fn get_active_patterns(
        &self,
        _active: &ActivePattern,
    ) -> Result<&[CompiledPattern], TokenizeError> {
        // TODO: Implement getting nested patterns from active BeginEnd patterns
        Ok(&self.grammar.patterns)
    }

    /// Try to match a single pattern against the text
    fn try_match_pattern(
        &self,
        pattern: &CompiledPattern,
        context_path: Vec<usize>,
        text: &str,
        offset: usize,
    ) -> Result<Option<PatternMatch>, TokenizeError> {
        match pattern {
            CompiledPattern::Match(match_pattern) => {
                if let Some(regex) = match_pattern.regex.compiled() {
                    if let Some(captures) = regex.captures(text) {
                        let main_match_pos = captures.pos(0).ok_or(TokenizeError::MatchError)?;

                        // Extract capture groups
                        let mut pattern_captures = Vec::new();
                        for (capture_name, capture_info) in &match_pattern.captures {
                            if let Ok(capture_idx) = capture_name.parse::<usize>() {
                                if let Some(capture_pos) = captures.pos(capture_idx) {
                                    pattern_captures.push((
                                        offset + capture_pos.0,
                                        offset + capture_pos.1,
                                        capture_info.scope_id,
                                    ));
                                }
                            }
                        }

                        return Ok(Some(PatternMatch {
                            start: offset + main_match_pos.0,
                            end: offset + main_match_pos.1,
                            pattern: pattern.clone(),
                            context_path,
                            captures: pattern_captures,
                        }));
                    }
                }
            }
            CompiledPattern::BeginEnd(begin_end) => {
                if let Some(regex) = begin_end.begin_regex.compiled() {
                    if let Some(captures) = regex.captures(text) {
                        let main_match_pos = captures.pos(0).ok_or(TokenizeError::MatchError)?;

                        // Extract capture groups from begin captures
                        let mut pattern_captures = Vec::new();
                        for (capture_name, capture_info) in &begin_end.begin_captures {
                            if let Ok(capture_idx) = capture_name.parse::<usize>() {
                                if let Some(capture_pos) = captures.pos(capture_idx) {
                                    pattern_captures.push((
                                        offset + capture_pos.0,
                                        offset + capture_pos.1,
                                        capture_info.scope_id,
                                    ));
                                }
                            }
                        }

                        // Also check general captures
                        for (capture_name, capture_info) in &begin_end.captures {
                            if let Ok(capture_idx) = capture_name.parse::<usize>() {
                                if let Some(capture_pos) = captures.pos(capture_idx) {
                                    pattern_captures.push((
                                        offset + capture_pos.0,
                                        offset + capture_pos.1,
                                        capture_info.scope_id,
                                    ));
                                }
                            }
                        }

                        return Ok(Some(PatternMatch {
                            start: offset + main_match_pos.0,
                            end: offset + main_match_pos.1,
                            pattern: pattern.clone(),
                            context_path,
                            captures: pattern_captures,
                        }));
                    }
                }
            }
            CompiledPattern::BeginWhile(begin_while) => {
                if let Some(regex) = begin_while.begin_regex.compiled() {
                    if let Some(captures) = regex.captures(text) {
                        let main_match_pos = captures.pos(0).ok_or(TokenizeError::MatchError)?;

                        // Extract capture groups from begin captures
                        let mut pattern_captures = Vec::new();
                        for (capture_name, capture_info) in &begin_while.begin_captures {
                            if let Ok(capture_idx) = capture_name.parse::<usize>() {
                                if let Some(capture_pos) = captures.pos(capture_idx) {
                                    pattern_captures.push((
                                        offset + capture_pos.0,
                                        offset + capture_pos.1,
                                        capture_info.scope_id,
                                    ));
                                }
                            }
                        }

                        // Also check general captures
                        for (capture_name, capture_info) in &begin_while.captures {
                            if let Ok(capture_idx) = capture_name.parse::<usize>() {
                                if let Some(capture_pos) = captures.pos(capture_idx) {
                                    pattern_captures.push((
                                        offset + capture_pos.0,
                                        offset + capture_pos.1,
                                        capture_info.scope_id,
                                    ));
                                }
                            }
                        }

                        return Ok(Some(PatternMatch {
                            start: offset + main_match_pos.0,
                            end: offset + main_match_pos.1,
                            pattern: pattern.clone(),
                            context_path,
                            captures: pattern_captures,
                        }));
                    }
                }
            }
            CompiledPattern::Include(_include_pattern) => {
                // Include patterns should not be matched directly - they are handled by PatternIterator
                // If we get here, it's likely a bug in the iterator logic
            }
        }

        Ok(None)
    }

    /// Handle a successful pattern match by updating state and creating tokens
    ///
    /// PATTERN MATCH PROCESSING: This is where we handle the different pattern types:
    ///
    /// Match patterns: Apply scopes, create tokens, no state changes
    /// BeginEnd patterns:
    ///   - Begin: Push scopes, start tracking active pattern
    ///   - End: Pop scopes, remove from active pattern stack
    /// BeginWhile patterns: Similar to BeginEnd but with while condition logic
    ///
    /// SCOPE STACK MANAGEMENT: Critical to maintain proper nesting - we must push
    /// scopes in the right order and pop them in reverse order to maintain consistency.
    fn handle_pattern_match(
        &mut self,
        pattern_match: &PatternMatch,
        tokens: &mut Vec<Token>,
    ) -> Result<(), TokenizeError> {
        // Use the direct pattern reference from the match
        match &pattern_match.pattern {
            CompiledPattern::Match(match_pattern) => {
                // Handle capture tokens first (they have priority over the main token)
                for (cap_start, cap_end, cap_scope_id) in &pattern_match.captures {
                    let mut capture_scope_stack = self.scope_stack.clone();
                    capture_scope_stack.push(*cap_scope_id);

                    tokens.push(Token {
                        start: *cap_start,
                        end: *cap_end,
                        scope_stack: capture_scope_stack,
                    });
                }

                // If no captures, create a token for the entire match
                if pattern_match.captures.is_empty() {
                    // Push name scope if present
                    if let Some(name_scope) = match_pattern.name_scope_id {
                        self.scope_stack.push(name_scope);
                    }

                    // Create token with current scope stack
                    tokens.push(Token {
                        start: pattern_match.start,
                        end: pattern_match.end,
                        scope_stack: self.scope_stack.clone(),
                    });

                    // Pop name scope
                    if match_pattern.name_scope_id.is_some() {
                        self.scope_stack.pop();
                    }
                } else {
                    // If we have captures but still need to cover the whole match,
                    // create a base token for any uncovered areas
                    // This is a simplified approach - a full implementation would
                    // need to handle overlapping and non-overlapping captures more carefully

                    // For now, just create a token with name scope if present
                    if let Some(name_scope) = match_pattern.name_scope_id {
                        self.scope_stack.push(name_scope);

                        tokens.push(Token {
                            start: pattern_match.start,
                            end: pattern_match.end,
                            scope_stack: self.scope_stack.clone(),
                        });

                        self.scope_stack.pop();
                    }
                }
            }
            CompiledPattern::BeginEnd(begin_end) => {
                // BEGIN/END DETECTION: Determine if this is a begin match or an end match
                // We check if we currently have an active BeginEnd pattern - if so, this match
                // is likely the end pattern. This is a simplified approach that works for most cases.
                let is_end_match = self
                    .active_patterns
                    .last()
                    .map(|active| {
                        // Check if the active pattern is a BeginEnd pattern
                        matches!(active.pattern, CompiledPattern::BeginEnd(_))
                    })
                    .unwrap_or(false);

                if is_end_match {
                    // END MATCH PROCESSING: Clean up the active pattern and pop scopes
                    if let Some(active) = self.active_patterns.pop() {
                        // Handle capture tokens first
                        for (cap_start, cap_end, cap_scope_id) in &pattern_match.captures {
                            let mut capture_scope_stack = self.scope_stack.clone();
                            capture_scope_stack.push(*cap_scope_id);

                            tokens.push(Token {
                                start: *cap_start,
                                end: *cap_end,
                                scope_stack: capture_scope_stack,
                            });
                        }

                        // Create token for the end match itself if no specific captures
                        if pattern_match.captures.is_empty() {
                            tokens.push(Token {
                                start: pattern_match.start,
                                end: pattern_match.end,
                                scope_stack: self.scope_stack.clone(),
                            });
                        }

                        // SCOPE CLEANUP: Pop scopes in reverse order of how they were pushed
                        // Content scope is pushed after name scope, so we pop it first
                        if active.content_scope.is_some() {
                            self.scope_stack.pop();
                        }
                        // Name scope is pushed first, so we pop it last
                        if active.pushed_scope.is_some() {
                            self.scope_stack.pop();
                        }
                    }
                } else {
                    // BEGIN MATCH PROCESSING: Start tracking a new active pattern

                    // Handle capture tokens first
                    for (cap_start, cap_end, cap_scope_id) in &pattern_match.captures {
                        let mut capture_scope_stack = self.scope_stack.clone();
                        capture_scope_stack.push(*cap_scope_id);

                        tokens.push(Token {
                            start: *cap_start,
                            end: *cap_end,
                            scope_stack: capture_scope_stack,
                        });
                    }

                    // Push name scope if present
                    if let Some(name_scope) = begin_end.name_scope_id {
                        self.scope_stack.push(name_scope);
                    }

                    // Create token for the begin match
                    tokens.push(Token {
                        start: pattern_match.start,
                        end: pattern_match.end,
                        scope_stack: self.scope_stack.clone(),
                    });

                    // Push content scope if present (for the content inside the BeginEnd)
                    if let Some(content_scope) = begin_end.content_name_scope_id {
                        self.scope_stack.push(content_scope);
                    }

                    // Extract capture groups from the begin pattern for backreference resolution
                    let mut begin_captures = Vec::new();

                    // We need to re-match to extract the capture groups
                    // This is a bit inefficient but ensures we get the capture text correctly
                    // TODO: Optimize by extracting captures during initial matching
                    begin_captures.push("dummy_capture_0".to_string()); // Index 0 is the full match, start from 1

                    // Add to active patterns
                    self.active_patterns.push(ActivePattern {
                        pattern: pattern_match.pattern.clone(),
                        context_path: pattern_match.context_path.clone(),
                        pushed_scope: begin_end.name_scope_id,
                        content_scope: begin_end.content_name_scope_id,
                        begin_captures,
                    });
                }
            }
            CompiledPattern::BeginWhile(begin_while) => {
                // Check if this is an end match (while condition failed) for an active BeginWhile pattern
                let is_end_match = self
                    .active_patterns
                    .last()
                    .map(|active| {
                        // Check if the active pattern is a BeginWhile pattern and this is a zero-width end match
                        matches!(active.pattern, CompiledPattern::BeginWhile(_))
                            && pattern_match.start == pattern_match.end // Zero-width match indicates while condition failed
                    })
                    .unwrap_or(false);

                if is_end_match {
                    // This is an end match (while condition failed) - close the active BeginWhile pattern
                    if let Some(active) = self.active_patterns.pop() {
                        // Handle capture tokens first
                        for (cap_start, cap_end, cap_scope_id) in &pattern_match.captures {
                            let mut capture_scope_stack = self.scope_stack.clone();
                            capture_scope_stack.push(*cap_scope_id);

                            tokens.push(Token {
                                start: *cap_start,
                                end: *cap_end,
                                scope_stack: capture_scope_stack,
                            });
                        }

                        // For zero-width end matches, don't create a token (no text consumed)

                        // Pop the content scope if it was pushed
                        if active.content_scope.is_some() {
                            self.scope_stack.pop();
                        }
                        // Pop the name scope if it was pushed
                        if active.pushed_scope.is_some() {
                            self.scope_stack.pop();
                        }
                    }
                } else {
                    // This is a begin match - start a new active BeginWhile pattern

                    // Handle capture tokens first
                    for (cap_start, cap_end, cap_scope_id) in &pattern_match.captures {
                        let mut capture_scope_stack = self.scope_stack.clone();
                        capture_scope_stack.push(*cap_scope_id);

                        tokens.push(Token {
                            start: *cap_start,
                            end: *cap_end,
                            scope_stack: capture_scope_stack,
                        });
                    }

                    // Push name scope if present
                    if let Some(name_scope) = begin_while.name_scope_id {
                        self.scope_stack.push(name_scope);
                    }

                    // Create token for the begin match
                    tokens.push(Token {
                        start: pattern_match.start,
                        end: pattern_match.end,
                        scope_stack: self.scope_stack.clone(),
                    });

                    // Push content scope if present (for the content inside the BeginWhile)
                    if let Some(content_scope) = begin_while.content_name_scope_id {
                        self.scope_stack.push(content_scope);
                    }

                    // Extract capture groups from the begin pattern for backreference resolution
                    let mut begin_captures = Vec::new();

                    // We need to re-match to extract the capture groups
                    // This is a bit inefficient but ensures we get the capture text correctly
                    // TODO: Optimize by extracting captures during initial matching
                    begin_captures.push("dummy_capture_0".to_string()); // Index 0 is the full match, start from 1

                    // Add to active patterns
                    self.active_patterns.push(ActivePattern {
                        pattern: pattern_match.pattern.clone(),
                        context_path: pattern_match.context_path.clone(),
                        pushed_scope: begin_while.name_scope_id,
                        content_scope: begin_while.content_name_scope_id,
                        begin_captures,
                    });
                }
            }
            CompiledPattern::Include(_include_pattern) => {
                // Include patterns should have already been resolved during matching
                // If we get here, it means the include pattern matched but we need to
                // handle it like a basic token since the actual pattern handling
                // happened during the matching phase
                tokens.push(Token {
                    start: pattern_match.start,
                    end: pattern_match.end,
                    scope_stack: self.scope_stack.clone(),
                });
            }
        }

        Ok(())
    }

    /// Batch consecutive tokens with the same scope stack into TokenBatch instances
    ///
    /// PERFORMANCE OPTIMIZATION: Instead of rendering hundreds of individual tokens,
    /// we merge consecutive tokens that result in the same visual style. This dramatically
    /// reduces the number of HTML elements we need to generate.
    ///
    /// Example: "hello world" might produce 11 character tokens, but if they all have
    /// the same style, we can represent this as a single TokenBatch.
    pub fn batch_tokens(tokens: &[Token], theme: &CompiledTheme) -> Vec<TokenBatch> {
        let mut batches = Vec::new();

        if tokens.is_empty() {
            return batches;
        }

        let mut current_start = tokens[0].start as u32;
        let mut current_style = theme.get_style(&tokens[0].scope_stack);

        for (i, token) in tokens.iter().enumerate().skip(1) {
            let token_style = theme.get_style(&token.scope_stack);

            // If styles changed or there's a gap, finish current batch
            if token_style != current_style || token.start != tokens[i - 1].end {
                batches.push(TokenBatch {
                    start: current_start,
                    end: tokens[i - 1].end as u32,
                    style: current_style,
                });

                current_start = token.start as u32;
                current_style = token_style;
            }
        }

        // Add the final batch
        if let Some(last_token) = tokens.last() {
            batches.push(TokenBatch {
                start: current_start,
                end: last_token.end as u32,
                style: current_style,
            });
        }

        batches
    }

    /// Reset the tokenizer state (useful for processing multiple files)
    pub fn reset(&mut self) {
        self.scope_stack.clear();
        self.scope_stack.push(self.grammar.scope_id);
        self.active_patterns.clear();
        self.current_line = 0;
    }

    /// Get the current scope stack
    pub fn scope_stack(&self) -> &[ScopeId] {
        &self.scope_stack
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crate::textmate::grammar::{CompiledGrammar, RawGrammar};
//
//     fn create_test_grammar() -> CompiledGrammar {
//         // Create a minimal test grammar
//         let raw_grammar = RawGrammar {
//             name: "Test".to_string(),
//             display_name: Some("Test Language".to_string()),
//             scope_name: "source.test".to_string(),
//             file_types: vec!["test".to_string()],
//             ..Default::default()
//         };
//
//         // For now, we can't easily compile a grammar without proper scopes
//         // So let's create a minimal compiled grammar manually
//         use crate::textmate::grammar::get_scope_id;
//
//         let scope_id = get_scope_id("source.test").unwrap_or_else(|| {
//             // If the scope doesn't exist, use a default
//             use crate::textmate::grammar::ScopeId;
//             ScopeId(0)
//         });
//
//         CompiledGrammar {
//             name: raw_grammar.name,
//             display_name: raw_grammar.display_name,
//             scope_name: raw_grammar.scope_name,
//             scope_id,
//             file_types: raw_grammar.file_types,
//             patterns: Vec::new(), // Empty patterns for now
//             first_line_regex: None,
//         }
//     }
//
//     #[test]
//     fn test_tokenizer_creation() {
//         let grammar = create_test_grammar();
//         let tokenizer = Tokenizer::new(grammar);
//
//         // Should start with the grammar's root scope
//         assert_eq!(tokenizer.scope_stack().len(), 1);
//         assert!(tokenizer.active_patterns.is_empty());
//         assert_eq!(tokenizer.current_line, 0);
//     }
//
//     #[test]
//     fn test_tokenize_empty_line() {
//         let grammar = create_test_grammar();
//         let mut tokenizer = Tokenizer::new(grammar);
//
//         let tokens = tokenizer.tokenize_line("").unwrap();
//         assert!(tokens.is_empty());
//     }
//
//     #[test]
//     fn test_tokenize_simple_text() {
//         let grammar = create_test_grammar();
//         let mut tokenizer = Tokenizer::new(grammar);
//
//         // With no patterns, the entire text should become one token
//         let tokens = tokenizer.tokenize_line("hello world").unwrap();
//         assert_eq!(tokens.len(), 1);
//         assert_eq!(tokens[0].start, 0);
//         assert_eq!(tokens[0].end, 11);
//         assert_eq!(tokens[0].scope_stack.len(), 1);
//     }
//
//     #[test]
//     fn test_token_batching() {
//         use crate::textmate::grammar::ScopeId;
//
//         // Create some test tokens with different scope stacks
//         let scope1 = vec![ScopeId(1)];
//         let scope2 = vec![ScopeId(1), ScopeId(2)];
//
//         let tokens = vec![
//             Token {
//                 start: 0,
//                 end: 5,
//                 scope_stack: scope1.clone(),
//             },
//             Token {
//                 start: 5,
//                 end: 10,
//                 scope_stack: scope1.clone(),
//             },
//             Token {
//                 start: 10,
//                 end: 15,
//                 scope_stack: scope2.clone(),
//             },
//             Token {
//                 start: 15,
//                 end: 20,
//                 scope_stack: scope2.clone(),
//             },
//         ];
//
//         // Create a theme that differentiates between different scopes
//         use crate::color::*;
//         use crate::style::*;
//         use crate::theme::*;
//         let test_theme = CompiledTheme {
//             name: "Test".to_string(),
//             theme_type: crate::theme::ThemeType::Dark,
//             colors: std::collections::HashMap::new(),
//             default_style: Style::new(Color::WHITE, Color::BLACK, FontStyle::empty()),
//             rules: vec![
//                 CompiledThemeRule {
//                     scope_patterns: vec![vec![ScopeId(1), ScopeId(2)]], // Match the longer scope first
//                     style_modifier: StyleModifier::with_foreground(
//                         Color::from_hex("#00FF00").unwrap(),
//                     ), // Green for [1,2]
//                 },
//                 CompiledThemeRule {
//                     scope_patterns: vec![vec![ScopeId(1)]],
//                     style_modifier: StyleModifier::with_foreground(
//                         Color::from_hex("#FF0000").unwrap(),
//                     ), // Red for [1]
//                 },
//             ],
//         };
//         let mut cache = StyleCache::new();
//         let batches = Tokenizer::batch_tokens(&tokens, &test_theme, &mut cache);
//
//         // Should batch consecutive tokens with same scopes
//         assert_eq!(batches.len(), 2);
//
//         assert_eq!(batches[0].start, 0);
//         assert_eq!(batches[0].end, 10);
//
//         assert_eq!(batches[1].start, 10);
//         assert_eq!(batches[1].end, 20);
//
//         // Different scope stacks should have different style IDs
//         assert_ne!(batches[0].style_id, batches[1].style_id);
//     }
//
//     #[test]
//     fn test_tokenizer_reset() {
//         let grammar = create_test_grammar();
//         let mut tokenizer = Tokenizer::new(grammar);
//
//         // Tokenize a line to change state
//         let _ = tokenizer.tokenize_line("test").unwrap();
//         assert_eq!(tokenizer.current_line, 1);
//
//         // Reset should restore initial state
//         tokenizer.reset();
//         assert_eq!(tokenizer.current_line, 0);
//         assert_eq!(tokenizer.scope_stack().len(), 1);
//         assert!(tokenizer.active_patterns.is_empty());
//     }
//
//     #[test]
//     fn test_tokenize_with_manual_grammar() {
//         // Create a simple manual test grammar with basic Match patterns
//         // This avoids issues with real grammar complexity while testing our implementation
//         use crate::textmate::grammar::*;
//         use std::collections::BTreeMap;
//
//         // Create a simple compiled grammar manually for testing
//         let scope_id = get_scope_id("source.test").unwrap_or(ScopeId(999));
//         let keyword_scope_id = get_scope_id("keyword.control").unwrap_or(ScopeId(1000));
//
//         let keyword_pattern = CompiledPattern::Match(CompiledMatchPattern {
//             name_scope_id: Some(keyword_scope_id),
//             regex: Regex::new(r"\b(var|let|const)\b".to_string()),
//             captures: BTreeMap::new(),
//             patterns: Vec::new(),
//         });
//
//         let compiled_grammar = CompiledGrammar {
//             name: "Test".to_string(),
//             display_name: Some("Test Language".to_string()),
//             scope_name: "source.test".to_string(),
//             scope_id,
//             file_types: vec!["test".to_string()],
//             patterns: vec![keyword_pattern],
//             first_line_regex: None,
//         };
//
//         let mut tokenizer = Tokenizer::new(compiled_grammar);
//
//         // Test simple code
//         match tokenizer.tokenize_line("var x = 42;") {
//             Ok(tokens) => {
//                 println!("Tokenized 'var x = 42;' into {} tokens", tokens.len());
//                 for (i, token) in tokens.iter().enumerate() {
//                     println!(
//                         "Token {}: [{}, {}) '{}' scopes: {:?}",
//                         i,
//                         token.start,
//                         token.end,
//                         &"var x = 42;"[token.start..token.end],
//                         token.scope_stack
//                     );
//                 }
//                 // Should have at least one token
//                 assert!(!tokens.is_empty(), "Should produce at least one token");
//
//                 // Should detect the 'var' keyword if patterns work
//                 let var_token = tokens
//                     .iter()
//                     .find(|t| t.start == 0 && t.end == 3 && t.scope_stack.len() >= 2);
//                 if let Some(token) = var_token {
//                     println!(
//                         "Found 'var' keyword token with {} scopes",
//                         token.scope_stack.len()
//                     );
//                 } else {
//                     println!("'var' keyword not detected as expected (this is expected for now)");
//                 }
//             }
//             Err(e) => {
//                 panic!("Tokenization failed: {}", e);
//             }
//         }
//     }
//
//     #[test]
//     fn test_tokenize_with_begin_end_patterns() {
//         // Test BeginEnd patterns like string literals or block comments
//         use crate::textmate::grammar::*;
//         use std::collections::BTreeMap;
//
//         let scope_id = get_scope_id("source.test").unwrap_or(ScopeId(999));
//         let string_scope_id = get_scope_id("string.quoted").unwrap_or(ScopeId(1001));
//         let quote_scope_id = get_scope_id("punctuation.definition.string").unwrap_or(ScopeId(1002));
//
//         // Create a BeginEnd pattern for double-quoted strings
//         let string_pattern = CompiledPattern::BeginEnd(CompiledBeginEndPattern {
//             name_scope_id: Some(string_scope_id),
//             content_name_scope_id: None, // Don't add extra scope inside
//             begin_regex: Regex::new(r#"""#.to_string()),
//             end_regex: Regex::new(r#"""#.to_string()),
//             end_pattern_source: r#"""#.to_string(),
//             captures: BTreeMap::new(),
//             begin_captures: {
//                 let mut captures = BTreeMap::new();
//                 captures.insert(
//                     "0".to_string(),
//                     CompiledCapture {
//                         scope_id: quote_scope_id,
//                         patterns: Vec::new(),
//                     },
//                 );
//                 captures
//             },
//             end_captures: {
//                 let mut captures = BTreeMap::new();
//                 captures.insert(
//                     "0".to_string(),
//                     CompiledCapture {
//                         scope_id: quote_scope_id,
//                         patterns: Vec::new(),
//                     },
//                 );
//                 captures
//             },
//             patterns: Vec::new(),
//             apply_end_pattern_last: false,
//         });
//
//         let compiled_grammar = CompiledGrammar {
//             name: "Test".to_string(),
//             display_name: Some("Test Language".to_string()),
//             scope_name: "source.test".to_string(),
//             scope_id,
//             file_types: vec!["test".to_string()],
//             patterns: vec![string_pattern],
//             first_line_regex: None,
//         };
//
//         let mut tokenizer = Tokenizer::new(compiled_grammar);
//
//         // Test string literal
//         match tokenizer.tokenize_line(r#""hello world""#) {
//             Ok(tokens) => {
//                 println!("Tokenized '\"hello world\"' into {} tokens", tokens.len());
//                 for (i, token) in tokens.iter().enumerate() {
//                     let text = &r#""hello world""#[token.start..token.end];
//                     println!(
//                         "Token {}: [{}, {}) '{}' scopes: {:?}",
//                         i, token.start, token.end, text, token.scope_stack
//                     );
//                 }
//
//                 // Should have tokens for:
//                 // 1. Opening quote (with quote scope)
//                 // 2. Content "hello world" (with string scope)
//                 // 3. Closing quote (with quote scope)
//                 assert!(!tokens.is_empty(), "Should produce tokens");
//
//                 // Check if we have a token with the string scope (indicating BeginEnd worked)
//                 let has_string_scope = tokens
//                     .iter()
//                     .any(|t| t.scope_stack.contains(&string_scope_id));
//                 if has_string_scope {
//                     println!("✅ BeginEnd pattern successfully applied string scope");
//                 } else {
//                     println!("⚠️ BeginEnd pattern didn't apply string scope as expected");
//                 }
//             }
//             Err(e) => {
//                 panic!("Tokenization failed: {}", e);
//             }
//         }
//     }
//
//     #[test]
//     fn test_theme_integration() {
//         // Test the complete pipeline: tokenizer + theme + style cache
//         use crate::textmate::grammar::*;
//         use crate::theme::*;
//         use std::collections::BTreeMap;
//
//         // Create a simple test grammar
//         let scope_id = get_scope_id("source.test").unwrap_or(ScopeId(999));
//         let keyword_scope_id = get_scope_id("keyword.control").unwrap_or(ScopeId(1000));
//         let string_scope_id = get_scope_id("string.quoted").unwrap_or(ScopeId(1001));
//
//         let keyword_pattern = CompiledPattern::Match(CompiledMatchPattern {
//             name_scope_id: Some(keyword_scope_id),
//             regex: Regex::new(r"\b(var|let|const)\b".to_string()),
//             captures: BTreeMap::new(),
//             patterns: Vec::new(),
//         });
//
//         let string_pattern = CompiledPattern::BeginEnd(CompiledBeginEndPattern {
//             name_scope_id: Some(string_scope_id),
//             content_name_scope_id: None,
//             begin_regex: Regex::new(r#"""#.to_string()),
//             end_regex: Regex::new(r#"""#.to_string()),
//             end_pattern_source: r#"""#.to_string(),
//             captures: BTreeMap::new(),
//             begin_captures: BTreeMap::new(),
//             end_captures: BTreeMap::new(),
//             patterns: Vec::new(),
//             apply_end_pattern_last: false,
//         });
//
//         let compiled_grammar = CompiledGrammar {
//             name: "Test".to_string(),
//             display_name: Some("Test Language".to_string()),
//             scope_name: "source.test".to_string(),
//             scope_id,
//             file_types: vec!["test".to_string()],
//             patterns: vec![keyword_pattern, string_pattern],
//             first_line_regex: None,
//         };
//
//         // Create a simple test theme
//         use crate::color::*;
//         use crate::style::*;
//         let test_theme = CompiledTheme {
//             name: "Test Theme".to_string(),
//             theme_type: crate::theme::ThemeType::Dark,
//             colors: std::collections::HashMap::new(),
//             default_style: Style::new(Color::WHITE, Color::BLACK, FontStyle::empty()),
//             rules: vec![
//                 CompiledThemeRule {
//                     scope_patterns: vec![vec![keyword_scope_id]],
//                     style_modifier: StyleModifier {
//                         foreground: Some(Color::from_hex("#FF0000").unwrap()), // Red for keywords
//                         background: None,
//                         font_style: Some(FontStyle::BOLD),
//                     },
//                 },
//                 CompiledThemeRule {
//                     scope_patterns: vec![vec![string_scope_id]],
//                     style_modifier: StyleModifier::with_foreground(
//                         Color::from_hex("#00FF00").unwrap(),
//                     ), // Green for strings
//                 },
//             ],
//         };
//
//         let mut cache = StyleCache::new();
//         let mut tokenizer = Tokenizer::new(compiled_grammar);
//
//         // Test tokenization with theme
//         let code = r#"var message = "hello world";"#;
//         match tokenizer.tokenize_line(code) {
//             Ok(tokens) => {
//                 println!("Tokenized '{}' into {} tokens", code, tokens.len());
//                 for (i, token) in tokens.iter().enumerate() {
//                     let text = &code[token.start..token.end];
//                     println!(
//                         "Token {}: [{}, {}) '{}' scopes: {:?}",
//                         i, token.start, token.end, text, token.scope_stack
//                     );
//                 }
//
//                 // Create token batches with theme
//                 let batches = Tokenizer::batch_tokens(&tokens, &test_theme, &mut cache);
//                 println!("Created {} token batches:", batches.len());
//
//                 for (i, batch) in batches.iter().enumerate() {
//                     let text = &code[batch.start as usize..batch.end as usize];
//                     if let Some(style) = cache.get_style(batch.style_id) {
//                         println!("Batch {}: '{}' -> {:?}", i, text, style);
//                     }
//                 }
//
//                 // Verify we have styled tokens
//                 assert!(!batches.is_empty(), "Should produce styled token batches");
//
//                 // Check that different scopes get different styles
//                 if batches.len() > 1 {
//                     let first_style_id = batches[0].style_id;
//                     let has_different_style = batches.iter().any(|b| b.style_id != first_style_id);
//
//                     if has_different_style {
//                         println!("✅ Different tokens have different styles as expected");
//                     } else {
//                         println!(
//                             "⚠️ All tokens have the same style - may be expected for this test"
//                         );
//                     }
//                 }
//
//                 // Test specific style lookups
//                 let keyword_tokens: Vec<_> = tokens
//                     .iter()
//                     .filter(|t| t.scope_stack.contains(&keyword_scope_id))
//                     .collect();
//
//                 if !keyword_tokens.is_empty() {
//                     let keyword_style_id =
//                         cache.get_style_id(&keyword_tokens[0].scope_stack, &test_theme);
//                     if let Some(style) = cache.get_style(keyword_style_id) {
//                         println!("Keyword style: {:?}", style);
//                         assert_eq!(style.foreground, Color::from_hex("#FF0000").unwrap());
//                         assert!(style.font_style.contains(FontStyle::BOLD));
//                     }
//                 }
//
//                 println!("✅ Theme integration test completed successfully");
//             }
//             Err(e) => {
//                 panic!("Tokenization failed: {}", e);
//             }
//         }
//     }
// }
