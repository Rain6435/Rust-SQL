//! **Rust-SQL**: A fast, lightweight, cross-database SQL client with native Desktop GUI (`egui`),
//! Terminal UI (`ratatui`), Web UI & IPC server (`axum`), and headless CLI batch execution.
//!
//! # Architecture Overview
//! - [`common`]: Shared domain models, configuration types, IPC event constants, and string/version utilities.
//! - [`core`]: Core services including keychain-backed XChaCha20-Poly1305 credential encryption, SSH port-forwarding tunnels,
//!   SQL statement splitter/classifier, SQL formatter, connection URI parser, query parameter binder,
//!   destructive query safety analyzer, bookmarks & audit history manager, and multi-format exporters.
//! - [`adapters`]: Database adapter trait, static dialect registry (`PostgreSQL`, `Redshift`, `MySQL`,
//!   `MariaDB`, `SQLite`, `SQL Server`, `Cassandra`), and multi-database session facade.
//! - [`state`]: Central application store (`AppStore`) managing 14 state slices and async actions.
//! - [`gui`]: Native cross-platform Desktop GUI built on `eframe` / `egui`.
//! - [`tui`]: Interactive Terminal User Interface built on `ratatui` and `crossterm`.
//! - [`web`]: Embedded HTTP/IPC web server and single-page web application built on `axum`.

pub mod adapters;
pub mod common;
pub mod core;
pub mod gui;
pub mod state;
pub mod tui;
pub mod web;

pub use adapters::{
    adapter_factory, clear_select_limit, create_server, get_adapters, get_select_limit,
    register_adapter, set_select_limit, DatabaseAdapter, DatabaseConnection, DatabaseFacade,
    ServerSession, ADAPTERS, CLIENTS,
};
