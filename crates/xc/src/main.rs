//! `xc` — the ExCaliber binary.
//!
//! Subcommands grow with milestones: `spike` (M0), `mcp` (M2, headless MCP
//! server), `<file.excalidraw>` (M3, canvas viewer), `headless`/`export`/
//! `install-claude` (M6).

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None | Some("spike") => xc_canvas::run_spike(),
        Some(path) if path.ends_with(".excalidraw") => {
            xc_canvas::open_scene_window(Some(std::path::PathBuf::from(path)))
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
            eprintln!("usage: xc [spike|mcp|<file.excalidraw>]");
            std::process::exit(2);
        }
    }
}
