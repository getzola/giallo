use std::fs;
use std::io::{self, Read};
use std::ops::RangeInclusive;
use std::path::Path;
use std::process;

use clap::{Parser, ValueEnum};

use giallo::{
    GIALLO_CSS, HighlightOptions, HtmlRenderer, Registry, RenderOptions, TerminalRenderer,
    ThemeType, ThemeVariant,
};

#[derive(Debug, Clone, ValueEnum)]
enum OutputFormat {
    /// ANSI terminal output (default)
    Terminal,
    /// HTML fragment (<pre><code>...</code></pre>)
    Html,
    /// Full HTML page with DOCTYPE, styles, etc.
    HtmlPage,
    /// Full HTML page showing the input highlighted with every available theme
    AllThemes,
}

#[derive(Parser, Debug)]
#[command(
    name = "giallo-cli",
    about = "A CLI tool for testing giallo syntax highlighting",
    long_about = "Reads a source code file, highlights it using giallo, and outputs the result \
                  as terminal ANSI codes or HTML."
)]
struct Cli {
    /// Path to the source file to highlight. Use `-` for stdin.
    #[arg(required_unless_present_any = ["list_languages", "list_themes"])]
    file: Option<String>,

    /// Theme name (e.g. catppuccin-frappe). Not required when using --format=all-themes.
    #[arg(short, long, required_unless_present_any = ["list_languages", "list_themes", "format"])]
    theme: Option<String>,

    /// Language/grammar name (e.g. rust, js, python). Auto-detected from file extension if omitted.
    #[arg(short, long)]
    language: Option<String>,

    /// Dark theme for dual light/dark mode. When set, --theme is used as the light theme.
    #[arg(long)]
    dark_theme: Option<String>,

    /// Output format
    #[arg(short, long, value_enum, default_value_t = OutputFormat::Terminal)]
    format: OutputFormat,

    /// Write output to a file instead of stdout
    #[arg(short, long)]
    output: Option<String>,

    /// Show line numbers
    #[arg(long)]
    line_numbers: bool,

    /// Starting line number
    #[arg(long, default_value_t = 1)]
    line_number_start: isize,

    /// Comma-separated line ranges to highlight (e.g. 1-3,5,7-9)
    #[arg(long)]
    highlight_lines: Option<String>,

    /// Comma-separated line ranges to hide (e.g. 2,4-6)
    #[arg(long)]
    hide_lines: Option<String>,

    /// Path to a custom registry dump file (builtin.zst)
    #[arg(long)]
    registry: Option<String>,

    /// List all available grammar names/aliases and exit
    #[arg(long)]
    list_languages: bool,

    /// List all available theme names and exit
    #[arg(long)]
    list_themes: bool,
}

fn parse_ranges(input: &str) -> Vec<RangeInclusive<usize>> {
    let mut ranges = Vec::new();
    for part in input.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(dash) = part.find('-') {
            if let (Ok(start), Ok(end)) = (part[..dash].parse(), part[dash + 1..].parse()) {
                let (start, end): (usize, usize) = if end < start {
                    (end, start)
                } else {
                    (start, end)
                };
                ranges.push(start..=end);
            } else {
                eprintln!("warning: ignoring invalid range '{part}'");
            }
        } else if let Ok(val) = part.parse::<usize>() {
            ranges.push(val..=val);
        } else {
            eprintln!("warning: ignoring invalid range '{part}'");
        }
    }
    ranges
}

fn detect_language(file_path: &str, registry: &Registry) -> Option<String> {
    let path = Path::new(file_path);

    // Try the full file extension first (e.g. "rs", "py")
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let ext_lower = ext.to_lowercase();
        if registry.contains_grammar(&ext_lower) {
            return Some(ext_lower);
        }
    }

    // Try the file stem (e.g. "Makefile", "Dockerfile")
    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
        let stem_lower = stem.to_lowercase();
        if registry.contains_grammar(&stem_lower) {
            return Some(stem_lower);
        }
    }

    // Try the full file name (e.g. "Dockerfile", ".gitignore")
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        let name_lower = name.to_lowercase();
        if registry.contains_grammar(&name_lower) {
            return Some(name_lower);
        }
    }

    None
}

fn wrap_html_page(fragment: &str, theme_css: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>Syntax Highlighted Code</title>
    <style>
    html {{
        font-size: 16px;
    }}
    {GIALLO_CSS}
    {theme_css}
    </style>
</head>
<body>
{fragment}
</body>
</html>"#
    )
}

fn render_all_themes(
    registry: &Registry,
    content: &str,
    language: &str,
    render_options: &RenderOptions,
) -> String {
    let theme_names = registry.theme_names();
    let total = theme_names.len();

    let mut sections = Vec::with_capacity(total);

    for (i, theme_name) in theme_names.iter().enumerate() {
        eprint!(
            "\r  Rendering theme {}/{total}: {theme_name}...            ",
            i + 1
        );

        let options = HighlightOptions::new(language, ThemeVariant::Single(*theme_name));
        let highlighted = match registry.highlight(content, &options) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("\nwarning: failed to highlight with theme '{theme_name}': {e}");
                continue;
            }
        };

        let theme = match highlighted.theme {
            ThemeVariant::Single(t) => t,
            _ => unreachable!(),
        };

        let theme_type_label = if theme.theme_type == ThemeType::Light {
            "light"
        } else {
            "dark"
        };
        let fg = theme.default_style.foreground.as_hex();
        let bg = theme.default_style.background.as_hex();

        let fragment = HtmlRenderer::default().render(&highlighted, render_options);

        // Pick a contrasting text color for the heading based on whether the theme is light or dark
        let heading_fg = if theme_type_label == "light" {
            "#222"
        } else {
            "#eee"
        };
        let heading_bg = if theme_type_label == "light" {
            "#f0f0f0"
        } else {
            "#1a1a1a"
        };

        sections.push(format!(
            r#"<section style="margin-bottom: 2em;">
    <div style="background: {heading_bg}; color: {heading_fg}; padding: 0.75em 1em; border-radius: 8px 8px 0 0; border-bottom: 2px solid {fg};">
        <h2 style="margin: 0; font-size: 1.25em; font-family: system-ui, -apple-system, sans-serif;">{theme_name}</h2>
        <p style="margin: 0.25em 0 0 0; font-size: 0.85em; font-family: system-ui, -apple-system, sans-serif; opacity: 0.8;">
            {theme_type_label} theme &middot; fg: <code>{fg}</code> &middot; bg: <code>{bg}</code>
        </p>
    </div>
    <div style="border-radius: 0 0 8px 8px; overflow: hidden;">
        {fragment}
    </div>
</section>"#,
        ));
    }

    eprintln!("\r  Rendered {total} themes.                                      ");

    let body = sections.join("\n");

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>All Themes — giallo syntax highlighting</title>
    <style>
    html {{
        font-size: 16px;
    }}
    body {{
        font-family: system-ui, -apple-system, sans-serif;
        max-width: 960px;
        margin: 0 auto;
        padding: 2em;
        background: #fafafa;
        color: #333;
    }}
    h1 {{
        border-bottom: 2px solid #ccc;
        padding-bottom: 0.5em;
    }}
    pre.giallo {{
        margin: 0;
        border-radius: 0 0 8px 8px;
    }}
    {GIALLO_CSS}
    </style>
</head>
<body>
<h1>All Themes ({total} themes, language: {language})</h1>
{body}
</body>
</html>"#
    )
}

fn main() {
    let cli = Cli::parse();

    // Load registry
    let registry = match &cli.registry {
        Some(path) => Registry::load_from_file(path),
        None => Registry::builtin(),
    };
    let mut registry = match registry {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: failed to load registry: {e}");
            process::exit(1);
        }
    };
    registry.link_grammars();

    // Handle --list-languages
    if cli.list_languages {
        for name in registry.grammar_names() {
            println!("{name}");
        }
        return;
    }

    // Handle --list-themes
    if cli.list_themes {
        for name in registry.theme_names() {
            println!("{name}");
        }
        return;
    }

    // At this point we need file
    let file_path = cli.file.as_deref().unwrap();

    // For all formats except all-themes, we require --theme
    let is_all_themes = matches!(cli.format, OutputFormat::AllThemes);
    if !is_all_themes && cli.theme.is_none() {
        eprintln!("error: --theme is required for this output format");
        process::exit(1);
    }

    // Read input
    let content = if file_path == "-" {
        let mut buf = String::new();
        if let Err(e) = io::stdin().read_to_string(&mut buf) {
            eprintln!("error: failed to read stdin: {e}");
            process::exit(1);
        }
        buf
    } else {
        match fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: failed to read '{file_path}': {e}");
                process::exit(1);
            }
        }
    };

    // Determine language
    let language = if let Some(lang) = &cli.language {
        lang.clone()
    } else if file_path == "-" {
        eprintln!("error: --language is required when reading from stdin");
        process::exit(1);
    } else {
        match detect_language(file_path, &registry) {
            Some(lang) => lang,
            None => {
                eprintln!(
                    "warning: could not detect language for '{file_path}', falling back to 'plain'"
                );
                "plain".to_string()
            }
        }
    };

    // Build render options
    let render_options = RenderOptions {
        show_line_numbers: cli.line_numbers,
        line_number_start: cli.line_number_start,
        highlight_lines: cli
            .highlight_lines
            .as_deref()
            .map(parse_ranges)
            .unwrap_or_default(),
        hide_lines: cli
            .hide_lines
            .as_deref()
            .map(parse_ranges)
            .unwrap_or_default(),
    };

    // Handle all-themes format separately since it doesn't use a single theme
    if is_all_themes {
        let output = render_all_themes(&registry, &content, &language, &render_options);

        if let Some(output_path) = &cli.output {
            if let Err(e) = fs::write(output_path, &output) {
                eprintln!("error: failed to write to '{output_path}': {e}");
                process::exit(1);
            }
        } else {
            print!("{output}");
        }
        return;
    }

    let theme = cli.theme.as_deref().unwrap();

    // Build theme variant
    let theme_variant = match &cli.dark_theme {
        Some(dark) => ThemeVariant::Dual {
            light: theme,
            dark: dark.as_str(),
        },
        None => ThemeVariant::Single(theme),
    };

    // Build highlight options
    let highlight_options = HighlightOptions::new(&language, theme_variant);

    // Highlight
    let highlighted = match registry.highlight(&content, &highlight_options) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("error: highlighting failed: {e}");
            process::exit(1);
        }
    };

    // Render
    let output = match cli.format {
        OutputFormat::Terminal => TerminalRenderer::default().render(&highlighted, &render_options),
        OutputFormat::Html => HtmlRenderer::default().render(&highlighted, &render_options),
        OutputFormat::HtmlPage => {
            let fragment = HtmlRenderer::default().render(&highlighted, &render_options);
            wrap_html_page(&fragment, "")
        }
        OutputFormat::AllThemes => unreachable!(),
    };

    // Write output
    if let Some(output_path) = &cli.output {
        if let Err(e) = fs::write(output_path, &output) {
            eprintln!("error: failed to write to '{output_path}': {e}");
            process::exit(1);
        }
    } else {
        print!("{output}");
    }
}
