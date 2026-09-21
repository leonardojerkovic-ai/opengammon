//! Position representation, rules, and legal move generation for backgammon.
//!
//! Depends on nothing else in the workspace and targets zero (or near-zero) external
//! dependencies, so it compiles cleanly to WASM. See `OPENGAMMON.md`, Phase 1.

mod moves;
mod position;

#[cfg(test)]
mod gnubg_diff;
#[cfg(test)]
mod self_play;

pub use moves::{CheckerMove, Destination, Die, Origin, Ply, PointIndex, Roll};
pub use position::Position;
