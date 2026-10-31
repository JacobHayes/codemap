//! A read handle on the index, and the aggregates every query and detector is built from.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use duckdb::params_from_iter;

use crate::config::ConfigError;
use crate::measure::{Filter, Term};
use crate::store::{DATABASE_FILE, STATE_DIR, Store, StoreError};

#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("there is no index under {0}: run `codemap index` first")]
    NoIndex(String),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("index database: {0}")]
    Database(#[from] duckdb::Error),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("unknown measure {0:?}: see the catalog")]
    UnknownMeasure(String),
    #[error("the size of a tile must be a sum, not a ratio")]
    SizeIsRatio,
    #[error("no area for {0:?}")]
    UnknownArea(String),
    #[error("unknown {dimension} {value:?}")]
    UnknownValue {
        dimension: &'static str,
        value: String,
    },
    #[error("cannot encode a finding: {0}")]
    Encode(#[from] serde_json::Error),
}

/// The key values of a group and one sum per term.
pub(crate) type Group = (Vec<Option<String>>, Vec<f64>);
/// A file, the innermost symbol (none outside symbols) and one sum per term.
pub(crate) type SymbolSums = (String, Option<String>, Vec<f64>);

/// A symbol as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolRow {
    pub id: String,
    pub path: String,
    pub name: String,
    pub kind: String,
    pub parent: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
    /// The role of the symbol's first line.
    pub role: String,
    /// The callee a function only forwards its arguments to.
    pub forwards: Option<String>,
    /// A trait, interface or abstract class.
    pub abstraction: bool,
}

/// The index under one root.
pub struct Index {
    store: Store,
    root: PathBuf,
}

impl Index {
    /// Opens an existing index.
    ///
    /// # Errors
    /// Fails when there is no index under `root` or it cannot be opened.
    pub fn open(root: &Path) -> Result<Self, QueryError> {
        if !root.join(STATE_DIR).join(DATABASE_FILE).exists() {
            return Err(QueryError::NoIndex(root.display().to_string()));
        }
        Ok(Self::new(Store::open(root)?, root))
    }

    /// Another handle on the same open database, with its own transactions. Opening the file a
    /// second time would be a separate instance that never sees this one's writes, so every
    /// reader of a running refresh comes from here.
    ///
    /// # Errors
    /// Fails when the database cannot hand out another connection.
    pub fn try_clone(&self) -> Result<Self, QueryError> {
        Ok(Self::new(self.store.try_clone()?, &self.root))
    }

    pub(crate) fn new(store: Store, root: &Path) -> Self {
        Self {
            store,
            root: root.to_owned(),
        }
    }

    pub(crate) fn store(&self) -> &Store {
        &self.store
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The name of the root directory, for the top of a hierarchy.
    #[must_use]
    pub fn root_name(&self) -> String {
        std::fs::canonicalize(&self.root)
            .ok()
            .and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| ".".to_owned())
    }

    /// Runs `SELECT <key columns>, <term sums> FROM line_facts WHERE <filter> GROUP BY <keys>`,
    /// returning the key values and one sum per term.
    pub(crate) fn grouped(
        &self,
        keys: &[&str],
        filter: &Filter,
        terms: &[Term],
    ) -> Result<Vec<Group>, QueryError> {
        let mut values = Vec::new();
        let mut select: Vec<String> = keys
            .iter()
            .map(|key| format!("CAST({key} AS VARCHAR)"))
            .collect();
        for term in terms {
            let (sql, bound) = term.sql();
            select.push(sql);
            values.extend(bound);
        }
        let (condition, bound) = filter.sql();
        values.extend(bound);
        let group = if keys.is_empty() {
            String::new()
        } else {
            format!("GROUP BY {}", keys.join(", "))
        };
        let sql = format!(
            "SELECT {} FROM line_facts WHERE {condition} {group} ORDER BY {}",
            select.join(", "),
            (1..=keys.len().max(1))
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(", "),
        );
        let mut statement = self.store.connection().prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values.iter()), |row| {
            let key = (0..keys.len())
                .map(|i| row.get::<_, Option<String>>(i))
                .collect::<Result<Vec<_>, _>>()?;
            let sums = (0..terms.len())
                .map(|i| row.get::<_, f64>(keys.len() + i))
                .collect::<Result<Vec<_>, _>>()?;
            Ok((key, sums))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Term sums per file, for files that have lines passing the filter.
    pub(crate) fn file_sums(
        &self,
        filter: &Filter,
        terms: &[Term],
    ) -> Result<BTreeMap<String, Vec<f64>>, QueryError> {
        Ok(self
            .grouped(&["path"], filter, terms)?
            .into_iter()
            .filter_map(|(key, sums)| Some((key.into_iter().next()??, sums)))
            .collect())
    }

    /// Term sums per innermost symbol of each file; `None` for lines outside any symbol.
    pub(crate) fn symbol_sums(
        &self,
        filter: &Filter,
        terms: &[Term],
    ) -> Result<Vec<SymbolSums>, QueryError> {
        Ok(self
            .grouped(&["path", "symbol"], filter, terms)?
            .into_iter()
            .filter_map(|(mut key, sums)| {
                let symbol = key.pop()?;
                Some((key.pop()??, symbol, sums))
            })
            .collect())
    }

    /// Every symbol, in file and line order.
    pub(crate) fn symbols(&self) -> Result<Vec<SymbolRow>, QueryError> {
        let mut statement = self.store.connection().prepare(
            "SELECT s.id, s.path, s.name, s.kind, s.parent, s.start_line, s.end_line,
                    coalesce(l.role, 'prod'), s.forwards, coalesce(s.abstraction, false)
             FROM symbols s LEFT JOIN lines l ON l.path = s.path AND l.line = s.start_line
             ORDER BY s.path, s.start_line, s.end_line DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(SymbolRow {
                id: row.get(0)?,
                path: row.get(1)?,
                name: row.get(2)?,
                kind: row.get(3)?,
                parent: row.get(4)?,
                start_line: row.get(5)?,
                end_line: row.get(6)?,
                role: row.get(7)?,
                forwards: row.get(8)?,
                abstraction: row.get(9)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The longest runs of consecutive lines of a file where `column` equals `value`.
    pub(crate) fn runs(
        &self,
        path: &str,
        column: &str,
        value: &str,
        limit: usize,
    ) -> Result<Vec<(u32, u32)>, QueryError> {
        let sql = format!(
            "SELECT min(line), max(line) FROM (
                 SELECT line, line - row_number() OVER (ORDER BY line) AS island
                 FROM lines WHERE path = ? AND {column} = ?)
             GROUP BY island ORDER BY max(line) - min(line) DESC, min(line) LIMIT {limit}"
        );
        let mut statement = self.store.connection().prepare(&sql)?;
        let rows = statement.query_map([path, value], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}
