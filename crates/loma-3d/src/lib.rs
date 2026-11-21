#![forbid(unsafe_code)]
//! The 3D preview: a Bevy app that draws a garden scene into a canvas.
//! Built only for the browser with the `viewer` feature; otherwise this crate is empty.

pub mod gestures;

#[cfg(all(feature = "viewer", target_arch = "wasm32"))]
mod viewer;

#[cfg(all(feature = "viewer", target_arch = "wasm32"))]
pub use viewer::{open, show};
