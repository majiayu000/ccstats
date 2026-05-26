//! Grok CLI data source
//!
//! Parses session signal summaries from ~/.grok/sessions/.

mod config;
mod parser;

pub(crate) use config::GrokSource;
