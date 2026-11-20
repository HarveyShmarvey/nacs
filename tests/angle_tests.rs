//! The rows' direction may be set (ROADMAP item 2, slice 3): `angle=` on a
//! Clear or an End cutting rows pins the direction spine's comb and the
//! parallel rows would otherwise choose, and the comb chooses only the phase
//! and the way. Where no rows are cut the key is refused.
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

/// Every flat feed move in the block that changes X or Y, as `(dx, dy)`. A
/// move with a Z word is a descent, not a row: a ramp runs down the first
/// stroke backwards.
fn feed_moves(b: &str) -> Vec<(f64, f64)> {
    let mut at: Option<(f64, f64)> = None;
    let mut out = Vec::new();
    for l in b.lines() {
        let moves = l.starts_with("G00") || l.starts_with("G01");
        let (Some(x), Some(y)) = (word(l, 'X'), word(l, 'Y')) else { continue };
        if !moves {
            continue;
        }
        if let (Some((px, py)), true) = (at, l.starts_with("G01") && word(l, 'Z').is_none()) {
            let (dx, dy) = (x - px, y - py);
            if dx.abs() > 1e-6 || dy.abs() > 1e-6 {
                out.push((dx, dy));
            }
        }
        at = Some((x, y));
    }
    out
}

/// How far the block feeds along X and along Y, in all.
fn travel(b: &str) -> (f64, f64) {
    feed_moves(b).iter().fold((0.0, 0.0), |(x, y), (dx, dy)| (x + dx.abs(), y + dy.abs()))
}

#[test]
fn test_the_pinned_angle_turns_the_rows() {
    // The plate's face is 120 wide and 80 deep; the comb lays its rows along
    // X. Pinned to 90 the rows run along Y, and the program cuts more of them.
    let chosen = gcode::program(&project("plate.cam")).unwrap();
    let pinned = with_key("plate.cam", "End", "angle=90").unwrap();
    let (cx, cy) = travel(block(&chosen, "floor: TopFace"));
    let (px, py) = travel(block(&pinned, "floor: TopFace"));
    assert!(cx > 3.0 * cy, "the comb's rows run along X: {cx} along X, {cy} along Y");
    assert!(py > 3.0 * px, "the pinned rows run along Y: {px} along X, {py} along Y");
    assert!(pinned.contains("(pattern: spine at 90.000deg, the angle set, "), "{pinned}");
    // Every row is a stroke along Y: no feed move in the block runs oblique.
    for (dx, dy) in feed_moves(block(&pinned, "floor: TopFace")) {
        assert!(dx.abs() < 1e-6 || dy.abs() < 1e-6, "an oblique move ({dx}, {dy}) on a face pinned to 90");
    }
}

#[test]
fn test_the_rows_run_at_the_oblique_angle_set() {
    // The notch's walls stand along X and Y; the comb lays its rows along
    // one of them. Pinned to 45, the rows run at 45 degrees and carry nearly
    // all the feed travel that is not a pass along a wall; what is left is
    // the links between rows and a shore's leads.
    let chosen = gcode::program(&project("plate.cam")).unwrap();
    let pinned = with_key("plate.cam", "Clear", "angle=45").unwrap();
    let classes = |b: &str| {
        let (mut at_45, mut on_axis, mut other) = (0.0, 0.0, 0.0);
        let mut n_45 = 0;
        for (dx, dy) in feed_moves(b) {
            let len = dx.hypot(dy);
            if dx.abs() < 1e-6 || dy.abs() < 1e-6 {
                on_axis += len;
            } else if (dx.abs() - dy.abs()).abs() < 1e-3 {
                at_45 += len;
                n_45 += 1;
            } else {
                other += len;
            }
        }
        (at_45, n_45, on_axis, other)
    };
    let (c45, cn, _, _) = classes(block(&chosen, "pocket clear: Notch"));
    assert_eq!((c45, cn), (0.0, 0), "the comb chose an axis on the notch");
    let (p45, pn, axis, other) = classes(block(&pinned, "pocket clear: Notch"));
    assert!(pn >= 10 && p45 > axis && p45 > 3.0 * other, "{pn} rows at 45 degrees, {p45} along them, {axis} along the walls, {other} elsewhere");
    assert!(pinned.contains("(pattern: spine at 45.000deg, the angle set, "), "{pinned}");
}

#[test]
fn test_pinning_the_angle_the_comb_chose_changes_only_the_words() {
    // Spine on the plate's face, held on the post pocket, one-way on the
    // wrench: each comb's own choice, set by hand, moves nothing.
    for (dir, verb, key, at) in [
        ("plate.cam", "End", "angle=0", "spine at 0.000deg, the angle set"),
        ("pocket_post_held.cam", "Clear", "angle=90", "spine's comb at 90.000deg, the angle set"),
        ("wrench_oneway.cam", "End", "angle=0", "oneway along X, the angle set"),
    ] {
        let chosen = gcode::program(&project(dir)).unwrap();
        let pinned = with_key(dir, verb, key).unwrap();
        assert!(pinned.contains(at), "{dir}: {pinned}");
        assert_eq!(pinned.replace(", the angle set", ""), chosen, "{dir}: the comb's own direction, set, moved the program");
    }
}

#[test]
fn test_the_one_way_rows_follow_the_angle_and_still_run_one_way() {
    let pinned = with_key("wrench_oneway.cam", "End", "angle=90").unwrap();
    let b = block(&pinned, "floor: Top");
    let rows: Vec<(f64, f64)> = feed_moves(b).into_iter().filter(|(dx, dy)| dx.abs() > 1e-6 || dy.abs() > 1e-6).filter(|(_, dy)| dy.abs() > 5.0).collect();
    assert!(rows.len() >= 10, "{} rows along Y in\n{b}", rows.len());
    for (dx, _) in &rows {
        assert!(dx.abs() < 1e-6, "a row ({dx}) off the Y axis");
    }
    let up = rows.iter().filter(|(_, dy)| *dy > 0.0).count();
    assert!(up == 0 || up == rows.len(), "one-way rows run both ways: {up} up of {}", rows.len());
    assert!(pinned.contains("(pattern: oneway along Y, the angle set, "), "{pinned}");
}

#[test]
fn test_angle_is_refused_where_nothing_takes_it() {
    // A Side cuts no rows.
    let e = with_key("plate.cam", "Side", "angle=45").unwrap_err();
    assert!(e.contains("angle= is read by a Clear or an End cutting rows") && e.contains("a Side cuts no rows"), "{e}");
    // The offset family cuts loops, said or unsaid.
    let e = with_key("wrench_offset.cam", "End", "angle=45").unwrap_err();
    assert!(e.contains("angle=45 on 'Top'") && e.contains("the offset family cuts loops, not rows"), "{e}");
    let e = with_key("sphere.cam", "Clear", "angle=45").unwrap_err();
    assert!(e.contains("which this op cuts unsaid, cuts loops"), "{e}");
    // The raster's rows run along X by construction.
    let e = with_key("plate_raster.cam", "End", "angle=45").unwrap_err();
    assert!(e.contains("the raster's rows run along X and nothing turns them"), "{e}");
    // A Copy and a visit to points cut no rows either.
    let e = with_key("sphere.cam", "Copy", "angle=45").unwrap_err();
    assert!(e.contains("a Copy cuts no rows"), "{e}");
    let e = with_key("cbore.cam", "End", "angle=45").unwrap_err();
    assert!(e.contains("an End at points cuts no rows"), "{e}");
    // Rows have no sense: the half-turn is the whole range.
    for bad in ["angle=180", "angle=-5", "angle=360"] {
        let e = with_key("plate.cam", "Clear", bad).unwrap_err();
        assert!(e.contains("0 up to but not including 180"), "{bad}: {e}");
    }
    assert!(with_key("plate.cam", "Clear", "angle=across").is_err());
}

#[test]
fn test_plate_angled_golden() {
    let g = gcode::program(&project("plate_angled.cam")).unwrap();
    let golden = fs::read_to_string("tests/plate_angled.golden.gcode").expect("golden");
    assert_eq!(g.trim(), golden.trim(), "plate_angled golden drifted");
}
