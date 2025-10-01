use std::fmt;
use std::fs::File;
use std::path::Path;

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, de};

use crate::themes::compiled::CompiledTheme;

/// Token color settings from VSCode theme JSON
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TokenColorSettings {
    foreground: Option<String>,
    background: Option<String>,
    #[serde(rename = "fontStyle")]
    pub font_style: Option<String>,
}

impl TokenColorSettings {
    pub fn foreground(&self) -> Option<&str> {
        if let Some(s) = &self.foreground {
            if s == "inherit" { None } else { Some(s) }
        } else {
            None
        }
    }

    pub fn background(&self) -> Option<&str> {
        if let Some(s) = &self.background {
            if s == "inherit" { None } else { Some(s) }
        } else {
            None
        }
    }
}

/// Custom deserializer for scope field that can be string or array
fn deserialize_string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Colors {
    pub foreground: String,
    pub background: String,
}

// Some themes have it as editor.foreground/background some don't have the editor. prefix
impl<'de> Deserialize<'de> for Colors {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ColorsVisitor;

        impl<'de> Visitor<'de> for ColorsVisitor {
            type Value = Colors;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct Colors")
            }

            fn visit_map<V>(self, mut map: V) -> Result<Colors, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut foreground = None;
                let mut background = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "foreground" | "editor.foreground" => {
                            if foreground.is_none() {
                                foreground = Some(map.next_value()?);
                            } else {
                                // Skip the value if we already have one
                                let _: serde::de::IgnoredAny = map.next_value()?;
                            }
                        }
                        "background" | "editor.background" => {
                            if background.is_none() {
                                background = Some(map.next_value()?);
                            } else {
                                // Skip the value if we already have one
                                let _: serde::de::IgnoredAny = map.next_value()?;
                            }
                        }
                        _ => {
                            // Skip unknown fields
                            let _: serde::de::IgnoredAny = map.next_value()?;
                        }
                    }
                }

                let foreground = foreground
                    .ok_or_else(|| de::Error::missing_field("foreground or editor.foreground"))?;
                let background = background
                    .ok_or_else(|| de::Error::missing_field("background or editor.background"))?;

                Ok(Colors {
                    foreground,
                    background,
                })
            }
        }

        deserializer.deserialize_map(ColorsVisitor)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenColorRule {
    #[serde(deserialize_with = "deserialize_string_or_vec", default)]
    pub scope: Vec<String>,
    #[serde(default)]
    pub settings: TokenColorSettings,
}

impl TokenColorRule {
    pub fn get_scope_patterns(&self) -> Vec<Vec<String>> {
        let mut out = Vec::new();

        for s in &self.scope {
            let mut inner = Vec::new();
            let parts: Vec<&str> = s.split('.').collect();
            let mut accumulated = String::new();

            for (i, part) in parts.iter().enumerate() {
                if i > 0 {
                    accumulated.push('.');
                }
                accumulated.push_str(part);
                inner.push(accumulated.clone());
            }

            out.push(inner);
        }

        out
    }
}

/// Raw theme loaded from a JSON theme file
#[derive(Debug, Clone, Deserialize)]
pub struct RawTheme {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub colors: Colors,
    /// Token color rules for syntax highlighting
    #[serde(rename = "tokenColors")]
    pub token_colors: Vec<TokenColorRule>,
}

impl RawTheme {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let file = File::open(path)?;
        let theme = serde_json::from_reader(file)?;
        Ok(theme)
    }

    /// Compile this raw grammar into an optimized compiled grammar
    pub fn compile(self) -> Result<CompiledTheme, Box<dyn std::error::Error>> {
        CompiledTheme::from_raw_theme(self)
    }
}
