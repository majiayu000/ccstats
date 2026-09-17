//! Cursor data source
//!
//! Reads usage events from Cursor's Admin API or dashboard usage API.

mod client;
mod config;
mod parser;

pub(crate) use client::{CursorPlanUsage, fetch_plan_usage as fetch_cursor_plan_usage};
pub(crate) use config::CursorSource;
