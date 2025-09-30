use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use serde::Deserialize;

use crate::textmate::grammar::{ScopeId, get_scope_id};

/// A color in CSS format (#RRGGBB)
pub type Color = String;

/// Font style flags
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FontStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

impl Default for FontStyle {
    fn default() -> Self {
        Self {
            bold: false,
            italic: false,
            underline: false,
        }
    }
}

/// A computed style for a token
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Style {
    pub foreground: Option<Color>,
    pub background: Option<Color>,
    pub font_style: FontStyle,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            foreground: None,
            background: None,
            font_style: FontStyle::default(),
        }
    }
}

/// Style ID for efficient storage and comparison
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StyleId(pub u32);

/// Token color settings from VSCode theme
#[derive(Debug, Clone, Deserialize)]
pub struct TokenColorSettings {
    pub foreground: Option<Color>,
    pub background: Option<Color>,
    #[serde(rename = "fontStyle")]
    pub font_style: Option<String>,
}

/// A single token color rule from VSCode theme
#[derive(Debug, Clone, Deserialize)]
pub struct TokenColorRule {
    /// Scope(s) this rule applies to - can be a string or array of strings
    #[serde(deserialize_with = "deserialize_scope")]
    pub scope: Vec<String>,
    pub settings: TokenColorSettings,
}

/// Raw theme loaded from VSCode theme JSON
#[derive(Debug, Clone, Deserialize)]
pub struct RawTheme {
    pub name: Option<String>,
    pub type_: Option<String>,
    #[serde(rename = "tokenColors")]
    pub token_colors: Vec<TokenColorRule>,
}

/// Compiled theme rule for efficient matching
#[derive(Debug, Clone)]
pub struct CompiledThemeRule {
    /// Compiled scope patterns - each pattern is a sequence of scope IDs
    pub scope_patterns: Vec<Vec<ScopeId>>,
    /// The style to apply
    pub style: Style,
}

/// Compiled theme optimized for fast lookups
#[derive(Debug, Clone)]
pub struct CompiledTheme {
    pub name: String,
    /// Default style for tokens with no specific rules
    pub default_style: Style,
    /// Theme rules sorted by specificity (most specific first)
    pub rules: Vec<CompiledThemeRule>,
}

/// Simple cache for style lookups
pub struct StyleCache {
    /// Cache from scope stack hash to style ID
    cache: HashMap<u64, StyleId>,
    /// Style registry (StyleId -> Style)
    styles: HashMap<StyleId, Style>,
    /// Next style ID to assign
    next_id: u32,
}

impl StyleCache {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            styles: HashMap::new(),
            next_id: 1, // Start from 1, 0 can be reserved for "no style"
        }
    }

    /// Get style ID for a scope stack, computing and caching if needed
    pub fn get_style_id(&mut self, scope_stack: &[ScopeId], theme: &CompiledTheme) -> StyleId {
        // Hash the scope stack
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        scope_stack.hash(&mut hasher);
        let hash = hasher.finish();

        // Check cache first
        if let Some(&style_id) = self.cache.get(&hash) {
            return style_id;
        }

        // Compute style and cache it
        let style = self.compute_style(scope_stack, theme);
        let style_id = self.get_or_create_style_id(style);
        self.cache.insert(hash, style_id);
        style_id
    }

    /// Get the Style for a given StyleId
    pub fn get_style(&self, style_id: StyleId) -> Option<&Style> {
        self.styles.get(&style_id)
    }

    /// Get or create a StyleId for a given Style
    fn get_or_create_style_id(&mut self, style: Style) -> StyleId {
        // Look for existing style
        for (&id, existing_style) in &self.styles {
            if *existing_style == style {
                return id;
            }
        }

        // Create new style ID
        let style_id = StyleId(self.next_id);
        self.next_id += 1;
        self.styles.insert(style_id, style);
        style_id
    }

    /// Compute style for a scope stack using theme rules
    fn compute_style(&self, scope_stack: &[ScopeId], theme: &CompiledTheme) -> Style {
        let mut style = theme.default_style.clone();

        // Find the most specific matching rule
        for rule in &theme.rules {
            if self.matches_scope_stack(scope_stack, &rule.scope_patterns) {
                // Merge the rule's style with current style
                if rule.style.foreground.is_some() {
                    style.foreground = rule.style.foreground.clone();
                }
                if rule.style.background.is_some() {
                    style.background = rule.style.background.clone();
                }
                // Font styles are additive
                if rule.style.font_style.bold {
                    style.font_style.bold = true;
                }
                if rule.style.font_style.italic {
                    style.font_style.italic = true;
                }
                if rule.style.font_style.underline {
                    style.font_style.underline = true;
                }

                // For now, take the first matching rule
                // TODO: Implement proper specificity ordering
                break;
            }
        }

        style
    }

    /// Check if a scope stack matches any of the scope patterns
    fn matches_scope_stack(&self, scope_stack: &[ScopeId], patterns: &[Vec<ScopeId>]) -> bool {
        for pattern in patterns {
            if self.matches_pattern(scope_stack, pattern) {
                return true;
            }
        }
        false
    }

    /// Check if a scope stack matches a specific pattern
    fn matches_pattern(&self, scope_stack: &[ScopeId], pattern: &[ScopeId]) -> bool {
        // Check if pattern matches the scope stack exactly (for now - we can make this more sophisticated later)
        // For better specificity, we should prefer exact matches
        if scope_stack == pattern {
            return true;
        }

        // Fallback: check if pattern is a suffix of scope_stack
        // This handles hierarchical scopes like "string" matching ["source.js", "string.quoted"]
        if scope_stack.len() >= pattern.len() {
            let suffix = &scope_stack[scope_stack.len() - pattern.len()..];
            return suffix == pattern;
        }

        false
    }
}

/// Custom deserializer for scope field that can be string or array
fn deserialize_scope<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    use std::fmt;

    struct ScopeVisitor;

    impl<'de> Visitor<'de> for ScopeVisitor {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("string or array of strings")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(vec![value.to_owned()])
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut vec = Vec::new();
            while let Some(item) = seq.next_element::<String>()? {
                vec.push(item);
            }
            Ok(vec)
        }
    }

    deserializer.deserialize_any(ScopeVisitor)
}

impl RawTheme {
    /// Load a theme from a VSCode theme JSON file
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let file = File::open(path)?;
        let theme = serde_json::from_reader(file)?;
        Ok(theme)
    }

    /// Load a built-in theme by name (e.g., "material-theme")
    pub fn load_builtin(name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let theme_path = format!("grammars-themes/packages/tm-themes/themes/{}.json", name);
        Self::load_from_file(theme_path)
    }

    /// Compile this raw theme into an optimized compiled theme
    pub fn compile(&self) -> Result<CompiledTheme, Box<dyn std::error::Error>> {
        let mut rules = Vec::new();
        let mut default_style = Style::default();

        for token_rule in &self.token_colors {
            // Handle default style (rule with no scope)
            if token_rule.scope.is_empty() {
                default_style = convert_settings_to_style(&token_rule.settings);
                continue;
            }

            // Compile scope patterns
            let mut scope_patterns = Vec::new();
            for scope_str in &token_rule.scope {
                let pattern = compile_scope_pattern(scope_str)?;
                if !pattern.is_empty() {
                    scope_patterns.push(pattern);
                }
            }

            if !scope_patterns.is_empty() {
                let style = convert_settings_to_style(&token_rule.settings);
                rules.push(CompiledThemeRule {
                    scope_patterns,
                    style,
                });
            }
        }

        Ok(CompiledTheme {
            name: self.name.clone().unwrap_or_else(|| "Unknown Theme".to_string()),
            default_style,
            rules,
        })
    }
}

/// Convert TokenColorSettings to Style
fn convert_settings_to_style(settings: &TokenColorSettings) -> Style {
    let mut font_style = FontStyle::default();

    if let Some(font_style_str) = &settings.font_style {
        font_style.bold = font_style_str.contains("bold");
        font_style.italic = font_style_str.contains("italic");
        font_style.underline = font_style_str.contains("underline");
    }

    Style {
        foreground: settings.foreground.clone(),
        background: settings.background.clone(),
        font_style,
    }
}

/// Compile a scope string into a pattern of ScopeIds
fn compile_scope_pattern(scope_str: &str) -> Result<Vec<ScopeId>, Box<dyn std::error::Error>> {
    let mut pattern = Vec::new();

    // Split scope string by dots to create pattern
    // e.g., "string.quoted.double" -> ["string", "string.quoted", "string.quoted.double"]
    let parts: Vec<&str> = scope_str.split('.').collect();
    let mut accumulated = String::new();

    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            accumulated.push('.');
        }
        accumulated.push_str(part);

        if let Some(scope_id) = get_scope_id(&accumulated) {
            pattern.push(scope_id);
        }
    }

    Ok(pattern)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_deserialize_string() {
        let json = r##"{"scope": "string", "settings": {"foreground": "#C3E88D"}}"##;
        let rule: TokenColorRule = serde_json::from_str(json).unwrap();
        assert_eq!(rule.scope, vec!["string"]);
    }

    #[test]
    fn test_scope_deserialize_array() {
        let json = r##"{"scope": ["string", "constant"], "settings": {"foreground": "#C3E88D"}}"##;
        let rule: TokenColorRule = serde_json::from_str(json).unwrap();
        assert_eq!(rule.scope, vec!["string", "constant"]);
    }

    #[test]
    fn test_font_style_parsing() {
        let settings = TokenColorSettings {
            foreground: Some("#FFFFFF".to_string()),
            background: None,
            font_style: Some("bold italic".to_string()),
        };

        let style = convert_settings_to_style(&settings);
        assert!(style.font_style.bold);
        assert!(style.font_style.italic);
        assert!(!style.font_style.underline);
    }
}