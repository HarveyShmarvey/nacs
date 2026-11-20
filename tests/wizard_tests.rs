// ---- the feature-statement wizard's pins (Phase 19, slice 1) -----------
//
// Fork 1's lock, the template precedent applied to the feature half: the
// wizard's claim is WHICH keys a variant takes and HOW the statement is
// spelled, so the pins FILL an answer set and require the result to
// (a) byte-equal the corpus's own hand-typed spelling and (b) PARSE —
// the one parser is the judge, never a restatement here. A wizard whose
// statement the engine rejects is a wizard that lies.

use nacs::{parse, template};

fn kv(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

const HEADER: &str = "\
units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\n";

/// Every variant, filled with the corpus's own values: the statement
/// checks alone (`check_statement`) AND parses+validates inside a project
/// — and the table's length is pinned to the spec table's, so a variant
/// added without a fill fails here by count.
#[test]
fn test_every_variant_fills_checks_and_parses_into_a_project() {
    let fills: &[(&str, &str, &[(&str, &str)])] = &[
        ("Plane", "TopFace", &[("z", "-0.5")]),
        ("ProfilePocket", "Window", &[("extent", "2.0"), ("points", "20,20;80,20;80,60;20,60")]),
        ("ProfileBoss", "Outline", &[("extent", "2.0"), ("points", "10,10;10,70;90,70;90,10")]),
        // Phase 28: a loop that can contain loops — the outer CCW, the inner CW,
        // '|' between them.
        ("Face", "Deck", &[("z", "-1.0"), ("points", "10,10;90,10;90,70;10,70|40,30;40,50;60,50;60,30")]),
        ("ContourPocket", "Rim", &[("extent", "3.0"), ("points", "20,20,0;80,20,-2;80,60,0;20,60,-2")]),
        ("ContourBoss", "Crest", &[("extent", "0"), ("points", "30,30,-1;30,60,-3;70,60,-1;70,30,-3")]),
        ("CylinderBore", "Bore", &[("dia", "40.0"), ("extent", "12.0"), ("x", "50.0"), ("y", "40.0")]),
        // Phase 41 slice 5: the fixture's own bore — both angles required.
        (
            "TiltedBore",
            "Drain",
            &[("dia", "12"), ("extent", "14"), ("x", "70"), ("y", "40"), ("tilt", "30"), ("heading", "180")],
        ),
        (
            "CylinderBoss",
            "Post",
            &[("dia", "40.0"), ("extent", "12.0"), ("x", "50.0"), ("y", "40.0"), ("z", "-12.0")],
        ),
        ("Sphere", "TopDome", &[("dia", "40"), ("x", "50"), ("y", "40"), ("z", "0")]),
        ("Cone", "Point", &[("dia", "40"), ("height", "20"), ("x", "50"), ("y", "40"), ("z", "0")]),
        ("Torus", "Ring", &[("major_dia", "50"), ("minor_dia", "8"), ("x", "50"), ("y", "40"), ("z", "0")]),
        ("Mesh", "Blob", &[("file", "mesh.cam/pyramid.stl"), ("x", "10"), ("y", "10"), ("z", "0")]),
        (
            "SplineRevolve",
            "Cap",
            &[("file", "spline_cap.cam/meridian.spline"), ("x", "50"), ("y", "40"), ("z", "0")],
        ),
        (
            "SplineRevolveFunnel",
            "Funnel",
            &[("file", "spline_funnel.cam/meridian.spline"), ("x", "50"), ("y", "40")],
        ),
        ("NurbsPatch", "Dome", &[("file", "nurbs_patch.cam/cap.patch"), ("x", "18"), ("y", "18"), ("z", "0")]),
    ];
    assert_eq!(
        fills.len(),
        template::FEATURE_SPECS.len(),
        "every variant in the spec table gets a fill — a new variant lands here too"
    );
    for (variant, name, answers) in fills {
        let stmt = template::feature_statement(variant, name, &kv(answers))
            .unwrap_or_else(|e| panic!("{variant} must render: {e}"));
        parse::check_feature_statement(&stmt)
            .unwrap_or_else(|e| panic!("{variant} must check alone: {e}\n{stmt}"));
        let full = format!("{HEADER}{stmt}\n");
        parse::parse(&full)
            .unwrap_or_else(|e| panic!("{variant} must parse and validate in a project: {e}\n{stmt}"));
    }
}

/// The statement differs by zero bytes from the hand-typed one — pinned
/// against the corpus's own lines: required-only, a seat key after
/// placement, a shape-slot optional before it, and the block form with
/// its four-space point lines and the `)` alone.
#[test]
fn test_statements_byte_equal_the_corpus_hand_typed_lines() {
    // cylinder_bore.cam/main.part — required keys, canonical order.
    assert_eq!(
        template::feature_statement(
            "CylinderBore",
            "Bore",
            &kv(&[("dia", "40.0"), ("extent", "12.0"), ("x", "50.0"), ("y", "50.0")]),
        )
        .unwrap(),
        "feature Bore CylinderBore(dia=40.0, extent=12.0, x=50.0, y=50.0)"
    );
    // csink.cam/main.part — the seat keys land AFTER placement.
    assert_eq!(
        template::feature_statement(
            "CylinderBore",
            "Hole1",
            &kv(&[
                ("csink_angle", "90.0"),
                ("dia", "6.0"),
                ("extent", "15.0"),
                ("x", "30.0"),
                ("y", "40.0"),
                ("csink_dia", "11.0"),
            ]),
        )
        .unwrap(),
        "feature Hole1 CylinderBore(dia=6.0, extent=15.0, x=30.0, y=40.0, csink_dia=11.0, csink_angle=90.0)",
        "the answer set's order does not matter — the spec's slots do"
    );
    // profile_pocket.cam/main.part — the block: opener, four-space point
    // lines, the `)` alone on its line.
    assert_eq!(
        template::feature_statement(
            "ProfilePocket",
            "Window",
            &kv(&[("extent", "2.0"), ("points", "20,20;80,20;80,80;20,80")]),
        )
        .unwrap(),
        "feature Window ProfilePocket(extent=2.0\n    20 20\n    80 20\n    80 80\n    20 80\n)"
    );
    // mesh.cam/main.part — a quoted file value passes through verbatim.
    assert_eq!(
        template::feature_statement(
            "Mesh",
            "Blob",
            &kv(&[("file", "\"pyramid.stl\""), ("x", "10"), ("y", "10"), ("z", "0")]),
        )
        .unwrap(),
        "feature Blob Mesh(file=\"pyramid.stl\", x=10, y=10, z=0)"
    );
}

/// A missing answer is refused with a sentence that names the missing key
/// and its kind — the "series of questions" as a refusal loop.
#[test]
fn test_missing_answers_are_named_with_their_kind() {
    let err = template::feature_statement("CylinderBore", "Hole", &kv(&[("dia", "6.0")]))
        .expect_err("three keys are missing");
    for needed in ["extent (a number)", "x (a number)", "y (a number)"] {
        assert!(err.contains(needed), "the refusal names {needed}: {err}");
    }
    assert!(err.contains("optionally"), "the whole menu is in the sentence: {err}");

    let err = template::feature_statement("ProfilePocket", "Window", &kv(&[("extent", "2.0")]))
        .expect_err("the points are missing");
    assert!(err.contains("points (x,y pairs"), "points are named with their shape: {err}");

    let err = template::feature_statement("ContourPocket", "Rim", &kv(&[("extent", "3.0")]))
        .expect_err("the wire points are missing");
    assert!(err.contains("points (x,y,z triples"), "the wire names its own arity: {err}");
}

/// The wizard refuses only what the parser cannot catch LOUDLY: an
/// unknown variant, an unspellable name, an answer given twice, a value
/// that '#' would silently truncate, a point that would silently vanish.
#[test]
fn test_the_wizards_own_refusals_are_the_silent_wrongs() {
    let err = template::feature_statement("Helix", "Thread", &kv(&[]))
        .expect_err("a helix is a motion family, not a statement");
    assert!(err.contains("not a feature variant") && err.contains("CylinderBore") && err.contains("NurbsPatch"),
        "the vocabulary is the answer: {err}");

    let err = template::feature_statement("Plane", "My Face", &kv(&[("z", "-1")]))
        .expect_err("a name with a space cannot be referenced");
    assert!(err.contains("cannot name a feature"), "got: {err}");

    let err = template::feature_statement("Plane", "Top", &kv(&[("z", "-1"), ("z", "-2")]))
        .expect_err("the parser would keep the last one silently");
    assert!(err.contains("answered twice"), "got: {err}");

    let err = template::feature_statement("Plane", "Top", &kv(&[("z", "-1 # trust me")]))
        .expect_err("'#' opens a comment anywhere on a line");
    assert!(err.contains("silently change meaning"), "got: {err}");

    let err = template::feature_statement(
        "ProfilePocket",
        "Window",
        &kv(&[("extent", "2.0"), ("points", "20,20;;80,80")]),
    )
    .expect_err("a blank point line is skipped by the parser");
    assert!(err.contains("point 2 of 'points' is empty"), "got: {err}");
}

/// Everything the parser CAN catch loudly passes through to the parser's
/// own sentence — the whitelist, the reserved `z` on a wire, a point's
/// arity — never a second spelling of those rules in the wizard.
#[test]
fn test_loud_wrongs_pass_through_to_the_parsers_sentence() {
    let stmt = template::feature_statement(
        "CylinderBore",
        "Hole",
        &kv(&[("dia", "6.0"), ("extent", "15.0"), ("x", "30.0"), ("y", "40.0"), ("dai", "6.0")]),
    )
    .expect("the wizard renders; the parser judges");
    let err = parse::check_feature_statement(&stmt).expect_err("the whitelist refuses");
    assert!(err.contains("has no field named 'dai'"), "the parser's sentence: {err}");

    let stmt = template::feature_statement(
        "ContourPocket",
        "Rim",
        &kv(&[("extent", "3.0"), ("z", "1.0"), ("points", "20,20,0;80,20,-2;50,60,0")]),
    )
    .expect("the wizard renders; the parser judges");
    let err = parse::check_feature_statement(&stmt).expect_err("a wire takes no z datum");
    assert!(err.contains("takes no z datum"), "the parser's own reserved-word sentence: {err}");

    let stmt = template::feature_statement(
        "ProfilePocket",
        "Window",
        &kv(&[("extent", "2.0"), ("points", "20,20,5;80,20;80,80;20,80")]),
    )
    .expect("the wizard renders; the parser judges");
    let err = parse::check_feature_statement(&stmt).expect_err("a planar point is two numbers");
    assert!(err.contains("needs 2 numbers"), "the parser's arity sentence: {err}");
}

/// Fork 3's depth, pinned: winding is `validate()`'s law, which
/// `check_statement` (one statement, no project) deliberately does not
/// reach — the whole-project re-parse does. A statement can be honest
/// alone and refused in company.
#[test]
fn test_check_depth_statement_alone_passes_project_refuses_winding() {
    let stmt = template::feature_statement(
        "ProfilePocket",
        "Window",
        &kv(&[("extent", "2.0"), ("points", "20,20;20,80;80,80;80,20")]), // CW: a boss's winding
    )
    .unwrap();
    parse::check_feature_statement(&stmt).expect("alone, the statement parses — winding is validate()'s");
    let err = parse::parse(&format!("{HEADER}{stmt}\n"))
        .expect_err("in a project, the winding law refuses");
    assert!(err.contains("Wind the loop"), "the parser's winding sentence: {err}");
}

// ---- the CLI voice (Phase 19, slice 2) ---------------------------------

fn nacs(args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_nacs")).args(args).output().expect("run nacs")
}

/// `--new` from nothing: the statement is the artifact, newline-terminated
/// on stdout, and the statement-level check is the honest whole.
#[test]
fn test_new_mode_prints_the_statement_from_nothing() {
    let out = nacs(&["--new", "CylinderBore Hole dia=6.0 extent=15.0 x=30.0 y=40.0"]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "feature Hole CylinderBore(dia=6.0, extent=15.0, x=30.0, y=40.0)\n"
    );

    // The block form crosses the command line intact — no spaces in the
    // points answer, lines in the artifact.
    let out = nacs(&["--new", "ProfilePocket Window extent=2.0 points=20,20;80,20;80,80;20,80"]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "feature Window ProfilePocket(extent=2.0\n    20 20\n    80 20\n    80 80\n    20 80\n)\n"
    );
}

/// The refusal loop IS the question series: a missing answer exits
/// nonzero with the key and its kind on stderr and an EMPTY stdout — no
/// partial artifact, the --template convention.
#[test]
fn test_new_mode_refusals_name_the_next_answer_and_leak_nothing() {
    let out = nacs(&["--new", "CylinderBore Hole dia=6.0"]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty(), "no partial artifact on stdout");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("still needs") && err.contains("extent (a number)"), "got: {err}");

    // A malformed answer set is a usage error (exit 2), and says its shape.
    let out = nacs(&["--new", "Plane Top z"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("'z' is not an answer"));

    // Two artifacts refuse by name, the standing rule.
    let out = nacs(&["--new", "Plane Top z=-1", "--outline", "plate.cam"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("one artifact each"));
}

/// With a project, the check is the whole project re-parsed in memory with
/// the statement appended (fork 3) — and one refusal is the wizard's own:
/// a duplicate feature name, which the parser's name-keyed map would
/// silently shadow (the duplicate-tool lesson, applied to features).
#[test]
fn test_new_mode_with_a_project_checks_in_company() {
    // Notch lives in plate.cam's geometry.part — the duplicate refusal
    // sees both part files.
    let out = nacs(&["--new", "Plane Notch z=-1.0", "plate.cam"]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty(), "no partial artifact on stdout");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("'Notch' is already declared"),
        "the shadow is refused by name: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Winding is validate()'s law: the statement is honest alone and
    // refused in company, with the parser's own sentence.
    let out = nacs(&["--new", "ProfileBoss Fence extent=2.0 points=20,20;80,20;80,60;20,60", "plate.cam"]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("Wind the loop"),
        "the winding sentence: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // A statement the project accepts prints, and the project is untouched
    // — the CLI never writes (fork 4).
    let before = std::fs::read_to_string("plate.cam/main.part").unwrap();
    let out = nacs(&["--new", "CylinderBore Dowel dia=6.0 extent=5.0 x=10.0 y=10.0", "plate.cam"]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "feature Dowel CylinderBore(dia=6.0, extent=5.0, x=10.0, y=10.0)\n"
    );
    assert_eq!(std::fs::read_to_string("plate.cam/main.part").unwrap(), before);
}

/// The seventh write's refusal predicate (DESIGN.md §1): plate.cam keeps
/// its features in geometry.part, so a created feature's destination
/// there is ambiguous and the window refuses; a project whose second
/// source is an external (a meridian, a machine file) is NOT ambiguous —
/// only geometry.part by name counts.
#[test]
fn test_second_file_declares_features_is_plate_cams_shape() {
    let p = parse::load_project("plate.cam").unwrap();
    assert!(nacs::view::second_file_declares_features(&p));
    let p = parse::load_project("spline_cap.cam").unwrap();
    assert!(
        !nacs::view::second_file_declares_features(&p),
        "an external second source is not a feature home"
    );
}

/// `spec_for` answers for every parsed feature — the corpus is the
/// enumeration, and the block variants report their point shape.
#[test]
fn test_spec_for_covers_the_parsed_corpus() {
    let src = format!(
        "{HEADER}feature Top Plane(z=-2.0)\n\
         feature Window ProfilePocket(extent=2.0\n    20 20\n    80 20\n    80 60\n    20 60\n)\n\
         feature Rim ContourPocket(extent=3.0\n    20 20 0\n    80 20 -2\n    80 60 0\n    20 60 -2\n)\n"
    );
    let p = parse::parse(&src).unwrap();
    let spec = template::spec_for(&p.features["Top"]);
    assert_eq!((spec.variant, spec.points), ("Plane", template::Points::None));
    let spec = template::spec_for(&p.features["Window"]);
    assert_eq!((spec.variant, spec.points), ("ProfilePocket", template::Points::Planar));
    let spec = template::spec_for(&p.features["Rim"]);
    assert_eq!((spec.variant, spec.points), ("ContourPocket", template::Points::Wire));
}

// ---- the tool half (Phase 20, slice 2) ----------------------------------
//
// The same lock applied to the `tool` statement: one spec per kind, the
// corpus's key order, fill-and-parse over every kind. The kind word was
// the one thing the parser did not judge; since Phase 36 slice 0 it does,
// and the wizard's menu and the parser's are pinned to be one list.

/// Which of a tool's surfaces cut is derived from its kind and its records
/// (Phase 36 slice 0): a rod by its word, a form by its record, and a drawn
/// envelope that narrows going up is the one tool with a back.
#[test]
fn test_each_tool_kind_derives_the_surfaces_it_cuts() {
    use parse::Cut::*;
    let cuts = |line: &str| {
        let p = parse::parse(&format!("{HEADER}{line}\nfeature Top Plane(z=-0.5)\n")).unwrap_or_else(|e| panic!("{line}: {e}"));
        parse::derive_tool_cuts(&p.tools[&1])
    };
    assert_eq!(cuts("tool T1 Endmill(dia=10.0)"), vec![End, Side]);
    assert_eq!(cuts("tool T1 Ballmill(dia=10.0)"), vec![Nose, Side]);
    assert_eq!(cuts("tool T1 Drill(dia=10.0)"), vec![Form]);
    assert_eq!(cuts("tool T1 FormTool(dia=10.0, profile=\"sphere_formtool.cam/tool_profile.svg\")"), vec![Form]);
    assert_eq!(cuts("tool T1 ChamferMill(angle=90.0, gauge_dia=8.0, gauge_z=3.75, dia=12.0, tip_dia=0.5)"), vec![Form]);
    // A drawn profile decides whatever the word: the records come first.
    assert_eq!(cuts("tool T1 Endmill(dia=10.0, profile=\"sphere_formtool.cam/tool_profile.svg\")"), vec![Form]);
    // Every kind the corpus declares derives something.
    for dir in std::fs::read_dir(".").unwrap().flatten().filter(|d| d.file_name().to_string_lossy().ends_with(".cam")) {
        let p = parse::load_project(dir.path().to_str().unwrap()).unwrap();
        for t in p.tools.values() {
            assert!(!parse::derive_tool_cuts(t).is_empty(), "{:?} T{}", dir.path(), t.num);
        }
    }
    assert_eq!(End.label(), "end");
}

/// Every kind fills, checks alone, and parses into a project — and the
/// fill table's length is pinned to the spec table's.
#[test]
fn test_every_tool_kind_fills_checks_and_parses_into_a_project() {
    let fills: &[(&str, usize, &[(&str, &str)])] = &[
        ("Endmill", 1, &[("dia", "10.0")]),
        ("Ballmill", 9, &[("dia", "6.0")]),
        ("Drill", 7, &[("dia", "6.0")]),
        ("FormTool", 1, &[("dia", "10.0"), ("profile", "\"sphere_formtool.cam/tool_profile.svg\"")]),
        (
            "CornerRounder",
            2,
            &[("radius", "3.0"), ("effective_dia", "6.0"), ("gauge_z", "6.0"), ("dia", "12.0"), ("pilot", "5.94")],
        ),
        (
            "ChamferMill",
            3,
            &[("angle", "90.0"), ("gauge_dia", "8.0"), ("gauge_z", "3.75"), ("dia", "12.0"), ("tip_dia", "0.5")],
        ),
    ];
    assert_eq!(fills.len(), template::TOOL_SPECS.len(), "every kind in the spec table gets a fill");
    for (kind, num, answers) in fills {
        let stmt = template::tool_statement(kind, *num, &kv(answers)).unwrap_or_else(|e| panic!("{kind} must render: {e}"));
        parse::check_tool_statement(&stmt).unwrap_or_else(|e| panic!("{kind} must check alone: {e}\n{stmt}"));
        parse::parse(&format!("{HEADER}{stmt}\nfeature Top Plane(z=-0.5)\n"))
            .unwrap_or_else(|e| panic!("{kind} must parse and validate in a project: {e}\n{stmt}"));
    }
}

/// Zero bytes from the corpus's own hand-typed lines: the form keys before
/// the body OD on a rounder and a chamfer mill, the body ladder in the
/// corpus's order on a ball mill with a holder.
#[test]
fn test_tool_statements_byte_equal_the_corpus_hand_typed_lines() {
    // edge_round.cam/main.part
    assert_eq!(
        template::tool_statement(
            "CornerRounder",
            2,
            &kv(&[("dia", "12.0"), ("pilot", "5.94"), ("radius", "3.0"), ("effective_dia", "6.0"), ("gauge_z", "6.0")]),
        )
        .unwrap(),
        "tool T2 CornerRounder(radius=3.0, effective_dia=6.0, gauge_z=6.0, dia=12.0, pilot=5.94)"
    );
    // chamfer.cam/main.part
    assert_eq!(
        template::tool_statement(
            "ChamferMill",
            3,
            &kv(&[("tip_dia", "0.5"), ("dia", "12.0"), ("angle", "90.0"), ("gauge_dia", "8.0"), ("gauge_z", "3.75")]),
        )
        .unwrap(),
        "tool T3 ChamferMill(angle=90.0, gauge_dia=8.0, gauge_z=3.75, dia=12.0, tip_dia=0.5)"
    );
    // spline_funnel_holder.cam/main.part — the body ladder with a holder.
    assert_eq!(
        template::tool_statement(
            "Ballmill",
            10,
            &kv(&[
                ("holder_body_dia", "50.0"),
                ("holder", "\"ER32 slim nose\""),
                ("dia", "6.0"),
                ("stickout", "8.0"),
                ("holder_nose_len", "40.0"),
                ("shank_dia", "10.0"),
                ("flute_len", "3.0"),
                ("holder_nose_dia", "32.0"),
            ]),
        )
        .unwrap(),
        "tool T10 Ballmill(dia=6.0, flute_len=3.0, shank_dia=10.0, stickout=8.0, holder=\"ER32 slim nose\", holder_nose_dia=32.0, holder_nose_len=40.0, holder_body_dia=50.0)"
    );
    // cylinder_bore_rest_flutes.cam — flute_count after stickout's slot.
    assert_eq!(
        template::tool_statement("Endmill", 5, &kv(&[("flute_count", "3"), ("dia", "10.0")])).unwrap(),
        "tool T5 Endmill(dia=10.0, flute_count=3)"
    );
}

/// The wizard's own refusals on a tool: a kind word nobody knows (the
/// parser refuses it too, by the same list), an unspellable number, an
/// answer twice, a '#'.
/// A missing key is named with its kind; a stranger passes through to the
/// parser's own whitelist sentence.
#[test]
fn test_the_tool_wizards_own_refusals_and_the_parsers_pass_through() {
    let err = parse::parse(&format!("{HEADER}tool T1 Endmil(dia=10.0)\nfeature Top Plane(z=-0.5)\n"))
        .expect_err("the parser judges the kind word");
    assert!(err.contains("T1 is a 'Endmil'") && err.contains("not a tool kind") && err.contains("ChamferMill"), "{err}");
    assert_eq!(
        parse::TOOL_KINDS.to_vec(),
        template::TOOL_SPECS.iter().map(|s| s.kind).collect::<Vec<_>>(),
        "one list of kinds, in one order"
    );
    let err = template::tool_statement("Endmil", 1, &kv(&[("dia", "10.0")])).expect_err("a phantom kind");
    assert!(err.contains("not a tool kind") && err.contains("Endmill") && err.contains("ChamferMill"), "{err}");

    assert!(template::parse_tool_number("T3").unwrap() == 3 && template::parse_tool_number("3").unwrap() == 3);
    let err = template::parse_tool_number("T0").expect_err("T0 names nothing");
    assert!(err.contains("positive whole number"), "{err}");
    let err = template::parse_tool_number("Tx").expect_err("not a number");
    assert!(err.contains("not a tool number"), "{err}");

    let err = template::tool_statement("Endmill", 1, &kv(&[("dia", "10"), ("dia", "12")])).expect_err("twice");
    assert!(err.contains("answered twice"), "{err}");
    let err = template::tool_statement("Endmill", 1, &kv(&[("dia", "10 # body")])).expect_err("'#'");
    assert!(err.contains("silently change meaning"), "{err}");

    let err = template::tool_statement("CornerRounder", 2, &kv(&[("radius", "3.0")])).expect_err("missing");
    for needed in ["effective_dia (a number)", "gauge_z (a number)", "dia (a number)", "pilot (a number)"] {
        assert!(err.contains(needed), "the refusal names {needed}: {err}");
    }
    assert!(err.contains("optionally") && err.contains("flute_len") && err.contains("source"), "the whole menu: {err}");

    let stmt = template::tool_statement("Endmill", 1, &kv(&[("dia", "10.0"), ("dai", "1")])).expect("renders; the parser judges");
    let err = parse::check_tool_statement(&stmt).expect_err("the whitelist refuses");
    assert!(err.contains("has no field named 'dai'"), "the parser's sentence: {err}");
    let stmt = template::tool_statement("Endmill", 1, &kv(&[("dia", "10.0"), ("gauge_z", "1")])).expect("renders");
    let err = parse::check_tool_statement(&stmt).expect_err("a gauge on no form");
    assert!(err.contains("gauge_z"), "the parser's family sentence: {err}");
    assert_eq!(template::lowest_undeclared_tool(&parse::parse(&format!("{HEADER}tool T1 Endmill(dia=10.0)\ntool T2 Endmill(dia=8.0)\ntool T4 Endmill(dia=6.0)\nfeature Top Plane(z=-0.5)\n")).unwrap()), 3);
}

/// Fork 6: the head word decides. The three vocabularies are disjoint, a
/// word in neither is refused with both lists, and `--new` prints a tool
/// statement from a kind word the way it prints a feature from a variant.
#[test]
fn test_new_mode_dispatches_on_the_head_word_and_the_vocabularies_are_disjoint() {
    for s in template::FEATURE_SPECS {
        assert!(template::TOOL_SPECS.iter().all(|t| t.kind != s.variant), "{} is in both", s.variant);
        assert_eq!(template::head_of(s.variant, "X").unwrap(), template::Head::Feature);
    }
    for t in template::TOOL_SPECS {
        assert_eq!(template::head_of(t.kind, "T3").unwrap(), template::Head::Tool);
    }
    let err = template::head_of("Helix", "X").unwrap_err();
    assert!(err.contains("CylinderBore") && err.contains("Endmill"), "both lists: {err}");

    let out = nacs(&["--new", "Endmill T3 dia=6.0"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "tool T3 Endmill(dia=6.0)\n");

    let out = nacs(&["--new", "Endmil T3 dia=6.0"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty(), "no partial artifact");
    assert!(String::from_utf8_lossy(&out.stderr).contains("not a feature variant, a tool kind or an operation"));

    // In company: T5 is plate.cam's — the parser's own loud refusal.
    let out = nacs(&["--new", "Endmill T5 dia=6.0", "plate.cam"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("tool T5 is already declared"), "{}", String::from_utf8_lossy(&out.stderr));
    let out = nacs(&["--new", "Endmill T6 dia=6.0", "plate.cam"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "tool T6 Endmill(dia=6.0)\n");
    assert!(std::fs::read_to_string("plate.cam/main.part").unwrap().lines().all(|l| !l.contains("T6")), "the project is untouched");
}

/// Fork 7, the parser rule: a `run` or `tool` line in `geometry.part` is
/// refused by name — the 2026-06 layout made a rule — while `feature`
/// lines stay legal there (plate.cam's whole reason). Every shipped
/// project parses unchanged (check_tests' corpus pins say so).
#[test]
fn test_run_and_tool_lines_belong_to_main_part() {
    let main = format!("{HEADER}tool T1 Endmill(dia=10.0)\nfeature Top Plane(z=-0.5)\nrun End(tool=T1, surface=Top, stepover=6.0)\n");
    parse::parse_project(&main, Some("feature Lid Plane(z=-1.0)\n")).expect("a feature in geometry.part is the layout");
    let err = parse::parse_project(&main, Some("# geometry\nrun End(tool=T1, surface=Top, stepover=6.0)\n")).unwrap_err();
    assert!(err.starts_with("geometry.part: line 2: a run line belongs in main.part"), "{err}");
    let err = parse::parse_project(&main, Some("tool T2 Endmill(dia=4.0)\n")).unwrap_err();
    assert!(err.starts_with("geometry.part: line 1: tool T2 belongs in main.part"), "{err}");
}

// ---- the run half: the skeleton is the form (Phase 20, slice 3) ---------
//
// No third spec table: `op_lines`' skeleton says which keys and how the
// reference is spelled, `set_key` fills a blank, and the parser judges the
// whole project with the line appended. The pins say the filled line is
// the corpus's own, the blanks are the questions, a redirect is refused,
// and a stranger reaches the parser's sentence.

fn plate() -> parse::Part {
    parse::load_project("plate.cam").unwrap_or_else(|e| panic!("load plate.cam: {e:?}"))
}

#[test]
fn test_a_filled_skeleton_is_the_corpus_own_run_line_and_the_blanks_are_the_questions() {
    let p = plate();
    // plate.cam's own first run line, byte for byte.
    assert_eq!(
        template::run_statement(&p, "End", "TopFace", &kv(&[("stepover", "6.0"), ("tool", "T5")])).unwrap(),
        "run End(tool=T5, surface=TopFace, stepover=6.0)"
    );
    let stmt = template::run_statement(&p, "Side", "Notch", &kv(&[("tool", "T5")])).unwrap();
    assert_eq!(stmt, "run Side(tool=T5, surface=Notch)");
    parse::parse_project(&format!("{}\n{stmt}\n", std::fs::read_to_string("plate.cam/main.part").unwrap()), Some(&std::fs::read_to_string("plate.cam/geometry.part").unwrap()))
        .expect("in company, the line parses and validates");

    // The blanks are the questions, named with their kinds.
    let err = template::run_statement(&p, "End", "TopFace", &kv(&[])).unwrap_err();
    assert!(err.contains("tool (a declared tool, T<n>)") && err.contains("stepover (a number)"), "{err}");
    assert!(err.contains("run End(tool=T?, surface=TopFace, stepover=?)"), "the skeleton is shown: {err}");
    assert_eq!(template::skeleton_blanks("run Form(points=[Hole], tool=T?, cycle=Simple)"), vec![("tool".to_string(), "a declared tool, T<n>")]);

    // The reference is the skeleton's: a redirect is refused.
    let err = template::run_statement(&p, "End", "TopFace", &kv(&[("tool", "T5"), ("stepover", "6.0"), ("surface", "Profile1")])).unwrap_err();
    assert!(err.contains("'surface' is the skeleton's"), "{err}");
    // A SECOND reference is a different wrong, and it has one sentence —
    // the parser's (Phase 22 slice 3, the reference rule): the wizard
    // appends it like any stranger, and the line is refused there.
    let stmt = template::run_statement(&p, "End", "TopFace", &kv(&[("tool", "T5"), ("stepover", "6.0"), ("profile", "Profile1")])).unwrap();
    assert_eq!(stmt, "run End(tool=T5, surface=TopFace, stepover=6.0, profile=Profile1)");
    let err = parse::parse_project(&format!("{}\n{stmt}\n", std::fs::read_to_string("plate.cam/main.part").unwrap()), Some(&std::fs::read_to_string("plate.cam/geometry.part").unwrap())).unwrap_err();
    assert!(err.ends_with("an End names its feature in surface=; profile= is not read"), "{err}");
    // A stranger rides to the parser: a known optional key is accepted
    // there, an unknown one refused with the parser's own sentence.
    let stmt = template::run_statement(&p, "End", "TopFace", &kv(&[("tool", "T5"), ("stepover", "6.0"), ("spindle", "2400")])).unwrap();
    assert_eq!(stmt, "run End(tool=T5, surface=TopFace, stepover=6.0, spindle=2400)");
    let stmt = template::run_statement(&p, "End", "TopFace", &kv(&[("tool", "T5"), ("stepover", "6.0"), ("stepovr", "6.0")])).unwrap();
    let err = parse::parse_project(&format!("{}\n{stmt}\n", std::fs::read_to_string("plate.cam/main.part").unwrap()), Some(&std::fs::read_to_string("plate.cam/geometry.part").unwrap())).unwrap_err();
    assert!(err.contains("stepovr"), "the parser's sentence: {err}");

    // An op the template does not offer, an op word that is not one, a
    // feature that is not declared — each refused by name.
    let err = template::run_statement(&p, "Side", "TopFace", &kv(&[("tool", "T5")])).unwrap_err();
    assert!(err.contains("not an operation the template offers for 'TopFace'") && err.contains("End"), "{err}");
    let err = template::run_statement(&p, "Bore", "TopFace", &kv(&[])).unwrap_err();
    assert!(err.contains("not an operation") && err.contains("End, Clear, Side, Copy, Form"), "{err}");
    let err = template::run_statement(&p, "End", "Nowhere", &kv(&[])).unwrap_err();
    assert!(err.contains("not declared"), "{err}");
    // A bore's seat line keeps its comment through the fill.
    assert_eq!(
        template::fill_skeleton("run Form(points=[Hole], tool=T?, cycle=Simple)  # the seat", &kv(&[("tool", "T3")])).unwrap(),
        "run Form(points=[Hole], tool=T3, cycle=Simple)  # the seat"
    );
}

/// The CLI voice: the run head needs a project (a skeleton is a feature's),
/// prints the filled line in company, and the parser refuses an
/// undeclared tool through the same re-parse.
#[test]
fn test_new_mode_run_head_needs_a_project_and_the_parser_judges_the_tool() {
    assert_eq!(template::head_of("End", "Top").unwrap(), template::Head::Run);
    // `Drill` was a tool kind AND an op word until Phase 36 slice 3
    // dissolved the op: the three vocabularies are disjoint at last.
    let shared: Vec<&str> = template::OP_WORDS.iter().copied().filter(|w| template::TOOL_SPECS.iter().any(|t| t.kind == *w)).collect();
    assert!(shared.is_empty(), "no word is both a tool kind and an op: {shared:?}");
    for w in template::OP_WORDS {
        assert!(template::FEATURE_SPECS.iter().all(|s| s.variant != w), "{w} is a feature variant too");
    }
    assert_eq!(template::head_of("Drill", "T7").unwrap(), template::Head::Tool);
    assert_eq!(template::head_of("Drill", "7").unwrap(), template::Head::Tool);
    assert_eq!(template::head_of("Drill", "Hole").unwrap(), template::Head::Tool, "a tool kind, whatever the name slot holds");
    let err = template::head_of("Wobble", "X").unwrap_err();
    assert!(err.contains("the operations: End, Clear, Side, Copy, Form"), "{err}");

    let out = nacs(&["--new", "End TopFace tool=T5 stepover=6.0"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("needs the project"), "{}", String::from_utf8_lossy(&out.stderr));

    let out = nacs(&["--new", "End TopFace tool=T5 stepover=6.0", "plate.cam"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "run End(tool=T5, surface=TopFace, stepover=6.0)\n");

    let out = nacs(&["--new", "End TopFace tool=T9 stepover=6.0", "plate.cam"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("T9"), "the parser names the undeclared tool: {}", String::from_utf8_lossy(&out.stderr));

    let out = nacs(&["--new", "End TopFace", "plate.cam"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("still needs: tool (a declared tool, T<n>), stepover (a number)"), "{}", String::from_utf8_lossy(&out.stderr));
}

// ---- Phase 21 slice 1: the mirror cannot drift ------------------------
//
// The form's inventory is the specs and the parser's whitelists, read
// and never restated — and this is the pin that keeps the two mirrors
// honest in BOTH directions: every key a spec or a skeleton offers is a
// key the parser accepts, and every key the parser accepts is one the
// window offers. A field added to the parser without its spec entry, or
// to a spec without its read, fails here by name.

/// Feature: each spec's keys equal the parser's `known` set for its
/// variant (as sets — the spec's order is the corpus's spelling order,
/// the parser's is its reading order); every variant the table names has
/// a spec, and every spec a table row.
#[test]
fn test_every_feature_spec_mirrors_the_parsers_whitelist_both_ways() {
    fn set<'a>(mut keys: Vec<&'a str>) -> Vec<&'a str> {
        keys.sort_unstable();
        keys
    }
    for spec in template::FEATURE_SPECS {
        let (known, _tolerated) = parse::feature_keys(spec.variant).unwrap_or_else(|| panic!("{}: the parser's table has no row", spec.variant));
        assert_eq!(set(spec.keys.iter().map(|k| k.name).collect()), set(known.to_vec()), "{}: spec vs parser", spec.variant);
    }
    for (variants, ..) in parse::FEATURE_KEYS {
        for v in *variants {
            assert!(template::FEATURE_SPECS.iter().any(|s| s.variant == *v), "{v}: the parser names a variant the wizard cannot offer");
        }
    }
}

/// Tool: the union of every kind's keys (own keys + the body ladder) is
/// exactly the parser's `TOOL_KEYS`.
#[test]
fn test_the_tool_specs_together_are_the_parsers_tool_whitelist() {
    let mut offered: Vec<&str> = template::TOOL_SPECS.iter().flat_map(|s| template::tool_keys(s).into_iter().map(|k| k.name)).collect();
    offered.sort_unstable();
    offered.dedup();
    let mut known = parse::TOOL_KEYS.to_vec();
    known.sort_unstable();
    assert_eq!(offered, known);
    for spec in template::TOOL_SPECS {
        for k in template::tool_keys(spec) {
            assert!(parse::TOOL_KEYS.contains(&k.name), "{}: {} is offered and never read", spec.kind, k.name);
        }
    }
}

/// Run: every key every skeleton spells, over every feature of every
/// shipped project, is in `RUN_KEYS`; the form for a run line offers
/// exactly `RUN_KEYS` — so a skeleton's question is always a field, and a
/// field is always a key the parser reads. And the corpus is the proof
/// that `required` never lies: every line the parser accepted carries
/// every key its form calls required — for the tools and the single-line
/// features too, where the spec's word is the claim.
#[test]
fn test_every_skeleton_key_is_a_run_key_and_the_form_offers_them_all() {
    let mut projects = 0;
    let mut skeletons = 0;
    for entry in std::fs::read_dir(".").unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().to_string();
        if !name.ends_with(".cam") {
            continue;
        }
        let Ok(p) = parse::load_project(&name) else { continue };
        projects += 1;
        for feature in p.features.keys() {
            for skeleton in template::op_lines(&p, feature).unwrap() {
                skeletons += 1;
                for (k, _) in nacs::view::fields_of(&skeleton) {
                    assert!(parse::RUN_KEYS.contains(&k.as_str()), "{name}/{feature}: the skeleton spells `{k}`, which the parser rejects: {skeleton}");
                }
            }
        }
        use nacs::view::{statement_form, Statement};
        let mut forms: Vec<(String, Result<nacs::view::StatementForm, String>)> = Vec::new();
        for i in 0..p.operations.len() {
            let form = statement_form(&p, &Statement::Op(i)).unwrap();
            let carries: Vec<String> = nacs::view::fields_of(&p.op_texts[i]).into_iter().map(|(k, _)| k).collect();
            let expected: Vec<&str> = parse::RUN_KEYS
                .iter()
                .copied()
                .filter(|k| !matches!(*k, "tool" | "surface" | "profile" | "points") || carries.iter().any(|c| c == k))
                .collect();
            assert_eq!(form.fields.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(), expected, "{name} op {i}: the whitelist, less the references the line does not carry");
            forms.push((format!("op {i}"), Ok(form)));
        }
        for (t, ..) in &p.tool_lines {
            forms.push((format!("T{t}"), statement_form(&p, &Statement::Tool(*t))));
        }
        for (f, ..) in &p.feature_lines {
            forms.push((f.clone(), statement_form(&p, &Statement::Feature(f.clone()))));
        }
        for (what, form) in forms {
            // A block feature declines; every other statement has a form.
            let Ok(form) = form else { continue };
            for f in form.fields.iter().filter(|f| f.required) {
                assert!(f.value.is_some(), "{name} {what}: a required `{}` the parsed line lacks — the spec's word is wrong", f.key);
            }
        }
    }
    assert!(projects > 40 && skeletons > 40, "{projects} projects, {skeletons} skeletons");
}

/// The stock (Phase 22 slice 1): the fourth arm of the mirror. The form
/// for the stock line offers exactly the parser's whitelist for its SHAPE
/// (`STOCK_KEYS`, `STOCK_CYLINDER_KEYS`), in the parser's order, on every
/// shipped project — and every project's stock
/// line carries the keys the form calls required.
#[test]
fn test_the_stock_form_mirrors_the_parsers_stock_whitelist_over_the_corpus() {
    use nacs::view::{statement_form, Statement};
    let mut projects = 0;
    for entry in std::fs::read_dir(".").unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().to_string();
        if !name.ends_with(".cam") {
            continue;
        }
        let Ok(p) = parse::load_project(&name) else { continue };
        projects += 1;
        let form = statement_form(&p, &Statement::Stock).unwrap_or_else(|e| panic!("{name}: {e}"));
        // The whitelist is the shape's (Phase 46 slice 1): a Box's seven
        // keys with three required, a Cylinder's six with two.
        let (keys, required): (&[&str], &[&str]) = match p.stock.shape {
            parse::StockShape::Box => (&parse::STOCK_KEYS, &["w", "l", "h"]),
            parse::StockShape::Cylinder { .. } => (&parse::STOCK_CYLINDER_KEYS, &["dia", "h"]),
            parse::StockShape::Profile { .. } => (&parse::STOCK_PROFILE_KEYS, &["h"]),
        };
        assert_eq!(form.fields.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(), keys.to_vec(), "{name}: the whitelist, in order");
        for f in form.fields.iter().filter(|f| f.required) {
            assert!(f.value.is_some(), "{name}: `{}` is required and the line lacks it: {}", f.key, p.stock_line.as_ref().unwrap().2);
            assert!(required.contains(&f.key.as_str()));
        }
        for (k, _) in nacs::view::fields_of(&p.stock_line.as_ref().unwrap().2) {
            assert!(keys.contains(&k.as_str()), "{name}: the line carries `{k}`, which the parser rejects");
        }
    }
    assert!(projects > 20, "the corpus: {projects}");
}
