// ---- The lead reads the room (Phase 47 slice 2, fork 1A) ----------------------
// `DECISIONS.md` → "Phase 47 slice 2: the lead reads the room…". A wall
// trace's lead point used to be one diameter off the path along the ray from
// the loop's vertex-average centroid, whatever stood there. It is now one
// diameter along the path's void-side normal at its first vertex where the
// stock model says a tool standing there is in air, the longest shorter
// back-off that is otherwise, down to a plunge on the path — and the program
// says which, when it is not the diameter asked. `disc_ribs.cam` is the
// fixture: two L-bosses facing each other across a slot 1.1 tool diameters
// wide, each listed so the slot wall is the first edge of the loop its
// `Side` drives.

use nacs::{gcode, parse};
use std::fs;

fn disc_ribs() -> parse::Part {
    let main = fs::read_to_string("disc_ribs.cam/main.part").unwrap();
    let geom = fs::read_to_string("disc_ribs.cam/geometry.part").unwrap();
    parse::parse_project(&main, Some(&geom)).expect("disc_ribs.cam parses")
}

fn op_block<'a>(g: &'a str, header: &str) -> &'a str {
    g.split(header).nth(1).unwrap_or_else(|| panic!("no op {header}")).split("(---").next().unwrap()
}

fn room_said(block: &str) -> Option<f64> {
    let line = block.lines().find(|l| l.starts_with("(lead: "))?;
    let n = line.trim_start_matches("(lead: ").split("mm").next()?;
    n.parse().ok()
}

#[test]
fn test_the_l_bosses_lead_into_the_slots_one_millimetre_of_room_and_say_so() {
    let p = disc_ribs();
    let g = gcode::program(&p).expect("disc_ribs generates");

    // LW's loop starts at the foot of the slot wall, (-0.5, -20) — the wall
    // x = -5.5 plus a radius — and heads up it; its void-side normal is +X,
    // into the slot. A tool standing a diameter out would be 9 mm into LE.
    // The far wall is at x = 5.5, the body reaches 5: one millimetre of room
    // to the chord tolerance (the model's hole round LE is the declared
    // outline to that tolerance), printed, and the plunge is there.
    let lw = op_block(&g, "(--- side contour (engine): LW with T5 ---)");
    let said = room_said(lw).expect("LW's lead says its room");
    assert!((said - 1.0).abs() <= nacs::CHORD_TOL_MM, "LW's room is one millimetre to the chord tolerance: {said} — {lw}");
    assert!(lw.contains(&format!("G00 X{:.3} Y-20.000", said - 0.5)), "the plunge stands one millimetre into the slot: {lw}");
    assert!(lw.contains("G01 X-0.500 Y-20.000 F800.000"), "and leads in to the path's first vertex: {lw}");

    // LE mirrors it from the slot's other end, heading down. Its far wall is
    // LW, and the model's hole around LW is the declared outline to within
    // the chord tolerance (the Level's claimed removal is what the lead
    // reads): the room it says is one millimetre to that tolerance, never
    // more than what the model holds open.
    let le = op_block(&g, "(--- side contour (engine): LE with T5 ---)");
    let said = room_said(le).expect("LE's lead says its room");
    assert!((said - 1.0).abs() <= nacs::CHORD_TOL_MM, "LE's room is one millimetre to the chord tolerance: {said} — {le}");
    let x = le.lines().find(|l| l.starts_with("G00 X")).unwrap();
    assert_eq!(x, format!("G00 X{:.3} Y20.000", 0.5 - said), "the plunge is where the room says: {le}");

    // The rim pads have a whole diameter of air off their first vertex and
    // say so: every descent is said, and a lead's sentence carries its room
    // when it is short of the diameter asked.
    for pad in ["RimN", "RimS"] {
        let b = op_block(&g, &format!("(--- side contour (engine): {pad} with T5 ---)"));
        // (The pads are traced along their wall chains, so the lead runs
        // back along the chain's own line.)
        assert!(b.contains("(lead: 10.000mm along the path's own line, back from its start, in air)"), "{pad} leads a whole diameter and says so: {b}");
    }
}

#[test]
fn test_a_slot_one_diameter_wide_plunges_on_the_path_by_name() {
    // Bring LE's slot wall in to x = 4.5: the slot is exactly one diameter
    // wide, a tool standing on either path point touches both walls, and
    // there is no room at all. The lead says so and plunges ON the path —
    // which the entry restriction accepts (contact is not a gouge).
    let main = fs::read_to_string("disc_ribs.cam/main.part").unwrap().replace("    5.500 -20.000\n    5.500 20.000\n", "    4.500 -20.000\n    4.500 20.000\n");
    assert!(main.contains("4.500 20.000"), "the fixture's LE wall moved");
    let geom = fs::read_to_string("disc_ribs.cam/geometry.part").unwrap();
    let p = parse::parse_project(&main, Some(&geom)).expect("parses");
    let g = gcode::program(&p).expect("a slot one diameter wide is cut wall by wall, each plunge on its path");
    let lw = op_block(&g, "(--- side contour (engine): LW with T5 ---)");
    assert!(lw.contains("(plunge: on the path — 0.000mm of room along the path's normal)"), "{lw}");
    assert!(lw.contains("G00 X-0.500 Y-20.000"), "the plunge IS the path's first vertex: {lw}");
    assert!(lw.contains("G01 X-0.500 Y-20.000 F800.000"), "and the lead-in is the zero-length move to it: {lw}");
}

#[test]
fn test_a_pockets_lead_rides_off_the_compensated_start_under_control_comp() {
    // The plain window, control comp: the parallel's first vertex is a radius
    // along BOTH edges from the authored corner (25, 25), and the lead is a
    // diameter along the corner's bisector from there, (32.071, 32.071) — in
    // the window's own declared void, a whole diameter, in silence, where it
    // rode a diameter along the same diagonal from the AUTHORED corner before
    // this slice, (27.071, 27.071). Two things this slice tried and the entry
    // taxonomy refused on the way: a radius along the first edge alone as the
    // start, (20, 25), half a tool in the left wall; and the first edge's own
    // normal as the direction, (25, 35), the body rubbing the left wall down
    // the whole plunge.
    let src = fs::read_to_string("profile_pocket.cam/main.part").unwrap();
    let ctl = src.replace("run Side(tool=T5, surface=Window)", "run Side(tool=T5, surface=Window, comp=control)");
    let p = parse::parse(&ctl).expect("parses");
    let g = gcode::program(&p).expect("generates");
    let b = op_block(&g, "(--- side contour: Window with T5 ---)");
    assert!(b.contains("G00 X32.071 Y32.071"), "{b}");
    assert!(b.contains("G01 G41 D5 X20.000 Y20.000 F800.000"), "the comp-on move runs from the lead to the authored corner: {b}");
    assert!(b.contains("G01 G40 X32.071 Y32.071"), "and the lead-out returns to it: {b}");
    assert!(b.contains("(lead: 10.000mm along the path's normal, in air)"), "{b}");
}
