//! ExCaliber core: the Excalidraw-compatible scene model. Zero rendering/UI deps.
//!
//! Lands in slices: element vocabulary (now) → full typed schema + restore semantics +
//! fractional indexing + undo stack (M1).

pub mod element;
