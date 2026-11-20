// ---- the intruder with an axis (Phase 41 slice 5) ---------------------------
//
// `DECISIONS.md` → "Phase 41 slice 5…", ruled A on the DRAFT: a bore whose
// axis is not +Z gets a noun, `TiltedBore`, and everything derived from it —
// its plan ENVELOPE over the slab it spans, its reading as a void in a
// floor, its footprint for the arc construction, its wireframe — falls out
// of the one declaration through homes that already exist. No op cuts it:
// every verb that names it refuses by name. These pins hold the geometry
// (the envelope's extremes from the declared angles, by hand), the refusals,
// and each consumer reading the same loop.

use nacs::construct;
use nacs::parse::{self, derive_tilted_axis, derive_tilted_bore_envelope, Feature, Pt};
use nacs::{region, stock, view};

const HEAD: &str = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\ntool T5 Endmill(dia=10.0)\n";

fn program(lines: &str) -> parse::Part {
    parse::parse(&format!("{HEAD}{lines}")).unwrap_or_else(|e| panic!("must parse: {e}"))
}

fn refusal(lines: &str) -> String {
    parse::parse(&format!("{HEAD}{lines}")).err().unwrap_or_else(|| panic!("must refuse:\n{lines}"))
}

/// The fixture's bore: 12mm, 14 deep, entering at (70, 40), leaning toward
/// −X at 30°. By hand: the lower section's centre is extent·tan(30°) =
/// 8.083 along −X; each section is an ellipse with semi-axis 6/cos(30°) =
/// 6.928 along the lean and 6 across it.
const DRAIN: &str = "feature Drain TiltedBore(dia=12, extent=14, x=70, y=40, tilt=30, heading=180)\n";

fn bbox(loop_pts: &[Pt]) -> (f64, f64, f64, f64) {
    loop_pts.iter().fold((f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY), |b, q| {
        (b.0.min(q.x), b.1.min(q.y), b.2.max(q.x), b.3.max(q.y))
    })
}

#[test]
fn test_the_axis_direction_is_the_two_angles_going_down() {
    let d = derive_tilted_axis(30.0, 180.0);
    assert!((d[0] + 0.5).abs() < 1e-12 && d[1].abs() < 1e-12 && (d[2] + 0.75f64.sqrt()).abs() < 1e-12, "{d:?}");
    let d = derive_tilted_axis(45.0, 90.0);
    assert!(d[0].abs() < 1e-12 && (d[1] - 0.5f64.sqrt()).abs() < 1e-12 && (d[2] + 0.5f64.sqrt()).abs() < 1e-12, "{d:?}");
}

#[test]
fn test_the_envelope_is_two_lines_and_two_half_ellipses_at_the_hand_computed_extremes() {
    let env = derive_tilted_bore_envelope(12.0, 14.0, 70.0, 40.0, 30.0, 180.0);
    let shift = 14.0 * 30f64.to_radians().tan();
    let a = 6.0 / 30f64.to_radians().cos();
    let (x0, y0, x1, y1) = bbox(&env);
    // The far end of the lower section, the far end of the upper, a radius
    // either side of the axis's plan line.
    assert!((x0 - (70.0 - shift - a)).abs() < 1e-9, "{x0}");
    assert!((x1 - (70.0 + a)).abs() < 1e-9, "{x1}");
    assert!((y0 - 34.0).abs() < 1e-9 && (y1 - 46.0).abs() < 1e-9, "{y0} {y1}");
    // CCW, as every loop a region reads.
    assert!(parse::signed_area(&env) > 0.0);
    // Every point lies on one of the two ellipses: the squared parametric
    // distance from the nearer centre, in the ellipse's own frame, is 1.
    for q in &env {
        let on = |cx: f64| {
            let (u, v) = (q.x - cx, q.y - 40.0);
            ((u / a).powi(2) + (v / 6.0).powi(2) - 1.0).abs() < 1e-9
        };
        assert!(on(70.0) || on(70.0 - shift), "{q:?} is on neither section");
    }
    // The join between the halves is the straight run along the lean: two
    // consecutive points a radius off the axis line on each side, with the
    // lower section's point further along −X.
    let top_side: Vec<&Pt> = env.iter().filter(|q| (q.y - 46.0).abs() < 1e-9).collect();
    let bottom_side: Vec<&Pt> = env.iter().filter(|q| (q.y - 34.0).abs() < 1e-9).collect();
    assert_eq!(top_side.len(), 2, "{top_side:?}");
    assert_eq!(bottom_side.len(), 2, "{bottom_side:?}");
    assert!((top_side[0].x - top_side[1].x).abs() - shift < 1e-9);
}

#[test]
fn test_the_declaration_is_judged_alone_and_the_degenerate_tilt_points_at_the_vertical_noun() {
    let e = refusal("feature D TiltedBore(dia=12, extent=14, x=70, y=40, tilt=0, heading=0)\n");
    assert!(e.contains("IS a vertical bore") && e.contains("CylinderBore"), "{e}");
    let e = refusal("feature D TiltedBore(dia=12, extent=14, x=70, y=40, tilt=90, heading=0)\n");
    assert!(e.contains("strictly inside (0, 90)"), "{e}");
    let e = refusal("feature D TiltedBore(dia=12, extent=14, x=70, y=40, tilt=-5, heading=0)\n");
    assert!(e.contains("strictly inside (0, 90)"), "{e}");
    let e = refusal("feature D TiltedBore(dia=0, extent=14, x=70, y=40, tilt=30, heading=0)\n");
    assert!(e.contains("dia must be positive"), "{e}");
    let e = refusal("feature D TiltedBore(dia=12, extent=14, x=70, y=40, tilt=30)\n");
    assert!(e.contains("heading"), "{e}");
    let e = refusal("feature D TiltedBore(dia=12, extent=14, x=70, y=40, tilt=30, heading=0, z=-1)\n");
    assert!(e.contains("no field named 'z'"), "{e}");
    // A heading of any size is a direction; it is not canonicalised.
    program("feature D TiltedBore(dia=12, extent=14, x=70, y=40, tilt=30, heading=405)\n");
}

#[test]
fn test_every_verb_that_names_a_tilted_bore_refuses_by_name() {
    for run in [
        "run Side(tool=T5, surface=Drain, stepover=2)\n",
        "run Clear(tool=T5, profile=Drain, stepover=4)\n",
        "run End(tool=T5, surface=Drain, stepover=4)\n",
        "run Copy(tool=T5, surface=Drain, stepover=1)\n",
        "run Form(points=[Drain], tool=T5, cycle=Simple)\n",
        "run End(points=[Drain], tool=T5, cycle=Simple)\n",
    ] {
        let e = refusal(&format!("{DRAIN}{run}"));
        assert!(e.contains("'Drain', a TiltedBore") && e.contains("never tilts the tool"), "{run}: {e}");
    }
    // A lint-only read: the fixture's program, which never names it, is green.
    let p = program(&format!("{DRAIN}feature P ProfilePocket(extent=8\n    10 20\n    50 20\n    50 60\n    10 60\n)\nrun Clear(tool=T5, profile=P, stepover=4.0)\n"));
    assert!(matches!(p.features["Drain"], Feature::TiltedBore { tilt, heading, .. } if tilt == 30.0 && heading == 180.0));
    assert!(nacs::gcode::program(&p).is_ok());
}

#[test]
fn test_the_construction_reads_the_envelope_pushed_out_and_the_arc_kisses_the_near_cap() {
    let p = program(DRAIN);
    let refs = construct::references_of(&p, "Drain", 3.0).unwrap();
    assert!(refs.iter().all(|r| matches!(r, construct::Reference::Segment(..))), "chorded caps: every reference a segment");
    // Pushed out 3mm, the near cap's extreme is the envelope's −X extreme
    // less 3: 70 − 8.083 − 6.928 − 3 = 51.989 (the chording lands within
    // the tolerance of it).
    let x_min = refs.iter().map(|r| match r { construct::Reference::Segment(a, b) => a.x.min(b.x), _ => f64::INFINITY }).fold(f64::INFINITY, f64::min);
    assert!((x_min - (70.0 - 14.0 * 30f64.to_radians().tan() - 6.0 / 30f64.to_radians().cos() - 3.0)).abs() < 0.02, "{x_min}");
    let arcs = construct::arcs_through(Pt { x: 58.0, y: 20.0 }, Pt { x: 58.0, y: 60.0 }, &refs).unwrap();
    assert_eq!(arcs.len(), 2, "{arcs:?}");
    // The one that bulges into the pocket touches the near cap on the axis's
    // plan line, and its centre lies on that line too.
    let near = arcs.iter().find(|a| a.tx < 58.0).expect("an arc kissing the near cap");
    assert!((near.ty - 40.0).abs() < 0.05 && (near.cy - 40.0).abs() < 1e-6, "{near:?}");
    assert!((near.tx - x_min).abs() < 0.05, "{near:?}");
    // The terminal's voice names both and prints the pick as loop lines
    // that begin and end on the two points.
    let text = construct::report(&p, Pt { x: 58.0, y: 20.0 }, Pt { x: 58.0, y: 60.0 }, "Drain", 3.0, 2).unwrap();
    assert!(text.contains("# 2 arcs") && text.contains("tangent to 'Drain' pushed out 3.000mm"), "{text}");
    let pts: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
    assert_eq!(pts.first().map(|s| s.trim()), Some("58.000 20.000"));
    assert_eq!(pts.last().map(|s| s.trim()), Some("58.000 60.000"));
}

#[test]
fn test_a_tilted_bore_wholly_in_a_floor_is_bridged_and_beside_it_is_nothing() {
    // The pocket holds the whole envelope (x 55..77, y 34..46) and is
    // shallower than the bore: the floor runs over its mouth.
    let p = program(&format!(
        "{DRAIN}feature P ProfilePocket(extent=8\n    40 20\n    90 20\n    90 60\n    40 60\n)\nrun Clear(tool=T5, profile=P, stepover=4.0)\n"
    ));
    let r = region::of(&p, &p.operations[0]).unwrap();
    assert_eq!(r.bridged(), ["Drain"]);
    let g = nacs::gcode::program(&p).unwrap();
    assert!(g.contains("(bridged: Drain)\n"), "{}", &g[..400]);
    // The envelope is what is judged, not the mouth: a pocket that holds the
    // mouth (x 63..77) but not the lower section's far end (x to 55) is not
    // over the whole intruder, so the void is not wholly in its floor.
    let p = program(&format!(
        "{DRAIN}feature P ProfilePocket(extent=8\n    60 20\n    90 20\n    90 60\n    60 60\n)\nrun Clear(tool=T5, profile=P, stepover=4.0)\n"
    ));
    let r = region::of(&p, &p.operations[0]).unwrap();
    assert!(r.bridged().is_empty(), "{:?}", r.bridged());
    // Beside it, apart: the fixture's own case. The pocket's edges stay walls
    // (the web between them is the host's material) and nothing is bridged.
    let p = program(&format!(
        "{DRAIN}feature P ProfilePocket(extent=8\n    10 20\n    50 20\n    50 60\n    10 60\n)\nrun Clear(tool=T5, profile=P, stepover=4.0)\n"
    ));
    let r = region::of(&p, &p.operations[0]).unwrap();
    assert!(r.bridged().is_empty());
    assert_eq!(r.open_edges(), 0);
}

#[test]
fn test_the_stock_model_reads_the_envelope_as_the_voids_footprint() {
    // A tilted bore is a void: a boss standing across its envelope straddles
    // it exactly as one across a vertical bore would, and the sentence names
    // both — the geometry tells the stock model nothing is prismatic here.
    let e = refusal(&format!(
        "{DRAIN}feature Post CylinderBoss(dia=10, extent=14, x=56, y=40, z=-14)\nrun Clear(tool=T5, profile=Post, stepover=4, stepdown=5)\n"
    ));
    assert!(e.contains("Drain") && e.contains("Post"), "{e}");
}

#[test]
fn test_a_bore_leaning_out_of_the_stock_is_uncontained_by_its_lower_section() {
    // Entering at x = 8 and leaning toward −X, the lower section's far end
    // is at 8 − 8.083 − 6.928 < 0: past the −x face. Read from the
    // declaration alone — nothing runs.
    let p = program("feature D TiltedBore(dia=12, extent=14, x=8, y=40, tilt=30, heading=180)\n");
    let found = stock::containment_findings(&p);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].message.contains("'D' (TiltedBore)") && found[0].message.contains("past the stock's -x face"), "{}", found[0].message);
    // The same bore entering further in is contained.
    let p = program("feature D TiltedBore(dia=12, extent=14, x=30, y=40, tilt=30, heading=180)\n");
    assert!(stock::containment_findings(&p).is_empty());
}

#[test]
fn test_the_wireframe_is_the_two_sections_and_four_generators() {
    let p = program(DRAIN);
    let edges = view::feature_edges(&p);
    let w = edges.iter().find(|w| w.name == "Drain").unwrap();
    assert_eq!(w.polylines.len(), 6, "two rings and four ties");
    let shift = 14.0 * 30f64.to_radians().tan();
    let a = 6.0 / 30f64.to_radians().cos();
    // The upper ring is at the stock top about (70, 40); the lower at −14
    // about (70 − shift, 40); both are the declared section.
    for (ring, z, cx) in [(&w.polylines[0], 0.0, 70.0), (&w.polylines[1], -14.0, 70.0 - shift)] {
        assert!(ring.len() > 8);
        for q in ring {
            assert!((q[2] - z).abs() < 1e-9, "{q:?}");
            let (u, v) = (q[0] - cx, q[1] - 40.0);
            assert!(((u / a).powi(2) + (v / 6.0).powi(2) - 1.0).abs() < 1e-9, "{q:?} off the section at z {z}");
        }
    }
    // Each tie runs down the declared axis direction, from the upper section
    // to the lower one.
    let d = derive_tilted_axis(30.0, 180.0);
    for tie in &w.polylines[2..] {
        assert_eq!(tie.len(), 2);
        let (top, bot) = (tie[0], tie[1]);
        // The generator's LENGTH is the axial distance, extent / cos(tilt)
        // — `extent` is vertical occupancy, not a length along the axis.
        let k = (bot[2] - top[2]) / d[2];
        assert!((k - 14.0 / 30f64.to_radians().cos()).abs() < 1e-9, "{k}");
        assert!((bot[0] - (top[0] + d[0] * k)).abs() < 1e-9 && (bot[1] - (top[1] + d[1] * k)).abs() < 1e-9, "{tie:?}");
    }
}

#[test]
fn test_inch_inputs_scale_the_lengths_and_never_the_angles() {
    let src = "units inch\nfeed 30\nplunge 10\nrapid_z 0.2\nclearance_z 1\nspindle 3000\n\
stock Box(w=4, l=3, h=1, x=0, y=0, z=0)\ntool T5 Endmill(dia=0.5)\n\
feature D TiltedBore(dia=0.5, extent=0.5, x=2, y=1.5, tilt=30, heading=180)\n";
    let p = parse::parse(src).unwrap();
    let Feature::TiltedBore { dia, extent, x, y, tilt, heading } = &p.features["D"] else { panic!() };
    assert!((dia - 12.7).abs() < 1e-9 && (extent - 12.7).abs() < 1e-9 && (x - 50.8).abs() < 1e-9 && (y - 38.1).abs() < 1e-9);
    assert_eq!((*tilt, *heading), (30.0, 180.0));
}

#[test]
fn test_the_outline_lists_it_as_unused_and_the_template_says_why() {
    let p = program(DRAIN);
    let o = nacs::outline::outline(&p);
    let text = nacs::outline::render(&p, &o);
    assert!(text.contains("unused") && text.contains("Drain  TiltedBore"), "{text}");
    assert!(nacs::template::op_lines(&p, "Drain").unwrap().is_empty(), "no op cuts it, so no skeleton");
    let said = nacs::template::report(&p, "Drain").unwrap();
    assert!(said.starts_with('#') && said.contains("no op of this program cuts a tilted bore"), "{said}");
}

/// The vertex arm (Phase 42 slice 1; the intake's fifth premise correction):
/// at `offset=0` both tangencies on the fixture's points fall on the
/// envelope's extreme VERTICES — the lower section's far end at
/// (70 − 8.083 − 6.928, 40) and the mouth's far end at (76.928, 40) — and
/// the segment arm, which keeps a touch only within a chord, answered "no
/// circle". The circle through the two points and a vertex is the answer:
/// through (58, 20), (58, 60) and (54.989, 40) the centre is on y = 40 at
/// x = 54.989 + r with r² = 400 + (3.011 − r)², r = 67.93.
#[test]
fn test_a_tangency_at_a_chorded_loops_vertex_is_found_at_zero_offset() {
    let p = program(DRAIN);
    let refs = construct::references_of(&p, "Drain", 0.0).unwrap();
    let (a, b) = (Pt { x: 58.0, y: 20.0 }, Pt { x: 58.0, y: 60.0 });
    let arcs = construct::arcs_through(a, b, &refs).unwrap();
    assert_eq!(arcs.len(), 2, "{arcs:?}");
    let near_x = 70.0 - 14.0 * 30f64.to_radians().tan() - 6.0 / 30f64.to_radians().cos();
    let near = arcs.iter().find(|c| c.tx < 58.0).expect("the arc kissing the near cap");
    assert!((near.tx - near_x).abs() < 1e-9 && (near.ty - 40.0).abs() < 1e-9, "{near:?}");
    let r = (400.0 + (58.0 - near_x).powi(2)) / (2.0 * (58.0 - near_x));
    assert!((near.r - r).abs() < 1e-9 && (near.cx - (near_x + r)).abs() < 1e-9 && (near.cy - 40.0).abs() < 1e-9, "{near:?}");
    let far = arcs.iter().find(|c| c.tx > 58.0).expect("the arc wrapping the hole");
    let far_x = 70.0 + 6.0 / 30f64.to_radians().cos();
    assert!((far.tx - far_x).abs() < 1e-9 && (far.ty - 40.0).abs() < 1e-9, "{far:?}");
    // Negative offsets (the reference pulled in) reach the same vertices.
    let refs = construct::references_of(&p, "Drain", -1.0).unwrap();
    assert_eq!(construct::arcs_through(a, b, &refs).unwrap().len(), 2);
    // The terminal's voice at zero offset, which said "no circle" before.
    let text = construct::report(&p, a, b, "Drain", 0.0, 2).unwrap();
    assert!(text.contains("# 2 arcs"), "{text}");
}
