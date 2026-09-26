use std::borrow::Cow;
use std::sync::{Arc, OnceLock};

use crate::grammars::caches::RegexCache;
use crate::grammars::engine::{self, fancy_options};
use fancy_regex::ByteSet;
use serde::{Deserialize, Serialize};

// Hardcoded replacements until upstream is fixed
const REPLACEMENT_STRINGS: &[(&str, &str)] = &[
    // https://github.com/rust-lang/regex/pull/1396 for PHP grammar
    (
        r#"[\&()0-9\\_a-z|\x7F-\x{10FFFF}\s]"#,
        r#"(?-i:[\&()0-9\\_A-Za-z|\x7F-\x{10FFFF}\s])"#,
    ),
    (
        r#"[^0-9A-Z\\_a-z\x7F-\x{10FFFF}]"#,
        r#"(?-i:[^0-9A-Z\\_A-Za-z\x7F-\x{10FFFF}])"#,
    ),
    (
        r#"[^$0-9\\_a-z\x7F-\x{10FFFF}]"#,
        r#"(?-i:[^$0-9\\_A-Za-z\x7F-\x{10FFFF}])"#,
    ),
    (
        r#"[^0-9\\_a-z\x7F-\x{10FFFF}]"#,
        r#"(?-i:[^0-9\\_A-Za-z\x7F-\x{10FFFF}])"#,
    ),
    (
        r#"[$0-9\\_a-z\x7F-\x{10FFFF}]"#,
        r#"(?-i:[$0-9\\_A-Za-z\x7F-\x{10FFFF}])"#,
    ),
    (
        r#"[(0-9\\_a-z\x7F-\x{10FFFF}]"#,
        r#"(?-i:[(0-9\\_A-Za-z\x7F-\x{10FFFF}])"#,
    ),
    (
        r#"[0-9\\_a-z\x7F-\x{10FFFF}]"#,
        r#"(?-i:[0-9\\_A-Za-z\x7F-\x{10FFFF}])"#,
    ),
    (
        r#"[0-9_a-z\x7F-\x{10FFFF}]"#,
        r#"(?-i:[0-9_A-Za-z\x7F-\x{10FFFF}])"#,
    ),
    (
        r#"[\\_a-z\x7F-\x{10FFFF}]"#,
        r#"(?-i:[\\_A-Za-z\x7F-\x{10FFFF}])"#,
    ),
    (
        r#"[_a-z\x7F-\x{10FFFF}]"#,
        r#"(?-i:[_A-Za-z\x7F-\x{10FFFF}])"#,
    ),
    // https://github.com/slevithan/oniguruma-parser/pull/28 for shellscript grammar
    (
        r#"(?!nocorrect\W|nocorrect\$|function\W|function\$|foreach\W|foreach\$|repeat\W|repeat\$|logout\W|logout\$|coproc\W|coproc\$|select\W|select\$|while\W|while\$|pushd\W|pushd\$|until\W|until\$|case\W|case\$|done\W|done\$|elif\W|elif\$|else\W|else\$|esac\W|esac\$|popd\W|popd\$|then\W|then\$|time\W|time\$|for\W|for\$|end\W|end\$|fi\W|fi\$|do\W|do\$|in\W|in\$|if\W|if\$)"#,
        r#"(?!(?:nocorrect|function|foreach|repeat|logout|coproc|select|while|pushd|until|case|done|elif|else|esac|popd|then|time|for|end|fi|do|in|if)(?:\W|\$))"#,
    ),
];

fn fix_slow_patterns(pattern: &str) -> Cow<'_, str> {
    let mut out = Cow::Borrowed(pattern);
    for (from, to) in REPLACEMENT_STRINGS {
        if out.contains(from) {
            out = Cow::Owned(out.replace(from, to));
        }
    }
    out
}

/// Escapes regular expression characters in a given string
pub fn escape_regexp_characters(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '-' | '\\' | '{' | '}' | '*' | '+' | '?' | '|' | '^' | '$' | '.' | ',' | '[' | ']'
            | '(' | ')' | '#' => {
                format!("\\{}", c)
            }
            c if c.is_whitespace() => {
                format!("\\{}", c)
            }
            _ => c.to_string(),
        })
        .collect()
}

pub fn resolve_backreferences(
    pattern: &str,
    input: &str,
    captures_pos: &[Option<(usize, usize)>],
) -> String {
    let captures: Vec<_> = captures_pos
        .iter()
        .map(|cap| match cap {
            Some((start, end)) => &input[*start..*end],
            None => "",
        })
        .collect();

    let mut result = String::new();
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            // Collect all consecutive digits
            let mut digits = String::new();
            while let Some(&next_char) = chars.peek() {
                if next_char.is_ascii_digit() {
                    digits.push(next_char);
                    chars.next();
                } else {
                    break;
                }
            }

            if !digits.is_empty() {
                // Parse the digits as an index
                if let Ok(index) = digits.parse::<usize>() {
                    let captured = captures.get(index).unwrap_or(&"");
                    result.push_str(&escape_regexp_characters(captured));
                } else {
                    // Invalid number, keep original
                    result.push('\\');
                    result.push_str(&digits);
                }
            } else {
                // No digits after backslash
                result.push(c);
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// Transform \z anchor from Oniguruma "end of string" to TextMate "end without newline"
/// This matches the behavior in vscode-textmate's RegExpSource constructor
fn transform_z_anchor(pattern: &str) -> String {
    pattern
        .replace("\\\\z", "___TEMP___") // Protect literal \\z
        .replace("\\z", "$(?!\\n)(?<!\\n)") // Transform \z anchor
        .replace("___TEMP___", "\\\\z") // Restore literal \\z
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pattern {
    pat: String,
    #[serde(with = "optional_byte_set_serde")]
    start_byte_set: Option<ByteSet>,
    #[serde(with = "byte_set_serde")]
    required_byte_set: ByteSet,
    /// The original compiled one is in the `RegexCache`, this is just a Arc clone so we save some time
    /// by skipping a hashmap lookup (+ to_string() since papaya requires an owned key).
    /// Only used for end/while regex, it's not worth using that as well for the matcher.
    #[serde(skip)]
    compiled: OnceLock<Arc<engine::Regex>>,
}

impl PartialEq for Pattern {
    fn eq(&self, other: &Self) -> bool {
        self.pat == other.pat
    }
}

mod optional_byte_set_serde {
    use fancy_regex::ByteSet;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(v: &Option<ByteSet>, s: S) -> Result<S::Ok, S::Error> {
        v.map(|b| *b.words()).serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<ByteSet>, D::Error> {
        let words = Option::<[u64; 4]>::deserialize(d)?;
        Ok(words.map(ByteSet::from_words))
    }
}

mod byte_set_serde {
    use fancy_regex::ByteSet;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(v: &ByteSet, s: S) -> Result<S::Ok, S::Error> {
        v.words().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<ByteSet, D::Error> {
        let words = <[u64; 4]>::deserialize(d)?;
        Ok(ByteSet::from_words(words))
    }
}

impl Pattern {
    pub fn new(pattern: String) -> Self {
        // Transform \z to $(?!\n)(?<!\n) to match vscode-textmate behavior
        // \z in Oniguruma matches absolute end of string, but TextMate grammars
        // expect it to match end-of-string-or-before-final-newline
        // This is needed at least for the po grammar sample from shiki
        let transformed_pattern = transform_z_anchor(&pattern);
        let transformed_pattern = fix_slow_patterns(&transformed_pattern).to_string();

        let options = fancy_options();
        let start_byte_set = options.start_bytes(&transformed_pattern).unwrap_or(None);
        let required_byte_set = options
            .required_bytes(&transformed_pattern)
            .expect("should be able to get required bytes");

        Self {
            pat: transformed_pattern,
            start_byte_set,
            required_byte_set,
            compiled: OnceLock::new(),
        }
    }

    pub(crate) fn regex(&self, cache: &RegexCache) -> Arc<engine::Regex> {
        self.compiled
            .get_or_init(|| cache.get_regex(&self.pat))
            .clone()
    }

    /// Drops the compiled regex for benchmarks.
    pub(crate) fn reset(&mut self) {
        self.compiled.take();
    }

    pub fn pattern(&self) -> &str {
        &self.pat
    }

    pub fn byte_set(&self) -> Option<&ByteSet> {
        self.start_byte_set.as_ref()
    }

    pub fn required_byte_set(&self) -> &ByteSet {
        &self.required_byte_set
    }
}

/// Whether a pattern contains a \A or \G
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AnchorUsage {
    pub uses_a: bool,
    pub uses_g: bool,
}

impl AnchorUsage {
    pub fn from_pattern(pattern: &str) -> Self {
        let bytes = pattern.as_bytes();
        let has = |letter: u8| bytes.windows(2).any(|w| w == [b'\\', letter]);
        Self {
            uses_a: has(b'A'),
            uses_g: has(b'G'),
        }
    }

    pub fn union(self, other: Self) -> Self {
        Self {
            uses_a: self.uses_a || other.uses_a,
            uses_g: self.uses_g || other.uses_g,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transform_z_anchor() {
        // Test basic \z transformation
        assert_eq!(transform_z_anchor("\\z"), "$(?!\\n)(?<!\\n)");

        // Test \z at end of pattern
        assert_eq!(transform_z_anchor("^start\\z"), "^start$(?!\\n)(?<!\\n)");

        // Test \z in middle of pattern
        assert_eq!(transform_z_anchor("\\zmiddle"), "$(?!\\n)(?<!\\n)middle");

        // Test multiple \z in pattern
        assert_eq!(
            transform_z_anchor("\\z.*\\z"),
            "$(?!\\n)(?<!\\n).*$(?!\\n)(?<!\\n)"
        );

        // Test no \z in pattern (should return unchanged)
        assert_eq!(transform_z_anchor("^normal$"), "^normal$");

        // Test literal \\z (escaped backslash + z) should NOT be transformed
        assert_eq!(transform_z_anchor("\\\\z"), "\\\\z");

        // Test other backslash sequences should remain unchanged
        assert_eq!(transform_z_anchor("\\A\\G\\n\\t"), "\\A\\G\\n\\t");

        // Test empty pattern
        assert_eq!(transform_z_anchor(""), "");

        // Test complex pattern from PO grammar
        assert_eq!(
            transform_z_anchor("^(?:(?=(msg(?:id(_plural)?|ctxt))\\s*\"[^\"])|\\s*$).*\\z"),
            "^(?:(?=(msg(?:id(_plural)?|ctxt))\\s*\"[^\"])|\\s*$).*$(?!\\n)(?<!\\n)"
        );
    }
}
