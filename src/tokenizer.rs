use crate::grammars::{CompiledGrammar, CompiledPattern, ScopeId};

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

/// State for an active BeginEnd or BeginWhile pattern that's waiting for its end match
///
/// Critical for proper scope management: When we encounter a BeginEnd pattern like
/// string literals or block comments, we need to track the active pattern state
/// until we find the matching end pattern. This prevents scope stack corruption.
///
/// Example: For a string "hello world", we push string scopes on the opening quote,
/// maintain them for the content, then pop them on the closing quote.
#[derive(Debug, Clone)]
struct ActivePattern<'g> {
    /// The BeginEnd/BeginWhile pattern that was matched (cloned for end pattern matching)
    pattern: &'g CompiledPattern,
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

/// Result of a successful pattern match against text
///
/// When a regex pattern matches against input text, we create one of these to track
/// the match details, which pattern matched, and any capture groups that were found.
#[derive(Debug, Clone)]
struct PatternMatch<'g> {
    /// Start position of the match in the input text
    start: usize,
    /// End position of the match in the input text
    end: usize,
    /// Direct reference to the pattern that matched (Match, BeginEnd, BeginWhile)
    pattern: &'g CompiledPattern,
    /// Path through include chain - helps debug complex nested grammars
    context_path: Vec<usize>,
    /// Capture groups and their scopes: (start, end, scope_id)
    /// Example: For regex "(\w+)", captures would contain the word match with its scope
    captures: Vec<(usize, usize, ScopeId)>,
}


#[derive(Debug)]
pub struct Tokenizer<'g> {
    /// Reference to the compiled grammar to use for tokenization
    grammar: &'g CompiledGrammar,
    /// Current stack of active scopes - grows as patterns nest, shrinks as they end
    /// Example: ["source.js"] -> ["source.js", "string.quoted"] -> ["source.js"]
    scope_stack: Vec<ScopeId>,
    /// Stack of active patterns (for BeginEnd/BeginWhile patterns waiting for their end)
    /// Example: After matching opening quote, we track the string pattern until closing quote
    active_patterns: Vec<ActivePattern<'g>>,
    /// Current line being processed (for debugging and error reporting)
    current_line: usize,
}
impl<'g> Tokenizer<'g> {
    pub fn new(grammar: &'g CompiledGrammar) -> Self {
        let scope_stack = vec![grammar.scope_id];

        Self {
            grammar,
            scope_stack,
            current_line: 0,
            active_patterns: Vec::new(),
        }
    }

    pub fn tokenize_line(&mut self, text: &str) -> Result<Vec<Token>, TokenizeError> {
        self.current_line += 1;
        let mut tokens = Vec::new();
        let mut position = 0;

        while position < text.len() {
            break;
        }

        Ok(tokens)
    }

    fn find_next_match(&self, text: &str, start: usize) -> Result<Option<Token>, TokenizeError> {
        let text = text.get(start..).unwrap_or("");
        if text.is_empty() {
            return Ok(None);
        }

        let mut best_match: Option<PatternMatch> = None;

        todo!()
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