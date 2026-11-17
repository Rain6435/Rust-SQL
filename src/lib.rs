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
