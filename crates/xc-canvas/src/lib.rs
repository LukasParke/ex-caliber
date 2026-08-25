//! GPUI canvas surface for ExCaliber: pan/zoom viewport over a live `Scene`.
//!
//! Rendering contract (M3): static scene render of every element kind with rough
//! styling via `xc-render`, viewport culling, text/image overlays as positioned
//! elements. Editing interactions land in M4; the MCP bridge shares this scene
//! through the same `Arc<Mutex<Scene>>`.

mod scene_canvas;
pub mod text_edit;
pub mod tools;
mod spike;

pub use scene_canvas::{open_scene_window, SceneCanvas};
pub use spike::run_spike;
