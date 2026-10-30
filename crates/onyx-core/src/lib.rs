//! Onyx's core: everything without a window. The app crate draws it;
//! the CLI (`onyx-core`) runs each part from a terminal.

pub mod config;
pub mod paths;
pub mod process;
pub mod search;
pub mod update;
pub mod vault;
