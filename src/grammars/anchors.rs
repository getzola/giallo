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

    pub fn allow_a(&self) -> bool {
        matches!(self, AnchorActive::A | AnchorActive::AG)
    }

    pub fn allow_g(&self) -> bool {
        matches!(self, AnchorActive::G | AnchorActive::AG)
    }

    /// The same anchor context with \G deactivated.
    /// RegexSet handles it on its own (inside fancy-regex) but not the normal Regex
    /// we walk through
    pub fn without_g(&self) -> Self {
        match self {
            AnchorActive::A | AnchorActive::AG => AnchorActive::A,
            AnchorActive::G | AnchorActive::None => AnchorActive::None,
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
