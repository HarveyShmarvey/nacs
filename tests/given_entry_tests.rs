// ---- the lead-in from given air (Phase 42 slice 4) ---------------------------
//
// `DECISIONS.md` → "Phase 42 slice 4…", fork 4 of the Phase 42 DRAFT. No
// new preference law: "lead in from air where it is in reach" (Phase 27
// slice 2) extends its definition of air from "beyond the blank" to
// "beyond the blank, or in a given void's lens" — the centre set the whole
// body can stand in. The first loop's entry is a plunge INTO that air at
// depth, licensed by the plunge taxonomy (the before-state is void there),
// then the same straight lead into the pass; where no lens reaches, the
// ramp as today, and the note says why. These pins hold the entry's shape
// on the straddling pocket, the ramp it replaced, a bridged given void as
// an entry too, the out-of-reach note, the End's floor pass taking the
// same entry, and the certificate standing behind the plunge.

use nacs::parse::{self, Pt};
use nacs::{gcode, stock};

const HEAD: &str = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\ntool T1 Endmill(dia=4.0)\n";
const DRAIN: &str = "feature Drain TiltedBore(dia=12, extent=20, x=70, y=40, tilt=30, heading=180)\n";
const STRADDLING: &str = "feature Pocket ProfilePocket(extent=8\n    30 20\n    66 20\n    66 60\n    30 60\n)\n";
const RUN: &str = "run Clear(tool=T1, profile=Pocket, stepover=2)\n";

fn program(lines: &str) -> parse::Part {
    parse::parse(&format!("{HEAD}{lines}")).unwrap_or_else(|e| panic!("must parse: {e}"))
}

/// The op's lines after its title, up to the first retract after cutting.
fn first_entry(g: &str) -> Vec<&str> {
    let body: Vec<&str> = g.lines().skip_while(|l| !l.starts_with("(--- ")).skip(1).collect();
    let mut out = Vec::new();
    for l in body {
        // The op's notes, and the retract to clearance that opens every op.
        if l.starts_with('(') || (out.is_empty() && l.starts_with("G00 Z")) {
            continue;
        }
        out.push(l);
        if l.starts_with("G00 Z") && out.len() > 3 {
            break;
        }
    }
    out
}

fn xy(line: &str) -> Option<Pt> {
    let mut x = None;
    let mut y = None;
    for w in line.split_whitespace() {
        if let Some(v) = w.strip_prefix('X') {
            x = v.parse().ok();
        }
        if let Some(v) = w.strip_prefix('Y') {
            y = v.parse().ok();
        }
    }
    Some(Pt { x: x?, y: y? })
}

#[test]
fn test_the_first_loop_plunges_into_the_given_air_at_depth_and_leads_straight_in() {
    let p = program(&format!("{DRAIN}given Drain\n{STRADDLING}{RUN}"));
    let g = gcode::program(&p).unwrap();
    assert!(g.contains("(entered from given air: Drain)\n"), "{}", &g[..800]);
    let entry = first_entry(&g);
    // G00 to the air point, G00 to rapid_z, the plunge at depth, the lead
    // at feed — and no ramp line (an XY move carrying a Z).
    assert!(entry[0].starts_with("G00 X"), "{entry:?}");
    assert_eq!(entry[1], "G00 Z5.000");
    assert_eq!(entry[2], "G01 Z-8.000 F300.000");
    assert!(entry[3].starts_with("G01 X") && entry[3].ends_with("F800.000"), "{entry:?}");
    assert!(!entry.iter().any(|l| l.starts_with("G01 X") && l.contains(" Z")), "no ramp: {entry:?}");
    assert!(!g.contains("G01 Z0.000 F300.000"), "nothing starts a ramp at the stock top");
    // The air point stands in the lens's centre set — the whole body in the
    // bore's air — and the model says void there before the op runs.
    let from = xy(entry[0]).unwrap();
    let lens = &stock::given_air(&p, &p.operations[0])[0];
    assert!(lens.centre.iter().any(|c| stock::point_in_polygon(c, from.x, from.y)), "{from:?} not in {:?}", lens.centre);
    let before = stock::remaining_before(&p, 0);
    assert!(!before.material_at(from.x, from.y, -8.0) && !before.material_at(from.x, from.y, -0.1));
    // The lead lands on the loop and the loop starts there.
    let land = xy(entry[3]).unwrap();
    let next = xy(entry[4]).unwrap();
    assert!((land.x - next.x).hypot(land.y - next.y) > 1e-9, "the loop continues from the landing");
    // Not given, the same pocket ramps in as it always did, and says
    // nothing of air.
    let p = program(&format!("{DRAIN}{STRADDLING}{RUN}"));
    let g = gcode::program(&p).unwrap();
    assert!(!g.contains("given air"), "{}", &g[..600]);
    let entry = first_entry(&g);
    assert_eq!(entry[2], "G01 Z0.000 F300.000", "the ramp starts at the stock top: {entry:?}");
    assert!(entry[3].starts_with("G01 X") && entry[3].contains(" Z-"), "{entry:?}");
}

#[test]
fn test_a_bridged_given_void_is_an_entry_too_and_one_out_of_reach_says_so() {
    // A given bore wholly inside the floor, deeper than it: bridged by the
    // region, and its air is where the first loop enters.
    let p = program(&format!(
        "feature Hole CylinderBore(dia=12, extent=20, x=48, y=40)\ngiven Hole\n{STRADDLING}{RUN}"
    ));
    let g = gcode::program(&p).unwrap();
    assert!(g.contains("(bridged: Hole)\n") && g.contains("(entered from given air: Hole)\n"), "{}", &g[..800]);
    let from = xy(first_entry(&g)[0]).unwrap();
    assert!((from.x - 48.0).hypot(from.y - 40.0) <= 4.0 + 1e-6, "inside the bore less the tool's radius: {from:?}");
    // A given void apart from the pocket: no loop point stands in its air,
    // so no lead reaches, the ramp stays, and the program says why.
    let p = program(&format!(
        "{DRAIN}given Drain\nfeature Pocket ProfilePocket(extent=8\n    10 20\n    50 20\n    50 60\n    10 60\n)\n{RUN}"
    ));
    let g = gcode::program(&p).unwrap();
    assert!(g.contains("(given air of Drain out of reach of every pass — ramped)\n"), "{}", &g[..800]);
    assert!(!g.contains("entered from given air"));
    assert!(g.contains("G01 Z0.000 F300.000"), "the ramp");
}

/// The rows read the given air too (Phase 44 slice 3): under spine, zigzag
/// and oneway a chain whose first stroke starts in the bore's lens is
/// entered there at depth without a ramp — the one `in_air` predicate now
/// reads the lens beside the blank's air — and the program says which void.
/// Every entry is still blessed by the certificate.
#[test]
fn test_the_rows_enter_from_the_given_air_without_a_ramp_and_say_so() {
    // The drain straddles the wall near the pocket's BOTTOM edge, so the
    // lowest row — the one every row pattern enters first — runs into its
    // lens; a drain mid-height is reached only by rows the chain joins
    // without an entry of their own.
    const DRAIN_LOW: &str = "feature Drain TiltedBore(dia=12, extent=20, x=70, y=24, tilt=30, heading=180)
";
    for pattern in ["spine", "zigzag", "oneway"] {
        let p = program(&format!("{DRAIN_LOW}given Drain\n{STRADDLING}run Clear(tool=T1, profile=Pocket, stepover=2, pattern={pattern})\n"));
        let g = gcode::program(&p).unwrap();
        assert!(g.contains("(rows entered from given air: Drain)\n"), "{pattern}: {g}");
        assert!(g.contains("\n(plunge: in the given air of Drain)\n"), "the chain that starts in the lens says so: {pattern}: {g}");
        let (_, emissions) = gcode::program_with_moves(&p).unwrap();
        for em in &emissions {
            assert!(stock::check_moves(&p, em.op_idx, gcode::resolved_moves(em)).is_ok(), "{pattern}");
        }
        // Without the given word the bore stands, the lens is nothing, and
        // the rows ramp into solid as they always did.
        let q = program(&format!("{DRAIN_LOW}{STRADDLING}run Clear(tool=T1, profile=Pocket, stepover=2, pattern={pattern})\n"));
        let h = gcode::program(&q).unwrap();
        assert!(!h.contains("rows entered from given air"), "{pattern}: {h}");
    }
}

#[test]
fn test_the_floor_pass_enters_from_the_same_air_and_every_entry_is_blessed() {
    // The End's floor pass on the OFFSET family takes the same entry; its
    // default, spine, walked rows with an entry rule of its own until Phase
    // 44 slice 3 (the test above); this pin keeps the offset family's.
    let p = program(&format!("{DRAIN}given Drain\n{STRADDLING}{RUN}run End(tool=T1, surface=Pocket, stepover=2, pattern=offset)\n"));
    // `gcode::program` runs `check_moves` over every op's moves before
    // emitting: a program that comes back is one whose every entry the
    // stock model blessed.
    let g = gcode::program(&p).unwrap();
    assert_eq!(g.matches("(entered from given air: Drain)\n").count(), 2, "{g}");
    assert_eq!(g.matches("\n(lead: from the air at (").count(), 2, "each family's lead says where it came from: {g}");
    let (_, emissions) = gcode::program_with_moves(&p).unwrap();
    for em in &emissions {
        assert!(stock::check_moves(&p, em.op_idx, gcode::resolved_moves(em)).is_ok());
    }
}
