// ---- the operation template's pins (Phase 6, slice 5) ------------------------
//
// The template's whole claim is WHICH ops consume a feature kind and HOW
// each names it. These pins lock that claim to validate()'s own acceptance
// the only way that avoids a second home for the rules: FILL the skeleton
// the way the machinist would (a real tool for `T?`, a real number for
// every `?`) and require the result to PARSE — the parser's validate is
// the judge, not a restatement. A template whose filled form the engine
// rejects is a template that lies.

use nacs::{parse, template};

/// A machinist's fill: the named tool for `T?`, one number for every `?`.
fn fill(line: &str, tool: &str) -> String {
    line.replace("tool=T?", &format!("tool={tool}")).replace("=?", "=3.0")
}

/// Parse a project source with the filled template lines appended — the
/// paste the template exists for.
fn parses_with(src: &str, lines: &[String]) {
    let full = format!("{src}{}\n", lines.join("\n"));
    parse::parse(&full).unwrap_or_else(|e| {
        panic!("filled template must parse, but the engine rejected: {e}\nlines: {lines:#?}")
    });
}

const HEADER: &str = "\
units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\n";

#[test]
fn test_plane_template_fills_to_a_valid_floor() {
    let src = format!("{HEADER}tool T5 Endmill(dia=10.0)\nfeature Top Plane(z=-2.0)\n");
    let p = parse::parse(&src).unwrap();
    let lines = template::op_lines(&p, "Top").unwrap();
    // The volume, then the floor (Phase 61 slice 1): a Plane takes the Clear.
    assert_eq!(lines, vec!["run Clear(tool=T?, profile=Top, stepover=?)", "run End(tool=T?, surface=Top, stepover=?)"]);
    parses_with(&src, &lines.iter().map(|l| fill(l, "T5")).collect::<Vec<_>>());
}

#[test]
fn test_profile_pocket_template_fills_to_clear_then_finish() {
    let src = format!(
        "{HEADER}tool T5 Endmill(dia=10.0)\n\
         feature Window ProfilePocket(extent=6.0\n  20 20\n  80 20\n  80 60\n  20 60\n)\n"
    );
    let p = parse::parse(&src).unwrap();
    let lines = template::op_lines(&p, "Window").unwrap();
    assert_eq!(
        lines,
        // The volume, the floor, the wall (Phase 27 slice 4: `Floor` takes
        // a pocket's floor — a spring pass unless the Clear leaves an
        // allowance, and a line the author is free to delete).
        vec![
            "run Clear(tool=T?, profile=Window, stepover=?)",
            "run End(tool=T?, surface=Window, stepover=?)",
            "run Side(tool=T?, surface=Window)",
        ]
    );
    parses_with(&src, &lines.iter().map(|l| fill(l, "T5")).collect::<Vec<_>>());
}

#[test]
fn test_cylinder_bore_template_fills_to_the_drill_clear_finish_trio() {
    let src = format!(
        "{HEADER}tool T5 Endmill(dia=10.0)\ntool T7 Drill(dia=8.0)\n\
         feature Hole CylinderBore(dia=30.0, extent=12.0, x=50.0, y=40.0)\n"
    );
    let p = parse::parse(&src).unwrap();
    let lines = template::op_lines(&p, "Hole").unwrap();
    assert_eq!(
        lines,
        vec![
            "run Form(points=[Hole], tool=T?, cycle=Simple)",
            "run Clear(tool=T?, profile=Hole, stepover=?)",
            "run Side(tool=T?, surface=Hole, stepover=?)",
        ]
    );
    // The machinist's fill is per-line: the drill line gets the drill.
    let filled: Vec<String> = lines
        .iter()
        .map(|l| fill(l, if l.contains("points=") { "T7" } else { "T5" }))
        .collect();
    parses_with(&src, &filled);
}

#[test]
fn test_surface_template_fills_to_a_valid_ball_finish() {
    // The stock stands above the dome's base plane (sphere.cam's own
    // stock): a Clear roughs the moat down to it, then the ball finishes.
    // With the base AT the stock top the Clear has nothing to rough and
    // validate says so — the skeleton is the shop order, and the stock
    // is the machinist's to declare.
    let src = "units mm\nfeed 800\nplunge 300\nrapid_z 25\nclearance_z 40\nspindle 3000\n\
stock Box(w=100, l=80, h=47, x=0, y=0, z=22)\ntool T1 Endmill(dia=10.0)\ntool T9 Ballmill(dia=6.0)\nfeature Dome Sphere(dia=20, x=30, y=20, z=0)\n".to_string();
    let p = parse::parse(&src).unwrap();
    let lines = template::op_lines(&p, "Dome").unwrap();
    assert_eq!(
        lines,
        vec![
            "run Clear(tool=T?, profile=Dome, stepover=?, stepdown=?)",
            "run Copy(tool=T?, surface=Dome, stepover=?)",
        ]
    );
    // Filled the way the shop would — the endmill roughs the moat (a
    // Clear refuses a ball by name), the ball finishes the dome: which
    // tool each line wants stays the machinist's, as the seat line's does.
    let filled: Vec<String> = lines.iter().enumerate().map(|(i, l)| fill(l, if i == 0 { "T1" } else { "T9" })).collect();
    parses_with(&src, &filled);
}

/// The corpus is the proof of the shop order: every feature kind some
/// shipped project Clears has a Clear skeleton, and every Clear skeleton
/// spells `stepdown=?` exactly where validate requires a stepdown (a
/// standing boss or revolve) and nowhere else — so the window's
/// creation form can schedule every Clear the corpus runs.
#[test]
fn test_every_kind_the_corpus_clears_has_a_clear_skeleton_and_stepdown_lands_where_validate_wants_it() {
    let mut cleared: std::collections::BTreeSet<&'static str> = std::collections::BTreeSet::new();
    let mut offered: std::collections::BTreeSet<&'static str> = std::collections::BTreeSet::new();
    for entry in std::fs::read_dir(".").unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().to_string();
        if !name.ends_with(".cam") {
            continue;
        }
        let Ok(p) = parse::load_project(&name) else { continue };
        for op in &p.operations {
            if op.op_type == parse::OpType::Clear {
                let f = p.features.get(&op.feature_name).unwrap();
                cleared.insert(f.kind());
                // `stepdown` is REQUIRED over standing material (a revolve or a
                // boss), where the skeleton spells it; on every other host it
                // is optional since Phase 61 slice 2 (absent is one level), so
                // the skeleton stays silent and a fixture may or may not use it.
                let standing = !matches!(f, parse::Feature::ProfilePocket { .. } | parse::Feature::CylinderBore { .. } | parse::Feature::Plane { .. } | parse::Feature::Face { .. });
                let spelled = template::op_lines(&p, &op.feature_name).unwrap().iter().any(|l| l.contains("stepdown=?"));
                assert_eq!(standing, spelled, "{name}/{}: the skeleton spells stepdown exactly where validate requires it", op.feature_name);
                assert!(!standing || op.stepdown.is_some(), "{name}/{}: a Clear over standing material carries its stepdown", op.feature_name);
            }
        }
        for (fname, f) in &p.features {
            if template::op_lines(&p, fname).unwrap().iter().any(|l| l.starts_with("run Clear(")) {
                offered.insert(f.kind());
            }
        }
    }
    for kind in &cleared {
        assert!(offered.contains(kind), "{kind}: the corpus Clears it and the skeleton offers no Clear");
    }
    assert!(cleared.len() >= 10, "{cleared:?}");
}

#[test]
fn test_boss_wall_template_fills_to_a_valid_flank_finish() {
    let src = format!(
        "{HEADER}tool T5 Endmill(dia=10.0)\n\
         feature Post CylinderBoss(dia=24.0, extent=8.0, x=50.0, y=40.0, z=-8.0)\n"
    );
    let p = parse::parse(&src).unwrap();
    let lines = template::op_lines(&p, "Post").unwrap();
    // The moat first (Phase 22 slice 3: `stepdown=?` is the skeleton's
    // question now — validate requires it on a Clear over standing
    // material, and no skeleton used to spell it), then the flank.
    assert_eq!(
        lines,
        vec![
            "run Clear(tool=T?, profile=Post, stepover=?, stepdown=?)",
            "run Side(tool=T?, surface=Post, stepover=?)",
        ]
    );
    parses_with(&src, &lines.iter().map(|l| fill(l, "T5")).collect::<Vec<_>>());
}

/// An ISLAND's skeleton is its Finish alone (Phase 26, fork 6A): the same
/// post, declared standing inside a pocket, loses its moat line — the
/// pocket's own Clear roughs around it, and a Clear aimed at the island is
/// the line `validate` would refuse. The host's skeleton is untouched, and
/// the two together, filled, are a whole legal program. Same through a face.
#[test]
fn test_an_islands_skeleton_is_its_finish_and_its_hosts_clear_is_its_roughing() {
    let src = format!(
        "{HEADER}tool T5 Endmill(dia=10.0)\n\
         feature Pocket ProfilePocket(extent=8.0\n  10 10\n  90 10\n  90 70\n  10 70\n)\n\
         feature Post CylinderBoss(dia=24.0, extent=8.0, x=50.0, y=40.0, z=-8.0)\n"
    );
    let p = parse::parse(&src).unwrap();
    let post = template::op_lines(&p, "Post").unwrap();
    assert_eq!(post, vec!["run Side(tool=T?, surface=Post, stepover=?)"]);
    let pocket = template::op_lines(&p, "Pocket").unwrap();
    assert_eq!(
        pocket,
        vec![
            "run Clear(tool=T?, profile=Pocket, stepover=?)",
            "run End(tool=T?, surface=Pocket, stepover=?)",
            "run Side(tool=T?, surface=Pocket)",
        ]
    );
    let all: Vec<String> = pocket.iter().chain(&post).map(|l| fill(l, "T5")).collect();
    parses_with(&src, &all);
    // The line the skeleton dropped is the line validate refuses.
    let moat = format!("{src}run Clear(tool=T5, profile=Post, stepover=4.0, stepdown=4.0)\n");
    assert!(parse::parse(&moat).unwrap_err().contains("'Post' is an island of 'Pocket'"));

    let faced = format!(
        "{HEADER}tool T5 Endmill(dia=10.0)\n\
         feature Deck Plane(z=-8.0)\n\
         feature Post CylinderBoss(dia=24.0, extent=8.0, x=50.0, y=40.0, z=-8.0)\n"
    );
    let p = parse::parse(&faced).unwrap();
    assert_eq!(template::op_lines(&p, "Post").unwrap(), vec!["run Side(tool=T?, surface=Post, stepover=?)"]);
}

/// The polyline/wire wall-tracing arm — the one whose ops take no
/// stepover — filled and parsed, so every distinct template shape has a
/// lock and the module's claim covers the whole match.
#[test]
fn test_wall_tracing_templates_fill_to_valid_full_depth_finishes() {
    let src = format!(
        "{HEADER}tool T5 Endmill(dia=10.0)\n\
         feature Outline ProfileBoss(extent=2.0\n  20 20\n  20 60\n  80 60\n  80 20\n)\n\
         feature Rim ContourPocket(extent=2.0\n  20 20 -1\n  70 20 -2\n  70 55 -3\n  20 55 -2\n)\n"
    );
    let p = parse::parse(&src).unwrap();
    // The wire pocket: its wall alone (a Clear over a rim is refused —
    // the wire declares a wall, not an interior). The polyline boss:
    // its moat, then its wall.
    let lines = template::op_lines(&p, "Rim").unwrap();
    assert_eq!(lines, vec!["run Side(tool=T?, surface=Rim)"]);
    parses_with(&src, &lines.iter().map(|l| fill(l, "T5")).collect::<Vec<_>>());
    let lines = template::op_lines(&p, "Outline").unwrap();
    assert_eq!(
        lines,
        vec![
            "run Clear(tool=T?, profile=Outline, stepover=?, stepdown=?)",
            "run Side(tool=T?, surface=Outline)",
        ]
    );
    parses_with(&src, &lines.iter().map(|l| fill(l, "T5")).collect::<Vec<_>>());
}

/// A hole that declares a SEAT gets a second line at its points for it — the shop
/// order `cbore.cam` and `csink.cam` spell. Filled with the seat's own
/// tool, the whole skeleton parses: the counterbore's seat law
/// (`cbore_dia` == the endmill's diameter) is validate()'s to judge, and
/// the filled template satisfies it.
#[test]
fn test_a_declared_seat_earns_its_own_drill_line() {
    let plain = format!(
        "{HEADER}tool T7 Drill(dia=6.0)\n\
         feature Hole CylinderBore(dia=6.0, extent=15.0, x=30.0, y=40.0)\n"
    );
    let p = parse::parse(&plain).unwrap();
    assert_eq!(
        template::op_lines(&p, "Hole").unwrap().iter().filter(|l| l.contains("points=")).count(),
        1,
        "a plain bore asks for one drilling pass"
    );

    let seated = format!(
        "{HEADER}tool T7 Drill(dia=6.0)\ntool T4 Endmill(dia=16.0)\n\
         feature Hole CylinderBore(dia=6.0, extent=15.0, x=30.0, y=40.0, cbore_dia=16.0, cbore_depth=6.0)\n"
    );
    let p = parse::parse(&seated).unwrap();
    let lines = template::op_lines(&p, "Hole").unwrap();
    let drills: Vec<&String> = lines.iter().filter(|l| l.contains("points=")).collect();
    assert_eq!(drills.len(), 2, "a declared seat is its own pass: {lines:#?}");
    assert!(drills[1].contains("the seat"), "the second line names itself: {}", drills[1]);

    // Filled the way the shop would — the drill drills, the endmill seats.
    let filled: Vec<String> = drills
        .iter()
        .enumerate()
        .map(|(i, l)| fill(l, if i == 0 { "T7" } else { "T4" }))
        .collect();
    parses_with(&seated, &filled);
}

/// An undeclared name is a hard error that lists what IS declared — the
/// template's own error convention, mirroring the parser's referenced-
/// feature message shape.
#[test]
fn test_unknown_feature_is_an_error_naming_the_declared() {
    let src = format!("{HEADER}tool T5 Endmill(dia=10.0)\nfeature Top Plane(z=-2.0)\nfeature Hole CylinderBore(dia=10.0, extent=5.0, x=20.0, y=20.0)\n");
    let p = parse::parse(&src).unwrap();
    let err = template::op_lines(&p, "Nope").expect_err("an undeclared feature must reject");
    assert!(err.contains("'Nope'") && err.contains("Hole, Top"), "got: {err}");
}

/// The CLI voice: `--template <feature>` prints the same lines the panel
/// shows, newline-terminated; a bad name exits nonzero with an empty
/// stdout (no partial artifact).
#[test]
fn test_template_mode_prints_the_lines_and_refuses_bad_names() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_nacs"))
        .args(["--template", "Notch", "plate.cam"])
        .output()
        .expect("run nacs");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        text,
        "run Clear(tool=T?, profile=Notch, stepover=?)\nrun End(tool=T?, surface=Notch, stepover=?)\nrun Side(tool=T?, surface=Notch)\n"
    );

    let bad = std::process::Command::new(env!("CARGO_BIN_EXE_nacs"))
        .args(["--template", "Nope", "plate.cam"])
        .output()
        .expect("run nacs");
    assert!(!bad.status.success());
    assert!(bad.stdout.is_empty(), "no partial artifact on stdout");
    assert!(String::from_utf8_lossy(&bad.stderr).contains("not declared"));
}
