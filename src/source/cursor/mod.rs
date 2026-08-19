//! Cursor data source
//!
//! Reads usage events from Cursor's Admin API or dashboard usage API.

mod client;
mod config;
mod parser;

pub(crate) use config::CursorSource;
