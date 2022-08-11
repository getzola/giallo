mod errors;
mod highlight;
mod languages;
mod options;
mod themes;

pub use highlight::HtmlRenderer;
pub use options::HighlightStyle;
pub use themes::Theme;

// TODO:
// pub api: render(source, extension, config)?
// the inner will do:
//  1. highlight the code with tree-sitter
//  2. build the actual <pre> item depending on the options (split the code by line)
