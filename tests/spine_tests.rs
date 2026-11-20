// ---- spine: the pattern born between planing and the offset family -------------------
// Phase 30 (`DECISIONS.md` → "Phase 30 slice 2: mow…", its name until Phase 35). Rows cut only where
// they own floor, one shore pass along what stands and only there, on a comb
// chosen by measuring it. Every claim the entry makes is a number pinned here.

use nacs::parse::{Pattern, Pt};
use nacs::{compare, gcode, spine, ops, parse, region};

fn load(dir: &str) -> parse::Part {
    let main = std::fs::read_to_string(format!("{dir}/main.part")).unwrap();
    let geom = std::fs::read_to_string(format!("{dir}/geometry.part")).ok();
    parse::parse_project(&main, geom.as_deref()).unwrap_or_else(|e| panic!("{dir}: {e}"))
}

fn row<'a>(r: &'a compare::OpReport, k: Pattern) -> &'a compare::Stats {
    r.rows.iter().find(|row| row.pattern == k).unwrap().outcome.as_ref().unwrap_or_else(|e| panic!("{}: {e}", k.word()))
}

/// The plan an op's spine would cut, chosen (`None`) or pinned to one comb.
fn plan_of(p: &parse::Part, idx: usize, at: Option<(f64, f64, bool)>) -> spine::Plan {
    let op = &p.operations[idx];
    let region = region::of(p, op).unwrap();
    ops::clear::spine_plan(p, op, &region, 0.0, (18.0, 20.0), at).unwrap()
}

#[test]
fn test_the_spine_fixtures_are_golden() {
    for dir in ["wrench_spine.cam", "wrench_step_spine.cam", "lawn_fence_bed_spine.cam", "pocket_post_spine.cam", "rail_plate_offset.cam", "rail_plate_spine.cam", "curved_rails_offset.cam", "curved_rails_spine.cam"] {
        let g = gcode::program(&load(dir)).unwrap_or_else(|e| panic!("{dir}: {e}"));
        let golden = std::fs::read_to_string(format!("tests/{}.golden.gcode", dir.trim_end_matches(".cam"))).unwrap();
        assert_eq!(g.trim(), golden.trim(), "{dir}");
    }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Pt> {
    vec![Pt { x: x0, y: y0 }, Pt { x: x1, y: y0 }, Pt { x: x1, y: y1 }, Pt { x: x0, y: y1 }]
}

/// The ownership rule, on the barest floor there is: 100x30, every edge open,
/// a 10mm tool at a 5mm stepover. Row `v` owns the band its advancing edge
/// uncovers, the bands tile the floor, so it takes exactly 30/5 = SIX rows —
/// the first ON the floor's edge, cutting one stepover wide — where passes
/// fitted to the allowed region (a radius bigger all round) take nine, the
/// outer two of them tangent to the floor and removing nothing.
#[test]
fn test_a_row_is_cut_only_where_it_owns_floor() {
    let (floor, allowed) = (vec![rect(0.0, 0.0, 100.0, 30.0)], vec![rect(-5.0, -5.0, 105.0, 35.0)]);
    let never = |_: Pt| false;
    let ask = spine::Ask { ccw: &allowed, cw: &[], floor: &floor, walls: &[], r: 5.0, stand_off: 5.0, stepover: 5.0, lift_cost: 18.0, ramp_len: 20.0, in_air: &never, level: &[], level_d: 0.0, held: None, conventional: false, angle: None };
    let plan = spine::plan_at(&ask, 0.0, 0.0, false).unwrap();
    assert_eq!((plan.rows, plan.shores), (6, 0));
    let mut ys: Vec<f64> = plan.chains.iter().flat_map(|c| c.pts.iter().map(|q| q.y)).collect();
    ys.sort_by(|a, b| a.partial_cmp(b).unwrap());
    ys.dedup();
    assert_eq!(ys, [0.0, 5.0, 10.0, 15.0, 20.0, 25.0], "the first row rides the floor's own edge: one stepover of cut, not a slot");
    // Each row runs a radius past each end of what it owns — the tool leaving.
    let xs: Vec<f64> = plan.chains.iter().flat_map(|c| c.pts.iter().map(|q| q.x)).collect();
    assert!(xs.iter().all(|x| (-5.0 - 1e-9..=105.0 + 1e-9).contains(x)) && xs.contains(&-5.0) && xs.contains(&105.0));
    // The zigzag's planner, same floor: nine passes.
    let zig = nacs::pattern::plan(true, 0.0, &allowed, &[], 5.0, &never, None).unwrap();
    assert_eq!(zig.passes, 9);

    // An L: the rows the tall leg alone needs are not dragged along the foot.
    let l = vec![vec![Pt { x: 0.0, y: 0.0 }, Pt { x: 100.0, y: 0.0 }, Pt { x: 100.0, y: 20.0 }, Pt { x: 30.0, y: 20.0 }, Pt { x: 30.0, y: 60.0 }, Pt { x: 0.0, y: 60.0 }]];
    let l_allowed = vec![vec![Pt { x: -5.0, y: -5.0 }, Pt { x: 105.0, y: -5.0 }, Pt { x: 105.0, y: 25.0 }, Pt { x: 35.0, y: 25.0 }, Pt { x: 35.0, y: 65.0 }, Pt { x: -5.0, y: 65.0 }]];
    let ask = spine::Ask { ccw: &l_allowed, cw: &[], floor: &l, walls: &[], r: 5.0, stand_off: 5.0, stepover: 5.0, lift_cost: 18.0, ramp_len: 20.0, in_air: &never, level: &[], level_d: 0.0, held: None, conventional: false, angle: None };
    let plan = spine::plan_at(&ask, 0.0, 0.0, false).unwrap();
    for c in &plan.chains {
        for w in c.pts.windows(2).filter(|w| (w[0].y - w[1].y).abs() < 1e-9 && w[0].y > 20.0 - 1e-9) {
            assert!(w[0].x.max(w[1].x) <= 35.0 + 1e-9, "a row above the foot stays on the leg: {:?}", w);
        }
    }
}

/// The comb is the PART's, not the table's: the same walled strip drawn square
/// and drawn at 30 degrees gets the same plan — rows along its long edges —
/// to a micron of cost.
#[test]
fn test_the_comb_follows_the_part_not_the_table() {
    let turn = |ls: &[Vec<Pt>], deg: f64| -> Vec<Vec<Pt>> {
        let (s, c) = deg.to_radians().sin_cos();
        ls.iter().map(|l| l.iter().map(|q| Pt { x: q.x * c - q.y * s, y: q.x * s + q.y * c }).collect()).collect()
    };
    // 120x40, walled along its top edge (y=40): the allowed region stands a
    // radius off that wall and a radius past the three open edges.
    let (floor, allowed) = (vec![rect(0.0, 0.0, 120.0, 40.0)], vec![rect(-5.0, -5.0, 125.0, 35.0)]);
    let wall = [(Pt { x: 0.0, y: 40.0 }, Pt { x: 120.0, y: 40.0 })];
    let never = |_: Pt| false;
    let plan_for = |deg: f64| {
        let (f, a) = (turn(&floor, deg), turn(&allowed, deg));
        let w: Vec<(Pt, Pt)> = turn(&[vec![wall[0].0, wall[0].1]], deg).into_iter().map(|l| (l[0], l[1])).collect();
        let ask = spine::Ask { ccw: &a, cw: &[], floor: &f, walls: &w, r: 5.0, stand_off: 5.0, stepover: 5.0, lift_cost: 18.0, ramp_len: 20.0, in_air: &never, level: &[], level_d: 0.0, held: None, conventional: false, angle: None };
        spine::plan(&ask).unwrap()
    };
    let (square, canted) = (plan_for(0.0), plan_for(30.0));
    assert_eq!((square.angle_deg, canted.angle_deg), (0.0, 30.0));
    assert_eq!((square.rows, square.shores), (canted.rows, canted.shores));
    assert!((square.cost - canted.cost).abs() < 1e-3, "{} vs {}", square.cost, canted.cost);
    // …and the wall got ONE shore pass, its own length plus a lead each end.
    assert_eq!(square.shores, 1);
    assert!((square.shore_mm - 130.0).abs() < 1e-6, "{}", square.shore_mm);
    // A chord of a flattened arc is not a direction anyone would name.
    let ring: Vec<Pt> = (0..72).map(|k| (k as f64 * 5f64).to_radians()).map(|a| Pt { x: 50.0 * a.cos(), y: 50.0 * a.sin() }).collect();
    assert_eq!(spine::directions(&[ring], 10.0), [0.0, 90.0]);
}

/// Where nothing stands, spine is PLANING: no shore, and on the wrench the shank
/// (14mm across, a 10mm tool) takes the three passes it needs.
#[test]
fn test_with_nothing_standing_spine_is_planing() {
    let p = load("wrench_spine.cam");
    let plan = plan_of(&p, 0, None);
    assert_eq!((plan.shores, plan.shore_mm), (0, 0.0));
    let g = gcode::program(&p).unwrap();
    assert!(g.contains("(pattern: spine at 0.000deg,") && !g.contains("shore"), "the note says what was chosen");
    let r = &compare::op_reports(&load("wrench_offset.cam"))[0];
    let shank = r.lands.iter().position(|l| (l.width - 14.0).abs() < 0.1).expect("the shank is a land");
    assert_eq!(row(r, Pattern::Spine).land_passes[shank], 3);
    assert!(row(r, Pattern::Zigzag).land_passes[shank] > 3);
}

/// The headland: one closed arc out of eight edges gets one shore pass ITS OWN
/// length — the zigzag closes with a lap of the whole outline — and a wall
/// chain that ends mid-side is followed to its end and no further.
#[test]
fn test_a_shore_pass_is_as_long_as_its_wall_and_never_wraps_its_end() {
    let deck = plan_of(&load("wrench_step_spine.cam"), 1, None);
    assert_eq!(deck.shores, 1);
    // The arc against the ring: r 25 about (28,30) between the shank's edges,
    // ~17mm, plus a radius of lead on each end.
    assert!(deck.shore_mm > 17.0 && deck.shore_mm < 40.0, "{}", deck.shore_mm);

    let lawn = plan_of(&load("lawn_fence_bed_spine.cam"), 0, None);
    assert_eq!(lawn.shores, 2, "the fence, and the bed");
    // The fence's wall is y=90, x 10..130; the allowed region wraps both its
    // ends (up to y=90 at x<10 and x>130). No tool position does.
    let top = lawn.chains.iter().flat_map(|c| &c.pts).map(|q| q.y).fold(f64::NEG_INFINITY, f64::max);
    assert!((top - 85.0).abs() < 1e-6, "nothing rides above the fence's own stand-off line: {top}");
    let along: Vec<&Pt> = lawn.chains.iter().flat_map(|c| &c.pts).filter(|q| (q.y - 85.0).abs() < 1e-6).collect();
    let (x0, x1) = along.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| (a.min(q.x), b.max(q.x)));
    assert!((x0 - 5.0).abs() < 1e-6 && (x1 - 135.0).abs() < 1e-6, "the wall's length and a lead past each end: {x0}..{x1}");
}

/// The comb is CHOSEN by measuring it, from directions a person would name —
/// and a sweep of every fifteen degrees, four phases, both ways, finds nothing
/// on these floors worth more than two percent of what was chosen. (A full
/// one-degree sweep was run when this was decided and bought at most one
/// second anywhere: `DECISIONS.md`.)
#[test]
fn test_no_swept_comb_beats_the_chosen_one_by_more_than_two_percent() {
    for (dir, idx) in [("wrench_step_spine.cam", 1), ("lawn_fence_bed_spine.cam", 0)] {
        let p = load(dir);
        let chosen = plan_of(&p, idx, None);
        for deg in (0..180).step_by(15) {
            for phase in [0.0, 0.25, 0.5, 0.75] {
                for down in [false, true] {
                    let swept = plan_of(&p, idx, Some((deg as f64, phase, down)));
                    assert!(swept.cost > chosen.cost * 0.98, "{dir}: {deg}deg phase {phase} down {down} costs {:.0} against the chosen {:.0}", swept.cost, chosen.cost);
                }
            }
        }
    }
}

/// The gate a pattern built to cut LESS has to pass first: across the floors
/// this corpus holds — open, walled, mixed, islanded, at depth — spine leaves no
/// reachable floor uncut, and (the emitter having answered at all) drove the
/// tool into nothing that outlives its op.
#[test]
fn test_spine_covers_every_floor_in_the_roster() {
    let roster = [
        "wrench_offset.cam", "wrench_step_offset.cam", "wrench_thick_offset.cam", "cottage_interior_offset.cam", "lawn_fence_bed_offset.cam", "face_boss_offset.cam",
        "pocket_post.cam", "pocket_islands.cam", "profile_pocket_clear.cam", "pocket_floor_offset.cam", "step.cam", "floor_clamped_offset.cam",
        // A floor that is ALL edge: 3mm rails under a 10mm tool, three faces.
        "rail_plate_offset.cam",
        // …and the same, curved.
        "curved_rails_offset.cam",
        // Phase 34 slice 1: the wide floors, under a 6mm tool.
        "open_field.cam", "post_field.cam",
    ];
    for dir in roster {
        for r in compare::op_reports(&load(dir)) {
            let st = row(&r, Pattern::Spine);
            assert_eq!(st.uncut_mm2, Some(0.0), "{dir}: {}", r.title);
        }
    }
}

/// What the phase was for, as the report's own numbers. On the floors with
/// open edges spine is quicker than both patterns it was born between; in a
/// pocket closed all round — the offset family's home ground — it is not, and
/// that row stays in the report: nothing here moves a default.
#[test]
fn test_the_report_says_where_spine_wins_and_where_it_does_not() {
    for dir in ["wrench_offset.cam", "wrench_step_offset.cam", "cottage_interior_offset.cam", "lawn_fence_bed_offset.cam", "face_boss_offset.cam", "step.cam"] {
        for r in compare::op_reports(&load(dir)) {
            let (m, o, z) = (row(&r, Pattern::Spine).cutting_s, row(&r, Pattern::Offset).cutting_s, row(&r, Pattern::Zigzag).cutting_s);
            assert!(m < o && m < z, "{dir} {}: spine {m:.0}s, offset {o:.0}s, zigzag {z:.0}s", r.title);
        }
    }
    let deck = &compare::op_reports(&load("wrench_step_offset.cam"))[1];
    let (m, o) = (row(deck, Pattern::Spine).cutting_s, row(deck, Pattern::Offset).cutting_s);
    // Under 0.7 until Phase 44 slice 2; spine's chord ramps on the headland
    // are legs or plunges at the plunge feed now, and it reads 83s against
    // the offset family's 117s. The law is the price, printed.
    assert!(m < 0.75 * o, "the headland, the fixture spine was aimed at: {m:.0}s against {o:.0}s");

    // A closed pocket was the offset family's on the clock (offset under
    // spine) until Phase 44 slice 1: with the ramp's law the family laps
    // its three short first loops and reads 160s against spine's 150s. The
    // two are within a tenth of each other; the Clear's default stays
    // offset by ruling, and the report never chooses.
    let closed = &compare::op_reports(&load("pocket_islands.cam"))[0];
    let (o, m) = (row(closed, Pattern::Offset).cutting_s, row(closed, Pattern::Spine).cutting_s);
    assert!((o / m - 1.0).abs() < 0.15, "a closed pocket: offset {o:.0}s, spine {m:.0}s");
    assert!(row(closed, Pattern::Spine).cutting_s < row(closed, Pattern::Zigzag).cutting_s);

    // The pinned fixture's row is the current one, and its program is the row.
    let pinned = &compare::op_reports(&load("wrench_spine.cam"))[0];
    assert_eq!(pinned.rows.iter().map(|r| (r.pattern.word(), r.current)).collect::<Vec<_>>(), [("offset", false), ("zigzag", false), ("oneway", false), ("spine", true), ("held", false)]);
}

/// At depth in a closed pocket there is no air to enter from: the first row
/// RAMPS along its own stroke, and the shores — cut last, beside floor the rows
/// already took — do not.
#[test]
fn test_a_closed_pocket_ramps_its_first_row_and_the_vocabulary_knows_the_word() {
    let p = load("pocket_post_spine.cam");
    let plan = plan_of(&p, 0, None);
    assert!(plan.chains[0].ramp);
    assert!(plan.chains[1..].iter().all(|c| !c.ramp), "every later chain starts beside something cut");
    assert_eq!(plan.shores, 2, "round the post, and round the wall");

    let err = parse::parse("units mm\nstock Box(w=10, l=10, h=5)\ntool T1 Endmill(dia=2)\nfeature F Plane(z=0)\nrun End(tool=T1, surface=F, stepover=1, pattern=mower)").unwrap_err();
    assert!(err.contains("'offset', 'zigzag', 'oneway', 'spine', 'held' or 'raster'"), "{err}");
}

/// The rail plate (Phase 30 slice 3): a network of 3mm rails, 22% of the plate,
/// every edge open, two rails standing free inside channel pockets and so
/// faces of their own. Every pattern covers it; the offset family laps each
/// rail on both sides, tangent, and spends two fifths of its length over stock
/// that is not the land; spine is the quickest of the four on all three faces.
#[test]
fn test_the_rail_plate_is_covered_by_every_pattern_and_spine_is_quickest() {
    let reports = compare::op_reports(&load("rail_plate_offset.cam"));
    assert_eq!(reports.iter().map(|r| r.title.split('\'').nth(1).unwrap()).collect::<Vec<_>>(), ["Land", "RingRail", "BarRail"]);
    for r in &reports {
        for k in Pattern::ALL {
            assert_eq!(row(r, k).uncut_mm2, Some(0.0), "{} {}", r.title, k.word());
        }
        let m = row(r, Pattern::Spine).cutting_s;
        assert!(Pattern::ALL.iter().all(|k| m <= row(r, *k).cutting_s), "{}", r.title);
    }
    let (floor, other, air) = row(&reports[0], Pattern::Offset).over.unwrap();
    assert!(other / (floor + other + air) > 0.35, "the 2r push, in full bloom");
}

/// Slice 4 — the CROSS COMB. A comb has one direction, and on a network of
/// rails the right direction is local: a row crossing a rail at right angles
/// owns a run of floor shorter than the tool is wide — a TOOTH — and hands it
/// to a comb at right angles, along which that rail is one long pass. Here a
/// T of two 3mm rails: a single comb must cross one of them in teeth a
/// stepover apart; the pair cuts each rail once, along it.
#[test]
fn test_a_tooth_is_handed_to_the_comb_at_right_angles() {
    // The bar along X (y 0..3), the stem along Y (x 48.5..51.5) below it.
    let t = vec![vec![
        Pt { x: 0.0, y: 0.0 }, Pt { x: 100.0, y: 0.0 }, Pt { x: 100.0, y: 3.0 }, Pt { x: 0.0, y: 3.0 },
    ], ];
    let stem = rect(48.5, -90.0, 51.5, 0.0);
    // One outline: bar and stem joined (CCW).
    let outline = vec![vec![
        Pt { x: 0.0, y: 0.0 }, Pt { x: 48.5, y: 0.0 }, stem[0], stem[1], Pt { x: 51.5, y: 0.0 }, Pt { x: 100.0, y: 0.0 }, Pt { x: 100.0, y: 3.0 }, Pt { x: 0.0, y: 3.0 },
    ]];
    let _ = t;
    // Where the centre may be: far enough out to be no constraint at all.
    let allowed = vec![rect(-20.0, -110.0, 120.0, 20.0)];
    let never = |_: Pt| false;
    let ask = spine::Ask { ccw: &allowed, cw: &[], floor: &outline, walls: &[], r: 5.0, stand_off: 5.0, stepover: 5.0, lift_cost: 18.0, ramp_len: 20.0, in_air: &never, level: &[], level_d: 0.0, held: None, conventional: false, angle: None };
    let strokes = |p: &spine::Plan| -> Vec<f64> { p.chains.iter().flat_map(|c| c.pts.windows(2).map(|w| (w[1].x - w[0].x).hypot(w[1].y - w[0].y)).collect::<Vec<_>>()).collect() };

    let single = spine::plan_at(&ask, 0.0, 0.5, false).unwrap();
    let teeth = strokes(&single).iter().filter(|l| (12.9..13.1).contains(*l)).count();
    assert!(teeth >= 17, "rows along X cross the 90mm stem every stepover, 3mm + a radius each side: {teeth}");

    let pair = spine::plan(&ask).unwrap();
    assert!(pair.cross_rows >= 1 && pair.cost < 0.6 * single.cost, "{} rows + {} across, {:.0} against {:.0}", pair.rows, pair.cross_rows, pair.cost, single.cost);
    assert_eq!(strokes(&pair).iter().filter(|l| (12.9..13.1).contains(*l)).count(), 0, "no teeth left");
    // …and where nothing is narrower than the tool there is nothing to hand
    // over: the pair is the single comb, and says so by not existing.
    let wide = vec![rect(0.0, 0.0, 100.0, 30.0)];
    let wide_allowed = vec![rect(-5.0, -5.0, 105.0, 35.0)];
    let ask = spine::Ask { ccw: &wide_allowed, cw: &[], floor: &wide, walls: &[], r: 5.0, stand_off: 5.0, stepover: 5.0, lift_cost: 18.0, ramp_len: 20.0, in_air: &never, level: &[], level_d: 0.0, held: None, conventional: false, angle: None };
    assert!(spine::plan_crossed(&ask, 0.0, (0.0, false), (0.0, false)).is_none());
    assert_eq!(spine::plan(&ask).unwrap().cross_rows, 0);
}

/// A ramped chain pays for a ramp's length of its first stroke twice, not the
/// whole pass (Phase 30 slice 4) — and a chain whose first segment is a chord
/// (a rail pass) gets a straight lead a ramp long instead of a plunge down a
/// third of a millimetre at the feed rate (Phase 31).
#[test]
fn test_a_ramp_is_a_ramps_length_and_a_chord_gets_a_straight_lead() {
    let p = load("rail_plate_spine.cam");
    let land = plan_of(&p, 0, None);
    for c in land.chains.iter().filter(|c| c.ramp) {
        let first = (c.pts[1].x - c.pts[0].x).hypot(c.pts[1].y - c.pts[0].y);
        assert!(first <= 20.0 + 1e-6, "a ramp is at most `ramp_len` long: {first}");
        assert!(first >= 5.0 - 1e-6, "…and never less than a radius, the run-out past a dead end: {first}");
    }
    // The emitter's own ramp: three degrees over the 1mm this face loses.
    let depth_over_tan = 1.0 / ops::clear::RAMP_DEG.to_radians().tan();
    assert!((depth_over_tan - 19.081).abs() < 1e-3);
    // The wrench's ring pass — a chorded arc — is entered down a straight
    // lead a ramp long along its tangent, then the arc, not down its first
    // third of a millimetre at the feed rate.
    let ring = plan_of(&load("wrench_spine.cam"), 0, None);
    let c = ring.chains.iter().find(|c| c.pts.len() > 100).expect("the ring's pass");
    assert!(c.ramp);
    let first = (c.pts[1].x - c.pts[0].x).hypot(c.pts[1].y - c.pts[0].y);
    assert!((first - 20.0).abs() < 1e-6, "the lead is `ramp_len` long: {first}");
}

/// Phase 31 — THE RAIL PASS. Where a single pass covers the floor's whole
/// width, one pass down its centreline, however it curves: the centreline is
/// the midline of the HAIRLINE — the floor eroded to a hair under half its
/// narrowest land, whose two sides come back a hair apart wherever the floor
/// is that rail. On synthetic floors, from a level handed in by hand:
#[test]
fn test_a_rail_gets_one_pass_down_its_centreline_and_a_wide_floor_gets_none() {
    let never = |_: Pt| false;
    // A 100 x 3 rail, 10mm tool: the hairline is the rail eroded by 1.48.
    let rail = vec![rect(0.0, 0.0, 100.0, 3.0)];
    let level = vec![rect(1.48, 1.48, 98.52, 1.52)];
    let allowed = vec![rect(-20.0, -20.0, 120.0, 23.0)];
    let ask = spine::Ask { ccw: &allowed, cw: &[], floor: &rail, walls: &[], r: 5.0, stand_off: 5.0, stepover: 5.0, lift_cost: 18.0, ramp_len: 20.0, in_air: &never, level: &level, level_d: 1.48, held: None, conventional: false, angle: None };
    let passes = spine::rails(&ask);
    assert_eq!(passes.len(), 1, "{passes:?}");
    let ys: Vec<f64> = passes[0].iter().map(|q| q.y).collect();
    assert!(ys.iter().all(|y| (y - 1.5).abs() < 0.05), "down the centreline: {ys:?}");
    let (x0, x1) = passes[0].iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| (a.min(q.x), b.max(q.x)));
    assert!(x0 < -3.0 && x1 > 103.0, "a radius past each dead end: {x0}..{x1}");
    let plan = spine::plan(&ask).unwrap();
    assert_eq!((plan.rails, plan.rows), (1, 0), "the rail pass owns the whole rail; no row is left anything");

    // A 100 x 14 floor: its hairline (at 1.48, from a 14mm 'narrowest land')
    // has sides eleven apart — no single pass covers it — so no rail pass,
    // and the rows as before.
    let wide = vec![rect(0.0, 0.0, 100.0, 14.0)];
    let level = vec![rect(1.48, 1.48, 98.52, 12.52)];
    let ask = spine::Ask { ccw: &allowed, cw: &[], floor: &wide, walls: &[], r: 5.0, stand_off: 5.0, stepover: 5.0, lift_cost: 18.0, ramp_len: 20.0, in_air: &never, level: &level, level_d: 1.48, held: None, conventional: false, angle: None };
    assert!(spine::rails(&ask).is_empty());
    assert_eq!(spine::plan(&ask).unwrap().rails, 0);
}

/// The curved rail plate (Phase 31 slice 2): one pass along every rail, found
/// from the hairline; rows only where the S-trunk widens past a pass; every
/// pattern still covers it; spine at under half the offset family's time.
#[test]
fn test_the_curved_rails_are_each_one_pass_and_spine_is_twice_as_quick() {
    let p = load("curved_rails_spine.cam");
    let land = plan_of(&p, 0, None);
    // Three rows for the one stretch the rails cannot cover (the S-trunk's
    // 14mm section); everything else is a rail pass.
    assert!(land.rows + land.cross_rows <= 4, "{} rows and {} across", land.rows, land.cross_rows);
    assert!(land.rails >= 15 && land.rails <= 30, "{} rail passes", land.rails);
    // The land is 3859mm² of rail 3mm wide (plus bosses and one 14mm blob):
    // its centreline is about a third of a kilometre long. The passes are
    // that, plus a radius run out past each dead end.
    assert!((1150.0..1500.0).contains(&land.rail_mm), "{}", land.rail_mm);
    let reports = compare::op_reports(&load("curved_rails_offset.cam"));
    for r in &reports {
        for k in Pattern::ALL {
            assert_eq!(row(r, k).uncut_mm2, Some(0.0), "{} {}", r.title, k.word());
        }
    }
    let (m, o, z) = (row(&reports[0], Pattern::Spine).cutting_s, row(&reports[0], Pattern::Offset).cutting_s, row(&reports[0], Pattern::Zigzag).cutting_s);
    assert!(m < 0.5 * o && m < 0.5 * z, "spine {m:.0}s, offset {o:.0}s, zigzag {z:.0}s");
    let (f, other, air) = row(&reports[0], Pattern::Spine).over.unwrap();
    assert!(f / (f + other + air) > 0.97, "nearly all of it on the land");
}
/// Phase 31 slice 1 — the curved rail plate, pinned as Phase 30 leaves it: a
/// part no comb can do. Every pattern covers it; spine is the quickest of the
/// four and still spends most of its passes crossing rails in teeth.
#[test]
fn test_the_curved_rails_are_covered_by_every_pattern() {
    let reports = compare::op_reports(&load("curved_rails_offset.cam"));
    assert_eq!(reports.iter().map(|r| r.title.split('\'').nth(1).unwrap()).collect::<Vec<_>>(), ["Land", "SpiralBoss"]);
    for r in &reports {
        for k in Pattern::ALL {
            assert_eq!(row(r, k).uncut_mm2, Some(0.0), "{} {}", r.title, k.word());
        }
        let m = row(r, Pattern::Spine).cutting_s;
        assert!(Pattern::ALL.iter().all(|k| m <= row(r, *k).cutting_s), "{}", r.title);
    }
}

// ---- the answer key ------------------------------------------------------------
// `curved_rails_offset.cam/key.svg` is the centreline GRAPH the part was drawn from
// and then thickened (Phase 31 slice 3). The rail passes are scored against
// it, edge by edge, so "one pass down the centreline, to chord tolerance" is a
// measured sentence and stays one as spine changes.

/// The key's edges as sampled polylines in the part's frame (y flipped about
/// the drawing's height, as `geometry.part` was), with their class word.
fn key_edges(path: &str) -> Vec<(String, Vec<Pt>)> {
    let src = std::fs::read_to_string(path).unwrap();
    let h = 120.0;
    let mut out = Vec::new();
    for line in src.lines().filter(|l| l.contains("<path class=")) {
        let class = line.split("class=\"").nth(1).unwrap().split('"').next().unwrap().to_string();
        let d = line.split(" d=\"").nth(1).unwrap().split('"').next().unwrap();
        let tok: Vec<&str> = d.split_whitespace().collect();
        let mut pts: Vec<Pt> = Vec::new();
        let mut i = 0;
        while i < tok.len() {
            match tok[i] {
                "M" => {
                    pts.push(Pt { x: tok[i + 1].parse().unwrap(), y: tok[i + 2].parse().unwrap() });
                    i += 3;
                }
                "L" => {
                    let (a, b) = (*pts.last().unwrap(), Pt { x: tok[i + 1].parse().unwrap(), y: tok[i + 2].parse().unwrap() });
                    let n = ((b.x - a.x).hypot(b.y - a.y) / 0.25).ceil().max(1.0) as usize;
                    pts.extend((1..=n).map(|k| Pt { x: a.x + (b.x - a.x) * k as f64 / n as f64, y: a.y + (b.y - a.y) * k as f64 / n as f64 }));
                    i += 3;
                }
                "A" => {
                    let v: Vec<f64> = tok[i + 1..i + 8].iter().map(|s| s.parse().unwrap()).collect();
                    let (r, fa, fs, b) = (v[0], v[3] != 0.0, v[4] != 0.0, Pt { x: v[5], y: v[6] });
                    pts.extend(svg_arc(*pts.last().unwrap(), r, fa, fs, b));
                    i += 8;
                }
                _ => i += 1,
            }
        }
        out.push((class, pts.into_iter().map(|q| Pt { x: q.x, y: h - q.y }).collect()));
    }
    out
}

/// SVG's circular arc (F.6.5), sampled every quarter millimetre.
fn svg_arc(a: Pt, r: f64, large: bool, sweep: bool, b: Pt) -> Vec<Pt> {
    let (dx, dy) = ((a.x - b.x) / 2.0, (a.y - b.y) / 2.0);
    let r = r.max((dx * dx + dy * dy).sqrt());
    let num = (r * r * r * r - r * r * dy * dy - r * r * dx * dx).max(0.0);
    let co = (num / (r * r * dy * dy + r * r * dx * dx)).sqrt() * if large == sweep { -1.0 } else { 1.0 };
    let (cxp, cyp) = (co * dy, -co * dx);
    let (cx, cy) = (cxp + (a.x + b.x) / 2.0, cyp + (a.y + b.y) / 2.0);
    let a1 = ((dy - cyp) / r).atan2((dx - cxp) / r);
    let a2 = ((-dy - cyp) / r).atan2((-dx - cxp) / r);
    let mut da = a2 - a1;
    if sweep && da < 0.0 {
        da += std::f64::consts::TAU;
    }
    if !sweep && da > 0.0 {
        da -= std::f64::consts::TAU;
    }
    let n = ((da.abs() * r) / 0.25).ceil().max(4.0) as usize;
    (1..=n).map(|k| Pt { x: cx + r * (a1 + da * k as f64 / n as f64).cos(), y: cy + r * (a1 + da * k as f64 / n as f64).sin() }).collect()
}

/// Every at-depth feed move of a program.
fn cuts_at_depth(g: &str, z_cut: f64) -> Vec<(Pt, Pt)> {
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
        if line.starts_with("G01") && x.is_finite() && (z - z_cut).abs() < 1e-9 && (nz - z_cut).abs() < 1e-9 {
            out.push((Pt { x, y }, Pt { x: nx, y: ny }));
        }
        (x, y, z) = (nx, ny, nz);
    }
    out
}

/// The key, scored: of its 1165mm of centreline, how much has a pass running
/// ALONG it (within a quarter millimetre), how far that pass sits from it, and
/// how much is within a tool radius of any cut at all. The places the program
/// leaves the key are the places it should: the five bosses (the key runs
/// straight through each bolt hole; the pass rounds the annulus, 5.5mm out,
/// which is what covers the boss's outer ring), the junctions (a pass stops
/// and the crossing pass covers), and the 14mm pad (the key gives it no
/// centreline; the rows take it).
#[test]
fn test_the_rail_passes_sit_on_the_answer_keys_centrelines() {
    let g = gcode::program(&load("curved_rails_spine.cam")).unwrap();
    let cuts = cuts_at_depth(&g, 0.0);
    let dist = |q: Pt| cuts.iter().map(|(a, b)| nacs::stock::dist_to_polygon(&[*a, *b], q.x, q.y)).fold(f64::INFINITY, f64::min);
    let edges = key_edges("curved_rails_offset.cam/key.svg");
    assert_eq!(edges.len(), 46);
    let (mut total, mut along, mut covered, mut dev_sum, mut dev_n) = (0.0, 0.0, 0.0, 0.0, 0usize);
    let mut per_class: std::collections::BTreeMap<&str, (f64, f64)> = Default::default();
    for (class, pts) in &edges {
        let len: f64 = pts.windows(2).map(|w| (w[1].x - w[0].x).hypot(w[1].y - w[0].y)).sum();
        let ds: Vec<f64> = pts.iter().map(|q| dist(*q)).collect();
        let al = ds.iter().filter(|d| **d <= 0.25).count() as f64 / ds.len() as f64;
        let cv = ds.iter().filter(|d| **d <= 5.0).count() as f64 / ds.len() as f64;
        total += len;
        along += len * al;
        covered += len * cv;
        for d in ds.iter().filter(|d| **d <= 0.25) {
            dev_sum += d;
            dev_n += 1;
        }
        let e = per_class.entry(class.as_str()).or_insert((0.0, 0.0));
        e.0 += len;
        e.1 += len * al;
    }
    assert!((total - 1165.3).abs() < 1.0, "the key is 1165.3mm of centreline: {total:.1}");
    // Where a pass runs along the key it sits on it to chord tolerance.
    let mean_dev = dev_sum / dev_n as f64;
    assert!(mean_dev < 0.03, "mean deviation along the key: {mean_dev:.3}");
    // Nine tenths of the key has a pass along it; all but the last half
    // percent is within a radius of a cut (that half percent is the pad's
    // dashed outline, which the key itself does not draw a centreline for).
    assert!(along / total > 0.90, "along: {:.1}%", 100.0 * along / total);
    assert!(covered / total > 0.995, "covered: {:.2}%", 100.0 * covered / total);
    // The curveballs, by name: the wave, the hairpin, the curl's R6 hook, the
    // ring, the tangent branch, both twin rails — each has its pass.
    for class in ["wave", "hairpin", "ring", "twin30", "twin36.5", "branch", "island"] {
        let (len, al) = per_class[class];
        assert!(al / len > 0.85, "{class}: {:.0}% along", 100.0 * al / len);
    }
    // The twin rails (9.5mm total under a 10mm tool) are cut as TWO rails
    // today. A single centred pass would cover both; when that lands, this
    // is the line that moves.
    assert!(per_class["twin30"].1 / per_class["twin30"].0 > 0.99 && per_class["twin36.5"].1 / per_class["twin36.5"].0 > 0.99);
}

// ---- Phase 32: walls on rails ----------------------------------------------------
// `walled_rails_offset.cam` is the staged puzzle: rails of 3, 6 and 8mm beside walls
// on the outer and the inner side of an arc, a wall ending mid-rail, a comb of
// slot floors 12 / 10.5 / 8 / 3mm wide between raised fins, a post through
// the land, a disc filling a bay, a collar round a boss. Every raised region
// lies inside the land's outer loop, so the engine reads all twelve as
// ISLANDS. Three stretches no 10mm tool can enter; its run line concedes them.

fn walled(main: &dyn Fn(String) -> String, geom: &dyn Fn(String) -> String) -> parse::Part {
    let m = main(std::fs::read_to_string("walled_rails_offset.cam/main.part").unwrap());
    let g = geom(std::fs::read_to_string("walled_rails_offset.cam/geometry.part").unwrap());
    parse::parse_project(&m, Some(&g)).unwrap()
}

fn unconceded(s: String) -> String {
    s.replace(", cover=92.7, pattern=offset)", ", pattern=offset)")
}

/// The text with one `feature … (` block removed — from its header line to
/// the next line that is just `)`.
fn without_feature(text: String, header: &str) -> String {
    let mut out = String::new();
    let mut skipping = false;
    for line in text.lines() {
        if line.starts_with(header) {
            skipping = true;
            continue;
        }
        if skipping {
            if line.trim() == ")" {
                skipping = false;
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Amendment 1: the refusal is ACTIONABLE. Without the concession the plate
/// refuses naming EVERY stretch it cannot reach — the three the answer key
/// marks `refuse`, at the key's widths, with what stands either side and the
/// largest tool that fits — then the reachable share and the exact word of
/// program text that would emit the rest. It is the same with a 5.5mm tool
/// (only the 3mm slot then), and the region reads every raised region as an
/// island.
#[test]
fn test_the_refusal_names_every_unreachable_stretch_and_the_word_that_concedes_it() {
    let err = gcode::program(&walled(&unconceded, &|g| g)).unwrap_err();
    assert!(err.starts_with("End on 'Land' with T5 (10.000mm) cannot reach 3 stretches of its floor"), "{err}");
    assert!(err.contains("between 'Fin3' and 'Fin4'") && err.contains("8.000mm wide — a tool up to 8.000mm would reach it"), "the 8mm slot: {err}");
    assert!(err.contains("between 'Fin4' and 'Fin5'") && err.contains("3.000mm wide — a tool up to 3.000mm would reach it"), "the 3mm slot: {err}");
    assert!(err.contains("'Disc' and 'WallWest'") && err.contains("5.99"), "B, walled both sides: {err}");
    assert!(err.contains("(92.7% reachable)") && err.contains("add cover=92.7 to its run line, then"), "{err}");
    // A 5.5mm tool reaches the 8mm slot and B; only the 3mm slot is left.
    let err = gcode::program(&walled(&|s| unconceded(s).replace("dia=10.0", "dia=5.5").replace("stepover=5.0", "stepover=2.75"), &|g| g)).unwrap_err();
    assert!(err.contains("cannot reach 1 stretch of its floor") && err.contains("'Fin4' and 'Fin5'"), "{err}");
    // Without the fifth fin the 3mm slot is a 3mm rail with one wall, and
    // two stretches are left.
    let err = gcode::program(&walled(&unconceded, &|g| without_feature(g, "feature Fin5 ProfileBoss("))).unwrap_err();
    assert!(err.contains("cannot reach 2 stretches"), "{err}");
    let p = walled(&|s| s, &|g| g);
    let region = region::of(&p, &p.operations[0]).unwrap();
    assert_eq!((region.open_edges(), region.edges.len(), region.feature_islands().len()), (88, 88, 12));
}

/// Amendment 2: the concession is BOUNDED. `cover=92.7` emits, and the
/// program carries the note; asking for more than is reachable refuses, with
/// the same list of stretches and the concession that would pass; the report
/// says what the run line concedes, and every pattern answers under it.
#[test]
fn test_the_concession_is_bounded_and_said_in_the_program_and_the_report() {
    let g = gcode::program(&walled(&|s| s, &|g| g)).unwrap();
    assert!(g.contains("(unreachable: 3 stretches, 419.4mm² of 5771.1mm² — 92.7% covered, cover=92.7 conceded;"), "the program says what it left");
    let err = gcode::program(&walled(&|s| s.replace("cover=92.7", "cover=95"), &|g| g)).unwrap_err();
    assert!(err.contains("cover=95 was conceded, but only 92.7% of the floor is reachable now") && err.ends_with("Concede cover=92.7 if that is intended"), "{err}");
    let err = parse::parse("units mm\nstock Box(w=10, l=10, h=5)\ntool T1 Endmill(dia=2)\nfeature F Plane(z=0)\nrun End(tool=T1, surface=F, stepover=1, cover=101)").unwrap_err();
    assert!(err.contains("cover=101 — the least share of the floor this op must reach, in percent, between 0 and 100"), "{err}");
    let p = walled(&|s| s, &|g| g);
    let text = compare::render(&p);
    assert!(text.contains("  reaches 92.7% of its floor — the run line concedes the rest (`cover=92.7`)\n"), "{text}");
    let r = &compare::op_reports(&p)[0];
    for row in &r.rows {
        let st = row.outcome.as_ref().unwrap_or_else(|e| panic!("{}: {e}", row.pattern.word()));
        assert_eq!(st.uncut_mm2, Some(0.0), "{}: every reachable cell is swept", row.pattern.word());
    }
    let refused = compare::render(&walled(&unconceded, &|g| g));
    assert!(refused.contains("  reaches 92.7% of its floor — every row below is refused unless the run line concedes the rest (`cover=92.7`)\n"), "{refused}");
}

/// What the concession leaves STANDS — in the declared stock model, for
/// whatever op comes next. Each stretch's outline is the exact edge of the
/// tool's reach (marching squares on the distance to where its centre may
/// be), so the entry check never sees a legitimate pass touch it and the model
/// never claims a slot the tool did not enter. The 3mm slot, the 8mm slot and
/// B still hold their stock after the Floor; a rail beside them does not.
#[test]
fn test_what_the_concession_leaves_stands_for_the_next_op() {
    let p = walled(&|s| format!("{s}\nrun End(tool=T5, surface=Land, stepover=5.0, cover=92.7)\n"), &|g| g);
    let after = nacs::stock::remaining_before(&p, 1);
    for (name, x, y) in [("the 3mm slot", 128.0, 18.6), ("the 8mm slot", 116.5, 20.2), ("B", 29.203, 49.4)] {
        assert!(after.material_at(x, y, -7.9), "{name} still holds stock above its floor");
    }
    assert!(!after.material_at(6.0, 70.0, -7.9), "the west perimeter rail was cut");
    assert!(!after.material_at(84.0, 20.0, -7.9), "the 12mm slot was cut");
}

// ---- Phase 32 slice 3: the claim and the permission are two sizes ----------------

/// Fork D. The declared removal claims what the PATTERN sweeps; the overhang
/// a face is allowed is a rule `check_moves` carries, not a claim about
/// material. So: three millimetres beyond the lawn's open south edge, the
/// model says scrap STANDS after a spine Floor (spine guarantees the floor and
/// nothing past it) — and the same program still emits, which is the
/// permission doing its job. Under the offset family the same point is
/// claimed removed, because its outermost pass does reach a diameter out.
#[test]
fn test_what_a_pattern_claims_and_what_it_may_overhang_are_different_sizes() {
    // The lawn's floor runs from y = 10; the tool is 10mm across.
    let (x, y_out, z) = (70.0, 7.0, -3.9);
    let spine = load("lawn_fence_bed_spine.cam");
    let offset = load("lawn_fence_bed_offset.cam");
    assert_eq!(spine.operations[0].pattern, Some(Pattern::Spine));
    assert_eq!(offset.operations[0].pattern, Some(Pattern::Offset));

    let after_spine = nacs::stock::remaining_after(&spine, 0);
    let after_offset = nacs::stock::remaining_after(&offset, 0);
    assert!(after_spine.material_at(x, y_out, z), "spine claims nothing past the floor's edge");
    assert!(!after_offset.material_at(x, y_out, z), "the offset family's outermost pass reaches a diameter out, and claims it");
    // On the floor itself both claim, and both sweep.
    assert!(!after_spine.material_at(x, 20.0, z) && !after_offset.material_at(x, 20.0, z));

    // The permission: the narrower claim does not make the op's own overhang
    // a foul. Both still emit.
    gcode::program(&spine).expect("spine emits — its face may overhang its own edge");
    gcode::program(&offset).expect("so does the offset family");
}

/// Phase 40 slice 0 (`DECISIONS.md` → "Phase 40 slice 0…"): the same two sizes
/// for the patterns that run rows. Three millimetres past `thin_wall`'s open
/// south edge the model says scrap STANDS after a zigzag `End` and after a
/// oneway one — their rows end a radius out, but between two row ends is a
/// crescent no disc reaches, and the lap that used to sweep it is not a
/// promise the rows make — and is GONE after an offset `End`, whose outermost
/// loop rides there all the way round. On the floor all three claim and all
/// three sweep; all three still emit, the permission being unchanged.
#[test]
fn test_the_parallel_passes_claim_the_floor_as_spine_does() {
    // The Face runs from y = 20 under a 10mm tool; its south edge is open.
    let (x, y_out, z) = (35.0, 17.0, -3.9);
    let src = std::fs::read_to_string("thin_wall.cam/main.part").unwrap();
    let run = src.lines().find(|l| l.starts_with("run ")).unwrap();
    for (word, stands) in [("zigzag", true), ("oneway", true), ("spine", true), ("offset", false)] {
        let p = parse::parse(&src.replace(run, &nacs::view::set_key(run, "pattern", Some(word)).unwrap())).unwrap();
        let after = nacs::stock::remaining_after(&p, 0);
        assert_eq!(after.material_at(x, y_out, z), stands, "{word}: three millimetres past the open edge");
        assert!(!after.material_at(x, 30.0, z), "{word}: the floor is claimed");
        gcode::program(&p).unwrap_or_else(|e| panic!("{word} emits — its face may overhang its own edge: {e}"));
    }
}

/// …and the field agrees, BOTH WAYS. With the claim narrowed to what spine
/// sweeps, the rest report reads FORGOTTEN NONE (`lawn_fence_bed_spine` read
/// 31800 cells of scrap claimed and never swept) — and VIOLATED NONE, which
/// is the other half: the permission is a rule with two readers, and a
/// narrower claim without it would have called every permitted overhang a
/// violation instead (19764 cells on the same fixture, all of them inside an
/// op's own swath and none inside any declared feature). Nothing about the
/// toolpath changed; the claim did.
#[test]
fn test_the_field_finds_nothing_forgotten_and_nothing_violated_under_a_spine_program() {
    for dir in ["lawn_fence_bed_spine.cam", "wrench_step_spine.cam", "curved_rails_spine.cam"] {
        let p = load(dir);
        let (_, emitted) = gcode::program_with_moves(&p).unwrap();
        let cells = nacs::vox::rest_cells(&p, &emitted, p.operations.len(), None).unwrap();
        assert!(cells.forgotten.is_empty(), "{dir}: {} cells claimed removed and never swept", cells.forgotten.len());
        assert!(cells.violated.is_empty(), "{dir}: {} cells the declaration says stand, swept anyway", cells.violated.len());
    }
}

/// The permission covers undeclared STOCK, never a declaration. A fin stands
/// inside the Land's own swath on `walled_rails_offset.cam` — the op may overhang
/// the scrap around it, and the fin is still the part: `claim_at` answers
/// with the feature's name, so sweeping it would still be a violation. That
/// ordering is what keeps Phase 29's raised ring safe.
#[test]
fn test_the_permission_does_not_swallow_a_declaration() {
    let p = walled(&|s| s, &|g| g);
    let part = nacs::stock::DeclaredPart::of(&p);
    // Inside 'Fin3' (the rib between the 10.5mm and 8mm slots: x 106.5..112.5,
    // y 18..52 in the part's frame), above the land's floor at z = -8.
    match part.claim_at(109.5, 35.0, -4.0) {
        Some(nacs::stock::PartClaim::Feature(name)) => assert_eq!(name, "Fin3"),
        other => panic!("a declared fin inside the swath is still the part: {other:?}"),
    }
    // …while the scrap beside the plate, inside the same swath, is not.
    assert!(part.claim_at(-2.0, 65.0, -4.0).is_none(), "scrap the op may overhang is nobody's part");
}

/// Phase 32 slice 4 — a SHORE is a pass, and it runs only where there is
/// floor to clean. Two defects, one mechanism:
///
/// 1. A wall is a region whose one edge touches this floor and whose others
///    do not, so the allowed boundary wraps it all the way round; the shore
///    now stops where the floor beside it does, instead of cutting scrap for
///    the wall's whole length.
/// 2. The rows are told what the shores cover, as they were already told what
///    the rails cover. An 8mm rail beside a curved wall was five rows of
///    teeth AND the shore over the same floor — 140mm; it is now the shore
///    alone, 53mm, every millimetre of it on the floor.
#[test]
fn test_a_shore_runs_only_where_there_is_floor_and_the_rows_know_it_covers() {
    // A quarter-arc rail 8mm wide with a wall along its outer side, in the
    // frame `spine::Ask` speaks: where the centre may be, the floor, the wall.
    let arc = |r: f64, a0: f64, a1: f64| -> Vec<Pt> {
        (0..=40).map(|k| (a0 + (a1 - a0) * k as f64 / 40.0).to_radians()).map(|a| Pt { x: 10.0 + r * a.cos(), y: 5.0 + r * a.sin() }).collect()
    };
    let rail: Vec<Pt> = arc(31.5, 0.0, 90.0).into_iter().chain(arc(23.5, 90.0, 0.0)).collect();
    let allowed: Vec<Pt> = arc(26.5, 0.0, 90.0).into_iter().chain(arc(18.5, 90.0, 0.0)).collect();
    let wall: Vec<(Pt, Pt)> = arc(31.5, 0.0, 90.0).windows(2).map(|w| (w[0], w[1])).collect();
    let floor = vec![rail];
    let never = |_: Pt| false;
    let ask = spine::Ask {
        ccw: &[allowed], cw: &[], floor: &floor, walls: &wall, r: 5.0, stand_off: 5.0, stepover: 5.0,
        lift_cost: 18.0, ramp_len: 20.0, in_air: &never, level: &[], level_d: 0.0, held: None, conventional: false,
    angle: None,
    };
    let plan = spine::plan(&ask).unwrap();
    assert_eq!(plan.shores, 1, "one wall, one shore");
    assert_eq!(plan.rows + plan.cross_rows, 0, "the shore covers this rail; no row owns anything left");

    // The shore is as long as its wall (a quarter of a 26.5 circle is 41.6mm,
    // plus a lead at each end), and it lies ON the floor.
    assert!((36.0..60.0).contains(&plan.shore_mm), "{}", plan.shore_mm);
    let pts: Vec<Pt> = plan.chains.iter().flat_map(|c| c.pts.iter().copied()).collect();
    let on_floor = pts.iter().filter(|q| nacs::stock::point_in_polygon(&floor[0], q.x, q.y)).count();
    assert!(on_floor * 10 >= pts.len() * 9, "the pass stays on the rail: {on_floor} of {}", pts.len());
}

// ---- Phase 32 slice 5: the answer key, scored -----------------------------------
// `walled_rails_offset.cam/key.json` gives 53 edges of the part's centreline graph,
// each with the wall on its side and the pass its author expects. This scores
// the program spine actually emits against it — the Phase 31 pattern, one part
// further on.

/// One key edge's centreline, sampled in the PART's frame (the key's
/// coordinates are the drawing's, y down, height 130).
fn key_edge_samples(e: &str) -> Vec<Pt> {
    let (s, t) = (e.split("\"start\": [").nth(1).unwrap(), e.split("\"end\": [").nth(1).unwrap());
    let num = |txt: &str| -> (f64, f64) {
        let v: Vec<f64> = txt.split(']').next().unwrap().split(',').map(|n| n.trim().parse().unwrap()).collect();
        (v[0], 130.0 - v[1])
    };
    let (a, b) = (num(s), num(t));
    // Straight samples between the ends: every key edge is a line or an arc,
    // and an arc's chord midpoint is inside its own rail either way — the
    // ends alone would miss what happens along it.
    (1..10).map(|k| k as f64 / 10.0).map(|f| Pt { x: a.0 + (b.0 - a.0) * f, y: a.1 + (b.1 - a.1) * f }).collect()
}

/// The program's at-depth cutting moves, as segments.
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

/// THE KEY, SCORED. Every edge its author marks `rail`, `shore` or `two
/// passes` is CUT by the program — some pass comes within the tool's radius
/// of its centreline — and every edge marked `refuse` is LEFT: no pass comes
/// near it, because a 10mm tool cannot enter it and the run line conceded it
/// rather than pretending otherwise. That is the whole claim of this phase on
/// the part it was staged for.
#[test]
fn test_the_program_answers_the_key_edge_by_edge() {
    let g = gcode::program(&load("walled_rails_spine.cam")).unwrap();
    let cuts = at_depth(&g, -8.0);
    let near = |q: Pt| cuts.iter().map(|(a, b)| nacs::stock::dist_to_polygon(&[*a, *b], q.x, q.y)).fold(f64::INFINITY, f64::min);
    let key = std::fs::read_to_string("walled_rails_offset.cam/key.json").unwrap();
    let (mut cut, mut left, mut wrong) = (0, 0, Vec::new());
    for e in key.split("{\"id\":").skip(1) {
        let expected = e.split("\"expected\": \"").nth(1).unwrap().split('"').next().unwrap();
        let id = e.split('"').nth(1).unwrap();
        if !matches!(expected, "rail" | "shore" | "two passes" | "refuse") {
            continue; // the two zones (the pad, the boss) are areas, not edges
        }
        let samples = key_edge_samples(e);
        let closest = samples.iter().map(|q| near(*q)).fold(f64::INFINITY, f64::min);
        match expected {
            "refuse" => {
                // Nothing may come within a radius of a stretch no pass fits.
                if closest <= 5.0 {
                    wrong.push(format!("{id} ({expected}): a pass comes {closest:.2}mm from it"));
                } else {
                    left += 1;
                }
            }
            _ => {
                if closest > 5.0 {
                    wrong.push(format!("{id} ({expected}): nothing within {closest:.2}mm"));
                } else {
                    cut += 1;
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{} of the key's edges disagree: {}", wrong.len(), wrong.join("; "));
    // 36 rail + 6 shore + 4 two-passes = 46 cut; 3 refuse left; the two
    // zones (the pad, the boss) are areas and are scored by `uncut`.
    assert_eq!((cut, left), (46, 3), "the key's own tally");
}

/// THE NOOKS. The offset family claims the scrap past a face's edge — but
/// only the scrap its tool can REACH: the swath's opening, the family's
/// allowed region grown back out by a radius. Before it, these read
/// forgotten 283mm³ (the lawn: the nooks past a wall chain's end), 221mm³
/// (the strip behind a wall thinner than the push), 18mm³ (the plate's
/// collar) and a few cells on the wrench — scrap claimed that no pass could
/// enter. The FLOOR too is claimed as reached (2026-11-10): the cottage's
/// inside corner no 10mm tool reaches, 33 cells once forgotten, is declared
/// standing, and nothing is forgotten on any of the six.
#[test]
fn test_the_offset_family_claims_only_the_scrap_it_can_reach() {
    for dir in ["lawn_fence_bed_offset.cam", "thin_wall.cam", "walled_rails_offset.cam", "wrench_step_offset.cam", "wrench_thick_offset.cam", "cottage_interior_offset.cam"] {
        let p = load(dir);
        let (_, emitted) = gcode::program_with_moves(&p).unwrap();
        let cells = nacs::vox::rest_cells(&p, &emitted, p.operations.len(), None).unwrap();
        assert!(cells.violated.is_empty(), "{dir}: {} cells the declaration says stand, swept anyway", cells.violated.len());
        let op = &p.operations[0];
        let parse::Feature::Face { loops, .. } = &p.features[&op.feature_name] else { panic!("{dir}: a Face") };
        let off_floor: Vec<_> = cells.forgotten.iter().filter(|c| !nacs::stock::point_in_polygon(&loops[0], c[0], c[1])).collect();
        assert!(off_floor.is_empty(), "{dir}: {} cells of SCRAP claimed and never swept, first at {:?}", off_floor.len(), off_floor[0]);
        assert!(cells.forgotten.is_empty(), "{dir}: {} cells forgotten", cells.forgotten.len());
    }
}

/// The word spine was born under refuses, naming the word that replaced it
/// (Phase 35 slice 1): never an alias.
#[test]
fn test_the_retired_word_refuses_and_names_its_successor() {
    let err = parse::parse("units mm\nstock Box(w=10, l=10, h=5)\ntool T1 Endmill(dia=2)\nfeature F Plane(z=0)\nrun End(tool=T1, surface=F, stepover=1, pattern=mow)").unwrap_err();
    assert!(err.contains("pattern=mow is now pattern=spine"), "{err}");
}

/// Phase 38 slice 0: the price a comb was chosen by is `pattern::price` —
/// the one home the parallel passes choose their direction by too. A plan's
/// own `cost` is that price on its own chains, with the lift it was asked
/// with.
#[test]
fn test_spines_cost_is_the_one_price() {
    for dir in ["wrench_spine.cam", "rail_plate_spine.cam", "pocket_post_spine.cam"] {
        let p = load(dir);
        let idx = p.operations.iter().position(|op| op.pattern == Some(Pattern::Spine)).unwrap();
        let plan = plan_of(&p, idx, None);
        let price = nacs::pattern::price(&plan.chains, 18.0);
        assert!((price - plan.cost).abs() < 1e-6, "{dir}: cost {} against the price {}", plan.cost, price);
    }
}
