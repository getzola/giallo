//! A lot of grammars are creating a lot of capturing groups and then not using them at all
//! This might have not been an issue with onig but with fancy-regex it's a pretty big issue
//! for the JS family in particular. Not really impactful elsewhere but pruning gives
//! a good 10-15% speedup for the JS family benches both cold and warm.
//! The JS grammar has a pattern with 410 captures and only 5 are used,
//! TS even has one with 555 for the same 5 used!
//!
//! It should probably be fixed upstream but the repo does not seem maintained.
//! This rewrites the captures that are not used to not actually be captures.
//! Captures are either referenced via the captures list, `$N` in scope names or `\N` backref
//! in end/while pattern after a `begin` pattern.

use fancy_regex::Expr;
use fancy_regex::internal::FLAG_ONIGURUMA_MODE;

use crate::grammars::compiled::CAPTURING_NAME_RE;
use crate::grammars::raw::RawRule;

pub(crate) fn prune_rule(rule: &mut RawRule) {
    let tables = [
        &rule.captures,
        &rule.begin_captures,
        &rule.end_captures,
        &rule.while_captures,
    ];
    let mut highest_ref = find_highest_group_ref(rule);

    for table in tables {
        for (index, capture) in table.iter() {
            highest_ref = highest_ref.max(*index).max(find_highest_group_ref(capture));
        }
    }

    for pattern in rule.end.iter().chain(&rule.while_) {
        highest_ref = highest_ref.max(find_highest_backref(pattern));
    }

    let patterns = [
        &mut rule.match_,
        &mut rule.begin,
        &mut rule.end,
        &mut rule.while_,
    ];

    for pattern in patterns.into_iter().flatten() {
        if let Some(pruned) = prune(pattern, highest_ref) {
            *pattern = pruned;
        }
    }
}

/// Highest group reference (`$N` or `${N:/command}`) used in a pattern
fn find_highest_group_ref(rule: &RawRule) -> usize {
    let mut highest = 0;

    for name in rule.name.iter().chain(&rule.content_name) {
        for caps in CAPTURING_NAME_RE.captures_iter(name).flatten() {
            if let Some(group) = caps.get(1).or_else(|| caps.get(2)) {
                let v = group.as_str().parse::<usize>().unwrap_or(0);
                highest = std::cmp::max(highest, v);
            }
        }
    }

    highest
}

/// Highest numbered backref in end/while pattern that references the begin rule
fn find_highest_backref(pattern: &str) -> usize {
    let mut highest = 0;

    for rest in pattern.split('\\').skip(1) {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if let Some(num) = rest[..digits].parse().ok() {
            highest = std::cmp::max(highest, num);
        }
    }

    highest
}

/// Returns `pattern` with every capture group numbered above `highest_ref` made
/// non-capturing, or `None` when nothing can be removed safely.
fn prune(pattern: &str, highest_ref: usize) -> Option<String> {
    let positions = group_positions(pattern)?;
    if positions.len() <= highest_ref {
        return None;
    }

    let tree = Expr::parse_tree_with_flags(pattern, FLAG_ONIGURUMA_MODE).ok()?;
    // `group_positions` does not handle every single case, only stuff we've seen in the grammars
    // if there is a new group type we didn't count, everything will error so we can handle it
    debug_assert_eq!(positions.len(), count_groups(&tree.expr));
    if refers_to_groups(&tree.expr) {
        return None;
    }

    let mut rewritten = pattern.to_owned();
    for &open in positions[highest_ref..].iter().rev() {
        rewritten.insert_str(open + 1, "?:");
    }
    Some(rewritten)
}

fn count_groups(expr: &Expr) -> usize {
    let own = usize::from(matches!(expr, Expr::Group(_)));
    own + expr.children_iter().map(count_groups).sum::<usize>()
}

/// If the tree refers to a group, we will just skip pruning this pattern
fn refers_to_groups(expr: &Expr) -> bool {
    let is_ref = |e: &Expr| {
        matches!(
            e,
            Expr::Backref { .. }
                | Expr::BackrefWithRelativeRecursionLevel { .. }
                | Expr::BackrefExistsCondition { .. }
                | Expr::SubroutineCall(_)
        )
    };
    is_ref(expr) || expr.has_descendant(is_ref)
}

/// If we find any group where we are not sure whether it's a group,
/// we just skip the whole pruning to avoid issues.
fn group_positions(pattern: &str) -> Option<Vec<usize>> {
    let bytes = pattern.as_bytes();
    let mut positions = Vec::new();
    let mut depth = 0usize;
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            b'[' => {
                depth += 1;
                // A `]` right after `[` or `[^` is a literal
                if bytes.get(i + 1) == Some(&b'^') {
                    i += 1;
                }
                if bytes.get(i + 1) == Some(&b']') {
                    i += 1;
                }
            }
            b']' => depth = depth.saturating_sub(1),
            b'(' if depth == 0 => match &bytes[i + 1..] {
                // (?<name> is ok but not the lookbehinds (?<= and (?<!
                [b'?', b'<', c, ..] if !matches!(c, b'=' | b'!') => return None,
                [b'?', ..] => {}
                _ => positions.push(i),
            },
            _ => {}
        }
        i += 1;
    }
    Some(positions)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    #[test]
    fn can_prune_groups_above_highest_ref() {
        for (pattern, highest_ref, expected) in [
            ("(a)(b)(c)", 1, Some("(a)(?:b)(?:c)")),
            ("(a)(b)(c)", 3, None),
            ("((a)(b))", 2, Some("((a)(?:b))")),
            // still works with escapes
            (
                r"\([()][]()][^]\[(][[:alpha:]()][a-z&&[^(]]](?:x)(?<=y)(?i)(a)",
                0,
                Some(r"\([()][]()][^]\[(][[:alpha:]()][a-z&&[^(]]](?:x)(?<=y)(?i)(?:a)"),
            ),
            // group references and named groups are left alone
            (r"(a)\1(b)", 0, None),
            (r"(a)\g<1>(b)", 0, None),
            (r"(?<word>a)(b)", 0, None),
        ] {
            assert_eq!(
                prune(pattern, highest_ref).as_deref(),
                expected
            );
        }
    }
    
    #[test]
    fn prune_rule_keeps_groups_the_rule_references() {
        let prune = |value: Value| {
            let mut rule: RawRule = serde_json::from_value(value).unwrap();
            prune_rule(&mut rule);
            rule
        };

        let r = prune(json!({
            "match": "(a)(b)(c)(d)",
            "captures": {"1": {"name": "x.$2", "patterns": [{"match": "(y)(z)"}]}},
            "name": "x.${3:/downcase}"
        }));
        assert_eq!(r.match_.as_deref(), Some("(a)(b)(c)(?:d)"));

        let r = prune(json!({
            "begin": "(a)(b)(c)(d)", "end": "\\2(x)(y)", "while": "(p)(q)(r)",
            "endCaptures": {"1": {"name": "e"}}
        }));
        assert_eq!(r.begin.as_deref(), Some("(a)(b)(?:c)(?:d)"));
        assert_eq!(
            r.end.as_deref(),
            Some(r"\2(x)(y)"),
            "a `\\N` pattern is left alone"
        );
        assert_eq!(r.while_.as_deref(), Some("(p)(q)(?:r)"));
    }
}
