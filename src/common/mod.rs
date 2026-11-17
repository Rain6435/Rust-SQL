//! Shared domain types, IPC event constants, and string/value utilities used across Rust-SQL.
//!
//! This module provides foundational data structures (`Server`, `Config`, `QueryRowResult`,
//! `AdapterInfo`), IPC channel identifiers (`event`), and formatting utilities (`utils`).

pub mod event;
pub mod types;
pub mod utils;

pub use event::*;
pub use types::*;
pub use utils::*;
