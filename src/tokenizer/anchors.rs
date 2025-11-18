use std::borrow::Cow;
use std::fmt;

/// We use that as a way to convey both the rule and which anchors should be active
/// in regexes. We don't want to enable \A or \G everywhere, it's context dependent.
#[derive(Copy, Clone, PartialEq, Hash, Eq)]
pub enum AnchorActive {
    /// Only \A is active
    A,
    /// Only \G is active
    G,
    /// Both \A and \G are active
    AG,
    /// Neither \A nor \G are active
    None,
}

impl AnchorActive {
    pub fn new(is_first_line: bool, anchor_position: Option<usize>, current_pos: usize) -> Self {
        let g_active = if let Some(a_pos) = anchor_position {
            a_pos == current_pos
        } else {
            false
        };

        if is_first_line {
            if g_active {
                AnchorActive::AG
            } else {
                AnchorActive::A
            }
        } else if g_active {
            AnchorActive::G
        } else {
            AnchorActive::None
        }
    }

    /// This follows vscode-textmate and replaces it with something that is very unlikely
    /// to match
    pub fn replace_anchors<'a>(&self, pat: &'a str) -> Cow<'a, str> {
        match self {
            AnchorActive::AG => {
                // No replacements needed
                Cow::Borrowed(pat)
            }
            AnchorActive::A => {
                if pat.contains("\\G") {
                    Cow::Owned(pat.replace("\\G", "\u{FFFF}"))
                } else {
                    Cow::Borrowed(pat)
                }
            }
            AnchorActive::G => {
                if pat.contains("\\A") {
                    Cow::Owned(pat.replace("\\A", "\u{FFFF}"))
                } else {
                    Cow::Borrowed(pat)
                }
            }
            AnchorActive::None => {
                if pat.contains("\\A") || pat.contains("\\G") {
                    Cow::Owned(pat.replace("\\A", "\u{FFFF}").replace("\\G", "\u{FFFF}"))
                } else {
                    Cow::Borrowed(pat)
                }
            }
        }
    }
}

impl fmt::Debug for AnchorActive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            AnchorActive::A => "allow_A=true, allow_G=false",
            AnchorActive::G => "allow_A=false, allow_G=true",
            AnchorActive::AG => "allow_A=true, allow_G=true",
            AnchorActive::None => "allow_A=false, allow_G=false",
        };
        f.write_str(s)
    }
}
