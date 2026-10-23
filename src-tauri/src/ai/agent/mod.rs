//! Agent mode: answers a question in several read-only steps and ends with a cited answer.
//! `run.rs` is the loop; the model asks for `tools.rs`, SayQL runs them through the
//! read-only gate and `mask.rs`, and `checks.rs` checks the work in code.

pub mod checks;
pub mod mask;
pub mod model;
pub mod prompt;
pub mod run;
pub mod text_protocol;
pub mod tools;
#[cfg(test)]
mod eval;
#[cfg(test)]
mod run_tests;
