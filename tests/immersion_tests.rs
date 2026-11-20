// ---- the immersion field (Phase 41 slice 2) --------------------------------
//
// `DECISIONS.md` → "Phase 41 slice 2…". What the tool WOULD meet at every
// place it can stand on a floor, before the op cuts — the op-history stock
// asked around the tool's circle. A ceiling, a picture, never a certificate:
// these pins hold the two ends a reader can check by hand (a floor nobody has
// touched is a full slot everywhere; a floor already cut to height is nothing
// anywhere), where the tool may stand, the bands' words, the terminal's two
// voices, and the declines by name.

use nacs::engagement::{self, Band, IMMERSION_CELL_OF_DIA, IMMERSION_SAMPLES};
use nacs::parse;

fn load(dir: &str) -> parse::Part {
    let main_src = std::fs::read_to_string(format!("{dir}/main.part")).unwrap_or_else(|_| panic!("read {dir}/main.part"));
    let geom = std::fs::read_to_string(format!("{dir}/geometry.part")).ok();
    parse::parse_project_with_base(&main_src, geom.as_deref(), Some(std::path::Path::new(dir))).unwrap_or_else(|e| panic!("parse {dir}: {e}"))
}

fn program(body: &str) -> parse::Part {
    let src = format!(
        "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
         stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\ntool T5 Endmill(dia=10.0)\n{body}"
    );
    parse::parse_project_with_base(&src, None, None).unwrap_or_else(|e| panic!("{e}\n{src}"))
}

/// The step's floor before its only op: untouched stock, so every cell a
/// radius inside the open edges is a FULL slot — the brief's own
/// motivating picture, said as a ceiling — while a cell at an open edge
/// overhangs AIR beyond the blank and reads less, which is exactly where
/// air ends and material begins. The grid's grain is the tool's: an eighth
/// of its diameter, stated in the line.
#[test]
fn test_an_untouched_floor_is_a_full_slot_everywhere_the_tool_is_in_the_blank() {
    let p = load("step.cam");
    let f = engagement::immersion(&p, 0).unwrap();
    assert_eq!(f.cell, 10.0 * IMMERSION_CELL_OF_DIA);
    assert_eq!(f.z, -5.0);
    let cells: Vec<_> = f.cells().collect();
    assert!(!cells.is_empty());
    let inside = |x: f64, y: f64| x <= 95.0 && (5.0..=75.0).contains(&y);
    assert!(cells.iter().filter(|(x, y, _)| inside(*x, *y)).all(|(_, _, t)| (*t - std::f64::consts::PI).abs() < 1e-12), "a radius inside every open edge: π");
    assert!(cells.iter().any(|(x, y, t)| !inside(*x, *y) && *t < std::f64::consts::PI - 1e-9), "at an open edge the circle overhangs air");
    let ([within, above, past], slot) = f.shares();
    assert!(past > 0.95 && slot > 0.7 && within < 0.05 && above < 0.02, "{within} {above} {past} {slot}");
    assert_eq!(f.worst.map(|w| w.2), Some(std::f64::consts::PI));
    // The stepover's nominal: acos(1 - 4/5) = 78.46°.
    assert!((f.nominal - (1.0f64 - 4.0 / 5.0).acos()).abs() < 1e-12);
    let line = f.line(&p);
    assert!(line.starts_with("immersion before Clear on 'Step' with T5 (1.250mm cells, "), "{line}");
    assert!(line.contains("within the stepover's 78°") && line.contains("past 90° (") && line.contains("a full slot); worst 180° at ("), "{line}");
    // The map: rows top first, the three glyphs and blanks, nothing else.
    let map = f.map();
    assert_eq!(map.lines().count(), f.ny);
    assert!(map.chars().all(|c| c == '#' || c == '+' || c == '.' || c == ' ' || c == '\n'), "{map}");
    assert!(map.contains('#'));
}

/// Where the tool may STAND: on the floor, a radius off every WALL edge
/// (never off an OPEN one — the tool may run off there), a radius off what
/// stands. The step's one wall is x = 60, so no cell centre lies in
/// (60, 65); its open edges at x = 100 and y = 0, 80 are reached to the
/// last cell.
#[test]
fn test_the_tool_stands_a_radius_off_walls_and_islands_and_right_up_to_an_open_edge() {
    let p = load("step.cam");
    let f = engagement::immersion(&p, 0).unwrap();
    let xs: Vec<f64> = f.cells().map(|c| c.0).collect();
    assert!(xs.iter().all(|x| *x >= 65.0 - 1e-9), "a radius off the wall at x=60: min {}", xs.iter().cloned().fold(f64::INFINITY, f64::min));
    assert!(xs.iter().any(|x| *x > 99.0), "right up to the open edge at x=100");
    let ys: Vec<f64> = f.cells().map(|c| c.1).collect();
    assert!(ys.iter().any(|y| *y < 1.0) && ys.iter().any(|y| *y > 79.0), "and the open edges at y=0 and y=80");

    let p = load("pocket_islands.cam");
    let i = (0..p.operations.len()).find(|&i| engagement::immersion(&p, i).is_ok()).expect("a floor op");
    let f = engagement::immersion(&p, i).unwrap();
    // PostA is a dia-12 boss at (45, 60): no cell centre within 6 + 5 of it.
    assert!(f.cells().all(|(x, y, _)| (x - 45.0).hypot(y - 60.0) >= 11.0 - 1e-9), "a radius off the post");
    // A closed pocket: a radius off every wall of the 20..120 × 20..100 cavity.
    assert!(f.cells().all(|(x, y, _)| x >= 25.0 - 1e-9 && x <= 115.0 + 1e-9 && y >= 25.0 - 1e-9 && y <= 95.0 + 1e-9));
}

/// A floor already cut to height before the op is nothing anywhere — and
/// an op with no stepover has no "within", so its one band is honest.
#[test]
fn test_a_floor_already_cut_is_within_everywhere() {
    let p = program("feature Deck Plane(z=-2)\nrun End(tool=T5, surface=Deck, stepover=6.0)\nrun End(tool=T5, surface=Deck, stepover=2.0)\n");
    let first = engagement::immersion(&p, 0).unwrap();
    assert!(first.shares().0[2] > 0.9, "the first facing meets the whole block, less its open rim");
    assert!(first.cells().filter(|(x, y, _)| (5.0..=95.0).contains(x) && (5.0..=75.0).contains(y)).all(|(_, _, t)| t == std::f64::consts::PI));
    let second = engagement::immersion(&p, 1).unwrap();
    assert!(second.cells().all(|(_, _, t)| t == 0.0), "the second finds the deck already cut");
    assert_eq!(second.shares(), ([1.0, 0.0, 0.0], 0.0));
    assert_eq!(second.worst.map(|w| w.2), Some(0.0));
    // acos(1 − 2/5) = 53.1°.
    assert!(second.line(&p).contains("100% within the stepover's 53°"), "{}", second.line(&p));
}

/// The bands are three words the house already uses, and the sampling's
/// quantum is the slack between them.
#[test]
fn test_the_bands_are_within_above_and_past_90() {
    let p = load("step.cam");
    let f = engagement::immersion(&p, 0).unwrap();
    let q = std::f64::consts::PI / IMMERSION_SAMPLES as f64;
    assert_eq!(f.band(0.0), Band::Within);
    assert_eq!(f.band(f.nominal), Band::Within);
    assert_eq!(f.band(f.nominal + q * 0.5), Band::Within, "within one sample is within");
    assert_eq!(f.band(f.nominal + q * 1.5), Band::Above);
    assert_eq!(f.band(std::f64::consts::FRAC_PI_2), Band::Above);
    assert_eq!(f.band(std::f64::consts::FRAC_PI_2 + 2.0 * q), Band::Past90);
    assert_eq!(f.band(std::f64::consts::PI), Band::Past90);
}

/// The declines, each by name: a flank op works no floor; an index past
/// the end; a rest floor is several floors.
#[test]
fn test_the_field_declines_by_name() {
    let p = load("profile_pocket.cam");
    let side = (0..p.operations.len()).find(|&i| p.operations[i].op_type == parse::OpType::Side).expect("a Side op");
    let e = engagement::immersion(&p, side).unwrap_err();
    assert!(e.contains("works no floor region"), "{e}");
    let e = engagement::immersion(&p, 99).unwrap_err();
    assert!(e.contains("no operation [99]"), "{e}");
    let p = load("walled_rails_rest_offset.cam");
    let rest = (0..p.operations.len()).find(|&i| nacs::rest::is_rest(&p.operations[i])).expect("a rest op");
    let e = engagement::immersion(&p, rest).unwrap_err();
    assert!(e.contains("rest floor"), "{e}");
    // The report is the line then the map, one voice for the terminal.
    let p = load("step.cam");
    let r = engagement::immersion_report(&p, 0).unwrap();
    let f = engagement::immersion(&p, 0).unwrap();
    assert_eq!(r, format!("{}\n{}", f.line(&p), f.map()));
}
