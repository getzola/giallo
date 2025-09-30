mod common;
mod compiled;
mod raw;

// Import the generated scope mappings
include!("../../generated/scopes.rs");

// Re-export all public types to maintain API compatibility
pub use common::{CompileError, Regex};
pub use compiled::{
    CompiledBeginEndPattern, CompiledBeginWhilePattern, CompiledCapture, CompiledGrammar,
    CompiledIncludePattern, CompiledMatchPattern, CompiledPattern,
};
pub use raw::{
    BeginEndPattern, BeginWhilePattern, Capture, IncludePattern, MatchPattern, Pattern, RawGrammar,
    RepositoryEntry, RepositoryPattern,
};

// ScopeId is already available from the included generated code
