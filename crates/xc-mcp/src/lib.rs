//! ExCaliber's MCP surface: local tools that let coding agents (Claude Code,
//! Cursor, Codex CLI, …) inspect and drive an ExCaliber scene.
//!
//! Design rules (plan §7): batch-oriented tools, always return assigned ids,
//! screenshot closes the vision feedback loop. Every mutation goes through
//! `Scene`'s command stack, so agent edits share undo history with the GUI and
//! are persisted atomically.

pub mod server;

pub use server::{XcMcpServer, XcServerConfig, run_stdio};
