//! The `DuckDB` index at `<root>/.codemap/index.duckdb`.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use duckdb::{Connection, params};

use crate::facts::FileFacts;

/// Directory holding everything codemap keeps for a project.
pub const STATE_DIR: &str = ".codemap";
const DATABASE_FILE: &str = "index.duckdb";
/// Ignores everything in the state directory except the committed config and itself.
const STATE_GITIGNORE: &str = "*\n!.gitignore\n!config.toml\n";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (key VARCHAR, value VARCHAR);
CREATE TABLE IF NOT EXISTS files (
    path VARCHAR, hash VARCHAR, size_bytes UBIGINT, language VARCHAR, role VARCHAR, is_binary BOOLEAN);
CREATE TABLE IF NOT EXISTS lines (
    path VARCHAR, line UINTEGER, tokens UINTEGER, nodes UINTEGER, depth UINTEGER,
    role VARCHAR, concern VARCHAR, symbol VARCHAR);
CREATE TABLE IF NOT EXISTS symbols (
    id VARCHAR, path VARCHAR, name VARCHAR, kind VARCHAR, parent VARCHAR,
    start_line UINTEGER, end_line UINTEGER, content_hash VARCHAR);
CREATE TABLE IF NOT EXISTS refs (
    path VARCHAR, line UINTEGER, name VARCHAR, kind VARCHAR, symbol VARCHAR);
";

/// Tables holding per-file rows, keyed by `path`.
const FILE_TABLES: [&str; 4] = ["lines", "symbols", "refs", "files"];

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("cannot prepare {path}: {source}")]
    Prepare { path: String, source: io::Error },
    #[error("index database: {0}")]
    Database(#[from] duckdb::Error),
}

pub struct Store {
    connection: Connection,
}

/// A symbol id is unique across the index: the file path and the symbol's place in the file.
fn symbol_id(path: &str, index: u32) -> String {
    format!("{path}#{index}")
}

impl Store {
    /// Opens the index under `root`, creating it and `.codemap/.gitignore` when missing.
    ///
    /// # Errors
    /// Fails when the state directory or database cannot be created or opened.
    pub fn open(root: &Path) -> Result<Self, StoreError> {
        let dir = root.join(STATE_DIR);
        let prepare = |source| StoreError::Prepare {
            path: dir.display().to_string(),
            source,
        };
        fs::create_dir_all(&dir).map_err(prepare)?;
        let ignore = dir.join(".gitignore");
        if !ignore.exists() {
            fs::write(&ignore, STATE_GITIGNORE).map_err(prepare)?;
        }
        let connection = Connection::open(dir.join(DATABASE_FILE))?;
        connection.execute_batch(SCHEMA)?;
        Ok(Self { connection })
    }

    pub fn begin(&self) -> Result<(), StoreError> {
        Ok(self.connection.execute_batch("BEGIN TRANSACTION")?)
    }

    pub fn commit(&self) -> Result<(), StoreError> {
        Ok(self.connection.execute_batch("COMMIT")?)
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>, StoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT value FROM meta WHERE key = ?")?;
        let mut rows = statement.query_map(params![key], |row| row.get::<_, String>(0))?;
        Ok(rows.next().transpose()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.connection
            .execute("DELETE FROM meta WHERE key = ?", params![key])?;
        self.connection
            .execute("INSERT INTO meta VALUES (?, ?)", params![key, value])?;
        Ok(())
    }

    /// Stored content hash of every indexed file.
    pub fn file_hashes(&self) -> Result<BTreeMap<String, String>, StoreError> {
        let mut statement = self.connection.prepare("SELECT path, hash FROM files")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Drops every per-file row.
    pub fn clear(&self) -> Result<(), StoreError> {
        for table in FILE_TABLES {
            self.connection
                .execute(&format!("DELETE FROM {table}"), [])?;
        }
        Ok(())
    }

    /// Drops the rows of these files.
    pub fn delete_files(&self, paths: &[&str]) -> Result<(), StoreError> {
        if paths.is_empty() {
            return Ok(());
        }
        self.connection
            .execute_batch("CREATE OR REPLACE TEMP TABLE gone (path VARCHAR)")?;
        let mut appender = self.connection.appender("gone")?;
        for path in paths {
            appender.append_row(params![path])?;
        }
        appender.flush()?;
        drop(appender);
        for table in FILE_TABLES {
            self.connection.execute(
                &format!("DELETE FROM {table} WHERE path IN (SELECT path FROM gone)"),
                [],
            )?;
        }
        self.connection.execute_batch("DROP TABLE gone")?;
        Ok(())
    }

    /// Bulk-loads the rows of the extracted files.
    pub fn insert<'a>(
        &self,
        files: impl IntoIterator<Item = (&'a str, &'a str, &'a FileFacts)>,
    ) -> Result<(), StoreError> {
        let mut file_rows = self.connection.appender("files")?;
        let mut line_rows = self.connection.appender("lines")?;
        let mut symbol_rows = self.connection.appender("symbols")?;
        let mut ref_rows = self.connection.appender("refs")?;
        for (path, hash, facts) in files {
            file_rows.append_row(params![
                path,
                hash,
                facts.size_bytes,
                facts.language,
                facts.role.as_str(),
                facts.binary
            ])?;
            for (index, line) in facts.lines.iter().enumerate() {
                line_rows.append_row(params![
                    path,
                    u32::try_from(index + 1).unwrap_or(u32::MAX),
                    line.token_count,
                    line.node_count,
                    line.depth,
                    line.role.as_str(),
                    line.concern.as_str(),
                    line.symbol.map(|symbol| symbol_id(path, symbol))
                ])?;
            }
            for (index, symbol) in facts.symbols.iter().enumerate() {
                symbol_rows.append_row(params![
                    symbol_id(path, u32::try_from(index).unwrap_or(u32::MAX)),
                    path,
                    symbol.name,
                    symbol.kind.as_str(),
                    symbol.parent.map(|parent| symbol_id(path, parent)),
                    symbol.start_line,
                    symbol.end_line,
                    symbol.content_hash
                ])?;
            }
            for reference in &facts.refs {
                ref_rows.append_row(params![
                    path,
                    reference.line,
                    reference.name,
                    reference.kind,
                    reference.symbol.map(|symbol| symbol_id(path, symbol))
                ])?;
            }
        }
        file_rows.flush()?;
        line_rows.flush()?;
        symbol_rows.flush()?;
        ref_rows.flush()?;
        Ok(())
    }

    /// Number of indexed files and of lines.
    pub fn totals(&self) -> Result<(u64, u64), StoreError> {
        let count = |table: &str| -> Result<u64, StoreError> {
            Ok(self
                .connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })?)
        };
        Ok((count("files")?, count("lines")?))
    }
}
