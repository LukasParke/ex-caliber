//! GPUI canvas surface for ExCaliber: pan/zoom viewport, drawing tools, selection and
//! transform handles, inline text editing. Rendering primitives live here until
//! `xc-render` splits out at M3.

mod spike;

pub use spike::run_spike;
