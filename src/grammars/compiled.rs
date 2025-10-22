use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

use crate::grammars::pattern_set::PatternSet;
use crate::grammars::raw::{Captures, RawGrammar, RawRule};
use crate::grammars::regex::Regex;

static CAPTURING_NAME_RE: LazyLock<onig::Regex> =
    LazyLock::new(|| onig::Regex::new(r"\$(\d+)|\${(\d+):\/(downcase|upcase)}").unwrap());

fn has_captures(pat: Option<&str>) -> bool {
    if let Some(p) = pat {
        CAPTURING_NAME_RE.find(p).is_some()
    } else {
        false
    }
}

pub fn replace_captures(
    original_name: &str,
    text: &str,
    captures_pos: &[Option<(usize, usize)>],
) -> String {
    CAPTURING_NAME_RE
        .replace_all(original_name, |caps: &onig::Captures| {
            let capture_num = caps
                .at(1)
                .or_else(|| caps.at(2))
                .unwrap_or("0")
                .parse::<usize>()
                .unwrap_or(0);
            let command = caps.at(3);

            if let Some(Some((start, end))) = captures_pos.get(capture_num) {
                // Remove leading dots that would make the selector invalid
                let result = text[*start..*end].trim_start_matches('.').to_string();
                match command {
                    Some("downcase") => result.to_lowercase(),
                    Some("upcase") => result.to_uppercase(),
                    _ => result,
                }
            } else {
                // Invalid capture bounds or None capture, return original match
                caps.at(0).unwrap().to_string()
            }
        })
        .to_string()
}

fn process_scope_name(
    name: &Option<String>,
    is_capturing: bool,
    input: &str,
    captures_pos: &[Option<(usize, usize)>],
) -> Option<String> {
    match name {
        Some(name) if is_capturing => Some(replace_captures(name, input, captures_pos)),
        Some(name) => Some(name.clone()),
        None => None,
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuleId(pub u16);

impl RuleId {
    pub fn id(self) -> usize {
        self.0 as usize
    }
}

pub const END_RULE_ID: RuleId = RuleId(u16::MAX);

impl Deref for RuleId {
    type Target = u16;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RegexId(u16);

impl RegexId {
    pub fn id(self) -> usize {
        self.0 as usize
    }
}
impl Deref for RegexId {
    type Target = u16;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RepositoryId(u16);
impl RepositoryId {
    pub fn id(self) -> usize {
        self.0 as usize
    }
}
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
    // The 3 below are not present at runtime
    Self_,
    // TODO: not entirely how $base differs from $self and what it actually means
    // seems like only injection, like calling the parent language when nesting
    Base,
    Local(String),
    // Below are still present at runtime
    OtherComplete(String),
    OtherSpecific(String, String),
    // Pointing to something in the current grammar but we can't find it
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
                Self::OtherSpecific(scope.to_string(), rule.to_string())
            }
            s if s.contains('.') => {
                // Try parsing as scope.repository format (e.g., "source.js.regexp")
                if let Some(dot_pos) = s.rfind('.') {
                    let (scope_part, rule_part) = s.split_at(dot_pos);
                    let rule_part = &rule_part[1..]; // Remove the '.'
                    return Self::OtherSpecific(scope_part.to_string(), rule_part.to_string());
                }
                // Complete scope lookup
                Self::OtherComplete(value.to_string())
            }
            _ => Self::OtherComplete(value.to_string()),
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
    // some match only care about the captures and thus don't have a name themselves
    pub name: Option<String>,
    pub name_is_capturing: bool,
    /// The regex ID for this match rule.
    /// None for scope-only rules (e.g., capture groups that only assign scopes like
    /// punctuation.definition.string.begin without their own pattern to match)
    pub regex_id: Option<RegexId>,
    pub captures: Vec<Option<RuleId>>,
    pub repository_stack: RepositoryStack,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct IncludeOnly {
    pub id: RuleId,
    pub name: Option<String>,
    pub name_is_capturing: bool,
    pub content_name: Option<String>,
    pub content_name_is_capturing: bool,
    pub repository_stack: RepositoryStack,
    pub patterns: Vec<RuleIdOrReference>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct BeginEnd {
    pub id: RuleId,
    pub name: Option<String>,
    pub name_is_capturing: bool,
    pub content_name: Option<String>,
    pub content_name_is_capturing: bool,
    pub begin: RegexId,
    pub begin_captures: Vec<Option<RuleId>>,
    pub end: RegexId,
    pub end_has_backrefs: bool,
    pub end_captures: Vec<Option<RuleId>>,
    pub apply_end_pattern_last: bool,
    pub patterns: Vec<RuleIdOrReference>,
    pub repository_stack: RepositoryStack,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct BeginWhile {
    pub id: RuleId,
    pub name: Option<String>,
    pub name_is_capturing: bool,
    pub content_name: Option<String>,
    pub content_name_is_capturing: bool,
    pub begin: RegexId,
    pub begin_captures: Vec<Option<RuleId>>,
    pub while_: RegexId,
    pub while_has_backrefs: bool,
    pub while_captures: Vec<Option<RuleId>>,
    pub patterns: Vec<RuleIdOrReference>,
    pub repository_stack: RepositoryStack,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum Rule {
    Match(Match),
    IncludeOnly(IncludeOnly),
    BeginEnd(BeginEnd),
    BeginWhile(BeginWhile),
    Noop,
}

impl Rule {
    pub fn original_name(&self) -> Option<&str> {
        match self {
            Rule::Match(m) => m.name.as_deref(),
            Rule::IncludeOnly(m) => m.name.as_deref(),
            Rule::BeginEnd(m) => m.name.as_deref(),
            Rule::BeginWhile(m) => m.name.as_deref(),
            Rule::Noop => None,
        }
    }

    pub fn name(&self, input: &str, captures_pos: &[Option<(usize, usize)>]) -> Option<String> {
        let (name, is_capturing) = match self {
            Rule::Match(m) => (&m.name, m.name_is_capturing),
            Rule::IncludeOnly(i) => (&i.name, i.name_is_capturing),
            Rule::BeginEnd(b) => (&b.name, b.name_is_capturing),
            Rule::BeginWhile(b) => (&b.name, b.name_is_capturing),
            Rule::Noop => return None,
        };

        process_scope_name(name, is_capturing, input, captures_pos)
    }

    pub fn content_name(
        &self,
        input: &str,
        captures_pos: &[Option<(usize, usize)>],
    ) -> Option<String> {
        let (content_name, is_capturing) = match self {
            Rule::IncludeOnly(i) => (&i.content_name, i.content_name_is_capturing),
            Rule::BeginEnd(b) => (&b.content_name, b.content_name_is_capturing),
            Rule::BeginWhile(b) => (&b.content_name, b.content_name_is_capturing),
            _ => return None,
        };

        process_scope_name(content_name, is_capturing, input, captures_pos)
    }

    pub fn apply_end_pattern_last(&self) -> bool {
        match self {
            Rule::BeginEnd(b) => b.apply_end_pattern_last,
            _ => false,
        }
    }

    pub fn end_has_backrefs(&self) -> bool {
        match self {
            Rule::BeginEnd(b) => b.end_has_backrefs,
            Rule::BeginWhile(b) => b.while_has_backrefs,
            _ => false,
        }
    }

    pub fn has_patterns(&self) -> bool {
        match self {
            Rule::Match(_) | Rule::Noop => false,
            Rule::IncludeOnly(b) => b.patterns.len() > 0,
            Rule::BeginEnd(b) => b.patterns.len() > 0,
            Rule::BeginWhile(b) => b.patterns.len() > 0,
        }
    }

    pub fn collect_patterns(&self) -> Vec<RegexId> {
        let mut out = Vec::new();
        match self {
            Rule::Match(m) => {
                if let Some(r) = m.regex_id {
                    out.push(r)
                }
            }
            Rule::IncludeOnly(_) => {}
            Rule::BeginEnd(_) => {}
            Rule::BeginWhile(_) => {}
            Rule::Noop => {}
        }

        out
    }
}

/// TODO: ignore rules from includes we can't find. See markdown fenced block for an example
/// and _compilePatterns in rule.ts in vscode-textmate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledGrammar {
    pub name: String,
    pub display_name: Option<String>,
    pub scope_name: String,
    pub file_types: Vec<String>,
    pub regexes: Vec<Regex>,
    pub rules: Vec<Rule>,
    pub repositories: Vec<Repository>,
}

impl CompiledGrammar {
    pub fn from_raw_grammar(raw: RawGrammar) -> Result<Self, CompileError> {
        let mut grammar = Self {
            name: raw.name,
            display_name: raw.display_name,
            scope_name: raw.scope_name,
            file_types: raw.file_types,
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

        // Resolve all Local references after compilation is complete
        grammar.resolve_local_references();

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
        let name = raw_rule.name;

        // https://github.com/microsoft/vscode-textmate/blob/f03a6a8790af81372d0e81facae75554ec5e97ef/src/rule.ts#L389-L447
        let rule = if let Some(pat) = raw_rule.match_ {
            Rule::Match(Match {
                id,
                name_is_capturing: has_captures(name.as_deref()),
                name,
                regex_id: Some(self.compile_regex(pat).0),
                captures: self.compile_captures(raw_rule.captures, repository_stack)?,
                repository_stack,
            })
        } else if let Some(begin_pat) = raw_rule.begin {
            let content_name = raw_rule.content_name;
            let apply_end_pattern_last = raw_rule.apply_end_pattern_last;
            if let Some(while_pat) = raw_rule.while_ {
                let (while_, while_has_backrefs) = self.compile_regex(while_pat);
                let patterns = self.compile_patterns(raw_rule.patterns, repository_stack)?;
                Rule::BeginWhile(BeginWhile {
                    id,
                    name_is_capturing: has_captures(name.as_deref()),
                    name,
                    content_name_is_capturing: has_captures(content_name.as_deref()),
                    content_name,
                    begin: self.compile_regex(begin_pat).0,
                    begin_captures: self.compile_captures(
                        // Some grammars use "captures" instead of "beginCaptures" for BeginEnd/BeginWhile rules
                        if !raw_rule.begin_captures.is_empty() {
                            raw_rule.begin_captures
                        } else {
                            raw_rule.captures.clone()
                        },
                        repository_stack,
                    )?,
                    while_,
                    while_has_backrefs,
                    while_captures: self.compile_captures(
                        if !raw_rule.while_captures.is_empty() {
                            raw_rule.while_captures
                        } else {
                            raw_rule.captures
                        },
                        repository_stack,
                    )?,
                    patterns,
                    repository_stack,
                })
            } else if let Some(end_pat) = raw_rule.end {
                let (end, end_has_backrefs) = self.compile_regex(end_pat);
                let patterns = self.compile_patterns(raw_rule.patterns, repository_stack)?;
                Rule::BeginEnd(BeginEnd {
                    id,
                    name_is_capturing: has_captures(name.as_deref()),
                    name,
                    content_name_is_capturing: has_captures(content_name.as_deref()),
                    content_name,
                    begin: self.compile_regex(begin_pat).0,
                    begin_captures: self.compile_captures(
                        // Some grammars use "captures" instead of "beginCaptures" for BeginEnd/BeginWhile rules
                        if !raw_rule.begin_captures.is_empty() {
                            raw_rule.begin_captures
                        } else {
                            raw_rule.captures.clone()
                        },
                        repository_stack,
                    )?,
                    end,
                    end_has_backrefs,
                    end_captures: self.compile_captures(
                        if !raw_rule.end_captures.is_empty() {
                            raw_rule.end_captures
                        } else {
                            raw_rule.captures
                        },
                        repository_stack,
                    )?,
                    patterns,
                    apply_end_pattern_last,
                    repository_stack,
                })
            } else {
                // a rule that has begin without while/end is just a match, probably a typo
                Rule::Match(Match {
                    id,
                    name_is_capturing: has_captures(name.as_deref()),
                    name,
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
            if name.is_some() && raw_rule.patterns.is_empty() && raw_rule.include.is_none() {
                // This is a scope-only rule - create a Match rule with no regex
                // This handles captures that only assign scopes
                Rule::Match(Match {
                    id,
                    name_is_capturing: has_captures(name.as_deref()),
                    name,
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
                    Rule::IncludeOnly(IncludeOnly {
                        id,
                        name_is_capturing: has_captures(name.as_deref()),
                        name,
                        content_name_is_capturing: has_captures(raw_rule.content_name.as_deref()),
                        content_name: raw_rule.content_name,
                        repository_stack,
                        patterns: compiled_patterns,
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

    /// Resolve all Local references after compilation is complete.
    /// This must be called after all repositories are fully compiled.
    fn resolve_local_references(&mut self) {
        let convert_local_references = |patterns: &[RuleIdOrReference],
                                        repository_stack: RepositoryStack|
         -> Vec<RuleIdOrReference> {
            patterns
                .iter()
                .filter_map(|pattern| match pattern {
                    RuleIdOrReference::RuleId(rule_id) => Some(RuleIdOrReference::RuleId(*rule_id)),
                    RuleIdOrReference::Reference(reference) => match reference {
                        Reference::Local(name) => {
                            // Walks the repository stack from most recent (top) to oldest (bottom)
                            // and returns the first matching rule ID found.
                            for repo_id in
                                repository_stack.stack.iter().filter(|x| x.is_some()).rev()
                            {
                                let repo = &self.repositories[repo_id.unwrap().id()];
                                if let Some(rule_id) = repo.get(name) {
                                    return Some(RuleIdOrReference::RuleId(*rule_id));
                                }
                            }
                            if cfg!(feature = "debug") {
                                eprintln!("Warning: Local reference '{name}' not found");
                            }
                            None
                        }
                        Reference::Self_ | Reference::Base => {
                            Some(RuleIdOrReference::RuleId(RuleId(0)))
                        }
                        _ => Some(pattern.clone()),
                    },
                })
                .collect()
        };

        for rule in self.rules.iter_mut() {
            match rule {
                Rule::IncludeOnly(x) => {
                    let resolved_patterns =
                        convert_local_references(&x.patterns, x.repository_stack);
                    x.patterns = resolved_patterns;
                }
                Rule::BeginEnd(x) => {
                    let resolved_patterns =
                        convert_local_references(&x.patterns, x.repository_stack);
                    x.patterns = resolved_patterns;
                }
                Rule::BeginWhile(x) => {
                    let resolved_patterns =
                        convert_local_references(&x.patterns, x.repository_stack);
                    x.patterns = resolved_patterns;
                }
                _ => (),
            }
        }
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

                // Parse reference but defer Local resolution to post-compilation pass
                let reference = Reference::from(include.as_str());
                match reference {
                    Reference::Self_ | Reference::Base => {
                        // Self/Base references always resolve to root rule
                        out.push(RuleIdOrReference::RuleId(RuleId(0)));
                    }
                    Reference::Local(_)
                    | Reference::OtherComplete(_)
                    | Reference::OtherSpecific(_, _) => {
                        // Keep all other references for post-compilation resolution
                        out.push(RuleIdOrReference::Reference(reference));
                    }
                    Reference::Unknown(_) => {}
                }
            } else {
                out.push(RuleIdOrReference::RuleId(
                    self.compile_rule(r, repository_stack)?,
                ));
            }
        }

        Ok(out)
    }

    fn get_rule_patterns(
        &self,
        rule_id: RuleId,
        visited: &mut HashSet<RuleId>,
    ) -> Vec<(RuleId, String)> {
        let mut out = vec![];
        if visited.contains(&rule_id) {
            return out;
        }
        visited.insert(rule_id);

        let rule = &self.rules[rule_id.id()];

        match rule {
            Rule::Match(Match { regex_id, .. }) => {
                if let Some(re) = regex_id.and_then(|x| self.regexes.get(x.id())) {
                    out.push((rule_id, re.pattern().to_string()));
                }
            }
            Rule::IncludeOnly(i) => {
                out.extend(self.get_pattern_set_data(&i.patterns, visited));
            }
            Rule::BeginEnd(b) => {
                out.push((rule_id, self.regexes[b.begin.id()].pattern().to_string()))
            }
            Rule::BeginWhile(b) => {
                out.push((rule_id, self.regexes[b.begin.id()].pattern().to_string()))
            }
            Rule::Noop => {}
        }

        out
    }

    fn get_pattern_set_data(
        &self,
        patterns: &[RuleIdOrReference],
        visited: &mut HashSet<RuleId>,
    ) -> Vec<(RuleId, String)> {
        let mut out = vec![];

        for p in patterns {
            match p {
                RuleIdOrReference::RuleId(rule_id) => {
                    let rule_patterns = self.get_rule_patterns(*rule_id, visited);
                    out.extend(rule_patterns)
                }
                RuleIdOrReference::Reference(reference) => {
                    match reference {
                        // Handled at compile time
                        Reference::Base | Reference::Self_ | Reference::Local(_) => {}
                        // TODO
                        Reference::OtherComplete(_) => {}
                        Reference::OtherSpecific(_, _) => {}
                        // We skip those
                        Reference::Unknown(_) => {}
                    }
                }
            }
        }

        out
    }

    pub fn collect_patterns(&self, rule_id: RuleId) -> Vec<(RuleId, String)> {
        // If we have a RuleId, we should have it in our self.rules unless we called the wrong
        // grammar
        let patterns = match &self.rules[rule_id.id()] {
            Rule::Match(_) | Rule::Noop => return vec![],
            Rule::IncludeOnly(a) => &a.patterns,
            Rule::BeginEnd(a) => &a.patterns,
            Rule::BeginWhile(a) => &a.patterns,
        };

        let mut visited = HashSet::new();
        let result = self.get_pattern_set_data(patterns, &mut visited);

        result
    }

    /// Get a pattern set for a rule.
    /// Only IncludeOnly/BeginEnd/BeginWhile have patterns, it will return None for Match/Noop
    pub fn get_pattern_set(&self, rule_id: RuleId) -> Option<PatternSet> {
        // If we have a RuleId, we should have it in our self.rules unless we called the wrong
        // grammar
        let patterns = match &self.rules[rule_id.id()] {
            Rule::Match(_) | Rule::Noop => return None,
            Rule::IncludeOnly(a) => &a.patterns,
            Rule::BeginEnd(a) => &a.patterns,
            Rule::BeginWhile(a) => &a.patterns,
        };

        let mut visited = HashSet::new();
        Some(PatternSet::new(
            self.get_pattern_set_data(patterns, &mut visited),
        ))
    }

    pub(crate) fn get_original_rule_name(&self, rule_id: RuleId) -> Option<&str> {
        self.rules[rule_id.id()].original_name()
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
    use super::{has_captures, replace_captures};
    use std::fs;

    use crate::grammars::raw::RawGrammar;

    #[test]
    fn can_find_captures() {
        let inputs = vec![
            (None, false),
            (Some("source"), false),
            (Some("source.hey"), false),
            (Some("source.hey.$1"), true),
            (Some("keyword.operator.logical.$1.media.css"), true),
            (Some("keyword.control.at-rule.${3:/downcase}.css"), true),
        ];

        for (input, expected) in inputs {
            assert_eq!(has_captures(input), expected);
        }
    }

    #[test]
    fn can_replace_captures() {
        let test_cases = vec![
            // (original_name, text, captures_pos, expected)

            // No captures - should return original
            (
                "source.hey",
                "hello world",
                vec![(0, 11), (0, 5), (6, 11)],
                "source.hey",
            ),
            // Simple capture replacements
            (
                "prefix.$1.suffix",
                "hello",
                vec![(0, 5), (0, 5)],
                "prefix.hello.suffix",
            ),
            (
                "$1_$2",
                "hello world",
                vec![(0, 11), (0, 5), (6, 11)],
                "hello_world",
            ),
            (
                "keyword.operator.logical.$1.media.css",
                "and",
                vec![(0, 3), (0, 3)],
                "keyword.operator.logical.and.media.css",
            ),
            // Transformation captures
            ("${1:/downcase}", "HELLO", vec![(0, 5), (0, 5)], "hello"),
            ("${1:/upcase}", "world", vec![(0, 5), (0, 5)], "WORLD"),
            (
                "keyword.control.at-rule.${1:/downcase}.css",
                "MEDIA",
                vec![(0, 5), (0, 5)],
                "keyword.control.at-rule.media.css",
            ),
            // Mixed simple and transformation captures
            (
                "$1.${2:/upcase}",
                "hello world",
                vec![(0, 11), (0, 5), (6, 11)],
                "hello.WORLD",
            ),
            (
                "${1:/downcase}_$2_${3:/upcase}",
                "Hello big WORLD",
                vec![(0, 15), (0, 5), (6, 9), (10, 15)],
                "hello_big_WORLD",
            ),
            // Leading dots removal
            ("scope.$1", "..method", vec![(0, 8), (0, 8)], "scope.method"),
            (
                "prefix.${1:/downcase}",
                "...CLASS",
                vec![(0, 8), (0, 8)],
                "prefix.class",
            ),
            // Missing captures - should return original capture reference
            ("$1 $5", "hello", vec![(0, 5), (0, 5)], "hello $5"),
            (
                "${3:/downcase}",
                "hello",
                vec![(0, 5), (0, 5)],
                "${3:/downcase}",
            ),
            // Full match (capture 0)
            (
                "match: $0",
                "hello world",
                vec![(0, 11), (0, 5), (6, 11)],
                "match: hello world",
            ),
            // Multiple occurrences of same capture
            (
                "$1.$1.suffix",
                "hello",
                vec![(0, 5), (0, 5)],
                "hello.hello.suffix",
            ),
            (
                "${1:/downcase}.${1:/upcase}",
                "Hello",
                vec![(0, 5), (0, 5)],
                "hello.HELLO",
            ),
        ];

        for (original_name, text, captures_pos, expected) in test_cases {
            // Convert Vec<(usize, usize)> to Vec<Option<(usize, usize)>>
            let option_captures: Vec<Option<(usize, usize)>> =
                captures_pos.into_iter().map(Some).collect();
            let result = replace_captures(original_name, text, &option_captures);
            assert_eq!(
                result, expected,
                "Failed for input: '{}' with text: '{}' - expected: '{}', got: '{}'",
                original_name, text, expected, result
            );
        }
    }

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
}
