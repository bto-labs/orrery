//! Stage-1 ingestion: tokio runtime, sources, reducer, and the triple_buffer seam.

// Stage-1 ingestion is built incrementally over Tasks 1–6; these types are
// wired up by the reducer/seam/visuals in later tasks. Tighten/remove this
// once the module is fully wired (Task 6).
#![allow(dead_code)]

pub mod model;
pub mod reducer;
