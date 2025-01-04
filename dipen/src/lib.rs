//! DiPeN - Distributed Petri Net runner
//!
//! This crate implements a distributed runner to use petri nets as a workflow engine.
//! See [repository](https://github.com/valgarf/dipen) for an introductory readme.
//!

pub mod error;
pub mod etcd;
pub mod exec;
pub mod net;
pub mod runner;

pub use error::Result;
