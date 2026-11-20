// ---- The wall trace follows the wall (Phase 60 slice 1) ---------------------
// `DECISIONS.md` → "Phase 60 slice 1: the wall trace follows the wall…". A
// `Side` on a prismatic pocket whose loop runs on or past the blank used to
// trace the whole loop, half of the plate's in air with the controller's comp
// holding the tool a radius further out. It now asks `region::wall_edges` —
// the same judgement `region::of` has made of a floor's edges since Phase 27,
// lifted into `region::judge_edges` — and traces the WALL CHAINS only, each
// entered one diameter before its start along its own line and left one
// diameter past its end, the program saying `(wall k of K: …)` per chain.

use nacs::region::{self, EdgeKind};
use nacs::{gcode, parse};
use std::fs;

fn project(dir: &str) -> parse::Part {
    let main = fs::read_to_string(format!("{dir}/main.part")).unwrap();
    let geom = fs::read_to_string(format!("{dir}/geometry.part")).ok();
    parse::parse_project(&main, geom.as_deref()).unwrap_or_else(|e| panic!("{dir} parses: {e}"))
}

fn op_block<'a>(g: &'a str, header: &str) -> &'a str {
    g.split(header).nth(1).unwrap_or_else(|| panic!("no op {header}")).split("(---").next().unwrap()
}

fn side_of<'a>(p: &'a parse::Part, feature: &str) -> &'a parse::Operation {
    p.operations
        .iter()
        .find(|o| o.op_type == parse::OpType::Side && o.feature_name == feature)
        .unwrap_or_else(|| panic!("a Side on {feature}"))
}

/// The X and Y words of a move line, when it carries both.
fn xy(line: &str) -> Option<(f64, f64)> {
    let word = |tag: char| line.split_whitespace().find(|w| w.starts_with(tag))?.get(1..)?.parse::<f64>().ok();
    Some((word('X')?, word('Y')?))
}

#[test]
fn test_the_notch_is_two_walls_and_two_open_edges_to_the_wall_trace() {
    // The plate's notch is drawn 5 mm past the blank on its west and north
    // sides; clipped to the blank it is the 60 × 40 corner, and the edges on
    // x = 0 and y = 80 are the blank's — open. The two walls are the L's
    // inner edges: y = 40 from x = 0 to 60, and x = 60 from y = 40 to 80.
    let p = project("plate.cam");
    let (outer, kinds) = region::wall_edges(&p, side_of(&p, "Notch")).expect("a pocket has wall edges");
    assert_eq!(outer.len(), 4, "the clipped notch is a rectangle: {outer:?}");
    assert_eq!(kinds.len(), 4);
    for i in 0..4 {
        let (a, b) = (outer[i], outer[(i + 1) % 4]);
        let on_south_wall = (a.y - 40.0).abs() < 1e-6 && (b.y - 40.0).abs() < 1e-6;
        let on_east_wall = (a.x - 60.0).abs() < 1e-6 && (b.x - 60.0).abs() < 1e-6;
        let wall = kinds[i] == EdgeKind::Wall;
        assert_eq!(wall, on_south_wall || on_east_wall, "edge {i} from {a:?} to {b:?} is {}", if wall { "a wall" } else { "open" });
    }
    assert_eq!(kinds.iter().filter(|k| **k == EdgeKind::Wall).count(), 2);
}

#[test]
fn test_the_plates_side_traces_its_two_walls_in_the_loops_direction() {
    // One chain, two edges, in the loop's own CCW direction: in from the west
    // along the south wall, up the east wall, out past the north edge. Under
    // control comp the text is the edge itself; the controller holds the tool
    // a radius to the left, which is the notch's side.
    let p = project("plate.cam");
    let g = gcode::program(&p).expect("plate generates");
    let b = op_block(&g, "(--- side contour: Notch with T5 ---)");
    assert!(b.contains("(wall 1 of 1: 2 edges, open at both ends)"), "{b}");
    let cut: Vec<&str> = b.lines().filter(|l| l.starts_with("G01 G41") || l.starts_with("G01 X") || l.starts_with("G01 G40")).collect();
    assert_eq!(
        cut,
        ["G01 G41 D5 X0.000 Y40.000 F800.000", "G01 X60.000 Y40.000", "G01 X60.000 Y80.000", "G01 G40 X55.000 Y90.000"],
        "{b}"
    );
    // The lead stands one diameter west of the blank on the tool-centre line
    // of the south wall (y = 40 + r); the lead-out one diameter north of it on
    // the east wall's (x = 60 − r).
    assert!(b.contains("G00 X-10.000 Y45.000"), "{b}");
    assert!(b.contains("(lead: 10.000mm along the path's own line, back from its start, in air)"), "a lead in air is said: {b}");
}

#[test]
fn test_the_plates_side_touches_no_point_outside_the_blank_but_its_two_leads() {
    // Every X/Y the Side names lies in the 120 × 80 blank except the lead and
    // the lead-out — which lie past its west and north edges, in air by
    // construction, because a chain's ends sit on open edges.
    let p = project("plate.cam");
    let g = gcode::program(&p).expect("plate generates");
    let b = op_block(&g, "(--- side contour: Notch with T5 ---)");
    let points: Vec<(f64, f64)> = b.lines().filter(|l| l.starts_with('G')).filter_map(xy).collect();
    assert!(points.len() >= 5, "{b}");
    let inside = |&(x, y): &(f64, f64)| (0.0..=120.0).contains(&x) && (0.0..=80.0).contains(&y);
    let outside: Vec<&(f64, f64)> = points.iter().filter(|q| !inside(q)).collect();
    assert_eq!(outside, [&(-10.0, 45.0), &(55.0, 90.0)], "only the two leads stand outside the blank: {b}");
}

#[test]
fn test_the_engine_arm_rounds_the_chains_inside_corner_and_runs_its_parallel_to_the_blank() {
    // `profile_pocket_arc.cam`: the original L drawn as a void, four of its six
    // edges ON the blank's edges. The engine arm offsets the chain itself: the
    // inside corner at (60, 40) is still one G02 fillet of the tool's radius,
    // and the parallel now runs to the blank's west edge (x = 0) where it used
    // to stop a radius short of a "wall" that was air.
    let p = project("profile_pocket_arc.cam");
    let g = gcode::program(&p).expect("profile_pocket_arc generates");
    let b = op_block(&g, "(--- side contour (engine): Lprofile with T5 ---)");
    assert!(b.contains("(wall 1 of 1: 2 edges, open at both ends)"), "{b}");
    let cut: Vec<&str> = b.lines().filter(|l| l.starts_with("G01 X") || l.starts_with("G02") || l.starts_with("G03")).collect();
    assert_eq!(
        cut,
        [
            "G01 X65.000 Y80.000 F800.000",
            "G01 X65.000 Y40.000",
            "G02 X60.000 Y35.000 I-5.000 J0.000 F800.000",
            "G01 X0.000 Y35.000",
            "G01 X-10.000 Y35.000",
        ],
        "{b}"
    );
    assert!(b.contains("G00 X65.000 Y90.000"), "{b}");
}

#[test]
fn test_a_closed_pockets_side_reads_no_open_edge_and_traces_the_whole_loop() {
    // A pocket wholly inside its blank has no open edge: `wall_edges` says so
    // and the trace is the closed loop it always was, with no chain note.
    let p = project("pocket_islands.cam");
    let (outer, kinds) = region::wall_edges(&p, side_of(&p, "Cavity")).expect("a pocket has wall edges");
    assert_eq!(outer.len(), 4);
    assert!(kinds.iter().all(|k| *k == EdgeKind::Wall), "every edge of a closed pocket is a wall");
    let g = gcode::program(&p).expect("pocket_islands generates");
    let b = op_block(&g, "(--- side contour (engine): Cavity with T5 ---)");
    assert!(!b.contains("(wall "), "{b}");
    assert!(b.lines().filter(|l| l.starts_with("G01 X")).count() >= 5, "the closed loop, every edge: {b}");
}

#[test]
fn test_a_loop_with_no_wall_is_refused_by_name() {
    // The blank itself drawn as a pocket: every edge on the blank, nothing to
    // trace. An op that cannot be machined as declared is a hard error.
    let src = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=120, l=80, h=20, x=0, y=0, z=0)\ntool T5 Endmill(dia=10.0)\n\
feature Slab ProfilePocket(extent=2\n    0 0\n    120 0\n    120 80\n    0 80\n)\n\
run Side(tool=T5, surface=Slab, comp=control)\n";
    let p = parse::parse(src).expect("the slab parses");
    let err = gcode::program(&p).expect_err("no wall to trace");
    assert!(err.contains("Side on 'Slab' with T5"), "{err}");
    assert!(err.contains("there is no wall to trace"), "{err}");
}

// ---- Slice 2: the boss arm, and the three readings of one L ------------------
// `DECISIONS.md` → "Phase 60 slice 2: the boss arm…". A `ProfileBoss` is CW
// with the part inside, and nothing stands beyond its edge but the floor it
// stands on, so the blank rule alone judges it: on or past the blank, open.
// `plate_boss.cam` is plate.cam's L drawn as the boss it is, on the plate's
// own blank; `plate_floated.cam` is the same L floated in a larger blank — the
// control, six walls, untouched by this phase.

/// A fixture's Side block without its header line, for comparing two.
fn side_moves(g: &str, header: &str) -> Vec<String> {
    op_block(g, header).lines().filter(|l| !l.is_empty()).map(str::to_string).collect()
}

#[test]
fn test_the_l_as_a_boss_on_its_own_blank_is_four_open_edges_and_two_walls() {
    let p = project("plate_boss.cam");
    let (outer, kinds) = region::wall_edges(&p, side_of(&p, "Plate")).expect("a boss has wall edges");
    assert_eq!(outer.len(), 6, "the L as authored, on the blank: {outer:?}");
    assert!(parse::signed_area(&outer) < 0.0, "a boss's loop is held CW: {outer:?}");
    for i in 0..6 {
        let (a, b) = (outer[i], outer[(i + 1) % 6]);
        let on_south_wall = (a.y - 40.0).abs() < 1e-6 && (b.y - 40.0).abs() < 1e-6;
        let on_east_wall = (a.x - 60.0).abs() < 1e-6 && (b.x - 60.0).abs() < 1e-6;
        let wall = kinds[i] == EdgeKind::Wall;
        assert_eq!(wall, on_south_wall || on_east_wall, "edge {i} from {a:?} to {b:?} is {}", if wall { "a wall" } else { "open" });
    }
    assert_eq!(kinds.iter().filter(|k| **k == EdgeKind::Open).count(), 4, "the four edges on the blank's faces");
}

#[test]
fn test_a_boss_side_and_a_pocket_side_cut_the_same_two_walls() {
    // The user's reading, measured: "once the notch is gone, the profile
    // responsibility drops to the full part". The Notch pocket (CCW) and the
    // Plate boss (CW) name the same two walls, and their Sides are the same
    // moves line for line — the lead, the comp-on vertex, the two edges in the
    // same direction, the lead-out — because the kinds are read off the blank.
    let plate = gcode::program(&project("plate.cam")).expect("plate generates");
    let boss = gcode::program(&project("plate_boss.cam")).expect("plate_boss generates");
    let a = side_moves(&plate, "(--- side contour: Notch with T5 ---)");
    let b = side_moves(&boss, "(--- side contour: Plate with T5 ---)");
    assert!(a.iter().any(|l| l.starts_with("G01 G41")), "{a:?}");
    assert_eq!(a, b);
}

#[test]
fn test_the_floated_plate_is_six_walls_and_the_closed_trace() {
    // No edge on the blank: every edge a wall, no chain, the loop traced whole
    // and closed as a boss's Side always was — the control.
    let p = project("plate_floated.cam");
    let (outer, kinds) = region::wall_edges(&p, side_of(&p, "Plate")).expect("a boss has wall edges");
    assert_eq!(outer.len(), 6);
    assert!(kinds.iter().all(|k| *k == EdgeKind::Wall), "six walls: {kinds:?}");
    let g = gcode::program(&p).expect("plate_floated generates");
    let b = op_block(&g, "(--- side contour: Plate with T5 ---)");
    assert!(!b.contains("(wall "), "{b}");
    assert_eq!(b.lines().filter(|l| l.starts_with("G01 X")).count(), 6, "six edges and the close: {b}");
    assert!(b.contains("G01 G41 D5 X10.000 Y10.000 F800.000") && b.contains("G01 X10.000 Y10.000
"), "{b}");
}

#[test]
fn test_a_boss_drawn_past_the_blank_still_traces_its_two_walls() {
    // The L's four outer edges drawn 5 mm past the blank: the containment lint
    // has its say about a boss outside the stock (an advisory, not this
    // test's business), and the wall trace clips the loop to the blank, holds
    // it CW and traces the same two walls.
    let src = fs::read_to_string("plate_boss.cam/main.part")
        .unwrap()
        .replace("    0 0
    0 40
", "    -5 -5
    -5 40
")
        .replace("    60 80
    120 80
    120 0
", "    60 85
    125 85
    125 -5
");
    let p = parse::parse(&src).expect("parses");
    let g = gcode::program(&p).expect("generates");
    let b = op_block(&g, "(--- side contour: Plate with T5 ---)");
    let cut: Vec<&str> = b.lines().filter(|l| l.starts_with("G01 G41") || l.starts_with("G01 X") || l.starts_with("G01 G40")).collect();
    assert_eq!(cut, ["G01 G41 D5 X0.000 Y40.000 F800.000", "G01 X60.000 Y40.000", "G01 X60.000 Y80.000", "G01 G40 X55.000 Y90.000"], "{b}");
}

#[test]
fn test_plate_boss_golden() {
    let g = gcode::program(&project("plate_boss.cam")).expect("plate_boss generates");
    let golden = fs::read_to_string("tests/plate_boss.golden.gcode").expect("plate_boss golden");
    assert_eq!(g.trim(), golden.trim());
}

#[test]
fn test_plate_floated_golden() {
    let g = gcode::program(&project("plate_floated.cam")).expect("plate_floated generates");
    let golden = fs::read_to_string("tests/plate_floated.golden.gcode").expect("plate_floated golden");
    assert_eq!(g.trim(), golden.trim());
}

// ---- The fix the pair found: an End around a bitten boss ----------------------
// `plate_boss.cam`'s End gouged the plate when first generated: a Plane's
// family boundary was the blank grown a diameter whatever the region said,
// and a boss bitten out of the floor (Phase 49) has flanks the region calls
// WALLS — so the rows ran a radius into the L along its east wall, 6 300 mm³
// of declared part swept (the rest report's `violated`). `ops::clear::
// family_boundary` now reads the region's own family wall for a Plane that
// carries a wall; the corpus's vox sweep pins it on every fixture.

#[test]
fn test_the_end_around_a_bitten_boss_stops_a_radius_short_of_its_flank() {
    // The pair's floor op is a Clear since Phase 61; the End's gouge is pinned
    // on the same text with the End put back.
    let src = fs::read_to_string("plate_boss.cam/main.part")
        .unwrap()
        .replace("run Clear(tool=T5, profile=Floor, stepover=4.0, stepdown=5)", "run End(tool=T5, surface=Floor, stepover=4.0)");
    assert!(src.contains("run End("), "the replacement took");
    let p = parse::parse_project(&src, None).expect("plate_boss with its End parses");
    let g = gcode::program(&p).expect("plate_boss with its End generates");
    let b = op_block(&g, "(--- floor: Floor with T5 ---)");
    assert!(b.contains("(on the shore: Plate)"), "{b}");
    let points: Vec<(f64, f64)> = b.lines().filter(|l| l.starts_with("G01")).filter_map(xy).collect();
    assert!(points.iter().any(|&(x, _)| (x - 55.0).abs() < 1e-6), "the rows reach the east wall's offset: {b}");
    for &(x, y) in &points {
        assert!(!(x > 55.0 + 1e-6 && y > 40.0 - 1e-6), "a feed at ({x}, {y}) stands inside the L's east arm: {b}");
        assert!(!(y < 45.0 - 1e-6), "a feed at ({x}, {y}) stands inside the L's south arm: {b}");
    }
}
