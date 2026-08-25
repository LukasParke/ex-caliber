//! `xc` — the ExCaliber binary.
//!
//! Subcommands:
//! - `xc [spike]`                     GPUI pan/zoom spike (M0)
//! - `xc <file.excalidraw>`           open the canvas (M3)
//! - `xc mcp [--file <scene>]`        headless MCP server over stdio (M2)
//! - `xc export <in> -f svg|png|excalidraw [-o out]`  render/export (M6)
//! - `xc install-claude [--file <scene>]`             register MCP in Claude Code (M6)

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None | Some("spike") => xc_canvas::run_spike(),
        Some(path) if path.ends_with(".excalidraw") => {
            xc_canvas::open_scene_window(Some(std::path::PathBuf::from(path)))
        }
        Some("export") => {
            let mut input = None;
            let mut format = String::new();
            let mut out = None;
            let mut rest = args.peekable();
            while let Some(arg) = rest.next() {
                match arg.as_str() {
                    "-f" | "--format" => format = rest.next().unwrap_or_default(),
                    "-o" | "--out" => out = rest.next().map(std::path::PathBuf::from),
                    other if !other.starts_with('-') => {
                        input = Some(std::path::PathBuf::from(other))
                    }
                    other => {
                        eprintln!("xc export: unknown flag `{other}`");
                        std::process::exit(2);
                    }
                }
            }
            let Some(input) = input else {
                eprintln!("usage: xc export <file.excalidraw> -f svg|png|excalidraw [-o out]");
                std::process::exit(2);
            };
            if format.is_empty() {
                format = out
                    .as_ref()
                    .and_then(|o| o.extension().map(|e| e.to_string_lossy().to_string()))
                    .unwrap_or_default();
            }
            if let Err(e) = run_export(&input, &format, out.as_deref()) {
                eprintln!("xc export: {e}");
                std::process::exit(1);
            }
        }
        Some("install-claude") => {
            let mut file = None;
            let mut rest = args.peekable();
            while let Some(arg) = rest.next() {
                match arg.as_str() {
                    "--file" => file = rest.next().map(std::path::PathBuf::from),
                    other => {
                        eprintln!("xc install-claude: unknown flag `{other}`");
                        std::process::exit(2);
                    }
                }
            }
            if let Err(e) = run_install_claude(file) {
                eprintln!("xc install-claude: {e}");
                std::process::exit(1);
            }
        }
        Some("mcp") => {
            let mut file = None;
            let mut rest = args.peekable();
            while let Some(arg) = rest.next() {
                match arg.as_str() {
                    "--file" => file = rest.next().map(std::path::PathBuf::from),
                    other => {
                        eprintln!("xc mcp: unknown flag `{other}`");
                        eprintln!("usage: xc mcp [--file <scene.excalidraw>]");
                        std::process::exit(2);
                    }
                }
            }
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            let result = rt.block_on(xc_mcp::run_stdio(xc_mcp::XcServerConfig { file }));
            if let Err(e) = result {
                eprintln!("xc mcp: {e}");
                std::process::exit(1);
            }
        }
        Some(other) => {
            eprintln!("xc: unknown command `{other}`");
            eprintln!("usage: xc [spike|mcp|export|install-claude|<file.excalidraw>]");
            std::process::exit(2);
        }
    }
}

fn run_export(
    input: &std::path::Path,
    format: &str,
    out: Option<&std::path::Path>,
) -> Result<(), String> {
    let scene = xc_core::file::load_scene(input).map_err(|e| e.to_string())?;
    match format {
        "svg" => {
            let svg = xc_io::scene_to_svg(&scene, 12.0);
            let fallback;
            let path = match out {
                Some(p) => p,
                None => {
                    fallback = default_out(input, "svg");
                    &fallback
                }
            };
            write_out(path, svg.into_bytes())?;
            println!("wrote svg");
        }
        "png" => {
            let svg = xc_io::scene_to_svg(&scene, 12.0);
            let png = xc_io::svg_to_png(&svg, 2.0).map_err(|e| e.to_string())?;
            let fallback;
            let path = match out {
                Some(p) => p,
                None => {
                    fallback = default_out(input, "png");
                    &fallback
                }
            };
            write_out(path, png)?;
            println!("wrote png");
        }
        "excalidraw" => {
            let body = xc_core::file::save_scene_to_string(&scene);
            write_out(out.unwrap_or(input), body.into_bytes())?;
            println!("wrote excalidraw");
        }
        other => return Err(format!("unsupported format `{other}` (svg|png|excalidraw)")),
    }
    Ok(())
}

fn default_out(input: &std::path::Path, ext: &str) -> std::path::PathBuf {
    let mut stem = input
        .file_stem()
        .map(|s| s.to_os_string())
        .unwrap_or_default();
    stem.push(".");
    stem.push(ext);
    input.with_file_name(stem)
}

fn write_out(path: &std::path::Path, bytes: Vec<u8>) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("write {}: {e}", path.display()))
}

fn run_install_claude(file: Option<std::path::PathBuf>) -> Result<(), String> {
    let xc_path = std::env::current_exe().map_err(|e| e.to_string())?;
    let file_arg = file
        .as_ref()
        .map(|f| format!(" --file {}", f.display()))
        .unwrap_or_default();
    let cmd = format!(
        "claude mcp add excaliber -- {} mcp{}",
        xc_path.display(),
        file_arg
    );
    let status = std::process::Command::new("claude")
        .args(["mcp", "add", "excaliber", "--"])
        .arg(&xc_path)
        .args(["mcp"])
        .args(
            file.iter()
                .flat_map(|f| ["--file".to_string(), f.display().to_string()]),
        )
        .status()
        .map_err(|e| format!("cannot run claude CLI: {e} (is it installed?)"))?;
    if status.success() {
        println!("registered. Start the canvas alongside your session with:");
        println!(
            "  xc {} &",
            file.as_ref()
                .map(|f| f.display().to_string())
                .unwrap_or_default()
        );
        Ok(())
    } else {
        Err(format!("claude mcp add failed; run manually:\n  {cmd}"))
    }
}
