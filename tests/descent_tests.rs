//! Every descent the engine chooses is said before it is made (ROADMAP item
//! 2, slice 1): one comment line — `(plunge: …)`, `(ramp: …)` or `(lead: …)`
//! with the reason — ahead of each straight descent at a stated feed in a
//! Clear, End or Side block. A Form's visit to points and a Copy's passes
//! are the operation itself and say nothing extra.
use nacs::{gcode, parse};
use std::fs;

const SAID: [&str; 3] = ["(plunge:", "(ramp:", "(lead:"];
const BLOCKS: [&str; 8] = ["clear:", "regions:", "floor:", "side contour", "side bore wall", "side boss wall", "break edge", "chamfer edge"];

fn project(dir: &str) -> parse::Part {
    parse::load_project(dir).unwrap_or_else(|e| panic!("{dir}: {e:?}"))
}

fn block<'a>(g: &'a str, title: &str) -> &'a str {
    g.split("\n(--- ").find(|b| b.starts_with(title)).unwrap_or_else(|| panic!("no block `{title}` in\n{g}"))
}

/// `G01 Z<z> F<f>`: a straight descent at a stated feed, no X or Y.
fn is_descent(l: &str) -> bool {
    l.starts_with("G01 Z") && l.contains(" F") && !l.contains(" X") && !l.contains(" Y")
}

fn is_said(l: &str) -> bool {
    SAID.iter().any(|s| l.starts_with(s))
}

#[test]
fn test_every_descent_in_a_clear_end_or_side_block_is_said_first() {
    let (mut fixtures, mut descents) = (0, 0);
    for entry in fs::read_dir(".").unwrap() {
        let path = entry.unwrap().path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()).map(str::to_owned) else { continue };
        if !name.ends_with(".cam") || !path.join("main.part").exists() {
            continue;
        }
        let g = match gcode::program(&project(&name)) {
            Ok(g) => g,
            Err(_) => continue,
        };
        fixtures += 1;
        for b in g.split("\n(--- ").skip(1) {
            let title = b.lines().next().unwrap();
            if !BLOCKS.iter().any(|w| title.contains(w)) {
                continue;
            }
            let lines: Vec<&str> = b.lines().collect();
            let mut n = 0;
            for (i, l) in lines.iter().enumerate() {
                if !is_descent(l) {
                    continue;
                }
                n += 1;
                // The sentence stands at most four lines up: itself, then the
                // retract, the rapid over the point and the rapid down to
                // rapid_z, then the descent.
                let window = &lines[i.saturating_sub(4)..i];
                assert!(window.iter().any(|w| is_said(w)), "{name}: a descent nobody announced in `{title}`:\n{}\n{l}", window.join("\n"));
            }
            let said = lines.iter().filter(|l| is_said(l)).count();
            assert_eq!(said, n, "{name}: `{title}` says {said} entries and makes {n} descents");
            descents += n;
        }
    }
    assert!(fixtures > 100 && descents > 1000, "{fixtures} fixtures, {descents} descents: the corpus shrank");
}

#[test]
fn test_the_sentences_name_the_reason() {
    // The notch is open on two edges: its first row starts past them, in
    // air, and spine links its one shore pass to the rows without a lift;
    // the face's raster starts off the blank's edge; the wall's lead has a
    // whole diameter of air and says so.
    let g = gcode::program(&project("plate.cam")).unwrap();
    let notch = block(&g, "pocket clear: Notch");
    assert!(notch.contains("\n(plunge: in air outside the blank)\n"), "{notch}");
    assert_eq!(notch.lines().filter(|l| is_said(l)).count(), 1, "one descent, one sentence: {notch}");
    let side = block(&g, "side contour: Notch");
    assert!(side.contains("\n(lead: 10.000mm along the path's own line, back from its start, in air)\n"), "{side}");
    let raster = gcode::program(&project("plate_raster.cam")).unwrap();
    let face = block(&raster, "floor: TopFace");
    assert!(face.contains("\n(plunge: in air off the blank's edge)\n"), "{face}");

    // A closed pocket's innermost loop is 8 mm round, under the diameter, so
    // it plunges and says why; every loop out from it plunges beside the air
    // the one before cut, a stepover off, and says how far.
    let g = gcode::program(&project("profile_pocket_clear.cam")).unwrap();
    let b = block(&g, "pocket clear:");
    assert!(b.contains("\n(plunge: this loop is 8.000mm round, under one diameter — no room to ramp)\n"), "{b}");
    assert_eq!(b.matches("\n(plunge: beside air an earlier loop cut, 4.000mm off its path)\n").count(), 6, "{b}");

    // A loop with the room ramps round itself once.
    let g = gcode::program(&project("chamfer.cam")).unwrap();
    assert!(g.contains("\n(ramp: 1 lap of this loop, ") && g.contains("mm round, to hold 3.000deg)\n"), "{g}");

    // The zigzag's open shore passes come after its rows, each entered at
    // its lead's start beside floor the rows cut.
    let g = gcode::program(&project("step_zigzag.cam")).unwrap();
    assert!(g.contains("\n(plunge: at the lead's start, beside floor the rows cut)\n"), "{g}");

    // The held pattern's rows plunge beside a stroke already cut, within a
    // stepover, and the distance is said — the re-entry plunge is no longer
    // silent.
    let g = gcode::program(&project("plate_held.cam")).unwrap();
    let b = block(&g, "pocket clear: Notch");
    assert!(b.contains("\n(plunge: beside a stroke already cut, "), "{b}");

    // A bore's wall is entered down its axis, a boss's a diameter outside
    // its wall, both in the air the clear left.
    let g = gcode::program(&project("cylinder_bore.cam")).unwrap();
    assert!(g.contains("\n(lead: from the bore's axis, in the air its clear left)\n"), "{g}");
    let g = gcode::program(&project("cylinder_boss.cam")).unwrap();
    assert!(g.contains("\n(lead: from one diameter outside the wall, in the air its clear left)\n"), "{g}");

    // The face around standing bosses: its first loop rides a radius outside
    // the blank and plunges in air; the rest beside what the last one cut.
    let g = gcode::program(&project("face_boss_offset.cam")).unwrap();
    let b = block(&g, "floor: Deck");
    assert!(b.contains("\n(plunge: in air outside the blank)\n"), "{b}");
    assert!(b.contains("\n(plunge: beside air an earlier loop cut, "), "{b}");
}

#[test]
fn test_a_ramp_down_one_stroke_is_said() {
    // The wrench's spine rows ramp down their first stroke where it has the
    // room; the sentence names the length the law needs. (A row that starts
    // in given air names its void: `given_entry_tests`.)
    let g = gcode::program(&project("wrench_spine.cam")).unwrap();
    assert!(g.contains("\n(ramp: down the first stroke, ") && g.contains("mm of it, to hold 3.000deg)\n"), "{g}");
}
