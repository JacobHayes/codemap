//! `codemap mcp`: a stdio MCP server with one tool per query. Each tool takes its verb's CLI
//! arguments and returns the same versioned envelope, after the same refresh the CLI does.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use codemap_core::{Envelope, Query};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::transport::stdio;
use rmcp::{ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use tokio::sync::Mutex;

use crate::args::{AreaArgs, AreasArgs, SourceArgs, TreeArgs, XtabArgs};
use crate::cli::answer_at;

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("cannot start the MCP runtime: {0}")]
    Runtime(std::io::Error),
    #[error("MCP session failed: {0}")]
    Session(String),
}

#[derive(Clone)]
struct Codemap {
    root: PathBuf,
    /// Tool calls refresh the index before reading it, and only one refresh may run at a time.
    busy: Arc<Mutex<()>>,
}

impl Codemap {
    async fn ask(&self, query: Query) -> CallToolResult {
        let _turn = self.busy.lock().await;
        let root = self.root.clone();
        let result = tokio::task::spawn_blocking(move || {
            let answer = answer_at(&root, &query).map_err(|error| error.to_string())?;
            serde_json::to_string(&Envelope::from(answer)).map_err(|error| error.to_string())
        })
        .await
        .unwrap_or_else(|error| Err(error.to_string()));
        match result {
            Ok(json) => CallToolResult::success(vec![ContentBlock::text(json)]),
            Err(message) => CallToolResult::error(vec![ContentBlock::text(message)]),
        }
    }
}

#[tool_router]
impl Codemap {
    #[tool(
        description = "Start here: totals and shares by role, concern and language, and counts of ranked areas by detector."
    )]
    async fn overview(&self) -> CallToolResult {
        self.ask(Query::Overview).await
    }

    #[tool(
        description = "Where the lines are: a directory/file (or file/symbol) hierarchy sized by a measure and coloured by another. Use to drill into a part of the tree."
    )]
    async fn tree(&self, Parameters(args): Parameters<TreeArgs>) -> CallToolResult {
        self.ask(args.query()).await
    }

    #[tool(
        description = "Two dimensions crossed (role, concern, language, kind, path) with one measure per cell. Use to see composition, such as concern by role."
    )]
    async fn xtab(&self, Parameters(args): Parameters<XtabArgs>) -> CallToolResult {
        self.ask(args.query()).await
    }

    #[tool(
        description = "Ranked areas worth a closer look, each with the detectors that flagged it and why. A group (copies, a naming family, a repeated switch) is one area, whatever the number of files. Use after overview to pick where to investigate."
    )]
    async fn areas(&self, Parameters(args): Parameters<AreasArgs>) -> CallToolResult {
        self.ask(args.query()).await
    }

    #[tool(
        description = "One area's findings, evidence line ranges and related areas; for a file or directory also the groups (copies, naming families, repeated switches) it belongs to. Pass a path or a group id exactly as `areas` printed it."
    )]
    async fn area(&self, Parameters(args): Parameters<AreaArgs>) -> CallToolResult {
        self.ask(args.query()).await
    }

    #[tool(
        description = "A file's lines with role and concern per line, plus the finding ranges that touch them. Use to read the code behind a finding; pass a line range for long files."
    )]
    async fn source(&self, Parameters(args): Parameters<SourceArgs>) -> CallToolResult {
        match args.query() {
            Ok(query) => self.ask(query).await,
            Err(message) => CallToolResult::error(vec![ContentBlock::text(message)]),
        }
    }

    #[tool(
        description = "The dimensions and values filters accept, the named measures, and the detectors. Use to learn valid arguments."
    )]
    async fn catalog(&self) -> CallToolResult {
        self.ask(Query::Catalog).await
    }
}

#[expect(
    clippy::unused_async_trait_impl,
    reason = "the macro generates the async tool methods"
)]
#[tool_handler]
impl ServerHandler for Codemap {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("codemap", crate::VERSION))
            .with_instructions(
                "Profile a codebase's mass and structure: start with overview, then areas, then area and source.",
            )
    }
}

/// Serves MCP on stdin and stdout until the client disconnects.
///
/// # Errors
/// Fails when the runtime cannot start or the session ends abnormally.
pub fn run(root: &Path) -> Result<(), McpError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(McpError::Runtime)?;
    runtime.block_on(async {
        let server = Codemap {
            root: root.to_owned(),
            busy: Arc::new(Mutex::new(())),
        };
        let running = server
            .serve(stdio())
            .await
            .map_err(|error| McpError::Session(error.to_string()))?;
        running
            .waiting()
            .await
            .map(|_| ())
            .map_err(|error| McpError::Session(error.to_string()))
    })
}
