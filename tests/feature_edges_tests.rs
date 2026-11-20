// ---- the feature wireframe's pins (DECISIONS.md "Phase 6, slice 2…") ---------
//
// `view::feature_edges` is the per-variant geometry home slice 0 deferred
// with "written once, when drawn". These pins hold it to the declarations:
// every point of a revolved feature's edge satisfies that feature's own
// implicit equation, the datums sit where the parser says they sit, and the
// deliberately-undrawn residue (tool-dependent decoration geometry,
// polyline-edge decorations) stays absent by assertion, not by forgetting.
//
// There is deliberately NO global containment assertion in the sweep:
// sphere.cam's dome rises 20mm ABOVE a stock whose top is z=0 — declared
// features legitimately stand outside the stock's box, which is exactly why
// the window's fit folds edges in (`view::edges_aabb`).

use nacs::parse::{self, Feature, Part};
use nacs::view::{self, FeatureEdges};

fn load(dir: &str) -> Part {
    let main_src = std::fs::read_to_string(format!("{dir}/main.part"))
        .unwrap_or_else(|_| panic!("read {dir}/main.part"));
    let geom = std::fs::read_to_string(format!("{dir}/geometry.part")).ok();
    parse::parse_project_with_base(&main_src, geom.as_deref(), Some(std::path::Path::new(dir)))
        .unwrap_or_else(|e| panic!("parse {dir}: {e}"))
}

fn edge<'a>(edges: &'a [FeatureEdges], name: &str) -> &'a FeatureEdges {
    edges.iter().find(|w| w.name == name).unwrap_or_else(|| panic!("no edge named {name}"))
}

fn points(w: &FeatureEdges) -> impl Iterator<Item = &[f64; 3]> {
    w.polylines.iter().flatten()
}

/// Is some polyline a closed ring of the given radius about (x, y) at height
/// z? The pins' workhorse: every named circle a variant promises is found by
/// this predicate, never by index into the polyline list.
fn closed(pl: &[[f64; 3]]) -> bool {
    // The closing point comes back through cos(2π)/sin(2π), which is 1.0
    // and -0.0 only approximately — closure is geometric, not bitwise.
    let (a, b) = (pl[0], pl[pl.len() - 1]);
    (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9 && (a[2] - b[2]).abs() < 1e-9
}

fn has_ring(w: &FeatureEdges, x: f64, y: f64, r: f64, z: f64) -> bool {
    w.polylines.iter().any(|pl| {
        pl.len() > 4
            && closed(pl)
            && pl.iter().all(|p| {
                ((p[0] - x).hypot(p[1] - y) - r).abs() < 1e-9 && (p[2] - z).abs() < 1e-9
            })
    })
}

/// The totality sweep: every project in the repo, every declared feature,
/// a real edge — finite, non-degenerate, name-sorted. Parse failures are
/// failures: every committed fixture parses.
#[test]
fn test_every_fixture_feature_yields_a_finite_wire() {
    let mut dirs: Vec<_> = std::fs::read_dir(".")
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "cam"))
        .map(|e| e.path())
        .collect();
    dirs.sort();
    assert!(dirs.len() > 50, "the fixture population went missing: {}", dirs.len());

    for dir in dirs {
        let p = load(dir.to_str().unwrap());
        let edges = view::feature_edges(&p);
        let names: Vec<&str> = edges.iter().map(|w| w.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "{dir:?}: edges must be name-sorted");
        assert_eq!(edges.len(), p.features.len(), "{dir:?}: every feature draws");
        for w in &edges {
            assert!(!w.polylines.is_empty(), "{dir:?}/{}: no polylines", w.name);
            for pl in &w.polylines {
                assert!(pl.len() >= 2, "{dir:?}/{}: degenerate polyline", w.name);
                for pt in pl {
                    assert!(
                        pt.iter().all(|c| c.is_finite()),
                        "{dir:?}/{}: non-finite coordinate {pt:?}",
                        w.name
                    );
                }
            }
        }
    }
}

/// Revolved-surface pins: every point of the edge satisfies the feature's
/// own implicit equation, taken from the parsed declaration — never from
/// numbers restated here.
#[test]
fn test_revolved_wires_lie_on_their_declared_surfaces() {
    // Sphere: |p - centre| == r on the dome half.
    let p = load("sphere.cam");
    let (dia, x, y, z) = match p.features["TopDome"] {
        Feature::Sphere { dia, x, y, z } => (dia, x, y, z),
        _ => unreachable!(),
    };
    let edges = view::feature_edges(&p);
    let w = edge(&edges, "TopDome");
    for pt in points(w) {
        let d = ((pt[0] - x).powi(2) + (pt[1] - y).powi(2) + (pt[2] - z).powi(2)).sqrt();
        assert!((d - dia / 2.0).abs() < 1e-9, "off-sphere point {pt:?}");
        assert!(pt[2] >= z - 1e-9, "below the equator: the dome is the declaration");
    }
    assert!(has_ring(w, x, y, dia / 2.0, z), "the equator circle is a named parallel");

    // Torus: meridian distance to the tube-centre circle == tube radius,
    // upper half only; both equators are rings AT z.
    let p = load("torus.cam");
    let (maj, min, x, y, z) = match p.features["T"] {
        Feature::Torus { major_dia, minor_dia, x, y, z } => (major_dia, minor_dia, x, y, z),
        _ => unreachable!(),
    };
    let (rm, rt) = (maj / 2.0, min / 2.0);
    let edges = view::feature_edges(&p);
    let w = edge(&edges, "T");
    for pt in points(w) {
        let rho = (pt[0] - x).hypot(pt[1] - y);
        let d = (rho - rm).hypot(pt[2] - z);
        assert!((d - rt).abs() < 1e-9, "off the tube circle: {pt:?}");
        assert!(pt[2] >= z - 1e-9, "lower tube half is not declared");
    }
    assert!(has_ring(w, x, y, rm + rt, z), "outer equator at z");
    assert!(has_ring(w, x, y, rm - rt, z), "inner equator at z");
    assert!(has_ring(w, x, y, rm, z + rt), "crown at z + tube radius");
}

/// The spline pair: edge meridians are the authored b-spline, evaluated —
/// bitwise against `meridian.eval`, offset by the placement.
#[test]
fn test_spline_wires_are_the_curve_evaluated() {
    let p = load("spline_cap.cam");
    let (meridian, x, y, z) = match &p.features["Cap"] {
        Feature::SplineRevolve { meridian, x, y, z, .. } => (meridian.clone(), *x, *y, *z),
        _ => unreachable!(),
    };
    let edges = view::feature_edges(&p);
    let w = edge(&edges, "Cap");
    // The azimuth-0 meridian is a polyline in the +X half-plane from (x, y).
    let (u0, u1) = meridian.domain();
    let expected: Vec<[f64; 3]> = (0..view::EDGE_MERIDIAN_PTS)
        .map(|i| {
            let u = u0 + (u1 - u0) * i as f64 / (view::EDGE_MERIDIAN_PTS - 1) as f64;
            let (rho, z_rel) = meridian.eval(u);
            [x + rho, y, z + z_rel]
        })
        .collect();
    assert!(
        w.polylines.iter().any(|pl| pl == &expected),
        "the azimuth-0 meridian is eval() verbatim"
    );

    // The funnel: mouth maps to the stock top.
    let p = load("spline_funnel.cam");
    let (meridian, x, y) = match &p.features["Vortex"] {
        Feature::SplineRevolveFunnel { meridian, x, y, .. } => (meridian.clone(), *x, *y),
        _ => unreachable!(),
    };
    let edges = view::feature_edges(&p);
    let w = edge(&edges, "Vortex");
    let (u0, _) = meridian.domain();
    let mouth_rho = meridian.eval(u0).0;
    assert!(has_ring(w, x, y, mouth_rho, p.stock.z), "the mouth circle sits in the stock top");
}

/// The prismatic pins: the polyline pair is top-down in BOTH variants (the
/// boss does NOT borrow CylinderBoss's base-up convention) and the
/// zero-extent contour is one edge.
#[test]
fn test_prismatic_wires_and_datums() {
    // ProfileBoss: loops at derive_contour_top and top - extent.
    let p = load("profile_boss.cam");
    let (points_n, z, extent) = match &p.features["Outline"] {
        Feature::ProfileBoss { points, z, extent, .. } => (points.len(), *z, *extent),
        _ => unreachable!(),
    };
    let top = parse::derive_contour_top(p.stock.z, z);
    let edges = view::feature_edges(&p);
    let w = edges
        .iter()
        .find(|w| w.kind == "ProfileBoss")
        .expect("the boss wire");
    let loop_at = |zl: f64| {
        w.polylines.iter().any(|pl| {
            pl.len() == points_n + 1 && pl.iter().all(|pt| (pt[2] - zl).abs() < 1e-9)
        })
    };
    assert!(loop_at(top), "top loop at derive_contour_top — the boss is TOP-DOWN");
    assert!(loop_at(top - extent), "bottom loop one extent below");

    // ContourBoss with extent=0: the trim edge is ONE edge — no bottom loop,
    // no ties.
    let p = load("contour_boss.cam");
    let edges = view::feature_edges(&p);
    let w = edges.iter().find(|w| w.kind == "ContourBoss").expect("the crest wire");
    assert_eq!(w.polylines.len(), 1, "extent == 0 is a single wire");

    // ContourPocket with extent=3: every tie spans exactly the extent.
    let p = load("contour_pocket.cam");
    let (extent,) = match &p.features["Rim"] {
        Feature::ContourPocket { extent, .. } => (*extent,),
        _ => unreachable!(),
    };
    let edges = view::feature_edges(&p);
    let w = edge(&edges, "Rim");
    let ties: Vec<_> = w.polylines.iter().filter(|pl| pl.len() == 2).collect();
    assert!(!ties.is_empty(), "a swept edge has ties");
    for t in ties {
        assert!((t[0][2] - t[1][2] - extent).abs() < 1e-9, "tie must span the extent");
        assert!((t[0][0] - t[1][0]).abs() < 1e-12 && (t[0][1] - t[1][1]).abs() < 1e-12);
    }
}

/// The camp-3 pair: mesh edges dedup'd and placed, patch iso-grid on the
/// exact surface.
#[test]
fn test_mesh_and_patch_wires() {
    // The 4-triangle open pyramid: 12 edge instances, 4 shared flanks -> 8
    // unique edges; apex at model (20,20,10) + placement (10,10,0).
    let p = load("mesh.cam");
    let edges = view::feature_edges(&p);
    let w = edge(&edges, "Blob");
    assert_eq!(w.polylines.len(), 8, "8 dedup'd edges of the open pyramid");
    assert!(
        points(w).any(|pt| (pt[0] - 30.0).abs() < 1e-12
            && (pt[1] - 30.0).abs() < 1e-12
            && (pt[2] - 10.0).abs() < 1e-12),
        "the apex lands at its placed position"
    );

    // The patch: every drawn point IS surface.eval at its reconstructed
    // parameters plus the placement offset.
    let p = load("nurbs_patch.cam");
    let (surface, ox, oy, oz) = match &p.features["Dome"] {
        Feature::NurbsPatch { surface, x, y, z, .. } => (surface.clone(), *x, *y, *z),
        _ => unreachable!(),
    };
    let edges = view::feature_edges(&p);
    let w = edge(&edges, "Dome");
    assert!(w.polylines.len() >= 16, "at least 8 iso-lines per direction");
    let (v0, v1) = surface.domain_v();
    // The first polyline is the first u iso-line at the u-domain start.
    let (u0, _) = surface.domain_u();
    let first = &w.polylines[0];
    for (i, pt) in first.iter().enumerate() {
        let s = i as f64 / (view::EDGE_MERIDIAN_PTS - 1) as f64;
        let ev = surface.eval(u0, v0 + (v1 - v0) * s);
        assert!(
            (pt[0] - (ev[0] + ox)).abs() < 1e-9
                && (pt[1] - (ev[1] + oy)).abs() < 1e-9
                && (pt[2] - (ev[2] + oz)).abs() < 1e-9,
            "iso-line point off the exact surface"
        );
    }
}

/// Decorations: what the declaration alone pins is drawn; what needs the
/// tool stays a NAMED absence, asserted so the residue can't be forgotten.
#[test]
fn test_decoration_rings_are_tool_free_only() {
    // cbore: seat + floor circles at the declared dia/depth.
    let p = load("cbore.cam");
    let edges = view::feature_edges(&p);
    let w = edge(&edges, "Hole1");
    let (x, y, cb_dia, cb_depth) = match p.features["Hole1"] {
        Feature::CylinderBore { x, y, cbore: Some(cb), .. } => (x, y, cb.dia, cb.depth),
        _ => unreachable!(),
    };
    assert!(has_ring(w, x, y, cb_dia / 2.0, p.stock.z), "cbore seat circle");
    assert!(has_ring(w, x, y, cb_dia / 2.0, p.stock.z - cb_depth), "cbore floor circle");

    // csink: the seat circle only — the frustum floor needs the tool's tip
    // flat, so no polyline may dip between the seat and the bore wall.
    let p = load("csink.cam");
    let edges = view::feature_edges(&p);
    let w = edge(&edges, "Hole1");
    let (x, y, bore_dia, seat) = match &p.features["Hole1"] {
        Feature::CylinderBore { x, y, dia, csink: Some(spec), .. } => {
            (*x, *y, *dia, spec.seat_dia())
        }
        _ => unreachable!(),
    };
    assert!(has_ring(w, x, y, seat / 2.0, p.stock.z), "csink seat circle at the top");
    for pt in points(w) {
        let rho = (pt[0] - x).hypot(pt[1] - y);
        if rho > bore_dia / 2.0 + 1e-9 && rho < seat / 2.0 - 1e-9 {
            assert!(
                (pt[2] - p.stock.z).abs() < 1e-9,
                "nothing between seat and wall below the top — the frustum is the tool's"
            );
        }
    }

    // edge_round on the circular pair: both tangency circles.
    let p = load("edge_round.cam");
    let edges = view::feature_edges(&p);
    for (name, f) in &p.features {
        let er = parse::derive_edge_round(&p, f).expect("both fixtures declare a round");
        let (x, y, wall_r) = er.circle().expect("circular edges here");
        let w = edge(&edges, name);
        assert!(has_ring(w, x, y, wall_r, er.centre_z(0.0)), "{name}: wall tangency circle");
        assert!(has_ring(w, x, y, er.centre(0.0, 0.0).0, er.z_top), "{name}: face tangency circle");
    }

    // chamfer: the declared leg pins its circle; without an angle the other
    // leg's circle is absent.
    let p = load("chamfer.cam");
    let edges = view::feature_edges(&p);
    let (x, y, wall_r, depth) = match p.features["Bore"] {
        Feature::CylinderBore {
            x, y, dia, chamfer: Some(parse::ChamferSpec { leg: parse::ChamferLeg::Depth(d), .. }), ..
        } => (x, y, dia / 2.0, d),
        _ => unreachable!(),
    };
    let w = edge(&edges, "Bore");
    assert!(has_ring(w, x, y, wall_r, p.stock.z - depth), "depth leg pins the wall circle");

    // Polyline-edge decorations: the named absence. The profile fixtures
    // declare edge_round=3 and get loops + ties ONLY.
    let p = load("edge_round_profile.cam");
    let edges = view::feature_edges(&p);
    for w in &edges {
        for pl in &w.polylines {
            let planar_ring = pl.len() > 4
                && pl.first() == pl.last()
                && pl.iter().all(|pt| (pt[2] - pl[0][2]).abs() < 1e-9);
            if planar_ring {
                // The only closed planar loops are the feature's own top and
                // bottom loops — never a derived tangency path.
                let n_pts = match &p.features[&w.name] {
                    Feature::ProfilePocket { points, .. } | Feature::ProfileBoss { points, .. } => {
                        points.len() + 1
                    }
                    _ => continue,
                };
                assert_eq!(
                    pl.len(),
                    n_pts,
                    "{}: a polyline-edge decoration appeared — that residue is the offsetter's",
                    w.name
                );
            }
        }
    }
}

/// Phase 45 slice 2: a bore's wire is drawn from its RIM — the rim ring at
/// `z`, the floor ring one extent under it — and so are the mouth rings of a
/// counterbore and a countersink. With no `z` the rim is the stock top, which
/// every other bore pin in this file already holds.
#[test]
fn test_sunken_bore_wires_sit_on_the_rim() {
    let p = load("step_bore.cam");
    let edges = view::feature_edges(&p);
    let bore = edge(&edges, "Bore");
    assert!(has_ring(bore, 50.0, 50.0, 15.0, -8.0), "the rim ring at z");
    assert!(has_ring(bore, 50.0, 50.0, 15.0, -14.0), "the floor ring one extent under the rim");
    assert!(!has_ring(bore, 50.0, 50.0, 15.0, p.stock.z), "nothing drawn at the stock top");
    let hole = edge(&edges, "Hole");
    assert!(has_ring(hole, 20.0, 20.0, 4.0, -8.0) && has_ring(hole, 20.0, 20.0, 4.0, -16.0), "the hole's rim and floor");

    let src = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
               stock Box(w=100, l=100, h=20, x=0, y=0, z=0)\ntool T5 Endmill(dia=10.0)\n\
               feature Seat CylinderBore(dia=6, extent=8, x=30, y=30, z=-5, cbore_dia=12, cbore_depth=3)\n\
               feature Sink CylinderBore(dia=6, extent=8, x=60, y=30, z=-5, csink_dia=10, csink_angle=90)\n";
    let p = parse::parse_project_with_base(src, None, None).unwrap();
    let edges = view::feature_edges(&p);
    let seat = edge(&edges, "Seat");
    assert!(has_ring(seat, 30.0, 30.0, 6.0, -5.0), "cbore seat circle at the rim");
    assert!(has_ring(seat, 30.0, 30.0, 6.0, -8.0), "cbore floor circle one seat depth under the rim");
    let sink = edge(&edges, "Sink");
    assert!(has_ring(sink, 60.0, 30.0, 5.0, -5.0), "csink seat circle at the rim");
}
