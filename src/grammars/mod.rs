mod common;
mod compiled;
mod raw;

// Import the generated scope mappings
include!("../generated/scopes.rs");

pub use compiled::{CompiledGrammar, CompiledPattern};
pub use raw::RawGrammar;
