//! Common utilities and traits for the dr_ones network simulator.
//!
//! This crate provides shared functionality used by the drone, client,
//! and server components of the network simulator.

pub mod logging;
mod network_node;

pub use logging::*;
pub use network_node::*;
