//! The per-file and per-directory totals every detector starts from, loaded once.

use std::collections::BTreeMap;

use codemap_lang::{Concern, Role};

use crate::reader::{Index, QueryError, SymbolRow};

/// Generated and vendored lines are reported by the overview but never counted toward an
/// opportunity: nobody can cut them by editing them.
pub(crate) fn is_ranked(role: &str) -> bool {
    role != Role::Generated.as_str() && role != Role::Vendored.as_str()
}

/// Lines and tokens of the ranked lines under one path, by concern and by role.
#[derive(Debug, Clone, Default)]
pub struct Agg {
    pub files: f64,
    pub lines: f64,
    pub tokens: f64,
    /// Lines by role (rows) and concern (columns).
    pub cells: [[f64; Concern::ALL.len()]; Role::ALL.len()],
}

impl Agg {
    fn add(&mut self, other: &Self) {
        self.files += other.files;
        self.lines += other.lines;
        self.tokens += other.tokens;
        for (mine, theirs) in self.cells.iter_mut().zip(other.cells) {
            for (a, b) in mine.iter_mut().zip(theirs) {
                *a += b;
            }
        }
    }

    /// Lines of a concern across every role.
    pub fn concern(&self, concern: Concern) -> f64 {
        let column = position(&Concern::ALL, concern);
        self.cells.iter().map(|row| row[column]).sum()
    }

    /// Lines of a role across every concern.
    pub fn role(&self, role: Role) -> f64 {
        self.cells[position(&Role::ALL, role)].iter().sum()
    }

    /// Lines of a concern within one role.
    pub fn cell(&self, role: Role, concern: Concern) -> f64 {
        self.cells[position(&Role::ALL, role)][position(&Concern::ALL, concern)]
    }
}

fn position<T: Copy + PartialEq>(all: &[T], item: T) -> usize {
    all.iter()
        .position(|&x| x == item)
        .expect("every variant is in ALL")
}

/// A function or test with its size.
#[derive(Debug, Clone)]
pub struct FunctionStat {
    pub symbol: SymbolRow,
    /// Names from the outermost parent down, joined with `::`.
    pub qualified: String,
    pub lines: f64,
    pub tokens: f64,
}

#[derive(Debug, Default)]
pub struct Corpus {
    /// Files with ranked lines.
    pub files: BTreeMap<String, Agg>,
    /// Every directory above those files, root excluded.
    pub dirs: BTreeMap<String, Agg>,
    pub total: Agg,
    pub functions: Vec<FunctionStat>,
}

/// The directories above a path, outermost first, root excluded.
pub(crate) fn ancestors(path: &str) -> impl Iterator<Item = &str> {
    path.match_indices('/').map(move |(at, _)| &path[..at])
}

impl Corpus {
    /// # Errors
    /// Fails when the index cannot be read.
    pub fn load(index: &Index) -> Result<Self, QueryError> {
        let mut corpus = Self::default();
        let mut statement = index.store().connection().prepare(
            "SELECT path, role, concern, count(*), CAST(sum(tokens) AS DOUBLE)
             FROM lines GROUP BY path, role, concern ORDER BY path, role, concern",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, f64>(3)?,
                row.get::<_, f64>(4)?,
            ))
        })?;
        for row in rows {
            let (path, role, concern, lines, tokens) = row?;
            if !is_ranked(&role) {
                continue;
            }
            let (Some(role), Some(concern)) = (Role::parse(&role), parse_concern(&concern)) else {
                continue;
            };
            let file = corpus.files.entry(path).or_default();
            file.lines += lines;
            file.tokens += tokens;
            file.cells[position(&Role::ALL, role)][position(&Concern::ALL, concern)] += lines;
        }
        for (path, file) in &mut corpus.files {
            file.files = 1.0;
            for dir in ancestors(path) {
                corpus.dirs.entry(dir.to_owned()).or_default().add(file);
            }
            corpus.total.add(file);
        }
        corpus.functions = functions(index, &corpus.files)?;
        Ok(corpus)
    }
}

fn parse_concern(name: &str) -> Option<Concern> {
    Concern::ALL
        .into_iter()
        .find(|concern| concern.as_str() == name)
}

fn functions(
    index: &Index,
    files: &BTreeMap<String, Agg>,
) -> Result<Vec<FunctionStat>, QueryError> {
    let symbols = index.symbols()?;
    let by_id: BTreeMap<&str, &SymbolRow> = symbols.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut statement = index.store().connection().prepare(
        "SELECT s.id, CAST(sum(l.tokens) AS DOUBLE) FROM symbols s
         JOIN lines l ON l.path = s.path AND l.line BETWEEN s.start_line AND s.end_line
         WHERE s.kind IN ('function', 'test') GROUP BY s.id",
    )?;
    let tokens: BTreeMap<String, f64> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut out = Vec::new();
    for symbol in &symbols {
        if !matches!(symbol.kind.as_str(), "function" | "test") || !files.contains_key(&symbol.path)
        {
            continue;
        }
        let mut names = vec![symbol.name.as_str()];
        let mut parent = symbol.parent.as_deref();
        while let Some(found) = parent.and_then(|id| by_id.get(id)) {
            names.push(found.name.as_str());
            parent = found.parent.as_deref();
        }
        names.reverse();
        out.push(FunctionStat {
            lines: f64::from(symbol.end_line - symbol.start_line + 1),
            tokens: tokens.get(&symbol.id).copied().unwrap_or(0.0),
            qualified: names.join("::"),
            symbol: symbol.clone(),
        });
    }
    Ok(out)
}
