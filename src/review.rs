// The review — the Rust reader of the sidecar's `report.json`, and the wizard
// proper as one pure function (Phase 17, lane A, slice 2; `DECISIONS.md` →
// "Phase 17 opens: the authoring-stack DRAFT…", fork A2).
//
// Three things live here and nothing else:
//
// 1. `read` — `report.json` (schema v1, `sidecar/CONTRACT.md`) as typed data.
//    It checks `schema` AND `version` (the contract says consumers must),
//    refuses a missing required field by name, and tolerates fields it does
//    not know (v1 grows additively). It is a report CONSUMER, never a sidecar
//    driver: nothing here runs Python, and the report it reads is one the
//    user or an agent already produced.
// 2. `compose` — `(report, answers) → .part text`, the T6 entry's mechanism
//    taken literally. An answer is an accepted face id with an optional
//    edited statement; the output is the accepted statements in the
//    report's traversal order, a grouped proposal emitted ONCE at its
//    primary face's position — exactly the rule `proposed.part` follows —
//    and every emitted statement checked through the ONE parser
//    (`parse::parse_into`), so the wizard cannot hand you a line that then
//    rejects. A wizard's only output is `.cam` text the user could have
//    typed: no comments, no preamble, no state.
// 3. `render` — the terminal's review: proposals first, then the residue,
//    grouped by `checklist_row` then `code`, one line per face, the total
//    accounting visible (an unrecognized face is something you DECLINED).
//    The prose `reason` is routed as-is — never rewritten, never parsed
//    (`DESIGN.md` I7): the structure a consumer groups by is `code` and
//    `refs`, the fields slice 1 added for exactly this.
//
// It sits behind `frontend`/`agent` — the features that already own
// `serde_json` (the theme reader's gate, the MCP loop's gate) — so the
// lean build stays JSON-free. `--review`/`--accept` in `main.rs` and, in
// slice 3, the window's review view are thin drivers of these three; the
// MCP verb is named out ("the flag came first").

use crate::parse;
use serde_json::{Map, Value};

/// The report's own name for itself, checked before anything is read.
pub const SCHEMA: &str = "nacs-recognition-report";
/// The one schema version this reader consumes.
pub const VERSION: u64 = 1;

/// One face of the report, in the report's traversal order.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    pub id: String,
    /// The kernel's surface type — `plane`, `cylinder`, … (new values are additive).
    pub surface: String,
    /// The `CHECKLIST.md` row, or `"no row"`.
    pub checklist_row: String,
    pub status: Status,
}

/// What the sidecar said about the face — exactly two statuses, no third.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Proposed {
        /// One parseable `feature` statement (a Contour block spans several
        /// lines, carried whole).
        part: String,
        why: String,
        /// The group name when several faces own one feature (`F27_Pocket`);
        /// absent on a single-face proposal.
        feature: Option<String>,
    },
    Unsupported {
        reason: String,
        /// The refusing check's name — the key a consumer groups by.
        code: String,
        /// The face ids / feature names the reason names; empty when the
        /// report carried none.
        refs: Vec<String>,
    },
}

/// A `report.json`, read.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub source: String,
    pub units: String,
    pub faces_total: usize,
    pub faces: Vec<Face>,
    /// The setup declarations, present only when the import needed one.
    pub solid: Option<String>,
    pub solids_total: Option<u64>,
    pub up: Option<String>,
    /// The `--origin` words the import was made with, when it had any.
    pub origin: Option<String>,
}

/// One answer of the review: an accepted face, and — optionally — the
/// statement the reviewer edited in place of the report's `part`.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub face: String,
    pub statement: Option<String>,
}

impl Answer {
    /// Accept the report's statement as it stands.
    pub fn accept(face: &str) -> Answer {
        Answer { face: face.to_string(), statement: None }
    }
}

// ---- the reader --------------------------------------------------------------

fn req<'a>(obj: &'a Map<String, Value>, key: &str, ctx: &str) -> Result<&'a Value, String> {
    obj.get(key).ok_or_else(|| format!("{ctx}: missing required field `{key}`"))
}

fn req_str(obj: &Map<String, Value>, key: &str, ctx: &str) -> Result<String, String> {
    req(obj, key, ctx)?
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("{ctx}: field `{key}` is not a string"))
}

fn opt_str(obj: &Map<String, Value>, key: &str, ctx: &str) -> Result<Option<String>, String> {
    match obj.get(key) {
        None => Ok(None),
        Some(v) => v
            .as_str()
            .map(|s| Some(s.to_string()))
            .ok_or_else(|| format!("{ctx}: field `{key}` is not a string")),
    }
}

fn req_u64(obj: &Map<String, Value>, key: &str, ctx: &str) -> Result<u64, String> {
    req(obj, key, ctx)?
        .as_u64()
        .ok_or_else(|| format!("{ctx}: field `{key}` is not a non-negative integer"))
}

fn face_from(v: &Value, index: usize) -> Result<Face, String> {
    let ctx = format!("face entry {}", index + 1);
    let obj = v.as_object().ok_or_else(|| format!("{ctx}: not an object"))?;
    let id = req_str(obj, "id", &ctx)?;
    let ctx = format!("face {id}");
    let surface = req_str(obj, "surface", &ctx)?;
    let checklist_row = req_str(obj, "checklist_row", &ctx)?;
    let status = match req_str(obj, "status", &ctx)?.as_str() {
        "proposed" => Status::Proposed {
            part: req_str(obj, "part", &ctx)?,
            why: req_str(obj, "why", &ctx)?,
            feature: opt_str(obj, "feature", &ctx)?,
        },
        "unsupported" => {
            let refs = match obj.get("refs") {
                None => Vec::new(),
                Some(r) => {
                    let arr = r
                        .as_array()
                        .ok_or_else(|| format!("{ctx}: field `refs` is not a list"))?;
                    if arr.is_empty() {
                        // The contract: present only when there is at least one.
                        return Err(format!("{ctx}: field `refs` is present but empty"));
                    }
                    arr.iter()
                        .map(|x| {
                            x.as_str()
                                .map(str::to_string)
                                .ok_or_else(|| format!("{ctx}: an entry of `refs` is not a string"))
                        })
                        .collect::<Result<Vec<_>, _>>()?
                }
            };
            Status::Unsupported {
                reason: req_str(obj, "reason", &ctx)?,
                code: req_str(obj, "code", &ctx)?,
                refs,
            }
        }
        other => {
            return Err(format!(
                "{ctx}: status `{other}` — the contract has exactly `proposed` and `unsupported`, no third"
            ))
        }
    };
    Ok(Face { id, surface, checklist_row, status })
}

/// A `Report` from the text of a `report.json`. Refuses by name: not JSON,
/// the wrong `schema`, a `version` this reader does not consume, a missing
/// required field, a third status, or a total-accounting break (the entry
/// count disagreeing with `faces_total`, a duplicated id). Extra fields
/// pass through unread — v1 grows additively.
pub fn read(text: &str) -> Result<Report, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    let obj = v.as_object().ok_or("not a report: the document is not a JSON object")?;
    let ctx = "report";
    // Both checked, in this order: a document that is not ours is named
    // before its version is judged.
    let schema = req_str(obj, "schema", ctx)?;
    if schema != SCHEMA {
        return Err(format!("schema is `{schema}`, not `{SCHEMA}` — not a recognition report"));
    }
    let version = req_u64(obj, "version", ctx)?;
    if version != VERSION {
        return Err(format!(
            "report version {version} — this reader consumes version {VERSION} (sidecar/CONTRACT.md)"
        ));
    }
    let source = req_str(obj, "source", ctx)?;
    let units = req_str(obj, "units", ctx)?;
    let faces_total = req_u64(obj, "faces_total", ctx)? as usize;
    let faces_v = req(obj, "faces", ctx)?
        .as_array()
        .ok_or("report: field `faces` is not a list")?;
    let faces = faces_v
        .iter()
        .enumerate()
        .map(|(i, f)| face_from(f, i))
        .collect::<Result<Vec<_>, _>>()?;
    if faces.len() != faces_total {
        return Err(format!(
            "faces_total says {faces_total} but `faces` lists {} — total accounting broken",
            faces.len()
        ));
    }
    for (i, f) in faces.iter().enumerate() {
        if faces[..i].iter().any(|g| g.id == f.id) {
            return Err(format!("face id {} appears twice", f.id));
        }
    }
    let solids_total = match obj.get("solids_total") {
        None => None,
        Some(_) => Some(req_u64(obj, "solids_total", ctx)?),
    };
    Ok(Report {
        source,
        units,
        faces_total,
        faces,
        solid: opt_str(obj, "solid", ctx)?,
        solids_total,
        up: opt_str(obj, "up", ctx)?,
        origin: opt_str(obj, "origin", ctx)?,
    })
}

impl Report {
    pub fn face(&self, id: &str) -> Option<&Face> {
        self.faces.iter().find(|f| f.id == id)
    }

    /// The faces the report proposes something for, in traversal order.
    pub fn proposed(&self) -> impl Iterator<Item = &Face> {
        self.faces.iter().filter(|f| matches!(f.status, Status::Proposed { .. }))
    }

    pub fn proposed_count(&self) -> usize {
        self.proposed().count()
    }

    pub fn unsupported_count(&self) -> usize {
        self.faces.len() - self.proposed_count()
    }

    /// The PRIMARY face of a grouped proposal — where `proposed.part` emits
    /// the block. The report carries no `primary` field: the sidecar names
    /// every group after its primary face (`F27_Pocket` is the floor F27's
    /// pocket, `F8_Hole` the wall F8's hole), so the primary is the member
    /// whose id is the name's `F<n>_` prefix. NOT the first member in
    /// traversal order — a pocket's walls precede its floor. Pinned in
    /// `tests/review_tests.rs` on every shipped golden.
    pub fn primary_of(&self, feature: &str) -> Option<&Face> {
        self.faces.iter().find(|f| {
            matches!(&f.status, Status::Proposed { feature: Some(g), .. } if g == feature)
                && feature.starts_with(&format!("{}_", f.id))
        })
    }
}

// ---- the one parser ----------------------------------------------------------

/// Whether `statement` is exactly one `feature` statement — the one home
/// moved core-side (Phase 19 slice 1: the wizard is its third consumer
/// and must exist in the lean build); re-exported here so the review's
/// callers and pins stand unchanged.
pub use crate::parse::check_feature_statement as check_statement;

// ---- the wizard --------------------------------------------------------------

/// The wizard: the accepted statements as `.part` text, in the report's
/// traversal order, a grouped proposal ONCE at its primary face's position
/// (the `proposed.part` rule). Statements are separated by one blank line
/// and the text ends with a newline — `proposed.part`'s own shape, so the
/// two agree byte for byte once that file's `#` lines are set aside.
///
/// Refuses by name: an id the report does not list, a face the report did
/// not propose (you cannot accept a residue entry — that is what "declined"
/// means), a face answered twice, two members of one group edited to
/// different texts, and any statement — the report's or an edit — that the
/// one parser rejects, with the parser's sentence.
pub fn compose(report: &Report, answers: &[Answer]) -> Result<String, String> {
    // Every answer resolved to a face first, so a bad id is named before
    // anything is composed.
    let mut edits: Vec<(&str, Option<&str>)> = Vec::new();
    for a in answers {
        let face = report
            .face(&a.face)
            .ok_or_else(|| format!("face {} is not in the report ({} faces, F1..)", a.face, report.faces_total))?;
        let Status::Proposed { .. } = &face.status else {
            return Err(format!(
                "face {} is unsupported, not proposed — the report declined it (code {}); there is nothing to accept",
                face.id,
                match &face.status {
                    Status::Unsupported { code, .. } => code.as_str(),
                    Status::Proposed { .. } => unreachable!(),
                }
            ));
        };
        if edits.iter().any(|(id, _)| *id == face.id) {
            return Err(format!("face {} is answered twice", face.id));
        }
        edits.push((&face.id, a.statement.as_deref()));
    }
    let accepted = |id: &str| edits.iter().any(|(i, _)| *i == id);
    let edit_of = |id: &str| edits.iter().find(|(i, _)| *i == id).and_then(|(_, s)| *s);

    let mut out: Vec<String> = Vec::new();
    for face in &report.faces {
        let Status::Proposed { part, feature, .. } = &face.status else { continue };
        let statement = match feature {
            None => {
                if !accepted(&face.id) {
                    continue;
                }
                edit_of(&face.id).unwrap_or(part.as_str()).to_string()
            }
            Some(group) => {
                // The block belongs to the group: it emits at the primary's
                // position when ANY member is accepted, and an edit on any
                // member replaces it — two members edited to different texts
                // is a contradiction, refused rather than resolved.
                let primary = report.primary_of(group).ok_or_else(|| {
                    format!("grouped proposal {group} names no member face as its primary (F<n>_ prefix)")
                })?;
                if primary.id != face.id {
                    continue;
                }
                let members: Vec<&Face> = report
                    .faces
                    .iter()
                    .filter(|f| matches!(&f.status, Status::Proposed { feature: Some(g), .. } if g == group))
                    .collect();
                if !members.iter().any(|m| accepted(&m.id)) {
                    continue;
                }
                let mut chosen: Option<(&str, &str)> = None;
                for m in &members {
                    if let Some(e) = edit_of(&m.id) {
                        match chosen {
                            Some((who, text)) if text != e => {
                                return Err(format!(
                                    "faces {who} and {} both edit the {group} block, to different texts — one edit per block",
                                    m.id
                                ))
                            }
                            Some(_) => {}
                            None => chosen = Some((&m.id, e)),
                        }
                    }
                }
                chosen.map(|(_, t)| t).unwrap_or(part.as_str()).to_string()
            }
        };
        check_statement(&statement).map_err(|e| format!("face {}: {e}", face.id))?;
        out.push(statement);
    }
    if out.is_empty() {
        return Ok(String::new());
    }
    Ok(out.join("\n\n") + "\n")
}

// ---- the terminal's review ---------------------------------------------------

/// A `checklist_row` as a sort key: numeric rows in order, `"no row"` last.
fn row_key(row: &str) -> (u32, u32, String) {
    let mut it = row.splitn(2, '.');
    let a = it.next().and_then(|s| s.parse::<u32>().ok());
    let b = it.next().and_then(|s| s.parse::<u32>().ok());
    match (a, b) {
        (Some(a), Some(b)) => (a, b, String::new()),
        (Some(a), None) => (a, 0, String::new()),
        _ => (u32::MAX, u32::MAX, row.to_string()),
    }
}

fn sorted_unique<'a>(items: impl Iterator<Item = &'a str>, key: impl Fn(&str) -> (u32, u32, String)) -> Vec<&'a str> {
    let mut v: Vec<&str> = Vec::new();
    for i in items {
        if !v.contains(&i) {
            v.push(i);
        }
    }
    v.sort_by_key(|s| key(s));
    v
}

/// A statement on one line: its opener, with a block's remaining lines counted
/// rather than printed.
fn statement_head(part: &str) -> String {
    let mut lines = part.lines();
    let head = lines.next().unwrap_or("").to_string();
    let rest = lines.count();
    if rest == 0 {
        head
    } else {
        format!("{head} (+{rest} lines)")
    }
}

/// The review as text: the header's total accounting (the sidecar's own
/// `N faces: P proposed, U unsupported`), the setup the coordinates assume,
/// then every proposal grouped by row and every residue entry grouped by
/// row then code — one line per face, each face exactly once.
pub fn render(report: &Report) -> String {
    render_marked(report, &[])
}

/// The same page with the project's answers beside each proposal (ROADMAP
/// item 1, slice 3): `· in project` or `· rejected`, what the window's rows
/// say — printed when `--review` finds main.part beside the report.
pub fn render_marked(report: &Report, answers: &[crate::view::ReviewRow]) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "{}: {} faces: {} proposed, {} unsupported (report v{VERSION}, {})\n",
        report.source,
        report.faces_total,
        report.proposed_count(),
        report.unsupported_count(),
        report.units
    ));
    for line in setup_lines(report) {
        s.push_str(&line);
        s.push('\n');
    }
    let id_w = report.faces.iter().map(|f| f.id.len()).max().unwrap_or(2);
    let surf_w = report.faces.iter().map(|f| f.surface.len()).max().unwrap_or(5);

    s.push_str(&format!("proposed ({}):\n", report.proposed_count()));
    let rows = sorted_unique(report.proposed().map(|f| f.checklist_row.as_str()), row_key);
    for row in rows {
        s.push_str(&format!("  row {row}:\n"));
        for f in report.proposed().filter(|f| f.checklist_row == row) {
            let Status::Proposed { part, feature, .. } = &f.status else { unreachable!() };
            s.push_str(&format!(
                "    {:<id_w$}  {:<surf_w$}  {}",
                f.id,
                f.surface,
                statement_head(part)
            ));
            if let Some(g) = feature {
                s.push_str(&format!("  [{g}]"));
            }
            if let Some(r) = answers.iter().find(|r| r.id == f.id) {
                if r.in_project {
                    s.push_str("  · in project");
                } else if r.rejected {
                    s.push_str("  · rejected");
                }
            }
            s.push('\n');
        }
    }

    s.push_str(&format!("unsupported ({}):\n", report.unsupported_count()));
    let residue = || report.faces.iter().filter(|f| matches!(f.status, Status::Unsupported { .. }));
    let rows = sorted_unique(residue().map(|f| f.checklist_row.as_str()), row_key);
    for row in rows {
        s.push_str(&format!("  row {row}:\n"));
        let in_row = || residue().filter(move |f| f.checklist_row == row);
        let codes = sorted_unique(
            in_row().map(|f| match &f.status {
                Status::Unsupported { code, .. } => code.as_str(),
                Status::Proposed { .. } => unreachable!(),
            }),
            |c| (0, 0, c.to_string()),
        );
        for code in codes {
            s.push_str(&format!("    code {code}:\n"));
            for f in in_row() {
                let Status::Unsupported { reason, code: c, refs } = &f.status else { unreachable!() };
                if c != code {
                    continue;
                }
                s.push_str(&format!("      {:<id_w$}  {:<surf_w$}  {}", f.id, f.surface, reason));
                if !refs.is_empty() {
                    s.push_str(&format!("  refs {}", refs.join(" ")));
                }
                s.push('\n');
            }
        }
    }
    s
}

/// The setup the report declares, as the page prints it and the Review
/// shows it (ROADMAP item 1, slice 4): the solid chosen and the up axis,
/// each a line, none when the import needed neither.
pub fn setup_lines(report: &Report) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(solid) = &report.solid {
        out.push(match report.solids_total {
            Some(n) => format!("setup: solid {solid} of {n} in the file"),
            None => format!("setup: solid {solid}"),
        });
    }
    if let Some(up) = &report.up {
        out.push(format!("setup: imported with --up {up} (model {up} is machine +Z) — coordinates are post-rotation"));
    }
    out
}

/// `--accept`'s value — `F1,F6,…` — as answers. Empty entries are dropped;
/// an empty list is the caller's usage error, not this function's.
pub fn answers_from_list(list: &str) -> Vec<Answer> {
    list.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(Answer::accept)
        .collect()
}

// ---- the review's rows (DESIGN.md §5.2) -------------------------------------

/// The pane's rows from a report and the program it would be accepted
/// into: proposals first in the report's traversal order, then the residue
/// grouped by checklist row and by the refusing check's `code` (a stable
/// sort — within a group the report's order stands). Every face appears
/// exactly once: the contract's total accounting, kept visible. A
/// proposal's `in_project` is derived from the program's feature names —
/// accepted by this window or by any editor, it reads the same.
pub fn rows(report: &Report, part: &parse::Part, rejected: &[String]) -> Vec<crate::view::ReviewRow> {
    use crate::view::{feature_name_of, ReviewRow};
    let mut out: Vec<ReviewRow> = Vec::new();
    for (i, f) in report.faces.iter().enumerate() {
        if let Status::Proposed { part: statement, why, feature } = &f.status {
            // A rejection on any member rejects the group, as an accept on
            // any member accepts it once.
            let in_group = |o: &Face| match (&o.status, feature) {
                (Status::Proposed { feature: Some(h), .. }, Some(g)) => h == g,
                _ => o.id == f.id,
            };
            let is_rejected = report.faces.iter().filter(|o| in_group(o)).any(|o| rejected.iter().any(|r| *r == o.id));
            let primary = feature.as_deref().and_then(|g| report.primary_of(g)).map(|p| p.id == f.id);
            let member_of = match (feature, primary) {
                (Some(g), Some(false)) => Some(g.clone()),
                _ => None,
            };
            let name = feature_name_of(statement).unwrap_or("");
            let head = match &member_of {
                Some(g) => format!("{}  {}  — member of {g}", f.id, f.surface),
                None => format!("{}  {}", f.id, f.surface),
            };
            out.push(ReviewRow {
                face: i,
                id: f.id.clone(),
                section: "Proposed".to_string(),
                head,
                text: format!("{statement}\n{why}"),
                statement: Some(statement.clone()),
                refs: Vec::new(),
                in_project: !name.is_empty() && part.features.contains_key(name),
                rejected: is_rejected,
                member_of,
            });
        }
    }
    let mut residue: Vec<(usize, &Face)> = report
        .faces
        .iter()
        .enumerate()
        .filter(|(_, f)| matches!(f.status, Status::Unsupported { .. }))
        .collect();
    residue.sort_by(|(_, a), (_, b)| {
        let ka = (row_key(&a.checklist_row), code_of(a));
        let kb = (row_key(&b.checklist_row), code_of(b));
        ka.cmp(&kb)
    });
    for (i, f) in residue {
        let Status::Unsupported { reason, code, refs } = &f.status else { continue };
        out.push(ReviewRow {
            face: i,
            id: f.id.clone(),
            section: format!("Residue · row {} · {code}", f.checklist_row),
            head: format!("{}  {}", f.id, f.surface),
            text: reason.clone(),
            statement: None,
            refs: refs.clone(),
            in_project: false,
            rejected: false,
            member_of: None,
        });
    }
    out
}

fn code_of(f: &Face) -> &str {
    match &f.status {
        Status::Unsupported { code, .. } => code,
        Status::Proposed { .. } => "",
    }
}

// ---- the rejection, a line of the project (ROADMAP item 1, slice 3) ----------

/// The review's one mark in the project text: a comment line, `# rejected
/// F7`, that the parser ignores and this reader reads — so a rejection is a
/// fact of the folder like an acceptance is, shown the same by the window,
/// by `--review`, and by anyone's editor.
pub fn rejection_line(id: &str) -> String {
    format!("# rejected {id}")
}

/// Every face id the text's `# rejected …` lines name, once each, in order.
/// Only ids (`F` and digits) count, so a comment that happens to start with
/// the word is prose.
pub fn rejected_ids(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix('#') else { continue };
        let Some(ids) = rest.trim_start().strip_prefix("rejected") else { continue };
        if !ids.starts_with(char::is_whitespace) {
            continue;
        }
        for id in ids.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()) {
            let is_id = id.len() > 1 && id.starts_with('F') && id[1..].chars().all(|c| c.is_ascii_digit());
            if is_id && !out.iter().any(|o| o == id) {
                out.push(id.to_string());
            }
        }
    }
    out
}

/// The text with `id` rejected: its line straight under an earlier
/// rejection when one ends the text, after one blank line otherwise — the
/// shape an accepted statement lands in.
pub fn with_rejection(text: &str, id: &str) -> String {
    let trimmed = text.trim_end_matches(['\n', '\r']);
    if trimmed.lines().last().is_some_and(|l| !rejected_ids(l).is_empty()) {
        format!("{trimmed}\n{}\n", rejection_line(id))
    } else {
        crate::view::append_statement(text, &rejection_line(id))
    }
}

/// The text with the rejection of `ids` taken back: every line naming only
/// ids among them leaves. A line that names others beside them is the
/// hand's to edit, refused by name; so is a text that rejects none of them.
pub fn without_rejection(text: &str, ids: &[&str]) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut removed = 0usize;
    for l in text.split_inclusive('\n') {
        let on_line = rejected_ids(l);
        if on_line.is_empty() || !on_line.iter().any(|i| ids.contains(&i.as_str())) {
            out.push_str(l);
        } else if on_line.iter().all(|i| ids.contains(&i.as_str())) {
            removed += 1;
        } else {
            return Err(format!("the line `{}` rejects other faces too; edit it by hand", l.trim()));
        }
    }
    if removed == 0 {
        return Err(format!("no `# rejected` line names {}", ids.join(", ")));
    }
    Ok(out)
}
