//! A syntax highlighting library using TextMate grammars and themes and producing the same
//! output as VSCode.
//!
//! # Example
//!
//! ```ignore
//! use giallo::{HighlightOptions, HtmlRenderer, Options, Registry};
//!
//! // Using the `dump` feature and loading the prebuilt assets
//! let registry = Registry::load_from_file("builtin.msgpack")?;
//! let code = "let x = 42;";
//!
//! let options = HighlightOptions::new("javascript").single_theme("catppuccin-frappe");
//! let highlighted = registry.highlight(code, options)?;
//!
//! let render_options = Options {
//!     show_line_numbers: true,
//!     ..Default::default()
//! };
//! let html = HtmlRenderer::default().render(&highlighted, &render_options);
//! ```

#![deny(missing_docs)]

mod error;
mod grammars;
mod registry;
mod scope;
mod themes;

mod highlight;
mod markdown_fence;
mod renderers;
mod tokenizer;

pub use error::Error;
pub use highlight::HighlightedText;
pub use markdown_fence::{ParsedFence, parse_markdown_fence};
pub use registry::{HighlightOptions, HighlightedCode, PLAIN_GRAMMAR_NAME, Registry};
pub use renderers::{RenderOptions, html::HtmlRenderer};
pub use themes::{Color, CompiledTheme, FontStyle, Style, ThemeVariant};

/// The CSS needed for the line number gutter to display properly
pub const GIALLO_CSS: &str = r#".giallo-l {
  display: block;
}
.giallo-ln {
  display: inline-block;
  user-select: none;
  white-space: pre;
  margin-right: 0.4em;
  padding: 0.4em;
  min-width: 3ch;
  text-align: right;
  opacity: 0.8;
}
"#;
