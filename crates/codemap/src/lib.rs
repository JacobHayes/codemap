//! The codemap binary's surfaces: CLI and HTTP server.

mod args;
mod cli;
mod mcp;
mod render;
mod serve;
mod watch;

/// The release this binary was built as, set by the release workflow; `dev` anywhere else. It is
/// for people. The index's build id (in `codemap-core`) is what decides index compatibility.
pub(crate) const VERSION: &str = match option_env!("CODEMAP_VERSION") {
    Some(version) => version,
    None => "dev",
};

pub use cli::run;
