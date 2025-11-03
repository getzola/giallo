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

    /// Checks if this selector matches the given scope stack.
    ///
    /// This implements the VSCode-TextMate scope matching algorithm:
    /// 1. The target scope must match the innermost scope (last in stack)
    /// 2. All parent scope requirements must be satisfied walking up the stack
    /// 3. `Parent::Anywhere` can skip intermediate scopes
    /// 4. `Parent::Direct` requires immediate parent relationship
    ///
    /// # Arguments
    /// * `scope_stack` - Scopes from outermost to innermost (e.g., `[source.js, meta.function, string.quoted]`)
    ///
    /// # Returns
    /// `true` if all selector requirements are satisfied, `false` otherwise
    ///
    /// # Examples
    /// ```ignore
    /// let selector = parse_selector("comment").unwrap();
    /// let stack = vec![scope("source.js"), scope("comment.line")];
    /// assert!(selector.matches(&stack));
    ///
    /// let selector = parse_selector("source.js string").unwrap();
    /// let stack = vec![scope("source.js"), scope("meta.function"), scope("string.quoted")];
    /// assert!(selector.matches(&stack)); // "string" matches "string.quoted", source.js is ancestor
    /// ```
    pub fn matches(&self, scope_stack: &[Scope]) -> bool {
        // Empty stack cannot match any selector
        if scope_stack.is_empty() {
            return false;
        }

        // Check if target scope matches the innermost scope (last in stack)
        let innermost_scope = scope_stack[scope_stack.len() - 1];
        if !self.target_scope.is_prefix_of(innermost_scope) {
            return false;
        }

        // If no parent requirements, we're done
        if self.parent_scopes.is_empty() {
            return true;
        }

        // Check parent scope requirements using slice-based approach
        // Work with parent scopes only (everything except the target scope)
        let mut remaining_parents = &scope_stack[..scope_stack.len() - 1];

        // parent_scopes are stored deepest-first, so iterate in order
        for (parent_idx, required_parent) in self.parent_scopes.iter().enumerate() {
            let is_last_parent = parent_idx == self.parent_scopes.len() - 1;

            match required_parent {
                Parent::Direct(parent_scope) => {
                    // Direct parent must match the last remaining parent
                    match remaining_parents.last() {
                        Some(&stack_scope) if parent_scope.is_prefix_of(stack_scope) => {
                            // Consume this parent by removing it from the end
                            remaining_parents = &remaining_parents[..remaining_parents.len() - 1];
                        }
                        _ => return false, // No match or no more parents
                    }

                    // Check if we have more requirements but no more parents
                    if remaining_parents.is_empty() && !is_last_parent {
                        return false;
                    }
                }
                Parent::Anywhere(parent_scope) => {
                    // Find this parent anywhere in remaining parents (from end to start)
                    match remaining_parents
                        .iter()
                        .rposition(|&scope| parent_scope.is_prefix_of(scope))
                    {
                        Some(pos) => {
                            // Consume all parents up to and including this match
                            remaining_parents = &remaining_parents[..pos];
                        }
                        None => return false, // Required parent not found
                    }

                    // Check if we have more requirements but no more parents
                    if remaining_parents.is_empty() && !is_last_parent {
                        return false;
                    }
                }
            }
        }

        true
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

    fn create_scope_stack(scope_names: &[&str]) -> Vec<Scope> {
        scope_names
            .iter()
            .map(|name| Scope::new(name).into_iter().next().unwrap())
            .collect()
    }

    #[test]
    fn test_selector_matches() {
        let test_cases = vec![
            // (selector_string, scope_stack, expected_match)

            // Simple selector tests
            ("comment", vec!["source.js", "comment.line"], true),
            ("comment", vec!["source.js", "string.quoted"], false),
            ("comment", vec!["comment"], true),
            (
                "comment.line",
                vec!["source.js", "comment.line.double-slash"],
                true,
            ),
            // Parent selector tests (Anywhere)
            (
                "source.js string",
                vec!["source.js", "meta.function", "string.quoted"],
                true,
            ),
            ("source.js string", vec!["source.js", "string.quoted"], true),
            (
                "source.js string",
                vec!["source.py", "string.quoted"],
                false,
            ),
            (
                "source string",
                vec!["source.js", "meta.function", "string.quoted"],
                true,
            ),
            (
                "meta.function string",
                vec!["source.js", "meta.function.arrow", "string.quoted"],
                true,
            ),
            // Child combinator tests (Direct)
            (
                "meta.function > string",
                vec!["source.js", "meta.function", "string.quoted"],
                true,
            ),
            (
                "meta.function > string",
                vec!["source.js", "meta.function", "punctuation", "string.quoted"],
                false,
            ),
            (
                "meta > string",
                vec!["source.js", "meta.function", "string.quoted"],
                true,
            ),
            // Mixed combinator tests
            (
                "source.js meta.function > string.quoted",
                vec!["source.js", "meta.function", "string.quoted"],
                true,
            ),
            (
                "source.js meta.function > string.quoted",
                vec!["source.js", "meta.function", "punctuation", "string.quoted"],
                false,
            ),
            (
                "source.js meta.function > string.quoted",
                vec!["source.py", "meta.function", "string.quoted"],
                false,
            ),
            // Multiple direct parents
            (
                "source > meta > string",
                vec!["source.js", "meta.function", "string.quoted"],
                true,
            ),
            (
                "source > meta > string",
                vec!["source.js", "punctuation", "meta.function", "string.quoted"],
                false,
            ),
            // Edge cases
            ("comment", vec![], false),                      // empty stack
            ("source.js comment", vec!["source.js"], false), // target not found
            ("meta.function > string", vec!["meta.function"], false), // no target found
            // Complex nesting
            (
                "source.js meta string",
                vec![
                    "source.js",
                    "meta.function",
                    "meta.parameter",
                    "string.quoted",
                ],
                true,
            ),
            (
                "source.js meta.function string",
                vec!["source.js", "meta.class", "meta.function", "string.quoted"],
                true,
            ),
        ];

        for (selector_str, scope_names, expected) in test_cases {
            let selector = parse_selector(selector_str)
                .unwrap_or_else(|| panic!("Failed to parse selector: '{}'", selector_str));
            let scope_stack = create_scope_stack(&scope_names);
            let result = selector.matches(&scope_stack);

            assert_eq!(
                result, expected,
                "Selector '{}' matching scope stack {:?}: expected {}, got {}",
                selector_str, scope_names, expected, result
            );
        }
    }
}
