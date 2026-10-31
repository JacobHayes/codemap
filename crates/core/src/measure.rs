//! Filters and measures. Only base measures are stored (lines, tokens, nodes); a measure in a
//! query is a sum of one base under a filter, or a ratio of two such sums, computed when asked.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Narrows lines on any dimension. Every field that is set must match.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export, export_to = "Query.ts", optional_fields)]
pub struct Filter {
    /// A file or directory, relative to the root; matches it and everything under it.
    pub path: Option<String>,
    pub language: Option<String>,
    pub role: Option<String>,
    pub concern: Option<String>,
    /// The kind of the innermost symbol a line sits in.
    pub symbol_kind: Option<String>,
}

impl Filter {
    /// Whether no dimension is constrained.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// A SQL condition over `line_facts` with its bound values; `true` when empty.
    pub(crate) fn sql(&self) -> (String, Vec<String>) {
        let mut conditions = Vec::new();
        let mut values = Vec::new();
        if let Some(path) = self.path.as_deref().map(|p| p.trim_matches('/'))
            && !path.is_empty()
        {
            conditions.push("(path = ? OR starts_with(path, ?))".to_owned());
            values.push(path.to_owned());
            values.push(format!("{path}/"));
        }
        for (column, value) in [
            ("language", &self.language),
            ("role", &self.role),
            ("concern", &self.concern),
            ("symbol_kind", &self.symbol_kind),
        ] {
            if let Some(value) = value {
                // Files without a grammar have no language: "other" names them.
                if column == "language" && value == OTHER_LANGUAGE {
                    conditions.push("language IS NULL".to_owned());
                } else {
                    conditions.push(format!("{column} = ?"));
                    values.push(value.clone());
                }
            }
        }
        if conditions.is_empty() {
            ("true".to_owned(), values)
        } else {
            (conditions.join(" AND "), values)
        }
    }
}

/// How files with no grammar are named as a language.
pub const OTHER_LANGUAGE: &str = "other";

/// What is counted per line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "Query.ts")]
pub enum Base {
    Lines,
    Tokens,
    Nodes,
}

impl Base {
    /// The unit a value of this base is read in.
    #[must_use]
    pub fn unit(self) -> &'static str {
        match self {
            Self::Lines => "lines",
            Self::Tokens => "tokens",
            Self::Nodes => "syntax nodes",
        }
    }

    fn column(self) -> &'static str {
        match self {
            Self::Lines => "1",
            Self::Tokens => "tokens",
            Self::Nodes => "nodes",
        }
    }
}

/// A base measure summed over the lines that pass a filter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Query.ts", optional_fields)]
pub struct Term {
    pub base: Base,
    #[serde(default)]
    pub filter: Filter,
}

impl Term {
    #[must_use]
    pub fn new(base: Base, filter: Filter) -> Self {
        Self { base, filter }
    }

    /// `SUM(CASE WHEN <filter> THEN <base> ELSE 0 END)` and the values it binds.
    pub(crate) fn sql(&self) -> (String, Vec<String>) {
        let (condition, values) = self.filter.sql();
        (
            format!(
                "CAST(coalesce(sum(CASE WHEN {condition} THEN {} ELSE 0 END), 0) AS DOUBLE)",
                self.base.column()
            ),
            values,
        )
    }
}

/// A sum, or a ratio of two sums.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export, export_to = "Query.ts")]
pub enum Measure {
    Sum {
        base: Base,
        #[serde(default)]
        filter: Filter,
    },
    Ratio {
        numerator: Term,
        denominator: Term,
    },
}

/// A measure by catalog name, or written out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(untagged)]
#[ts(export, export_to = "Query.ts")]
#[expect(
    clippy::large_enum_variant,
    reason = "a wire type built once per query"
)]
pub enum MeasureRef {
    Named(String),
    Inline(Measure),
}

impl Measure {
    /// The sums this measure is made of: one for a sum, numerator and denominator for a ratio.
    #[must_use]
    pub fn terms(&self) -> Vec<Term> {
        match self {
            Self::Sum { base, filter } => vec![Term::new(*base, filter.clone())],
            Self::Ratio {
                numerator,
                denominator,
            } => vec![numerator.clone(), denominator.clone()],
        }
    }

    /// The value from the term sums; `None` for a ratio over nothing.
    #[must_use]
    pub fn value(&self, sums: &[f64]) -> Option<f64> {
        match self {
            Self::Sum { .. } => sums.first().copied(),
            Self::Ratio { .. } => {
                let (numerator, denominator) = (*sums.first()?, *sums.get(1)?);
                (denominator > 0.0).then(|| numerator / denominator)
            }
        }
    }

    /// Ratios have a meaningful middle at 1:1.
    #[must_use]
    pub fn is_ratio(&self) -> bool {
        matches!(self, Self::Ratio { .. })
    }
}

/// A named measure in the catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "Answer.ts")]
pub struct MeasureDef {
    pub name: String,
    pub description: String,
    pub measure: Measure,
}

fn sum(base: Base, dimension: &str, value: &str) -> Measure {
    let mut filter = Filter::default();
    match dimension {
        "role" => filter.role = Some(value.to_owned()),
        "concern" => filter.concern = Some(value.to_owned()),
        _ => {}
    }
    Measure::Sum { base, filter }
}

fn term(dimension: &str, value: &str) -> Term {
    let mut filter = Filter::default();
    match dimension {
        "role" => filter.role = Some(value.to_owned()),
        _ => filter.concern = Some(value.to_owned()),
    }
    Term::new(Base::Lines, filter)
}

/// The common measures, by name.
#[must_use]
pub fn catalog() -> Vec<MeasureDef> {
    let def = |name: &str, description: &str, measure| MeasureDef {
        name: name.to_owned(),
        description: description.to_owned(),
        measure,
    };
    vec![
        def("lines", "Lines of every kind", sum(Base::Lines, "", "")),
        def(
            "tokens",
            "Tokens, as a model reads them",
            sum(Base::Tokens, "", ""),
        ),
        def("nodes", "Syntax nodes", sum(Base::Nodes, "", "")),
        def(
            "handler_lines",
            "Lines of error handling",
            sum(Base::Lines, "concern", "handler"),
        ),
        def(
            "comment_lines",
            "Lines of comments",
            sum(Base::Lines, "concern", "comment"),
        ),
        def(
            "assertion_lines",
            "Lines of assertions",
            sum(Base::Lines, "concern", "assertion"),
        ),
        def(
            "logging_lines",
            "Lines of logging",
            sum(Base::Lines, "concern", "logging"),
        ),
        def(
            "test_lines",
            "Lines of test code",
            sum(Base::Lines, "role", "test"),
        ),
        def(
            "docs_lines",
            "Lines of documentation",
            sum(Base::Lines, "role", "docs"),
        ),
        def(
            "handler_to_core_ratio",
            "Error-handling lines per line of core logic",
            Measure::Ratio {
                numerator: term("concern", "handler"),
                denominator: term("concern", "core"),
            },
        ),
        def(
            "test_to_prod_ratio",
            "Test lines per line of production code",
            Measure::Ratio {
                numerator: term("role", "test"),
                denominator: term("role", "prod"),
            },
        ),
    ]
}

/// A catalog measure by name.
#[must_use]
pub fn named(name: &str) -> Option<Measure> {
    catalog()
        .into_iter()
        .find(|def| def.name == name)
        .map(|def| def.measure)
}

/// A readable label for a measure: its catalog name when it has one.
#[must_use]
pub fn label(reference: &MeasureRef) -> String {
    match reference {
        MeasureRef::Named(name) => name.clone(),
        MeasureRef::Inline(measure) => catalog()
            .into_iter()
            .find(|def| def.measure == *measure)
            .map_or_else(|| "custom measure".to_owned(), |def| def.name),
    }
}
