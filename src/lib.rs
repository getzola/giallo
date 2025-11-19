pub mod grammars;
pub mod registry;
pub mod scope;
pub mod themes;

mod highlight;
mod renderers;
mod tokenizer;

pub use registry::Registry;
pub use renderers::{Options, Renderer};
