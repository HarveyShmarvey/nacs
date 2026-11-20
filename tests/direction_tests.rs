//! The direction is said and may be set (ROADMAP item 2, slice 2): every
//! Clear, End and Side block carries one `(direction: …)` line before its
//! first move, and
//! `direction=climb|conventional` on the run line turns every pass that has
//! material on one side, writing G42 where control comp is asked.
use nacs::{gcode, parse};
use std::fs;

const BLOCKS: [&str; 6] = ["clear:", "regions:", "floor:", "side ", "break edge", "chamfer edge"];

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

fn xy(line: &str) -> Option<(f64, f64)> {
    Some((word(line, 'X')?, word(line, 'Y')?))
}

#[test]
fn test_every_clear_end_and_side_block_says_its_direction_once_before_it_moves() {
    let mut blocks = 0;
    for entry in fs::read_dir(".").unwrap() {
        let path = entry.unwrap().path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()).map(str::to_owned) else { continue };
        if !name.ends_with(".cam") || !path.join("main.part").exists() {
            continue;
        }
        let Ok(g) = gcode::program(&project(&name)) else { continue };
        // A block runs from its title line to the next; a title may carry an
        // N word before it.
        let lines: Vec<&str> = g.lines().collect();
        let starts: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| l.contains("(--- ")).map(|(i, _)| i).collect();
        for (k, &s) in starts.iter().enumerate() {
            let b = &lines[s..starts.get(k + 1).copied().unwrap_or(lines.len())];
            let title = b[0];
            if !BLOCKS.iter().any(|w| title.contains(w)) || !b.iter().any(|l| l.starts_with("G01")) {
                continue;
            }
            blocks += 1;
            let said = b.iter().filter(|l| l.starts_with("(direction: ")).count();
            assert_eq!(said, 1, "{name}: `{title}` says its direction {said} times:\n{}", b.join("\n"));
            let first_move = b.iter().position(|l| l.starts_with('G')).unwrap();
            let where_said = b.iter().position(|l| l.starts_with("(direction: ")).unwrap();
            assert!(where_said < first_move, "{name}: `{title}` moves before it says its direction:\n{}", b.join("\n"));
        }
    }
    assert!(blocks > 150, "{blocks} blocks: the corpus shrank");
}

/// The points of a control-comp pass: the comp-on line's and every G01 after
/// it until comp goes off.
fn comped_path(b: &str) -> (String, Vec<(f64, f64)>) {
    let mut pts = Vec::new();
    let mut comp = String::new();
    let mut on = false;
    for l in b.lines() {
        if l.contains("G41") || l.contains("G42") {
            comp = if l.contains("G41") { "G41" } else { "G42" }.to_string();
            on = true;
        }
        if l.contains("G40") {
            break;
        }
        if on {
            if let Some(q) = xy(l) {
                pts.push(q);
            }
        }
    }
    (comp, pts)
}

#[test]
fn test_conventional_runs_the_wall_the_other_way_and_writes_g42() {
    // plate_conventional.cam is plate.cam with `direction=conventional` on
    // its Clear and its Side: the same points in the other order, the tool on
    // the same side of the same edge, which under control comp is G42.
    let climb = gcode::program(&project("plate.cam")).unwrap();
    let conv = gcode::program(&project("plate_conventional.cam")).unwrap();
    let (cs, cp) = comped_path(block(&climb, "side contour: Notch"));
    let (vs, vp) = comped_path(block(&conv, "side contour: Notch"));
    assert_eq!((cs.as_str(), vs.as_str()), ("G41", "G42"));
    assert_eq!(cp.len(), 3, "{cp:?}");
    assert_eq!(cp.iter().rev().cloned().collect::<Vec<_>>(), vp, "the same edge, the other way");
    assert!(!conv.contains("G41"), "{conv}");
    let side = block(&conv, "side contour: Notch");
    assert!(side.contains("\n(direction: conventional — the material on the tool's left under M03)\n"), "{side}");
    // The lead still stands in the notch's void (x < 60, y > 40, or past the
    // blank), not in the plate.
    let lead = side.lines().find(|l| l.starts_with("G00 X")).and_then(xy).unwrap();
    assert!(lead.0 < 60.0 + 1e-9 && lead.1 > 40.0 - 1e-9, "the lead stands in material: {lead:?}");

    // The Clear's pass along the wall x=55 (the notch's east wall is x=60, a
    // radius off): up it climbing — the wall on the right travelling +Y —
    // and down it conventional.
    let along_wall = |b: &str| -> Vec<f64> {
        let mut best: Vec<f64> = Vec::new();
        let mut run: Vec<f64> = Vec::new();
        for l in b.lines() {
            match xy(l) {
                Some((x, y)) if l.starts_with("G01") && (x - 55.0).abs() < 1e-9 => run.push(y),
                _ => {
                    if run.len() > best.len() {
                        best = run.clone();
                    }
                    run.clear();
                }
            }
        }
        if run.len() > best.len() { run } else { best }
    };
    let up = along_wall(block(&climb, "pocket clear: Notch"));
    let down = along_wall(block(&conv, "pocket clear: Notch"));
    assert!(up.len() >= 3 && up.windows(2).all(|w| w[1] >= w[0]), "{up:?}");
    assert!(down.len() >= 3 && down.windows(2).all(|w| w[1] <= w[0]), "{down:?}");
    assert!(block(&conv, "pocket clear: Notch").contains("; the passes along the walls cut conventional)\n"), "{conv}");
}

#[test]
fn test_a_ring_turns_with_the_direction() {
    // A bore ring climbs CCW (G03) and conventional runs CW (G02) about the
    // same centre; the comp word turns with it.
    let climb = gcode::program(&project("cylinder_bore_arcs.cam")).unwrap();
    let conv = with_key("cylinder_bore_arcs.cam", "Side", "direction=conventional").unwrap();
    let ring = |g: &str| -> Vec<String> { block(g, "side bore wall").lines().filter(|l| l.starts_with("G02") || l.starts_with("G03")).map(str::to_string).collect() };
    let (a, b) = (ring(&climb), ring(&conv));
    assert!(!a.is_empty() && a.iter().all(|l| l.starts_with("G03")), "{a:?}");
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(&b) {
        assert_eq!(x.replacen("G03", "G02", 1), *y, "the same ring, the other way");
    }
    assert!(block(&conv, "side bore wall (control)").contains("G42 D") && !conv.contains("G41"), "{conv}");

    // A chorded boss ring: conventional visits the climb ring's points in the
    // other order.
    let climb = gcode::program(&project("cylinder_boss.cam")).unwrap();
    let conv = with_key("cylinder_boss.cam", "Side", "direction=conventional").unwrap();
    let chords = |g: &str| -> Vec<(f64, f64)> {
        let b = block(g, "side boss wall (control)");
        let (_, pts) = comped_path(b);
        pts
    };
    let (a, b) = (chords(&climb), chords(&conv));
    assert!(a.len() > 8, "{}", a.len());
    // Both start and end at angle 0; between, the order reverses.
    assert_eq!(a[0], b[0]);
    assert_eq!(a[1..a.len() - 1].iter().rev().cloned().collect::<Vec<_>>(), b[1..b.len() - 1].to_vec());
}

/// The one-way rows of a floor block: (the direction each stroke runs in X,
/// its Y), for strokes long enough to mean something.
fn strokes(b: &str) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut chain: Vec<(f64, f64)> = Vec::new();
    for l in b.lines() {
        if l.starts_with("G01") {
            if let Some(q) = xy(l) {
                chain.push(q);
            }
        } else if l.starts_with("G00 Z") && chain.len() >= 2 {
            let (a, z) = (chain[0], chain[chain.len() - 1]);
            if (z.0 - a.0).abs() > 1.0 {
                out.push((z.0 - a.0, a.1));
            }
            chain.clear();
        } else if l.starts_with("G00") {
            chain.clear();
        }
    }
    out
}

#[test]
fn test_the_one_way_rows_say_which_side_the_uncut_floor_stands_and_take_the_key() {
    // The wrench's one-way rows run along X, each row one stepover up in Y
    // from the last: the uncut floor stands toward +Y. Travelling +X that is
    // on the tool's left — conventional — and the sentence says so; asked to
    // climb, every stroke turns to run −X and the sentence follows.
    let side = |g: &str| -> (bool, &'static str) {
        let b = block(g, "floor: Top");
        let rows = strokes(b);
        assert!(rows.len() >= 6, "{rows:?}");
        // (A row the outline splits is two strokes at one Y.)
        assert!(rows.windows(2).all(|w| w[1].1 >= w[0].1 - 1e-9), "rows in order of Y: {rows:?}");
        let plus_x = rows.iter().all(|(dx, _)| *dx > 0.0);
        let minus_x = rows.iter().all(|(dx, _)| *dx < 0.0);
        assert!(plus_x || minus_x, "every row the same way: {rows:?}");
        // Material at +Y: on the left of +X travel (conventional), on the
        // right of −X travel (climb).
        let climb = minus_x;
        let said = if b.contains("(direction: the rows climb, the uncut floor on the tool's right under M03, every row the same way") {
            "climb"
        } else if b.contains("(direction: the rows cut conventional, the uncut floor on the tool's left under M03, every row the same way") {
            "conventional"
        } else {
            panic!("{b}")
        };
        (climb, said)
    };
    let unsaid = gcode::program(&project("wrench_oneway.cam")).unwrap();
    assert_eq!(side(&unsaid), (false, "conventional"));
    let asked_climb = with_key("wrench_oneway.cam", "End", "direction=climb").unwrap();
    assert_eq!(side(&asked_climb), (true, "climb"));
    let asked_conv = with_key("wrench_oneway.cam", "End", "direction=conventional").unwrap();
    assert_eq!(side(&asked_conv), (false, "conventional"));
    assert_eq!(asked_conv, unsaid, "conventional is what the one-way rows did unsaid here, so the program is the same");
}

#[test]
fn test_direction_said_as_climb_changes_nothing() {
    let g = gcode::program(&project("plate.cam")).unwrap();
    assert_eq!(with_key("plate.cam", "Side", "direction=climb").unwrap(), g);
    assert_eq!(with_key("plate.cam", "Clear", "direction=climb").unwrap(), g);
}

#[test]
fn test_direction_is_refused_where_nothing_takes_it() {
    // The raster's rows alternate and nothing else is cut.
    let e = with_key("plate_raster.cam", "End", "direction=conventional").unwrap_err();
    assert!(e.contains("direction=conventional has nothing to govern") && e.contains("raster"), "{e}");
    // The zigzag's rows alternate and this face has no wall to run along.
    let e = with_key("wrench_zigzag.cam", "End", "direction=climb").unwrap_err();
    assert!(e.contains("direction=climb has nothing to govern") && e.contains("zigzag"), "{e}");
    // Spine's rows run whichever way the link is shortest; with no wall
    // pass there is nothing for the key to turn.
    let e = with_key("wrench_spine.cam", "End", "direction=conventional").unwrap_err();
    assert!(e.contains("has nothing to govern") && e.contains("spine"), "{e}");
    // A Copy follows a surface with a ball's nose, a visit to points has no
    // side at all: the key is refused at parse, not read by nothing.
    let e = with_key("sphere.cam", "Copy", "direction=climb").unwrap_err();
    assert!(e.contains("direction= is read by a Clear, an End or a Side") && e.contains("a Copy has no side"), "{e}");
    let e = with_key("cbore.cam", "End", "direction=climb").unwrap_err();
    assert!(e.contains("an End at points has no side"), "{e}");
    let e = with_key("plate.cam", "Side", "direction=sideways").unwrap_err();
    assert!(e.contains("unknown direction 'sideways'"), "{e}");
}

#[test]
fn test_plate_conventional_golden() {
    let g = gcode::program(&project("plate_conventional.cam")).unwrap();
    let golden = fs::read_to_string("tests/plate_conventional.golden.gcode").expect("golden");
    assert_eq!(g.trim(), golden.trim(), "plate_conventional golden drifted");
}
