//! Bearoff database computation (backward dynamic programming) and lookup.
//!
//! Depends only on `og-core`. Targets near-zero external dependencies so it compiles
//! cleanly to WASM. See `OPENGAMMON.md`, Phase 2.

pub mod combinatorial;
pub mod disk;
pub mod one_sided;
pub mod quantize;

#[cfg(test)]
mod gnubg_diff;
