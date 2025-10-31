use crate::scope::Scope;
use serde::{Deserialize, Serialize};

/// Represents a parent scope requirement in a theme selector.
///
/// # Examples
/// - `Anywhere(source.js)` from "source.js meta.function" - can have scopes between
/// - `Direct(meta.function)` from "meta.function > string" - must be immediate parent
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Parent {
    /// Parent scope that can appear anywhere up the scope stack
    Anywhere(Scope),
    /// Parent scope that must be the immediate parent (child combinator `>`)
    Direct(Scope),
}

/// A parsed theme selector that matches against scope stacks.
///
/// Theme selectors can be:
/// - Simple: `"comment"`
/// - With parents: `"source.js meta.function string"`
/// - With child combinator: `"meta.function > string"`
/// - Mixed: `"source.js meta.function > string.quoted"`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeSelector {
    /// The target scope to match (rightmost in the selector string)
    pub target_scope: Scope,
    /// Required parent scopes from deepest to shallowest (right to left from selector string)
    /// This matches VSCode-TextMate's reversed storage for efficient matching.
    pub parent_scopes: Vec<Parent>,
}

impl ThemeSelector {
    /// Creates a new theme selector.
    pub fn new(target_scope: Scope, parent_scopes: Vec<Parent>) -> Self {
        Self {
            target_scope,
            parent_scopes,
        }
    }
}

/// Parses a theme selector string into a structured ThemeSelector.
///
/// # Examples
/// ```ignore
/// let selector = parse_selector("comment").unwrap();
/// assert_eq!(selector.parent_scopes.len(), 0);
///
/// let selector = parse_selector("source.js meta.function string").unwrap();
/// assert_eq!(selector.parent_scopes.len(), 2);
///
/// let selector = parse_selector("meta.function > string").unwrap();
/// assert!(matches!(selector.parent_scopes[0], Parent::Direct(_)));
/// ```
///
/// # Selector Format
/// - Scopes are separated by whitespace: `"source.js meta.function string"`
/// - Child combinator `>` creates direct parent requirement: `"parent > child"`
/// - Target scope is always the rightmost non-`>` token
/// - Parent scopes are processed left to right
///
/// # Returns
/// - `Some(ThemeSelector)` if parsing succeeds
/// - `None` if the selector string is invalid or empty
pub fn parse_selector(input: &str) -> Option<ThemeSelector> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }

    let parts: Vec<&str> = input.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }

    // Find the target scope (rightmost non-">" token)
    let target_str = parts.iter().rev().find(|&&s| s != ">")?;
    let target_scopes = Scope::new(target_str);
    let target_scope = target_scopes.into_iter().next()?;

    // Build parent chain from left to right, handling ">" combinators
    let mut parents = Vec::new();
    let mut i = 0;

    while i < parts.len() {
        let part = parts[i];

        // Skip ">" tokens
        if part == ">" {
            i += 1;
            continue;
        }

        // Skip the target scope (we already processed it)
        if part == *target_str {
            break;
        }

        // Parse the parent scope
        let parent_scopes = Scope::new(part);
        let parent_scope = parent_scopes.into_iter().next()?;

        // Check if this parent has a direct relationship (followed by ">")
        let is_direct = i + 1 < parts.len() && parts[i + 1] == ">";

        parents.push(if is_direct {
            Parent::Direct(parent_scope)
        } else {
            Parent::Anywhere(parent_scope)
        });

        i += 1;
    }

    // Reverse to match VSCode-TextMate ordering: deepest parent first
    parents.reverse();
    Some(ThemeSelector::new(target_scope, parents))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_selector() {
        let test_cases = vec![
            (
                "comment",
                ThemeSelector {
                    target_scope: Scope::new("comment").into_iter().next().unwrap(),
                    parent_scopes: vec![],
                },
            ),
            (
                "source.js meta.function string",
                ThemeSelector {
                    target_scope: Scope::new("string").into_iter().next().unwrap(),
                    parent_scopes: vec![
                        Parent::Anywhere(Scope::new("meta.function").into_iter().next().unwrap()), // deepest parent
                        Parent::Anywhere(Scope::new("source.js").into_iter().next().unwrap()), // shallowest parent
                    ],
                },
            ),
            (
                "meta.function > string",
                ThemeSelector {
                    target_scope: Scope::new("string").into_iter().next().unwrap(),
                    parent_scopes: vec![Parent::Direct(
                        Scope::new("meta.function").into_iter().next().unwrap(),
                    )],
                },
            ),
            (
                "source.js meta.function > string.quoted",
                ThemeSelector {
                    target_scope: Scope::new("string.quoted").into_iter().next().unwrap(),
                    parent_scopes: vec![
                        Parent::Direct(Scope::new("meta.function").into_iter().next().unwrap()), // deepest parent
                        Parent::Anywhere(Scope::new("source.js").into_iter().next().unwrap()), // shallowest parent
                    ],
                },
            ),
            (
                "source > meta > string",
                ThemeSelector {
                    target_scope: Scope::new("string").into_iter().next().unwrap(),
                    parent_scopes: vec![
                        Parent::Direct(Scope::new("meta").into_iter().next().unwrap()), // deepest parent
                        Parent::Direct(Scope::new("source").into_iter().next().unwrap()), // shallowest parent
                    ],
                },
            ),
            (
                "  source.js   meta.function  >   string  ",
                ThemeSelector {
                    target_scope: Scope::new("string").into_iter().next().unwrap(),
                    parent_scopes: vec![
                        Parent::Direct(Scope::new("meta.function").into_iter().next().unwrap()), // deepest parent
                        Parent::Anywhere(Scope::new("source.js").into_iter().next().unwrap()), // shallowest parent
                    ],
                },
            ),
        ];

        for (input, expected) in test_cases {
            let result = parse_selector(input).unwrap();
            assert_eq!(result, expected, "Mismatch for input: '{}'", input);
        }
    }
}
