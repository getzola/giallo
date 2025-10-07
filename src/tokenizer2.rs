use crate::grammars::ScopeId;

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
    fn new(grammar_scope_id: ScopeId) -> Self {
        Self {
            stack: StateStack {
                parent: None,
                rule_id: RuleId(0),
                name_scopes: vec![grammar_scope_id],
                content_scopes: vec![grammar_scope_id],
                end_rule: None,
                begin_captures: Vec::new(),
            },
        }
    }
}

