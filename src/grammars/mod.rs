mod compiled;
mod raw;
mod regex;

// Import the generated scope mappings
include!("../generated/scopes.rs");

pub use raw::RawGrammar;
