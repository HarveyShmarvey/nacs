// ---- the lens: the air a tool body can stand in (Phase 42 slice 2) ----------
//
// `DECISIONS.md` → "Phase 42 slice 2…", fork 2 of the Phase 42 DRAFT. The
// envelope is the union of a tilted bore's sections over its depth — the
// keep-out `--arc` builds against — and the wrong curve for a region the
// tool may occupy: a tool spanning the floor's depth is in air at a plan
// point only if that point is inside the void at EVERY height the body
// occupies, each height's section eroded by the body's radius there. That
// is `stock::given_air`: per given void, per op, the CENTRE set and the
// floor it reaches with the cutting radius back on, from K2's declared
// pieces, closed form (the lens of two sections, erosion commuting with
// intersection). These pins hold the column case by hand, the body's
// stretches, the holder that does not fit, the tilted bore's lens against
// the certificate, the floor below the bore, the outline's voice and the
// wall stroke.

use nacs::parse::{self, Pt};
use nacs::{outline, stock, view};

const HEAD: &str = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\n";
const DRAIN: &str = "feature Drain TiltedBore(dia=12, extent=20, x=70, y=40, tilt=30, heading=180)\ngiven Drain\n";
const HOLE: &str = "feature Hole CylinderBore(dia=12, extent=20, x=70, y=40)\ngiven Hole\n";
/// A pocket 8 deep, apart from both voids: the floor the lens is judged for.
const POCKET: &str = "feature Pocket ProfilePocket(extent=8\n    10 20\n    50 20\n    50 60\n    10 60\n)\n";

fn program(lines: &str) -> parse::Part {
    parse::parse(&format!("{HEAD}{lines}")).unwrap_or_else(|e| panic!("must parse: {e}"))
}

fn bbox(lp: &[Pt]) -> (f64, f64, f64, f64) {
    lp.iter().fold((f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY), |b, q| {
        (b.0.min(q.x), b.1.min(q.y), b.2.max(q.x), b.3.max(q.y))
    })
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn tan30() -> f64 {
    30f64.to_radians().tan()
}

fn semi_a() -> f64 {
    6.0 / 30f64.to_radians().cos()
}

#[test]
fn test_a_column_void_with_no_body_declared_is_the_section_eroded_by_the_flutes() {
    // No body: one stretch at the cutting radius from the tip (z −8) to the
    // top — the rod the stamp already is. The centre set is the bore's
    // circle less 2, the floor that circle back out to 6.
    let p = program(&format!("tool T1 Endmill(dia=4.0)\n{HOLE}{POCKET}run Clear(tool=T1, profile=Pocket, stepover=2)\n"));
    let lenses = stock::given_air(&p, &p.operations[0]);
    assert_eq!(lenses.len(), 1);
    let l = &lenses[0];
    assert_eq!(l.void, "Hole");
    assert_eq!(l.stretches, vec![(0.0, 8.0, 2.0)]);
    assert!(l.declined.is_none(), "{l:?}");
    assert_eq!(l.centre.len(), 1, "{:?}", l.centre);
    let (x0, y0, x1, y1) = bbox(&l.centre[0]);
    assert!(near(x0, 66.0, 0.02) && near(x1, 74.0, 0.02) && near(y0, 36.0, 0.02) && near(y1, 44.0, 0.02), "{x0} {y0} {x1} {y1}");
    assert_eq!(l.floor.len(), 1);
    let (x0, y0, x1, y1) = bbox(&l.floor[0]);
    assert!(near(x0, 64.0, 0.03) && near(x1, 76.0, 0.03) && near(y0, 34.0, 0.03) && near(y1, 46.0, 0.03), "{x0} {y0} {x1} {y1}");
    // An op that works no floor has no lens to speak of.
    let p = program(&format!("tool T1 Endmill(dia=4.0)\n{HOLE}feature Win ProfilePocket(extent=8\n    10 20\n    50 20\n    50 60\n    10 60\n)\nrun Side(tool=T1, surface=Win)\n"));
    assert!(stock::given_air(&p, &p.operations[0]).is_empty());
    // And a program that gives nothing has none.
    let p = program(&format!("tool T1 Endmill(dia=4.0)\n{POCKET}run Clear(tool=T1, profile=Pocket, stepover=2)\n"));
    assert!(stock::given_air(&p, &p.operations[0]).is_empty());
}

#[test]
fn test_the_body_is_judged_by_its_declared_pieces_and_the_widest_governs_a_column() {
    // Flutes r2 for 5, then a relieved shank r1.5: the stretches are the
    // ladder's rungs clipped to the 8 the body occupies, and the column's
    // lens is eroded by the WIDEST (the flutes) — a narrower shank above
    // frees nothing in a column.
    let p = program(&format!("tool T2 Endmill(dia=4.0, flute_len=5, shank_dia=3)\n{HOLE}{POCKET}run Clear(tool=T2, profile=Pocket, stepover=2)\n"));
    let l = &stock::given_air(&p, &p.operations[0])[0];
    assert_eq!(l.stretches, vec![(0.0, 5.0, 2.0), (5.0, 8.0, 1.5)]);
    let (x0, _, x1, _) = bbox(&l.centre[0]);
    assert!(near(x0, 66.0, 0.02) && near(x1, 74.0, 0.02), "{x0} {x1}");
    // A holder whose nose enters the stock: stickout 5, nose r5 — the
    // centre set shrinks to the bore's circle less 5.
    let p = program(&format!("tool T2 Endmill(dia=4.0, flute_len=5, shank_dia=3, stickout=5, holder_nose_dia=10)\n{HOLE}{POCKET}run Clear(tool=T2, profile=Pocket, stepover=2)\n"));
    let l = &stock::given_air(&p, &p.operations[0])[0];
    assert_eq!(l.stretches, vec![(0.0, 5.0, 2.0), (5.0, 8.0, 5.0)]);
    let (x0, _, x1, _) = bbox(&l.centre[0]);
    assert!(near(x0, 69.0, 0.02) && near(x1, 71.0, 0.02), "{x0} {x1}");
    let (x0, _, x1, _) = bbox(&l.floor[0]);
    assert!(near(x0, 67.0, 0.03) && near(x1, 73.0, 0.03), "{x0} {x1}");
    // A holder above the stock's top is not in the stock and not judged.
    let p = program(&format!("tool T2 Endmill(dia=4.0, flute_len=5, shank_dia=3, stickout=20, holder_nose_dia=30)\n{HOLE}{POCKET}run Clear(tool=T2, profile=Pocket, stepover=2)\n"));
    let l = &stock::given_air(&p, &p.operations[0])[0];
    assert_eq!(l.stretches, vec![(0.0, 5.0, 2.0), (5.0, 8.0, 1.5)]);
    assert!(l.declined.is_none());
}

#[test]
fn test_a_holder_wider_than_the_void_leaves_no_air_and_says_so() {
    let p = program(&format!("tool T2 Endmill(dia=4.0, flute_len=5, shank_dia=3, stickout=5, holder_nose_dia=14)\n{HOLE}{POCKET}run Clear(tool=T2, profile=Pocket, stepover=2)\n"));
    let l = &stock::given_air(&p, &p.operations[0])[0];
    assert!(l.centre.is_empty() && l.floor.is_empty());
    let why = l.declined.as_deref().unwrap();
    assert!(why.contains("no plan point of 'Hole' holds the body") && why.contains("r 7.000"), "{why}");
    assert!(l.line().starts_with("given air 'Hole': none — "), "{}", l.line());
}

#[test]
fn test_the_tilted_bores_lens_is_the_sections_overlap_eroded_and_the_certificate_agrees() {
    // A 4mm rod, floor at −8: the sections at depths 0 and 8 overlap in x
    // from the mouth's near end (70 − 6.928) to the lower section's far end
    // (70 − 4.619 + 6.928); eroded by 2 — plus the slab stack's tenth of a
    // millimetre, so the lens reads the cylinder the certificate reads —
    // the centre set runs 65.17..70.21, and the floor puts the 2 back.
    let p = program(&format!("tool T1 Endmill(dia=4.0)\n{DRAIN}{POCKET}run Clear(tool=T1, profile=Pocket, stepover=2)\n"));
    let l = &stock::given_air(&p, &p.operations[0])[0];
    assert_eq!(l.void, "Drain");
    assert!(l.declined.is_none(), "{l:?}");
    assert_eq!(l.centre.len(), 1, "{:?}", l.centre);
    let (x0, y0, x1, y1) = bbox(&l.centre[0]);
    let (lo, hi) = (70.0 - semi_a(), 70.0 - 8.0 * tan30() + semi_a());
    const M: f64 = 0.1;
    assert!(near(x0, lo + 2.0 + M, 0.03) && near(x1, hi - 2.0 - M, 0.03), "{x0} {x1} vs {} {}", lo + 2.0 + M, hi - 2.0 - M);
    assert!(y0 > 36.0 && y1 < 44.0 && near(y0 + y1, 80.0, 0.03), "{y0} {y1}");
    let (fx0, _, fx1, _) = bbox(&l.floor[0]);
    assert!(near(fx0, lo + M, 0.05) && near(fx1, hi - M, 0.05), "{fx0} {fx1}");
    // The certificate agrees: a plunge with the centre anywhere in the lens
    // is blessed by `check_moves`, and one a hair outside its x extremes is
    // not — the lens and the slab stack are the same cylinder read twice.
    let plunge = |x: f64| vec![format!("G01 X{x:.3} Y40 Z-8 F300")];
    assert!(stock::check_moves(&p, 0, &plunge((x0 + x1) / 2.0)).is_ok());
    assert!(stock::check_moves(&p, 0, &plunge(x0 + 0.3)).is_ok());
    assert!(stock::check_moves(&p, 0, &plunge(x1 - 0.3)).is_ok());
    assert!(stock::check_moves(&p, 0, &plunge(x0 - 0.5)).is_err());
    assert!(stock::check_moves(&p, 0, &plunge(x1 + 0.5)).is_err());
    // A long-reach tool with a relieved shank: flutes r2 to 3, shank r1 to
    // 8. The stretches' own lenses — depths 5..8 eroded by 2, 0..5 eroded by
    // 1 — intersect to MORE than the rod's lens, because the body is thinner
    // where the sections have drifted apart.
    let p = program(&format!("tool T2 Endmill(dia=4.0, flute_len=3, shank_dia=2)\n{DRAIN}{POCKET}run Clear(tool=T2, profile=Pocket, stepover=2)\n"));
    let l2 = &stock::given_air(&p, &p.operations[0])[0];
    assert_eq!(l2.stretches, vec![(0.0, 3.0, 2.0), (3.0, 8.0, 1.0)]);
    let area = |lp: &[Pt]| parse::signed_area(lp).abs() / 2.0;
    assert!(area(&l2.centre[0]) > area(&l.centre[0]) * 1.1, "{} vs {}", area(&l2.centre[0]), area(&l.centre[0]));
    let (x0, _, x1, _) = bbox(&l2.centre[0]);
    // The mouth's near end at depth 0 is eroded by the shank's 1, the lower
    // face at depth 8 by the flutes' 2.
    assert!(near(x0, lo + 1.0 + M, 0.03) && near(x1, hi - 2.0 - M, 0.03), "{x0} {x1}");
}

#[test]
fn test_a_floor_below_the_bore_has_material_under_the_tip_and_no_lens() {
    let shallow = "feature Short TiltedBore(dia=12, extent=5, x=70, y=40, tilt=30, heading=180)\ngiven Short\n";
    let p = program(&format!("tool T1 Endmill(dia=4.0)\n{shallow}{POCKET}run Clear(tool=T1, profile=Pocket, stepover=2)\n"));
    let l = &stock::given_air(&p, &p.operations[0])[0];
    assert!(l.centre.is_empty());
    assert!(l.declined.as_deref().unwrap().contains("below the bore's bottom"), "{l:?}");
    // The same for a column: a given pocket shallower than this floor.
    let p = program(&format!(
        "tool T1 Endmill(dia=4.0)\nfeature Dish ProfilePocket(extent=5\n    60 30\n    90 30\n    90 50\n    60 50\n)\ngiven Dish\n{POCKET}run Clear(tool=T1, profile=Pocket, stepover=2)\n"
    ));
    let l = &stock::given_air(&p, &p.operations[0])[0];
    assert!(l.declined.as_deref().unwrap().contains("below 'Dish''s bottom"), "{l:?}");
}

#[test]
fn test_the_outline_voices_the_lens_under_the_op_and_the_window_reads_the_same_lines() {
    let p = program(&format!("tool T1 Endmill(dia=4.0)\n{DRAIN}{HOLE}{POCKET}run Clear(tool=T1, profile=Pocket, stepover=2)\n"));
    let o = outline::outline(&p);
    assert_eq!(o.given_air.len(), 1);
    assert_eq!(o.given_air[0].len(), 2, "{:?}", o.given_air[0]);
    assert!(o.given_air[0][0].starts_with("given air 'Drain': the body (r 2.000 from 0.000 to 8.000) stands in 1 piece — centre "), "{}", o.given_air[0][0]);
    assert!(o.given_air[0][1].starts_with("given air 'Hole': the body (r 2.000 from 0.000 to 8.000) stands in 1 piece — centre "), "{}", o.given_air[0][1]);
    let text = outline::render(&p, &o);
    assert!(text.contains("      given air 'Drain': the body"), "{text}");
    assert!(text.contains("      given air 'Hole': the body"), "{text}");
    // Nothing given, nothing said — every program before this phase.
    let p = program(&format!("tool T1 Endmill(dia=4.0)\n{POCKET}run Clear(tool=T1, profile=Pocket, stepover=2)\n"));
    assert!(!outline::report(&p).contains("given air"));
}

#[test]
fn test_the_lens_edge_is_a_wall_stroke_at_the_floor() {
    let p = program(&format!("tool T1 Endmill(dia=4.0)\n{DRAIN}{POCKET}run Clear(tool=T1, profile=Pocket, stepover=2)\n"));
    let edges = view::feature_edges(&p);
    let strokes = view::region_strokes(&p, 0, &edges);
    let l = &stock::given_air(&p, &p.operations[0])[0];
    let lens_stroke = strokes
        .iter()
        .find(|s| s.pts.len() == l.floor[0].len() + 1 && s.pts.iter().zip(&l.floor[0]).all(|(q, f)| (q[0] - f.x).abs() < 1e-12 && (q[1] - f.y).abs() < 1e-12))
        .unwrap_or_else(|| panic!("the lens's floor as one closed stroke: {strokes:?}"));
    assert_eq!(lens_stroke.kind, view::EdgeStroke::Wall);
    assert!(lens_stroke.pts.iter().all(|q| (q[2] + 8.0).abs() < 1e-12), "at the floor");
    assert_eq!(lens_stroke.pts.first(), lens_stroke.pts.last(), "closed on its start");
}
