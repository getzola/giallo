use fancy_regex::{Expr, LookAround};
use regex_syntax::hir::{Class, Hir, HirKind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ByteSet([bool; 256]);

impl ByteSet {
    pub(crate) fn new() -> Self {
        Self([false; 256])
    }

    pub(crate) fn insert(&mut self, byte: u8) {
        self.0[byte as usize] = true;
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0 == [false; 256]
    }

    pub(crate) fn contains(&self, byte: u8) -> bool {
        self.0[byte as usize]
    }

    pub(crate) fn union(&mut self, other: &Self) {
        for (i, b) in other.0.iter().enumerate() {
            if *b {
                self.0[i] = true;
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Start {
    /// Every match must start with a byte from the set
    Definite(ByteSet),
    /// The match can be empty but otherwise it starts with a byte from the set
    MaybeEmpty(ByteSet),
    /// Could be anything.
    Bail,
}

/// Same logic as fancy_start but on regex-syntax HIR instead.
fn regex_syntax_start(expr: &Hir) -> Start {
    match expr.kind() {
        // 0 len match
        HirKind::Empty | HirKind::Look(_) => Start::MaybeEmpty(ByteSet::new()),
        HirKind::Literal(l) => {
            if let Some(first) = l.0.first() {
                let mut set = ByteSet::new();
                set.insert(*first);
                Start::Definite(set)
            } else {
                Start::MaybeEmpty(ByteSet::new())
            }
        }
        HirKind::Class(class) => {
            let mut set = ByteSet::new();
            match class {
                Class::Unicode(cls) => {
                    for r in cls.ranges() {
                        if r.start().is_ascii() {
                            for i in (r.start() as u32)..=(r.end() as u32).min(0x7F) {
                                set.insert(i as u8);
                            }
                        }

                        if !r.end().is_ascii() {
                            // unicode, just insert lead bytes range
                            for i in 0xC2..=0xF4 {
                                set.insert(i as u8);
                            }
                        }
                    }
                }
                Class::Bytes(cls) => {
                    // Can we even reach there?
                    for r in cls.ranges() {
                        for i in r.start()..=r.end() {
                            set.insert(i);
                        }
                    }
                }
            }

            if set.is_empty() {
                Start::Bail
            } else {
                Start::Definite(set)
            }
        }
        HirKind::Capture(c) => regex_syntax_start(&c.sub),
        HirKind::Repetition(r) => {
            let child_start = regex_syntax_start(&r.sub);
            if r.min == 0 {
                // if we allow 0 reps then it's not definite
                match child_start {
                    Start::Definite(set) | Start::MaybeEmpty(set) => Start::MaybeEmpty(set),
                    Start::Bail => Start::Bail,
                }
            } else {
                child_start
            }
        }
        HirKind::Concat(c) => {
            let mut set = ByteSet::new();
            for expr in c {
                match regex_syntax_start(expr) {
                    Start::Definite(mut s) => {
                        // We only care about the first byte so as soon as we hit a definite match
                        // we don't need to continue processing the rest
                        s.union(&set);
                        return Start::Definite(s);
                    }
                    Start::MaybeEmpty(s) => {
                        set.union(&s);
                    }
                    Start::Bail => {
                        // If we even get one bail, we bail everything
                        return Start::Bail;
                    }
                }
            }
            Start::MaybeEmpty(set)
        }
        HirKind::Alternation(exprs) => {
            let mut total = None;
            for expr in exprs {
                let expr_start = regex_syntax_start(expr);
                total = Some(match (total, expr_start) {
                    (None, b) => b,
                    (Some(Start::Bail), _) | (Some(_), Start::Bail) => Start::Bail,
                    (Some(Start::Definite(mut a)), Start::Definite(b)) => {
                        a.union(&b);
                        Start::Definite(a)
                    }
                    (Some(Start::Definite(mut a)), Start::MaybeEmpty(b))
                    | (Some(Start::MaybeEmpty(mut a)), Start::Definite(b))
                    | (Some(Start::MaybeEmpty(mut a)), Start::MaybeEmpty(b)) => {
                        a.union(&b);
                        Start::MaybeEmpty(a)
                    }
                });
            }
            total.unwrap_or(Start::MaybeEmpty(ByteSet::new()))
        }
    }
}

/// TODO: what to do with low selective things like `\S+` or anything negated like `[^a]` that can match pretty much any chars?
fn fancy_start(expr: &Expr) -> Start {
    match expr {
        Expr::Empty | Expr::Assertion(_) | Expr::KeepOut | Expr::ContinueFromPreviousMatchEnd => {
            Start::MaybeEmpty(ByteSet::new())
        }
        Expr::Literal { val, casei } => {
            let bytes = val.as_bytes();
            if let Some(first) = bytes.first() {
                let mut set = ByteSet::new();
                if first.is_ascii() {
                    if *casei {
                        set.insert(first.to_ascii_uppercase());
                        set.insert(first.to_ascii_lowercase());
                    } else {
                        set.insert(*first);
                    }
                    Start::Definite(set)
                } else {
                    // utf-8, we ignore if casei is true since we can't represent it i think?
                    if *casei {
                        Start::Bail
                    } else {
                        set.insert(*first);
                        Start::Definite(set)
                    }
                }
            } else {
                Start::MaybeEmpty(ByteSet::new())
            }
        }
        // We parse those with regex-syntax
        Expr::Delegate { inner, casei } => {
            match regex_syntax::ParserBuilder::new()
                .case_insensitive(*casei)
                .build()
                .parse(inner)
            {
                Ok(hir) => regex_syntax_start(&hir),
                Err(_) => Start::Bail,
            }
        }

        Expr::Concat(exprs) => {
            let mut set = ByteSet::new();
            for expr in exprs {
                match fancy_start(expr) {
                    Start::Definite(mut s) => {
                        // We only care about the first byte so as soon as we hit a definite match
                        // we don't need to continue processing the rest
                        s.union(&set);
                        return Start::Definite(s);
                    }
                    Start::MaybeEmpty(s) => {
                        set.union(&s);
                    }
                    Start::Bail => {
                        // If we even get one bail, we bail everything
                        return Start::Bail;
                    }
                }
            }
            Start::MaybeEmpty(set)
        }

        Expr::Alt(exprs) => {
            let mut total = None;
            for expr in exprs {
                let expr_start = fancy_start(expr);
                total = Some(match (total, expr_start) {
                    (None, b) => b,
                    (Some(Start::Bail), _) | (Some(_), Start::Bail) => Start::Bail,
                    (Some(Start::Definite(mut a)), Start::Definite(b)) => {
                        a.union(&b);
                        Start::Definite(a)
                    }
                    (Some(Start::Definite(mut a)), Start::MaybeEmpty(b))
                    | (Some(Start::MaybeEmpty(mut a)), Start::Definite(b))
                    | (Some(Start::MaybeEmpty(mut a)), Start::MaybeEmpty(b)) => {
                        a.union(&b);
                        Start::MaybeEmpty(a)
                    }
                });
            }
            total.unwrap_or(Start::MaybeEmpty(ByteSet::new()))
        }

        Expr::Group(child) => fancy_start(child),
        Expr::AtomicGroup(child) => fancy_start(child),

        Expr::Repeat { child, lo, .. } => {
            let child_start = fancy_start(child);
            if *lo == 0 {
                // if we allow 0 reps then it's not definite
                match child_start {
                    Start::Definite(set) | Start::MaybeEmpty(set) => Start::MaybeEmpty(set),
                    Start::Bail => Start::Bail,
                }
            } else {
                child_start
            }
        }
        Expr::LookAround(expr, lookaround) => match lookaround {
            LookAround::LookAhead => match fancy_start(expr) {
                Start::Definite(set) => Start::Definite(set),
                _ => Start::MaybeEmpty(ByteSet::new()),
            },
            _ => Start::MaybeEmpty(ByteSet::new()),
        },
        _ => Start::Bail,
    }
}

fn get_byteset_from_pattern(pattern: &str) -> Option<ByteSet> {
    let tree = Expr::parse_tree(pattern).ok()?;
    match fancy_start(&tree.expr) {
        Start::Definite(set) if !set.is_empty() => Some(set),
        _ => None,
    }
}

#[derive(PartialEq)]
pub struct Prefilter {
    /// List of 256 bool per pattern.
    /// A Vec<Vec<bool>> inlined to avoid too many allocations
    table: Vec<bool>,
    /// Does byte i has a pattern matching?
    quick_lookup: [bool; 256],
}

impl Prefilter {
    pub fn build(patterns: &[String]) -> Option<Self> {
        if patterns.is_empty() {
            return None;
        }
        let n = patterns.len();

        let mut table = vec![false; 256 * n];
        let mut quick_lookup = [false; 256];

        for (idx, pattern) in patterns.iter().enumerate() {
            let set = get_byteset_from_pattern(pattern)?;
            for b in 0..=u8::MAX {
                if set.contains(b) {
                    quick_lookup[b as usize] = true;
                    table[b as usize * n + idx] = true;
                }
            }
        }

        Some(Self {
            table,
            quick_lookup,
        })
    }

    #[inline]
    pub fn may_match_at(&self, byte: u8) -> bool {
        self.quick_lookup[byte as usize]
    }

    pub fn candidates(&self, byte: u8) -> impl Iterator<Item = usize> + '_ {
        let n = self.table.len() / 256;
        let start = byte as usize * n;
        self.table[start..start + n]
            .iter()
            .enumerate()
            .filter_map(|(idx, &present)| present.then_some(idx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_bytes_of(pattern: &str) -> Option<Vec<u8>> {
        let set = get_byteset_from_pattern(pattern)?;
        let mut out = vec![];
        for (i, val) in set.0.iter().enumerate() {
            if *val {
                out.push(i as u8);
            }
        }
        Some(out)
    }

    #[test]
    fn can_extract_ascii_byteset_from_pattern() {
        let inputs = vec![
            (r"fn", Some(vec![b'f'])),
            (r"::", Some(vec![b':'])),
            (r"=>", Some(vec![b'='])),
            (r"\{", Some(vec![b'{'])),
            (r"fn|let|impl", Some(vec![b'f', b'i', b'l'])),
            (r"(fn|let|impl)", Some(vec![b'f', b'i', b'l'])),
            (r"(?:->|<-)", Some(vec![b'-', b'<'])),
            (r"\bfn\b", Some(vec![b'f'])),
            (r"^let", Some(vec![b'l'])),
            (r"\Gx", Some(vec![b'x'])),
            (r"(?<!\.)let", Some(vec![b'l'])),
            (r"[abc]x", Some(vec![b'a', b'b', b'c'])),
            (r"[a-d]", Some(vec![b'a', b'b', b'c', b'd'])),
            (r"(?i)fn", Some(vec![b'F', b'f'])),
            (r"(?i)[a-b]", Some(vec![b'A', b'B', b'a', b'b'])),
            (r"&(?![&=])", Some(vec![b'&'])),
            (r"(?x) fn # comment", Some(vec![b'f'])),
            (r"(?=\[)", Some(vec![b'['])),
            (r"(?=.*)x", Some(vec![b'x'])),
            (r"(?=a?)x", Some(vec![b'x'])),
            (r"(?!x)a", Some(vec![b'a'])),
            (r#"(?=["'`])""#, Some(vec![b'"', b'\'', b'`'])),
            (r#"(?=ab|xy)"#, Some(vec![b'a', b'x'])),
            (r#"r?""#, Some(vec![b'"', b'r'])),
            (r"a+", Some(vec![b'a'])),
            (r"(?>fn)", Some(vec![b'f'])),
            (r"\Kfn", Some(vec![b'f'])),
            (r"über", Some(vec![0xC3])),
            (r"a?", None),
            (r"(?!x)", None),
            (r".", None),
            (r".*x", None),
            (r"\R", None),
            (r"(?i)é", None),
            (r"(", None),
        ];

        for (input, expected) in inputs {
            assert_eq!(first_bytes_of(input), expected);
        }
    }

    #[test]
    fn can_extract_byteset_from_unicode() {
        let upper = first_bytes_of(r"\p{upper}!").unwrap();
        assert!(upper.contains(&b'A') && upper.contains(&b'Z'));
        assert!(!upper.contains(&b'a') && !upper.contains(&b'!'));
        // unicode lead byte is present
        assert!(upper.contains(&0xC3));

        let not_upper = first_bytes_of(r"\P{upper}!").unwrap();
        assert!(not_upper.contains(&b'a') && not_upper.contains(&b'z'));
        // ! is not uppercase so it can be the first match
        assert!(!not_upper.contains(&b'A') && not_upper.contains(&b'!'));
        // something not recognised is just nothing for the prefilter
        assert!(first_bytes_of(r"\P{typo}!").is_none());
    }

    #[test]
    fn can_extract_byteset_from_negation() {
        let b1 = first_bytes_of(r"[^a-c]x").unwrap();
        assert!(!b1.contains(&b'a') && !b1.contains(&b'c'));
        assert!(b1.contains(&b'd') && b1.contains(&b' ') && b1.contains(&0xC3));

        let b2 = first_bytes_of(r"(?i)[^a]").unwrap();
        assert!(!b2.contains(&b'a') && !b2.contains(&b'A') && b2.contains(&b'b'));

        let b3 = first_bytes_of(r"[^[:alpha:]]").unwrap();
        assert!(!b3.contains(&b'a') && !b3.contains(&b'Z'));
        assert!(b3.contains(&b'0') && b3.contains(&b' ') && b3.contains(&0xC3));
    }
}
