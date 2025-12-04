# Giallo

A Rust syntax highlighting library using TextMate grammars and themes, producing the same output as VSCode.

This uses the curated grammars and themes from <https://github.com/shikijs/textmate-grammars-themes> for an optional built-in
starting kit and testing, but you can start from an empty canvas if you want.

## Installation

```toml
[dependencies]
giallo = { version = "0.0.1", features = ["dump"] }
```

The `dump` feature is required to use `Registry::builtin()` or create/load your own dump.

## Usage

```rust
use giallo::{HighlightOptions, HtmlRenderer, RenderOptions, Registry};

// Load the pre-built registry
let mut registry = Registry::builtin()?;
registry.link_grammars();

let code = "let x = 42;";
let options = HighlightOptions::new("javascript").single_theme("catppuccin-frappe");
// For light/dark support, you can specify 2 themes
// let options = HighlightOptions::new("javascript").light_dark_themes("catppuccin-latte", "catppuccin-mocha");
let highlighted = registry.highlight(code, options)?;

// Render to HTML
let html = HtmlRenderer::default().render(&highlighted, &RenderOptions::default());
```

## Renderers

Highlighting some code is done the same way regardless of where/how you're planning to display the output.
Giallo will give you back everything you need to implement your own renderer but also provides some (well one currently)
renderers.

### HTML renderer

This renderer outputs the text wrapped in a `<pre><code>...</code></pre>` with all the colours and attributes set correctly
as well as escaping the HTML content.

If you want to use line numbers, giallo will set some classes on `<span>` that you will need to target via CSS to have
something looking good. The minimal CSS is exported as `GIALLO_CSS` by the crate.

This renderer also supports light/dark mode automatically if you highlight the text using 2 themes by using the [light-dark](https://developer.mozilla.org/en-US/docs/Web/CSS/Reference/Values/color_value/light-dark)
function in the `style` attribute.

