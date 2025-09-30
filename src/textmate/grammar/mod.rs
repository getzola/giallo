mod common;
mod raw;
mod compiled;

// Import the generated scope mappings
include!("../../generated/scopes.rs");

// Re-export all public types to maintain API compatibility
pub use common::{Regex, CompileError};
pub use raw::{
    RawGrammar, Pattern, Capture, MatchPattern, BeginEndPattern,
    BeginWhilePattern, IncludePattern, RepositoryPattern, RepositoryEntry
};
pub use compiled::{
    CompiledGrammar, CompiledPattern, CompiledCapture, CompiledMatchPattern,
    CompiledBeginEndPattern, CompiledBeginWhilePattern, CompiledIncludePattern
};

// ScopeId is already available from the included generated code