//! The codemap binary's surfaces: CLI and HTTP server.

mod cli;
mod render;
mod serve;

pub use cli::run;
