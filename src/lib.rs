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
