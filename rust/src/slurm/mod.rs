//! Data layer: the sole interface to the Slurm CLI.
//!
//! Every command goes through `exec`, which enforces a timeout and records the
//! invocation into the command history read by the Health view.

pub mod model;
pub mod parse;
