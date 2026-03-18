//! Search strategy trait and worker types for MineGraph.
//!
//! This crate defines the interface between the worker platform and
//! search strategies. Strategies are pure computation — no network,
//! no filesystem, no async.

mod command;
mod observer;
mod strategy;

pub use command::*;
pub use observer::*;
pub use strategy::*;
