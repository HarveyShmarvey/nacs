// The review's pins (Phase 17, lane A, slice 2 — DECISIONS.md "Phase 17
// opens: the authoring-stack DRAFT…", fork A2). Whole-file gated the way
// `review.rs` itself is: the lean build has no JSON reader, so it has no
// review to pin — its one pin (the flags refuse by name) lives in
// tests/check_tests.rs beside `--mcp`'s.
#![cfg(any(feature = "frontend", feature = "agent"))]

use nacs::parse;
use nacs::review::{self, Answer, Status};
use std::fs;
use std::process::Command;

/// The nine shipped report/proposal pairs — the same table
/// tests/sidecar_tests.rs runs the sidecar against; here read from the
/// committed goldens, so nothing needs the venv.
const FIXTURES: &[(&str, &str)] = &[
    ("tests/fixtures/block_bore.report.golden.json", "tests/fixtures/block_bore.proposed.golden.part"),
    ("tests/fixtures/quadric_boss.report.golden.json", "tests/fixtures/quadric_boss.proposed.golden.part"),
    ("tests/fixtures/pocket_plate.report.golden.json", "tests/fixtures/pocket_plate.proposed.golden.part"),
    ("tests/fixtures/drilled_plate.report.golden.json", "tests/fixtures/drilled_plate.proposed.golden.part"),
    ("tests/fixtures/assembly_pair.report.golden.json", "tests/fixtures/assembly_pair.proposed.golden.part"),
    ("tests/fixtures/tilted_plate.report.golden.json", "tests/fixtures/tilted_plate.proposed.golden.part"),
    ("tests/fixtures/outer_profile.report.golden.json", "tests/fixtures/outer_profile.proposed.golden.part"),
    ("tests/fixtures/trim_wire.report.golden.json", "tests/fixtures/trim_wire.proposed.golden.part"),
    ("tests/fixtures/lip_bore.report.golden.json", "tests/fixtures/lip_bore.proposed.golden.part"),
    ("tests/fixtures/uneven_bevels.report.golden.json", "tests/fixtures/uneven_bevels.proposed.golden.part"),
    ("tests/fixtures/rib_head.report.golden.json", "tests/fixtures/rib_head.proposed.golden.part"),
    ("tests/fixtures/mushroom.report.golden.json", "tests/fixtures/mushroom.proposed.golden.part"),
    ("tests/fixtures/rib_disc.report.golden.json", "tests/fixtures/rib_disc.proposed.golden.part"),
    ("tests/fixtures/rib_disc_back.report.golden.json", "tests/fixtures/rib_disc_back.proposed.golden.part"),
    ("tests/fixtures/step_hole_back.report.golden.json", "tests/fixtures/step_hole_back.proposed.golden.part"),
    ("tests/fixtures/step_hole.report.golden.json", "tests/fixtures/step_hole.proposed.golden.part"),
    ("tests/fixtures/split_hole.report.golden.json", "tests/fixtures/split_hole.proposed.golden.part"),
    ("tests/fixtures/split_hole_back.report.golden.json", "tests/fixtures/split_hole_back.proposed.golden.part"),
    ("tests/fixtures/open_pocket.report.golden.json", "tests/fixtures/open_pocket.proposed.golden.part"),
];

fn read_golden(path: &str) -> review::Report {
    let text = fs::read_to_string(path).expect("read report golden");
    review::read(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn accept_all(report: &review::Report) -> Vec<Answer> {
    report.proposed().map(|f| Answer::accept(&f.id)).collect()
}

/// `proposed.part` minus what the report does not carry: the sidecar's
/// `#` lines (the file header, the per-proposal provenance, the "consume
/// with" suggestions) and the blank line that follows the header. What
/// remains is the statement text — and THAT is what a wizard emits, by the
/// law of the phase (`.cam` text the user could have typed, never comments).
fn statements_of(proposed_golden: &str) -> String {
    let kept: Vec<&str> = proposed_golden
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect();
    let mut text = kept.join("\n");
    text.push('\n');
    text.trim_start_matches('\n').to_string()
}

/// Pin (a): accept-all on every shipped fixture reproduces its
/// `proposed.part` byte for byte — the statement bytes, in the report's
/// traversal order, each grouped block once at its primary face, one
/// blank line between statements, a trailing newline and no trailing
/// blank. What the pin found: the sidecar's file also carries `#` lines the
/// report never had (provenance, "consume with"), so "byte for byte" is
/// exactly the statement bytes, stated in CONTRACT.md's emission rule now.
#[test]
fn test_review_accept_all_reproduces_proposed_part() {
    for (report_path, proposed_path) in FIXTURES {
        let report = read_golden(report_path);
        let golden = fs::read_to_string(proposed_path).expect("read proposed golden");
        let composed = review::compose(&report, &accept_all(&report)).unwrap_or_else(|e| panic!("{report_path}: {e}"));
        assert_eq!(composed, statements_of(&golden), "{proposed_path}: accept-all diverged from the golden's statements");
    }
}

/// Pin (b): every accepted statement parses through the one parser — and
/// the composed text, as a whole, parses to exactly one feature per
/// distinct proposal (the grouped-once rule, counted). The Contour block's
/// `part` (a multi-line statement carried as one JSON string) round-trips
/// through serde_json's escape and back into the block form the parser
/// reads: trim_wire and pocket_plate are in the table, so this pin covers it.
#[test]
fn test_review_every_accepted_statement_parses() {
    for (report_path, _) in FIXTURES {
        let report = read_golden(report_path);
        let mut distinct: Vec<String> = Vec::new();
        for f in report.proposed() {
            let Status::Proposed { part, feature, .. } = &f.status else { unreachable!() };
            review::check_statement(part).unwrap_or_else(|e| panic!("{report_path} {}: {e}", f.id));
            let name = feature.clone().unwrap_or_else(|| f.id.clone());
            if !distinct.contains(&name) {
                distinct.push(name);
            }
        }
        let composed = review::compose(&report, &accept_all(&report)).unwrap();
        let mut p = parse::Part::default();
        parse::parse_into(&composed, &mut p, None).unwrap_or_else(|e| panic!("{report_path}: composed text rejected: {e}"));
        assert_eq!(p.features.len(), distinct.len(), "{report_path}: one feature per distinct proposal, grouped blocks once");
    }
}

/// The primary-face rule the contract states in words ("the pocket's
/// floor; the hole/bore's wall") and the reader recovers from the name:
/// every grouped feature on every golden names exactly one member by its
/// `F<n>_` prefix — and on pocket_plate that member is NOT the first in
/// traversal order (walls F7–F10 precede floor F27), which is why "first
/// member" was never the rule.
#[test]
fn test_review_primary_is_the_name_prefix_not_the_first_member() {
    for (report_path, _) in FIXTURES {
        let report = read_golden(report_path);
        let mut groups: Vec<String> = Vec::new();
        for f in report.proposed() {
            if let Status::Proposed { feature: Some(g), .. } = &f.status {
                if !groups.contains(g) {
                    groups.push(g.clone());
                }
            }
        }
        for g in &groups {
            let primaries: Vec<&review::Face> = report
                .faces
                .iter()
                .filter(|f| matches!(&f.status, Status::Proposed { feature: Some(n), .. } if n == g))
                .filter(|f| g.starts_with(&format!("{}_", f.id)))
                .collect();
            assert_eq!(primaries.len(), 1, "{report_path}: group {g} must name exactly one member as primary");
            assert_eq!(report.primary_of(g).map(|f| f.id.as_str()), Some(primaries[0].id.as_str()));
        }
    }
    let pocket = read_golden("tests/fixtures/pocket_plate.report.golden.json");
    let first_member = pocket
        .faces
        .iter()
        .find(|f| matches!(&f.status, Status::Proposed { feature: Some(n), .. } if n == "F27_Pocket"))
        .unwrap();
    assert_eq!(first_member.id, "F7", "the first member in traversal order is a wall");
    assert_eq!(pocket.primary_of("F27_Pocket").unwrap().id, "F27", "the primary is the floor");
}

/// A grouped block emits once at its primary's position even when only a
/// non-primary member is accepted, or when several members are.
#[test]
fn test_review_grouped_block_emits_once_at_the_primary() {
    let report = read_golden("tests/fixtures/pocket_plate.report.golden.json");
    let one_wall = review::compose(&report, &[Answer::accept("F9")]).unwrap();
    let all_walls = review::compose(&report, &accept_all(&report)
        .into_iter()
        .filter(|a| ["F7", "F8", "F9", "F10", "F27"].contains(&a.face.as_str()))
        .collect::<Vec<_>>())
    .unwrap();
    assert_eq!(one_wall, all_walls);
    assert_eq!(one_wall.matches("feature F27_Pocket").count(), 1);
    // Accepting the top plane (F3, before the floor F27 in traversal order)
    // beside a wall puts the pocket AFTER the plane — the floor's position,
    // not the wall's (F9 precedes F3? no — F3 precedes F7; the order is by
    // primary either way, which is what this asserts).
    let mixed = review::compose(&report, &[Answer::accept("F9"), Answer::accept("F3")]).unwrap();
    let plane_at = mixed.find("feature F3_Top").unwrap();
    let pocket_at = mixed.find("feature F27_Pocket").unwrap();
    assert!(plane_at < pocket_at, "F3 (traversal 3) before the F27 block (traversal 27)");
}

/// Pin (c): an unsupported face refuses by name; an unknown id refuses by
/// name; a face answered twice refuses.
#[test]
fn test_review_accept_refuses_unsupported_and_unknown() {
    let report = read_golden("tests/fixtures/trim_wire.report.golden.json");
    let e = review::compose(&report, &[Answer::accept("F1")]).unwrap_err();
    assert!(e.contains("F1 is unsupported, not proposed"), "{e}");
    assert!(e.contains("trim_wire_anchor_bottom"), "the refusal names the code: {e}");
    let e = review::compose(&report, &[Answer::accept("F99")]).unwrap_err();
    assert!(e.contains("F99 is not in the report"), "{e}");
    let e = review::compose(&report, &[Answer::accept("F2"), Answer::accept("F2")]).unwrap_err();
    assert!(e.contains("F2 is answered twice"), "{e}");
    // Nothing accepted composes nothing — not an error, an empty page.
    assert_eq!(review::compose(&report, &[]).unwrap(), "");
}

/// Pin (d): the review prints every face exactly once (count ==
/// faces_total), every residue line under its own `code` header and its
/// own row header, every proposal line under its row — and the header
/// line carries the total accounting in the sidecar's own words.
#[test]
fn test_review_render_groups_by_row_and_code_every_face_once() {
    for (report_path, _) in FIXTURES {
        let report = read_golden(report_path);
        let text = review::render(&report);
        let header = text.lines().next().unwrap();
        assert!(
            header.contains(&format!(
                "{} faces: {} proposed, {} unsupported",
                report.faces_total,
                report.proposed_count(),
                report.unsupported_count()
            )),
            "{report_path}: {header}"
        );
        let mut seen = 0usize;
        let mut section = "";
        let mut row = String::new();
        let mut code = String::new();
        for line in text.lines().skip(1) {
            let t = line.trim_start();
            if line.starts_with("proposed (") || line.starts_with("unsupported (") {
                section = if line.starts_with("proposed") { "proposed" } else { "unsupported" };
                continue;
            }
            if line.starts_with("setup:") {
                continue;
            }
            if let Some(r) = t.strip_prefix("row ").and_then(|r| r.strip_suffix(':')) {
                row = r.to_string();
                code.clear();
                continue;
            }
            if let Some(c) = t.strip_prefix("code ").and_then(|c| c.strip_suffix(':')) {
                code = c.to_string();
                continue;
            }
            let id = t.split_whitespace().next().unwrap();
            let face = report.face(id).unwrap_or_else(|| panic!("{report_path}: line names no face: {line}"));
            seen += 1;
            assert_eq!(face.checklist_row, row, "{report_path}: {id} under the wrong row");
            match &face.status {
                Status::Proposed { .. } => assert_eq!(section, "proposed"),
                Status::Unsupported { code: c, reason, refs } => {
                    assert_eq!(section, "unsupported");
                    assert_eq!(c, &code, "{report_path}: {id} under the wrong code");
                    assert!(line.contains(reason.as_str()), "the reason is routed as-is");
                    for r in refs {
                        assert!(line.contains(r.as_str()), "{report_path}: {id} lists ref {r}");
                    }
                }
            }
        }
        assert_eq!(seen, report.faces_total, "{report_path}: every face exactly once");
        // Exactly once: no id line appears twice.
        for f in &report.faces {
            let n = text.lines().filter(|l| l.trim_start().starts_with(&format!("{} ", f.id))).count();
            assert_eq!(n, 1, "{report_path}: {} printed {n} times", f.id);
        }
    }
}

/// Pin (e): the reader checks both `schema` and `version`, and a missing
/// required field refuses by name; an unknown extra field passes (v1 grows
/// additively).
#[test]
fn test_review_reader_refuses_by_name() {
    let good = fs::read_to_string("tests/fixtures/block_bore.report.golden.json").unwrap();
    let e = review::read(&good.replace("\"version\": 1", "\"version\": 2")).unwrap_err();
    assert!(e.contains("report version 2") && e.contains("version 1"), "{e}");
    let e = review::read(&good.replace("nacs-recognition-report", "someone-elses-report")).unwrap_err();
    assert!(e.contains("schema is `someone-elses-report`"), "{e}");
    let e = review::read(&good.replace("\"faces_total\": 10,\n", "")).unwrap_err();
    assert!(e.contains("missing required field `faces_total`"), "{e}");
    let e = review::read(&good.replacen("\"why\":", "\"whyy\":", 1)).unwrap_err();
    assert!(e.contains("missing required field `why`"), "{e}");
    let e = review::read(&good.replacen("\"code\":", "\"codee\":", 1)).unwrap_err();
    assert!(e.contains("missing required field `code`"), "{e}");
    let e = review::read(&good.replace("\"faces_total\": 10", "\"faces_total\": 11")).unwrap_err();
    assert!(e.contains("faces_total says 11") && e.contains("lists 10"), "{e}");
    let e = review::read("not json at all").unwrap_err();
    assert!(e.starts_with("not JSON"), "{e}");
    // Additive: a field nobody knows yet is not an error.
    let extra = good.replacen("\"units\": \"mm\"", "\"units\": \"mm\",\n  \"future\": {\"x\": 1}", 1);
    assert_eq!(review::read(&extra).unwrap(), review::read(&good).unwrap());
}

/// Pin (f): an edited statement that parses replaces the report's; one
/// that does not refuses with the parser's own sentence — the exact text
/// `parse::parse_into` gives, not a paraphrase; a non-feature statement
/// refuses by name; an edit on a group member replaces the block, and two
/// members edited to different texts refuse.
#[test]
fn test_review_edited_statement_replaces_or_refuses_with_the_parsers_sentence() {
    let report = read_golden("tests/fixtures/block_bore.report.golden.json");
    let edited = Answer { face: "F10".into(), statement: Some("feature F10_Top Plane(z=19.5)".into()) };
    let out = review::compose(&report, &[edited]).unwrap();
    assert_eq!(out, "feature F10_Top Plane(z=19.5)\n");

    let bad = "feature F10_Top Plane(z=nineteen)";
    let parser_says = parse::parse_into(bad, &mut parse::Part::default(), None).unwrap_err();
    let e = review::compose(&report, &[Answer { face: "F10".into(), statement: Some(bad.into()) }]).unwrap_err();
    assert!(e.contains(&parser_says), "compose said `{e}`, the parser said `{parser_says}`");

    let e = review::compose(&report, &[Answer { face: "F10".into(), statement: Some("run End(tool=T1)".into()) }]).unwrap_err();
    assert!(e.contains("not a feature statement"), "{e}");

    let pocket = read_golden("tests/fixtures/pocket_plate.report.golden.json");
    let block = "feature F27_Pocket ProfilePocket(z=15.0, extent=6.5\n    10.0 10.0\n    40.0 10.0\n    40.0 30.0\n    10.0 30.0\n)";
    let out = review::compose(&pocket, &[Answer { face: "F8".into(), statement: Some(block.into()) }, Answer::accept("F27")]).unwrap();
    assert_eq!(out, format!("{block}\n"));
    let e = review::compose(
        &pocket,
        &[
            Answer { face: "F8".into(), statement: Some(block.into()) },
            Answer { face: "F27".into(), statement: Some(block.replace("6.5", "7.0")) },
        ],
    )
    .unwrap_err();
    assert!(e.contains("both edit the F27_Pocket block"), "{e}");
    // A block that never closes: the parser has its own sentence for that
    // too (the build found this — "end of file reached while reading
    // contour profile points" — so the reader's own "parses as no feature"
    // fallback is exactly that, a fallback the one parser reaches first).
    let open = "feature X ProfilePocket(z=1, extent=1\n 0 0\n 1 1";
    let parser_says = parse::parse_into(open, &mut parse::Part::default(), None).unwrap_err();
    let e = review::compose(&pocket, &[Answer { face: "F27".into(), statement: Some(open.into()) }]).unwrap_err();
    assert!(e.contains(&parser_says), "compose said `{e}`, the parser said `{parser_says}`");
}

/// The terminal voice: `--review` prints `render`, `--accept` prints
/// `compose`, a refusal is exit 1 with nothing on stdout, and the two are
/// one artifact each against every other mode.
#[test]
fn test_review_flags_are_the_functions_voices() {
    let report_path = "tests/fixtures/drilled_plate.report.golden.json";
    let report = read_golden(report_path);
    let out = Command::new(env!("CARGO_BIN_EXE_nacs")).args(["--review", report_path]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), review::render(&report));

    let ids: Vec<String> = report.proposed().map(|f| f.id.clone()).collect();
    let out = Command::new(env!("CARGO_BIN_EXE_nacs")).args(["--accept", &ids.join(","), report_path]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), review::compose(&report, &accept_all(&report)).unwrap());

    let out = Command::new(env!("CARGO_BIN_EXE_nacs")).args(["--accept", "F1,F999", report_path]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty(), "a refusal composes nothing");
    assert!(String::from_utf8_lossy(&out.stderr).contains("F999 is not in the report"));

    let out = Command::new(env!("CARGO_BIN_EXE_nacs")).args(["--review", "--check", "plate.cam"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("one artifact each"));

    let out = Command::new(env!("CARGO_BIN_EXE_nacs")).args(["--review", "no/such/report.json"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("never runs the sidecar"), "a missing report says where one comes from: {err}");
}

// ---- Phase 17 lane A slice 3: the review's rows (DESIGN.md §5.2) ----

/// Every face exactly once, proposals first in the report's order, then
/// the residue by row (numeric, "no row" last) and code; `in_project` is
/// read off the program, so an accepted statement reads as accepted after
/// the reload it causes — by this window or any editor.
#[test]
fn test_review_rows_account_for_every_face_and_read_in_project_off_the_program() {
    let text = fs::read_to_string("tests/fixtures/block_bore.report.golden.json").unwrap();
    let report = review::read(&text).unwrap();
    let empty = parse::Part::default();
    let rows = review::rows(&report, &empty, &[]);
    assert_eq!(rows.len(), report.faces_total, "total accounting: every face once");
    let mut seen: Vec<usize> = rows.iter().map(|r| r.face).collect();
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), report.faces_total);
    let n_prop = report.proposed_count();
    assert!(rows[..n_prop].iter().all(|r| r.section == "Proposed" && r.statement.is_some()));
    assert!(rows[n_prop..].iter().all(|r| r.section.starts_with("Residue · row ") && r.statement.is_none()));
    let prop_faces: Vec<usize> = rows[..n_prop].iter().map(|r| r.face).collect();
    let mut sorted = prop_faces.clone();
    sorted.sort();
    assert_eq!(prop_faces, sorted, "proposals keep the report's traversal order");
    let sections: Vec<&str> = rows[n_prop..].iter().map(|r| r.section.as_str()).collect();
    let mut s2 = sections.clone();
    s2.dedup();
    assert!(s2.len() <= sections.len(), "residue is grouped: equal sections are adjacent");
    assert!(rows.iter().all(|r| !r.in_project), "an empty program declares nothing");
    // Accept one through the text path and re-read the rows against it.
    let first = &rows[0];
    let statement = first.statement.clone().unwrap();
    let src = nacs::view::append_statement("stock Box(w=120, l=80, h=20, x=0, y=0, z=0)\n", &statement);
    let part = parse::parse_project(&src, None).unwrap();
    let rows2 = review::rows(&report, &part, &[]);
    assert!(rows2[0].in_project, "the accepted proposal reads as in project: {statement}");
    assert_eq!(rows2.iter().filter(|r| r.in_project).count(), rows2.iter().filter(|r| r.statement.as_deref() == Some(statement.as_str())).count(), "every member of the same statement reads the same");
}

/// A grouped proposal's members name their group; its primary does not.
#[test]
fn test_review_rows_mark_group_members_and_leave_the_primary_bare() {
    let text = fs::read_to_string("tests/fixtures/pocket_plate.report.golden.json").unwrap();
    let report = review::read(&text).unwrap();
    let rows = review::rows(&report, &parse::Part::default(), &[]);
    let members: Vec<&nacs::view::ReviewRow> = rows.iter().filter(|r| r.member_of.is_some()).collect();
    assert!(!members.is_empty(), "pocket_plate has grouped proposals");
    for m in &members {
        let g = m.member_of.as_deref().unwrap();
        let primary = report.primary_of(g).unwrap();
        assert_ne!(primary.id, m.id);
        let prow = rows.iter().find(|r| r.id == primary.id).unwrap();
        assert!(prow.member_of.is_none(), "the primary {} is bare", primary.id);
        assert!(m.head.contains(g), "{}", m.head);
    }
}

/// A rejection is a `# rejected F7` line of main.part (ROADMAP item 1,
/// slice 3): written under the last one or after a blank line, read back as
/// ids only, a rejection on one member rejecting its group, printed beside
/// the row by `render_marked`, and taken back line by line — a line that
/// names other faces too is the hand's.
#[test]
fn test_review_rejection_is_a_line_of_the_project_read_back_off_the_text() {
    let text = fs::read_to_string("tests/fixtures/disc_step.report.golden.json").unwrap();
    let report = review::read(&text).unwrap();
    let main = "stock Cylinder(dia=120, h=30, x=0, y=0, z=30)\n";
    let once = review::with_rejection(main, "F7");
    assert_eq!(once, "stock Cylinder(dia=120, h=30, x=0, y=0, z=30)\n\n# rejected F7\n");
    let twice = review::with_rejection(&once, "F5");
    assert!(twice.ends_with("\n\n# rejected F7\n# rejected F5\n"), "a second rejection sits under the first: {twice}");
    assert_eq!(review::rejected_ids(&twice), ["F7", "F5"]);
    assert!(review::rejected_ids("# rejected because it rains\n# rejectedF9\n#rejected F7, F7\n") == ["F7"], "prose is not a rejection; an id counts once");
    let part = parse::parse_project(&twice, None).expect("the lines are comments to the parser");
    let rejected = review::rejected_ids(&twice);
    let rows = review::rows(&report, &part, &rejected);
    let by = |id: &str| rows.iter().find(|r| r.id == id).unwrap();
    assert!(by("F7").rejected && by("F12").rejected, "a rejection on one member of F7_Bore rejects the group");
    assert!(by("F5").rejected && !by("F13").rejected && !by("F3").rejected);
    let page = review::render_marked(&report, &rows);
    assert!(page.lines().any(|l| l.trim_start().starts_with("F7 ") && l.ends_with("· rejected")), "{page}");
    assert!(page.lines().any(|l| l.trim_start().starts_with("F13 ") && !l.contains('·')), "{page}");
    assert!(!review::render(&report).contains("· rejected"), "the bare page carries no answers");
    let back = review::without_rejection(&twice, &["F7", "F12"]).unwrap();
    assert_eq!(review::rejected_ids(&back), ["F5"]);
    assert!(review::without_rejection(&back, &["F7"]).is_err(), "nothing to take back is said");
    assert!(review::without_rejection("# rejected F5 F6\n", &["F5"]).is_err(), "a line naming others too is the hand's");
}
