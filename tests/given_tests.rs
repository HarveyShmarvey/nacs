// ---- given: a void that is there before the program starts (Phase 42 slice 1)
//
// `DECISIONS.md` → "Phase 42 slice 1…", fork 1A of the Phase 42 DRAFT: a
// bore cut on another setup is AIR to this one. `given <feature>` says so —
// a statement beside `fixture`, a program fact and not a key on the noun —
// and the stock model subtracts the void at prefix 0, so the plunge
// taxonomy, `check_moves`, the rest report and `--outline` all read it from
// the one home. The tilted bore, the one void that is not a column, is a
// stack of slabs each the LENS of its two end sections: air at both, so air
// between, and a crescent a tenth of a millimetre wide left unclaimed in
// the direction that can only under-claim. These pins hold the statement's
// refusals, the subtraction, the lens by hand, and the certificate's two
// answers — a plunge into the given air blessed, one into the lane beside
// it (inside the envelope, outside the section at depth) rejected.

use nacs::parse::{self, derive_tilted_bore_lens, derive_tilted_bore_section, Pt};
use nacs::{outline, stock, vox};

const HEAD: &str = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\ntool T5 Endmill(dia=10.0)\ntool T1 Endmill(dia=4.0)\n";

/// The fixture's bore, through the whole slab: 12mm, entering at (70, 40),
/// leaning toward −X at 30°. The section at depth `d` is centred at
/// 70 − d·tan 30° with semi-axes 6/cos 30° = 6.928 along X and 6 across.
const DRAIN: &str = "feature Drain TiltedBore(dia=12, extent=20, x=70, y=40, tilt=30, heading=180)\n";
const HOLE: &str = "feature Hole CylinderBore(dia=12, extent=20, x=70, y=40)\n";
const POCKET: &str = "feature Pocket ProfilePocket(extent=8\n    10 20\n    50 20\n    50 60\n    10 60\n)\n";

fn program(lines: &str) -> parse::Part {
    parse::parse(&format!("{HEAD}{lines}")).unwrap_or_else(|e| panic!("must parse: {e}"))
}

fn refusal(lines: &str) -> String {
    parse::parse(&format!("{HEAD}{lines}")).err().unwrap_or_else(|| panic!("must refuse:\n{lines}"))
}

fn tan30() -> f64 {
    30f64.to_radians().tan()
}

fn semi_a() -> f64 {
    6.0 / 30f64.to_radians().cos()
}

fn bbox(lp: &[Pt]) -> (f64, f64, f64, f64) {
    lp.iter().fold((f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY), |b, q| {
        (b.0.min(q.x), b.1.min(q.y), b.2.max(q.x), b.3.max(q.y))
    })
}

#[test]
fn test_the_statement_records_the_name_and_refuses_what_it_cannot_mean() {
    let p = program(&format!("{DRAIN}given Drain\n"));
    assert_eq!(p.given, ["Drain"]);
    // In declaration order, however many.
    let p = program(&format!("{DRAIN}{HOLE}given Hole\ngiven Drain\n"));
    assert_eq!(p.given, ["Hole", "Drain"]);
    let e = refusal(&format!("{DRAIN}given\n"));
    assert!(e.contains("names exactly one feature"), "{e}");
    let e = refusal(&format!("{DRAIN}{HOLE}given Drain Hole\n"));
    assert!(e.contains("names exactly one feature"), "{e}");
    let e = refusal(&format!("{DRAIN}given Drain\ngiven Drain\n"));
    assert!(e.contains("'Drain' is given twice"), "{e}");
    let e = refusal("given Nope\n");
    assert!(e.contains("given 'Nope': no feature named 'Nope'"), "{e}");
    // Only a void: what stands is material, and material is never given.
    let e = refusal("feature Post CylinderBoss(dia=10, extent=5, x=50, y=40, z=-5)\ngiven Post\n");
    assert!(e.contains("a CylinderBoss cannot be given") && e.contains("material is never given"), "{e}");
    // A kind the model has no closed form for is a guess, refused by name.
    let e = refusal("feature Top Plane(z=0)\ngiven Top\n");
    assert!(e.contains("a Plane cannot be given"), "{e}");
}

#[test]
fn test_a_verb_that_would_remove_a_given_void_refuses_and_a_wall_dress_does_not() {
    for run in ["run Clear(tool=T5, profile=Pocket, stepover=4)\n", "run End(tool=T5, surface=Pocket, stepover=4)\n"] {
        let e = refusal(&format!("{POCKET}given Pocket\n{run}"));
        assert!(e.contains("names 'Pocket', which is given") && e.contains("cannot be cleared, floored or drilled again"), "{run}: {e}");
    }
    let e = refusal(&format!("{HOLE}given Hole\nrun Form(points=[Hole], tool=T5, cycle=Simple)\n"));
    assert!(e.contains("names 'Hole', which is given"), "{e}");
    // Dressing the wall of a given bore is a pass on air's rim: legal.
    let p = program(&format!("{HOLE}given Hole\nrun Side(tool=T5, surface=Hole, stepover=5.0)\n"));
    assert_eq!(p.given, ["Hole"]);
    assert!(nacs::gcode::program(&p).is_ok());
}

#[test]
fn test_the_stock_model_reads_a_given_void_as_air_before_the_first_op() {
    // A given bore: void down its column before anything runs; beside it,
    // material. The same bore not given is material everywhere — no op of
    // this program cuts it, so the model stands on it (the premise the
    // phase corrects).
    let p = program(&format!("{HOLE}given Hole\n{POCKET}run Clear(tool=T5, profile=Pocket, stepover=4)\n"));
    let before = stock::remaining_before(&p, 0);
    assert!(before.complete);
    assert!(!before.material_at(70.0, 40.0, -5.0));
    assert!(!before.material_at(70.0, 40.0, -19.9));
    assert!(before.material_at(80.0, 40.0, -5.0));
    assert!(before.material_at(30.0, 40.0, -5.0), "the pocket is not cut yet");
    assert!(!stock::remaining_after(&p, 0).material_at(30.0, 40.0, -5.0));
    let p = program(&format!("{HOLE}{POCKET}run Clear(tool=T5, profile=Pocket, stepover=4)\n"));
    assert!(stock::remaining_before(&p, 0).material_at(70.0, 40.0, -5.0));
    // A given pocket is its own column.
    let p = program(&format!("{POCKET}given Pocket\n"));
    let before = stock::remaining_before(&p, 0);
    assert!(!before.material_at(30.0, 40.0, -7.9) && before.material_at(30.0, 40.0, -8.1));
}

#[test]
fn test_a_given_tilted_bore_is_air_along_its_axis_and_material_in_the_lane() {
    let p = program(&format!("{DRAIN}given Drain\n"));
    let before = stock::remaining_before(&p, 0);
    // One slab per tenth of a millimetre of drift: 20 · tan 30° / 0.1.
    assert_eq!(stock::given_removals(&p).len(), (20.0 * tan30() / 0.1).ceil() as usize);
    // Down the axis, air; at the mouth and the bottom too.
    for depth in [0.05, 5.0, 10.0, 15.0, 19.95] {
        let cx = 70.0 - depth * tan30();
        assert!(!before.material_at(cx, 40.0, -depth), "the axis at depth {depth}");
        assert!(!before.material_at(cx + 0.8 * semi_a(), 40.0, -depth), "inside the section at depth {depth}");
        assert!(!before.material_at(cx, 40.0 + 5.0, -depth), "inside the section across the lean at depth {depth}");
        // The lane: inside the plan envelope, outside the section at this
        // height — the tube wall over (or under) the bore. The envelope
        // spans x 51.5..76.9; at depth 10 the section reaches only 71.2.
        assert!(before.material_at(cx + 1.1 * semi_a(), 40.0, -depth), "the lane at depth {depth}");
        assert!(before.material_at(cx, 40.0 + 6.3, -depth), "beside the section at depth {depth}");
    }
    assert!(before.material_at(76.0, 40.0, -10.0), "inside the envelope, over the bore: material");
    assert!(before.material_at(53.0, 40.0, -10.0), "inside the envelope, under the bore: material");
}

#[test]
fn test_the_lens_is_the_section_at_equal_depths_and_the_two_arcs_between() {
    let (a, b) = (semi_a(), 6.0);
    let on = |q: &Pt, depth: f64| {
        let (u, v) = (q.x - (70.0 - depth * tan30()), q.y - 40.0);
        ((u / a).powi(2) + (v / b).powi(2) - 1.0).abs() < 1e-9
    };
    // At one depth the lens is the section: same extremes, every point on it.
    let section = derive_tilted_bore_section(12.0, 70.0, 40.0, 30.0, 180.0, 5.0);
    let lens = derive_tilted_bore_lens(12.0, 70.0, 40.0, 30.0, 180.0, 5.0, 5.0);
    assert!(section.len() > 8 && lens.len() > 8);
    assert!(section.iter().all(|q| on(q, 5.0)) && lens.iter().all(|q| on(q, 5.0)));
    let (sx0, sy0, sx1, sy1) = bbox(&section);
    let (lx0, ly0, lx1, ly1) = bbox(&lens);
    assert!((sx0 - lx0).abs() < 1e-9 && (sx1 - lx1).abs() < 1e-9 && (sy0 - ly0).abs() < 1e-9 && (sy1 - ly1).abs() < 1e-9);
    assert!((sx0 - (70.0 - 5.0 * tan30() - a)).abs() < 1e-9 && (sy1 - 46.0).abs() < 1e-9);
    assert!(parse::signed_area(&lens) > 0.0, "CCW");
    // Between the mouth and the bottom: x from the bottom section's near end
    // to the mouth's far end — the two sections' overlap — every point on one
    // ellipse or the other, the width across the lean at the middle what
    // both ellipses give there.
    let lens = derive_tilted_bore_lens(12.0, 70.0, 40.0, 30.0, 180.0, 0.0, 20.0);
    let (x0, y0, x1, y1) = bbox(&lens);
    assert!((x0 - (70.0 - a)).abs() < 1e-9, "{x0}");
    assert!((x1 - (70.0 - 20.0 * tan30() + a)).abs() < 1e-9, "{x1}");
    assert!(lens.iter().all(|q| on(q, 0.0) || on(q, 20.0)), "{lens:?}");
    let mid = (x0 + x1) / 2.0;
    let half = b * (1.0 - ((mid - 70.0) / a).powi(2)).sqrt();
    assert!((y1 - (40.0 + half)).abs() < 0.02 && (y0 - (40.0 - half)).abs() < 0.02, "{y0} {y1} vs ±{half}");
    assert!(parse::signed_area(&lens) > 0.0, "CCW");
    // Two semi-axes of shift or more and the sections clear each other.
    assert!(derive_tilted_bore_lens(12.0, 70.0, 40.0, 60.0, 180.0, 0.0, 20.0).is_empty());
}

#[test]
fn test_check_moves_blesses_a_plunge_into_given_air_and_rejects_one_into_the_lane() {
    // A 4mm tool entering the bore 8 deep at (67.7, 40): the lens of the
    // sections over 0..8 spans x 63.07..72.31 there, 5.66 either side in y —
    // the whole body stands in the given air.
    let into_air = vec!["G01 X67.7 Y40 Z-8 F300".to_string()];
    let into_lane = vec!["G01 X75 Y40 Z-8 F300".to_string()];
    let p = program(&format!("{DRAIN}given Drain\n{POCKET}run Clear(tool=T1, profile=Pocket, stepover=2)\n"));
    assert!(stock::check_moves(&p, 0, &into_air).is_ok());
    let e = stock::check_moves(&p, 0, &into_lane).unwrap_err();
    assert!(e.contains("drives the tool into material that outlives this operation"), "{e}");
    // Not given, the same entry is the rejected move it was before the phase.
    let p = program(&format!("{DRAIN}{POCKET}run Clear(tool=T1, profile=Pocket, stepover=2)\n"));
    let e = stock::check_moves(&p, 0, &into_air).unwrap_err();
    assert!(e.contains("drives the tool into material that outlives this operation"), "{e}");
}

#[test]
fn test_the_outline_lists_a_given_void_under_its_own_heading() {
    let p = program(&format!("{DRAIN}{HOLE}given Hole\ngiven Drain\n{POCKET}run Clear(tool=T5, profile=Pocket, stepover=4)\n"));
    let o = outline::outline(&p);
    let given: Vec<&str> = o.given.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(given, ["Hole", "Drain"], "declaration order");
    assert!(o.unused.is_empty(), "a given void is read, so it is never unused: {:?}", o.unused.iter().map(|n| &n.name).collect::<Vec<_>>());
    let text = outline::render(&p, &o);
    assert!(text.contains("\ngiven (air before this program starts — cut on another setup):\n  Hole  CylinderBore\n  Drain  TiltedBore\n"), "{text}");
    assert!(!text.contains("unused ("), "{text}");
    // Not given, the same bore is unused as before.
    let p = program(&format!("{HOLE}{POCKET}run Clear(tool=T5, profile=Pocket, stepover=4)\n"));
    let text = outline::report(&p);
    assert!(text.contains("unused (") && !text.contains("given ("), "{text}");
}

#[test]
fn test_the_rest_report_does_not_call_given_air_forgotten() {
    // The field never swept the given bore — it was never this program's
    // to sweep — and the declared model subtracts it at prefix 0. That
    // divergence is the other setup's work, not forgotten material.
    let p = program(&format!("{HOLE}given Hole\n{POCKET}run Clear(tool=T5, profile=Pocket, stepover=4)\n"));
    let report = vox::rest_report(&p, Some(100.0 / 64.0)).unwrap();
    assert!(report.contains("forgotten (declared removed, never swept): none"), "{report}");
}
