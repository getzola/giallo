use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::common::Regex;
use super::ScopeId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledCapture {
    pub scope_id: ScopeId,
    #[serde(default)]
    pub patterns: Vec<CompiledPattern>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledMatchPattern {
    pub name_scope_id: Option<ScopeId>,
    pub regex: Regex,
    #[serde(default)]
    pub captures: BTreeMap<String, CompiledCapture>,
    #[serde(default)]
    pub patterns: Vec<CompiledPattern>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledBeginEndPattern {
    pub name_scope_id: Option<ScopeId>,
    pub content_name_scope_id: Option<ScopeId>,
    pub begin_regex: Regex,
    pub end_regex: Regex,
    /// The original end pattern string (may contain unresolved backreferences)
    pub end_pattern_source: String,
    #[serde(default)]
    pub captures: BTreeMap<String, CompiledCapture>,
    #[serde(default)]
    pub begin_captures: BTreeMap<String, CompiledCapture>,
    #[serde(default)]
    pub end_captures: BTreeMap<String, CompiledCapture>,
    #[serde(default)]
    pub patterns: Vec<CompiledPattern>,
    #[serde(default)]
    pub apply_end_pattern_last: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledBeginWhilePattern {
    pub name_scope_id: Option<ScopeId>,
    pub content_name_scope_id: Option<ScopeId>,
    pub begin_regex: Regex,
    pub while_regex: Regex,
    /// The original while pattern string (may contain unresolved backreferences)
    pub while_pattern_source: String,
    #[serde(default)]
    pub captures: BTreeMap<String, CompiledCapture>,
    #[serde(default)]
    pub begin_captures: BTreeMap<String, CompiledCapture>,
    #[serde(default)]
    pub while_captures: BTreeMap<String, CompiledCapture>,
    #[serde(default)]
    pub patterns: Vec<CompiledPattern>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledIncludePattern {
    /// The resolved patterns from the include reference
    pub patterns: Vec<CompiledPattern>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CompiledPattern {
    BeginEnd(CompiledBeginEndPattern),
    BeginWhile(CompiledBeginWhilePattern),
    Match(CompiledMatchPattern),
    Include(CompiledIncludePattern),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledGrammar {
    pub name: String,
    pub display_name: Option<String>,
    pub scope_name: String,
    pub scope_id: ScopeId,
    pub file_types: Vec<String>,
    pub patterns: Vec<CompiledPattern>,
    pub first_line_regex: Option<Regex>,
}
