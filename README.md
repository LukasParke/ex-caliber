# ExCaliber

An [Excalidraw](https://excalidraw.com)-compatible native desktop whiteboard,
written in Rust on [GPUI](https://github.com/zed-industries/zed) (Zed's GPU UI
framework), with a first-class [MCP](https://modelcontextprotocol.io) server so
coding agents — Claude Code, Cursor, Codex CLI — can draw on the canvas with you.

```
claude mcp add excaliber -- /path/to/xc mcp --file diagram.excalidraw
# then: "draw our auth flow in the excaliber canvas, screenshot it, iterate"
```

## Status

Milestones M1–M8 of [PLAN.md](PLAN.md) are implemented and tested; the GUI's
inline text-editing caret and the 60 fps visual confirmation are pending a
desktop session (see PLAN.md §Resolved decisions).

## Build

```sh
cargo build --release -p xc
./target/release/xc --help
```

Linux needs the usual GPUI system libs: `libwayland-dev libxkbcommon-dev
libfontconfig1-dev libx11-dev`.

## Use

```sh
xc diagram.excalidraw          # open the canvas (pan: drag, zoom: wheel)
xc export diagram.excalidraw -f png -o diagram.png
xc mcp --file diagram.excalidraw   # headless MCP server (stdio)
xc install-claude --file diagram.excalidraw   # register with Claude Code
```

### MCP tools

`get_scene`, `create_elements`, `update_elements`, `delete_elements`, `connect`,
`add_text`, `reorder`, `undo`, `redo`, `screenshot`, `export_file`. All
mutations share the undo stack with the GUI and persist atomically.

## Layout

| Crate | Purpose |
|---|---|
| `xc-core` | Excalidraw-compatible scene model: schema, fractional indexing, undo, hit testing, edit ops, elbow router, text engine |
| `xc-io` | `.excalidraw`/`.excalidrawlib` load+save, SVG/PNG export, image crop |
| `xc-render` | deterministic rough-style geometry (roughr → lyon) |
| `xc-canvas` | GPUI canvas: viewport, tools, overlays |
| `xc-mcp` | rmcp MCP server (stdio) |
| `xc` | the binary |

## Compatibility

Round-trips real `.excalidraw` files; output is validated against Excalidraw's
own `restoreElements`/`serializeAsJSON` (see `scripts/mcp_e2e.py`, harness in
`~/exval`). Known divergences are listed in [PLAN.md](PLAN.md) §4.

## License

MIT OR Apache-2.0. Bundled fonts carry their own permissive licenses — see
`assets/fonts/LICENSES.md`.
