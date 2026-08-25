//! ExCaliber core: the Excalidraw-compatible scene model. Zero rendering/UI deps.
//!
//! Lands in slices: element vocabulary (now) → full typed schema + restore semantics +
//! fractional indexing + undo stack (M1).

pub mod edit;
pub mod element;
pub mod file;
pub mod findex;
pub mod history;
pub mod hit_test;
pub mod idgen;
pub mod router;
pub mod scene;
pub mod text;
pub mod time;
