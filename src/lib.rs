pub mod grammars;
pub mod registry;
pub mod scope;
pub mod themes;

mod highlight;
mod markdown_fence;
pub(crate) mod renderers;
mod tokenizer;

pub use markdown_fence::parse_markdown_fence;
pub use registry::{HighlightOptions, HighlightedCode, Registry};
pub use renderers::{Options, html::HtmlRenderer};
pub use themes::ThemeVariant;

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
