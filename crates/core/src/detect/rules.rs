//! Rules: the project's own thresholds. One generic detector driven by `[[rule]]` entries in the
//! config; without any it finds nothing.

use std::collections::BTreeMap;

use super::corpus::ancestors;
use super::{Context, Detector, Finding, SubjectKind, count};
use crate::config::{Granularity, Rule};
use crate::measure::{self, Filter, Measure};
use crate::reader::QueryError;

pub struct Rules;

impl Detector for Rules {
    fn name(&self) -> &'static str {
        "rules"
    }

    fn describe(&self) -> &'static str {
        "Places that break a threshold the project set in its config"
    }

    fn detect(&self, context: &Context) -> Result<Vec<Finding>, QueryError> {
        let mut findings = Vec::new();
        for rule in &context.config.rules {
            let measure = measure::named(&rule.measure)
                .ok_or_else(|| QueryError::UnknownMeasure(rule.measure.clone()))?;
            findings.extend(check(context, rule, &measure)?);
        }
        Ok(findings)
    }
}

/// A unit a rule applies to: where it is, how many lines it is, and its term sums.
struct Unit {
    subject: String,
    kind: SubjectKind,
    area: String,
    lines: f64,
    sums: Vec<f64>,
    evidence: Vec<(u32, u32)>,
}

fn check(context: &Context, rule: &Rule, measure: &Measure) -> Result<Vec<Finding>, QueryError> {
    let mut findings = Vec::new();
    for unit in units(context, rule.per, measure)? {
        let Some(value) = measure.value(&unit.sums) else {
            continue;
        };
        let (limit, direction) = match (rule.at_least, rule.at_most) {
            (Some(least), _) if value < least => (least, "at least"),
            (_, Some(most)) if value > most => (most, "at most"),
            _ => continue,
        };
        let per = match rule.per {
            Granularity::File => "file",
            Granularity::Function => "function",
            Granularity::Directory => "directory",
        };
        let reason = format!(
            "{} has {} for {}; the rule \"{}\" asks for {} {} per {}.",
            unit.subject,
            if measure.is_ratio() {
                format!("{value:.2}")
            } else {
                count(value)
            },
            rule.measure,
            rule.name,
            direction,
            limit,
            per,
        );
        let mut found = Finding::new("rules", unit.kind, &unit.subject, reason, unit.lines, 1.0)
            .measure("value", value)
            .measure("limit", limit);
        found = found.fact(format!("rule \"{}\"", rule.name)).fact(format!(
            "{} against {direction} {limit} per {per}",
            if measure.is_ratio() {
                format!("{value:.2}")
            } else {
                count(value)
            },
        ));
        found.labels.insert("rule".to_owned(), rule.name.clone());
        found
            .labels
            .insert("measure".to_owned(), rule.measure.clone());
        if !unit.evidence.is_empty() {
            found = found.evidence(&unit.area, &unit.evidence);
        }
        findings.push(found);
    }
    Ok(findings)
}

fn units(context: &Context, per: Granularity, measure: &Measure) -> Result<Vec<Unit>, QueryError> {
    let terms = measure.terms();
    let files = context.index.file_sums(&Filter::default(), &terms)?;
    let ranked = |path: &str| context.corpus.files.contains_key(path);
    match per {
        Granularity::File => Ok(files
            .into_iter()
            .filter(|(path, _)| ranked(path))
            .map(|(path, sums)| Unit {
                lines: context.corpus.files[&path].lines,
                subject: path.clone(),
                kind: SubjectKind::File,
                area: path,
                sums,
                evidence: Vec::new(),
            })
            .collect()),
        Granularity::Directory => {
            let mut dirs: BTreeMap<String, Vec<f64>> = BTreeMap::new();
            for (path, sums) in files.iter().filter(|(path, _)| ranked(path)) {
                for dir in ancestors(path) {
                    let total = dirs
                        .entry(dir.to_owned())
                        .or_insert_with(|| vec![0.0; sums.len()]);
                    total.iter_mut().zip(sums).for_each(|(a, b)| *a += b);
                }
            }
            Ok(dirs
                .into_iter()
                .map(|(dir, sums)| Unit {
                    lines: context.corpus.dirs[&dir].lines,
                    subject: dir.clone(),
                    kind: SubjectKind::Directory,
                    area: dir,
                    sums,
                    evidence: Vec::new(),
                })
                .collect())
        }
        Granularity::Function => {
            let sums: Vec<_> = context.index.symbol_sums(&Filter::default(), &terms)?;
            let zero = vec![0.0; terms.len()];
            let by_symbol: BTreeMap<String, Vec<f64>> = sums
                .into_iter()
                .filter_map(|(_, symbol, sums)| Some((symbol?, sums)))
                .collect();
            Ok(context
                .corpus
                .functions
                .iter()
                .map(|function| {
                    let symbol = &function.symbol;
                    Unit {
                        subject: format!("{}::{}", symbol.path, function.qualified),
                        kind: SubjectKind::Function,
                        area: symbol.path.clone(),
                        lines: function.lines,
                        sums: by_symbol.get(&symbol.id).unwrap_or(&zero).clone(),
                        evidence: vec![(symbol.start_line, symbol.end_line)],
                    }
                })
                .collect())
        }
    }
}
