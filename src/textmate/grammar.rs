use std::collections::{HashMap, BTreeMap};
use std::fs::File;
use std::path::Path;

use serde_derive::Deserialize;


#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all(deserialize = "snake_case"))]
struct Capture {
    name: String,
    #[serde(default)]
    patterns: Vec<Pattern>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct MatchPattern {
    name: Option<String>,
    #[serde(rename(deserialize = "match"))]
    match_: String,
    captures: BTreeMap<String, Capture>,
    patterns: Vec<Pattern>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all(deserialize = "camelCase"))]
pub struct BeginEndPattern {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub content_name: Option<String>,
    pub begin: String,
    pub end: String,
    pub begin_captures: BTreeMap<String, Capture>,
    pub end_captures: BTreeMap<String, Capture>,
    #[serde(default)]
    pub patterns: Vec<Pattern>,
    // set to 1 if true
    pub apply_end_pattern_last: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all(deserialize = "camelCase"))]
pub struct BeginWhilePattern {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub content_name: Option<String>,
    pub begin: String,
    #[serde(rename(deserialize = "while"))]
    pub while_: String,
    pub begin_captures: BTreeMap<String, Capture>,
    pub while_captures: BTreeMap<String, Capture>,
    #[serde(default)]
    pub patterns: Vec<Pattern>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IncludePattern {
    pub include: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum Pattern {
    Include(IncludePattern),
    BeginEnd(BeginEndPattern),
    BeginWhile(BeginWhilePattern),
    Match(MatchPattern),
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct RepositoryEntry {
    patterns: Vec<Pattern>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all(deserialize = "camelCase"))]
struct RawGrammar {
    name: String,
    file_types: Vec<String>,
    scope_name: String,
    repository: HashMap<String, RepositoryEntry>,
    patterns: Vec<Pattern>,
    first_line_match: Option<String>,
}

impl RawGrammar {
    pub fn load_from_json_file<P: AsRef<Path>>(path: P) -> Self {
        let file = File::open(path).expect("TODO");
        let raw_grammar = serde_json::from_reader(&file).expect("TODO");
        raw_grammar
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn can_deser_rust_syntax() {
        let g = RawGrammar::load_from_json_file("syntaxes/rust.tmLanguage.json");
        println!("{g:#?}");
        assert_eq!(1, 0);
    }

}