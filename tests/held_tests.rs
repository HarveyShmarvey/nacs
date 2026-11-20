// ---- held: the fifth pattern word (Phase 62 slice 1) -----------------------------
// `DECISIONS.md` → "Phase 62 slice 1…". Rows HELD under a ceiling on
// engagement: the spacing derived from the angle, the comb walked from the
// air, every link read through the walk as it is laid and lifted where it
// would exceed the ceiling, the places the ceiling cannot hold said by name.
// Every claim the entry makes is a number pinned here.

use nacs::parse::{self, Pattern, Pt};
use nacs::{compare, gcode, ops, region, view};

fn load(dir: &str) -> parse::Part {
    let main = std::fs::read_to_string(format!("{dir}/main.part")).unwrap();
    let geom = std::fs::read_to_string(format!("{dir}/geometry.part")).ok();
    parse::parse_project(&main, geom.as_deref()).unwrap_or_else(|e| panic!("{dir}: {e}"))
}

/// plate_held.cam's main.part with its Clear line replaced.
fn plate_with(clear_line: &str) -> Result<parse::Part, String> {
    let main = std::fs::read_to_string("plate_held.cam/main.part").unwrap();
    let geom = std::fs::read_to_string("plate_held.cam/geometry.part").unwrap();
    let main = main.replace("run Clear(tool=T5, profile=Notch, pattern=held)", clear_line);
    assert!(main.contains(clear_line), "the fixture's Clear line is the one this test rewrites");
    parse::parse_project(&main, Some(&geom))
}

fn row<'a>(r: &'a compare::OpReport, k: Pattern) -> &'a compare::Stats {
    r.rows.iter().find(|row| row.pattern == k).unwrap().outcome.as_ref().unwrap_or_else(|e| panic!("{}: {e}", k.word()))
}

fn in_air(p: &parse::Part, q: Pt) -> bool {
    let blank = parse::derive_blank(&p.stock);
    !nacs::stock::point_in_polygon(&blank, q.x, q.y) && nacs::stock::dist_to_polygon(&blank, q.x, q.y) >= 5.0 - nacs::CHORD_TOL_MM
}

// ---- the spacing is a theorem of the angle -------------------------------------

#[test]
fn test_the_spacing_is_derived_from_the_ceiling_and_the_tool() {
    // `acos(1 − a_e/r)` is the engagement of a stroke beside a previous pass
    // at radial depth `a_e`; the depth that engages exactly θ is `r(1 − cos θ)`.
    assert!((parse::held_spacing(5.0, 90.0) - 5.0).abs() < 1e-12, "the radius itself at 90°");
    assert!((parse::held_spacing(5.0, 60.0) - 2.5).abs() < 1e-12, "half the radius at 60°");
    assert!((parse::held_spacing(3.0, 45.0) - 3.0 * (1.0 - 45f64.to_radians().cos())).abs() < 1e-12);
    // One home for the stepover: the authored one, or the derived one.
    let held = load("plate_held.cam");
    assert_eq!(held.operations[1].stepover, None, "a held op authors no stepover");
    assert_eq!(parse::derive_held_ceiling(&held.operations[1]), 90.0, "the default ceiling is the 90° line");
    assert_eq!(parse::derive_stepover(&held, &held.operations[1]), Some(5.0), "r(1 − cos 90°) on a ⌀10");
    let plate = load("plate.cam");
    assert_eq!(parse::derive_stepover(&plate, &plate.operations[1]), Some(4.0), "an authored stepover is read as authored");
    let lowered = plate_with("run Clear(tool=T5, profile=Notch, pattern=held, engagement=60)").unwrap();
    assert_eq!(parse::derive_held_ceiling(&lowered.operations[1]), 60.0);
    assert!((parse::derive_stepover(&lowered, &lowered.operations[1]).unwrap() - 2.5).abs() < 1e-9);
}

// ---- the three refusals, and the key's one home --------------------------------

#[test]
fn test_a_stepover_beside_held_is_two_governors() {
    let err = plate_with("run Clear(tool=T5, profile=Notch, pattern=held, stepover=4.0)").unwrap_err();
    assert!(err.contains("two governors"), "{err}");
    assert!(err.contains("= 5.000 on T5"), "the refusal says the spacing the ceiling would derive: {err}");
    assert!(err.contains("drop the stepover"), "{err}");
}

#[test]
fn test_the_ceiling_is_held_s_one_key() {
    let err = plate_with("run Clear(tool=T5, profile=Notch, stepover=4.0, pattern=spine, engagement=80)").unwrap_err();
    assert!(err.contains("engagement=80 on 'Notch'") && err.contains("pin the pattern, or drop the key"), "{err}");
    let err = plate_with("run Clear(tool=T5, profile=Notch, stepover=4.0, engagement=80)").unwrap_err();
    assert!(err.contains("pin the pattern"), "an unpinned Clear is the offset family, not held: {err}");
}

#[test]
fn test_the_ceiling_cannot_be_raised_past_the_line_or_below_zero() {
    let err = plate_with("run Clear(tool=T5, profile=Notch, pattern=held, engagement=95)").unwrap_err();
    assert!(err.contains("slot by declaration") && err.contains("leave the key off for 90deg"), "{err}");
    let err = plate_with("run Clear(tool=T5, profile=Notch, pattern=held, engagement=0)").unwrap_err();
    assert!(err.contains("must be positive"), "{err}");
    assert!(plate_with("run Clear(tool=T5, profile=Notch, pattern=held, engagement=90)").is_ok(), "the line itself is legal, said as authored");
}

#[test]
fn test_held_is_legal_exactly_where_a_pattern_is() {
    let err = plate_with("run Side(tool=T5, surface=Notch, comp=control, pattern=held)").unwrap_err();
    assert!(err.contains("pattern=held on 'Notch'") && err.contains("a pattern is how a floor's region is walked"), "{err}");
}

// ---- the fixture, its program, and what the program says ----------------------

#[test]
fn test_the_held_fixture_is_golden() {
    let g = gcode::program(&load("plate_held.cam")).unwrap();
    let golden = std::fs::read_to_string("tests/plate_held.golden.gcode").unwrap();
    assert_eq!(g.trim(), golden.trim());
}

#[test]
fn test_the_program_says_the_ceiling_the_spacing_the_lifts_and_what_ran_over() {
    let g = gcode::program(&load("plate_held.cam")).unwrap();
    let note = g.lines().find(|l| l.starts_with("(pattern: held")).expect("the held note");
    assert_eq!(
        note,
        "(pattern: held under 90deg, the default: 11 rows at 5.000, the spacing the ceiling derives on T5, on spine's comb at 90.000deg, then 1 shore pass along what stands; 5 lifts where a link would exceed the ceiling; over it: 20.000mm on the rows, 4.991mm on the shores)"
    );
    // A lowered ceiling is said as authored, and the spacing follows it.
    let g = gcode::program(&plate_with("run Clear(tool=T5, profile=Notch, pattern=held, engagement=60)").unwrap()).unwrap();
    let note = g.lines().find(|l| l.starts_with("(pattern: held")).unwrap();
    assert!(note.starts_with("(pattern: held under 60deg: ") && note.contains(" at 2.500, the spacing the ceiling derives on T5"), "{note}");
    assert!(!note.contains("the default"), "{note}");
}

// ---- the construction: from the air, the links read, the rest said -----------

/// The plan as the emitter asks for it on the notch (the emitter's own lift
/// price and ramp room, so the comb is the program's).
fn notch_plan(p: &parse::Part) -> nacs::spine::Plan {
    let op = &p.operations[1];
    let reg = region::of(p, op).unwrap();
    let lift_cost = (5.0 - (-20.0)) * (1.0 + 800.0 / 300.0);
    let ramp_len = ops::clear::ramp_room(20.0).max(10.0);
    ops::clear::held_plan(p, op, &reg, 0.0, (lift_cost, ramp_len), -20.0).unwrap()
}

#[test]
fn test_the_comb_is_walked_from_the_air() {
    // The notch is open on its west and north edges and walled on its east
    // and south. Spine's price ties the two ways of a comb and keeps the
    // first, which on this floor cut a 58mm slot beside the wall; held
    // takes the way from the air, so the first row rides the open edge.
    let p = load("plate_held.cam");
    let plan = notch_plan(&p);
    assert_eq!(plan.angle_deg, 90.0, "rows along Y, from the west edge toward the L's east wall");
    let first = plan.chains.first().unwrap();
    assert!(!first.ramp, "entered from the air, not ramped into solid");
    assert!(in_air(&p, first.pts[0]), "the first point clears the blank by a radius: {:?}", first.pts[0]);
    assert!((first.pts[0].x - 0.0).abs() < 1e-9, "the first row rides the open edge, one spacing deep: {:?}", first.pts[0]);
    // What spine cuts at the same spacing: the price ties the two ways of
    // its comb and keeps the first, whose first row lies beside the south
    // wall — entered from the west air, and a slot for its whole length.
    let spine = plate_with("run Clear(tool=T5, profile=Notch, stepover=5.0, pattern=spine)").unwrap();
    let op = &spine.operations[1];
    let plan = ops::clear::spine_plan(&spine, op, &region::of(&spine, op).unwrap(), 0.0, ((25.0) * (1.0 + 800.0 / 300.0), ops::clear::ramp_room(20.0).max(10.0)), None).unwrap();
    assert_eq!(plan.angle_deg, 0.0);
    assert!((plan.chains[0].pts[0].y - 50.0).abs() < 1e-9, "spine's first row is 10 off the wall at y=40: {:?}", plan.chains[0].pts[0]);
    let r = compare::op_report(&spine, 1).unwrap();
    assert!(row(&r, Pattern::Spine).load.as_ref().unwrap().slot_mm >= 55.0, "{}", row(&r, Pattern::Spine).load.as_ref().unwrap().slot_mm);
}

#[test]
fn test_a_link_that_would_exceed_the_ceiling_is_a_lift() {
    let p = load("plate_held.cam");
    let plan = notch_plan(&p);
    let read = plan.held.as_ref().expect("a held plan carries its read");
    assert!(read.read, "the walk was read: the stock before the op is closed form");
    // Rows along Y turn at the north end in air (links kept) and at the
    // south wall into the next uncut band (a slot of a spacing at 165°,
    // every other row): those five are lifts.
    assert_eq!(read.lifts, 5);
    assert_eq!(plan.rows, 11);
    // The chains: one per lift plus the first, the shore joined to the last.
    assert_eq!(plan.chains.len(), 6, "{}", plan.chains.len());
    // What is left over the ceiling is said by kind: the rows' end caps
    // against the wall, and the shore's turn at the inside corner — and
    // nothing on a link.
    let over: Vec<(&str, f64)> = read.over.iter().map(|(k, mm)| (*k, (mm * 1000.0).round() / 1000.0)).collect();
    assert_eq!(over, vec![("the rows", 20.0), ("the shores", 4.991)]);
}

#[test]
fn test_the_report_measures_held_beside_the_four() {
    let p = load("plate_held.cam");
    let r = compare::op_report(&p, 1).unwrap();
    let words: Vec<&str> = r.rows.iter().map(|row| row.pattern.word()).collect();
    assert_eq!(words, ["offset", "zigzag", "oneway", "spine", "held"]);
    assert!(r.rows.iter().find(|row| row.pattern == Pattern::Held).unwrap().current, "the pin is the row that cuts today");
    let held = row(&r, Pattern::Held);
    assert_eq!(held.uncut_mm2, Some(0.0), "the Phase 30 gate");
    let met = |k: Pattern| row(&r, k).load.as_ref().unwrap();
    assert_eq!(met(Pattern::Held).slot_mm, 0.0, "no stretch at full width");
    // The acceptance (fork 6A): past 90° LOWER than every other row's.
    for k in [Pattern::Offset, Pattern::Zigzag, Pattern::Oneway, Pattern::Spine] {
        assert!(met(Pattern::Held).past_half_mm < met(k).past_half_mm, "held {} vs {} {}", met(Pattern::Held).past_half_mm, k.word(), met(k).past_half_mm);
    }
    // …and the measured numbers, to the millimetre: the entry's table.
    assert_eq!(met(Pattern::Held).past_half_mm.round(), 25.0);
    assert_eq!(met(Pattern::Spine).past_half_mm.round(), 86.0);
    assert_eq!(met(Pattern::Spine).slot_mm.round(), 55.0);
    // Every other row of a held op cuts at the spacing the ceiling derived,
    // so one spacing runs across the table and the word is the difference.
    let spine_at_5 = compare::op_report(&plate_with("run Clear(tool=T5, profile=Notch, stepover=5.0, pattern=spine)").unwrap(), 1).unwrap();
    assert_eq!(row(&r, Pattern::Spine).at_depth_mm, row(&spine_at_5, Pattern::Spine).at_depth_mm);
    // And a held row stands in every op's table, at its spacing, with the
    // authored stepover's rows beside it untouched.
    let plate = compare::op_report(&load("plate.cam"), 1).unwrap();
    assert_eq!(plate.rows.len(), 5);
    assert_eq!(row(&plate, Pattern::Held).at_depth_mm, held.at_depth_mm, "the held row of plate.cam's op is plate_held's program");
    assert_eq!(row(&plate, Pattern::Spine).load.as_ref().unwrap().past_half_mm.round(), 81.0, "spine at the authored 4.0, as before");
}

#[test]
fn test_the_pin_writes_the_word_and_the_stepover_it_refuses_or_needs() {
    let plate = load("plate.cam");
    assert_eq!(view::pin_writes(&plate, 1, "held"), vec![("pattern", Some("held".to_string())), ("stepover", None)]);
    assert_eq!(view::pin_writes(&plate, 1, "zigzag"), vec![("pattern", Some("zigzag".to_string()))]);
    let held = load("plate_held.cam");
    assert_eq!(view::pin_writes(&held, 1, "spine"), vec![("pattern", Some("spine".to_string())), ("stepover", Some("5".to_string()))]);
    assert_eq!(view::pin_writes(&held, 1, "held"), vec![("pattern", Some("held".to_string()))]);
    let lowered = plate_with("run Clear(tool=T5, profile=Notch, pattern=held, engagement=60)").unwrap();
    assert_eq!(view::pin_writes(&lowered, 1, "oneway"), vec![("pattern", Some("oneway".to_string())), ("stepover", Some("2.5".to_string())), ("engagement", None)]);
    // The written lines parse: a pin is text the author could have typed.
    let line = "run Clear(tool=T5, profile=Notch, pattern=held)";
    let mut l = line.to_string();
    for (k, v) in view::pin_writes(&held, 1, "spine") {
        l = view::set_key(&l, k, v.as_deref()).unwrap();
    }
    assert!(plate_with(&l).is_ok(), "{l}");
    let mut l = "run Clear(tool=T5, profile=Notch, stepover=4.0, pattern=spine)".to_string();
    for (k, v) in view::pin_writes(&plate, 1, "held") {
        l = view::set_key(&l, k, v.as_deref()).unwrap();
    }
    assert!(plate_with(&l).is_ok(), "{l}");
}

#[test]
fn test_held_reads_the_form_s_menu_and_the_reference() {
    assert!(view::enum_options("pattern").unwrap().contains(&"held".to_string()));
    assert_eq!(view::unit_of("engagement", true), Some("°"));
    assert!(parse::RUN_KEYS.contains(&"engagement"));
    assert_eq!(Pattern::ALL.len(), 5);
}

#[test]
fn test_the_immersion_line_reads_the_derived_spacing() {
    // The layer's nominal is the stepover's angle; a held op's is its ceiling.
    let line = nacs::engagement::immersion_report(&load("plate_held.cam"), 1).unwrap();
    assert!(line.contains("within the stepover's 90°"), "{}", line.lines().next().unwrap());
    let line = nacs::engagement::immersion_report(&plate_with("run Clear(tool=T5, profile=Notch, pattern=held, engagement=60)").unwrap(), 1).unwrap();
    assert!(line.contains("within the stepover's 60°"), "{}", line.lines().next().unwrap());
}

// ---- slice 2: the closed floor ---------------------------------------------------

#[test]
fn test_the_closed_floor_fixture_is_golden_and_says_its_first_slot() {
    let p = load("pocket_post_held.cam");
    let g = gcode::program(&p).unwrap();
    let golden = std::fs::read_to_string("tests/pocket_post_held.golden.gcode").unwrap();
    assert_eq!(g.trim(), golden.trim());
    let note = g.lines().find(|l| l.starts_with("(pattern: held")).unwrap();
    assert_eq!(
        note,
        "(pattern: held under 90deg, the default: 9 rows at 5.000, the spacing the ceiling derives on T5, on spine's comb at 90.000deg, then 2 shore passes along what stands; 13 lifts where a link would exceed the ceiling; over it: 50.000mm on the first stroke, 65.082mm on the rows, 13.000mm on the shores)"
    );
    // No side in air: the first stroke is a slot the whole way, entered by
    // a ramp under the law, and said by name with its length.
    let op = &p.operations[0];
    let reg = region::of(&p, op).unwrap();
    let plan = ops::clear::held_plan(&p, op, &reg, 0.0, ((5.0 + 5.0) * (1.0 + 800.0 / 300.0), ops::clear::ramp_room(5.0).max(10.0)), -5.0).unwrap();
    assert!(plan.chains[0].ramp, "no air to enter from");
    let read = plan.held.as_ref().unwrap();
    assert_eq!(read.over[0].0, "the first stroke");
    assert!((read.over[0].1 - 50.0).abs() < 1e-9, "{:?}", read.over);
    assert_eq!(read.lifts, 13, "every turn is at a wall");
}

#[test]
fn test_a_held_comb_hands_over_no_teeth() {
    // Spine's crossed pair cuts a tooth's window as a cross row BEFORE the
    // shores run — a slot through uncut floor by construction. Under a
    // ceiling the single comb cuts its teeth as short rows instead.
    let p = load("pocket_post_held.cam");
    let op = &p.operations[0];
    let reg = region::of(&p, op).unwrap();
    // (The emitter's own lift price and ramp room, so the combs are the programs'.)
    let costs = ((5.0 + 5.0) * (1.0 + 800.0 / 300.0), ops::clear::ramp_room(5.0).max(10.0));
    let plan = ops::clear::held_plan(&p, op, &reg, 0.0, costs, -5.0).unwrap();
    assert_eq!(plan.cross_rows, 0);
    // Spine at the same spacing, pinned, takes the single comb by its own
    // price; held took the PAIR before the rule — its single comb pays a
    // lift at every wall-end turn, which made the pair the cheaper plan and
    // its windows slots. With no pair to take, held's comb is spine's.
    let main = std::fs::read_to_string("pocket_post_held.cam/main.part").unwrap().replace("pattern=held)", "stepover=5.0, pattern=spine)");
    let spine = parse::parse_project(&main, None).unwrap();
    let sp = &spine.operations[0];
    let sp_plan = ops::clear::spine_plan(&spine, sp, &region::of(&spine, sp).unwrap(), 0.0, costs, None).unwrap();
    assert_eq!((sp_plan.rows, sp_plan.cross_rows, sp_plan.angle_deg), (9, 0, 90.0));
    assert_eq!((plan.rows, plan.cross_rows, plan.angle_deg), (9, 0, 90.0));
    assert!(plan.chains.len() > sp_plan.chains.len(), "held's chains are spine's rows, each lifted to: {} against {}", plan.chains.len(), sp_plan.chains.len());
}

#[test]
fn test_the_closed_floor_is_where_the_premise_does_not_hold() {
    // Fork 6A's acceptance — past 90° lower than every other row's — holds on
    // the open notch and FAILS here, and the phase says so with the table:
    // every edge stands, every row has two end caps against a wall, and
    // oneway's rows carry the same caps at a smaller spacing with no shore
    // band; held sits between oneway and spine. Pinned as measured.
    let r = compare::op_report(&load("pocket_post_held.cam"), 0).unwrap();
    let met = |k: Pattern| row(&r, k).load.as_ref().unwrap();
    assert_eq!(row(&r, Pattern::Held).uncut_mm2, Some(0.0));
    assert_eq!(met(Pattern::Held).past_half_mm.round(), 128.0);
    assert_eq!(met(Pattern::Held).slot_mm.round(), 50.0, "the first stroke, and nothing else at full width");
    assert!(met(Pattern::Oneway).past_half_mm < met(Pattern::Held).past_half_mm, "oneway {} < held {}", met(Pattern::Oneway).past_half_mm, met(Pattern::Held).past_half_mm);
    assert!(met(Pattern::Held).past_half_mm < met(Pattern::Spine).past_half_mm);
    assert!(met(Pattern::Held).past_half_mm < met(Pattern::Zigzag).past_half_mm);
    assert!(met(Pattern::Held).past_half_mm < met(Pattern::Offset).past_half_mm);
    assert!(met(Pattern::Held).slot_mm <= met(Pattern::Oneway).slot_mm, "held's only slot is the first stroke oneway has too");
}
