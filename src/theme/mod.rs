use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use serde::Deserialize;

use crate::color::{Color, ParseColorError};
use crate::style::{FontStyle, Style, StyleModifier};
use crate::textmate::grammar::{get_scope_id, ScopeId};

/// Token color settings from VSCode theme JSON
#[derive(Debug, Clone, Deserialize)]
pub struct TokenColorSettings {
    pub foreground: Option<String>,
    pub background: Option<String>,
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
    #[serde(rename = "type")]
    pub type_: Option<String>,
    /// Editor colors like "editor.foreground", "editor.background", etc.
    pub colors: Option<HashMap<String, String>>,
    /// Token color rules for syntax highlighting
    #[serde(rename = "tokenColors")]
    pub token_colors: Vec<TokenColorRule>,
}

/// Compiled theme rule for efficient matching
#[derive(Debug, Clone)]
pub struct CompiledThemeRule {
    /// Compiled scope patterns - each pattern is a sequence of scope IDs
    pub scope_patterns: Vec<Vec<ScopeId>>,
    /// The style modifier to apply
    pub style_modifier: StyleModifier,
}

/// Compiled theme optimized for fast lookups
#[derive(Debug, Clone)]
pub struct CompiledTheme {
    pub name: String,
    /// Theme type ("light" or "dark")
    pub theme_type: ThemeType,
    /// Editor colors (editor.foreground, editor.background, etc.)
    pub colors: HashMap<String, Color>,
    /// Default style for tokens with no specific rules
    pub default_style: Style,
    /// Theme rules sorted by specificity (most specific first)
    pub rules: Vec<CompiledThemeRule>,
}

/// Theme type for determining fallback colors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeType {
    Light,
    Dark,
}

impl Default for ThemeType {
    fn default() -> Self {
        ThemeType::Dark
    }
}

/// Style ID for efficient storage and comparison
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StyleId(pub u32);

/// Simple cache for style lookups with two-level caching
pub struct StyleCache {
    /// L1 cache: Recent 4 lookups (no HashMap overhead)
    recent: [(u64, StyleId); 4],
    /// L2 cache: Full HashMap cache
    cache: HashMap<u64, StyleId>,
    /// Style registry (StyleId -> Style)
    styles: HashMap<StyleId, Style>,
    /// Next style ID to assign
    next_id: u32,
    /// Recent cache index (round-robin)
    recent_index: usize,
}

impl StyleCache {
    pub fn new() -> Self {
        Self {
            recent: [(0, StyleId(0)); 4],
            cache: HashMap::new(),
            styles: HashMap::new(),
            next_id: 1, // Start from 1, 0 can be reserved for "no style"
            recent_index: 0,
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

        // Check L1 cache first
        for &(cached_hash, style_id) in &self.recent {
            if cached_hash == hash {
                return style_id;
            }
        }

        // Check L2 cache
        if let Some(&style_id) = self.cache.get(&hash) {
            // Move to L1 cache
            self.recent[self.recent_index] = (hash, style_id);
            self.recent_index = (self.recent_index + 1) % 4;
            return style_id;
        }

        // Compute style and cache it
        let style = self.compute_style(scope_stack, theme);
        let style_id = self.get_or_create_style_id(style);

        // Cache in both levels
        self.cache.insert(hash, style_id);
        self.recent[self.recent_index] = (hash, style_id);
        self.recent_index = (self.recent_index + 1) % 4;

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
        let mut style = theme.default_style;

        // Find matching rules and apply them
        for rule in &theme.rules {
            if self.matches_scope_stack(scope_stack, &rule.scope_patterns) {
                style = style.apply(rule.style_modifier);
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
        // Check if pattern matches the scope stack exactly
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

    /// Determine if this is a dark theme based on type or name
    fn determine_theme_type(&self) -> ThemeType {
        if let Some(type_str) = &self.type_ {
            if type_str.eq_ignore_ascii_case("light") {
                return ThemeType::Light;
            }
        }

        // Fallback to name-based detection
        if let Some(name) = &self.name {
            let name_lower = name.to_lowercase();
            if name_lower.contains("light") || name_lower.contains("day") {
                return ThemeType::Light;
            }
        }

        // Default to dark theme
        ThemeType::Dark
    }

    /// Compile this raw theme into an optimized compiled theme
    pub fn compile(&self) -> Result<CompiledTheme, Box<dyn std::error::Error>> {
        let mut rules = Vec::new();
        let theme_type = self.determine_theme_type();

        // Parse editor colors
        let colors = self.parse_colors()?;

        // Create default style using Shiki's approach
        let default_style = self.create_default_style(&colors, theme_type)?;

        for token_rule in &self.token_colors {
            // Handle default style (rule with no scope)
            if token_rule.scope.is_empty() {
                // Override default style if there's a global token color rule
                // TODO: Merge with existing default style
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
                let style_modifier = convert_settings_to_style_modifier(&token_rule.settings)?;
                rules.push(CompiledThemeRule {
                    scope_patterns,
                    style_modifier,
                });
            }
        }

        Ok(CompiledTheme {
            name: self
                .name
                .clone()
                .unwrap_or_else(|| "Unknown Theme".to_string()),
            theme_type,
            colors,
            default_style,
            rules,
        })
    }

    /// Parse colors from theme, handling hex color validation
    fn parse_colors(&self) -> Result<HashMap<String, Color>, Box<dyn std::error::Error>> {
        let mut parsed_colors = HashMap::new();

        if let Some(colors) = &self.colors {
            for (key, value) in colors {
                match Color::from_hex(value) {
                    Ok(color) => {
                        parsed_colors.insert(key.clone(), color);
                    }
                    Err(_) => {
                        // Skip invalid colors but don't fail the entire theme
                        eprintln!("Warning: Invalid color '{}' for key '{}'", value, key);
                    }
                }
            }
        }

        Ok(parsed_colors)
    }

    /// Create default style following Shiki's approach
    fn create_default_style(
        &self,
        colors: &HashMap<String, Color>,
        theme_type: ThemeType,
    ) -> Result<Style, Box<dyn std::error::Error>> {
        // Primary: use editor colors from theme
        let foreground = colors.get("editor.foreground").copied().unwrap_or_else(|| {
            // Theme-type-based fallbacks (like Shiki)
            match theme_type {
                ThemeType::Dark => Color::DARK_FG_DEFAULT,
                ThemeType::Light => Color::LIGHT_FG_DEFAULT,
            }
        });

        let background =
            colors
                .get("editor.background")
                .copied()
                .unwrap_or_else(|| match theme_type {
                    ThemeType::Dark => Color::DARK_BG_DEFAULT,
                    ThemeType::Light => Color::LIGHT_BG_DEFAULT,
                });

        Ok(Style::new(foreground, background, FontStyle::empty()))
    }
}

/// Convert TokenColorSettings to StyleModifier
fn convert_settings_to_style_modifier(
    settings: &TokenColorSettings,
) -> Result<StyleModifier, ParseColorError> {
    let foreground = if let Some(fg_str) = &settings.foreground {
        Some(Color::from_hex(fg_str)?)
    } else {
        None
    };

    let background = if let Some(bg_str) = &settings.background {
        Some(Color::from_hex(bg_str)?)
    } else {
        None
    };

    let font_style = if let Some(font_style_str) = &settings.font_style {
        Some(parse_font_style(font_style_str))
    } else {
        None
    };

    Ok(StyleModifier {
        foreground,
        background,
        font_style,
    })
}

/// Parse font style string into FontStyle bitflags
fn parse_font_style(font_style_str: &str) -> FontStyle {
    let mut font_style = FontStyle::empty();

    if font_style_str.contains("bold") {
        font_style |= FontStyle::BOLD;
    }
    if font_style_str.contains("italic") {
        font_style |= FontStyle::ITALIC;
    }
    if font_style_str.contains("underline") {
        font_style |= FontStyle::UNDERLINE;
    }

    font_style
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
        let font_style = parse_font_style("bold italic");
        assert!(font_style.contains(FontStyle::BOLD));
        assert!(font_style.contains(FontStyle::ITALIC));
        assert!(!font_style.contains(FontStyle::UNDERLINE));
    }

    #[test]
    fn test_theme_type_detection() {
        let dark_theme = RawTheme {
            name: Some("Dark Plus".to_string()),
            type_: None,
            colors: None,
            token_colors: vec![],
        };
        assert_eq!(dark_theme.determine_theme_type(), ThemeType::Dark);

        let light_theme = RawTheme {
            name: None,
            type_: Some("light".to_string()),
            colors: None,
            token_colors: vec![],
        };
        assert_eq!(light_theme.determine_theme_type(), ThemeType::Light);
    }

    #[test]
    fn test_style_cache_l1_cache() {
        let theme = CompiledTheme {
            name: "Test".to_string(),
            theme_type: ThemeType::Dark,
            colors: HashMap::new(),
            default_style: Style::default(),
            rules: vec![],
        };

        let mut cache = StyleCache::new();
        let scope_stack = vec![ScopeId(1), ScopeId(2)];

        // First lookup should compute and cache
        let style_id1 = cache.get_style_id(&scope_stack, &theme);

        // Second lookup should hit L1 cache
        let style_id2 = cache.get_style_id(&scope_stack, &theme);

        assert_eq!(style_id1, style_id2);
    }
}
