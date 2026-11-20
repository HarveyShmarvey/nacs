// ---- the project outline's pins (DECISIONS.md "Phase 6 opens…", slice 0) ----
//
// Assertion-based, not golden-based, deliberately: `--outline` is a REPORT,
// and a report is not a program. The same call the rest-material report got —
// its truth is what it says about the project, so the tests read the claims,
// not the bytes. The goldens in this repo stay reserved for emitted G-code,
// which is the thing a machine consumes.
//
// What these pin, in order of how badly a picture would lie without them:
// (1) a Drill cycle files under every hole it names, because its
//     `feature_name` is EMPTY and its holes live in `drill_points`;
// (2) `Floor`/`Drill` read `untagged`, never `finishing`, because validate()
//     rejects them the allowance the role is derived from;
// (3) the tree's order is `run` order then sorted names, never `HashMap`
//     iteration order;
// (4) a corner rounder is named by its ground arc, never its body OD;
// (5) a roughing line carries the skin the pass RIDES beside the ceiling it
//     promises — `leaves 0.500` alone was the one user-facing summary stating
//     the guarantee as if it were the metal (DECISIONS.md → "the roughing
//     allowance has two arms…", fork 2A).

use nacs::outline;
use nacs::parse::{self, OpRole, Part};

fn load(dir: &str) -> Part {
    let main_src = std::fs::read_to_string(format!("{dir}/main.part"))
        .unwrap_or_else(|_| panic!("read {dir}/main.part"));
    let geom = std::fs::read_to_string(format!("{dir}/geometry.part")).ok();
    parse::parse_project_with_base(&main_src, geom.as_deref(), Some(std::path::Path::new(dir)))
        .unwrap_or_else(|e| panic!("parse {dir}: {e}"))
}

fn source(src: &str) -> Part {
    parse::parse_project_with_base(src, None, None).unwrap_or_else(|e| panic!("parse: {e}"))
}

/// The trap this file exists for. `run Form(points=[Hole1,Hole2], ...)` sets
/// `Operation::feature_name` to the EMPTY STRING and puts the real references
/// in `drill_points` — so a tree built from `feature_name` alone would file
/// every drill cycle under a phantom "" node and show both holes as never
/// machined. `cylinder_bore_form_points.cam` runs a Simple cycle on Hole1 and a Peck
/// cycle on both, which makes the two holes' op lists genuinely different: the
/// pin fails if the multi-point arm is ever collapsed to a single name.
#[test]
fn test_a_drill_cycle_files_under_every_hole_it_names() {
    let p = load("cylinder_bore_form_points.cam");
    let o = outline::outline(&p);

    let names: Vec<&str> = o.features.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["Hole1", "Hole2"], "both holes must appear");

    let idxs = |n: &str| -> Vec<usize> {
        o.features.iter().find(|f| f.name == n).unwrap().ops.iter().map(|op| op.idx).collect()
    };
    // Op 0 is the Simple cycle on Hole1 alone; op 1 is the Peck cycle on both.
    assert_eq!(idxs("Hole1"), vec![0, 1]);
    assert_eq!(idxs("Hole2"), vec![1]);
    assert!(o.unused.is_empty() && o.shaping.is_empty(), "every declared hole is drilled");
}

/// `validate()` rejects `stock_to_leave` on `Floor` and `Drill` outright, so
/// those ops cannot carry the role axis at all. They must read `Untagged` —
/// calling them "finishing" would have the map view color a drill cycle with a
/// role it structurally cannot hold.
#[test]
fn test_floor_and_drill_read_untagged_never_finishing() {
    let drilled = outline::outline(&load("cylinder_bore_form_points.cam"));
    for f in &drilled.features {
        for op in &f.ops {
            assert_eq!(op.role, OpRole::Untagged, "a Drill has no role axis");
        }
    }
    assert!(
        !outline::report(&load("cylinder_bore_form_points.cam")).contains("finishing"),
        "no drill cycle may be labelled finishing"
    );

    // plate.cam is End, then Clear and Side on the notch (Phase 53 slice
    // 1): the facing pass is untagged, and the two finishing passes beside
    // it prove the arm is not just "everything reads untagged".
    let plate = outline::outline(&load("plate.cam"));
    let roles: Vec<OpRole> = plate.features.iter().flat_map(|f| f.ops.iter().map(|o| o.role)).collect();
    assert_eq!(roles, vec![OpRole::Untagged, OpRole::Finishing, OpRole::Finishing]);
}

/// The role IS the allowance's presence — no `mode` field, on either side of
/// the pair. The two fixtures are the same project with and without the tag,
/// which is what makes this a pin on the derivation rather than on a fixture.
#[test]
fn test_the_role_is_the_allowances_presence() {
    let finish = outline::outline(&load("profile_pocket_clear.cam"));
    let rough = outline::outline(&load("profile_pocket_clear_rough.cam"));

    assert_eq!(finish.features[0].ops[0].role, OpRole::Finishing);
    assert_eq!(finish.features[0].ops[0].allowance, None);
    assert_eq!(rough.features[0].ops[0].role, OpRole::Roughing);
    assert_eq!(rough.features[0].ops[0].allowance, Some(0.5));

    // The number rides the line only where a role was derived from it.
    assert!(outline::report(&load("profile_pocket_clear_rough.cam")).contains("roughing  leaves 0.500"));
    assert!(!outline::report(&load("profile_pocket_clear.cam")).contains("leaves"));
}

/// Order is `run` order, never alphabetical and never `HashMap` order. The
/// source declares `Alpha` first and runs `Zeta` first, so all three orderings
/// disagree and only one of them can pass.
#[test]
fn test_the_tree_follows_run_order_not_name_order() {
    let p = source(
        r#"
units mm
feed 800
plunge 300
rapid_z 5
clearance_z 25
spindle 3000
stock Box(w=100, l=100, h=20, x=0, y=0, z=0)
tool T5 Endmill(dia=10.0)
feature Alpha CylinderBore(dia=30.0, extent=10.0, x=25.0, y=50.0)
feature Zeta CylinderBore(dia=30.0, extent=10.0, x=75.0, y=50.0)
run Clear(tool=T5, profile=Zeta, stepover=4.0)
run Clear(tool=T5, profile=Alpha, stepover=4.0)
"#,
    );
    let o = outline::outline(&p);
    let names: Vec<&str> = o.features.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["Zeta", "Alpha"], "first use down the run sequence wins");
}

/// A feature nothing cuts is usually a mistake, so the tree names it rather
/// than dropping it — in its own section, name-sorted so the section cannot
/// inherit `HashMap` order.
#[test]
fn test_declared_but_never_run_features_are_named_and_sorted() {
    let p = source(
        r#"
units mm
feed 800
plunge 300
rapid_z 5
clearance_z 25
spindle 3000
stock Box(w=100, l=100, h=20, x=0, y=0, z=0)
tool T5 Endmill(dia=10.0)
feature Cut CylinderBore(dia=30.0, extent=10.0, x=25.0, y=50.0)
feature Zeta CylinderBore(dia=10.0, extent=5.0, x=75.0, y=20.0)
feature Alpha CylinderBore(dia=10.0, extent=5.0, x=75.0, y=80.0)
run Clear(tool=T5, profile=Cut, stepover=4.0)
"#,
    );
    let o = outline::outline(&p);
    assert_eq!(o.features.len(), 1, "only the machined feature has ops");
    let unused: Vec<&str> = o.unused.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(unused, vec!["Alpha", "Zeta"], "name-sorted, not HashMap-ordered");
    assert!(o.shaping.is_empty(), "two bores beside a third shape nothing it cuts");

    let text = outline::report(&p);
    assert!(text.contains("unused (no operation names it or is shaped by it):"));
    assert!(!text.contains("indirectly used"), "an empty list has no heading");
    let tail = text.split("unused (").nth(1).unwrap();
    assert!(tail.find("Alpha").unwrap() < tail.find("Zeta").unwrap());
}

/// "Declared, never run" was three things under one heading (Phase 37 slice
/// 5; the user's observation, 2026-09-22). A feature no operation names that
/// still shapes what one cuts is USED, and says how — read from the region
/// that already names it, never derived a second time. Only what is left is
/// unused, the one list that is "usually a mistake".
#[test]
fn test_a_feature_an_op_goes_around_or_is_walled_by_is_not_unused() {
    let load = |dir: &str| nacs::parse::load_project(dir).unwrap();
    let rows = |dir: &str| outline::outline(&load(dir)).shaping.iter().map(outline::shaping_row).collect::<Vec<_>>();

    // A post in a pocket: the path goes around it.
    assert_eq!(rows("pocket_post.cam"), ["Post  CylinderBoss  island of [0]"]);
    // A fence and a bed beside a lawn: each closes an edge of the floor.
    assert_eq!(rows("lawn_fence_bed_offset.cam"), ["Bed  ProfileBoss  closes an edge of [0]", "Fence  ProfileBoss  closes an edge of [0]"]);
    // The part the observation was made on: twelve islands, every one of
    // them listed two lines under the op that said it goes around them.
    let rails = outline::outline(&load("walled_rails_offset.cam"));
    assert_eq!(rails.shaping.len(), 12);
    assert!(rails.unused.is_empty());
    assert!(rails.shaping.iter().all(|n| n.uses == [(0, outline::Shaping::Island)]));
    let text = outline::report(&load("walled_rails_offset.cam"));
    assert!(text.contains("\nindirectly used (no operation names it; it shapes what one cuts):\n  Collar  ProfileBoss  island of [0]\n"), "{text}");
    assert!(!text.contains("unused (") && !text.contains("never run:"), "{text}");
    // The count in the header is every declared feature, whichever list it is in.
    assert!(text.contains("13 feature(s), 1 operation(s), 1 tool(s)"), "{text}");

    // A feature an op NAMES stays with its ops even when it also stands in
    // another op's floor: the directly-used list is not thinned.
    let boss = outline::outline(&load("face_boss_offset.cam"));
    assert!(boss.shaping.is_empty() && boss.unused.is_empty());
    assert!(boss.features.iter().any(|f| f.name == "Post" && !f.ops.is_empty()));
}

/// A `CornerRounder`'s `dia` is its BODY OD — clearance, the one number on the
/// tool that must never position anything. The G-code header already rejects
/// to print it bare; the tree makes the same error. `edge_round.cam` grinds
/// R3 on a 12mm body with a 5.94 pilot, so a label built from the wrong field
/// is visible in the text.
#[test]
fn test_a_rounder_is_named_by_its_arc_not_its_body() {
    let text = outline::report(&load("edge_round.cam"));
    assert!(text.contains("CornerRounder R3.000"), "the ground arc names the tool");
    for line in text.lines().filter(|l| l.contains("CornerRounder")) {
        assert!(!line.contains("12.000"), "body OD must not label a rounder: {line}");
        assert!(!line.contains("5.940"), "the pilot must not label a rounder: {line}");
    }

    // The chamfer mill is the same trap on a cone: included angle, not body OD.
    let text = outline::report(&load("chamfer.cam"));
    assert!(text.contains("ChamferMill 90.000deg"));
    for line in text.lines().filter(|l| l.contains("ChamferMill")) {
        assert!(!line.contains("12.000"), "body OD must not label a chamfer mill: {line}");
    }
}

/// Fork 2A's pin (DECISIONS.md → "the roughing allowance has two arms…"): the
/// roughing line says what the pass rides, not only what it promises. The
/// flagship number is the one the thread was started over — spline_funnel_rough
/// declares 0.500 and rides 0.098, an 80% spend that no user-facing summary
/// stated anywhere. Both cusp-spending arms print it: the stepover arm's split
/// is the emitter's sweep-then-verify choice, the scallop arm's is the file's
/// own subtraction, and a reader of the tree should not need to know which arm
/// they are looking at to learn where the metal is.
#[test]
fn test_the_line_says_what_the_pass_rides_not_just_what_it_promises() {
    let text = outline::report(&load("spline_funnel_rough.cam"));
    assert!(text.contains("leaves 0.500 (rides 0.098)"), "the 80% spend must be visible: {text}");

    let text = outline::report(&load("sphere_rough.cam"));
    assert!(text.contains("leaves 0.500 (rides 0.300)"), "the scallop arm rides allowance − scallop: {text}");
}

/// A prismatic flank spends no cusp — the skin IS the allowance — so printing
/// `(rides 0.500)` beside `leaves 0.500` would be the same number twice. The
/// line stays as it was, which is also what keeps every non-cusp roughing line
/// in the corpus byte-identical through this change.
#[test]
fn test_a_flank_that_spends_no_cusp_prints_no_rides() {
    let text = outline::report(&load("profile_pocket_clear_rough.cam"));
    assert!(text.contains("roughing  leaves 0.500"));
    assert!(!text.contains("(rides"), "the skin is the allowance on a flank: {text}");
}

/// A carrier with no arm in `derive_stepover_budget` leaves the model unable
/// to know the ridden skin. The line must say so — `leaves 0.500` alone is the
/// exact half-truth this thread exists to retire, and silence here would
/// reintroduce it for precisely the carriers where the number is least
/// knowable. (No shipped fixture reaches this arm — the corpus equality pin in
/// check_tests holds declines at zero — so the case is constructed: a stepover
/// rough on a Sphere, whose budget arm does not exist yet.)
#[test]
fn test_a_declining_model_says_so_instead_of_staying_quiet() {
    let p = source(
        r#"
units mm
feed 600
plunge 200
rapid_z 5
clearance_z 30
spindle 4000
stock Box(w=100, l=80, h=25, x=0, y=0, z=0)
tool T9 Ballmill(dia=6.0)
feature TopDome Sphere(dia=40, x=50, y=40, z=0)
run Copy(tool=T9, surface=TopDome, stepover=2.0, stock_to_leave=0.5)
"#,
    );
    let text = outline::report(&p);
    assert!(text.contains("leaves 0.500 (rides unmodelled)"), "a decline must be audible: {text}");
}

/// The outline is pure `Part` — it reads no field and emits no motion, so it
/// answers on a project the voxel engine declines to model and on one whose
/// geometry only the mesh path can express.
#[test]
fn test_the_outline_answers_where_the_field_declines() {
    let text = outline::report(&load("mesh.cam"));
    assert!(text.contains("Mesh"), "the mesh feature is named by kind, not dumped");
    assert!(text.len() < 400, "a triangle list must never reach the tree: {} bytes", text.len());
}

/// Phase 11, lane B: each op's line is followed by what the stock model says
/// it removes — the terminal voice the window's removal wires are a view of.
/// A closed-form op names its sections and z spans; an op the model declines
/// says so in the consumers' words.
#[test]
fn test_the_outline_says_what_each_op_removes() {
    let text = outline::report(&load("sphere_clear.cam"));
    assert!(text.contains("      removes: annulus r 0.000..32.500 z 0.000..22.000\n"), "{text}");
    assert!(text.contains("      removes: disc r 20.000 z 0.000..22.000; annulus r 20.000..26.000 z -3.000..16.613\n"), "the dome and its swept tube: {text}");
    let rim = outline::report(&load("contour_pocket.cam"));
    assert!(rim.contains("removes: no closed form — the model declines this op"), "{rim}");
}

/// Phase 20 slice 1: the declarations have a home. Every declared tool
/// appears in NUMBER order (never `HashMap` order), each with the
/// operations that run it, and a tool no operation runs is listed as such
/// instead of being invisible — the op lines above name a tool only where
/// one is used, which is how `tool T7` had no home anywhere in the outline.
#[test]
fn test_every_declared_tool_has_a_row_in_number_order_with_the_ops_that_run_it() {
    let p = source(
        r#"
units mm
feed 800
plunge 300
rapid_z 5
clearance_z 25
spindle 3000
stock Box(w=60, l=60, h=20, x=0, y=0, z=0)
tool T7 Endmill(dia=4.0)
tool T1 Endmill(dia=10.0)
tool T2 Endmill(dia=6.0)
feature Top Plane(z=-0.5)
feature Mid Plane(z=-1.0)
feature Fin Plane(z=-2.0)
run End(tool=T1, surface=Top, stepover=6.0)
run End(tool=T2, surface=Fin, stepover=4.0)
run End(tool=T1, surface=Mid, stepover=6.0)
"#,
    );
    let o = outline::outline(&p);
    let nums: Vec<usize> = o.tools.iter().map(|t| t.num).collect();
    assert_eq!(nums, vec![1, 2, 7], "number order, not declaration or HashMap order");
    assert_eq!(o.tools[0].ops, vec![0, 2], "T1 runs ops 0 and 2, in run order");
    assert_eq!(o.tools[1].ops, vec![1]);
    assert!(o.tools[2].ops.is_empty(), "T7 runs nothing");
    assert_eq!(outline::tool_row(&o.tools[0]), "T1 Endmill dia 10.000  runs [0] [2]");
    assert_eq!(outline::tool_row(&o.tools[2]), "T7 Endmill dia 4.000  never run");

    let text = outline::report(&p);
    assert!(text.contains("3 feature(s), 3 operation(s), 3 tool(s)"), "{text}");
    let tools = text.split("\ntools:\n").nth(1).expect("a tools section");
    assert!(tools.starts_with("  T1 Endmill dia 10.000  runs [0] [2]\n  T2 Endmill dia 6.000  runs [1]\n  T7 Endmill dia 4.000  never run\n"), "{tools}");
}

/// The tool row uses the same label the op lines use — a rounder by its
/// arc, never its body OD — because both are `tool_label`'s.
#[test]
fn test_the_tool_row_names_a_rounder_by_its_arc() {
    let o = outline::outline(&load("edge_round.cam"));
    let rows: Vec<String> = o.tools.iter().map(outline::tool_row).collect();
    assert!(rows.iter().any(|r| r.contains("CornerRounder R3.000")), "{rows:?}");
    assert!(rows.iter().all(|r| !r.contains("12.000")), "body OD must not label a rounder: {rows:?}");
}

/// The Sequence (DESIGN.md §5.2, Phase 21 slice 2): the program's
/// operations in RUN order, each node the tree's own, so the two axes
/// share one formatter — and a Drill over several holes, filed under
/// each in the tree, is ONE operation here.
#[test]
fn test_the_sequence_is_every_operation_once_in_run_order() {
    let p = source(
        r#"
units mm
feed 800
plunge 300
rapid_z 5
clearance_z 25
spindle 3000
stock Box(w=100, l=100, h=20, x=0, y=0, z=0)
tool T5 Endmill(dia=10.0)
feature Top Plane(z=-0.5)
feature Mid Plane(z=-1.0)
run End(tool=T5, surface=Top, stepover=6.0)
run End(tool=T5, surface=Mid, stepover=6.0)
run End(tool=T5, surface=Top, stepover=6.0)
"#,
    );
    let o = outline::outline(&p);
    let tree: Vec<(String, Vec<usize>)> = o.features.iter().map(|f| (f.name.clone(), f.ops.iter().map(|op| op.idx).collect())).collect();
    assert_eq!(tree, vec![("Top".to_string(), vec![0, 2]), ("Mid".to_string(), vec![1])], "the tree is feature-first");
    let seq: Vec<usize> = outline::sequence(&o).iter().map(|op| op.idx).collect();
    assert_eq!(seq, vec![0, 1, 2], "the sequence is run order");
    for op in outline::sequence(&o) {
        let in_tree = o.features.iter().flat_map(|f| f.ops.iter()).find(|t| t.idx == op.idx).unwrap();
        assert_eq!(outline::op_line(&p, &op), outline::op_line(&p, in_tree), "one node, one line");
    }
    // cbore.cam: two Drills over two holes each — filed under both holes,
    // run twice, so the sequence is two rows and the tree holds four.
    let c = parse::load_project("cbore.cam").unwrap();
    let oc = outline::outline(&c);
    assert_eq!(oc.features.iter().map(|f| f.ops.len()).sum::<usize>(), 4);
    assert_eq!(outline::sequence(&oc).iter().map(|op| op.idx).collect::<Vec<_>>(), vec![0, 1]);
}

/// Phase 45 slice 2: a bore's `removes:` range is derived from the removal,
/// so a rim below the stock top prints as the true heights — the mouth at
/// `z`, the floor one extent under it — with no second reading of the line.
#[test]
fn test_sunken_bore_removes_range_reads_the_rim() {
    let p = source(
        r#"
units mm
feed 800
plunge 300
rapid_z 5
clearance_z 25
spindle 3000
stock Box(w=100, l=100, h=20, x=0, y=0, z=0)
tool T5 Endmill(dia=10.0)
tool T7 Drill(dia=8.0)
feature Step Plane(z=-8.0)
feature Bore CylinderBore(dia=30.0, extent=6.0, x=50.0, y=50.0, z=-8.0)
feature Hole CylinderBore(dia=8.0, extent=8.0, x=20.0, y=20.0, z=-8.0)
run End(tool=T5, surface=Step, stepover=4.0)
run Clear(tool=T5, profile=Bore, stepover=4.0)
run Form(points=[Hole], tool=T7, cycle=Peck, peck_increment=3.0)
"#,
    );
    let text = outline::render(&p, &outline::outline(&p));
    assert!(text.contains("removes: disc r 15.000 z -14.000..-8.000\n"), "the bore from its rim: {text}");
    assert!(text.contains("removes: disc r 4.000 z -16.000..-8.000\n"), "the hole from its rim: {text}");
}
