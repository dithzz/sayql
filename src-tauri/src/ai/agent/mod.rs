//! Agent mode: answers a question in several read-only steps. Phase 1 holds the parts the loop
//! will build on; the loop itself, its tools and its commands come in phase 2.

// Nothing outside the tests calls these until the agent loop exists.
#[cfg_attr(not(test), allow(dead_code))]
pub mod mask;
#[cfg(test)]
mod eval;
