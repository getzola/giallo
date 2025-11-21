use giallo::registry::Registry;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registry = Registry::load_from_file("builtin.msgpack")?;

    let css = registry.generate_css("catppuccin-frappe", "g-")?;
    println!("{css}");

    Ok(())
}
