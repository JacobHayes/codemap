//! Duplication: functions and tests that are copies of each other, exactly or nearly.
//!
//! The similarity analysis pairs the copies; this detector reads each connected group as one
//! observation. A group is a finding in every file that holds a member, so the file's area can
//! agree with other detectors, and each of those findings lists all the members as evidence so
//! that every copy can be opened. Tests are held to the same engine; the reason says when the
//! members are tests.

use super::{Context, Detector, Finding, SubjectKind, count, percent};
use crate::reader::QueryError;
use crate::similarity::{Group, Member, groups};

pub struct Duplication;

impl Detector for Duplication {
    fn name(&self) -> &'static str {
        "duplication"
    }

    fn describe(&self) -> &'static str {
        "Functions and tests copied exactly, or nearly once names and constants are ignored"
    }

    fn detect(&self, context: &Context) -> Result<Vec<Finding>, QueryError> {
        Ok(groups(context.index)?
            .iter()
            .flat_map(findings_of)
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
    let (near, same) = if group.exact {
        ("", "the same")
    } else {
        ("near ", "a")
    };
    format!(
        "{} {near}copies of {same} {size} {noun} in {files} file{}.",
        group.members.len(),
        if files == 1 { "" } else { "s" },
    )
}

fn number(value: usize) -> f64 {
    f64::from(u32::try_from(value).unwrap_or(u32::MAX))
}

/// One finding per file that holds a member of the group.
fn findings_of(group: &Group) -> Vec<Finding> {
    let noun = if group.is_test() { "test" } else { "function" };
    let first = &group.members[0];
    let subject = format!("{}::{}#{}", first.path, first.name, first.start_line);
    let (total, duplicated) = (group.lines_total(), group.lines_duplicated());
    let mut files: Vec<&str> = group.members.iter().map(|m| m.path.as_str()).collect();
    files.dedup();
    files
        .into_iter()
        .map(|file| {
            let here: u32 = group
                .members
                .iter()
                .filter(|member| member.path == file)
                .map(Member::lines)
                .sum();
            let mut finding = Finding::new(
                "duplication",
                SubjectKind::Group,
                &subject,
                reason(group, noun),
                f64::from(here),
                if group.exact { 1.0 } else { group.score_min },
            )
            .measure("members", number(group.members.len()))
            .measure("files", number(group.file_count()))
            .measure("totalLines", f64::from(total))
            .measure("duplicatedLines", f64::from(duplicated))
            .measure(
                "largestLines",
                f64::from(group.members[group.largest()].lines()),
            )
            .measure("similarity", group.score_min)
            .fact(format!(
                "{} lines in all, {} beyond the largest copy",
                count(f64::from(total)),
                count(f64::from(duplicated))
            ))
            .fact(if group.exact {
                "identical apart from whitespace and comments".to_owned()
            } else {
                format!(
                    "at least {} alike once names and constants are ignored",
                    percent(group.score_min)
                )
            });
            file.clone_into(&mut finding.area);
            finding.labels.insert("kind".to_owned(), noun.to_owned());
            finding.labels.insert(
                "match".to_owned(),
                if group.exact { "exact" } else { "near" }.to_owned(),
            );
            for member in &group.members {
                finding = finding.evidence(&member.path, &[(member.start_line, member.end_line)]);
            }
            finding
        })
        .collect()
}
