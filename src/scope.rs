//! Ultra-fast scope system using u128 bit-packing
//!
//! Scopes like "source.rust.meta.function" are packed into a single u128:
//! Memory layout: [atom0][atom1][atom2][atom3][atom4][atom5][atom6][atom7]
//! Each atom is 16 bits, storing repository_index + 1 (0 = unused slot)

use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, MutexGuard};

pub const MAX_ATOMS: usize = 8;
pub const MAX_REPOSITORY_SIZE: usize = 65534; // 2^16 - 2, leaving room for 0 and max

/// A scope represents a hierarchical position in source code like "source.rust.meta.function"
/// Internally stored as a single u128 with up to 8 atoms packed as 16-bit indices
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Copy, Default, Hash)]
pub struct Scope {
    /// Packed atoms in MSB-first order for lexicographic comparison
    atoms: u128,
}

impl Scope {
    /// Create a new scope from a dot-separated string, truncating to 8 atoms if longer
    pub fn new(s: &str) -> Scope {
        let mut repo = lock_global_scope_repo();
        repo.build(s.trim())
    }

    /// Extract a single atom at the given index (0-7)
    /// Returns 0 for unused slots, or repository_index + 1 for valid atoms
    #[inline]
    pub fn atom_at(self, index: usize) -> u16 {
        debug_assert!(index < MAX_ATOMS);
        // MSB-first layout: index 0 is in bits [127:112], index 1 in [111:96], etc.
        let shift = (MAX_ATOMS - 1 - index) * 16;
        ((self.atoms >> shift) & 0xFFFF) as u16
    }

    /// Count the number of atoms in this scope using trailing zero optimization
    #[inline]
    pub fn len(self) -> u32 {
        MAX_ATOMS as u32 - self.missing_atoms()
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        self.atoms == 0
    }

    /// Count unused slots by finding trailing zeros (LSB side has unused atoms)
    /// Since atoms are packed MSB-first, unused slots create trailing zeros
    #[inline]
    fn missing_atoms(self) -> u32 {
        self.atoms.trailing_zeros() / 16
    }

    /// Check if this scope is a prefix of another scope using bitwise masking
    /// This is the core operation for theme selector matching - must be O(1)
    #[inline]
    pub fn is_prefix_of(self, other: Scope) -> bool {
        let missing = self.missing_atoms();

        if missing == MAX_ATOMS as u32 {
            return true; // Empty scope is prefix of everything
        }

        // Create a mask that covers only the prefix portion
        // For a 2-atom prefix (missing=6), mask covers top 32 bits (2 * 16)
        let mask_shift = missing * 16;
        let mask = if mask_shift >= 128 {
            0u128 // Would shift entire value away
        } else {
            u128::MAX << mask_shift
        };

        // XOR finds differing bits, mask isolates the prefix we care about
        (self.atoms ^ other.atoms) & mask == 0
    }

    /// Convert back to string form - expensive, only use for debugging/display
    pub fn build_string(self) -> String {
        let repo = lock_global_scope_repo();
        repo.to_string(self)
    }
}

impl fmt::Debug for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Scope(\"{}\")", self.build_string())
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.build_string())
    }
}

/// Global repository that maps atom strings to indices for deduplication
struct ScopeRepository {
    atoms: Vec<String>,                     // Index-to-string mapping
    atom_index_map: HashMap<String, usize>, // String-to-index for fast lookup
}

impl ScopeRepository {
    fn new() -> Self {
        Self {
            atoms: Vec::new(),
            atom_index_map: HashMap::new(),
        }
    }

    /// Get existing index or register new atom, returning repository index
    fn atom_to_index(&mut self, atom: &str) -> usize {
        // Fast path: atom already registered
        if let Some(&index) = self.atom_index_map.get(atom) {
            return index;
        }

        if self.atoms.len() >= MAX_REPOSITORY_SIZE {
            panic!(
                "Too many atoms in repository: exceeded MAX_REPOSITORY_SIZE of {}",
                MAX_REPOSITORY_SIZE
            );
        }

        // Slow path: register new atom
        let index = self.atoms.len();
        self.atoms.push(atom.to_owned());
        self.atom_index_map.insert(atom.to_owned(), index);
        index
    }

    /// Convert atom number back to string (atom_number is repository_index + 1)
    fn atom_str(&self, atom_number: u16) -> &str {
        debug_assert!(atom_number > 0);
        &self.atoms[(atom_number - 1) as usize]
    }

    /// Parse dot-separated string into bit-packed scope, truncating if > 8 atoms
    fn build(&mut self, s: &str) -> Scope {
        if s.is_empty() {
            return Scope::default();
        }

        let parts: Vec<&str> = s.split('.').collect();
        let atoms_to_process = parts.len().min(MAX_ATOMS); // Truncate to 8 atoms
        let mut atoms = 0u128;

        for (i, &atom_str) in parts.iter().take(atoms_to_process).enumerate() {
            if atom_str.is_empty() {
                continue; // Skip empty parts from "a..b"
            }

            let index = self.atom_to_index(atom_str);
            // Store as index + 1 so that 0 can mean "unused slot"
            let atom_value = (index + 1) as u128;

            // Pack MSB-first: first atom goes in highest bits for lexicographic ordering
            let shift = (MAX_ATOMS - 1 - i) * 16;
            atoms |= atom_value << shift;
        }

        Scope { atoms }
    }

    /// Reconstruct string from bit-packed scope
    fn to_string(&self, scope: Scope) -> String {
        let mut parts = Vec::new();

        for i in 0..MAX_ATOMS {
            let atom_number = scope.atom_at(i);
            if atom_number == 0 {
                break; // Hit unused slot
            }
            parts.push(self.atom_str(atom_number));
        }

        parts.join(".")
    }
}

/// Global singleton repository with thread-safe access
static SCOPE_REPO: std::sync::LazyLock<Mutex<ScopeRepository>> =
    std::sync::LazyLock::new(|| Mutex::new(ScopeRepository::new()));

fn lock_global_scope_repo() -> MutexGuard<'static, ScopeRepository> {
    SCOPE_REPO.lock().expect("Failed to lock scope repository")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_scope_creation() {
        let scope = Scope::new("source.rust.meta.function");
        assert_eq!(scope.len(), 4);
        assert_eq!(scope.build_string(), "source.rust.meta.function");
    }

    #[test]
    fn test_empty_scope() {
        let scope = Scope::new("");
        assert_eq!(scope.len(), 0);
        assert!(scope.is_empty());
        assert_eq!(scope.build_string(), "");
    }

    #[test]
    fn test_prefix_matching() {
        let prefix = Scope::new("source.rust");
        let full = Scope::new("source.rust.meta.function");
        let different = Scope::new("source.javascript");

        assert!(prefix.is_prefix_of(full));
        assert!(prefix.is_prefix_of(prefix));
        assert!(!prefix.is_prefix_of(different));
    }

    #[test]
    fn test_atom_truncation() {
        // Scopes with >8 atoms should be truncated to first 8
        let long_scope = Scope::new("a.b.c.d.e.f.g.h.i.j.k.l");
        assert_eq!(long_scope.len(), 8);
        assert_eq!(long_scope.build_string(), "a.b.c.d.e.f.g.h");
    }

    #[test]
    fn test_atom_extraction() {
        let scope = Scope::new("source.rust.meta");

        assert_ne!(scope.atom_at(0), 0); // "source" is present
        assert_ne!(scope.atom_at(1), 0); // "rust" is present
        assert_ne!(scope.atom_at(2), 0); // "meta" is present
        assert_eq!(scope.atom_at(3), 0); // unused slot
        assert_eq!(scope.atom_at(7), 0); // unused slot
    }

    #[test]
    fn test_scope_ordering() {
        let scope1 = Scope::new("source.rust");
        let scope2 = Scope::new("source.rust.meta");

        // Longer scopes should sort after shorter prefixes
        assert!(scope1 < scope2);
    }

    #[test]
    fn test_scope_equality() {
        let scope1 = Scope::new("source.rust.meta");
        let scope2 = Scope::new("source.rust.meta");
        let scope3 = Scope::new("source.rust");

        assert_eq!(scope1, scope2);
        assert_ne!(scope1, scope3);
    }
}
