mod compiled;
mod pattern_set;
mod raw;
mod regex;

// Import the generated scope mappings
include!("../generated/scopes.rs");

pub use compiled::*;
pub use raw::RawGrammar;
pub use regex::Regex;

/// Convert a ScopeId back to its original scope name string.
///
/// This function performs a reverse lookup in the SCOPE_MAP to find the string
/// representation of a ScopeId. Note that this is O(n) in the number of scopes,
/// so it's best used sparingly (e.g., for debugging or final output generation).
///
/// # Arguments
/// * `scope_id` - The ScopeId to convert to a string
///
/// # Returns
/// The scope name string, or a fallback string if the ID is not found
pub fn scope_id_to_name(scope_id: ScopeId) -> String {
    // Search through the PHF map to find the name for this ID
    for (name, &id) in SCOPE_MAP.entries() {
        if ScopeId(id) == scope_id {
            return name.to_string();
        }
    }
    // Fallback for unknown scope IDs (should rarely happen in practice)
    format!("unknown.scope.{}", scope_id.0)
}
