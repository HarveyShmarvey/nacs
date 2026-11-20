//! Phase 46 slice 1 — the ROUND blank (`DECISIONS.md` → "DRAFT: Phase 46
//! opens…", fork 1A, and "Phase 46 slice 1…"). `stock Cylinder(dia=, h=, x=,
//! y=, z=)` is the blank as a polygon in one home, `parse::derive_blank`,
//! and every reader of the footprint — the region, the stock model, the
//! containment lint, the voxel field, the picture, the form, the arc
//! construction, the outline's row — reads it there. Every pin here is over
//! what a derived view says; nothing is read back off a golden.
use nacs::construct::{self, Reference};
use nacs::parse::{self, StockShape};
use nacs::region::{self, EdgeKind};
use nacs::{outline, stock, view, vox};

fn load(dir: &str) -> parse::Part {
    let src = std::fs::read_to_string(format!("{dir}/main.part")).unwrap();
    let p = parse::parse(&src).unwrap();
    parse::validate(&p).unwrap();
    p
}

const PREAMBLE: &str = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n";

/// One stock line under a one-op program.
fn with_stock(units_and_stock: &str) -> Result<parse::Part, String> {
    parse::parse(&format!(
        "{units_and_stock}\ntool T5 Endmill(dia=10)\nfeature Top Plane(z=-1)\nrun End(tool=T5, surface=Top, stepover=4)\n"
    ))
}

fn radius(q: &parse::Pt) -> f64 {
    q.x.hypot(q.y)
}

fn signed_area(l: &[parse::Pt]) -> f64 {
    (0..l.len()).map(|i| l[i].x * l[(i + 1) % l.len()].y - l[(i + 1) % l.len()].x * l[i].y).sum::<f64>() / 2.0
}

/// The PROFILE blank (Phase 54 slice 5a, fork 3A): a block statement whose
/// points are the plan outline — the struct keeps the loop's bounding box,
/// `derive_blank` answers the loop, grown it grows, the region of a Plane
/// is the loop, the stock model and the voxel field read the loop, the
/// outline's row and the form say the kind; the winding law and the point
/// count refuse by name; the field is never cached.
#[test]
fn test_the_profile_blank_is_the_loop_in_every_reader() {
    let p = load("stock_profile.cam");
    let StockShape::Profile { ref points } = p.stock.shape else { panic!("a Profile") };
    assert_eq!(points.len(), 6);
    assert_eq!((p.stock.w, p.stock.l, p.stock.h, p.stock.x, p.stock.y, p.stock.z), (120.0, 80.0, 20.0, 0.0, 0.0, 0.0), "the loop's bounding box");
    assert_eq!(parse::derive_blank(&p.stock), *points, "the blank IS the loop");
    let grown = parse::derive_blank_grown(&p.stock, 5.0);
    assert!(signed_area(&grown) > signed_area(points) + 1.0, "grown grows: {} vs {}", signed_area(&grown), signed_area(points));
    assert!(signed_area(&parse::derive_blank_grown(&p.stock, 0.0)) == signed_area(points));
    // The region of the Plane is the loop: six edges, every one open (the
    // blank's own boundary is air, as a Box's is).
    let r = region::of(&p, &p.operations[0]).expect("the End's region");
    assert_eq!(r.outer.len(), 6);
    assert!(r.edges.iter().all(|e| *e == EdgeKind::Open), "{:?}", r.edges);
    // The stock model: inside the L is material, its missing corner (this
    // L's, the original plate's: x < 60, y > 40) is not.
    let before = stock::remaining_before(&p, 0);
    assert!(before.material_at(30.0, 20.0, -10.0));
    assert!(before.material_at(90.0, 60.0, -10.0), "the L's tall leg");
    assert!(!before.material_at(30.0, 60.0, -10.0), "the missing corner is outside the blank");
    // The voxel field builds on the loop and says it is not cached.
    let field = vox::VoxField::build_prefix(&p, 1, None).expect("the field on a profile blank");
    assert!(field.complete());
    assert!(field.save(std::path::Path::new("never.vox")).unwrap_err().contains("not cached"));
    // The outline's row and the form's kind.
    assert!(outline::stock_row(&p.stock).starts_with("stock  profile of 6 points within 120.000 x 80.000 x 20.000 mm"), "{}", outline::stock_row(&p.stock));
    assert!(stock::stock_words_row(&p.stock).starts_with("Profile(h=20, z=0) over 6 points"));
    let form = view::statement_form(&p, &view::Statement::Stock).unwrap();
    assert_eq!(form.head, "Profile");
    assert!(form.fields.iter().any(|f| f.key == "h"));
    // Refusals by name: the winding, the count, an unknown key, a short line.
    let cw = with_stock(&format!("{PREAMBLE}stock Profile(h=20
    0 0
    0 40
    40 40
    40 0
)
")).unwrap_err();
    assert!(cw.contains("winds clockwise"), "{cw}");
    let two = with_stock(&format!("{PREAMBLE}stock Profile(h=20
    0 0
    40 0
)
")).unwrap_err();
    assert!(two.contains("at least 3 points"), "{two}");
    let key = with_stock(&format!("{PREAMBLE}stock Profile(h=20, w=5
    0 0
    40 0
    40 40
)
")).unwrap_err();
    assert!(key.contains("stock Profile"), "{key}");
    // An unclosed block alone in the file: the end of the file says so (a
    // statement after it would be read as a point and refused as one).
    let open = parse::parse(&format!("{PREAMBLE}stock Profile(h=20{nl}    0 0{nl}    40 0{nl}    40 40{nl}", nl = char::from(10u8))).unwrap_err();
    assert!(open.contains("missing closing parenthesis"), "{open}");
}

/// The line says the AXIS; the struct keeps the bounding box, so every
/// extent reader (the raster, the voxel domain, the camera) reads it
/// unchanged, and `axis()` gives the centre back.
#[test]
fn test_the_round_blank_parses_to_its_bounding_box_and_axis() {
    let p = with_stock(&format!("{PREAMBLE}stock Cylinder(dia=120, h=30, x=10, y=-5, z=2)")).unwrap();
    assert_eq!(p.stock.shape, StockShape::Cylinder { dia: 120.0 });
    assert_eq!((p.stock.w, p.stock.l, p.stock.h), (120.0, 120.0, 30.0));
    assert_eq!((p.stock.x, p.stock.y, p.stock.z), (-50.0, -65.0, 2.0), "the box's datum corner, from the axis");
    assert_eq!(p.stock.axis(), (10.0, -5.0));
    assert_eq!(p.stock_line.as_ref().map(|(_, _, t)| t.as_str()), Some("stock Cylinder(dia=120, h=30, x=10, y=-5, z=2)"));

    // The axis defaults to the origin, as the Box's corner does.
    let p = with_stock(&format!("{PREAMBLE}stock Cylinder(dia=120, h=30)")).unwrap();
    assert_eq!((p.stock.x, p.stock.y, p.stock.z), (-60.0, -60.0, 0.0));
    assert_eq!(p.stock.axis(), (0.0, 0.0));

    // A Box is what it always was.
    let p = load("step.cam");
    assert_eq!(p.stock.shape, StockShape::Box);
    assert_eq!(p.stock.axis(), (50.0, 40.0));
}

/// The parser's whitelist is the Cylinder's own: the Box's keys are refused
/// by name, the diameter is required, a third shape is not a shape, and a
/// diameter of nothing fails `validate` in the Cylinder's words.
#[test]
fn test_the_round_blank_refuses_what_its_line_cannot_carry() {
    let err = with_stock(&format!("{PREAMBLE}stock Cylinder(dia=120, h=30, w=5)")).unwrap_err();
    assert!(err.contains("stock Cylinder has no field named 'w'"), "{err}");
    let err = with_stock(&format!("{PREAMBLE}stock Cylinder(h=30)")).unwrap_err();
    assert!(err.contains("missing key 'dia'"), "{err}");
    let err = with_stock(&format!("{PREAMBLE}stock Sphere(dia=5)")).unwrap_err();
    assert!(err.contains("only 'Box' and 'Cylinder' are supported"), "{err}");
    // (`parse` runs `validate`; the shape's own words either way.)
    assert_eq!(with_stock(&format!("{PREAMBLE}stock Cylinder(dia=0, h=30)")).unwrap_err(), "stock dia must be positive");
    assert_eq!(with_stock(&format!("{PREAMBLE}stock Box(w=0, l=10, h=30)")).unwrap_err(), "stock size w and l must be positive");
}

/// Inch input scales the diameter at the parse boundary like every length.
#[test]
fn test_inch_input_scales_the_round_blank_s_diameter() {
    let p = with_stock("units inch\nfeed 30\nplunge 10\nrapid_z 0.2\nclearance_z 1\nspindle 3000\nstock Cylinder(dia=4, h=1, x=1, y=0)").unwrap();
    assert_eq!(p.stock.shape, StockShape::Cylinder { dia: 4.0 * 25.4 });
    assert_eq!((p.stock.w, p.stock.l, p.stock.h), (4.0 * 25.4, 4.0 * 25.4, 25.4));
    assert_eq!(p.stock.axis(), (25.4, 0.0));
}

/// The one home: a Box's four corners CCW from the datum; a round's chords at
/// the one tolerance, CCW, on the radius; grown, a bigger round and never a
/// box.
#[test]
fn test_derive_blank_is_the_box_s_corners_or_the_round_s_chords() {
    let plate = load("step.cam");
    let b = parse::derive_blank(&plate.stock);
    assert_eq!(b.len(), 4);
    assert_eq!((b[0].x, b[0].y, b[2].x, b[2].y), (0.0, 0.0, 100.0, 80.0));
    assert!(signed_area(&b) > 0.0, "CCW");
    let g = parse::derive_blank_grown(&plate.stock, 5.0);
    assert_eq!((g[0].x, g[0].y, g[2].x, g[2].y), (-5.0, -5.0, 105.0, 85.0));

    let disc = load("disc_step.cam");
    let b = parse::derive_blank(&disc.stock);
    assert!(b.len() > 8, "a chorded circle, not a box: {}", b.len());
    for q in &b {
        assert!((radius(q) - 60.0).abs() < 1e-9, "on the radius: {q:?}");
    }
    assert!(signed_area(&b) > 0.0, "CCW");
    let area = signed_area(&b);
    let exact = std::f64::consts::PI * 60.0 * 60.0;
    assert!((exact - area) / exact < 0.01 && area < exact, "the chords sit inside the circle by a sagitta: {area} vs {exact}");
    for q in parse::derive_blank_grown(&disc.stock, 5.0) {
        assert!((radius(&q) - 65.0).abs() < 1e-9, "grown by 5: {q:?}");
    }
}

/// A `Plane` on a round blank is the disc: every edge open, the pad an
/// island, the bore a void the floor runs over — `--outline`'s own words.
#[test]
fn test_a_plane_on_a_round_blank_is_a_disc_open_all_round() {
    let p = load("disc_step.cam");
    let r = region::of(&p, &p.operations[0]).expect("an End on a Plane works a region");
    assert_eq!(r.edges.len(), parse::derive_blank(&p.stock).len(), "one edge per chord");
    assert!(r.edges.iter().all(|k| *k == EdgeKind::Open), "open all round: {:?}", r.edges);
    assert_eq!(r.feature_islands(), vec!["Pad"]);
    assert_eq!(r.bridged(), vec!["Hole"]);
    assert_eq!(outline::removal_line(&p, 0), format!("removes: polygon ({} pts) around 1 island z -10.000..0.000", r.edges.len()));
}

/// The stock model: beyond the round is air at prefix 0, where a Box of the
/// same size holds steel — the DRAFT's premise 7, the reason the shape
/// exists. The containment lint speaks of the round by its radius, and a
/// void past it is air declared air, as past a box's face.
#[test]
fn test_the_stock_model_reads_the_round_blank_as_air_beyond_its_radius() {
    let disc = load("disc_step.cam");
    // After the first op (the step to -10): below its floor the round is
    // untouched, and the probes are about the blank, not the cut.
    let before = stock::remaining_after(&disc, 0);
    assert!(!before.material_at(55.0, 55.0, -15.0), "the corner a box would have: air");
    assert!(before.material_at(0.0, 59.0, -15.0), "inside the round by a mm: steel");
    assert!(!before.material_at(0.0, 61.0, -15.0), "outside by a mm: air");
    assert!(before.material_at(0.0, 0.0, -29.0) && !before.material_at(0.0, 0.0, -31.0), "the height is the Box's law");

    let src = std::fs::read_to_string("disc_step.cam/main.part").unwrap();
    let boxed = parse::parse(&src.replace("stock Cylinder(dia=120, h=30, x=0, y=0, z=0)", "stock Box(w=120, l=120, h=30, x=-60, y=-60, z=0)")).unwrap();
    parse::validate(&boxed).unwrap();
    assert!(stock::remaining_after(&boxed, 0).material_at(55.0, 55.0, -15.0), "the same program on a Box: the corner is steel");

    // The lint: a boss reaching past the round names the reach; a void
    // past it is not a plan finding (Phase 27, fork 2A), as past a face.
    let past = parse::parse(&src.replace("    58 15\n    58 -15\n", "    70 15\n    70 -15\n")).unwrap();
    let found = stock::uncontained(&past);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].name, "Pad");
    let reach = (70.0f64).hypot(15.0) - 60.0;
    assert_eq!(found[0].faces[0].1, format!("{reach:.3}mm past the stock's round of dia 120.000 about (0.000, 0.000)"));
    let void_past = parse::parse(&src.replace("x=-35.0, y=0.0, z=-10.0", "x=-58.0, y=0.0, z=-10.0")).unwrap();
    assert!(stock::uncontained(&void_past).is_empty(), "{:?}", stock::uncontained(&void_past));
}

/// The outline's row says the axis, as the line does; the form offers the
/// Cylinder's keys with its two required.
#[test]
fn test_the_outline_s_row_and_the_form_speak_the_round_blank() {
    let p = load("disc_step.cam");
    assert_eq!(outline::stock_row(&p.stock), "stock  dia 120.000 x 30.000 mm  axis (0.000, 0.000)  top z 0.000");
    assert!(outline::render(&p, &outline::outline(&p)).starts_with("stock  dia 120.000 x 30.000 mm  axis (0.000, 0.000)  top z 0.000\n"));
    let form = view::statement_form(&p, &view::Statement::Stock).unwrap();
    assert_eq!(form.head, "Cylinder");
    assert_eq!(form.fields.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(), parse::STOCK_CYLINDER_KEYS.to_vec());
    assert_eq!(form.fields.iter().filter(|f| f.required).map(|f| f.key.as_str()).collect::<Vec<_>>(), vec!["dia", "h"]);
    let dia = form.fields.iter().find(|f| f.key == "dia").unwrap();
    assert_eq!((dia.value.as_deref(), dia.options.is_none()), (Some("120"), true));
    assert!(view::field_write(&form, "dia", "").unwrap_err().contains("needs dia"));
}

/// The picture: the `Plane`'s wire rides the round at its height, closed.
#[test]
fn test_the_plane_s_wire_rides_the_round_blank() {
    let p = load("disc_step.cam");
    let step = view::feature_edges(&p).into_iter().find(|w| w.name == "Step").unwrap();
    assert_eq!(step.polylines.len(), 1);
    let ring = &step.polylines[0];
    assert_eq!(ring.first(), ring.last(), "closed");
    assert_eq!(ring.len(), parse::derive_blank(&p.stock).len() + 1);
    for q in ring {
        assert!((q[0].hypot(q[1]) - 60.0).abs() < 1e-9 && q[2] == -10.0, "{q:?}");
    }
}

/// `--arc`'s reference for the blank is the round pulled IN by the offset —
/// the circle itself, never its chords — and an offset past the radius
/// leaves nothing.
#[test]
fn test_the_arc_construction_s_stock_reference_is_the_round_pulled_in() {
    let p = load("disc_step.cam");
    assert_eq!(construct::references_of(&p, "stock", 5.0).unwrap(), vec![Reference::Circle { cx: 0.0, cy: 0.0, r: 55.0 }]);
    assert!(construct::references_of(&p, "stock", 60.0).unwrap_err().contains("leaves nothing of the stock"));
}

/// The voxel field: outside the round is air at prefix 0, the shape rides
/// the cache's hash and bytes, and a loaded field answers the same.
#[test]
fn test_the_voxel_field_holds_air_beyond_the_round_and_round_trips_its_shape() {
    let p = load("disc_step.cam");
    let cell = Some(p.stock.w / 64.0);
    let field = vox::VoxField::build_with_cell(&p, cell).unwrap();
    assert!(!field.material_at(55.0, 55.0, -5.0), "a box's corner: air on a round blank");
    assert!(field.material_at(0.0, 0.0, -25.0), "under the step's floor: steel");
    assert!(!field.material_at(0.0, 45.0, -5.0), "the step, cut: air");

    let dir = std::path::Path::new("target/vox_cache_tests");
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("round_blank.vox");
    field.save(&path).unwrap();
    let loaded = vox::VoxField::load(&path, &p, cell).unwrap().expect("a fresh cache for the same inputs must hit");
    assert!(!loaded.material_at(55.0, 55.0, -5.0) && loaded.material_at(0.0, 0.0, -25.0), "the shape survived the file");

    // A Box of the same numbers is a different input: the cache misses.
    let src = std::fs::read_to_string("disc_step.cam/main.part").unwrap();
    let boxed = parse::parse(&src.replace("stock Cylinder(dia=120, h=30, x=0, y=0, z=0)", "stock Box(w=120, l=120, h=30, x=-60, y=-60, z=0)")).unwrap();
    assert!(vox::VoxField::load(&path, &boxed, cell).unwrap().is_none(), "the shape is in the hash");
}

// ---- Phase 47 slice 1: containment on a round reads the region's three decimals

/// A one-feature program on a ⌀120 round blank at the origin.
fn on_the_round(feature: &str) -> parse::Part {
    let p = parse::parse(&format!("{PREAMBLE}stock Cylinder(dia=120, h=30, x=0, y=0, z=0)\ntool T5 Endmill(dia=10)\n{feature}")).unwrap();
    parse::validate(&p).unwrap();
    p
}

/// A boss whose corner lands on the round as a three-decimal coordinate
/// can (fork 2A): (36.001, 48.000) is r 60.0006 — past the exact circle by
/// more than the old 1e-6 guard, and ON it at the region's `ON_THE_BLANK`.
/// The measure is the ROUND, not `derive_blank`'s chords: the chorded
/// footprint is inscribed, and a vertex on the circle sits up to the chord
/// tolerance (0.010) outside it. A corner 0.002 past is still named, by the
/// number, and the decline follows the finding as it always did.
#[test]
fn test_a_vertex_on_the_round_to_three_decimals_is_contained_and_a_hair_past_it_is_named() {
    let pad = |x: &str| format!("feature Pad ProfileBoss(extent=5.0\n    {x} 48.000\n    48.000 36.000\n    30 30\n)\nrun Side(tool=T5, surface=Pad)\n");
    let on = on_the_round(&pad("36.001"));
    assert!((radius(&parse::Pt { x: 36.001, y: 48.0 }) - 60.0 - 0.0006).abs() < 1e-4);
    assert!(stock::uncontained(&on).is_empty(), "{:?}", stock::uncontained(&on));
    let said: Vec<String> = parse::lint(&on).into_iter().map(|f| f.message).collect();
    assert!(!said.iter().any(|m| m.contains("stands outside")), "{said:?}");

    let past = on_the_round(&pad("36.002").replace("48.000\n    48.000 36.000", "48.001\n    48.000 36.000"));
    let found = stock::uncontained(&past);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].faces[0].1, "0.002mm past the stock's round of dia 120.000 about (0.000, 0.000)");
    let said: Vec<String> = parse::lint(&past).into_iter().map(|f| f.message).collect();
    assert!(said.iter().any(|m| m.contains("'Pad' (ProfileBoss) stands outside the stock: 0.002mm past")), "{said:?}");
    // The decline follows the finding, as it always did: the predicate both
    // consumers read (`check_moves` and the rest report) answers for the op.
    assert!(stock::uncontained_for(&past, 0, &found).is_some(), "the decline follows the finding");
    assert!(stock::uncontained_for(&on, 0, &stock::uncontained(&on)).is_none(), "and reads the contained one");
}

/// A `Face` is the plan's void (fork 2A): its loop is the silhouette drawn
/// past the blank for the region to clip, so in plan it is the pocket's case
/// — on a round and on a box alike — while its Z faces still speak.
#[test]
fn test_a_face_past_the_blank_is_quiet_in_plan_and_still_speaks_in_z() {
    let level = |z: &str| format!("feature Level Face(z={z}\n    loop\n        -70 -70\n        70 -70\n        70 70\n        -70 70\n)\nrun End(tool=T5, surface=Level, stepover=4)\n");
    let round = on_the_round(&level("-10"));
    assert!(stock::uncontained(&round).is_empty(), "{:?}", stock::uncontained(&round));
    let boxed = parse::parse(&format!("{PREAMBLE}stock Box(w=100, l=100, h=30, x=-50, y=-50, z=0)\ntool T5 Endmill(dia=10)\n{}", level("-10"))).unwrap();
    assert!(stock::uncontained(&boxed).is_empty(), "{:?}", stock::uncontained(&boxed));
    let high = parse::parse(&format!("{PREAMBLE}stock Cylinder(dia=120, h=30, x=0, y=0, z=0)\ntool T5 Endmill(dia=10)\n{}", level("1"))).unwrap();
    let found = stock::uncontained(&high);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].faces[0].1, "1.000mm above the stock top z 0.000");
}

/// The plan guard is the region's on either shape: a boss 0.0005 past a box
/// face is on it, 0.002 past is named. The Z guard is unchanged — a bore's
/// mouth ring on the stock top is exact, and stays inside.
#[test]
fn test_the_plan_guard_is_the_regions_three_decimals_on_a_box_too() {
    let boxed = |x: &str| {
        let p = parse::parse(&format!(
            "{PREAMBLE}stock Box(w=100, l=100, h=30, x=0, y=0, z=0)\ntool T5 Endmill(dia=10)\nfeature Pad ProfileBoss(extent=5.0\n    10 10\n    10 90\n    {x} 90\n    {x} 10\n)\nrun Side(tool=T5, surface=Pad)\n"
        ))
        .unwrap();
        parse::validate(&p).unwrap();
        p
    };
    assert!(stock::uncontained(&boxed("100.0005")).is_empty(), "{:?}", stock::uncontained(&boxed("100.0005")));
    let found = stock::uncontained(&boxed("100.002"));
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].faces[0].1, "0.002mm past the stock's +x face at x 100.000");
}

/// `disc_ribs.cam` is the real part's shape, self-authored: a `Face` level
/// run out to the round, two pads ON the round to three decimals, two sharp
/// L-bosses across a slot 1.1 tool diameters wide. The lint is quiet, every
/// op is read, and the region says what the outline says.
#[test]
fn test_disc_ribs_is_contained_read_and_open_all_round() {
    let p = parse::load_project("disc_ribs.cam").unwrap();
    parse::validate(&p).unwrap();
    assert!(stock::uncontained(&p).is_empty(), "{:?}", stock::uncontained(&p));
    // The pads' outer arcs: three-decimal points on r 60, some a hair past it.
    let reach = |name: &str| view::feature_edges(&p).into_iter().find(|w| w.name == name).unwrap().polylines.iter().flatten().map(|q| q[0].hypot(q[1])).fold(0.0, f64::max) - 60.0;
    assert!(reach("RimN") > 1e-6 && reach("RimN") < 1e-3, "{}", reach("RimN"));
    assert!(reach("RimS") > 1e-6 && reach("RimS") < 1e-3, "{}", reach("RimS"));
    let r = region::of(&p, &p.operations[0]).unwrap();
    // Open all round the blank — and since Phase 49 slice 1 the two pads ON
    // the round are bitten out of the boundary, so the edges that are not
    // open are theirs: their flanks and inner chords, every one lying on a
    // pad, and the pads are no longer islands.
    assert_eq!(r.on_the_shore, vec!["RimN".to_string(), "RimS".to_string()]);
    let pads: Vec<Vec<parse::Pt>> = ["RimN", "RimS"].iter().map(|n| match &p.features[*n] {
        parse::Feature::ProfileBoss { points, .. } => points.clone(),
        _ => unreachable!(),
    }).collect();
    let mut walls = 0;
    for (i, k) in r.edges.iter().enumerate() {
        if *k == EdgeKind::Open { continue; }
        walls += 1;
        let (a, b) = (r.outer[i], r.outer[(i + 1) % r.outer.len()]);
        let (mx, my) = ((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
        assert!(pads.iter().any(|pad| stock::dist_to_polygon(pad, mx, my) <= 1e-3), "edge {i} at ({mx:.3}, {my:.3}) is a wall on no pad");
    }
    assert_eq!(r.open_edges() + walls, r.edges.len());
    assert!(walls >= 16 && walls <= 24, "the two pads' flanks and inner chords: {walls}");
    assert_eq!(r.standing().len(), 2, "the two L-bosses stand inside; the pads are the shore");
    assert!((r.narrowest.as_ref().map_or(0.0, |n| n.width) - 11.0).abs() < 1e-6, "{:?}", r.narrowest);
    let said: Vec<String> = parse::lint(&p).into_iter().map(|f| f.message).collect();
    assert!(!said.iter().any(|m| m.contains("stands outside") || m.contains("declined")), "{said:?}");
}
