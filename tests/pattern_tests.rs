// ---- patterns: how a region is walked ---------------------------------------------
// Phase 28 slice 3 (`DECISIONS.md` → "Phase 28 slice 3: the patterns…").
// `pattern=offset|zigzag|oneway` on any op that works a region. Absent, every
// program cuts what it always cut; present, it is the author's pin — one word of
// program text.

use nacs::{compare, gcode, parse, pattern};

const HEAD: &str = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\ntool T5 Endmill(dia=10.0)\n";

fn load(dir: &str) -> parse::Part {
    let src = std::fs::read_to_string(format!("{dir}/main.part")).unwrap();
    let geom = std::fs::read_to_string(format!("{dir}/geometry.part")).ok();
    parse::parse_project(&src, geom.as_deref()).unwrap_or_else(|e| panic!("{dir}: {e}"))
}

fn words(g: &str) -> Vec<(String, f64, f64, f64)> {
    let (mut x, mut y, mut z) = (f64::NAN, f64::NAN, f64::NAN);
    let mut out = Vec::new();
    for line in g.lines().filter(|l| l.starts_with('G')) {
        for word in line.split_whitespace() {
            let value = || word[1..].parse::<f64>().expect("a number");
            match word.as_bytes()[0] {
                b'X' => x = value(),
                b'Y' => y = value(),
                b'Z' => z = value(),
                _ => {}
            }
        }
        out.push((line.to_string(), x, y, z));
    }
    out
}

#[test]
fn test_the_three_pinned_fixtures_are_golden() {
    for dir in ["wrench_zigzag.cam", "wrench_oneway.cam", "step_zigzag.cam"] {
        let g = gcode::program(&load(dir)).unwrap_or_else(|e| panic!("{dir}: {e}"));
        let golden = std::fs::read_to_string(format!("tests/{}.golden.gcode", dir.trim_end_matches(".cam"))).unwrap();
        assert_eq!(g.trim(), golden.trim(), "{dir}");
    }
}

/// The planner, on a bare rectangle: passes land exactly on both sides
/// (`stepover` is a maximum), a zigzag is ONE chain joined along the boundary,
/// a oneway is one chain per pass all cut the same way.
#[test]
fn test_the_planner_on_a_rectangle() {
    let rect = vec![vec![
        parse::Pt { x: 0.0, y: 0.0 },
        parse::Pt { x: 40.0, y: 0.0 },
        parse::Pt { x: 40.0, y: 10.0 },
        parse::Pt { x: 0.0, y: 10.0 },
    ]];
    let nowhere = |_: parse::Pt| false;
    let z = pattern::plan(true, 0.0, &rect, &[], 3.0, &nowhere, None).unwrap();
    assert_eq!((z.passes, z.chains.len()), (5, 1), "10mm at a 3mm maximum is four steps of 2.5");
    assert!((z.step - 2.5).abs() < 1e-3);
    assert!(z.chains[0].ramp, "nothing cut beside it and no air: the first stroke ramps");
    let o = pattern::plan(false, 0.0, &rect, &[], 3.0, &nowhere, None).unwrap();
    assert_eq!(o.chains.len(), 5);
    assert!(o.chains.iter().all(|c| c.pts[0].x < c.pts[1].x), "every pass the same way");
    assert!(o.chains[0].ramp && o.chains[1..].iter().all(|c| !c.ramp), "later passes plunge beside the one before");
    // Along Y is the same plan TURNED a quarter (Phase 38 slice 0: one frame
    // for every pattern, spine's), not swapped: its first row is the one at
    // the far side in x, and its passes run along y.
    let tall: Vec<Vec<parse::Pt>> = vec![rect[0].iter().map(|q| parse::Pt { x: q.y, y: q.x }).collect()];
    let y = pattern::plan(true, 90.0, &tall, &[], 3.0, &nowhere, None).unwrap();
    assert_eq!((y.passes, y.angle_deg), (5, 90.0));
    assert!((y.chains[0].pts[0].x - (10.0 - z.chains[0].pts[0].y)).abs() < 1e-9, "{:?}", y.chains[0].pts[0]);
    assert!((y.chains[0].pts[0].x - y.chains[0].pts[1].x).abs() < 1e-9 && y.chains[0].pts[1].y > y.chains[0].pts[0].y);
}

/// A step, walked from the OPEN side: the first pass is a plunge in air a radius
/// outside the blank (since Phase 58 slice 1 its row runs the whole open side,
/// ending at the chorded arc the open corner's sector erodes to — a hair
/// short of y 0), the wall (x=60) gets the last parallel pass and then the
/// shore pass along it — the wall alone, not a lap of the step (Phase 40 slice
/// 1) — and nothing ramps.
#[test]
fn test_a_step_is_walked_from_its_open_side_toward_its_wall() {
    let g = gcode::program(&load("step_zigzag.cam")).unwrap();
    assert!(g.contains("(pattern: zigzag along Y, 11 passes at 4.000, then 1 shore pass along what stands)"));
    assert!(g.contains("G00 X105.000 Y0.195\nG00 Z5.000\nG01 Z-5.000 F300.000\n"), "{}", &g[..700]);
    assert!(!g.contains("G01 Z0.000"), "no ramp: air is where it starts");
    let cuts: Vec<_> = words(&g).into_iter().filter(|w| w.3 < -1e-9).collect();
    assert!(cuts.iter().all(|w| w.1 >= 65.0 - 1e-3), "a radius off the wall, always");
    // The passes' X positions, in the order they are first cut: from the open
    // side (105, a radius outside the blank) down to the wall's stand-off (65).
    let mut order: Vec<f64> = Vec::new();
    for w in &cuts {
        if order.last().map_or(true, |last| (w.1 - last).abs() > 1e-6) && !order.iter().any(|x| (x - w.1).abs() < 1e-6) {
            order.push(w.1);
        }
        if (w.1 - 65.0).abs() < 1e-3 {
            break;
        }
    }
    // (Between passes the links follow the region's boundary, which rounds the
    // blank's corners — those X's are in the list too, and they too descend.)
    assert!(order.windows(2).all(|p| p[1] < p[0]), "open side first, wall last: {order:?}");
    assert_eq!((order[0], *order.last().unwrap()), (105.0, 65.0));
    let passes: Vec<f64> = (0..11).map(|k| 105.0 - 4.0 * k as f64).collect();
    assert!(passes.iter().all(|x| order.iter().any(|o| (o - x).abs() < 1e-3)), "{order:?}");
}

/// No pin, no change — and an explicit `pattern=offset` is the default said out
/// loud. A pattern belongs on an op that works a region, and not beside
/// `strategy=`.
#[test]
fn test_no_pin_no_change_and_a_pattern_belongs_on_a_region() {
    let src = std::fs::read_to_string("pocket_post.cam/main.part").unwrap();
    let pinned = src.replace("stepover=4.0)", "stepover=4.0, pattern=offset)");
    assert_eq!(gcode::program(&parse::parse(&pinned).unwrap()).unwrap(), gcode::program(&parse::parse(&src).unwrap()).unwrap());

    let e = parse::parse(&src.replace("stepover=4.0)", "stepover=4.0, pattern=spiral)")).unwrap_err();
    assert!(e.contains("unknown pattern 'spiral'") && e.contains("nacs --patterns"), "{e}");
    let e = parse::parse(&format!(
        "{HEAD}feature Post CylinderBoss(dia=20, extent=5, x=50, y=40, z=-5)\nrun Side(tool=T5, surface=Post, stepover=5.0, pattern=zigzag)\n"
    ))
    .unwrap_err();
    assert!(e.contains("belongs on an End, or on a Clear of a ProfilePocket"), "{e}");
    let e = parse::parse(&src.replace("stepover=4.0)", "stepover=4.0, pattern=zigzag, strategy=regions, voxel_cell=0.5)")).unwrap_err();
    assert!(e.contains("pattern=zigzag with strategy="), "{e}");
}

/// A pocket with a post, in zigzag: the passes stop a radius short of the post
/// and of the walls, and the shore goes round both — two closed shores, since
/// both stand all the way round. (Along
/// Y since Phase 38 slice 0: the pocket is square and the post is centred, but
/// the price reads the chorded loops, and the two axes are 0.04mm of 736
/// apart. Not a tie to the price, so the price decides — this is a test's
/// edit, not a shipped program.)
#[test]
fn test_a_zigzag_pocket_closes_along_its_wall_and_its_island() {
    let src = std::fs::read_to_string("pocket_post.cam/main.part").unwrap().replace("stepover=4.0)", "stepover=4.0, pattern=zigzag)");
    let p = parse::parse(&src).unwrap();
    let g = gcode::program(&p).expect("generates under check_moves");
    assert!(g.contains("(islands: Post)\n(pattern: zigzag along Y, 14 passes at 3.846, then 2 shore passes along what stands)"), "{}", &g[..500]);
    for (line, x, y, z) in words(&g) {
        if z < -1e-9 {
            let rho = ((x - 50.0).powi(2) + (y - 50.0).powi(2)).sqrt();
            assert!(rho >= 11.0 - nacs::CHORD_TOL_MM - 1e-3, "`{line}`: {rho:.3} from the post");
            assert!(x >= 25.0 - 1e-3 && x <= 75.0 + 1e-3 && y >= 25.0 - 1e-3 && y <= 75.0 + 1e-3, "`{line}`");
        }
    }
    // No ridge law between parallel lines: 6mm on a 10mm tool is legal here,
    // where the offset family around the same post refuses it.
    let wide = src.replace("stepover=4.0", "stepover=6.0");
    gcode::program(&parse::parse(&wide).unwrap()).expect("no ridge between parallel passes");
    let offset = wide.replace(", pattern=zigzag", "");
    assert!(gcode::program(&parse::parse(&offset).unwrap()).unwrap_err().contains("more than T5's radius"));
}

// ---- slice 4: the report ---------------------------------------------------------

/// The engine shows its options; it does not choose. On the wrench: three rows,
/// the default marked, every row's clock the clock's own number for the program
/// that pattern really emits, the at-depth shares adding to the whole, and the
/// lands a person would name — the ring's wall, the jaws, the shank — with the
/// passes each pattern takes across them.
#[test]
fn test_the_report_lays_the_patterns_side_by_side_and_agrees_with_the_programs() {
    let p = load("wrench_offset.cam");
    let reports = compare::op_reports(&p);
    assert_eq!(reports.len(), 1);
    let r = &reports[0];
    assert_eq!(r.rows.iter().map(|row| (row.pattern.word(), row.current)).collect::<Vec<_>>(), [("offset", true), ("zigzag", false), ("oneway", false), ("spine", false), ("held", false)]);

    let widths: Vec<f64> = r.lands.iter().map(|l| (l.width * 10.0).round() / 10.0).collect();
    assert_eq!(widths.first(), Some(&7.9), "the ring's wall, a chord short of 8: {widths:?}");
    assert_eq!(widths.last(), Some(&14.0), "the shank: {widths:?}");
    assert!(widths.iter().all(|w| *w < 20.0), "every land listed is under two tool diameters: {widths:?}");

    for (row, dir) in r.rows.iter().zip(["wrench_offset.cam", "wrench_zigzag.cam", "wrench_oneway.cam"]) {
        let st = row.outcome.as_ref().unwrap_or_else(|e| panic!("{}: {e}", row.pattern.word()));
        // The row IS the pinned fixture's program: same clock, to the second.
        let clock = nacs::cut_time::time_report(&load(dir)).unwrap();
        assert!((clock.rows[0].cutting_mm - st.cutting_mm).abs() < 1e-6, "{dir}: {} vs {}", clock.rows[0].cutting_mm, st.cutting_mm);
        assert!((clock.rows[0].cutting_s - st.cutting_s).abs() < 1e-6);
        let (floor, other, air) = st.over.expect("the stock model answers before op 1");
        assert!(floor > 0.0 && floor + other + air <= st.cutting_mm + 1e-6, "the at-depth shares are part of the cutting length");
        assert_eq!(st.land_passes.len(), r.lands.len());
        assert!(st.land_passes.iter().all(|n| *n >= 1), "every land is crossed by something: {:?}", st.land_passes);
    }
    // The point of the phase, as a number: the parallel passes spend far more
    // of their length on the floor than the loops do.
    let share = |i: usize| {
        let (f, o, a) = r.rows[i].outcome.as_ref().unwrap().over.unwrap();
        f / (f + o + a)
    };
    assert!(share(1) > share(0) + 0.2, "zigzag {:.2} vs offset {:.2}", share(1), share(0));

    let text = compare::render(&p);
    assert!(text.starts_with("patterns — what each would cut, measured; nothing here is a score, and the engine does not choose\n"));
    assert!(text.contains("  * offset ") && text.contains("    zigzag ") && text.contains("`pattern=zigzag`"), "{text}");
    // Phase 34 slice 0: the floor's shape, said once per op above its lands.
    assert!(text.contains("\n  floor: 4243mm², mean width 12.051mm (1.21 tool diameters), 0% wall\n  land 1: "), "{text}");
}

/// A pattern that would be refused says so in its own row, in the emitter's
/// own sentence — and an op that works no region has no rows at all.
#[test]
fn test_a_refused_pattern_is_a_row_and_a_project_with_no_region_says_so() {
    let src = std::fs::read_to_string("pocket_post.cam/main.part").unwrap().replace("stepover=4.0)", "stepover=6.0, pattern=zigzag)");
    let p = parse::parse(&src).unwrap();
    let r = &compare::op_reports(&p)[0];
    assert!(r.rows[1].current, "the pin is what it cuts today");
    assert!(r.rows[0].outcome.as_ref().unwrap_err().contains("more than T5's radius"), "{:?}", r.rows[0].outcome);
    assert!(r.rows[1].outcome.is_ok());
    assert!(compare::render(&p).contains("  offset    refused — "));

    let none = parse::parse(&format!(
        "{HEAD}feature Post CylinderBoss(dia=20, extent=5, x=50, y=40, z=-5)\nrun Side(tool=T5, surface=Post, stepover=5.0)\n"
    ))
    .unwrap();
    assert!(compare::render(&none).starts_with("patterns: no op in this project works a region"));
}

/// Phase 30 slice 1 — the `uncut` column can SEE. Every shipped row reads
/// 0.0mm², and a column that has only ever said zero is a green-but-blind
/// instrument until it is shown a floor with a stripe missing: one pass down
/// the middle of a 40mm square leaves the two 15mm bands beside it, less the
/// corners no 10mm tool reaches… which here is nothing, because the allowed
/// region is the whole square (every edge open).
#[test]
fn test_the_uncut_column_sees_a_missing_stripe_and_forgives_an_unreachable_corner() {
    use nacs::parse::Pt;
    let sq = |a: f64, b: f64| vec![Pt { x: a, y: a }, Pt { x: b, y: a }, Pt { x: b, y: b }, Pt { x: a, y: b }];
    let floor = vec![sq(0.0, 40.0)];
    let one_pass = vec![(Pt { x: -5.0, y: 20.0 }, Pt { x: 45.0, y: 20.0 })];
    let open = compare::uncut(&floor, &[sq(-5.0, 45.0)], &[], &one_pass, 5.0, &|_| true);
    assert!((open - 1200.0).abs() < 2.0 * 80.0 * compare::UNCUT_CELL, "two 15x40 bands: {open}");

    // Walled all round, the tool's centre stays 5mm in. A lap of that square
    // plus a cross through the middle reaches everything reachable: what is
    // left in the four corners (r² − πr²/4 each) was never a 10mm tool's.
    let lap = sq(5.0, 35.0);
    let mut moves: Vec<(Pt, Pt)> = (0..4).map(|i| (lap[i], lap[(i + 1) % 4])).collect();
    for k in 1..6 {
        let y = 5.0 + 5.0 * k as f64;
        moves.push((Pt { x: 5.0, y }, Pt { x: 35.0, y }));
    }
    assert_eq!(compare::uncut(&floor, &[lap.clone()], &[], &moves, 5.0, &|_| true), 0.0);
    // …and the same lap alone leaves the middle: 20x20.
    let lap_only = compare::uncut(&floor, &[lap], &[], &moves[..4], 5.0, &|_| true);
    assert!((lap_only - 400.0).abs() < 2.0 * 80.0 * compare::UNCUT_CELL, "{lap_only}");
    // Floor that held no stock when the op began is nobody's to cut either.
    assert_eq!(compare::uncut(&floor, &[sq(-5.0, 45.0)], &[], &one_pass, 5.0, &|_| false), 0.0);
}

// ---- Phase 34 slice 1: the wide floors ------------------------------------------

fn golden(dir: &str) {
    let p = load(&format!("{dir}.cam"));
    let want = std::fs::read_to_string(format!("tests/{dir}.golden.gcode")).unwrap();
    assert_eq!(gcode::program(&p).unwrap().trim(), want.trim(), "{dir}");
}

fn seconds(r: &compare::OpReport) -> Vec<f64> {
    r.rows.iter().map(|row| row.outcome.as_ref().unwrap_or_else(|e| panic!("{}: {e}", row.pattern.word())).cutting_s).collect()
}

/// A floor with no rails in it: twenty tool diameters of mean width, no wall,
/// nothing standing. Every fixture before it was under five. Here the four
/// patterns land within a tenth of each other — spine's lead on the rail plates
/// is 2.5x — and every one of them leaves nothing uncut. The report's claim
/// about a wide floor is that the pattern barely matters to the clock.
#[test]
fn test_on_an_open_field_the_four_patterns_land_within_a_tenth_of_each_other() {
    golden("open_field");
    let p = load("open_field.cam");
    let r = &compare::op_reports(&p)[0];
    let shape = r.shape.expect("the field has a shape");
    assert_eq!((shape.mean_width() / r.tool_dia, shape.wall), (20.0, 0.0));
    // Unpinned, so it cuts the default; and on a bare plane the first
    // raster is a fifth row, legal here and nowhere else.
    let current: Vec<&str> = r.rows.iter().filter(|row| row.current).map(|row| row.pattern.word()).collect();
    assert_eq!(current, ["spine"]);
    assert_eq!(r.rows.last().unwrap().pattern.word(), "raster");
    let s = seconds(r);
    let (lo, hi) = (s.iter().cloned().fold(f64::INFINITY, f64::min), s.iter().cloned().fold(0.0, f64::max));
    assert!(hi / lo < 1.10, "offset, zigzag, oneway, spine: {s:?}");
    assert!(r.rows.iter().all(|row| row.outcome.as_ref().unwrap().uncut_mm2 == Some(0.0)));
}

/// A wide pocket of twelve posts: all wall, eleven diameters of mean width.
/// Nothing is narrow, so no pattern is out of its domain, and on the clock
/// spine and the offset family are within a tenth. What differs is what each
/// does AROUND things, and the report has columns for it: the offset family
/// breaks into nests and lifts four times as often, spine turns half again as
/// often. Neither column is a score; both are why a machinist might pin.
#[test]
fn test_in_a_field_of_posts_the_clock_ties_and_the_lifts_and_turns_do_not() {
    golden("post_field");
    let p = load("post_field.cam");
    let r = &compare::op_reports(&p)[0];
    let shape = r.shape.expect("the yard has a shape");
    assert!(shape.mean_width() / r.tool_dia > 10.0 && (shape.wall_share() - 1.0).abs() < 1e-9, "{shape:?}");
    let st = |k: usize| r.rows[k].outcome.as_ref().unwrap();
    let (offset, spine) = (st(0), st(3));
    assert!(r.rows[0].current && r.rows[3].pattern.word() == "spine");
    // The clock tied within 10% until Phase 44 slice 1; now the offset
    // family pays the ramp's law round every post loop — twenty of them,
    // each too short for 3° in one lap — and reads 1297s against spine's
    // 1115s. The law is the price; the report prints it, nothing chooses by it.
    assert!(offset.cutting_s > spine.cutting_s && offset.cutting_s < 1.25 * spine.cutting_s, "offset {:.0}s, spine {:.0}s", offset.cutting_s, spine.cutting_s);
    assert!(offset.lifts > 4 * spine.lifts, "offset {} lifts, spine {}", offset.lifts, spine.lifts);
    assert!(spine.turns * 2 > offset.turns * 3, "spine {} turns, offset {}", spine.turns, offset.turns);
    assert!(r.rows.iter().all(|row| row.outcome.as_ref().unwrap().uncut_mm2 == Some(0.0)));
}

// ---- Phase 34 slice 2: what the sweep found, pinned where it found it ------------
// (`tests/sweep.rs`, `DECISIONS.md` → "Phase 34 slice 2…"). Each of these
// states TODAY'S truth about a defect that is filed and not fixed. The day
// one is fixed its pin fails, and is rewritten to say the floor is covered.

fn uncut_of(r: &compare::OpReport, word: &str) -> f64 {
    let row = r.rows.iter().find(|row| row.pattern.word() == word).unwrap();
    row.outcome.as_ref().unwrap_or_else(|e| panic!("{word}: {e}")).uncut_mm2.expect("the stock model answers")
}

/// A, closed (Phase 37 slice 3). Beside a clamp that BITES the blank's edge
/// the rasters left floor — 55.7mm² under zigzag, in the shipped fixture's own
/// report since the `uncut` column landed, and no gate read that row. The
/// closing pass walked island loops, and the wall loop only on a host with a
/// wall; a clamp across the edge is a bite out of the wall loop, and a bare
/// plane has no wall. What was left was a strip beside each side of the clamp
/// that runs with the passes. The closing pass now walks the stretch of the
/// wall loop the bite put there, and only that stretch. Since Phase 40 slice 1
/// that stretch is spine's shore from the one home — the clamp's sides are
/// standing edges — and the program is byte for byte what it was.
#[test]
fn test_the_rasters_close_beside_a_clamp_that_bites_the_blank() {
    let p = load("floor_clamped_offset.cam");
    let r = &compare::op_reports(&p)[0];
    for word in ["offset", "zigzag", "oneway", "spine"] {
        assert_eq!(uncut_of(r, word), 0.0, "{word}");
    }
    let mut pinned = p.clone();
    pinned.operations[0].pattern = Some(parse::Pattern::Zigzag);
    let g = gcode::program(&pinned).unwrap();
    assert!(g.contains("(pattern: zigzag along X, 15 passes at 5.000, then 1 shore pass along what stands)"), "{}", g.lines().find(|l| l.starts_with("(pattern")).unwrap_or(""));
    // The stretch, not the loop: it starts and ends where the bitten loop
    // parts from the unbitten one — on x = 65, a radius outside the blank —
    // and in between it never leaves the clamp's side of the face.
    let moves = compare::floor_moves(&p, 0, parse::Pattern::Zigzag).unwrap();
    let k = moves.iter().rposition(|(a, _)| (a.x - 65.0).abs() < 1e-3 && a.y < 20.0).expect("the stretch starts on the unbitten loop");
    let stretch = &moves[k..];
    assert!((stretch.last().unwrap().1.x - 65.0).abs() < 1e-3, "and ends on it");
    assert!(stretch.iter().all(|(a, b)| a.x >= 42.5 - 1e-3 && b.x >= 42.5 - 1e-3 && a.y > 12.0 && b.y < 48.0), "a lap of the blank would leave the clamp's side");
    let len: f64 = stretch.iter().map(|(a, b)| (b.x - a.x).hypot(b.y - a.y)).sum();
    assert!(len > 70.0 && len < 85.0, "two short sides and the long one, about 77mm: {len:.1}mm");
}

/// B, closed where it was what it mostly was (Phase 37 slice 2). It was filed
/// as "past half the tool the offset family leaves its corners", and on a
/// plain face it was neither: the family steps inward until the next offset
/// prunes empty, and at 7mm on a 10mm tool its last loop stops 2mm short of
/// the middle of a 60mm blank — a 4 x 4mm square, 16.0mm², emitted exit 0.
/// At 7.5 and 8 the levels land and nothing is left. The bare plane now
/// refuses the middle in the pocket's own sentence, and the corner — real
/// only past `r(1 + 1/sqrt 2)`, 85% of the tool — in its own.
#[test]
fn test_the_offset_family_on_a_bare_plane_refuses_the_middle_and_the_corner_it_would_leave() {
    let src = std::fs::read_to_string("floor_work_offset.cam/main.part").unwrap();
    let at = |stepover: &str| -> Result<f64, String> {
        let p = parse::parse(&src.replace("stepover=6.0", &format!("stepover={stepover}"))).unwrap();
        let r = &compare::op_reports(&p)[0];
        let row = r.rows.iter().find(|row| row.pattern.word() == "offset").unwrap();
        row.outcome.as_ref().map(|st| st.uncut_mm2.expect("the stock model answers")).map_err(|e| e.clone())
    };
    for covers in ["5.0", "6.0", "6.5", "7.5", "8.0", "8.5"] {
        assert_eq!(at(covers), Ok(0.0), "at {covers}");
    }
    let middle = at("7.0").unwrap_err();
    assert!(middle.contains("'Top' leaves an uncut island") && middle.contains("reaches 28.000mm from the stock's edge"), "{middle}");
    let corner = at("8.6").unwrap_err();
    assert!(corner.contains("round the corner at (0.000, 0.000)") && corner.contains("reduce stepover to 8.535 or less"), "{corner}");
    // Both at once: the middle is said first, as the pocket says it.
    assert!(at("9.0").unwrap_err().contains("leaves an uncut island"));
}

/// The middle is asked a hair beyond the reach. `face_boss_offset`'s skim —
/// 8mm on a 12mm tool across an 80mm blank — puts its last pass 40.000mm in
/// of 40: the centreline, a strip with no width, which the offsetter still
/// hands back as a loop. Asked at the reach itself, the law refused a
/// program that leaves nothing.
#[test]
fn test_a_last_pass_that_lands_on_the_centreline_leaves_no_middle() {
    let r = &compare::op_reports(&load("face_boss_offset.cam"))[0];
    assert_eq!(uncut_of(r, "offset"), 0.0);
}

/// B, the last of it, closed (ruled 2026-10-01; `DECISIONS.md` → "Phase 37
/// rulings…"). A wall that points INTO the floor — the L-pocket's inside
/// corner, the lawn's bed — pinches the offset family the way an island does:
/// past the pinch the next level is further away than its stepover, and with
/// a stepover over the tool's radius, floor between one level's reach and the
/// next level was stranded there (1.3mm² at the mouth of the L's narrow arm,
/// 7mm on a 10mm tool, exit 0). The ridge law forbade exactly this around an
/// ISLAND and was never asked of a wall. It is now, naming the corner.
#[test]
fn test_the_ridge_law_is_asked_of_a_wall_that_points_into_the_floor() {
    let offset_row = |p: &parse::Part| -> Result<f64, String> {
        let r = &compare::op_reports(p)[0];
        let row = r.rows.iter().find(|row| row.pattern.word() == "offset").unwrap();
        row.outcome.as_ref().map(|st| st.uncut_mm2.expect("the stock model answers")).map_err(|e| e.clone())
    };
    // The L: a closed pocket, its inside corner at (30, 50).
    let l = std::fs::read_to_string("chamfer_profile.cam/main.part").unwrap();
    let l_at = |stepover: &str| offset_row(&parse::parse(&l.replace("run Clear(tool=T5, profile=Cavity, stepover=4.0)", &format!("run Clear(tool=T5, profile=Cavity, stepover={stepover})"))).unwrap());
    assert_eq!(l_at("5.0"), Ok(0.0), "at the radius nothing is stranded");
    let why = l_at("7.0").unwrap_err();
    assert!(why.contains("pocket feature 'Cavity': its boundary turns into the floor at (30.000, 50.000)"), "{why}");
    assert!(why.contains("its stepover (7.000) is more than T5's radius (5.000)") && why.contains("reduce stepover to 5.000 or less"), "{why}");
    // The lawn: a `Face` whose bed bulges into it. At 7mm on its 10mm tool the
    // old middle check refused it for a centreline of no width, and behind
    // that refusal 3.6mm² was stranded on the bed's ridge. It is refused for
    // the ridge now, which is why the middle check may ask a hair past its
    // reach in every arm.
    let lawn = std::fs::read_to_string("lawn_fence_bed_offset.cam/main.part").unwrap();
    let lawn_at = |stepover: &str| offset_row(&parse::parse(&lawn.replace("stepover=5.0", &format!("stepover={stepover}"))).unwrap());
    assert_eq!(lawn_at("5.0"), Ok(0.0));
    let why = lawn_at("7.0").unwrap_err();
    assert!(why.contains("Face feature 'Lawn': its boundary turns into the floor at") && why.contains("T5's radius (5.000)"), "{why}");
    // A pocket whose wall never turns in is not asked: 7 on a 10mm tool in a
    // square pocket is a stepover the family covers.
    let square = format!("{HEAD}feature P ProfilePocket(extent=5\n10 10\n70 10\n70 70\n10 70\n)\nrun Clear(tool=T5, profile=P, stepover=6.0)");
    assert_eq!(offset_row(&parse::parse(&square).unwrap()), Ok(0.0));
}

/// C, closed (Phase 35 slice 3). Under a 6mm tool spine enters the lawn at
/// x 133, a radius past the floor's open edge, and the tool's own edge lies
/// at 136 — exactly on the swath's, a diameter past. The blank runs on to
/// 140, so there is stock beyond, and the tool TOUCHES it. The gate used to
/// refuse that tangency; it was the gate that was wrong, and the plan emits.
#[test]
fn test_spine_enters_a_radius_past_an_open_edge_with_stock_beyond_it() {
    for (dia, stepover) in [("4.0", "2.0"), ("6.0", "3.0")] {
        let src = std::fs::read_to_string("lawn_fence_bed_offset.cam/main.part").unwrap().replace("Endmill(dia=10.0)", &format!("Endmill(dia={dia})")).replace("stepover=5.0", &format!("stepover={stepover}"));
        let r = &compare::op_reports(&parse::parse(&src).unwrap())[0];
        for word in ["offset", "spine"] {
            assert_eq!(uncut_of(r, word), 0.0, "{word} under a {dia}mm tool");
        }
    }
}

/// …and the tangency is all that was let through: the same entry a tenth of
/// a millimetre further out is IN the stock beyond the swath, and refuses.
#[test]
fn test_an_entry_past_the_swath_still_refuses() {
    let src = std::fs::read_to_string("lawn_fence_bed_offset.cam/main.part").unwrap().replace("Endmill(dia=10.0)", "Endmill(dia=6.0)");
    let p = parse::parse(&src).unwrap();
    let moves = |x: &str| vec!["G00 Z25.000".to_string(), format!("G00 X{x} Y30.000"), "G01 Z-4.000 F300.000".to_string()];
    assert!(nacs::stock::check_moves(&p, 0, &moves("133.000")).is_ok());
    let err = nacs::stock::check_moves(&p, 0, &moves("133.100")).unwrap_err();
    assert!(err.contains("drives the tool into material that outlives this operation"), "{err}");
}

// ---- Phase 34 slice 4: what the tool met ------------------------------------------

fn load_of(r: &compare::OpReport, word: &str) -> compare::Load {
    let row = r.rows.iter().find(|row| row.pattern.word() == word).unwrap();
    row.outcome.as_ref().unwrap_or_else(|e| panic!("{word}: {e}")).load.expect("the walk answers")
}

/// The textbook, in the report: `plate.cam` is faced at 6mm under a 10mm
/// tool, and `acos(1 - 6/5)` is 101.54°. Every pattern's heaviest cut is
/// that, to a hundredth, and none of them ever slots — a face is entered
/// from the air beside it. On the open field, at half the tool, the same
/// identity reads 90° and nothing is past it.
#[test]
fn test_the_load_columns_read_the_textbook_on_a_plain_face() {
    let r = &compare::op_reports(&load("plate.cam"))[0];
    for word in ["offset", "zigzag", "oneway", "spine"] {
        let l = load_of(r, word);
        assert!((l.worst.to_degrees() - 101.54).abs() < 0.01, "{word}: {:.2}", l.worst.to_degrees());
        assert_eq!(l.slot_mm, 0.0, "{word}");
        assert!(l.past_half_mm > 0.75 * l.engaged_mm, "{word}: most of a 60% stepover is past half immersion");
    }
    let field = &compare::op_reports(&load("open_field.cam"))[0];
    for word in ["offset", "zigzag", "oneway", "spine"] {
        let l = load_of(field, word);
        assert!((l.worst.to_degrees() - 90.0).abs() < 0.01 && l.past_half_mm == 0.0 && l.slot_mm == 0.0, "{word}: {l:?}");
    }
    assert!(compare::render(&load("plate.cam")).contains("past 90°         slot worst (band)"));
}

/// What spine's clock costs, in the column beside it. On the rail plate spine
/// cuts the land in 133s against zigzag's 319s, and does it in ONE pass down
/// each rail: a full-width cut through the scrap either side. Two thirds of
/// spine's cutting is a slot; a twentieth of zigzag's is. Neither number is a
/// score. Together they are the choice.
#[test]
fn test_on_the_rails_spine_is_quick_and_most_of_it_is_a_slot() {
    let r = &compare::op_reports(&load("rail_plate_offset.cam"))[0];
    let (spine, zigzag) = (load_of(r, "spine"), load_of(r, "zigzag"));
    assert!(spine.slot_mm / spine.engaged_mm > 0.6, "spine {spine:?}");
    assert!(zigzag.slot_mm / zigzag.engaged_mm < 0.1, "zigzag {zigzag:?}");
    assert!(spine.slot_mm > 4.0 * zigzag.slot_mm);
    let s = seconds(r);
    assert!(s[3] < 0.5 * s[1], "and spine is still under half the clock: {s:?}");
}

/// What an op cuts has ONE home (Phase 35 slice 0): the pin where there is
/// one; without one, spine on a `Floor` and the offset family on a `Clear`
/// (slice 4); `None` on an op that works no region.
#[test]
fn test_what_an_op_cuts_has_one_home() {
    let what = |dir: &str, idx: usize| {
        let p = load(dir);
        parse::derive_pattern(&p, &p.operations[idx])
    };
    assert_eq!(what("plate.cam", 0), Some(parse::Pattern::Spine), "an unpinned End: the default");
    assert_eq!(what("plate_raster.cam", 0), Some(parse::Pattern::Raster), "the first raster, by its word");
    assert_eq!(what("plate.cam", 1), Some(parse::Pattern::Spine), "the notch's Clear, pinned to spine (Phase 53 slice 1)");
    assert_eq!(what("plate.cam", 2), None, "a Side works no region");
    assert_eq!(what("face_boss_offset.cam", 1), Some(parse::Pattern::Offset), "a plane with a boss through it");
    assert_eq!(what("deck_rest_offset.cam", 1), Some(parse::Pattern::Offset), "the rest floor");
    assert_eq!(what("pocket_post.cam", 0), Some(parse::Pattern::Offset), "an unpinned Clear: the offset family");
    assert_eq!(what("wrench_offset.cam", 0), Some(parse::Pattern::Offset), "a Face, pinned");
    assert_eq!(what("wrench_spine.cam", 0), Some(parse::Pattern::Spine), "a pin is a pin");
}

/// The op's retired word refuses, naming the word that replaced it (Phase 35
/// slice 2): never an alias.
#[test]
fn test_the_retired_op_word_refuses_and_names_its_successor() {
    let err = parse::parse(&format!("{HEAD}feature F Plane(z=0)\nrun Facing(tool=T5, surface=F, stepover=5)")).unwrap_err();
    assert!(err.contains("the Facing op is retired") && err.contains("run End("), "{err}");
}

/// `pattern=raster` sweeps the whole blank and reads no region (Phase 35
/// slice 4), so it is legal on a bare plane and refuses anywhere something
/// stands — by name, with the pattern that does the same job on a region.
#[test]
fn test_the_raster_belongs_on_a_bare_plane_and_nowhere_else() {
    let bare = parse::parse(&format!("{HEAD}feature F Plane(z=0)\nrun End(tool=T5, surface=F, stepover=5, pattern=raster)")).unwrap();
    assert!(gcode::program(&bare).unwrap().contains("G01 X105.000 F800.000"));
    let boss = "feature Post CylinderBoss(dia=12, extent=5, x=50, y=40, z=-5)\n";
    let err = parse::parse(&format!("{HEAD}{boss}feature Deck Plane(z=-5)\nrun End(tool=T5, surface=Deck, stepover=5, pattern=raster)")).unwrap_err();
    assert!(err.contains("pattern=raster on 'Deck'") && err.contains("pattern=zigzag"), "{err}");
    // The report shows its row where it is legal and only there.
    let words = |dir: &str| compare::op_reports(&load(dir))[0].rows.iter().map(|r| r.pattern.word()).collect::<Vec<_>>();
    assert_eq!(words("plate.cam"), ["offset", "zigzag", "oneway", "spine", "held", "raster"]);
    assert_eq!(words("wrench_offset.cam"), ["offset", "zigzag", "oneway", "spine", "held"]);
}

/// One op's report is the whole report's row for it (2026-09-29): the window
/// asks for the selected op alone, so it must not read differently from the
/// CLI's table — on a rest-floor program, where each op depends on the ops
/// before it, as much as on a flat one.
#[test]
fn test_one_ops_report_is_the_whole_reports_row_for_it() {
    for dir in ["deck_rest_offset.cam", "sphere.cam", "plate.cam"] {
        let p = nacs::parse::load_project(dir).unwrap();
        let all = nacs::compare::op_reports(&p);
        for idx in 0..p.operations.len() {
            let one = nacs::compare::op_report(&p, idx);
            let row = all.iter().find(|r| r.op_idx == idx);
            assert_eq!(one.map(|r| r.title), row.map(|r| r.title.clone()), "{dir} op {idx}");
            assert_eq!(
                nacs::compare::op_report(&p, idx).map(|r| r.rows.iter().map(nacs::compare::row_line).collect::<Vec<_>>()),
                row.map(|r| r.rows.iter().map(nacs::compare::row_line).collect::<Vec<_>>()),
                "{dir} op {idx}"
            );
        }
        assert!(nacs::compare::op_report(&p, p.operations.len()).is_none(), "past the end is nothing");
    }
}

// ---- Phase 37 slice 0: the declaration clamp, judged by MOVES --------------
// (`DECISIONS.md` → "Phase 37 slice 0…"). The clamps before this one read
// emitted POINTS, and a chord across an arc has two legal ends and an illegal
// middle: `chamfer_profile.cam` shipped a `Clear` that cut 1.46mm into its
// pocket's inside corner, and every point of it stood a radius off the wall.

/// Every fixture directory, parsed as a project somebody typed.
fn corpus() -> Vec<(String, parse::Part)> {
    let mut dirs: Vec<String> =
        std::fs::read_dir(".").unwrap().filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().to_string())).filter(|n| n.ends_with(".cam")).collect();
    dirs.sort();
    dirs.into_iter()
        .filter_map(|dir| {
            let main = std::fs::read_to_string(format!("{dir}/main.part")).ok()?;
            let geom = std::fs::read_to_string(format!("{dir}/geometry.part")).ok();
            let p = parse::parse_project_with_base(&main, geom.as_deref(), Some(std::path::Path::new(&dir))).unwrap_or_else(|e| panic!("{dir}: {e}"));
            Some((dir, p))
        })
        .collect()
}

/// The least distance between two segments: zero where they cross, and
/// otherwise attained at an end of one of them.
fn segment_gap(a: (parse::Pt, parse::Pt), b: (parse::Pt, parse::Pt)) -> f64 {
    let side = |p: parse::Pt, q: parse::Pt, s: parse::Pt| (q.x - p.x) * (s.y - p.y) - (q.y - p.y) * (s.x - p.x);
    if side(a.0, a.1, b.0) * side(a.0, a.1, b.1) < 0.0 && side(b.0, b.1, a.0) * side(b.0, b.1, a.1) < 0.0 {
        return 0.0;
    }
    let to = |s: (parse::Pt, parse::Pt), q: parse::Pt| nacs::stock::dist_to_polygon(&[s.0, s.1], q.x, q.y);
    to(a, b.0).min(to(a, b.1)).min(to(b, a.0)).min(to(b, a.1))
}

/// Every op of `p` that works a `ProfilePocket`, under the pattern it cuts:
/// each at-depth move, as a SEGMENT, against each WALL edge of the op's
/// region. Returns how many ops and moves it judged. The tolerance is the one
/// geometric one plus the program's last decimal: `flatten_loop` samples ON
/// an arc, so the short chords between samples sit inside it by that much.
fn judge_pocket_moves(label: &str, p: &parse::Part) -> (usize, usize) {
    let (mut ops, mut judged) = (0usize, 0usize);
    for (idx, op) in p.operations.iter().enumerate() {
        if !matches!(p.features.get(&op.feature_name), Some(parse::Feature::ProfilePocket { .. })) {
            continue;
        }
        let (Some(region), Some(pattern)) = (nacs::region::of(p, op), parse::derive_pattern(p, op)) else { continue };
        // A program the engine refuses, or an op the report does not
        // measure (a rest pass), makes no moves to judge here.
        let Ok(moves) = compare::floor_moves(p, idx, pattern) else { continue };
        let r = p.tools[&op.tool_num].dia / 2.0;
        let n = region.outer.len();
        let walls: Vec<(parse::Pt, parse::Pt)> =
            (0..n).filter(|&i| region.edges[i] == nacs::region::EdgeKind::Wall).map(|i| (region.outer[i], region.outer[(i + 1) % n])).collect();
        for m in &moves {
            for w in &walls {
                let gap = segment_gap(*m, *w);
                assert!(
                    gap >= r - nacs::CHORD_TOL_MM - 1e-3,
                    "{label} op {} on '{}': the move ({:.3}, {:.3}) → ({:.3}, {:.3}) passes {gap:.3}mm from the wall ({:.3}, {:.3}) → ({:.3}, {:.3}), under the tool's radius {r}",
                    idx + 1, op.feature_name, m.0.x, m.0.y, m.1.x, m.1.y, w.0.x, w.0.y, w.1.x, w.1.y
                );
            }
        }
        ops += 1;
        judged += moves.len();
    }
    (ops, judged)
}

/// No at-depth move of an op that works a pocket comes within the tool's
/// radius of that pocket's WALL — the whole move, not its two ends. Over the
/// corpus, under what each op actually cuts.
#[test]
fn test_no_move_at_depth_comes_within_a_radius_of_its_pockets_wall() {
    let (mut ops, mut moves, mut found) = (0usize, 0usize, false);
    for (dir, p) in corpus() {
        let (o, m) = judge_pocket_moves(&dir, &p);
        found |= o > 0 && dir == "chamfer_profile.cam";
        ops += o;
        moves += m;
    }
    println!("the move clamp judged {ops} ops, {moves} moves");
    assert!(found, "the pocket that found this must be among those judged");
    assert!(ops >= 10 && moves > 500, "the clamp stopped judging: {ops} ops, {moves} moves");
}

/// The one path the corpus walk cannot reach (pinned 2026-10-01, at the
/// user's word): `End pattern=offset` on the floor of a closed, empty pocket.
/// It walks `plain_pocket_levels`, the function Phase 37 slice 0 fixed, and no
/// shipped fixture pins that pattern on such a floor — so the fix was shared
/// with it and never shown on it. Here it is on the L-pocket that found the
/// defect: a rough leaving half a millimetre, then the floor by the offset
/// family. Round the inside corner at (30, 50) the wall loop rides the arc a
/// radius plus the skin from it, 5.5mm, and never the chord, which would
/// pass 3.889mm from it.
#[test]
fn test_the_floor_of_a_plain_pocket_under_the_offset_family_rounds_its_inside_corner() {
    let src = std::fs::read_to_string("chamfer_profile.cam/main.part").unwrap();
    let kept: String = src.lines().filter(|l| !l.starts_with("run ")).map(|l| format!("{l}\n")).collect();
    let p = parse::parse(&format!(
        "{kept}run Clear(tool=T5, profile=Cavity, stepover=4.0, stock_to_leave=0.5)\nrun End(tool=T5, surface=Cavity, stepover=4.0, stock_to_leave=0.5, pattern=offset)\n"
    ))
    .unwrap();
    assert_eq!(parse::derive_pattern(&p, &p.operations[1]), Some(parse::Pattern::Offset));
    let g = gcode::program(&p).unwrap();
    assert!(g.contains("(--- floor: Cavity floor with T5"), "the End must reach the pocket-floor arm: {}", g.lines().filter(|l| l.starts_with("(---")).collect::<Vec<_>>().join(" "));
    // The whole-move clamp, on both ops.
    assert_eq!(judge_pocket_moves("the L, roughed then floored", &p).0, 2);
    // And the corner itself, on the End: the wall loop's nearest approach to
    // (30, 50) is the arc's radius, as a segment — not the chord's 3.889.
    let moves = compare::floor_moves(&p, 1, parse::Pattern::Offset).unwrap();
    let corner = parse::Pt { x: 30.0, y: 50.0 };
    let nearest = moves.iter().map(|m| nacs::stock::dist_to_polygon(&[m.0, m.1], corner.x, corner.y)).fold(f64::INFINITY, f64::min);
    assert!((nearest - 5.5).abs() < nacs::CHORD_TOL_MM + 1e-3, "the End passes {nearest:.3}mm from the inside corner");
    // It is an arc's worth of short moves there, not one.
    let round_it = moves.iter().filter(|m| (nacs::stock::dist_to_polygon(&[m.0, m.1], corner.x, corner.y) - 5.5).abs() < 0.02 && m.0.x < 30.0 && m.0.y < 50.0).count();
    assert!(round_it >= 8, "{round_it} moves ride the arc");
}

// ---- Phase 37 slice 4: the row an op actually cuts, asked of the corpus ----
// (`DECISIONS.md` → "Phase 37 slice 4…"). The `uncut` column was read by one
// roster, for one pattern. `floor_clamped`'s zigzag row said 55.7mm² for as
// long as the column existed and no test asked it; the same is true of any
// default that starts leaving floor. This asks every op, under what it cuts.

/// Every op in the corpus that works a region leaves no reachable floor
/// uncut under the pattern it actually cuts — its pin, or the default.
///
/// The fixtures are judged on as many threads as the machine has: each row
/// is an emitter run and a grid, 130s in a debug build on one thread, and
/// this gate is in `check.sh` twice.
#[test]
fn test_no_op_in_the_corpus_leaves_reachable_floor_under_what_it_cuts() {
    let corpus = corpus();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let judge = |dir: &str, p: &parse::Part, idx: usize| -> Option<Result<(), String>> {
        let row = compare::current_row(p, idx)?;
        Some(match row.outcome {
            Err(e) => Err(format!("{dir} op {}: a shipped program its own report refuses: {e}", idx + 1)),
            Ok(st) => match st.uncut_mm2 {
                None => Err(format!("{dir} op {}: the stock model declines to say what was left", idx + 1)),
                Some(a) if a != 0.0 => Err(format!("{dir} op {} leaves {a:.1}mm² of reachable floor under {}", idx + 1, row.pattern.word())),
                Some(_) => Ok(()),
            },
        })
    };
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let rows: Vec<Result<(), String>> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut out = Vec::new();
                    while let Some((dir, p)) = corpus.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed)) {
                        out.extend((0..p.operations.len()).filter_map(|idx| judge(dir, p, idx)));
                    }
                    out
                })
            })
            .collect();
        workers.into_iter().flat_map(|w| w.join().expect("a fixture's report panicked")).collect()
    });
    let left: Vec<&String> = rows.iter().filter_map(|r| r.as_ref().err()).collect();
    println!("the corpus gate asked {} rows", rows.len());
    assert!(left.is_empty(), "{left:#?}");
    assert!(rows.len() >= 70, "the gate stopped asking: {} rows", rows.len());
}

/// The gate's row is the report's row: measured alone, an op's `*` row says
/// what it says in the whole table.
#[test]
fn test_the_row_measured_alone_is_the_reports_own_row() {
    for dir in ["plate.cam", "pocket_post.cam", "deck_rest_offset.cam", "floor_clamped_offset.cam"] {
        let p = nacs::parse::load_project(dir).unwrap();
        for r in compare::op_reports(&p) {
            let whole = r.rows.iter().find(|row| row.current).unwrap_or_else(|| panic!("{dir} {}: no current row", r.title));
            let alone = compare::current_row(&p, r.op_idx).unwrap();
            assert_eq!(alone.pattern, whole.pattern, "{dir} {}", r.title);
            let both = (alone.outcome.as_ref().unwrap(), whole.outcome.as_ref().unwrap());
            assert_eq!((both.0.uncut_mm2, both.0.cutting_mm), (both.1.uncut_mm2, both.1.cutting_mm), "{dir} {}", r.title);
        }
    }
}

/// Phase 38 slice 0 (`DECISIONS.md` → "Phase 38 slice 0…"): zigzag and
/// oneway choose their direction as spine chooses its comb — every direction
/// `spine::directions` names, planned in full, the cheapest by the one price
/// kept. `thin_wall` is the one shipped program it moved: its floor is
/// 50 × 20 under a 10mm tool, and thirteen passes of 20mm joined along the
/// boundary price under six of 50mm joined round the corner. Pinned to
/// oneway the same floor runs along X still — thirteen lifts cost more than
/// the length they save — so "the longer side" was right there, and is
/// still the answer where it is the cheaper one.
#[test]
fn test_the_parallel_passes_choose_their_direction_by_the_one_price() {
    let src = std::fs::read_to_string("thin_wall.cam/main.part").unwrap();
    let p = parse::parse(&src).unwrap();
    let g = gcode::program(&p).unwrap();
    assert!(g.contains("(pattern: zigzag along Y, 13 passes at 5.000, then 1 shore pass along what stands)"), "{}", g.lines().find(|l| l.starts_with("(pattern")).unwrap_or(""));
    let run = src.lines().find(|l| l.starts_with("run ")).unwrap();
    let oneway = src.replace(run, &nacs::view::set_key(run, "pattern", Some("oneway")).unwrap());
    let g = gcode::program(&parse::parse(&oneway).unwrap()).unwrap();
    assert!(g.contains("(pattern: oneway along X, 6 passes at 4.982, then 1 shore pass along what stands)"), "{}", g.lines().find(|l| l.starts_with("(pattern")).unwrap_or(""));

    // The same choice, made by hand on the planner: the region's allowed
    // loops, both axes, both patterns, priced with the op's own lift.
    let op = &p.operations[0];
    let region = nacs::region::of(&p, op).unwrap();
    let (ccw, cw) = nacs::ops::clear::allowed_loops(&p, op, &region, 5.0, 0.0).unwrap();
    let never = |_: parse::Pt| false;
    // rapid_z 5 over a floor at -4, feed 800 over plunge 300: what a lift costs.
    let lift = 9.0 * (1.0 + 800.0 / 300.0);
    let price = |zigzag: bool, angle: f64| pattern::price(&pattern::plan(zigzag, angle, &ccw, &cw, 5.0, &never, None).unwrap().chains, lift);
    assert!(price(true, 90.0) < price(true, 0.0), "zigzag: along Y {:.1} against along X {:.1}", price(true, 90.0), price(true, 0.0));
    assert!(price(false, 0.0) < price(false, 90.0), "oneway: along X {:.1} against along Y {:.1}", price(false, 0.0), price(false, 90.0));
    // A plan is turned, not swapped: a Y-running plan's passes run along Y
    // and its row order is the frame's, and the words it prints are an axis.
    let y = pattern::plan(true, 90.0, &ccw, &cw, 5.0, &never, None).unwrap();
    assert_eq!((y.angle_deg, y.passes), (90.0, 13));
    assert!(y.chains.iter().flat_map(|c| c.pts.windows(2)).filter(|w| (w[0].x - w[1].x).abs() < 1e-9).count() >= 13, "the passes run along Y");
}

/// The programs that keep their direction keep their bytes: the pinned
/// fixtures' goldens are the lock (`test_the_three_pinned_fixtures_are_golden`),
/// and this is the price saying why — on each, the direction the longer-side
/// rule took is the one the price takes.
#[test]
fn test_where_the_price_agrees_with_the_longer_side_nothing_moved() {
    for (dir, along) in [("wrench_zigzag.cam", "along X"), ("wrench_oneway.cam", "along X"), ("step_zigzag.cam", "along Y"), ("wrench_step_zigzag.cam", "along X"), ("wrench_step_oneway.cam", "along X")] {
        let g = gcode::program(&load(dir)).unwrap();
        let note = g.lines().find(|l| l.starts_with("(pattern:")).unwrap().to_string();
        assert!(note.contains(along), "{dir}: {note}");
    }
}

/// Phase 38 slice 1 (`DECISIONS.md` → "Phase 38 slice 1…"): the walk's load
/// of an op AS EMITTED has a home, `compare::load_as_emitted`, and it is the
/// report's own row — the one marked `*` — read from the emission in hand
/// instead of a re-emission. Equal to the bit, because both read the same
/// moves through the same `op_moves`. Eight floors, one of each kind the
/// corpus has: a spine land of rails, a rest op over pieces, a pocket with a
/// post under the offset family, a zigzag beside a wall, a step, a bare
/// plane, a field of posts, a faced plane round a boss — each op's
/// `current_row`, one re-emission apiece. (The whole corpus, every row, is
/// the sweep's to walk, not a lock's: minutes in a debug build.)
#[test]
fn test_the_load_as_emitted_is_the_reports_current_row() {
    let mut asked = 0;
    for dir in ["rail_plate_spine.cam", "walled_rails_rest_spine.cam", "pocket_post.cam", "thin_wall.cam", "step_zigzag.cam", "plate.cam", "post_field.cam", "face_boss_offset.cam"] {
        let p = load(dir);
        let (_, emissions) = gcode::program_with_moves(&p).unwrap();
        for idx in 0..p.operations.len() {
            let Some(row) = compare::current_row(&p, idx) else { continue };
            let st = row.outcome.as_ref().unwrap_or_else(|e| panic!("{dir} op {idx}: {e}"));
            let emitted = compare::load_as_emitted(&p, &emissions, idx);
            assert_eq!(emitted, st.load, "{dir} op {idx}: the emission's load against the report's");
            asked += usize::from(st.load.is_some());
        }
    }
    assert!(asked >= 10, "the eight answer for most of their region ops: {asked}");
}

/// ROADMAP item 3 slice 6: an operation that cuts nothing says so. The
/// plate with its Clear run twice: the second Clear's moves at depth all
/// run in the air the first left, and the check says so by name, an
/// advisory in the engagement family; the first Clear and the End say
/// nothing of the kind. The plate as shipped is quiet.
#[test]
fn test_an_operation_whose_floor_is_already_air_says_so() {
    let main = std::fs::read_to_string("plate.cam/main.part").unwrap();
    let line = "run Clear(tool=T5, profile=Notch, stepover=4.0, pattern=spine)";
    assert!(main.contains(line));
    let twice = main.replace(line, &format!("{line}\n{line}"));
    let geom = std::fs::read_to_string("plate.cam/geometry.part").unwrap();
    let p = parse::parse_project_with_base(&twice, Some(&geom), None).unwrap();
    let (_, emissions) = gcode::program_with_moves(&p).unwrap();
    let findings = nacs::engagement::slot_findings(&p, &emissions);
    let nothing: Vec<&nacs::parse::Finding> = findings.iter().filter(|f| f.message.contains("cuts nothing")).collect();
    assert_eq!(nothing.len(), 1, "{findings:?}");
    assert!(nothing[0].message.starts_with("Clear on 'Notch' with T5: cuts nothing — its "), "{}", nothing[0].message);
    assert!(matches!(nothing[0].subject, nacs::parse::FindingSubject::Op { index: 2, .. }), "the second Clear: {:?}", nothing[0].subject);
    let shipped = load("plate.cam");
    let (_, emissions) = gcode::program_with_moves(&shipped).unwrap();
    assert!(nacs::engagement::slot_findings(&shipped, &emissions).iter().all(|f| !f.message.contains("cuts nothing")));
}

/// ROADMAP item 3 slice 4: the walk as emitted, as stretches. The plate's
/// three ops: each stretch is a run of the walk's own millimetre stations
/// of one kind, so the stretches past half immersion and in a slot sum to
/// the load's `past 90°`, the slots to its `slot`, everything but air to
/// its `engaged`; the Clear's descents are its entries; and `--patterns`
/// lists the same under the table.
#[test]
fn test_the_walk_as_emitted_is_the_load_in_stretches_and_the_report_lists_it() {
    let p = load("plate.cam");
    let (_, emissions) = gcode::program_with_moves(&p).unwrap();
    let mut read = 0;
    for idx in 0..p.operations.len() {
        let Some(w) = compare::walk_as_emitted(&p, &emissions, idx) else { continue };
        read += 1;
        let load = compare::load_as_emitted(&p, &emissions, idx).expect("the same op has a load");
        assert_eq!(w.load, load, "op {idx}");
        let sum = |f: &dyn Fn(compare::Station) -> bool| w.stretches.iter().filter(|s| f(s.kind)).map(|s| s.mm).sum::<f64>();
        assert!((sum(&|k| k == compare::Station::PastHalf || k == compare::Station::Slot) - load.past_half_mm).abs() < 1e-6, "op {idx}: past 90°");
        assert!((sum(&|k| k == compare::Station::Slot) - load.slot_mm).abs() < 1e-6, "op {idx}: slot");
        assert!((sum(&|k| k != compare::Station::Air) - load.engaged_mm).abs() < 1e-6, "op {idx}: engaged");
        assert!(w.stretches.iter().all(|s| s.pts.len() >= 2 && s.mm > 0.0), "op {idx}: every stretch spans something");
        for pair in w.stretches.windows(2) {
            assert!(pair[0].kind != pair[1].kind || pair[0].pts.last() != pair[1].pts.first(), "op {idx}: two stretches of one kind that meet were not merged");
        }
        let lines = compare::walk_lines(&w);
        assert!(lines[0].starts_with("as cut: ") && lines[0].contains("past 90°") && lines[0].contains("entr"), "{}", lines[0]);
        assert_eq!(lines.len(), 1 + w.stretches.iter().filter(|s| matches!(s.kind, compare::Station::PastHalf | compare::Station::Slot)).count() + w.entries.len());
    }
    assert_eq!(read, 2, "the End and the Clear are read; the Side works no floor");
    let clear = compare::walk_as_emitted(&p, &emissions, 1).unwrap();
    assert!(!clear.entries.is_empty(), "the Clear descends at least once");
    assert!(clear.entries.iter().all(|e| e.2 <= -0.5 + 1e-6), "every entry arrives at a cut depth: {:?}", clear.entries);
    assert!(clear.load.past_half_mm > 0.0, "spine on the notch rides past 90° somewhere");
    let report = compare::render(&p);
    assert!(report.contains("  as cut: "), "the report lists the walk:\n{report}");
    assert!(report.contains("  entry at ("), "and the entries");
}

/// Where there is nothing to read, the home says so: an op that works no
/// region (a drill, a wall finish) is `None`, not a zero.
#[test]
fn test_the_load_as_emitted_declines_off_a_region() {
    let p = load("cylinder_bore.cam");
    let (_, emissions) = gcode::program_with_moves(&p).unwrap();
    for idx in 0..p.operations.len() {
        if nacs::region::of(&p, &p.operations[idx]).is_none() {
            assert_eq!(compare::load_as_emitted(&p, &emissions, idx), None, "op {idx}");
        }
    }
    assert_eq!(compare::load_as_emitted(&p, &emissions, p.operations.len()), None, "past the last op");
}

/// The debug-build cost of the home, over every op in the corpus — the
/// number `check.sh` pays per reading once something in the pipeline reads
/// it (Phase 38 slice 2). Printed, not asserted: a clock is a measurement.
/// Run with `cargo test --test pattern_tests cost_of -- --ignored --nocapture`.
#[test]
#[ignore]
fn cost_of_the_load_as_emitted_over_the_corpus() {
    let mut rows: Vec<(f64, String)> = Vec::new();
    let mut total = 0.0;
    for (dir, p) in corpus() {
        let Ok((_, emissions)) = gcode::program_with_moves(&p) else { continue };
        for idx in 0..p.operations.len() {
            let t = std::time::Instant::now();
            let load = compare::load_as_emitted(&p, &emissions, idx);
            let ms = t.elapsed().as_secs_f64() * 1e3;
            total += ms;
            if load.is_some() {
                rows.push((ms, format!("{dir} op {idx}")));
            }
        }
    }
    rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    println!("the load as emitted, debug build: {} ops answered, {total:.0}ms in all", rows.len());
    for (ms, who) in rows.iter().take(8) {
        println!("  {ms:>8.1}ms  {who}");
    }
}

/// Phase 39 slice 0 (`DECISIONS.md` → "Phase 39 slice 0…"): one base. Every
/// `%` in a row is a share of the length cut at depth, which is the row's
/// first number; `entries` is the rest of the feed length; the note above the
/// header says so once, in the terminal and in the window's table alike.
/// Before this, `cutting` included the entries, the three `over` shares
/// stood on the at-depth length and `past 90°`'s share on the walk's engaged
/// length — three bases on one line, none of them named.
#[test]
fn test_every_share_in_a_row_stands_on_the_length_cut_at_depth() {
    for dir in ["thin_wall.cam", "rail_plate_spine.cam", "pocket_post.cam"] {
        let p = load(dir);
        let text = compare::render(&p);
        let reports = compare::op_reports(&p);
        assert_eq!(text.matches(compare::BASE_NOTE).count(), reports.len(), "{dir}: the base is said once per op");
        for r in &reports {
            for row in &r.rows {
                let Ok(st) = &row.outcome else { continue };
                let line = compare::row_line(row);
                let base = st.at_depth_mm;
                assert!(base > 0.0 && st.cutting_mm >= base - 1e-6, "{dir} {}: at depth {base} of cutting {}", row.pattern.word(), st.cutting_mm);
                if let Some((a, b, c)) = st.over {
                    assert!((a + b + c - base).abs() < 1e-6, "{dir} {}: the three over-shares are of the at-depth length and add to it", row.pattern.word());
                }
                let share = |v: f64| format!("{:>5.0}mm {:>3.0}%", v, 100.0 * v / base);
                // The cells are the row: the same words, in the columns' order
                // (COVER, COST, WHERE, LOAD since slice 1), the base at the head
                // of COST.
                let cells = compare::cells(row);
                assert_eq!(cells.len(), compare::COLUMNS.len(), "{dir}: one cell per column");
                assert_eq!(cells[0].text, row.pattern.word());
                assert_eq!(cells[3].text, format!("{:.0}mm", base), "{dir}: `at depth` is the base");
                assert_eq!(cells[4].text, format!("{:.0}mm", st.cutting_mm - base), "{dir}: `entries` is the rest");
                assert_eq!(line.split_whitespace().collect::<Vec<_>>(), std::iter::once(if row.current { "*" } else { " " }).chain(cells.iter().map(|c| c.text.as_str())).collect::<Vec<_>>().join(" ").split_whitespace().collect::<Vec<_>>(), "{dir}: the line is the cells joined");
                if let Some((a, b, c)) = st.over {
                    assert!(line.contains(&format!("{} {} {}", share(a), share(b), share(c))), "{dir}: {line}");
                }
                if let Some(l) = st.load {
                    assert!(line.contains(&share(l.past_half_mm)) && line.contains(&share(l.slot_mm)), "{dir}: {line}");
                }
            }
        }
    }
    // The header's labels sit over their cells: the last label ends where
    // the last cell ends.
    let line = compare::row_line(&compare::op_reports(&load("thin_wall.cam"))[0].rows[0]);
    let header = compare::header();
    let header_end = header.find("worst (band)").map(|i| i + "worst (band)".len()).unwrap();
    let row_end = line.rfind("—").map(|i| i + "—".len()).unwrap();
    assert_eq!(header[..header_end].chars().count(), line[..row_end].chars().count(), "the `worst (band)` label ends where its cell ends:\n{header}\n{line}");
}
