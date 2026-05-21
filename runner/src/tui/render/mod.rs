//! Render primitives.
//!
//! `Renderable` is the lowest-level rendering contract: every widget
//! receives a `Rect` + `Buffer` and writes pixels into it. Render is
//! pure: it never schedules events, never mutates global state.

pub mod renderable;

pub use renderable::Renderable;
