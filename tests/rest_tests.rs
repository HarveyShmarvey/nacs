// ---- the rest floor: a concession becomes the first step of a plan ------------
// Phase 33 (`DECISIONS.md` → "Phase 33 slice 1: the rest floor…"). An End
// with `strategy=rest` cuts only what an earlier op on the same floor left
// standing — the stretches its concession named — each one a floor of its
// own, walled against what stands and open against the earlier cut.

use nacs::parse::Pt;
use nacs::{gcode, parse};

fn load(dir: &str) -> parse::Part {
    let main = std::fs::read_to_string(format!("{dir}/main.part")).unwrap();
    let geom = std::fs::read_to_string(format!("{dir}/geometry.part")).ok();
    parse::parse_project(&main, geom.as_deref()).unwrap_or_else(|e| panic!("{dir}: {e}"))
}

/// A small slot plate: a face at z=-4 with two fins standing 4mm apart on
/// it, so a 10mm tool concedes the slot between them and a 3mm tool takes it.
fn slot_plate(runs: &str) -> Result<parse::Part, String> {
    let src = format!(
        "units mm
feed 800
plunge 300
rapid_z 5
clearance_z 25
spindle 3000
stock Box(w=60, l=40, h=10, x=0, y=0, z=0)
tool T1 Endmill(dia=10.0)
tool T2 Endmill(dia=3.0)
feature Floor Face(z=-4
    loop
        0 0
        60 0
        60 40
        0 40
)
feature FinA ProfileBoss(extent=4
    20 10
    20 30
    28 30
    28 10
)
feature FinB ProfileBoss(extent=4
    32 10
    32 30
    40 30
    40 10
)
{runs}
"
    );
    parse::parse_project(&src, None)
}

/// The program's at-depth cutting moves, as segments — `spine_tests`' reading.
fn at_depth(g: &str, z_cut: f64) -> Vec<(Pt, Pt)> {
    let (mut x, mut y, mut z) = (f64::NAN, f64::NAN, f64::NAN);
    let mut out = Vec::new();
    for line in g.lines().filter(|l| l.starts_with("G0")) {
        let (mut nx, mut ny, mut nz) = (x, y, z);
        for w in line.split_whitespace() {
            match w.as_bytes()[0] {
                b'X' => nx = w[1..].parse().unwrap_or(nx),
                b'Y' => ny = w[1..].parse().unwrap_or(ny),
                b'Z' => nz = w[1..].parse().unwrap_or(nz),
                _ => {}
            }
        }
        if line.starts_with("G01") && x.is_finite() && (z - z_cut).abs() < 1e-6 && (nz - z_cut).abs() < 1e-6 {
            out.push((Pt { x, y }, Pt { x: nx, y: ny }));
        }
        (x, y, z) = (nx, ny, nz);
    }
    out
}

/// The op's own lines of the program: from its title to the next op's.
fn op_text(g: &str, title_prefix: &str) -> String {
    let start = g.find(title_prefix).unwrap_or_else(|| panic!("no '{title_prefix}' in the program"));
    let rest = &g[start..];
    let end = rest[1..].find("(--- ").map_or(rest.len(), |k| k + 1);
    rest[..end].to_string()
}

#[test]
fn test_the_rest_fixtures_are_golden() {
    for dir in ["walled_rails_rest_offset.cam", "walled_rails_ladder_offset.cam", "walled_rails_rest_spine.cam", "deck_rest_offset.cam"] {
        let g = gcode::program(&load(dir)).unwrap_or_else(|e| panic!("{dir}: {e}"));
        let golden = std::fs::read_to_string(format!("tests/{}.golden.gcode", dir.trim_end_matches(".cam"))).unwrap();
        assert_eq!(g.trim(), golden.trim(), "{dir}");
    }
}

/// THE KEY, FINISHED. Phase 32 scored the 10mm program against the answer
/// key and left three edges `refuse` — the stretches no 10mm tool enters.
/// The rest op comes within its own radius of every one of them, and its
/// passes stay on those stretches: none of it re-faces the land.
#[test]
fn test_the_rest_op_cuts_the_three_edges_the_key_refuses() {
    let g = gcode::program(&load("walled_rails_rest_offset.cam")).unwrap();
    let rest = op_text(&g, "(--- floor: Land with T6");
    let cuts = at_depth(&format!("G00 X0 Y0 Z25\n{rest}"), -8.0);
    assert!(!cuts.is_empty(), "the rest op cuts at the floor");
    let near = |q: Pt| cuts.iter().map(|(a, b)| nacs::stock::dist_to_polygon(&[*a, *b], q.x, q.y)).fold(f64::INFINITY, f64::min);
    let key = std::fs::read_to_string("walled_rails_offset.cam/key.json").unwrap();
    let mut refused = 0;
    for e in key.split("{\"id\":").skip(1) {
        let expected = e.split("\"expected\": \"").nth(1).unwrap().split('"').next().unwrap();
        if expected != "refuse" {
            continue;
        }
        let (s, t) = (e.split("\"start\": [").nth(1).unwrap(), e.split("\"end\": [").nth(1).unwrap());
        let num = |txt: &str| -> Pt {
            let v: Vec<f64> = txt.split(']').next().unwrap().split(',').map(|n| n.trim().parse().unwrap()).collect();
            Pt { x: v[0], y: 130.0 - v[1] }
        };
        let (a, b) = (num(s), num(t));
        let closest = (1..10)
            .map(|k| k as f64 / 10.0)
            .map(|f| near(Pt { x: a.x + (b.x - a.x) * f, y: a.y + (b.y - a.y) * f }))
            .fold(f64::INFINITY, f64::min);
        assert!(closest <= 1.5, "a key `refuse` edge: the nearest rest pass is {closest:.2}mm off it");
        refused += 1;
    }
    assert_eq!(refused, 3, "the key's own tally of refused edges");
    // …and nothing else: every at-depth move of the rest op lies within its
    // own diameter of the three stretches' outlines op 1 held back.
    let p = load("walled_rails_rest_offset.cam");
    let held = nacs::stock::conceded_holes(&p, &p.operations[0]);
    assert_eq!(held.len(), 3);
    for (a, _) in &cuts {
        let d = held.iter().map(|h| if nacs::stock::point_in_polygon(h, a.x, a.y) { 0.0 } else { nacs::stock::dist_to_polygon(h, a.x, a.y) }).fold(f64::INFINITY, f64::min);
        assert!(d <= 3.0 + 1e-6, "a rest pass at ({:.3}, {:.3}) is {d:.2}mm from every stretch", a.x, a.y);
    }
}

/// The swept truth agrees with the declared one: after the rest op nothing
/// the model says is gone was left, and nothing that stands was swept.
#[test]
fn test_the_rest_fixtures_forget_nothing_and_violate_nothing() {
    for dir in ["walled_rails_rest_offset.cam", "walled_rails_ladder_offset.cam", "walled_rails_rest_spine.cam", "deck_rest_offset.cam"] {
        let p = load(dir);
        let (_, emitted) = gcode::program_with_moves(&p).unwrap();
        let cells = nacs::vox::rest_cells(&p, &emitted, p.operations.len(), None).unwrap();
        assert!(cells.violated.is_empty(), "{dir}: {} cells the declaration says stand, swept anyway", cells.violated.len());
        assert!(cells.forgotten.is_empty(), "{dir}: {} cells forgotten, first at {:?}", cells.forgotten.len(), cells.forgotten.first());
    }
}

/// The rest floor is what the stock model held back, and after the rest op
/// the model holds nothing back on that floor: the op REMOVED the stretches.
#[test]
fn test_the_slot_is_cut_and_claimed() {
    let p = slot_plate(
        "run End(tool=T1, surface=Floor, stepover=5, cover=80)\nrun End(tool=T2, surface=Floor, stepover=1.5, strategy=rest)",
    )
    .unwrap();
    let g = gcode::program(&p).unwrap();
    let rest = op_text(&g, "(--- floor: Floor with T2");
    assert!(rest.contains("the rest: 1 floor op 1 left standing"), "{rest}");
    // The slot's middle is solid before the rest op and gone after it.
    let (before, after) = (nacs::stock::remaining_before(&p, 1), nacs::stock::remaining_after(&p, 1));
    assert!(before.material_at(30.0, 20.0, -3.0), "the slot stands before the rest op");
    assert!(!after.material_at(30.0, 20.0, -3.0), "and is gone after it");
    // A fin still stands after both.
    assert!(after.material_at(24.0, 20.0, -3.0));
    // The outline says what the op works — the rest, not the whole floor.
    assert_eq!(nacs::outline::region_line(&p, 1).unwrap(), "rest floor: 1 floor op [0] left standing, 77.8mm²");
}

/// Fork 4: nothing stands, and the op says so by name rather than emitting
/// nothing.
#[test]
fn test_a_rest_op_with_nothing_standing_is_a_hard_error() {
    let p = slot_plate("run End(tool=T2, surface=Floor, stepover=1.5)\nrun End(tool=T2, surface=Floor, stepover=1.5, strategy=rest)").unwrap();
    let e = gcode::program(&p).unwrap_err();
    assert!(e.contains("left none of this floor standing") && e.contains("op [0]"), "{e}");
    let p = slot_plate("run End(tool=T2, surface=Floor, stepover=1.5, strategy=rest)").unwrap();
    let e = gcode::program(&p).unwrap_err();
    assert!(e.contains("no earlier op works it"), "{e}");
}

/// The word's grammar: `rest` is the one strategy an End takes, it takes
/// no voxel cell (there is no field), and it faces a Face or a Plane.
#[test]
fn test_rest_on_a_floor_is_one_word_with_no_field() {
    let e = slot_plate("run End(tool=T1, surface=Floor, stepover=5, cover=80)\nrun End(tool=T2, surface=Floor, stepover=1.5, strategy=rest, voxel_cell=0.5)").unwrap_err();
    assert!(e.contains("no field"), "{e}");
    let e = slot_plate("run End(tool=T1, surface=Floor, stepover=5, cover=80)\nrun End(tool=T2, surface=Floor, stepover=1.5, strategy=regions, voxel_cell=0.5)").unwrap_err();
    assert!(e.contains("strategy=rest"), "{e}");
}

// ---- slice 2: the refusal and the concession name the next op ----------------

/// Fork 5: the refusal ends with the op that would take what the concession
/// leaves, as template text with `T?` (the tool is never guessed), sized by
/// the NARROWEST stretch: the 3mm slot, so a tool up to 3mm reaches all 3.
#[test]
fn test_the_refusal_names_the_rest_op_as_template_text() {
    let p = load("walled_rails_offset.cam");
    let mut p = p;
    p.operations[0].cover = None;
    let err = gcode::program(&p).unwrap_err();
    assert!(
        err.ends_with("add cover=92.7 to its run line, then take what it leaves with `run End(tool=T?, surface=Land, stepover=?, strategy=rest)` — a tool up to 3.000mm reaches all 3"),
        "{err}"
    );
}

/// The program says it too, in a comment a G-code reader can hold (no
/// parentheses inside): the rest op that is coming, or the one that would.
#[test]
fn test_the_concession_note_names_who_takes_the_rest() {
    let g = gcode::program(&load("walled_rails_offset.cam")).unwrap();
    assert!(g.contains("\n(the rest: an End with strategy=rest takes it — a tool up to 3.000mm reaches all 3)\n"), "the note, with no rest op in the program");
    let g = gcode::program(&load("walled_rails_rest_offset.cam")).unwrap();
    assert!(g.contains("\n(the rest: op 2 takes it with T6)\n"), "the note, naming the op that does");
    // One stretch reads as one: the slot plate's 4mm slot.
    let slot = slot_plate("run End(tool=T1, surface=Floor, stepover=5, cover=80)").unwrap();
    let g = gcode::program(&slot).unwrap();
    assert!(g.contains("(the rest: an End with strategy=rest takes it — a tool up to 4.000mm reaches it)"), "one stretch, 4mm wide");
}

/// The skeleton is a line validate accepts once filled — the template pins'
/// own law, for the line the refusal hands out.
#[test]
fn test_the_rest_line_fills_to_a_program_that_emits() {
    let line = nacs::template::rest_line("Floor");
    assert_eq!(line, "run End(tool=T?, surface=Floor, stepover=?, strategy=rest)");
    let filled = line.replace("tool=T?", "tool=T2").replace("=?", "=1.5");
    let p = slot_plate(&format!("run End(tool=T1, surface=Floor, stepover=5, cover=80)\n{filled}")).unwrap();
    gcode::program(&p).unwrap();
}

// ---- slice 3: the recursion ---------------------------------------------------

/// A rest tool too big for one stretch refuses by name — over the WHOLE rest
/// floor, not one piece of it — and names the op after it; `cover=` on a rest
/// op is a share of the rest floor.
#[test]
fn test_a_rest_tool_too_big_for_a_stretch_refuses_over_the_whole_rest_floor() {
    let mut p = load("walled_rails_ladder_offset.cam");
    p.operations[1].cover = None;
    let err = gcode::program(&p).unwrap_err();
    assert!(err.starts_with("End on 'Land' with T7 (5.000mm) cannot reach 1 stretch of its floor, 95.6mm² of 420.7mm² (77.3% reachable): 95.1mm²"), "{err}");
    assert!(err.contains("3.000mm wide") && err.contains("add cover=77.2 to its run line, then take what it leaves with `run End(tool=T?, surface=Land, stepover=?, strategy=rest)` — a tool up to 3.000mm reaches it"), "{err}");
    // …and the concession is checked like any other: it fails the day less is reachable.
    p.operations[1].cover = Some(80.0);
    let err = gcode::program(&p).unwrap_err();
    assert!(err.contains("cover=80 was conceded, but only 77.3%"), "{err}");
}

/// Each rest op reads what the one before it held back, and nothing more:
/// the 5mm tool concedes the 3mm slot WHOLE (no pass of it reaches the
/// slot), and the 3mm tool takes exactly that slot.
#[test]
fn test_the_ladder_hands_each_rest_op_what_the_last_one_left() {
    let p = load("walled_rails_ladder_offset.cam");
    let g = gcode::program(&p).unwrap();
    let t7 = op_text(&g, "(--- floor: Land with T7");
    assert!(t7.contains("(rest 2 of 3: 95.5mm² near (128.000, 34.076) — no pass of T7 reaches it, conceded whole)"), "{t7}");
    assert!(t7.contains("(the rest: op 3 takes it with T6)"), "{t7}");
    let t6 = op_text(&g, "(--- floor: Land with T6");
    assert!(t6.starts_with("(--- floor: Land with T6 — the rest: 1 floor op 2 left standing ---)\n(rest 1 of 1: 95.5mm² near (128.000, 34.076))"), "{t6}");
    // The slot between Fin4 and Fin5 stands until op 3, and only op 3 cuts it.
    let at = |k: usize| nacs::stock::remaining_after(&p, k).material_at(128.0, 34.0, -7.0);
    assert!(at(0) && at(1) && !at(2), "the 3mm slot: standing after ops 1 and 2, gone after op 3");
    // The 8mm slot is op 2's.
    let at = |k: usize| nacs::stock::remaining_after(&p, k).material_at(116.5, 34.0, -7.0);
    assert!(at(0) && !at(1));
}

// ---- slice 4: the report measures the rest floor ------------------------------

/// `nacs --patterns` measures a rest op like any region op — each pattern run
/// through the real emitter over the rest floor — and every pattern cuts all
/// of it. Its lands are its pieces' own (the stretches' widths), its title
/// says what it works, and the default is not moved.
#[test]
fn test_the_report_measures_the_rest_floor_every_pattern_whole() {
    let p = load("walled_rails_rest_offset.cam");
    let reports = nacs::compare::op_reports(&p);
    let r = reports.iter().find(|r| r.op_idx == 1).expect("the rest op has a report");
    assert_eq!(r.title, "op [1] End on 'Land' (T6 dia 3.000) — rest floor: 3 floors op [0] left standing, 420.7mm²");
    let widths: Vec<String> = r.lands.iter().map(|l| format!("{:.3}", l.width)).collect();
    assert_eq!(widths, ["3.000", "5.991", "8.000"], "the three stretches' widths, narrowest first");
    for row in &r.rows {
        let st = row.outcome.as_ref().unwrap_or_else(|e| panic!("{}: {e}", row.pattern.word()));
        assert_eq!(st.uncut_mm2, Some(0.0), "{} leaves rest floor uncut", row.pattern.word());
        assert!(st.worst_deg.is_none(), "the band prices the host's family, not the pieces");
    }
    let current: Vec<_> = r.rows.iter().filter(|row| row.current).map(|row| row.pattern).collect();
    assert_eq!(current, [nacs::parse::Pattern::Offset], "no default moved");
}

// ---- slice 5: the plane concedes ------------------------------------------------

/// A faced Plane's web between two posts was refused by the Phase 26 law
/// ("use a smaller tool, or move one of them") — the one floor Phase 32's
/// concession missed. It is the concession's now: the refusal names the
/// stretch, the `cover=` and the rest op; conceded, the rest op takes the web
/// and only the web.
#[test]
fn test_a_plane_concedes_its_web_and_the_rest_op_takes_it() {
    let mut p = load("deck_rest_offset.cam");
    p.operations[0].cover = None;
    let err = gcode::program(&p).unwrap_err();
    assert!(err.starts_with("End on 'Deck' with T2 (12.000mm) cannot reach 1 stretch of its floor"), "{err}");
    assert!(err.contains("between 'PostB' and 'PostA'") && err.contains("6.000mm wide"), "{err}");
    assert!(err.ends_with("then take what it leaves with `run End(tool=T?, surface=Deck, stepover=?, strategy=rest)` — a tool up to 6.000mm reaches it"), "{err}");
    let p = load("deck_rest_offset.cam");
    let g = gcode::program(&p).unwrap();
    assert!(g.contains("\n(the rest: op 2 takes it with T3)\n"));
    let at = |k: usize| nacs::stock::remaining_after(&p, k).material_at(48.0, 30.0, -4.0);
    assert!(at(0) && !at(1), "the web stands after op 1 and is gone after op 2");
    let rest = op_text(&g, "(--- floor: Deck with T3");
    let cuts = at_depth(&format!("G00 X0 Y0 Z25\n{rest}"), -5.0);
    for (a, _) in &cuts {
        assert!((a.x - 48.0).abs() < 6.0 && (a.y - 30.0).abs() < 8.0, "a rest pass at ({:.3}, {:.3}) is off the web", a.x, a.y);
    }
}
