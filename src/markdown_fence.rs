use crate::Options;
use std::collections::BTreeMap;
use std::ops::RangeInclusive;

#[derive(Debug)]
pub struct ParsedFence<'f> {
    pub lang: &'f str,
    pub options: Options,
    pub rest: BTreeMap<String, String>,
}

fn parse_range(s: &str) -> Option<RangeInclusive<usize>> {
    match s.find('-') {
        Some(dash) => {
            let mut from = s[..dash].parse().ok()?;
            let mut to = s[dash + 1..].parse().ok()?;
            if to < from {
                std::mem::swap(&mut from, &mut to);
            }
            Some(from..=to)
        }
        None => {
            let val = s.parse().ok()?;
            Some(val..=val)
        }
    }
}

pub fn parse_markdown_fence(fence: &str) -> ParsedFence<'_> {
    let mut language = None;
    let mut options = Options::default();
    let mut rest = BTreeMap::new();

    for token in fence.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }

        let mut token_split = token.split('=');
        match token_split.next().unwrap_or("").trim() {
            "linenostart" => {
                if let Some(start) = token_split.next().and_then(|s| s.parse().ok()) {
                    options.line_number_start = start;
                }
            }
            "linenos" => options.show_line_numbers = true,
            "hl_lines" => {
                if let Some(ranges_str) = token_split.next() {
                    for range_str in ranges_str.split(' ') {
                        if let Some(range) = parse_range(range_str) {
                            options.highlight_lines.push(range);
                        }
                    }
                }
            }
            "hide_lines" => {
                if let Some(ranges_str) = token_split.next() {
                    for range_str in ranges_str.split(' ') {
                        if let Some(range) = parse_range(range_str) {
                            options.hide_lines.push(range);
                        }
                    }
                }
            }
            key => {
                if let Some(value) = token_split.next() {
                    rest.insert(key.to_string(), value.trim().to_string());
                } else {
                    language = Some(key);
                }
            }
        }
    }

    ParsedFence {
        lang: language.unwrap_or(""),
        options,
        rest,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_only() {
        let result = parse_markdown_fence("rust");
        assert_eq!(result.lang, "rust");
        assert_eq!(result.options, Options::default());
        assert!(result.rest.is_empty());
    }

    #[test]
    fn test_empty_string() {
        let result = parse_markdown_fence("");
        assert_eq!(result.lang, "");
        assert_eq!(result.options, Options::default());
        assert!(result.rest.is_empty());
    }

    #[test]
    fn test_line_numbers() {
        let result = parse_markdown_fence("python,linenos");
        assert_eq!(result.lang, "python");
        assert!(result.options.show_line_numbers);
    }

    #[test]
    fn test_line_number_start() {
        let result = parse_markdown_fence("javascript,linenos,linenostart=5");
        assert_eq!(result.lang, "javascript");
        assert!(result.options.show_line_numbers);
        assert_eq!(result.options.line_number_start, 5);
    }

    #[test]
    fn test_highlight_lines_multiple() {
        let result = parse_markdown_fence("rust,hl_lines=1-3 5 7-9");
        assert_eq!(result.lang, "rust");
        assert_eq!(result.options.highlight_lines, vec![1..=3, 5..=5, 7..=9]);
    }

    #[test]
    fn test_hide_lines() {
        let result = parse_markdown_fence("rust,hide_lines=2 4-6");
        assert_eq!(result.lang, "rust");
        assert_eq!(result.options.hide_lines, vec![2..=2, 4..=6]);
    }

    #[test]
    fn test_metadata() {
        let result = parse_markdown_fence("rust,name=example,copy=true");
        assert_eq!(result.lang, "rust");
        assert_eq!(result.rest.get("name"), Some(&"example".to_string()));
        assert_eq!(result.rest.get("copy"), Some(&"true".to_string()));
    }

    #[test]
    fn test_complex_combination() {
        let result = parse_markdown_fence(
            "rust,linenos,linenostart=10,hl_lines=1-3 5,hide_lines=2,name=test",
        );
        assert_eq!(result.lang, "rust");
        assert!(result.options.show_line_numbers);
        assert_eq!(result.options.line_number_start, 10);
        assert_eq!(result.options.highlight_lines, vec![1..=3, 5..=5]);
        assert_eq!(result.options.hide_lines, vec![2..=2]);
        assert_eq!(result.rest.get("name"), Some(&"test".to_string()));
    }
}
