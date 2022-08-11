use std::collections::HashMap;
use std::path::Path;


#[derive(Debug)]
pub struct Theme {
    name: String,
    // editor.foreground in VSCode
    pub foreground: String,
    // editor.background in VSCode
    pub background: String,
    // editor.selectionBackground in VSCode
    pub highlight_background: String,
    // editorLineNumber.foreground in VSCode
    pub line_number_foreground: String,
    // TODO: keep size in sync with SCOPES
    scope_colors: [Option<String>; 17]
}

impl Theme {
    pub fn new() -> Self {
        pub(crate) const SCOPES: &[&str] = &[
            "constant",
            "type",
            "type.builtin",
            "property",
            "comment",
            "constructor",
            "function",
            "label",
            "keyword",
            "string",
            "variable",
            "variable.other.member",
            "operator",
            "attribute",
            "escape",
            "embedded",
            "symbol",
        ];
        Self {
            name: String::from("OneDark-Pro"),
            foreground: String::from("#abb2bf"),
            background: String::from("#282c34"),
            // TODO: convert to rgba when opacity is included in background of highlight
            // highlight_background: String::from("#67769660"),
            highlight_background: String::from("#677696"),
            line_number_foreground: String::from("#495162"),
            scope_colors: [
                // constant
                Some(String::from("#d19a66")),
                // type
                Some(String::from("#e5c07b")),
                // type.builtin
                Some(String::from("#e5c07b")),
                // property
                Some(String::from("#e5c07b")),
                // comment
                Some(String::from("#7f848e")),
                // constructor
                Some(String::from("#61afef")),
                // function
                Some(String::from("#61afef")),
                // label
                Some(String::from("#61afef")),
                // keyword
                Some(String::from("#c678dd")),
                // string
                Some(String::from("#98c379")),
                // variable
                Some(String::from("#e06c75")),
                // variable.other.member
                Some(String::from("#e06c75")),
                // operator
                Some(String::from("#56b6c2")),
                // attribute
                Some(String::from("#d19a66")),
                // escape
                Some(String::from("#56b6c2")),
                // embededd
                Some(String::from("#abb2bf")),
                // symbol
                Some(String::from("#56b6c2")),
            ]
        }
    }

    pub fn get_foreground(&self, scope_idx: usize) -> &str {
        if let Some(color) = &self.scope_colors[scope_idx] {
            return color;
        }

        &self.foreground
    }
}
