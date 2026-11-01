//! The `DuckDB` index at `<root>/.codemap/index.duckdb`.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use duckdb::{Connection, params};

use crate::facts::FileFacts;

/// Directory holding everything codemap keeps for a project.
pub const STATE_DIR: &str = ".codemap";
pub(crate) const DATABASE_FILE: &str = "index.duckdb";
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
    start_line UINTEGER, end_line UINTEGER, content_hash VARCHAR,
    exact_hash UBIGINT, minhash BLOB, idents BLOB, forwards VARCHAR, abstraction BOOLEAN);
CREATE TABLE IF NOT EXISTS refs (
    path VARCHAR, line UINTEGER, name VARCHAR, kind VARCHAR, symbol VARCHAR);
CREATE TABLE IF NOT EXISTS switches (
    path VARCHAR, symbol VARCHAR, start_line UINTEGER, end_line UINTEGER, labels VARCHAR,
    wildcard BOOLEAN, table_like BOOLEAN, degenerate BOOLEAN, duplicated BOOLEAN);
CREATE TABLE IF NOT EXISTS edges (
    from_symbol VARCHAR, to_symbol VARCHAR, kind VARCHAR, confidence DOUBLE);
CREATE TABLE IF NOT EXISTS measures (subject VARCHAR, key VARCHAR, value DOUBLE);
CREATE TABLE IF NOT EXISTS \"similar\" (a VARCHAR, b VARCHAR, kind VARCHAR, score DOUBLE);
CREATE TABLE IF NOT EXISTS embeddings (content_hash VARCHAR, model VARCHAR, vector BLOB);
CREATE TABLE IF NOT EXISTS answers (
    hash_a VARCHAR, hash_b VARCHAR, model VARCHAR, prompt VARCHAR, probability DOUBLE);
CREATE TABLE IF NOT EXISTS duplicated (path VARCHAR, line UINTEGER);
CREATE TABLE IF NOT EXISTS flagged (path VARCHAR, line UINTEGER);
CREATE TABLE IF NOT EXISTS findings (
    detector VARCHAR, subject VARCHAR, subject_kind VARCHAR, area VARCHAR, reason VARCHAR,
    mass_lines DOUBLE, confidence DOUBLE, measures VARCHAR, labels VARCHAR, facts VARCHAR, evidence VARCHAR,
    acknowledged VARCHAR);
CREATE TABLE IF NOT EXISTS areas (
    subject VARCHAR, title VARCHAR, subject_kind VARCHAR, rank UINTEGER, opportunity DOUBLE, mass_lines DOUBLE,
    confidence DOUBLE, detectors VARCHAR, finding_count UINTEGER, reason VARCHAR,
    acknowledged VARCHAR);
CREATE OR REPLACE VIEW line_facts AS
    SELECT l.path, l.line, l.tokens, l.nodes, l.role, l.concern, l.symbol, f.language,
           s.kind AS symbol_kind, CAST(d.line IS NOT NULL AS UINTEGER) AS duplicated,
           CAST(g.line IS NOT NULL AS UINTEGER) AS flagged
    FROM lines l JOIN files f ON f.path = l.path LEFT JOIN symbols s ON s.id = l.symbol
         LEFT JOIN duplicated d ON d.path = l.path AND d.line = l.line
         LEFT JOIN flagged g ON g.path = l.path AND g.line = l.line;
";

/// Separates the labels of a switch in `switches.labels`: a control character no label holds.
pub(crate) const LABEL_SEPARATOR: &str = "\u{1f}";

/// Tables holding per-file rows, keyed by `path`.
const FILE_TABLES: [&str; 5] = ["lines", "symbols", "refs", "switches", "files"];
/// Tables the analyses and detectors rewrite from the whole index.
const DERIVED_TABLES: [&str; 7] = [
    "similar",
    "duplicated",
    "flagged",
    "edges",
    "measures",
    "findings",
    "areas",
];

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

/// A signature as the little-endian bytes of its hashes.
fn minhash_bytes(minhash: &[u32]) -> Vec<u8> {
    minhash.iter().flat_map(|hash| hash.to_le_bytes()).collect()
}

/// Appends the case sets of one file.
fn append_switches(
    rows: &mut duckdb::Appender<'_>,
    path: &str,
    facts: &FileFacts,
) -> Result<(), StoreError> {
    for switch in &facts.switches {
        rows.append_row(params![
            path,
            switch.symbol.map(|symbol| symbol_id(path, symbol)),
            switch.start_line,
            switch.end_line,
            switch.labels.join(LABEL_SEPARATOR),
            switch.wildcard,
            switch.table,
            switch.degenerate,
            switch.duplicated
        ])?;
    }
    Ok(())
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

    pub(crate) fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn begin(&self) -> Result<(), StoreError> {
        Ok(self.connection.execute_batch("BEGIN TRANSACTION")?)
    }

    pub fn commit(&self) -> Result<(), StoreError> {
        Ok(self.connection.execute_batch("COMMIT")?)
    }

    pub fn rollback(&self) -> Result<(), StoreError> {
        Ok(self.connection.execute_batch("ROLLBACK")?)
    }

    /// Another connection to the same database instance, with its own transactions.
    pub fn try_clone(&self) -> Result<Self, StoreError> {
        Ok(Self {
            connection: self.connection.try_clone()?,
        })
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

    /// Drops every table and recreates them, so that a new build's columns apply to an index an
    /// older build wrote.
    pub fn clear(&self) -> Result<(), StoreError> {
        self.connection
            .execute_batch("DROP VIEW IF EXISTS line_facts")?;
        for table in FILE_TABLES.into_iter().chain(DERIVED_TABLES) {
            self.connection
                .execute_batch(&format!("DROP TABLE IF EXISTS \"{table}\""))?;
        }
        Ok(self.connection.execute_batch(SCHEMA)?)
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
        let mut switch_rows = self.connection.appender("switches")?;
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
                    symbol.content_hash,
                    symbol.fingerprint.as_ref().map(|f| f.exact),
                    symbol
                        .fingerprint
                        .as_ref()
                        .map(|f| minhash_bytes(&f.minhash)),
                    symbol
                        .fingerprint
                        .as_ref()
                        .map(|f| minhash_bytes(&f.idents)),
                    symbol.forwards,
                    symbol.abstraction
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
            append_switches(&mut switch_rows, path, facts)?;
        }
        file_rows.flush()?;
        line_rows.flush()?;
        symbol_rows.flush()?;
        ref_rows.flush()?;
        switch_rows.flush()?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clone_reads_the_previous_state_until_the_writer_commits() {
        let dir = tempfile::tempdir().expect("temp dir");
        let writer = Store::open(dir.path()).expect("opens");
        let reader = writer.try_clone().expect("clones");
        let files = |store: &Store| store.totals().expect("counts").0;
        writer
            .connection()
            .execute_batch("INSERT INTO files VALUES ('a', 'h', 1, 'text', 'docs', false)")
            .expect("seeds");

        writer.begin().expect("begins");
        writer
            .connection()
            .execute_batch("INSERT INTO files VALUES ('b', 'h', 1, 'text', 'docs', false)")
            .expect("writes");
        assert_eq!(files(&writer), 2);
        assert_eq!(files(&reader), 1);

        writer.commit().expect("commits");
        assert_eq!(files(&reader), 2);
    }
}
