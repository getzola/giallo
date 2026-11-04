use std::sync::Arc;

use crate::grammars::engine;
use crate::grammars::{GlobalRuleRef, GrammarId, ROOT_RULE_ID, RuleMatcher};
use crate::scope::ScopeListId;

#[derive(Clone, Debug)]
pub struct StackFrame {
    /// Global rule ref that created this stack element
    pub rule_ref: GlobalRuleRef,
    /// "name" scopes - applied to begin/end delimiters
    /// These scopes are active when matching the rule's boundaries
    pub name_scopes: ScopeListId,
    /// "contentName" scopes - applied to content between delimiters
    /// These scopes are active for the rule's interior content
    pub content_scopes: ScopeListId,
    /// Dynamic end/while pattern resolved with backreferences
    /// For BeginEnd rules: the end pattern with \1, \2, etc. resolved
    /// For BeginWhile rules: the while pattern with backreferences resolved
    pub end_pattern: Option<String>,
    /// The compiled end (or while) regex of this frame
    pub end_regex: Option<Arc<engine::Regex>>,
    /// The matcher for this frame patterns
    pub matcher: Option<Arc<RuleMatcher>>,
    /// The state has entered and captured \n.
    /// This means that the next line should start with an anchor_position of 0.
    pub begin_rule_has_captured_eol: bool,
    /// Where we currently are in a line
    pub anchor_position: Option<usize>,
    /// The position where this rule was entered during current line (for infinite loop detection)
    /// None at beginning of a line
    pub enter_position: Option<usize>,
}

/// Keeps track of nested context as well as how to exit that context and the captures
/// strings used in backreferences.
#[derive(Clone)]
pub struct StateStack {
    /// Stack frames from root to current
    pub frames: Vec<StackFrame>,
}

impl StateStack {
    pub fn new(grammar_id: GrammarId, grammar_scope: ScopeListId) -> Self {
        Self {
            frames: vec![StackFrame {
                rule_ref: GlobalRuleRef {
                    grammar: grammar_id,
                    rule: ROOT_RULE_ID,
                },
                name_scopes: grammar_scope,
                content_scopes: grammar_scope,
                end_pattern: None,
                end_regex: None,
                matcher: None,
                begin_rule_has_captured_eol: false,
                anchor_position: None,
                enter_position: None,
            }],
        }
    }

    /// Called when entering a nested context: when a BeginEnd or BeginWhile begin pattern matches
    pub fn push(
        &mut self,
        rule_ref: GlobalRuleRef,
        anchor_position: Option<usize>,
        begin_rule_has_captured_eol: bool,
        enter_position: Option<usize>,
    ) {
        let content_scopes = self.top().content_scopes;

        self.frames.push(StackFrame {
            rule_ref,
            // Start with the same scope they will diverge later
            name_scopes: content_scopes,
            content_scopes,
            end_pattern: None,
            end_regex: None,
            matcher: None,
            begin_rule_has_captured_eol,
            anchor_position,
            enter_position,
        });
    }

    pub fn push_with_scopes(
        &mut self,
        rule_ref: GlobalRuleRef,
        anchor_position: Option<usize>,
        begin_rule_has_captured_eol: bool,
        enter_position: Option<usize>,
        scopes: ScopeListId,
    ) {
        self.frames.push(StackFrame {
            rule_ref,
            name_scopes: scopes,
            content_scopes: scopes,
            end_pattern: None,
            end_regex: None,
            matcher: None,
            begin_rule_has_captured_eol,
            anchor_position,
            enter_position,
        });
    }

    pub fn set_content_scopes(&mut self, content_scopes: ScopeListId) {
        self.top_mut().content_scopes = content_scopes;
    }

    pub fn set_end_pattern(&mut self, end_pattern: String) {
        let top = self.top_mut();
        top.end_pattern = Some(end_pattern);
        top.end_regex = None;
    }

    /// Exits the current context, getting back to the parent
    pub fn pop(&mut self) -> Option<StackFrame> {
        if self.frames.len() > 1 {
            self.frames.pop()
        } else {
            None
        }
    }

    /// Pop but never go below root state - used in infinite loop protection
    pub fn safe_pop(&mut self) {
        if self.frames.len() > 1 {
            self.frames.pop();
        }
    }

    /// Resets enter_position/anchor_position for all stack elements to None
    pub fn reset(&mut self) {
        for frame in &mut self.frames {
            frame.enter_position = None;
            frame.anchor_position = None;
        }
    }

    /// Access the top frame of the stack
    pub fn top(&self) -> &StackFrame {
        self.frames.last().expect("stack never empty")
    }

    /// Mutable access to the top frame of the stack
    pub fn top_mut(&mut self) -> &mut StackFrame {
        self.frames.last_mut().expect("stack never empty")
    }
}
