use std::fmt;
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};

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

/// A regex wrapper that serializes as a string but compiles lazily at runtime
pub struct Regex {
    pattern: String,
    compiled: OnceLock<Option<Arc<onig::Regex>>>,
}

impl Clone for Regex {
    fn clone(&self) -> Self {
        // Create a new regex with the same pattern but fresh lazy compilation
        Regex::new(self.pattern.clone())
    }
}

impl fmt::Debug for Regex {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.pattern)
    }
}

impl Regex {
    pub fn new(pattern: String) -> Self {
        // TODO: validate and look for backreference

        // Transform \z to $(?!\n)(?<!\n) to match vscode-textmate behavior
        // \z in Oniguruma matches absolute end of string, but TextMate grammars
        // expect it to match end-of-string-or-before-final-newline
        // This is needed at least for the po grammar sample from shiki
        let transformed_pattern = transform_z_anchor(&pattern);

        Self {
            pattern: transformed_pattern,
            compiled: OnceLock::new(),
        }
    }

    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    pub fn compiled(&self) -> Option<&Arc<onig::Regex>> {
        self.compiled
            .get_or_init(|| onig::Regex::new(&self.pattern).ok().map(Arc::new))
            .as_ref()
    }

    /// Validate that this regex pattern compiles successfully
    pub fn validate(&self) -> Result<(), onig::Error> {
        onig::Regex::new(&self.pattern).map(|_| ())
    }

    pub fn has_backreferences(&self) -> bool {
        for i in 1..=9 {
            let backref = format!("\\{}", i);
            if self.pattern.contains(&backref) {
                return true;
            }
        }
        false
    }

    pub fn resolve_backreferences(
        &self,
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
        let mut chars = self.pattern.chars().peekable();
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

    /// Try to find a match starting at the given position
    pub fn find_at(&self, text: &str, start: usize) -> Option<(usize, usize)> {
        let regex = self.compiled()?;
        let search_text = text.get(start..)?;
        regex
            .find(search_text)
            .map(|pos| (pos.0 + start, pos.1 + start))
    }

    /// Try to get captures starting at the given position
    pub fn captures_at(&self, text: &str, start: usize) -> Option<Vec<String>> {
        let regex = self.compiled()?;
        let search_text = text.get(start..)?;

        if let Some(captures) = regex.captures(search_text) {
            let mut result = Vec::new();
            for i in 0..captures.len() {
                if let Some(capture) = captures.at(i) {
                    result.push(capture.to_string());
                } else {
                    result.push(String::new());
                }
            }
            Some(result)
        } else {
            None
        }
    }
}

/// Transform \z anchor from Oniguruma "end of string" to TextMate "end without newline"
/// This matches the behavior in vscode-textmate's RegExpSource constructor
fn transform_z_anchor(pattern: &str) -> String {
    pattern
        .replace("\\\\z", "___TEMP___") // Protect literal \\z
        .replace("\\z", "$(?!\\n)(?<!\\n)") // Transform \z anchor
        .replace("___TEMP___", "\\\\z") // Restore literal \\z
}

impl Serialize for Regex {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.pattern)
    }
}

impl<'de> Deserialize<'de> for Regex {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let pattern = String::deserialize(deserializer)?;
        Ok(Regex::new(pattern))
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
