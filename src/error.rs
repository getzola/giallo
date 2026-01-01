use std::fmt;
use std::io;

pub(crate) type GialloResult<T> = Result<T, Error>;

/// Errors that can occur during giallo usage
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// An I/O error occurred when reading a grammar of theme file
    /// or a dump file if the `dump` feature is enabled
    Io(io::Error),

    /// JSON parsing failed when loading a grammar or a theme.
    Json(serde_json::Error),

    /// MessagePack encoding failed.
    #[cfg(feature = "dump")]
    MsgPackEncode(rmp_serde::encode::Error),

    /// MessagePack decoding failed.
    #[cfg(feature = "dump")]
    MsgPackDecode(rmp_serde::decode::Error),

    /// An invalid hex color was encountered.
    /// Can only happen when loading a theme.
    #[allow(missing_docs)]
    InvalidHexColor { value: String, reason: String },

    /// A grammar was not found in the registry.
    /// Only happens when asking to highlight something with a grammar we can't find
    GrammarNotFound(String),

    /// A theme was not found in the registry.
    /// Only happens when asking to highlight something with a theme we can't find
    ThemeNotFound(String),

    /// A regex compilation error occurred during tokenization.
    /// This can happen because some regex patterns are modified at runtime so we can't validate
    /// them all ahead.
    TokenizeRegex(String),

    /// Tried to highlight some content before calling `registry.link_grammars()`
    /// This might result in broken highlighting for some languages
    UnlinkedGrammars,

    /// Tried to replace a grammar in the registry after calling `registry.link_grammars()`.
    /// External references to the original grammar will have
    ReplacingGrammarPostLinking(String),

    /// Tried to add two grammars with the same file type.
    ConflictingFileType(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(err) => write!(f, "I/O error: {}", err),
            Error::Json(err) => write!(f, "JSON parsing error: {}", err),
            #[cfg(feature = "dump")]
            Error::MsgPackEncode(err) => write!(f, "MessagePack encoding error: {}", err),
            #[cfg(feature = "dump")]
            Error::MsgPackDecode(err) => write!(f, "MessagePack decoding error: {}", err),
            Error::InvalidHexColor { value, reason } => {
                write!(f, "invalid hex color '{}': {}", value, reason)
            }
            Error::GrammarNotFound(name) => write!(f, "grammar '{}' not found", name),
            Error::ThemeNotFound(name) => write!(f, "theme '{}' not found", name),
            Error::TokenizeRegex(message) => write!(f, "regex compilation error: {}", message),
            Error::UnlinkedGrammars => {
                write!(f, "grammars are unlinked, call `registry.link_grammars()`")
            }
            Error::ReplacingGrammarPostLinking(s) => {
                write!(f, "Tried to replace grammar `{s}` after linking")
            }
            Error::ConflictingFileType(s) => {
                write!(f, "Conflicting file type `{s}`")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(err) => Some(err),
            Error::Json(err) => Some(err),
            #[cfg(feature = "dump")]
            Error::MsgPackEncode(err) => Some(err),
            #[cfg(feature = "dump")]
            Error::MsgPackDecode(err) => Some(err),
            Error::InvalidHexColor { .. }
            | Error::UnlinkedGrammars
            | Error::ReplacingGrammarPostLinking(_)
            | Error::ConflictingFileType(_)
            | Error::GrammarNotFound(_)
            | Error::ThemeNotFound(_)
            | Error::TokenizeRegex(_) => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Json(err)
    }
}

#[cfg(feature = "dump")]
impl From<rmp_serde::encode::Error> for Error {
    fn from(err: rmp_serde::encode::Error) -> Self {
        Error::MsgPackEncode(err)
    }
}

#[cfg(feature = "dump")]
impl From<rmp_serde::decode::Error> for Error {
    fn from(err: rmp_serde::decode::Error) -> Self {
        Error::MsgPackDecode(err)
    }
}
