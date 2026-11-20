// ---- Clear takes a Plane and a Face (Phase 61 slice 1) ----------------------
// `DECISIONS.md` → "Phase 61 slice 1: a Clear takes a Plane and a Face…". A
// `Plane` is the floor whose wall is the stock, and until this phase it took
// an End and not a Clear, so the volume above a floor with a part standing in
// it was authored as a pocket nobody meant or as the moat. The Clear is the
// VOLUME verb: it roughs everything above the floor around whatever stands in
// it (an island, derived from containment; a boss on the shore a bite out of
// the floor), the one allowance on every flank, and nothing on the floor
// unless an End on the same feature follows. One level until slice 2.

use nacs::{gcode, parse};
use std::fs;

fn source(dir: &str) -> String {
    fs::read_to_string(format!("{dir}/main.part")).unwrap()
}

fn generate(src: &str) -> Result<String, String> {
    let p = parse::parse_project(src, None).map_err(|e| format!("parse: {e}"))?;
    gcode::program(&p)
}

fn op_block<'a>(g: &'a str, header: &str) -> &'a str {
    g.split(header).nth(1).unwrap_or_else(|| panic!("no op {header}")).split("(---").next().unwrap()
}

fn word(line: &str, tag: char) -> Option<f64> {
    line.split_whitespace().find(|w| w.starts_with(tag))?.get(1..)?.parse::<f64>().ok()
}

/// Every feed move's (x, y, z) in a block, the words it does not carry held.
fn feeds(block: &str) -> Vec<(f64, f64, f64)> {
    let (mut x, mut y, mut z) = (0.0, 0.0, 0.0);
    let mut out = Vec::new();
    for l in block.lines() {
        if !(l.starts_with("G01") || l.starts_with("G00") || l.starts_with("G02") || l.starts_with("G03")) {
            continue;
        }
        x = word(l, 'X').unwrap_or(x);
        y = word(l, 'Y').unwrap_or(y);
        z = word(l, 'Z').unwrap_or(z);
        if l.starts_with("G01") || l.starts_with("G02") || l.starts_with("G03") {
            out.push((x, y, z));
        }
    }
    out
}

fn dist_to_polygon(poly: &[(f64, f64)], x: f64, y: f64) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let t = (((x - a.0) * dx + (y - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
            (x - (a.0 + t * dx)).hypot(y - (a.1 + t * dy))
        })
        .fold(f64::INFINITY, f64::min)
}

/// Even-odd point in polygon.
fn inside(poly: &[(f64, f64)], x: f64, y: f64) -> bool {
    let n = poly.len();
    let mut c = false;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.1 > y) != (b.1 > y) && x < (b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0 {
            c = !c;
        }
    }
    c
}

const THE_L: [(f64, f64); 6] = [(10.0, 10.0), (10.0, 50.0), (70.0, 50.0), (70.0, 90.0), (130.0, 90.0), (130.0, 10.0)];

#[test]
fn test_a_clear_on_a_plane_goes_around_the_boss_in_one_level() {
    // No `stepdown`: one level at the floor, as slice 1 cut it.
    let src = source("plate_floated.cam").replace(", stepdown=5", "");
    assert!(!src.contains("stepdown="), "the word is off");
    let g = generate(&src).expect("the Clear on the Plane generates");
    let b = op_block(&g, "(--- volume clear: Floor with T5 ---)");
    assert!(b.contains("(islands: Plate)"), "{b}");
    let zs: std::collections::BTreeSet<i64> = feeds(b).iter().map(|f| (f.2 * 1000.0).round() as i64).filter(|z| *z <= -19_900).collect();
    assert_eq!(zs.into_iter().collect::<Vec<_>>(), vec![-20_000], "one level at the floor: {b}");
    // The boss is never entered: every feed stands a radius off the L.
    for (x, y, _) in feeds(b) {
        let inside = (10.0..=130.0).contains(&x) && (10.0..=50.0).contains(&y) || (70.0..=130.0).contains(&x) && (50.0..=90.0).contains(&y);
        assert!(!inside || dist_to_polygon(&THE_L, x, y) >= 5.0 - 1e-6, "a feed at ({x}, {y}) stands inside the L: {b}");
    }
}

#[test]
fn test_the_allowance_stands_on_the_flank_and_not_on_the_open_edge() {
    // The Clear's one number is a radial allowance: 0.5 on every flank of the
    // L (the Side that follows takes it), and NOTHING on the stock's edge —
    // an open edge has no wall to protect, and the family used to stop the
    // allowance short of the face's own edge on it.
    let src = source("plate_floated.cam").replace("stepdown=5)\nrun Side", "stepdown=5, stock_to_leave=0.5)\nrun Side");
    assert!(src.contains("stock_to_leave=0.5"), "the replacement took");
    let g = generate(&src).expect("generates");
    let b = op_block(&g, "(--- volume clear: Floor with T5");
    let pts = feeds(b);
    let nearest = pts.iter().map(|&(x, y, _)| dist_to_polygon(&THE_L, x, y)).fold(f64::INFINITY, f64::min);
    assert!((nearest - 5.5).abs() < 1e-3, "the centre stands r + skin off the L's flank, not {nearest}");
    let (min_x, max_x) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |a, p| (a.0.min(p.0), a.1.max(p.0)));
    let (min_y, max_y) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |a, p| (a.0.min(p.1), a.1.max(p.1)));
    assert!((min_x + 5.0).abs() < 1e-3 && (max_x - 145.0).abs() < 1e-3, "x spans a radius past the blank: {min_x}..{max_x}");
    assert!((min_y + 5.0).abs() < 1e-3 && (max_y - 105.0).abs() < 1e-3, "y spans a radius past the blank: {min_y}..{max_y}");
}

#[test]
fn test_a_clear_around_a_boss_bitten_out_of_the_floor_stops_a_radius_short_of_its_flank() {
    let g = generate(&source("plate_boss.cam")).expect("the Clear on the Plane generates");
    let b = op_block(&g, "(--- volume clear: Floor with T5 ---)");
    assert!(b.contains("(on the shore: Plate)"), "{b}");
    // The tool's centre is never inside the L, nor nearer than a radius to its
    // flank: the notch's corner at (0, 40) is touched at a point and no more.
    let l = [(0.0, 0.0), (0.0, 40.0), (60.0, 40.0), (60.0, 80.0), (120.0, 80.0), (120.0, 0.0)];
    for (x, y, _) in feeds(b) {
        assert!(!inside(&l, x, y) && dist_to_polygon(&l, x, y) >= 5.0 - nacs::CHORD_TOL_MM, "a feed at ({x}, {y}) gouges the L: {b}");
    }
}

#[test]
fn test_a_face_is_cleared_with_the_floors_allowance_left_for_the_end_that_follows() {
    let g = generate(&source("face_clear.cam")).expect("generates");
    let clear = op_block(&g, "(--- volume clear: Floor with T5");
    assert!(clear.contains("(islands: Post)"), "{clear}");
    assert!(clear.contains("(floor: 0.500 left for the End op that follows)"), "{clear}");
    // …in two levels (slice 2): equal bites of 2.75 down to the skinned floor.
    assert!(clear.contains("(level z-2.750") && clear.contains("(level z-5.500"), "{clear}");
    let deepest = |b: &str| feeds(b).iter().map(|f| f.2).fold(f64::INFINITY, f64::min);
    assert!((deepest(clear) + 5.5).abs() < 1e-6, "the Clear stops 0.5 above the floor: {clear}");
    let end = op_block(&g, "(--- floor: Floor with T5 ---)");
    assert!((deepest(end) + 6.0).abs() < 1e-6, "the End takes the floor: {end}");
}

#[test]
fn test_without_an_end_to_take_it_the_clear_cuts_to_the_floor() {
    // Nothing declares a floor that serves the part, so nothing is left on one.
    let src = source("face_clear.cam").replace("run End(tool=T5, surface=Floor, stepover=4.0)\n", "").replace(", stepdown=3", "");
    assert!(!src.contains("run End"), "the End is gone");
    let g = generate(&src).expect("generates");
    let clear = op_block(&g, "(--- volume clear: Floor with T5");
    assert!(!clear.contains("(floor:"), "{clear}");
    let deepest = feeds(clear).iter().map(|f| f.2).fold(f64::INFINITY, f64::min);
    assert!((deepest + 6.0).abs() < 1e-6, "the Clear cuts to the floor's z: {clear}");
}

#[test]
fn test_a_plane_clear_walks_a_pinned_pattern_around_the_boss() {
    let src = source("plate_floated.cam").replace("stepdown=5)\nrun Side", "stepdown=5, pattern=zigzag)\nrun Side");
    let g = generate(&src).expect("a pattern on a Plane Clear is legal");
    let b = op_block(&g, "(--- volume clear: Floor with T5");
    assert!(b.contains("(islands: Plate)"), "{b}");
    for (x, y, _) in feeds(b) {
        let inside = (10.0..=130.0).contains(&x) && (10.0..=50.0).contains(&y) || (70.0..=130.0).contains(&x) && (50.0..=90.0).contains(&y);
        assert!(!inside || dist_to_polygon(&THE_L, x, y) >= 5.0 - 1e-6, "a zigzag feed at ({x}, {y}) stands inside the L");
    }
}

#[test]
fn test_a_plane_clear_refuses_a_strategy_by_name() {
    let src = source("plate_floated.cam").replace("stepdown=5)\nrun Side", "strategy=rest, voxel_cell=2.0)\nrun Side");
    let err = generate(&src).expect_err("a floor's volume has no declared pocket for a field to select inside");
    assert!(err.contains("Clear operation feature 'Floor' is a Plane") && err.contains("strategy"), "{err}");
}

#[test]
fn test_face_clear_golden() {
    let g = generate(&source("face_clear.cam")).expect("generates");
    let golden = fs::read_to_string("tests/face_clear.golden.gcode").expect("face_clear golden");
    assert_eq!(g.trim(), golden.trim(), "face_clear.cam drifted from its golden");
}

// ---- stepdown is every Clear's word (Phase 61 slice 2) -----------------------
// `DECISIONS.md` → "Phase 61 slice 2…". The word was the moat's alone; a
// pocket took its whole depth in one pass and refused it by name. Now the
// volume verb cuts any host in equal bites from the top down to the floor,
// `stepdown` a maximum, each level the family again; absent, one level.

#[test]
fn test_a_pocket_clear_cuts_equal_bites_and_each_level_ramps_from_the_one_above() {
    let g = generate(&source("pocket_levels.cam")).expect("a pocket Clear takes stepdown");
    let clear = g.split("(--- floor:").next().unwrap();
    for z in ["-4.000", "-8.000", "-12.000"] {
        assert!(clear.contains(&format!("(level z{z} — 4 passes)")), "twelve at five is three bites of four: {clear}");
    }
    // Every feed of a level stays inside its band: the first level from the
    // rim, the second from the first level's floor, and nothing is deeper
    // than the pocket.
    let bands = [(0.0, -4.0), (-4.0, -8.0), (-8.0, -12.0)];
    for (k, block) in clear.split("(level z").skip(1).enumerate() {
        let (hi, lo) = bands[k];
        for (_, _, z) in feeds(block) {
            assert!(z <= hi + 1e-6 && z >= lo - 1e-6, "level {k}: a feed at z {z} is outside {lo}..{hi}: {block}");
        }
        assert!(block.contains("(ramp:"), "level {k} enters by its own ramp: {block}");
    }
}

#[test]
fn test_without_stepdown_a_pocket_clear_is_one_level_as_ever() {
    let src = source("pocket_levels.cam").replace(", stepdown=5", "");
    assert!(!src.contains("stepdown="), "the word is off");
    let g = generate(&src).expect("generates");
    let clear = g.split("(--- floor:").next().unwrap();
    assert!(!clear.contains("(level"), "{clear}");
    let zs: std::collections::BTreeSet<i64> = feeds(clear).iter().map(|f| (f.2 * 1000.0).round() as i64).filter(|z| *z <= -11_900).collect();
    assert_eq!(zs.into_iter().collect::<Vec<_>>(), vec![-12_000], "one level at the floor: {clear}");
}

#[test]
fn test_the_plane_clear_cuts_four_levels_and_never_enters_the_boss_at_any() {
    let g = generate(&source("plate_floated.cam")).expect("generates");
    let b = op_block(&g, "(--- volume clear: Floor with T5 ---)");
    for z in ["-5.000", "-10.000", "-15.000", "-20.000"] {
        assert!(b.contains(&format!("(level z{z} — 9 passes)")), "{b}");
    }
    for (x, y, _) in feeds(b) {
        let inside = (10.0..=130.0).contains(&x) && (10.0..=50.0).contains(&y) || (70.0..=130.0).contains(&x) && (50.0..=90.0).contains(&y);
        assert!(!inside || dist_to_polygon(&THE_L, x, y) >= 5.0 - 1e-6, "a feed at ({x}, {y}) stands inside the L: {b}");
    }
}

#[test]
fn test_stepdown_belongs_to_clear_and_an_end_is_refused_by_name() {
    let src = source("plate_floated.cam").replace("run Clear(tool=T5, profile=Floor, stepover=4.0, stepdown=5)", "run End(tool=T5, surface=Floor, stepover=4.0, stepdown=5)");
    let err = generate(&src).expect_err("a surface verb has one increment");
    assert!(err.contains("stepdown slices a stack of Z levels and belongs to Clear, the volume verb") && err.contains("End on 'Floor'"), "{err}");
}

#[test]
fn test_stepdown_beside_a_strategy_is_refused_by_name() {
    let src = source("pocket_levels.cam").replace("stepdown=5)", "stepdown=5, strategy=rest, voxel_cell=2.0)");
    let err = generate(&src).expect_err("a field selects at one depth");
    assert!(err.contains("stepdown on 'Window' beside strategy="), "{err}");
}

#[test]
fn test_pocket_levels_golden() {
    let g = generate(&source("pocket_levels.cam")).expect("generates");
    let golden = fs::read_to_string("tests/pocket_levels.golden.gcode").expect("pocket_levels golden");
    assert_eq!(g.trim(), golden.trim(), "pocket_levels.cam drifted from its golden");
}

// ---- Phase 61 nudge 1 -----------------------------------------------------------
// Found in review of the stack: `stepdown=0` (or less) on a pocket, a bore, a
// Plane or a Face validated and cut ONE level — `level_zs` falls back to the
// floor — the parameter that silently does nothing; the door named its floor
// `Top1`; a Plane's refusal called it a pocket.

#[test]
fn test_a_non_positive_stepdown_on_a_clear_is_refused_by_name() {
    for (word, host) in [("stepdown=0", "Window"), ("stepdown=-3", "Window")] {
        let src = source("pocket_levels.cam").replace("stepdown=5", word);
        assert!(src.contains(word), "the replacement took");
        let err = generate(&src).expect_err("a bite of nothing names no stack");
        assert!(err.contains(&format!("{word} on '{host}'")) && err.contains("must be positive"), "{err}");
    }
    let src = source("plate_floated.cam").replace("stepdown=5", "stepdown=0");
    let err = generate(&src).expect_err("on a Plane too");
    assert!(err.contains("stepdown=0 on 'Floor'"), "{err}");
    // The standing hosts' own sentence is unchanged.
    let src = source("cylinder_boss.cam").replace("stepdown=4", "stepdown=0");
    assert!(src.contains("stepdown=0"), "cylinder_boss carries stepdown=4: {src}");
    let err = generate(&src).expect_err("a boss requires a positive stepdown");
    assert!(err.contains("requires a positive stepdown"), "{err}");
}

#[test]
fn test_a_plane_clear_refuses_in_its_own_noun() {
    // A stepover over the tool's radius round an island is the ridge law's
    // refusal; the sentence names the host as what it is.
    let src = source("plate_floated.cam").replace("stepover=4.0, stepdown=5", "stepover=6.0, stepdown=5");
    assert!(src.contains("stepover=6.0"), "the replacement took");
    let err = generate(&src).expect_err("a stepover over the radius round an island is refused");
    assert!(err.contains("Plane feature 'Floor'") && !err.contains("pocket feature 'Floor'"), "{err}");
}

// ---- the rings as arcs (ROADMAP item 1, slice 13) ---------------------------
// Under `compress=arcs` a Clear writes a loop that chords a circle as one
// full-circle G02/G03: the rings over a round blank's face and down a round
// bore. A loop that is no circle keeps its chords under the word, and without
// the word nothing moves.

const DISC: &str = "\
units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Cylinder(dia=120, h=30, x=0, y=0, z=0)\n\
tool T5 Endmill(dia=10.0)\n\
feature Top Plane(z=-1.0)\n";

/// The full-circle arcs of a block: (x, y, i, j) with the position before each.
fn arcs(block: &str) -> Vec<((f64, f64), (f64, f64, f64, f64))> {
    let (mut x, mut y) = (0.0, 0.0);
    let mut out = Vec::new();
    for l in block.lines() {
        if !(l.starts_with("G0")) {
            continue;
        }
        if l.starts_with("G02") || l.starts_with("G03") {
            let (ax, ay) = (word(l, 'X').unwrap(), word(l, 'Y').unwrap());
            out.push(((x, y), (ax, ay, word(l, 'I').unwrap(), word(l, 'J').unwrap())));
        }
        x = word(l, 'X').unwrap_or(x);
        y = word(l, 'Y').unwrap_or(y);
    }
    out
}

/// The distinct radii, to a tenth, at which a block's straight cuts and arcs run.
fn radii(block: &str) -> Vec<i64> {
    let mut out: Vec<i64> = block
        .lines()
        .filter(|l| l.starts_with("G01") && l.contains(" X"))
        .map(|l| (word(l, 'X').unwrap().hypot(word(l, 'Y').unwrap()) * 10.0).round() as i64)
        .chain(arcs(block).into_iter().map(|(_, (_, _, i, j))| (i.hypot(j) * 10.0).round() as i64))
        .collect();
    out.sort();
    out.dedup();
    out
}

#[test]
fn test_a_clear_over_a_round_blank_writes_each_ring_as_one_arc_under_compress_arcs() {
    let chords = generate(&format!("{DISC}run Clear(tool=T5, profile=Top, stepover=4.0)\n")).unwrap();
    let arcs_g = generate(&format!("{DISC}run Clear(tool=T5, profile=Top, stepover=4.0, compress=arcs)\n")).unwrap();
    let (cb, ab) = (op_block(&chords, "volume clear: Top"), op_block(&arcs_g, "volume clear: Top"));
    assert!(arcs(cb).is_empty(), "without the word every ring is chords");
    let rings = cb.lines().filter(|l| l.starts_with("G01 Z")).count();
    let found = arcs(ab);
    assert_eq!(found.len(), rings, "one arc per ring:\n{ab}");
    for ((px, py), (x, y, i, j)) in &found {
        assert!((px - x).abs() < 1e-9 && (py - y).abs() < 1e-9, "a ring starts where it ends: {px} {py} -> {x} {y}");
        assert!((x + i).abs() < 0.02 && (y + j).abs() < 0.02, "every ring is centred on the blank: centre ({}, {})", x + i, y + j);
    }
    // The same rings, spelled as arcs: the radii agree, the lines are a fraction.
    assert_eq!(radii(ab), radii(cb), "the arcs run at the chorded rings' radii");
    assert!(ab.lines().count() * 5 < cb.lines().count(), "{} lines against {}", ab.lines().count(), cb.lines().count());
    // What the report walks is the same path: the as-emitted load of the
    // facing agrees whichever way the rings are spelled.
    let (pc, pa) = (
        parse::parse_project(&format!("{DISC}run Clear(tool=T5, profile=Top, stepover=4.0)\n"), None).unwrap(),
        parse::parse_project(&format!("{DISC}run Clear(tool=T5, profile=Top, stepover=4.0, compress=arcs)\n"), None).unwrap(),
    );
    let (_, ec) = gcode::program_with_moves(&pc).unwrap();
    let (_, ea) = gcode::program_with_moves(&pa).unwrap();
    let (mc, ma) = (nacs::compare::op_moves(&ec, 0), nacs::compare::op_moves(&ea, 0));
    let cut = |m: &[nacs::compare::Move]| m.iter().filter(|m| m.feed).map(|m| (m.to.0 - m.from.0).hypot(m.to.1 - m.from.1)).sum::<f64>();
    assert!((cut(&mc) - cut(&ma)).abs() < 0.5, "the report walks the arcs as the rings they are: {} against {}", cut(&mc), cut(&ma));
}

#[test]
fn test_a_slot_keeps_its_chords_under_compress_arcs_and_a_round_pocket_takes_arcs() {
    let head = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\nstock Box(w=100, l=100, h=20, x=0, y=0, z=0)\ntool T5 Endmill(dia=10.0)\n";
    // A rectangle's rounded rings are never circles: the word changes nothing.
    let slot = format!("{head}feature Slot ProfilePocket(extent=5.0\n    20 35\n    80 35\n    80 65\n    20 65\n)\n");
    let chords = generate(&format!("{slot}run Clear(tool=T5, profile=Slot, stepover=4.0)\n")).unwrap();
    let with = generate(&format!("{slot}run Clear(tool=T5, profile=Slot, stepover=4.0, compress=arcs)\n")).unwrap();
    assert!(arcs(op_block(&with, "pocket clear: Slot")).is_empty(), "a slot's rings are no circles");
    assert_eq!(chords, with, "the word leaves a slot's program as it was");
    // A round pocket's rings are circles: each is one arc, the same radii.
    let round = format!("{head}feature Round CylinderBore(dia=40.0, extent=5.0, x=50, y=50)\n");
    let chords = generate(&format!("{round}run Clear(tool=T5, profile=Round, stepover=4.0)\n")).unwrap();
    let with = generate(&format!("{round}run Clear(tool=T5, profile=Round, stepover=4.0, compress=arcs)\n")).unwrap();
    let (cb, ab) = (op_block(&chords, "bore clear: Round"), op_block(&with, "bore clear: Round"));
    let found = arcs(ab);
    assert!(!found.is_empty(), "a round pocket's rings are arcs:\n{ab}");
    for (_, (x, y, i, j)) in &found {
        assert!((x + i - 50.0).abs() < 0.02 && (y + j - 50.0).abs() < 0.02, "centred on the bore: ({}, {})", x + i, y + j);
    }
    let shift = |block: &str| -> Vec<i64> {
        let mut out: Vec<i64> = block
            .lines()
            .filter(|l| l.starts_with("G01") && l.contains(" X"))
            .map(|l| ((word(l, 'X').unwrap() - 50.0).hypot(word(l, 'Y').unwrap() - 50.0) * 10.0).round() as i64)
            .chain(arcs(block).into_iter().map(|(_, (_, _, i, j))| (i.hypot(j) * 10.0).round() as i64))
            .collect();
        out.sort();
        out.dedup();
        out
    };
    assert_eq!(shift(ab), shift(cb), "the arcs run at the chorded rings' radii");
}
