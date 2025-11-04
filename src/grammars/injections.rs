//! TextMate grammar injection selector parsing and matching.

use std::sync::LazyLock;

use crate::scope::Scope;
use onig::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InjectionPrecedence {
    Left,  // L: prefix
    Right, // R: prefix
}

/// A compiled injection selector matcher with priority
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompiledInjectionMatcher {
    matcher: SelectorMatcher,
    priority: Option<InjectionPrecedence>,
}

/// Serializable selector matcher that can evaluate against scope stacks
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SelectorMatcher {
    Scope(Scope),
    /// All matchers must succeed (space-separated)
    And(Vec<SelectorMatcher>),
    /// Any matcher can succeed (OR operation `|` or `,` separated)
    Or(Vec<SelectorMatcher>),
    /// Matcher must NOT succeed (NOT operation `-` prefix)
    Not(Box<SelectorMatcher>),
}

/// Regex for tokenizing injection selectors (matches vscode-textmate exactly except for \* added)
static TOKEN_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"([LR]:|[\w.:]+[\w\*.:\-]*|[,|\-()])?").expect("Invalid selector regex")
});

fn is_identifier(s: &str) -> bool {
    if s.is_empty() || s == "-" {
        return false;
    }

    s.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == ':' || c == '-' || c == '*'
    })
}

fn parse_inner_expression(tokens: &[&str], position: &mut usize) -> SelectorMatcher {
    let mut out = Vec::new();
    while let Some(m) = parse_conjunction(tokens, position) {
        out.push(m);
        if *position < tokens.len() && matches!(tokens[*position], "|" | ",") {
            *position += 1;
        } else {
            break;
        }
    }

    let mut deduplicated = Vec::new();
    for matcher in out {
        if !deduplicated.contains(&matcher) {
            deduplicated.push(matcher);
        }
    }

    if deduplicated.len() == 1 {
        deduplicated.pop().unwrap()
    } else {
        SelectorMatcher::Or(deduplicated)
    }
}

fn parse_operand(tokens: &[&str], position: &mut usize) -> Option<SelectorMatcher> {
    if *position >= tokens.len() {
        return None;
    }

    match tokens[*position] {
        "-" => {
            *position += 1;
            let negated = parse_operand(tokens, position)?;
            Some(SelectorMatcher::Not(Box::new(negated)))
        }
        "(" => {
            *position += 1;
            let inner = parse_inner_expression(tokens, position);
            if *position < tokens.len() && tokens[*position] == ")" {
                *position += 1;
            }
            Some(inner)
        }
        _ => {
            let mut scopes = vec![];

            while *position < tokens.len() && is_identifier(tokens[*position]) {
                let token = tokens[*position];
                let scope = if let Some(pos) = token.find(".*") {
                    let base = &token[..pos];
                    Scope::new(base.trim_end_matches("."))[0]
                } else {
                    Scope::new(token)[0]
                };

                if !scopes.contains(&scope) {
                    scopes.push(scope);
                }
                *position += 1;
            }

            match scopes.len() {
                0 => None,
                1 => Some(SelectorMatcher::Scope(scopes.pop().unwrap())),
                _ => Some(SelectorMatcher::And(
                    scopes.into_iter().map(SelectorMatcher::Scope).collect(),
                )),
            }
        }
    }
}

fn parse_conjunction(tokens: &[&str], position: &mut usize) -> Option<SelectorMatcher> {
    let mut matchers = Vec::new();

    while let Some(m) = parse_operand(tokens, position) {
        matchers.push(m);
    }

    match matchers.len() {
        0 => None,
        1 => Some(matchers.pop().unwrap()),
        _ => Some(SelectorMatcher::And(matchers)),
    }
}

/// Parse injection selector string into compiled matchers
pub fn parse_injection_selector(selector: &str) -> Vec<CompiledInjectionMatcher> {
    let selector = selector.trim();
    if selector.is_empty() {
        return Vec::new();
    }

    let tokens: Vec<_> = TOKEN_REGEX
        .find_iter(selector)
        .map(|(start, end)| &selector[start..end])
        .collect();
    let mut position = 0;
    let mut res = Vec::new();

    let mut priority = None;
    while position < tokens.len() {
        let token = tokens[position];

        match token {
            "L:" => {
                priority = Some(InjectionPrecedence::Left);
                position += 1;
                continue;
            }
            "R:" => {
                priority = Some(InjectionPrecedence::Right);
                position += 1;
                continue;
            }
            _ => (),
        };

        if let Some(matcher) = parse_conjunction(&tokens, &mut position) {
            res.push(CompiledInjectionMatcher { matcher, priority });
            priority = None;
            if position < tokens.len() && tokens[position] == "," {
                position += 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::{assert_debug_snapshot, with_settings};

    #[test]
    fn test_parse_injection_selector_snapshots() {
        let test_cases = vec![
            // Simple patterns
            "L:text.html.markdown", // mermaid.json
            "L:text.html -comment", // angular-template.json
            "text.html",
            "L:meta.decorator.ts -comment -text.html", // angular-inline-template.json
            // Complex negations
            "L:text.pug -comment -string.comment, L:text.html.derivative -comment.block", // vue-interpolations.json
            "L:meta.tag -meta.attribute -meta.ng-binding -entity.name.tag.pug", // vue-directives.json
            // Parenthesized groups with OR operators
            "L:(meta.script.svelte | meta.style.svelte) (meta.lang.js | meta.lang.javascript) - (meta source)", // svelte.json
            "L:(source.ts, source.js, source.coffee)", // svelte.json
            // Specific contexts
            "L:meta.script.svelte - meta.lang - (meta source)", // svelte.json
            "L:(meta.script.astro) (meta.lang.json) - (meta source)", // astro.json
            "text.html.php.blade - (meta.embedded | meta.tag | comment.block.blade), L:(text.html.php.blade meta.tag - (comment.block.blade | meta.embedded.block.blade)), L:(source.js.embedded.html - (comment.block.blade | meta.embedded.block.blade))", // blade.json
            "R:text.html - (comment.block, text.html meta.embedded, meta.tag.*.*.html, meta.tag.*.*.*.html, meta.tag.*.*.*.*.html)",
            "L:source.css -comment, L:source.postcss -comment, L:source.sass -comment, L:source.stylus -comment",
        ];

        for (i, test_case) in test_cases.into_iter().enumerate() {
            let result = parse_injection_selector(test_case);
            with_settings!({description => test_case}, {
                assert_debug_snapshot!(format!("injection_{i}"), result);
            })
        }
    }

    #[test]
    fn can_parse_all_kinds_of_injections() {}
}
