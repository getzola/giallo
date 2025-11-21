List of issues in grammar:

- xml.json wrong level for nesting end/captures
- inline.es6-htmlx instead of inline.es6-html?
- jison.json referring to source.jisonlex
- text.html.markdown.source.gfm.apib in mdx.json
- nextflow missing nextflow-groovy from https://github.com/nextflow-io/vscode-language-nextflow
- markdown syntax does not have Scope text.html.markdown.math#math not found (wikitext.json)


TODOs:

- open up the repo
- add a comprehensive Options struct
- write proper HTML renderer that does follow all options
- add CSS export for themes and allow using classes for highlight rather than hex

let highlighted = registry.highlight(&code, HighlightOptions {
  lang: "javascript",
  theme: "catppuccin-frappe",
  ..Default::default()
  })?;

  let html = highlighted.render(Renderer::Html {
  css_class_prefix: Some("syntax-".to_string()), // None = inline styles, Some = CSS classes
  }, RenderOptions {
  show_line_numbers: true,
  line_number_start: 1,
  highlight_lines: vec![5..=7],
  hide_lines: vec![1..=2],
  })?;

  let css = registry.generate_css("catppuccin-frappe", "syntax-")?;