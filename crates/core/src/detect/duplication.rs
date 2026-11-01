//! Duplication: functions and tests that are copies of each other, exactly or nearly, or that a
//! model confirmed do the same thing.
//!
//! The similarity analysis pairs the copies; this detector reads each connected group as one
//! finding, which is its own area: its mass is its copies' lines counted once and its evidence
//! lists every copy. The group's id is a digest of the longest copy's content, so it holds while
//! the other copies are edited, added or dropped. Tests are held to the same engine; the reason
//! says when the members are tests.

use super::{
    Context, Detector, Finding, count, distinct_lines_by_path, group_id, percent, places_title,
};
use crate::reader::QueryError;
use crate::similarity::{Group, Member, groups};

/// A group with fewer duplicated lines than this is not an area: a pair of ten-line functions is
/// in `similar` and can be asked for, but a list of hundreds of such pairs buries the groups worth
/// a look. It keeps a pair of dozen-line test bodies and drops the helper-sized look-alikes.
pub(crate) const DUPLICATED_LINES_MIN: u32 = 12;

/// The same bar for a group whose copies are all identical (apart from whitespace and comments).
/// An exact pair is a copy nobody had to notice was changed, so even a short one is worth a
/// glance; near copies of that size are mostly functions that share a shape.
pub(crate) const EXACT_DUPLICATED_LINES_MIN: u32 = 6;

pub struct Duplication;

impl Detector for Duplication {
    fn name(&self) -> &'static str {
        "duplication"
    }

    fn describe(&self) -> &'static str {
        "Functions and tests copied exactly, or nearly once names and constants are ignored, or confirmed by a model to do the same thing"
    }

    fn detect(&self, context: &Context) -> Result<Vec<Finding>, QueryError> {
        Ok(groups(context.index)?
            .iter()
            .filter(|group| {
                let floor = if group.exact {
                    EXACT_DUPLICATED_LINES_MIN
                } else {
                    DUPLICATED_LINES_MIN
                };
                group.lines_duplicated() >= floor
            })
            .map(finding_of)
            .collect())
    }
}

fn reason(group: &Group, noun: &str) -> String {
    let files = group.file_count();
    let lines: Vec<u32> = group.members.iter().map(Member::lines).collect();
    let longest = lines.iter().copied().max().unwrap_or(0);
    let shortest = lines.iter().copied().min().unwrap_or(0);
    let size = if shortest == longest {
        format!("{longest}-line")
    } else {
        format!("{shortest} to {longest} line")
    };
    let plural = if files == 1 { "" } else { "s" };
    if group.semantic {
        return format!(
            "{} {noun}s that do the same thing in different words, in {files} file{plural}.",
            group.members.len(),
        );
    }
    let (near, same) = if group.exact {
        ("", "the same")
    } else {
        ("near ", "a")
    };
    format!(
        "{} {near}copies of {same} {size} {noun} in {files} file{plural}.",
        group.members.len(),
    )
}

fn number(value: usize) -> f64 {
    f64::from(u32::try_from(value).unwrap_or(u32::MAX))
}

/// One finding for the whole group: its mass is the lines duplicated beyond the largest copy,
/// and its evidence lists every copy.
fn finding_of(group: &Group) -> Finding {
    let noun = if group.is_test() { "test" } else { "function" };
    let (total, duplicated) = (group.lines_total(), group.lines_duplicated());
    let mass = f64::from(
        distinct_lines_by_path(
            group
                .members
                .iter()
                .map(|m| (m.path.as_str(), m.start_line, m.end_line)),
        )
        .values()
        .sum::<u32>(),
    );
    let largest = &group.members[group.largest()];
    // The longest copy's content names the group: it holds while the other copies are edited,
    // added or dropped.
    let key = [largest.content_hash.clone()];
    let prefix = if group.semantic {
        format!("Same {noun} in different words")
    } else {
        format!("Copies of {}", largest.name)
    };
    let mut finding = Finding::group(
        "duplication",
        &group_id("duplication", &key),
        places_title(&prefix, group.members.len(), group.file_count()),
        reason(group, noun),
        mass,
        if group.exact { 1.0 } else { group.score_min },
    )
    .measure("members", number(group.members.len()))
    .measure("files", number(group.file_count()))
    .measure("totalLines", f64::from(total))
    .measure("duplicatedLines", f64::from(duplicated))
    .measure("largestLines", f64::from(largest.lines()))
    .measure("similarity", group.score_min)
    .fact(format!(
        "{} lines in all, {} beyond the largest copy",
        count(f64::from(total)),
        count(f64::from(duplicated))
    ))
    .fact(if group.exact {
        "identical apart from whitespace and comments".to_owned()
    } else if let Some(probability) = group.model_score_min {
        format!(
            "a model judged the weakest semantic link at least {} likely to do the same thing",
            percent(probability)
        )
    } else {
        format!(
            "at least {} alike once names and constants are ignored",
            percent(group.score_min)
        )
    });
    finding = finding.members(group.members.iter().map(|m| m.key.clone()));
    finding.labels.insert("kind".to_owned(), noun.to_owned());
    finding.labels.insert(
        "match".to_owned(),
        match (group.exact, group.semantic) {
            (true, _) => "exact",
            (false, false) => "near",
            (false, true) => "semantic",
        }
        .to_owned(),
    );
    for member in &group.members {
        finding = finding.evidence(&member.path, &[(member.start_line, member.end_line)]);
    }
    finding
}
