//! The codemap binary's surfaces: CLI and HTTP server.

mod args;
mod cli;
mod mcp;
mod render;
mod serve;
mod watch;

pub use cli::run;
