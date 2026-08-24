//! Scene → draw-op pipeline: Excalidraw elements rendered as deterministic
//! rough-style geometry (roughr), independent of any UI toolkit.
//!
//! Layering: `geometry` (this module's core) is headless and unit-testable;
//! the gpui adapter consumes its opsets. Determinism: every op derives from the
//! element's stored `seed`, so renders are reproducible and snapshot-testable.

pub mod geometry;

pub use geometry::{DrawOp, RenderElementError};
