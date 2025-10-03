use std::collections::HashMap;
use std::ops::Deref;

use serde::{Deserialize, Serialize};

use crate::grammars::raw::{Captures, RawGrammar, RawRule};
use crate::grammars::regex::Regex;
use crate::grammars::{ScopeId, get_scope_id};

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuleId(u16);

impl Deref for RuleId {
    type Target = u16;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RegexId(u16);

impl Deref for RegexId {
    type Target = u16;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RepositoryId(u16);

impl Deref for RepositoryId {
    type Target = u16;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// TODO optimise the String here
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct Repository(HashMap<String, RuleId>);

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize, Default)]
pub struct RepositoryStack {
    // TODO: check what's the biggest stack we get from shiki grammars
    stack: [Option<RepositoryId>; 8],
    len: u8,
}

impl RepositoryStack {
    pub fn push(mut self, id: RepositoryId) -> Self {
        self.stack[self.len as usize] = Some(id);
        self.len += 1;
        self
    }

    pub fn pop(mut self) -> (RepositoryId, Self) {
        let popped = self.stack[self.len as usize - 1].take().unwrap();
        self.len -= 1;
        (popped, self)
    }
}

/// per vscode-textmate:
///  Allowed values:
///  * Scope Name, e.g. `source.ts`
///  * Top level scope reference, e.g. `source.ts#entity.name.class`
///  * Relative scope reference, e.g. `#entity.name.class`
///  * self, e.g. `$self`
///  * base, e.g. `$base`
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum Reference {
    Self_,
    Base,
    Local(String),
    OtherComplete(ScopeId),
    OtherSpecific(ScopeId, String),
    Unknown(String),
}

impl From<&str> for Reference {
    fn from(value: &str) -> Self {
        let r = match value.as_ref() {
            "$self" => Self::Self_,
            "$base" => Self::Base,
            s if s.starts_with('#') => Self::Local(s[1..].to_string()),
            s if s.contains('#') => {
                let (scope, rule) = s.split_once('#').unwrap();
                match get_scope_id(scope) {
                    Some(scope_id) => Self::OtherSpecific(scope_id, rule.to_string()),
                    None => Self::Unknown(value.to_string()),
                }
            }
            s if s.contains('.') => {
                // Try parsing as scope.repository format (e.g., "source.js.regexp")
                if let Some(dot_pos) = s.rfind('.') {
                    let (scope_part, rule_part) = s.split_at(dot_pos);
                    let rule_part = &rule_part[1..]; // Remove the '.'

                    // Check if the scope part is a valid scope
                    if let Some(scope_id) = get_scope_id(scope_part) {
                        return Self::OtherSpecific(scope_id, rule_part.to_string());
                    }
                }
                // If not a valid scope.rule format, fall through to complete scope lookup
                match get_scope_id(value) {
                    Some(scope_id) => Self::OtherComplete(scope_id),
                    None => Self::Unknown(value.to_string()),
                }
            }
            _ => match get_scope_id(value) {
                Some(scope_id) => Self::OtherComplete(scope_id),
                None => Self::Unknown(value.to_string()),
            },
        };

        if matches!(r, Self::Unknown(_)) {
            println!("Scope {value} not found");
        }
        r
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
enum RuleIdOrReference {
    RuleId(RuleId),
    Reference(Reference),
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Match {
    id: RuleId,
    // some match only care about the captures
    scope_id: Option<ScopeId>,
    regex_id: RegexId,
    captures: Vec<RuleId>,
    repository_stack: RepositoryStack,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct IncludeOnly {
    id: RuleId,
    scope_id: Option<ScopeId>,
    content_scope_id: Option<ScopeId>,
    repository_stack: RepositoryStack,
    patterns: Vec<RuleIdOrReference>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct BeginEnd {
    id: RuleId,
    scope_id: Option<ScopeId>,
    content_scope_id: Option<ScopeId>,
    begin: RegexId,
    begin_captures: Vec<Option<RuleId>>,
    end: RegexId,
    end_has_backrefs: bool,
    end_captures: Vec<Option<RuleId>>,
    apply_end_pattern_last: bool,
    patterns: Vec<RuleIdOrReference>,
    repository_stack: RepositoryStack,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct BeginWhile {
    id: RuleId,
    scope_id: Option<ScopeId>,
    content_scope_id: Option<ScopeId>,
    begin: RegexId,
    begin_captures: Vec<Option<RuleId>>,
    while_: RegexId,
    while_has_backrefs: bool,
    while_captures: Vec<Option<RuleId>>,
    apply_end_pattern_last: bool,
    patterns: Vec<RuleIdOrReference>,
    repository_stack: RepositoryStack,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum Rule {
    Match(Match),
    IncludeOnly(IncludeOnly),
    BeginEnd(BeginEnd),
    BeginWhile(BeginWhile),
    Noop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledGrammar {
    pub name: String,
    pub display_name: Option<String>,
    pub scope_name: String,
    pub scope_id: ScopeId,
    pub file_types: Vec<String>,
    pub regexes: Vec<Regex>,
    pub rules: Vec<Rule>,
    pub repositories: Vec<Repository>,
}

impl CompiledGrammar {
    pub fn from_raw_grammar(raw: RawGrammar) -> Result<Self, CompileError> {
        let scope_id = get_scope_id(&raw.scope_name).ok_or_else(|| CompileError::UnknownScope {
            scope: raw.scope_name.clone(),
        })?;

        let mut grammar = Self {
            name: raw.name,
            display_name: raw.display_name,
            scope_name: raw.scope_name,
            file_types: raw.file_types,
            scope_id,
            regexes: Vec::new(),
            rules: Vec::new(),
            repositories: Vec::new(),
        };

        let root_rule = RawRule {
            patterns: raw.patterns,
            repository: raw.repository,
            ..Default::default()
        };
        let root_rule_id = grammar.compile_rule(root_rule, RepositoryStack::default())?;
        assert_eq!(*root_rule_id, 0);

        Ok(grammar)
    }

    fn compile_rule(
        &mut self,
        raw_rule: RawRule,
        repository_stack: RepositoryStack,
    ) -> Result<RuleId, CompileError> {
        let id = RuleId(self.rules.len() as u16);

        // push a no-op to reserve its spot
        self.rules.push(Rule::Noop);
        let scope_id = raw_rule.name.map(|x| get_scope_id(&x).unwrap());

        // https://github.com/microsoft/vscode-textmate/blob/f03a6a8790af81372d0e81facae75554ec5e97ef/src/rule.ts#L389-L447
        let rule = if let Some(pat) = raw_rule.match_ {
            Rule::Match(Match {
                id,
                scope_id,
                regex_id: self.compile_regex(pat).0,
                captures: vec![],
                repository_stack,
            })
        } else if let Some(begin_pat) = raw_rule.begin {
            let content_scope_id = raw_rule.content_name.map(|x| get_scope_id(&x).unwrap());
            let apply_end_pattern_last = raw_rule.apply_end_pattern_last;
            if let Some(while_pat) = raw_rule.while_ {
                let (while_, while_has_backrefs) = self.compile_regex(while_pat);
                Rule::BeginWhile(BeginWhile {
                    id,
                    scope_id,
                    content_scope_id,
                    begin: self.compile_regex(begin_pat).0,
                    begin_captures: self
                        .compile_captures(raw_rule.begin_captures, repository_stack)?,
                    while_,
                    while_has_backrefs,
                    while_captures: self
                        .compile_captures(raw_rule.while_captures, repository_stack)?,
                    patterns: self.compile_patterns(raw_rule.patterns, repository_stack)?,
                    apply_end_pattern_last,
                    repository_stack,
                })
            } else if let Some(end_pat) = raw_rule.end {
                let (end, end_has_backrefs) = self.compile_regex(end_pat);
                Rule::BeginEnd(BeginEnd {
                    id,
                    scope_id,
                    content_scope_id,
                    begin: self.compile_regex(begin_pat).0,
                    begin_captures: self
                        .compile_captures(raw_rule.begin_captures, repository_stack)?,
                    end,
                    end_has_backrefs,
                    end_captures: self
                        .compile_captures(raw_rule.while_captures, repository_stack)?,
                    patterns: self.compile_patterns(raw_rule.patterns, repository_stack)?,
                    apply_end_pattern_last,
                    repository_stack,
                })
            } else {
                // a rule that has begin without while/end is just a match, probably a typo
                Rule::Match(Match {
                    id,
                    scope_id,
                    regex_id: self.compile_regex(begin_pat).0,
                    captures: vec![],
                    repository_stack,
                })
            }
        } else {
            let repository_stack = if raw_rule.repository.is_empty() {
                repository_stack
            } else {
                let repo_id = self.compile_repository(raw_rule.repository, repository_stack)?;
                repository_stack.push(repo_id)
            };

            // vscode-textmate does something funny here:
            // - if patterns are NOT present and includes are, it moves includes to patterns;
            // - however, if patterns ARE present, includes are ignored
            // https://github.com/microsoft/vscode-textmate/blob/f03a6a8790af81372d0e81facae75554ec5e97ef/src/rule.ts#L404
            let patterns = if raw_rule.patterns.is_empty() {
                if let Some(include) = raw_rule.include {
                    vec![RawRule {
                        include: Some(include),
                        ..Default::default()
                    }]
                } else {
                    raw_rule.patterns
                }
            } else {
                raw_rule.patterns
            };

            if patterns.is_empty() {
                Rule::Noop
            } else {
                Rule::IncludeOnly(IncludeOnly {
                    id,
                    scope_id,
                    repository_stack,
                    content_scope_id: raw_rule.content_name.map(|x| get_scope_id(&x).unwrap()),
                    patterns: self.compile_patterns(patterns, repository_stack)?,
                })
            }
        };

        self.rules[*id as usize] = rule;
        Ok(id)
    }

    fn compile_regex(&mut self, pattern: String) -> (RegexId, bool) {
        let regex_id = RegexId(self.regexes.len() as u16);
        let re = Regex::new(pattern);
        let has_backrefs = re.has_backreferences();
        self.regexes.push(re);

        (regex_id, has_backrefs)
    }

    fn compile_repository(
        &mut self,
        raw_repository: HashMap<String, RawRule>,
        repository_stack: RepositoryStack,
    ) -> Result<RepositoryId, CompileError> {
        let repo_id = RepositoryId(self.repositories.len() as u16);

        self.repositories.push(Repository::default());
        let stack = repository_stack.push(repo_id);

        let mut rules = HashMap::new();

        for (name, raw_rule) in raw_repository {
            rules.insert(name, self.compile_rule(raw_rule, stack)?);
        }

        self.repositories[*repo_id as usize] = Repository(rules);

        Ok(repo_id)
    }

    fn compile_captures(
        &mut self,
        captures: Captures,
        repository_stack: RepositoryStack,
    ) -> Result<Vec<Option<RuleId>>, CompileError> {
        if captures.is_empty() {
            return Ok(Vec::new());
        }

        // mdc.json syntax has actually a 912 backref
        let max_capture = captures.keys().max().copied().unwrap_or_default();
        let mut out: Vec<Option<RuleId>> = vec![None; max_capture + 1];

        for (key, rule) in captures.0 {
            out[key] = Some(self.compile_rule(rule, repository_stack)?);
        }

        Ok(out)
    }

    fn compile_patterns(
        &mut self,
        rules: Vec<RawRule>,
        repository_stack: RepositoryStack,
    ) -> Result<Vec<RuleIdOrReference>, CompileError> {
        let mut out = vec![];

        for r in rules {
            if let Some(include) = r.include {
                // vscode ignores other rule contents is there's an include
                // https://github.com/microsoft/vscode-textmate/blob/f03a6a8790af81372d0e81facae75554ec5e97ef/src/rule.ts#L495
                out.push(RuleIdOrReference::Reference(include.as_str().into()));
            } else {
                out.push(RuleIdOrReference::RuleId(
                    self.compile_rule(r, repository_stack)?,
                ));
            }
        }

        Ok(out)
    }
}

/// Errors that can occur during grammar compilation
#[derive(Debug)]
pub enum CompileError {
    InvalidRegex { pattern: String, error: onig::Error },
    UnknownScope { scope: String },
    UnresolvedInclude { include: String },
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompileError::InvalidRegex { pattern, error } => {
                write!(f, "Invalid regex pattern '{}': {}", pattern, error)
            }
            CompileError::UnknownScope { scope } => {
                write!(f, "Unknown scope '{}'", scope)
            }
            CompileError::UnresolvedInclude { include } => {
                write!(f, "Unresolved include '{}'", include)
            }
        }
    }
}

impl std::error::Error for CompileError {}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::grammars::raw::RawGrammar;

    #[test]
    fn can_compile_all_shiki_grammars() {
        let entries = fs::read_dir("grammars-themes/packages/tm-grammars/grammars")
            .expect("Failed to read grammars directory");

        for entry in entries {
            let entry = entry.expect("Failed to read directory entry");
            let path = entry.path();

            let raw_grammar = RawGrammar::load_from_file(&path).unwrap();

            println!(">> {path:?}");
            assert!(raw_grammar.compile().is_ok());
        }
        assert!(false);
    }
}
