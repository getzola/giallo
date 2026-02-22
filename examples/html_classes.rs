use giallo::{HighlightOptions, HtmlRenderer, Registry, RenderOptions, ThemeVariant};
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut registry = Registry::load_from_file("builtin.zst")?;
    registry.link_grammars();

    // Create output directory
    fs::create_dir_all("html-classes")?;

    let code = r#"function fibonacci(n) {
    if (n <= 1) return n;
    return fibonacci(n - 1) + fibonacci(n - 2);
}

// Calculate the 10th Fibonacci number
const result = fibonacci(10);
console.log(`Fibonacci(10) = ${result}`);
"#;

    let light_theme = "catppuccin-latte";
    let dark_theme = "catppuccin-frappe";
    let prefix = "g-";

    // Generate CSS for both themes
    let light_css = registry.generate_css(light_theme, prefix)?;
    let dark_css = registry.generate_css(dark_theme, prefix)?;

    fs::write("html-classes/light.css", &light_css)?;
    fs::write("html-classes/dark.css", &dark_css)?;

    // Highlight code (we can use either theme since CSS classes are theme-independent)
    let options = HighlightOptions::new("javascript", ThemeVariant::Single(light_theme));
    let highlighted = registry.highlight(code, &options)?;

    // Render with CSS classes instead of inline styles
    let renderer = HtmlRenderer {
        css_class_prefix: Some(prefix.to_string()),
        ..Default::default()
    };
    let render_options = RenderOptions {
        show_line_numbers: true,
        highlight_lines: vec![3..=3],
        ..Default::default()
    };
    let html_code = renderer.render(&highlighted, &render_options);

    // Generate full HTML page with theme switcher
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Giallo CSS Classes Demo</title>
    <link id="light-css" rel="stylesheet" href="light.css" media="(prefers-color-scheme: light)">
    <link id="dark-css" rel="stylesheet" href="dark.css" media="(prefers-color-scheme: dark)">
    <style>
        body {{
            font-family: system-ui, -apple-system, sans-serif;
            max-width: 800px;
            margin: 0 auto;
            padding: 2rem;
            transition: background-color 0.3s, color 0.3s;
        }}
        @media (prefers-color-scheme: dark) {{
            body {{
                background-color: #303446;
                color: #c6d0f5;
            }}
        }}
        @media (prefers-color-scheme: light) {{
            body {{
                background-color: #eff1f5;
                color: #4c4f69;
            }}
        }}
        body.dark {{
            background-color: #303446;
            color: #c6d0f5;
        }}
        body.light {{
            background-color: #eff1f5;
            color: #4c4f69;
        }}
        h1 {{
            margin-bottom: 1rem;
        }}
        .controls {{
            margin-bottom: 1rem;
        }}
        button {{
            padding: 0.5rem 1rem;
            font-size: 1rem;
            cursor: pointer;
            border: none;
            border-radius: 0.25rem;
            background-color: #7c3aed;
            color: white;
        }}
        button:hover {{
            background-color: #6d28d9;
        }}
        pre.giallo {{
            border-radius: 0.5rem;
            padding: 1rem 0;
            overflow-x: auto;
            border: 4px solid #7c7f93;
            font-size: 16px;
        }}
        {giallo_css}
    </style>
</head>
<body>
    <h1>Giallo CSS Classes Demo</h1>
    <div class="controls">
        <button onclick="toggleTheme()">Toggle Light/Dark</button>
        <span id="current-theme"></span>
    </div>

    {html_code}

    <script>
        let manualOverride = null; // null = follow system, 'light' or 'dark' = manual

        function getEffectiveTheme() {{
            if (manualOverride) return manualOverride;
            return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
        }}

        function applyTheme() {{
            const theme = getEffectiveTheme();
            const body = document.body;
            const lightCss = document.getElementById('light-css');
            const darkCss = document.getElementById('dark-css');
            const themeLabel = document.getElementById('current-theme');

            body.classList.remove('light', 'dark');

            if (manualOverride) {{
                // Manual override: force one stylesheet
                body.classList.add(theme);
                lightCss.media = theme === 'light' ? 'all' : 'not all';
                darkCss.media = theme === 'dark' ? 'all' : 'not all';
                themeLabel.textContent = 'Current: ' + (theme === 'dark' ? 'Dark (catppuccin-frappe)' : 'Light (catppuccin-latte)') + ' (manual)';
            }} else {{
                // Follow system preference
                lightCss.media = '(prefers-color-scheme: light)';
                darkCss.media = '(prefers-color-scheme: dark)';
                themeLabel.textContent = 'Current: ' + (theme === 'dark' ? 'Dark (catppuccin-frappe)' : 'Light (catppuccin-latte)') + ' (system)';
            }}
        }}

        function toggleTheme() {{
            const current = getEffectiveTheme();
            manualOverride = current === 'dark' ? 'light' : 'dark';
            applyTheme();
        }}

        // Listen for system theme changes
        window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {{
            if (!manualOverride) applyTheme();
        }});

        // Initialize
        applyTheme();
    </script>
</body>
</html>
"#,
        giallo_css = giallo::GIALLO_CSS,
        html_code = html_code
    );

    fs::write("html-classes/index.html", html)?;

    println!("Generated files in html-classes/:");
    println!("  - light.css");
    println!("  - dark.css");
    println!("  - index.html");
    println!("\nOpen html-classes/index.html in a browser to see the demo.");

    Ok(())
}
