//! Every descent may be made one kind (ROADMAP item 2, slice 4): `entry=`
//! on a Clear, an End or a Side makes each descent a ramp, a plunge or a
//! lead, and the op refuses where one cannot be.
use nacs::{gcode, parse};
use std::fs;

fn project(dir: &str) -> parse::Part {
    parse::load_project(dir).unwrap_or_else(|e| panic!("{dir}: {e:?}"))
}

/// The fixture's program with `key` added to every `run <verb>(` line.
fn with_key(dir: &str, verb: &str, key: &str) -> Result<String, String> {
    let main = fs::read_to_string(format!("{dir}/main.part")).unwrap();
    let geom = fs::read_to_string(format!("{dir}/geometry.part")).ok();
    let needle = format!("run {verb}(");
    let mut out = String::new();
    let mut done = false;
    for line in main.lines() {
        if line.starts_with(&needle) {
            let k = line.rfind(')').unwrap();
            out.push_str(&format!("{}, {}{}\n", &line[..k], key, &line[k..]));
            done = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    assert!(done, "{dir} has no `{needle}` line");
    let p = parse::parse_project(&out, geom.as_deref()).map_err(|e| format!("{e:?}"))?;
    gcode::program(&p)
}

fn block<'a>(g: &'a str, title: &str) -> &'a str {
    g.split("\n(--- ").find(|b| b.starts_with(title)).unwrap_or_else(|| panic!("no block `{title}` in\n{g}"))
}

fn word(line: &str, letter: char) -> Option<f64> {
    line.split_whitespace().find(|w| w.starts_with(letter)).and_then(|w| w[1..].parse().ok())
}

/// The descents in a block: every feed move that lowers Z, as `(sloped, line)`
/// — sloped where it moves in X or Y too, which is a ramp's leg.
fn descents(b: &str) -> Vec<(bool, &str)> {
    let mut z = f64::INFINITY;
    let mut out = Vec::new();
    for l in b.lines() {
        if !(l.starts_with("G00") || l.starts_with("G01")) {
            continue;
        }
        if let Some(nz) = word(l, 'Z') {
            if nz < z - 1e-9 && l.starts_with("G01") {
                out.push((word(l, 'X').is_some() || word(l, 'Y').is_some(), l));
            }
            z = nz;
        }
    }
    out
}

fn count(b: &str, prefix: &str) -> usize {
    b.lines().filter(|l| l.starts_with(prefix)).count()
}

#[test]
fn test_entry_plunge_makes_every_descent_a_plunge() {
    // The post pocket's rows: the plan ramps the first stroke, a slot in
    // standing stock, down in legs.
    let chosen = gcode::program(&project("pocket_post_spine.cam")).unwrap();
    assert!(block(&chosen, "pocket clear: PocketProfile").contains("(ramp: 2 legs along the first stroke"), "{chosen}");
    let g = with_key("pocket_post_spine.cam", "Clear", "entry=plunge").unwrap();
    let b = block(&g, "pocket clear: PocketProfile");
    assert_eq!(count(b, "(ramp: "), 0, "{b}");
    assert!(descents(b).iter().all(|(sloped, _)| !sloped), "a sloped descent under entry=plunge:\n{b}");
    assert!(b.contains("(plunge: at the stroke's start, into standing stock, as entry=plunge asks)"), "{b}");
    assert_eq!(count(b, "(plunge: "), descents(b).len(), "one sentence per descent:\n{b}");
    // The offset family: its first loop ramps; asked, it plunges at its start.
    let g = with_key("wrench_offset.cam", "End", "entry=plunge").unwrap();
    let b = block(&g, "floor: Top");
    assert_eq!(count(b, "(ramp: "), 0);
    assert!(descents(b).iter().all(|(sloped, _)| !sloped));
}

#[test]
fn test_entry_plunge_on_a_wall_drops_the_lead() {
    let chosen = gcode::program(&project("plate.cam")).unwrap();
    let g = with_key("plate.cam", "Side", "entry=plunge").unwrap();
    let (cb, pb) = (block(&chosen, "side contour: Notch"), block(&g, "side contour: Notch"));
    assert!(cb.contains("(lead: 10.000mm along the path's own line, back from its start, in air)"), "{cb}");
    assert!(pb.contains("(plunge: on the path, as entry=plunge asks)"), "{pb}");
    // Under control comp the descent is on the tool-centre path, a radius off
    // the authored edge the comp-on move reaches; unsaid it stands a diameter
    // further back along the path's line.
    let descent_off_edge = |b: &str| {
        let lines: Vec<&str> = b.lines().collect();
        let down = lines.iter().position(|l| l.starts_with("G01 Z")).unwrap();
        let at = lines[..down].iter().rev().find(|l| l.starts_with("G00 X")).unwrap();
        let on = lines[down..].iter().find(|l| l.starts_with("G01 G41")).unwrap();
        (word(at, 'X').unwrap() - word(on, 'X').unwrap()).hypot(word(at, 'Y').unwrap() - word(on, 'Y').unwrap())
    };
    assert!((descent_off_edge(pb) - 5.0).abs() < 1e-3, "the descent is off the path:\n{pb}");
    assert!((descent_off_edge(cb) - 10.0f64.hypot(5.0)).abs() < 1e-3, "the lead is not a diameter long:\n{cb}");
}

#[test]
fn test_entry_ramp_makes_every_descent_a_ramp() {
    // The notch: the rows that start in air and the shore pass plunge
    // unasked; asked, every descent ramps down its first stroke.
    let g = with_key("plate.cam", "Clear", "entry=ramp").unwrap();
    let b = block(&g, "pocket clear: Notch");
    assert_eq!(count(b, "(plunge: "), 0, "{b}");
    let n = descents(b).len();
    assert!(n >= 2 && descents(b).iter().filter(|(sloped, _)| *sloped).count() >= n - 1, "every descent after the plunge to the ramp's start is sloped:\n{b}");
    assert!(b.contains(", as entry=ramp asks)"), "{b}");
    // The face: its one chain starts in air past the blank and plunges
    // unasked; asked, it ramps, and the one sloped move is the ramp's.
    let g = with_key("plate.cam", "End", "entry=ramp").unwrap();
    let b = block(&g, "floor: TopFace");
    assert_eq!(count(b, "(plunge: "), 0, "{b}");
    assert_eq!(count(b, "(ramp: "), descents(b).iter().filter(|(sloped, _)| *sloped).count(), "{b}");
    assert!(b.contains("(ramp: down the first stroke, 10.000mm of it, to hold 3.000deg, as entry=ramp asks)"), "{b}");
    // The offset family: every loop ramps, the first unasked.
    let g = with_key("pocket_floor_offset.cam", "End", "entry=ramp").unwrap();
    let b = block(&g, "floor: Cavity floor");
    assert_eq!(count(b, "(plunge: "), 0, "{b}");
    assert!(b.contains("(ramp: 1 lap of this loop, 56.473mm round, to hold 3.000deg)") && b.contains(", as entry=ramp asks)"), "{b}");
}

#[test]
fn test_entry_ramp_refuses_where_there_is_no_room() {
    // The wrench's innermost loop is under a diameter round.
    let e = with_key("wrench_offset.cam", "End", "entry=ramp").unwrap_err();
    assert!(e.contains("entry=ramp on 'Top'") && e.contains("under one diameter, and has no room to ramp"), "{e}");
}

#[test]
fn test_entry_lead_is_kept_where_a_lead_is_made_and_refused_elsewhere() {
    // A wall trace and a ring are entered on a lead already: saying so moves
    // nothing.
    for (dir, verb) in [("plate.cam", "Side"), ("cylinder_bore.cam", "Side"), ("cylinder_boss.cam", "Side")] {
        let chosen = gcode::program(&project(dir)).unwrap();
        assert_eq!(with_key(dir, verb, "entry=lead").unwrap(), chosen, "{dir}");
    }
    // A row is entered down its own line; a loop in stock has no air in
    // reach for a lead.
    let e = with_key("plate.cam", "Clear", "entry=lead").unwrap_err();
    assert!(e.contains("a lead from the air beside it is not built"), "{e}");
    let e = with_key("wrench_offset.cam", "End", "entry=lead").unwrap_err();
    assert!(e.contains("no start on loop 1 of 6 has the whole lead in air"), "{e}");
}

#[test]
fn test_entry_is_refused_where_nothing_takes_it() {
    // A ring is entered on a lead and nothing else is built for it.
    let e = with_key("cylinder_bore.cam", "Side", "entry=plunge").unwrap_err();
    assert!(e.contains("a ring is entered on a lead") && e.contains("a plunge at the wall is not built"), "{e}");
    // The raster plunges in air off the blank's edge.
    let e = with_key("plate_raster.cam", "End", "entry=ramp").unwrap_err();
    assert!(e.contains("the raster plunges in air off the blank's edge"), "{e}");
    assert!(with_key("plate_raster.cam", "End", "entry=plunge").is_ok());
    // A Copy's descent, a Form's and a visit to points' is the operation.
    let e = with_key("sphere.cam", "Copy", "entry=plunge").unwrap_err();
    assert!(e.contains("a Copy has no entry to name"), "{e}");
    let e = with_key("cbore.cam", "End", "entry=lead").unwrap_err();
    assert!(e.contains("an End at points has no entry to name"), "{e}");
    let e = with_key("cbore.cam", "Form", "entry=ramp").unwrap_err();
    assert!(e.contains("a Form at points has no entry to name"), "{e}");
    let e = with_key("plate.cam", "Clear", "entry=dive").unwrap_err();
    assert!(e.contains("unknown entry 'dive'"), "{e}");
}

#[test]
fn test_pocket_post_plunged_golden() {
    let g = gcode::program(&project("pocket_post_plunged.cam")).unwrap();
    let golden = fs::read_to_string("tests/pocket_post_plunged.golden.gcode").expect("golden");
    assert_eq!(g.trim(), golden.trim(), "pocket_post_plunged golden drifted");
}
