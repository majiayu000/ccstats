//! Cursor data source
//!
//! Parses read-only `SQLite` state databases from Cursor's local user data dir.

mod config;
mod parser;

pub(crate) use config::CursorSource;
