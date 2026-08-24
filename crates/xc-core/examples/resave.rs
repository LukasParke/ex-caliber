//! Load a `.excalidraw` document and write it back out through our serializer.
//!
//! Usage: `cargo run -p xc-core --example resave -- <in.excalidraw> [out.excalidraw]`
//!
//! Used to validate compatibility against Excalidraw's own code
//! (`@excalidraw/utils` restore/export) in CI-style checks.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: resave <in> [out]");
        std::process::exit(2);
    }
    let raw = std::fs::read_to_string(&args[1])?;
    let scene = xc_core::file::load_document(&raw)?;
    println!("loaded {} elements", scene.len());
    let out = xc_core::file::save_scene_to_string(&scene);
    match args.get(2) {
        Some(path) => std::fs::write(path, out)?,
        None => print!("{out}"),
    }
    Ok(())
}
