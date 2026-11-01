//! Human text for each answer (DESIGN, Terminal output): one totals line, aligned tables,
//! numbers right aligned, paths relative to the root. Nothing here computes a value.

use std::fmt::Write as _;

use codemap_core::{
    Answer, Area, AreaDetail, AreaList, Catalog, Finding, Hidden, Overview, SOURCE_LINES_MAX,
    Share, Source, SubjectKind, Tree, TreeNode, Xtab,
};

/// Children listed under one tile before the rest are summarised.
const TREE_CHILDREN_MAX: usize = 12;

#[derive(Clone, Copy, PartialEq)]
enum Align {
    Left,
    Right,
}

/// Rows of cells under headers, padded to their column's widest cell.
fn table(headers: &[(&str, Align)], rows: &[Vec<String>]) -> String {
    let widths: Vec<usize> = (0..headers.len())
        .map(|col| {
            rows.iter()
                .map(|row| row[col].chars().count())
                .chain([headers[col].0.chars().count()])
                .max()
                .unwrap_or(0)
        })
        .collect();
    let line = |cells: Vec<&str>| {
        let padded: Vec<String> = cells
            .iter()
            .enumerate()
            .map(|(col, cell)| match headers[col].1 {
                Align::Left => format!("{cell:<width$}", width = widths[col]),
                Align::Right => format!("{cell:>width$}", width = widths[col]),
            })
            .collect();
        format!("{}\n", padded.join("  ").trim_end())
    };
    let mut out = line(headers.iter().map(|(name, _)| *name).collect());
    for row in rows {
        out.push_str(&line(row.iter().map(String::as_str).collect()));
    }
    out
}

/// Digits with thousands separators; a leading minus sign is kept.
fn grouped(digits: &str) -> String {
    let (sign, digits) = digits
        .strip_prefix('-')
        .map_or(("", digits), |rest| ("-", rest));
    let mut out = String::from(sign);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// A count with thousands separators.
fn whole(value: u64) -> String {
    grouped(&value.to_string())
}

/// A measure value: thousands separators, and two places when it is not a whole number.
fn number(value: f64) -> String {
    if value.fract() == 0.0 {
        return grouped(&format!("{value:.0}"));
    }
    let text = format!("{value:.2}");
    let (integer, fraction) = text.split_once('.').unwrap_or((&text, ""));
    format!("{}.{fraction}", grouped(integer))
}

fn percent(share: f64) -> String {
    format!("{:.1}%", share * 100.0)
}

/// Renders an answer as terminal text.
#[must_use]
pub fn render(answer: &Answer) -> String {
    match answer {
        Answer::Overview(overview) => overview_text(overview),
        Answer::Tree(tree) => tree_text(tree),
        Answer::Xtab(xtab) => xtab_text(xtab),
        Answer::Source(source) => source_text(source),
        Answer::Areas(list) => areas_text(list),
        Answer::Area(detail) => area_text(detail),
        Answer::Catalog(catalog) => catalog_text(catalog),
    }
}

fn shares_table(title: &str, shares: &[Share]) -> String {
    let rows: Vec<Vec<String>> = shares
        .iter()
        .map(|s| {
            vec![
                s.name.clone(),
                whole(s.lines),
                whole(s.tokens),
                percent(s.share),
            ]
        })
        .collect();
    let headers = [
        (title, Align::Left),
        ("lines", Align::Right),
        ("tokens", Align::Right),
        ("share", Align::Right),
    ];
    table(&headers, &rows)
}

fn overview_text(overview: &Overview) -> String {
    let t = &overview.totals;
    let mut out = format!(
        "{} files, {} lines, {} tokens, {} symbols; {} areas ({} acknowledged)\n",
        whole(t.files),
        whole(t.lines),
        whole(t.tokens),
        whole(t.symbols),
        overview.areas.total,
        overview.areas.acknowledged,
    );
    for (title, shares) in [
        ("role", &overview.by_role),
        ("concern", &overview.by_concern),
        ("language", &overview.by_language),
    ] {
        out.push('\n');
        out.push_str(&shares_table(title, shares));
    }
    let rows: Vec<Vec<String>> = overview
        .areas
        .by_detector
        .iter()
        .map(|d| vec![d.detector.clone(), d.areas.to_string()])
        .collect();
    out.push('\n');
    out.push_str(&table(
        &[("detector", Align::Left), ("areas", Align::Right)],
        &rows,
    ));
    out.push('\n');
    out.push_str(&overview.semantic);
    out.push('\n');
    out
}

/// Cross-tabs list their largest rows first; a cell with nothing in it shows a dot.
/// The line saying what the default population left out, and how to bring it back.
fn hidden_note(hidden: Option<&Hidden>) -> String {
    hidden.map_or_else(String::new, |hidden| {
        format!(
            "{} lines of {} code hidden; --include-generated counts them\n",
            whole(u64::from(hidden.lines)),
            hidden.roles.join(" and ")
        )
    })
}

fn xtab_text(xtab: &Xtab) -> String {
    let measure = if xtab.measure.measure == xtab.measure.unit {
        String::new()
    } else {
        format!(" ({})", xtab.measure.measure)
    };
    let mut out = format!(
        "{} {}{measure}, {} rows by {} columns\n",
        number(xtab.total),
        xtab.measure.unit,
        xtab.rows.len(),
        xtab.cols.len(),
    );
    out.push_str(&hidden_note(xtab.hidden.as_ref()));
    let mut headers = vec![(format!("{:?}", xtab.row_axis).to_lowercase(), Align::Left)];
    headers.extend(xtab.cols.iter().map(|c| (c.name.clone(), Align::Right)));
    headers.push(("total".to_owned(), Align::Right));
    let mut grid: Vec<Vec<String>> = xtab
        .rows
        .iter()
        .map(|row| {
            let mut cells = vec![row.name.clone()];
            cells.extend(xtab.cols.iter().map(|_| "·".to_owned()));
            cells.push(number(row.total));
            cells
        })
        .collect();
    for cell in &xtab.cells {
        grid[cell.row as usize][cell.col as usize + 1] = number(cell.value);
    }
    let mut margin = vec!["total".to_owned()];
    margin.extend(xtab.cols.iter().map(|c| number(c.total)));
    margin.push(number(xtab.total));
    grid.push(margin);
    let headers: Vec<(&str, Align)> = headers.iter().map(|(n, a)| (n.as_str(), *a)).collect();
    out.push('\n');
    out.push_str(&table(&headers, &grid));
    out
}

/// The file's lines, numbered, each with its role and concern, then the findings that touch them.
fn source_text(source: &Source) -> String {
    let language = source.language.as_deref().unwrap_or("no grammar");
    let mut out = format!(
        "{} ({language}): lines {}-{} of {}, {} findings touch them\n\n",
        source.path,
        source.start_line,
        source.end_line,
        source.line_count,
        source.findings.len(),
    );
    let width = source.end_line.to_string().len();
    for line in &source.lines {
        let _ = writeln!(
            out,
            "{:>width$}  {:<9} {:<10} {}",
            line.number, line.role, line.concern, line.text
        )
        .ok();
    }
    if source.more > 0 {
        let _ = writeln!(
            out,
            "... {} more lines; ask for them with --lines {}-{}",
            source.more,
            source.end_line + 1,
            source.end_line + source.more.min(SOURCE_LINES_MAX),
        )
        .ok();
    }
    if !source.findings.is_empty() {
        let rows: Vec<Vec<String>> = source
            .findings
            .iter()
            .map(|f| {
                vec![
                    format!("{}-{}", f.start_line, f.end_line),
                    f.detector.clone(),
                    f.area.clone(),
                    f.reason.clone(),
                ]
            })
            .collect();
        out.push('\n');
        out.push_str(&table(
            &[
                ("lines", Align::Left),
                ("detector", Align::Left),
                ("area", Align::Left),
                ("reason", Align::Left),
            ],
            &rows,
        ));
    }
    out
}

fn tree_text(tree: &Tree) -> String {
    let unit = &tree.size.unit;
    let mut out = format!(
        "{} {} by {}, {} tiles, {} levels deep\n",
        number(tree.total),
        unit,
        tree.size.measure,
        tree.nodes.len(),
        tree.depth,
    );
    out.push_str(&hidden_note(tree.hidden.as_ref()));
    if let Some(scale) = &tree.scale {
        let _ = writeln!(
            out,
            "colour: {} from {} to {} ({:?}, {} steps)",
            scale.measure,
            number(scale.min),
            number(scale.max),
            scale.spacing,
            scale.steps
        )
        .ok();
    }
    let mut children: Vec<Vec<&TreeNode>> = vec![Vec::new(); tree.nodes.len()];
    for node in &tree.nodes {
        if let Some(parent) = node.parent {
            children[parent as usize].push(node);
        }
    }
    for list in &mut children {
        list.sort_by(|a, b| b.total.total_cmp(&a.total).then(a.name.cmp(&b.name)));
    }
    let mut rows = Vec::new();
    if let Some(root) = tree.nodes.first() {
        walk(root, 0, &children, &mut rows, tree.scale.is_some());
    }
    let mut headers = vec![("tile", Align::Left), (unit.as_str(), Align::Right)];
    if tree.scale.is_some() {
        headers.push(("colour", Align::Right));
        headers.push(("step", Align::Right));
    }
    out.push('\n');
    out.push_str(&table(&headers, &rows));
    out
}

fn walk(
    node: &TreeNode,
    depth: usize,
    children: &[Vec<&TreeNode>],
    rows: &mut Vec<Vec<String>>,
    coloured: bool,
) {
    let mut name = format!("{}{}", "  ".repeat(depth), node.name);
    if node.hidden > 0 {
        let _ = write!(name, " (+{} deeper)", node.hidden);
    }
    let mut row = vec![name, number(node.total)];
    if coloured {
        row.push(node.value.map_or_else(|| "none".to_owned(), number));
        row.push(
            node.color_step
                .map_or_else(|| "-".to_owned(), |s| s.to_string()),
        );
    }
    rows.push(row);
    let list = &children[node.id as usize];
    for child in list.iter().take(TREE_CHILDREN_MAX) {
        walk(child, depth + 1, children, rows, coloured);
    }
    if list.len() > TREE_CHILDREN_MAX {
        let mut row = vec![format!(
            "{}... {} more",
            "  ".repeat(depth + 1),
            list.len() - TREE_CHILDREN_MAX
        )];
        row.resize(if coloured { 4 } else { 2 }, String::new());
        rows.push(row);
    }
}

fn areas_text(list: &AreaList) -> String {
    let mut out = format!(
        "{} areas ({} acknowledged), showing {}\n",
        list.matching,
        list.acknowledged,
        list.areas.len()
    );
    if list.areas.is_empty() {
        return out;
    }
    let rows: Vec<Vec<String>> = list.areas.iter().map(area_row).collect();
    let headers = [
        ("#", Align::Right),
        ("lines", Align::Right),
        ("detectors", Align::Left),
        ("area", Align::Left),
    ];
    let rendered = table(&headers, &rows);
    // Each row is followed by the reason it stands out.
    let mut lines = rendered.lines();
    out.push('\n');
    out.push_str(lines.next().unwrap_or_default());
    out.push('\n');
    for (line, area) in lines.zip(&list.areas) {
        let _ = writeln!(out, "{line}");
        let note = area.acknowledged.as_ref().map_or_else(
            || area.reason.clone(),
            |why| format!("acknowledged: {why}. {}", area.reason),
        );
        let _ = writeln!(out, "      {note}");
        if area.subject_kind == SubjectKind::Group {
            let _ = writeln!(out, "      id: {}", area.subject);
        }
    }
    out
}

fn area_row(area: &Area) -> Vec<String> {
    vec![
        area.rank.to_string(),
        number(area.mass_lines.round()),
        area.detectors.join(", "),
        area.title.clone(),
    ]
}

fn finding_text(finding: &Finding) -> String {
    let mut out = format!("[{}] {}\n", finding.detector, finding.reason);
    let measures: Vec<String> = finding
        .measures
        .iter()
        .map(|(name, value)| format!("{name} {}", number(*value)))
        .collect();
    if !measures.is_empty() {
        let _ = writeln!(out, "    measures: {}", measures.join(", "));
    }
    if let Some(members) = finding.labels.get("members") {
        let _ = writeln!(out, "    members: {}", members.replace('\n', ", "));
    }
    for evidence in &finding.evidence {
        let _ = writeln!(
            out,
            "    evidence: {}:{}-{}",
            evidence.path, evidence.start_line, evidence.end_line
        );
    }
    if let Some(why) = &finding.acknowledged {
        let _ = writeln!(out, "    acknowledged: {why}");
    }
    out
}

fn area_text(detail: &AreaDetail) -> String {
    let mut out = String::new();
    if let Some(area) = &detail.area {
        let _ = writeln!(
            out,
            "{}: rank {}, {} lines, {} finding{} from {}",
            area.title,
            area.rank,
            number(area.mass_lines),
            area.finding_count,
            if area.finding_count == 1 { "" } else { "s" },
            area.detectors.join(", "),
        );
        if area.subject_kind == SubjectKind::Group {
            let _ = writeln!(out, "id: {}", area.subject);
        }
        if let Some(why) = &area.acknowledged {
            let _ = writeln!(out, "acknowledged: {why}");
        }
    } else {
        out.push_str("no area of its own; only part of the groups below\n");
    }
    out.push('\n');
    for finding in &detail.findings {
        out.push_str(&finding_text(finding));
    }
    if !detail.groups.is_empty() {
        let rows: Vec<Vec<String>> = detail
            .groups
            .iter()
            .map(|share| {
                vec![
                    share.group.rank.to_string(),
                    format!("{} ({})", share.lines, percent(share.share)),
                    share.group.title.clone(),
                    share.group.subject.clone(),
                ]
            })
            .collect();
        let headers = [
            ("#", Align::Right),
            ("lines here", Align::Right),
            ("group touching this", Align::Left),
            ("id", Align::Left),
        ];
        out.push('\n');
        out.push_str(&table(&headers, &rows));
    }
    if !detail.related.is_empty() {
        let rows: Vec<Vec<String>> = detail.related.iter().map(area_row).collect();
        let headers = [
            ("#", Align::Right),
            ("lines", Align::Right),
            ("detectors", Align::Left),
            ("related area", Align::Left),
        ];
        out.push('\n');
        out.push_str(&table(&headers, &rows));
    }
    out
}

fn catalog_text(catalog: &Catalog) -> String {
    let mut out = format!(
        "{} dimensions, {} measures, {} detectors\n",
        catalog.dimensions.len(),
        catalog.measures.len(),
        catalog.detectors.len()
    );
    let rows: Vec<Vec<String>> = catalog
        .dimensions
        .iter()
        .map(|d| vec![d.name.clone(), d.values.join(", "), d.description.clone()])
        .collect();
    out.push('\n');
    out.push_str(&table(
        &[
            ("dimension", Align::Left),
            ("values", Align::Left),
            ("meaning", Align::Left),
        ],
        &rows,
    ));
    let rows: Vec<Vec<String>> = catalog
        .measures
        .iter()
        .map(|m| vec![m.name.clone(), m.description.clone()])
        .collect();
    out.push('\n');
    out.push_str(&table(
        &[("measure", Align::Left), ("meaning", Align::Left)],
        &rows,
    ));
    let rows: Vec<Vec<String>> = catalog
        .detectors
        .iter()
        .map(|d| vec![d.name.clone(), d.description.clone()])
        .collect();
    out.push('\n');
    out.push_str(&table(
        &[("detector", Align::Left), ("looks for", Align::Left)],
        &rows,
    ));
    out
}
