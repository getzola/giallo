pub mod grammars;
pub mod registry;
pub mod scope;
pub mod themes;

mod highlight;
pub(crate) mod renderers;
mod tokenizer;

pub use registry::{HighlightOptions, HighlightedCode, Registry};
pub use renderers::{Options, html::HtmlRenderer};
