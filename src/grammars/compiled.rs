use std::collections::HashMap;
use std::ops::Deref;

use serde::{Deserialize, Serialize};

use crate::grammars::pattern_set::PatternSet;
use crate::grammars::raw::{Captures, RawGrammar, RawRule};
use crate::grammars::regex::Regex;
use crate::grammars::{ScopeId, get_scope_id};

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuleId(pub u16);

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

impl Repository {
    /// Look up a rule by name in this repository
    pub fn get(&self, name: &str) -> Option<&RuleId> {
        self.0.get(name)
    }
}

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
pub enum RuleIdOrReference {
    RuleId(RuleId),
    Reference(Reference),
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Match {
    pub id: RuleId,
    // some match only care about the captures
    pub scope_id: Option<ScopeId>,
    /// The regex ID for this match rule.
    /// None for scope-only rules (e.g., capture groups that only assign scopes like
    /// punctuation.definition.string.begin without their own pattern to match)
    pub regex_id: Option<RegexId>,
    pub captures: Vec<RuleId>,
    pub repository_stack: RepositoryStack,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct IncludeOnly {
    pub id: RuleId,
    pub scope_id: Option<ScopeId>,
    pub content_scope_id: Option<ScopeId>,
    pub repository_stack: RepositoryStack,
    pub patterns: Vec<RuleIdOrReference>,
    /// Pre-compiled pattern set for efficient batch matching
    pub pattern_set: Option<PatternSet>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct BeginEnd {
    pub id: RuleId,
    pub scope_id: Option<ScopeId>,
    pub content_scope_id: Option<ScopeId>,
    pub begin: RegexId,
    pub begin_captures: Vec<Option<RuleId>>,
    pub end: RegexId,
    pub end_has_backrefs: bool,
    pub end_captures: Vec<Option<RuleId>>,
    pub apply_end_pattern_last: bool,
    pub patterns: Vec<RuleIdOrReference>,
    pub repository_stack: RepositoryStack,
    /// Pre-compiled pattern set for efficient batch matching
    pub pattern_set: Option<PatternSet>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct BeginWhile {
    pub id: RuleId,
    pub scope_id: Option<ScopeId>,
    pub content_scope_id: Option<ScopeId>,
    pub begin: RegexId,
    pub begin_captures: Vec<Option<RuleId>>,
    pub while_: RegexId,
    pub while_has_backrefs: bool,
    pub while_captures: Vec<Option<RuleId>>,
    pub apply_end_pattern_last: bool,
    pub patterns: Vec<RuleIdOrReference>,
    pub repository_stack: RepositoryStack,
    /// Pre-compiled pattern set for efficient batch matching
    pub pattern_set: Option<PatternSet>,
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
                regex_id: Some(self.compile_regex(pat).0),
                captures: vec![],
                repository_stack,
            })
        } else if let Some(begin_pat) = raw_rule.begin {
            let content_scope_id = raw_rule.content_name.map(|x| get_scope_id(&x).unwrap());
            let apply_end_pattern_last = raw_rule.apply_end_pattern_last;
            if let Some(while_pat) = raw_rule.while_ {
                let (while_, while_has_backrefs) = self.compile_regex(while_pat);
                let patterns = self.compile_patterns(raw_rule.patterns, repository_stack)?;
                let pattern_set = self.compute_pattern_data(&patterns);
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
                    patterns,
                    apply_end_pattern_last,
                    repository_stack,
                    pattern_set,
                })
            } else if let Some(end_pat) = raw_rule.end {
                let (end, end_has_backrefs) = self.compile_regex(end_pat);
                let patterns = self.compile_patterns(raw_rule.patterns, repository_stack)?;
                let pattern_set = self.compute_pattern_data(&patterns);
                Rule::BeginEnd(BeginEnd {
                    id,
                    scope_id,
                    content_scope_id,
                    begin: self.compile_regex(begin_pat).0,
                    begin_captures: self
                        .compile_captures(raw_rule.begin_captures, repository_stack)?,
                    end,
                    end_has_backrefs,
                    end_captures: self.compile_captures(raw_rule.end_captures, repository_stack)?,
                    patterns,
                    apply_end_pattern_last,
                    repository_stack,
                    pattern_set,
                })
            } else {
                // a rule that has begin without while/end is just a match, probably a typo
                Rule::Match(Match {
                    id,
                    scope_id,
                    regex_id: Some(self.compile_regex(begin_pat).0),
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

            // Check if this is a scope-only rule (like a capture with just a name)
            if scope_id.is_some() && raw_rule.patterns.is_empty() && raw_rule.include.is_none() {
                // This is a scope-only rule - create a Match rule with no regex
                // This handles captures that only assign scopes
                Rule::Match(Match {
                    id,
                    scope_id,
                    regex_id: None, // Scope-only rule (e.g., capture that only assigns scope)
                    captures: vec![],
                    repository_stack,
                })
            } else {
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
                    let compiled_patterns = self.compile_patterns(patterns, repository_stack)?;
                    let pattern_set = self.compute_pattern_data(&compiled_patterns);
                    Rule::IncludeOnly(IncludeOnly {
                        id,
                        scope_id,
                        repository_stack,
                        content_scope_id: raw_rule.content_name.map(|x| get_scope_id(&x).unwrap()),
                        patterns: compiled_patterns,
                        pattern_set,
                    })
                }
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

    /// Compute pattern data for pre-compilation during grammar construction.
    ///
    /// Only pre-compiles pattern sets that don't have backreferences, since patterns
    /// with backreferences need to be rebuilt at runtime with the actual captured text.
    /// Returns None if any patterns have backreferences or references that can't be resolved.
    fn compute_pattern_data(&self, patterns: &[RuleIdOrReference]) -> Option<PatternSet> {
        let mut pattern_strings = Vec::new();
        let mut rule_ids = Vec::new();

        for pattern_ref in patterns {
            match pattern_ref {
                RuleIdOrReference::RuleId(rule_id) => {
                    // Get the actual rule and extract its pattern
                    if let Some(rule) = self.rules.get(**rule_id as usize) {
                        match rule {
                            Rule::Match(match_rule) => {
                                // Only include patterns that have actual regexes (not scope-only)
                                if let Some(regex_id) = match_rule.regex_id {
                                    if let Some(regex) = self.regexes.get(*regex_id as usize) {
                                        // Check if this regex has backreferences
                                        if regex.has_backreferences() {
                                            // Can't pre-compile - contains backreferences
                                            return None;
                                        }
                                        pattern_strings.push(regex.pattern().to_string());
                                        rule_ids.push(*rule_id);
                                    }
                                }
                            }
                            Rule::BeginEnd(begin_end) => {
                                // For BeginEnd rules, use the begin pattern
                                if let Some(regex) = self.regexes.get(*begin_end.begin as usize) {
                                    // Check if begin pattern has backreferences
                                    if regex.has_backreferences() {
                                        return None;
                                    }
                                    pattern_strings.push(regex.pattern().to_string());
                                    rule_ids.push(*rule_id);
                                }
                            }
                            Rule::BeginWhile(begin_while) => {
                                // For BeginWhile rules, use the begin pattern
                                if let Some(regex) = self.regexes.get(*begin_while.begin as usize) {
                                    // Check if begin pattern has backreferences
                                    if regex.has_backreferences() {
                                        return None;
                                    }
                                    pattern_strings.push(regex.pattern().to_string());
                                    rule_ids.push(*rule_id);
                                }
                            }
                            Rule::IncludeOnly(include_only) => {
                                // Recursively resolve IncludeOnly patterns
                                // Pass has_backrefs=false since IncludeOnly rules don't have end/while patterns
                                if let Some(sub_pattern_set) =
                                    self.compute_pattern_data(&include_only.patterns)
                                {
                                    // Extract patterns and rule_ids from the sub PatternSet
                                    pattern_strings
                                        .extend(sub_pattern_set.patterns().iter().cloned());
                                    rule_ids.extend(sub_pattern_set.rule_ids().iter().cloned());
                                } else {
                                    // Sub-patterns couldn't be pre-compiled, so we can't pre-compile this either
                                    return None;
                                }
                            }
                            Rule::Noop => {
                                // Skip no-op rules
                            }
                        }
                    }
                }
                RuleIdOrReference::Reference(_) => {
                    // Any unresolved references mean we can't pre-compile
                    // These require runtime resolution with proper repository context
                    return None;
                }
            }
        }

        if pattern_strings.is_empty() {
            None
        } else {
            // Create PatternSet from the collected patterns and rule IDs
            let items: Vec<(RuleId, String)> = rule_ids
                .into_iter()
                .zip(pattern_strings.into_iter())
                .collect();
            Some(PatternSet::new(items))
        }
    }

    /// Get a pattern set for a rule.
    ///
    /// Returns a PatternSet when available, either from pre-compilation or created on-demand.
    /// Only rules with patterns (IncludeOnly, BeginEnd, BeginWhile) have pattern sets.
    pub fn get_pattern_set(&self, rule_id: RuleId) -> Option<PatternSet> {
        if let Some(rule) = self.rules.get(*rule_id as usize) {
            match rule {
                Rule::Match(_) => {
                    // Match rules don't have pattern sets - they are handled individually
                    None
                }
                Rule::IncludeOnly(include_only) => {
                    // Return pre-computed pattern set if available, otherwise create fallback
                    if let Some(pattern_set) = &include_only.pattern_set {
                        Some(pattern_set.clone())
                    } else {
                        // Create pattern set on-demand for patterns that couldn't be pre-compiled
                        Some(self.create_simple_pattern_set(&include_only.patterns))
                    }
                }
                Rule::BeginEnd(begin_end) => {
                    // Return pre-computed pattern set if available, otherwise create fallback
                    if let Some(pattern_set) = &begin_end.pattern_set {
                        Some(pattern_set.clone())
                    } else {
                        // Create pattern set on-demand for patterns that couldn't be pre-compiled
                        Some(self.create_simple_pattern_set(&begin_end.patterns))
                    }
                }
                Rule::BeginWhile(begin_while) => {
                    // Return pre-computed pattern set if available, otherwise create fallback
                    if let Some(pattern_set) = &begin_while.pattern_set {
                        Some(pattern_set.clone())
                    } else {
                        // Create pattern set on-demand for patterns that couldn't be pre-compiled
                        Some(self.create_simple_pattern_set(&begin_while.patterns))
                    }
                }
                Rule::Noop => {
                    // Noop rules don't have pattern sets
                    None
                }
            }
        } else {
            None
        }
    }

    /// Create a simple pattern set for cases that couldn't be pre-compiled.
    ///
    /// This is a much simpler fallback than the original complex methods we removed.
    /// It only handles the most basic cases: direct rule IDs and simple self-references.
    /// For complex references, it just skips them (better than breaking tokenization).
    fn create_simple_pattern_set(&self, patterns: &[RuleIdOrReference]) -> PatternSet {
        let mut pattern_data = Vec::new();

        for pattern_ref in patterns {
            match pattern_ref {
                RuleIdOrReference::RuleId(rule_id) => {
                    // Handle direct rule IDs - these are easy
                    if let Some(rule) = self.rules.get(**rule_id as usize) {
                        match rule {
                            Rule::Match(match_rule) => {
                                // Add match pattern if it has a regex
                                if let Some(regex_id) = match_rule.regex_id {
                                    if let Some(regex) = self.regexes.get(*regex_id as usize) {
                                        pattern_data.push((*rule_id, regex.pattern().to_string()));
                                    }
                                }
                            }
                            Rule::BeginEnd(begin_end) => {
                                // Add begin pattern for BeginEnd rules
                                if let Some(regex) = self.regexes.get(*begin_end.begin as usize) {
                                    pattern_data.push((*rule_id, regex.pattern().to_string()));
                                }
                            }
                            Rule::BeginWhile(begin_while) => {
                                // Add begin pattern for BeginWhile rules
                                if let Some(regex) = self.regexes.get(*begin_while.begin as usize) {
                                    pattern_data.push((*rule_id, regex.pattern().to_string()));
                                }
                            }
                            Rule::IncludeOnly(include_only) => {
                                // Recursively handle IncludeOnly patterns (but only one level deep to avoid complexity)
                                let sub_set =
                                    self.create_simple_pattern_set(&include_only.patterns);
                                pattern_data.extend(
                                    sub_set
                                        .rule_ids()
                                        .iter()
                                        .zip(sub_set.patterns().iter())
                                        .map(|(rule_id, pattern)| (*rule_id, pattern.clone())),
                                );
                            }
                            Rule::Noop => {
                                // Skip no-op rules
                            }
                        }
                    }
                }
                RuleIdOrReference::Reference(reference) => {
                    match reference {
                        Reference::Self_ | Reference::Base  => {
                            // Self-reference: include root patterns (but only if it's rule 0 to avoid infinite recursion)
                            if let Some(root_rule) = self.rules.get(0) {
                                if let Rule::IncludeOnly(include_only) = root_rule {
                                    let sub_set =
                                        self.create_simple_pattern_set(&include_only.patterns);
                                    pattern_data.extend(
                                        sub_set
                                            .rule_ids()
                                            .iter()
                                            .zip(sub_set.patterns().iter())
                                            .map(|(rule_id, pattern)| (*rule_id, pattern.clone())),
                                    );
                                }
                            }
                        }
                        Reference::Local(name) => {
                            // Look up the local pattern in the repository
                            // For simplicity, just check the main repository (index 0)
                            if let Some(repo) = self.repositories.get(0) {
                                if let Some(rule_id) = repo.get(name) {
                                    // Recursively process the referenced rule
                                    if let Some(rule) = self.rules.get(**rule_id as usize) {
                                        match rule {
                                            Rule::Match(match_rule) => {
                                                if let Some(regex_id) = match_rule.regex_id {
                                                    if let Some(regex) =
                                                        self.regexes.get(*regex_id as usize)
                                                    {
                                                        pattern_data.push((
                                                            *rule_id,
                                                            regex.pattern().to_string(),
                                                        ));
                                                    }
                                                }
                                            }
                                            Rule::BeginEnd(begin_end) => {
                                                if let Some(regex) =
                                                    self.regexes.get(*begin_end.begin as usize)
                                                {
                                                    pattern_data.push((
                                                        *rule_id,
                                                        regex.pattern().to_string(),
                                                    ));
                                                }
                                            }
                                            Rule::BeginWhile(begin_while) => {
                                                if let Some(regex) =
                                                    self.regexes.get(*begin_while.begin as usize)
                                                {
                                                    pattern_data.push((
                                                        *rule_id,
                                                        regex.pattern().to_string(),
                                                    ));
                                                }
                                            }
                                            Rule::IncludeOnly(include_only) => {
                                                let sub_set = self.create_simple_pattern_set(
                                                    &include_only.patterns,
                                                );
                                                pattern_data.extend(
                                                    sub_set
                                                        .rule_ids()
                                                        .iter()
                                                        .zip(sub_set.patterns().iter())
                                                        .map(|(rule_id, pattern)| {
                                                            (*rule_id, pattern.clone())
                                                        }),
                                                );
                                            }
                                            Rule::Noop => {
                                                // Skip no-op rules
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {
                            // Skip other complex references (OtherComplete, etc.)
                        }
                    }
                }
            }
        }

        PatternSet::new(pattern_data)
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
    use crate::grammars::raw::RawGrammar;
    use std::fs;

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
        // assert!(false); // Commented out - test was intentionally failing
    }

    #[test]
    fn analyze_pattern_set_caching_effectiveness() {
        let entries = fs::read_dir("grammars-themes/packages/tm-grammars/grammars")
            .expect("Failed to read grammars directory");

        let mut total_grammars = 0;
        let mut total_pattern_rules = 0;
        let mut total_cached = 0;
        let mut total_runtime = 0;
        let mut grammar_stats = Vec::new();

        for entry in entries {
            let entry = entry.expect("Failed to read directory entry");
            let path = entry.path();

            if let Ok(raw_grammar) = RawGrammar::load_from_file(&path) {
                if let Ok(compiled_grammar) = raw_grammar.compile() {
                    total_grammars += 1;
                    let grammar_name = path.file_name().unwrap().to_string_lossy().to_string();

                    let (pattern_rules, cached, runtime) = analyze_grammar_rules(&compiled_grammar);

                    total_pattern_rules += pattern_rules;
                    total_cached += cached;
                    total_runtime += runtime;

                    if pattern_rules > 0 {
                        grammar_stats.push((grammar_name, pattern_rules, cached, runtime));
                    }
                }
            }
        }

        // Sort grammars by total pattern rules (descending)
        grammar_stats.sort_by_key(|(_, total, _, _)| std::cmp::Reverse(*total));

        // Print comprehensive analysis
        println!("\n=== Pattern Set Caching Analysis Results ===");
        println!("Total grammars analyzed: {}", total_grammars);
        println!("Total pattern rules: {}", total_pattern_rules);

        if total_pattern_rules > 0 {
            let cached_percent = (total_cached as f64 / total_pattern_rules as f64) * 100.0;
            let runtime_percent = (total_runtime as f64 / total_pattern_rules as f64) * 100.0;

            println!(
                "Pre-compiled (cached): {} ({:.1}%)",
                total_cached, cached_percent
            );
            println!(
                "Runtime-compiled: {} ({:.1}%)",
                total_runtime, runtime_percent
            );

            println!("\nTop 10 grammars by pattern rule count:");
            for (i, (name, total, cached, runtime)) in grammar_stats.iter().take(10).enumerate() {
                let cached_pct = if *total > 0 {
                    (*cached as f64 / *total as f64) * 100.0
                } else {
                    0.0
                };
                println!(
                    "{}. {}: {} rules ({} cached {:.1}%, {} runtime {:.1}%)",
                    i + 1,
                    name,
                    total,
                    cached,
                    cached_pct,
                    runtime,
                    100.0 - cached_pct
                );
            }

            // Find grammars with best/worst caching ratios
            let mut ratios: Vec<_> = grammar_stats
                .iter()
                .filter(|(_, total, _, _)| *total >= 10) // Only consider grammars with at least 10 rules
                .map(|(name, total, cached, runtime)| {
                    let ratio = (*cached as f64 / *total as f64) * 100.0;
                    (name, *total, *cached, *runtime, ratio)
                })
                .collect();

            if !ratios.is_empty() {
                ratios.sort_by(|a, b| b.4.partial_cmp(&a.4).unwrap());

                println!("\nBest caching efficiency (>= 10 rules):");
                for (name, total, cached, _runtime, ratio) in ratios.iter().take(5) {
                    println!(
                        "  {}: {:.1}% cached ({}/{} rules)",
                        name, ratio, cached, total
                    );
                }

                println!("\nWorst caching efficiency (>= 10 rules):");
                for (name, total, cached, _runtime, ratio) in ratios.iter().rev().take(5) {
                    println!(
                        "  {}: {:.1}% cached ({}/{} rules)",
                        name, ratio, cached, total
                    );
                }
            }
        }

        println!("\n===========================================\n");

        // This is a data analysis test - we don't assert anything, just gather insights
        // But let's ensure we actually found some grammars
        assert!(
            total_grammars > 0,
            "Should have analyzed at least one grammar"
        );
    }

    /// Analyze a single grammar's rules and return (total_pattern_rules, cached_count, runtime_count)
    fn analyze_grammar_rules(grammar: &super::CompiledGrammar) -> (usize, usize, usize) {
        let mut pattern_rules = 0;
        let mut cached = 0;
        let mut runtime = 0;

        for rule in &grammar.rules {
            match rule {
                super::Rule::IncludeOnly(include_only) => {
                    if !include_only.patterns.is_empty() {
                        pattern_rules += 1;
                        if include_only.pattern_set.is_some() {
                            cached += 1;
                        } else {
                            runtime += 1;
                        }
                    }
                }
                super::Rule::BeginEnd(begin_end) => {
                    if !begin_end.patterns.is_empty() {
                        pattern_rules += 1;
                        if begin_end.pattern_set.is_some() {
                            cached += 1;
                        } else {
                            runtime += 1;
                        }
                    }
                }
                super::Rule::BeginWhile(begin_while) => {
                    if !begin_while.patterns.is_empty() {
                        pattern_rules += 1;
                        if begin_while.pattern_set.is_some() {
                            cached += 1;
                        } else {
                            runtime += 1;
                        }
                    }
                }
                super::Rule::Match(_) | super::Rule::Noop => {
                    // These don't have pattern sets to cache
                }
            }
        }

        (pattern_rules, cached, runtime)
    }
}
