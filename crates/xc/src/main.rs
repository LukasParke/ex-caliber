fn main() {
    // Subcommands grow with milestones: `mcp` (M2), `headless` (M2), `export` (M6),
    // `install-claude` (M6). Until then the spike is the default action.
    let cmd = std::env::args().nth(1);
    match cmd.as_deref() {
        None | Some("spike") => xc_canvas::run_spike(),
        Some(other) => {
            eprintln!("xc: unknown command `{other}`");
            eprintln!("usage: xc [spike]");
            std::process::exit(2);
        }
    }
}
