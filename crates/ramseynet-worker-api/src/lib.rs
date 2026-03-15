//! Worker API: trait definitions and schemas for RamseyNet search strategies.
//!
//! This crate defines the contract between the worker platform (engine) and
//! search strategy implementations. It has no runtime dependencies — no tokio,
//! no network, no filesystem.

pub mod command;
pub mod observer;
pub mod strategy;

pub use command::{ConfigParam, EngineConfigPatch, ParamType, WorkerCommand, WorkerEvent, WorkerState, WorkerStatus, StrategyInfo};
pub use observer::{ProgressInfo, SearchObserver};
pub use strategy::{RawDiscovery, SearchJob, SearchResult, SearchStrategy};
