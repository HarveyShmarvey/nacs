// ---- the toolpath view model's pins (DECISIONS.md "Phase 6 opens…", slice 1) --
//
// Everything the window draws is derived in `view` — std-only, so these pins
// run on a machine that cannot open a window, under BOTH feature configs.
// What they hold, in order of how confidently a viewport would lie otherwise:
// (1) a control-comp op draws the RESOLVED centerline (ring radius 15), not
//     the part edge the text spells (radius 20);
// (2) the stock box hangs BELOW `stock.z` — drawn the other way up, the stock
//     floats above the table;
// (3) the projection is the exact linear map it claims (pinned at the axis
//     views, where every coordinate is legible by hand);
// (4) flattened arcs stay inside the chord tolerance and land exactly on
//     their emitted endpoints — a full circle closes bitwise on its start.

use nacs::parse::{self, OpRole};
use nacs::theme::{self, Center, DockAction, Document, Layout, SidebarTab, Theme};
use nacs::{gcode, view, CHORD_TOL_MM};

fn load(dir: &str) -> parse::Part {
    let main_src = std::fs::read_to_string(format!("{dir}/main.part"))
        .unwrap_or_else(|_| panic!("read {dir}/main.part"));
    let geom = std::fs::read_to_string(format!("{dir}/geometry.part")).ok();
    parse::parse_project_with_base(&main_src, geom.as_deref(), Some(std::path::Path::new(dir)))
        .unwrap_or_else(|e| panic!("parse {dir}: {e}"))
}

/// The channel rule, drawn. The inline fixture is the one
/// `test_control_comp_programs_stamp_their_resolved_centerline` uses: a
/// dia-40 bore walled by a dia-10 tool under comp=control, whose emitted TEXT
/// rings the part edge at radius 20 while the machine's centerline rings at
/// 15. A view built from the text would draw a 33% oversized toolpath with
/// total confidence.
#[test]
fn test_the_view_draws_the_resolved_centerline_not_the_text() {
    let src = "\
units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n\
tool T5 Endmill(dia=10.0)\n\
feature Bore CylinderBore(dia=40.0, extent=12.0, x=50.0, y=50.0)\n\
run Side(tool=T5, surface=Bore, stepover=5.0, comp=control)\n";
    let p = parse::parse(src).unwrap();
    let (_, emitted_moves) = gcode::program_with_moves(&p).unwrap();
    let paths = view::op_paths(&p, &emitted_moves);
    assert_eq!(paths.len(), 1);

    let max_r = paths[0]
        .polylines
        .iter()
        .filter(|pl| !pl.rapid)
        .flat_map(|pl| pl.pts.iter())
        .map(|pt| (pt[0] - 50.0).hypot(pt[1] - 50.0))
        .fold(0.0_f64, f64::max);
    assert!(max_r > 1.0, "the op must have drawable cutting motion");
    assert!(
        max_r < 15.0 + 0.1,
        "cutting motion must ring the resolved centerline (r=15), not the part edge (r=20); max radius drawn was {max_r}"
    );
}

/// The join key and the role ride each path: plate.cam is an End (untagged)
/// then the notch's Clear and Side (finishing), and each OpPath carries the
/// op index the outline panel selects by.
#[test]
fn test_paths_join_on_op_idx_and_carry_the_derived_role() {
    let p = load("plate.cam");
    let (_, emitted_moves) = gcode::program_with_moves(&p).unwrap();
    let paths = view::op_paths(&p, &emitted_moves);
    assert_eq!(paths.len(), 3);
    assert_eq!((paths[0].op_idx, paths[0].role), (0, OpRole::Untagged));
    assert_eq!((paths[1].op_idx, paths[1].role), (1, OpRole::Finishing));
    assert_eq!((paths[2].op_idx, paths[2].role), (2, OpRole::Finishing));
    // Both kinds of motion exist and are separated by the rapid flag.
    for path in &paths {
        assert!(path.polylines.iter().any(|pl| pl.rapid));
        assert!(path.polylines.iter().any(|pl| !pl.rapid));
    }
}

/// The stock hangs BELOW its top datum. stock Box(..., z=0, h=20) spans
/// z in [-20, 0] — `stock::material_at` is the authority this restates.
#[test]
fn test_the_stock_box_hangs_below_its_top_datum() {
    let p = load("plate.cam");
    let bb = view::stock_aabb(&p.stock);
    assert_eq!(bb, [0.0, 0.0, -20.0, 120.0, 80.0, 0.0]);
    // And the scene box grows upward to include the rapid plane above it.
    let (_, emitted_moves) = gcode::program_with_moves(&p).unwrap();
    let paths = view::op_paths(&p, &emitted_moves);
    let scene = view::scene_aabb(&p.stock, &paths);
    assert!(scene[5] >= 25.0, "the clearance retract at z=25 is part of the fit");
    assert_eq!(scene[2], -20.0, "nothing cuts below the stock in plate.cam");
}

/// The projection, pinned where it is hand-checkable: the axis views.
#[test]
fn test_projection_pins_at_the_axis_views() {
    // Side view: the viewer stands on +X looking toward −X, so screen x =
    // +Y, screen y = +Z — and a point at +X is NEAR the viewer, so its
    // away-growing depth is negative.
    let side = view::Camera { yaw: 0.0, pitch: 0.0, scale: 1.0, target: [0.0; 3] };
    let (x, y, d) = side.project([3.0, 4.0, 5.0]);
    assert!((x - 4.0).abs() < 1e-6 && (y - 5.0).abs() < 1e-6 && (d + 3.0).abs() < 1e-6);

    // Top view (pitch = π/2): height leaves the screen and becomes nearness —
    // the highest point sits closest to the overhead viewer.
    let top = view::Camera { yaw: 0.0, pitch: std::f64::consts::FRAC_PI_2, scale: 1.0, target: [0.0; 3] };
    let (x, y, d) = top.project([3.0, 4.0, 5.0]);
    assert!((x - 4.0).abs() < 1e-6, "screen x is still +Y");
    assert!((y + 3.0).abs() < 1e-6, "looking down, +X runs down the screen");
    assert!((d + 5.0).abs() < 1e-6, "height is nearness to the overhead viewer");

    // The scale is a uniform screen zoom, and the target recenters.
    let scaled = view::Camera { yaw: 0.0, pitch: 0.0, scale: 2.0, target: [1.0, 1.0, 1.0] };
    let (x, y, _) = scaled.project([1.0, 3.0, 4.0]);
    assert!((x - 4.0).abs() < 1e-6 && (y - 6.0).abs() < 1e-6);
}

/// Fitting is orientation-proof: the bounding SPHERE fits the viewport, so
/// orbiting afterwards can never push the scene off-screen.
#[test]
fn test_fit_to_aabb_holds_under_any_orbit() {
    let aabb = [0.0, 0.0, -20.0, 120.0, 80.0, 25.0];
    let mut cam = view::Camera::default_orbit();
    cam.fit_to_aabb(aabb, 800.0, 600.0);
    assert_eq!(cam.target, [60.0, 40.0, 2.5]);

    let corners: Vec<[f64; 3]> = (0..8)
        .map(|k| {
            [
                aabb[if k & 1 == 0 { 0 } else { 3 }],
                aabb[if k & 2 == 0 { 1 } else { 4 }],
                aabb[if k & 4 == 0 { 2 } else { 5 }],
            ]
        })
        .collect();
    for step in 0..24 {
        cam.yaw = f64::from(step) * 0.7;
        cam.pitch = f64::from(step % 7) * 0.25;
        for c in &corners {
            let (x, y, _) = cam.project(*c);
            assert!(
                x.abs() <= 400.0 && y.abs() <= 300.0,
                "corner {c:?} left the viewport at yaw {} pitch {}",
                cam.yaw,
                cam.pitch
            );
        }
    }
}

/// Flattened arcs: exact emitted endpoints, chords inside the tolerance, and
/// a full circle that closes on its own start. The fixture is the arcs-mode
/// bore — its walls are true G02/G03 full circles.
#[test]
fn test_arc_flattening_is_exact_at_the_ends_and_bounded_between() {
    let p = load("cylinder_bore_arcs.cam");
    let (_, emitted_moves) = gcode::program_with_moves(&p).unwrap();
    let paths = view::op_paths(&p, &emitted_moves);

    // A ring rides one polyline with its lead moves (connected feed motion
    // merges — that is what drawing wants), so the closure to find is a point
    // that RECURS later in the same polyline: the full circle's start. The
    // TIGHTEST such span is the ring — a lead-in that returns through its own
    // engage point closes a wider, non-circular span around it.
    let ring_of = |pts: &[[f64; 3]]| -> Option<(usize, usize)> {
        let mut best: Option<(usize, usize)> = None;
        for i in 0..pts.len() {
            if let Some(j) = ((i + 21)..pts.len()).find(|&j| pts[i] == pts[j]) {
                if best.is_none_or(|(bi, bj)| j - i < bj - bi) {
                    best = Some((i, j));
                }
            }
        }
        best
    };

    let mut saw_ring = false;
    for path in &paths {
        for pl in path.polylines.iter().filter(|pl| !pl.rapid) {
            if let Some((i, j)) = ring_of(&pl.pts) {
                let pts = &pl.pts[i..=j];
                {
                    saw_ring = true;
                    // Every vertex of a flattened circle sits ON the circle;
                    // the center is the mean of the ring's XY. Tolerances are
                    // sized to the WRITER's rounding, not to f64: the ring's
                    // endpoints are emitted coordinates (`gcode::f`, %.3f), so
                    // they — and the center estimate through them — carry up
                    // to ~1e-3 of quantization the exact interior vertices
                    // don't. A wrong flattening is coarser by orders.
                    const EMIT_QUANTUM: f64 = 3e-3;
                    let n = (pts.len() - 1) as f64;
                    let (cx, cy) = pts[..pts.len() - 1]
                        .iter()
                        .fold((0.0, 0.0), |a, p| (a.0 + p[0] / n, a.1 + p[1] / n));
                    let r0 = (pts[0][0] - cx).hypot(pts[0][1] - cy);
                    for w in pts.windows(2) {
                        let r_mid = ((w[0][0] + w[1][0]) / 2.0 - cx).hypot((w[0][1] + w[1][1]) / 2.0 - cy);
                        let sagitta = r0 - r_mid;
                        assert!(
                            sagitta <= CHORD_TOL_MM + EMIT_QUANTUM,
                            "chord sagitta {sagitta} exceeds CHORD_TOL_MM"
                        );
                        let rv = (w[1][0] - cx).hypot(w[1][1] - cy);
                        assert!(
                            (rv - r0).abs() < EMIT_QUANTUM,
                            "a flattened vertex left the circle by {}",
                            (rv - r0).abs()
                        );
                    }
                }
            }
        }
    }
    assert!(saw_ring, "the arcs-mode bore must produce at least one flattened full ring");
}

/// The lean build keeps the flag but not the window: `--view` must parse and
/// reject with a message that names the missing feature, never "unexpected
/// argument". Compiled only without the `frontend` feature (with it, the same
/// invocation would try to open a real window, which a test must not) — so it
/// runs in check.sh's `cargo test --no-default-features` leg.
#[cfg(not(feature = "frontend"))]
#[test]
fn test_view_without_the_frontend_feature_refuses_by_name() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_nacs"))
        .args(["--view", "plate.cam"])
        .output()
        .expect("run nacs");
    assert!(!out.status.success());
    assert!(out.stdout.is_empty(), "no partial artifact on stdout");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("frontend"), "the error must name the missing feature, got: {err}");
}

// ---- the map view model's pins (Phase 6, slice 3) ---------------------------

/// The plan stance is the drawing sheet: +X right, +Y up, height is nearness
/// to the overhead viewer. Pinned separately from the generic top view
/// (yaw 0) because the map's whole claim is this exact orientation — a plan
/// that runs +X down the screen is a rotated lie a viewer would trust.
#[test]
fn test_the_plan_stance_is_the_drawing_sheet() {
    let plan = view::Camera::plan();
    let (x, y, d) = plan.project([3.0, 4.0, 5.0]);
    assert!((x - 3.0).abs() < 1e-9, "+X runs right across the plan");
    assert!((y - 4.0).abs() < 1e-9, "+Y runs up the plan");
    assert!((d + 5.0).abs() < 1e-9, "height is nearness");
    // The basis rows agree — right/up are what panning and unprojection use.
    let (r, u) = (plan.right(), plan.up());
    for k in 0..3 {
        assert!((r[k] - [1.0, 0.0, 0.0][k]).abs() < 1e-12);
        assert!((u[k] - [0.0, 1.0, 0.0][k]).abs() < 1e-12);
    }
}

/// `unproject_screen` inverts `project` on the camera plane — at an
/// arbitrary orbit stance, not just the axis views: it is the same basis
/// rows in a second spelling, and THIS pin is the lock that keeps the two
/// spellings one (project's are inlined for the hot path). Depth reads 0:
/// the camera plane is where a screen point anchors. The tolerance is sized
/// to `project`'s f32 return (half-ULP ~4e-6 at screen magnitudes), not to
/// the f64 math being pinned — 1e-4 stays orders under any real drift.
#[test]
fn test_unproject_screen_inverts_the_projection_on_the_camera_plane() {
    let cam =
        view::Camera { yaw: 0.7, pitch: 0.4, scale: 2.5, target: [10.0, -5.0, 3.0] };
    for (sx, sy) in [(0.0, 0.0), (120.0, -45.0), (-3.5, 88.0), (123.4, 67.89)] {
        let p = cam.unproject_screen(sx, sy);
        let (x, y, d) = cam.project(p);
        assert!((f64::from(x) - sx).abs() < 1e-4, "screen x round-trips");
        assert!((f64::from(y) - sy).abs() < 1e-4, "screen y round-trips");
        assert!(f64::from(d).abs() < 1e-4, "the camera plane is depth 0");
    }
}

/// The plan fit frames the XY footprint: the tight axis fills 90% of its
/// span, both axes fit, and the Z extent — which the plan cannot show —
/// cannot shrink it (the reason this is not `fit_to_aabb`).
#[test]
fn test_plan_fit_frames_the_xy_footprint() {
    let bb = [0.0, 0.0, -20.0, 120.0, 80.0, 25.0];
    let cam = view::Camera::plan_fit(bb, 200.0, 100.0);
    assert_eq!(cam.target, [60.0, 40.0, 2.5]);
    // min(200/120, 100/80) = 1.25, with the 90% margin.
    assert!((cam.scale - 1.125).abs() < 1e-12);
    for (px, py) in [(0.0, 0.0), (120.0, 0.0), (120.0, 80.0), (0.0, 80.0)] {
        let (x, y, _) = cam.project([px, py, 0.0]);
        assert!(x.abs() <= 100.0 && y.abs() <= 50.0, "corner ({px}, {py}) left the map");
    }
    // The FITTED camera holds the plan STANCE — pinned through projection,
    // because the magnitude-only bounds above survive a stance swap (a
    // fitted orbit camera also keeps the corners in-frame): the max corner
    // lands exactly at (+67.5, +45), which pins orientation, scale, and
    // target jointly.
    let plan = view::Camera::plan();
    assert_eq!((cam.yaw, cam.pitch), (plan.yaw, plan.pitch));
    let (x, y, _) = cam.project([120.0, 80.0, 0.0]);
    assert!((f64::from(x) - 67.5).abs() < 1e-4 && (f64::from(y) - 45.0).abs() < 1e-4);

    // A part ten times taller fits identically: height is not plan footprint.
    let tall = view::Camera::plan_fit([0.0, 0.0, -1000.0, 120.0, 80.0, 1000.0], 200.0, 100.0);
    assert_eq!(tall.scale, cam.scale);

    // A zero-size map (a viewport thinner than its own padding) keeps unit
    // scale — the guard `unproject_screen`'s division leans on.
    let degenerate = view::Camera::plan_fit(bb, 0.0, 0.0);
    assert_eq!(degenerate.scale, 1.0);
}

/// The viewport indicator: at the plan stance the footprint is the visible
/// axis-aligned rectangle; edge-on (pitch 0) it collapses to a segment —
/// the honest degeneracy, drawn rather than faked.
#[test]
fn test_viewport_footprint_is_the_visible_slab_on_the_plan() {
    let mut plan = view::Camera::plan();
    plan.scale = 2.0;
    plan.target = [10.0, 20.0, 0.0];
    let fp = view::viewport_footprint(&plan, 100.0, 60.0);
    // ±25 in X, ±15 in Y about the target (10, 20), in closed-quad order.
    let want = [[-15.0, 5.0], [35.0, 5.0], [35.0, 35.0], [-15.0, 35.0]];
    for (got, want) in fp.iter().zip(want) {
        assert!((got[0] - want[0]).abs() < 1e-9 && (got[1] - want[1]).abs() < 1e-9);
    }

    let side = view::Camera { yaw: 0.0, pitch: 0.0, scale: 2.0, target: [0.0; 3] };
    let fp = view::viewport_footprint(&side, 100.0, 60.0);
    for k in 0..2 {
        // The two corners that differ only in screen-up share an XY: up is
        // world +Z edge-on, so the XY footprint is a segment along `right`.
        let (a, b) = (fp[k], fp[3 - k]);
        assert!((a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9);
    }
}

// The two pins below stood in this file until the heightfield's retirement
// swept them out with it. Neither was ever about the heightfield: one is the
// MINIMAP's indicator and one is the CAMERA's scale guards, and both subjects
// are untouched and still live. They are not in that slice's retirement ledger
// (below) because nothing decided to retire them — they were collateral, and
// the ledger is the proof: a pin whose reason for dying is unwritten did not
// have one. Restored 2026-08-14 against the code as it stands; both passed on
// the first run, so this closes a coverage hole rather than a defect.

/// The indicator is the footprint CLIPPED to the stock: a view holding the
/// whole stock draws the stock's own rectangle (the frame lying on the part
/// — since 2026-08-29; it used to vanish, which at fullscreen read as a
/// defect), a view aimed off the stock entirely draws nothing (never a stray
/// quad parked against one edge), and a cropped view draws a polygon that
/// never leaves the stock's own footprint. Sits on the stock's top face,
/// which is the sheet the map draws.
#[test]
fn test_viewport_indicator_clips_to_the_blank_and_frames_it_whole_when_it_holds_it() {
    let aabb = [0.0, 0.0, -20.0, 120.0, 80.0, 0.0];
    let mut cam = view::Camera::default_orbit();
    cam.target = [60.0, 40.0, 0.0];

    // Zoomed out far enough to hold the whole stock: the stock's own frame.
    cam.scale = 0.2;
    let whole = view::viewport_indicator(&cam, 800.0, 600.0, aabb).expect("an uncropped view frames the whole stock");
    let (mut minx, mut maxx, mut miny, mut maxy) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for pt in &whole {
        minx = minx.min(pt[0]);
        maxx = maxx.max(pt[0]);
        miny = miny.min(pt[1]);
        maxy = maxy.max(pt[1]);
    }
    assert!((minx - 0.0).abs() < 1e-9 && (maxx - 120.0).abs() < 1e-9, "spans the stock in X: {minx}..{maxx}");
    assert!((miny - 0.0).abs() < 1e-9 && (maxy - 80.0).abs() < 1e-9, "spans the stock in Y: {miny}..{maxy}");

    // Zoomed in: a real crop, clipped to the stock's own footprint.
    cam.scale = 20.0;
    let poly = view::viewport_indicator(&cam, 800.0, 600.0, aabb)
        .expect("a cropped view annotations its boundary");
    assert!(poly.len() >= 3, "a cropped region is a polygon, got {} points", poly.len());
    for pt in &poly {
        assert!(
            pt[0] >= aabb[0] - 1e-9 && pt[0] <= aabb[3] + 1e-9,
            "indicator left the stock in X: {pt:?}"
        );
        assert!(
            pt[1] >= aabb[1] - 1e-9 && pt[1] <= aabb[4] + 1e-9,
            "indicator left the stock in Y: {pt:?}"
        );
        assert_eq!(pt[2], aabb[5], "the indicator is drawn on the stock's top face");
    }

    // Looking somewhere else entirely: nothing to draw rather than a stray
    // quad parked against one edge.
    cam.target = [10_000.0, 10_000.0, 0.0];
    assert!(
        view::viewport_indicator(&cam, 800.0, 600.0, aabb).is_none(),
        "a view off the stock annotations nothing"
    );

    // The edge-on stance keeps its documented meaning: a segment, trimmed.
    let edge = view::Camera { yaw: 0.0, pitch: 0.0, scale: 20.0, target: [60.0, 40.0, 0.0] };
    if let Some(seg) = view::viewport_indicator(&edge, 800.0, 600.0, aabb) {
        for pt in &seg {
            assert!(pt[0] >= -1e-9 && pt[0] <= 120.0 + 1e-9, "segment left the stock: {pt:?}");
        }
    }
}

/// The camera's scale guards, which nothing else reaches: a zero-sided
/// viewport must not latch a zero scale, scrolling forever stops at
/// `SCALE_MIN`/`SCALE_MAX`, and a non-finite or non-positive zoom factor is
/// rejected rather than propagated. Each one poisons `project` — and through
/// it the minimap's four unprojections a frame — with a NaN that no later
/// stance recovers from, which is why the assertion is finiteness at the
/// stock's eight corners rather than any particular number.
#[test]
fn test_no_reachable_camera_projects_to_infinity() {
    let bb = [0.0, 0.0, -20.0, 100.0, 80.0, 0.0];
    let corners = |c: &view::Camera| {
        for k in 0..8 {
            let p = [
                bb[if k & 1 == 0 { 0 } else { 3 }],
                bb[if k & 2 == 0 { 1 } else { 4 }],
                bb[if k & 4 == 0 { 2 } else { 5 }],
            ];
            let (x, y, d) = c.project(p);
            assert!(x.is_finite() && y.is_finite() && d.is_finite(), "projected {p:?} to {x},{y},{d}");
        }
        // The minimap's viewport indicator: four unprojections a frame, the
        // path a zero scale poisons first.
        for w in view::viewport_footprint(c, 1200.0, 800.0) {
            assert!(w.iter().all(|v| v.is_finite()), "viewport footprint corner {w:?}");
        }
    };

    // A viewport with a zero side must not latch a zero scale.
    for (w, h) in [(0.0f32, 800.0f32), (1200.0, 0.0), (0.0, 0.0)] {
        let mut cam = view::Camera::default_orbit();
        cam.fit_to_aabb(bb, w, h);
        assert!(cam.scale > 0.0 && cam.scale.is_finite(), "{w}x{h} viewport left scale {}", cam.scale);
        corners(&cam);
    }

    // Scrolling forever in either direction stops at the bounds.
    for factor in [1.2f64, 1.0 / 1.2] {
        let mut cam = view::Camera::default_orbit();
        cam.fit_to_aabb(bb, 1200.0, 800.0);
        for _ in 0..4000 {
            cam.zoom_by(factor);
        }
        assert!(
            (view::SCALE_MIN..=view::SCALE_MAX).contains(&cam.scale),
            "runaway zoom left scale {}",
            cam.scale
        );
        corners(&cam);
    }

    // A garbage factor is ignored rather than propagated.
    let mut cam = view::Camera::default_orbit();
    let before = cam.scale;
    for factor in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
        cam.zoom_by(factor);
    }
    assert_eq!(cam.scale, before, "a non-finite or non-positive factor is rejected");
}

// ---- the sim view's pins, re-homed onto the isosurface ---------------------
//
// The heightfield retired here (DECISIONS.md → "the field is already
// undercut-capable…", slice 2), and its pins retired WITH IT — but only the
// ones that were about the heightfield. Each claim below survived because it
// was never about the representation: it was about the FIELD, and the field
// did not change. What died with the column walk is listed at the bottom, so
// nobody has to diff two files to find out whether coverage was lost.

/// Is there surface within `tol` of this (x, y, z)? The isosurface's answer
/// to "what stands here" — a set of vertices rather than one height per
/// column, which is exactly the generality the heightfield could not hold.
fn near(iso: &nacs::view::StockSurface, x: f64, y: f64, z: f64, tol: f64) -> bool {
    iso.verts.iter().any(|v| {
        (f64::from(v[0]) - x).abs() < tol
            && (f64::from(v[1]) - y).abs() < tol
            && (f64::from(v[2]) - z).abs() < tol
    })
}

/// Is there ANY surface in this column, at any depth?
fn column_has_surface(iso: &nacs::view::StockSurface, x: f64, y: f64, tol: f64) -> bool {
    iso.verts
        .iter()
        .any(|v| (f64::from(v[0]) - x).abs() < tol && (f64::from(v[1]) - y).abs() < tol)
}

/// The untouched stock reads its own face. A bore-wall finish leaves the far
/// corner alone, and the surface there IS the stock's top — the claim the
/// heightfield made per column, made here per vertex.
#[test]
fn test_the_untouched_corner_stands_at_the_blank_top() {
    let p = load("cylinder_bore.cam");
    let iso = nacs::view::stock_surface(&p, Some(100.0 / 64.0))
        .expect("both wall passes stamp exactly — engine comp and resolved control comp");
    assert!(near(&iso, 5.0, 5.0, 0.0, 2.0), "the untouched corner stands at the top");
    assert!(
        !near(&iso, 5.0, 5.0, -10.0, 1.0),
        "and nothing hangs in the middle of solid material"
    );
}

/// A cut clean THROUGH leaves no top face over it — the isosurface's form of
/// "a hole in the surface, drawn as one". The trench wall still extracts
/// (that is what a through cut looks like from the side); what must NOT
/// exist is stock standing at the stock's top where the tool went through.
#[test]
fn test_a_through_cut_leaves_no_top_face_over_it() {
    let src = "\
units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n\
tool T5 Endmill(dia=10.0)\n\
feature Bore CylinderBore(dia=40.0, extent=20.0, x=50.0, y=50.0)\n\
run Side(tool=T5, surface=Bore, stepover=5.0)\n";
    let p = parse::parse(src).unwrap();
    let iso = nacs::view::stock_surface(&p, Some(100.0 / 64.0)).unwrap();
    // (65, 50) is r=15 from the bore axis: the MIDDLE of the swept annulus,
    // where a dia-10 tool riding r=15 cuts the full 20 mm extent. That column
    // is void from top to bottom, so the strongest true statement is not
    // "no top face" but "no surface at all".
    assert!(
        !column_has_surface(&iso, 65.0, 50.0, 1.2),
        "the swept annulus is cut clean through — nothing stands in that column"
    );
    // The trench has WALLS, and they are where the declaration puts them: the
    // bore's own radius, standing through the depth the cut opened.
    assert!(
        near(&iso, 70.0, 50.0, -10.0, 1.5),
        "the trench's outer wall stands at the declared R=20"
    );
    assert!(
        near(&iso, 50.0, 42.0, 0.0, 2.0),
        "the core plug off the entry corridor still stands at the top"
    );
}

/// The exact-alignment family, the two of three that were never about the
/// column walk. A stock whose height is a half-integer multiple of the cell
/// used to draw a standing floor as a through-hole — the worst lie this view
/// can tell — and an untouched stock at the same alignment must read its own
/// face rather than air. The third case (a boundary ROW of quads surviving at a
/// half-integer footprint edge) retired with the quads: there is no row.
#[test]
fn test_the_isosurface_holds_at_exact_cell_alignments() {
    let cell = 100.0 / 64.0; // 1.5625
    let faced = "\
units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=19.53125, x=0, y=0, z=0)\n\
tool T5 Endmill(dia=10.0)\n\
feature Top Plane(z=-18.4375)\n\
run End(tool=T5, surface=Top, stepover=6.0)\n";
    let p = parse::parse(faced).unwrap();
    let iso = nacs::view::stock_surface(&p, Some(cell)).unwrap();
    assert!(!iso.verts.is_empty(), "a faced stock with a floor standing is not a hole");
    assert!(
        near(&iso, 50.0, 40.0, -18.4375, cell),
        "the faced floor stands near its own z, and is not drawn through"
    );

    let stock = "\
units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=19.53125, x=0, y=0, z=0)\n\
tool T5 Endmill(dia=10.0)\n\
feature Top Plane(z=-1.0)\n";
    let p = parse::parse(stock).unwrap();
    let iso = nacs::view::stock_surface(&p, Some(cell)).unwrap();
    assert!(
        near(&iso, 50.0, 40.0, 0.0, cell),
        "an untouched column IS the stock's own face, at an exact alignment"
    );
}

/// The stock has SIDES, and they are extracted rather than synthesized. The
/// heightfield hung walls from the boundary because a lid alone reads as paper
/// laid over the wireframe; the isosurface has no lid and no boundary — the sides
/// are just where the field changes sign, like every other surface.
#[test]
fn test_the_blank_has_four_walls_reaching_its_bottom() {
    let src = "\
units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\n\
tool T5 Endmill(dia=10.0)\n\
feature Top Plane(z=-1.0)\n";
    let p = parse::parse(src).unwrap();
    let iso = nacs::view::stock_surface(&p, Some(100.0 / 64.0)).unwrap();
    // Every face walled, and the walls reach the bottom rather than stopping
    // at the boundary — the boundary shell's job, pinned from the view's side.
    for (axis, v) in [(0usize, 0.0f64), (0, 100.0), (1, 0.0), (1, 80.0)] {
        let on_face: Vec<f64> = iso
            .verts
            .iter()
            .filter(|p| (f64::from(p[axis]) - v).abs() < 1.5)
            .map(|p| f64::from(p[2]))
            .collect();
        assert!(!on_face.is_empty(), "the face at axis {axis} = {v} has no surface at all");
        let lo = on_face.iter().fold(f64::MAX, |m, z| m.min(*z));
        let hi = on_face.iter().fold(f64::MIN, |m, z| m.max(*z));
        assert!(hi > -1.5, "the wall at axis {axis} = {v} never reaches the top (max {hi})");
        assert!(lo < -18.5, "the wall at axis {axis} = {v} never reaches the bottom (min {lo})");
    }
}

/// The two errors that are about the FIELD's trustworthiness, unchanged by
/// the representation: an incomplete sweep over-states material and a viewport
/// of an over-statement shows stock that is not there; a stock thinner than a
/// cell holds no sample to read, and must decline by name rather than draw
/// itself away.
#[test]
fn test_sim_iso_declines_what_the_field_cannot_support() {
    let thin = "\
units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=1.5, x=0, y=0, z=0)\n\
tool T5 Endmill(dia=10.0)\n\
feature Top Plane(z=-0.5)\n\
run End(tool=T5, surface=Top, stepover=6.0)\n";
    let p = parse::parse(thin).unwrap();
    let err = nacs::view::stock_surface(&p, Some(100.0 / 32.0))
        .expect_err("a sub-cell stock must decline, not draw itself away");
    assert!(err.contains("thinner than"), "the error must say why: {err}");

    let src = "\
units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\n\
tool T5 Endmill(dia=10.0)\n\
feature Top Plane(z=-3.0)\n\
run End(tool=T5, surface=Top, stepover=6.0)\n";
    let mut p = parse::parse(src).unwrap();
    p.tools.get_mut(&5).unwrap().profile = Some(vec![
        parse::Pt { x: 0.0, y: 0.0 },
        parse::Pt { x: 5.0, y: 4.0 },
        parse::Pt { x: 2.0, y: 8.0 },
    ]);
    let err = nacs::view::stock_surface(&p, Some(100.0 / 32.0))
        .expect_err("a necked silhouette annotations the field incomplete");
    assert!(err.contains("incomplete"), "the error must say why: {err}");
}

/// THE PIN THAT INVERTED. A truncated (form-cutter) silhouette leaves a shank
/// column standing above a void, which broke the heightfield lemma and made
/// the old view decline by name. The isosurface has no such lemma to break,
/// so the same fixture must now DRAW — this is the whole thread in one
/// assertion, and it is written as an acceptance so that a regression to the
/// old error fails loudly rather than quietly returning to a stock panel.
#[test]
fn test_sim_iso_draws_the_truncated_silhouette_the_heightfield_refused() {
    let p = load("sphere_formtool.cam");
    let iso = nacs::view::stock_surface(&p, Some(100.0 / 32.0))
        .expect("the form-cutter field draws now — the upward-closure error is gone");
    assert!(iso.verts.len() > 1000, "the form-cutter surface has real geometry");
    assert!(iso.radius > 0.0 && iso.center.iter().all(|c| c.is_finite()));
}

/// Shading separates floor from wall and never goes black. The LAW now lives
/// in the sim pipeline's WGSL and cannot be called from here — so what is
/// pinned is the shared input the shader reads (`SIM_LIGHT`, `SIM_AMBIENT`),
/// with the lambert restated in the test. That is a weaker pin than calling
/// the function was, and it is named as one: the shader is shell, and shell
/// is untested by the same rule that leaves the rest of `frontend.rs` alone.
#[test]
fn test_the_shared_light_separates_floor_from_wall() {
    let k = |n: [f64; 3]| {
        let l = view::SIM_LIGHT;
        let ln = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
        let lam = ((n[0] * l[0] + n[1] * l[1] + n[2] * l[2]) / ln).max(0.0);
        view::SIM_AMBIENT + (1.0 - view::SIM_AMBIENT) * lam
    };
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let (flat, slope, wall) = (k([0.0, 0.0, 1.0]), k([s, 0.0, s]), k([1.0, 0.0, 0.0]));
    assert!(flat > slope, "a floor outshines a 45-degree flank");
    assert!(slope > wall, "a flank outshines a vertical wall");
    assert!(wall > 0.0, "the ambient floor keeps walls legible");
    assert!(flat <= 1.0, "nothing blows past full brightness");
}

// RETIRED WITH THE HEIGHTFIELD, and why — so a reader can tell deleted
// coverage from deleted representation:
//
//   test_sim_surface_steps_the_fields_own_treads_not_a_beat (the sea-urchin
//     pin) — it held the DRAWING GRID to the field's own cell stride, because
//     a grid off that tread walked the staircase in a beat and fringed a
//     round wall with teeth. There is no drawing grid any more: extraction
//     visits field cells, so the beat it guarded against is unrepresentable
//     rather than merely absent.
//
//   test_depth_sorted_draws_far_before_near — painter's-algorithm ordering,
//     replaced by a depth buffer. A depth test is not a sort, and there is
//     no order left to get wrong.
//
//   the boundary-row case of the exact-alignment family — it held that the max
//     face's own ROW OF QUADS survived a half-integer footprint edge. There
//     are no quads and no rows; the surviving two cases of that family are
//     above.

// ---- the click's hit test pins (Phase 6, slice 5) ---------------------------

/// The pick is by projected screen distance: a click ON an edge picks its
/// feature, a click within tolerance still picks it, a click in empty
/// space picks nothing — pinned at the plan stance, where every screen
/// coordinate is hand-checkable.
#[test]
fn test_pick_feature_selects_by_screen_distance() {
    let src = "\
units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\n\
tool T5 Endmill(dia=10.0)\n\
feature Hole CylinderBore(dia=20.0, extent=8.0, x=30.0, y=40.0)\n\
feature Post CylinderBoss(dia=20.0, extent=8.0, x=70.0, y=40.0, z=-8.0)\n";
    let p = parse::parse(src).unwrap();
    let edges = view::feature_edges(&p);
    assert_eq!(edges.len(), 2, "name-sorted: Hole then Post");
    let cam = view::Camera::plan_fit([0.0, 0.0, -20.0, 100.0, 80.0, 0.0], 400.0, 320.0);

    // The plan camera centers (50, 40) at scale 3.6: the bore's boundary point
    // (40, 40) projects to (-36, 0); the boss's boundary point (80, 40) to
    // (+108, 0). A click ON each boundary picks each feature.
    let on_hole = view::pick_feature(&edges, &cam, -36.0, 0.0, 8.0);
    assert_eq!(on_hole.map(|i| edges[i].name.as_str()), Some("Hole"));
    let on_post = view::pick_feature(&edges, &cam, 108.0, 0.0, 8.0);
    assert_eq!(on_post.map(|i| edges[i].name.as_str()), Some("Post"));

    // Within tolerance still picks; well past it (24 screen units from the
    // nearest boundary), nothing; dead center of the bore (36 units from its own
    // boundary) is empty space, not a hit.
    assert!(view::pick_feature(&edges, &cam, -42.0, 0.0, 8.0).is_some());
    assert_eq!(view::pick_feature(&edges, &cam, -70.0, 60.0, 8.0), None);
    assert_eq!(view::pick_feature(&edges, &cam, 0.0, 0.0, 8.0), None);
}

/// Role colors: total over the enum (the match in `Theme::role_color` is
/// exhaustive by construction — this pins distinctness), and the reserved
/// colors stay distinct from every role's.
#[test]
fn test_role_colors_are_distinct() {
    let t = Theme::default();
    let all = [
        t.role_color(OpRole::Roughing),
        t.role_color(OpRole::Finishing),
        t.role_color(OpRole::Untagged),
        t.rapid,
        t.select,
    ];
    for a in 0..all.len() {
        for b in (a + 1)..all.len() {
            assert_ne!(all[a], all[b], "colors {a} and {b} collide");
        }
    }
}

/// The shell constructs no colour of its own (Phase 12, slice 0's
/// source-scan, the comp-register staleness test's pattern): every colour
/// literal is a `Theme` slot, so a `Color32` in `frontend.rs` may only be
/// built from a slot's bytes — never from numbers. The scan allows a
/// numeric ALPHA (translucency is a presentation number, and it rides the
/// `rgba` helper's second argument) by checking only each constructor's
/// FIRST argument; `theme.rs` itself is the one home the numbers live in.
#[test]
fn test_the_shell_constructs_no_colour_of_its_own() {
    let src = std::fs::read_to_string("src/frontend.rs").expect("read src/frontend.rs");
    for ctor in [
        "from_rgb(",
        "from_rgba_unmultiplied(",
        "from_rgba_premultiplied(",
        "from_gray(",
        "from_black_alpha(",
        "from_white_alpha(",
        "from_additive_luminance(",
    ] {
        for (i, _) in src.match_indices(ctor) {
            let after = src[i + ctor.len()..].trim_start();
            let first = after.chars().next().unwrap_or(' ');
            assert!(
                !first.is_ascii_digit(),
                "frontend.rs builds a colour from a numeric literal at byte {i} ({ctor}…): \
                 the colour belongs in a theme slot"
            );
        }
    }
    // And the constructor family named `Color32::<NAME>` (RED, GOLD, …) —
    // a palette by another door.
    for (i, _) in src.match_indices("Color32::") {
        let name: String = src[i + "Color32::".len()..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        assert!(
            !name.chars().all(|c| c.is_ascii_uppercase() || c == '_') || name.is_empty(),
            "frontend.rs names a stock colour (Color32::{name}): the colour belongs in a theme slot"
        );
    }
}


// ---- what the PAINTER is asked for: retired with the painter ---------------
//
// This is where the sliver pin stood. It existed because every heightfield
// triangle was submitted as its own STROKED `convex_polygon`, and a
// heightfield projects near-degenerate slivers at any camera — stroking a
// closed path with a 180-degree hairpin sent the miter join to infinity, and
// measured vertices 29,784 px from the centre of an 875 px viewport.
//
// The isosurface slice removed the hazard rather than tested around it: the
// sim surface is drawn by a wgpu pipeline (DECISIONS.md → "the field is
// already undercut-capable…", slice 1), which has no strokes to blow up, no
// per-shape feathering to seam, and no painter's order to sort. There is
// nothing left for that pin to catch.
//
// What the pin was REALLY checking — that the geometry handed to the renderer
// is finite, in bounds, and correctly indexed at every stance — did not
// retire with it. It moved to where the geometry is now made:
// `tests/vox_tests.rs` holds the extraction (finite vertices, unit normals,
// watertight, outward winding), and `test_the_clip_matrix_is_the_lens` above
// holds the projection that places it. The pin was not dropped; it was
// re-homed, which is the same rule the sliver pin itself established —
// a test belongs where the decision is made.

// ---- which GPU driver the shell NAMES (the software-rendering pin) ----------
//
// The same rule as the sliver pin above, applied one layer down: the shell is
// untested for what it SHOWS, but what it DECIDES gets a test. Mesa 25 stopped
// probing `d3d12` on its own, and on WSL — GPU on `/dev/dxg`, no DRM node to
// match — its fallback is `llvmpipe`, every frame on the CPU, announced
// nowhere. Measured on the box that found it: 20k line segments at 2544x1384
// ran 220.73ms (4.5 fps) software against 17.80ms (56 fps) on the GPU, while
// an EMPTY window ran faster in software (13.84ms against 12.59ms) and would
// have blessed the wrong configuration. The decision is cheap; the mistake is
// not, and it is invisible — so it is pinned.

/// The machine the trap is set on: WSL's GPU node, no DRM node, nobody having
/// said otherwise. This is the only shape that gets a driver named.
#[cfg(feature = "frontend")]
#[test]
fn test_wsl_without_a_drm_node_names_d3d12() {
    assert_eq!(
        nacs::frontend::gpu_driver_default(None, None, true, false),
        Some("d3d12"),
        "WSL's GPU node with no DRM node is the llvmpipe trap — name the driver"
    );
}

/// A default, not a policy: a named driver is a choice, and choices stand.
/// Both spellings Mesa honors are honored here.
#[cfg(feature = "frontend")]
#[test]
fn test_a_named_driver_is_left_alone() {
    for (gallium, loader) in
        [(Some("zink"), None), (None, Some("zink")), (Some("llvmpipe"), None), (Some("d3d12"), None)]
    {
        assert_eq!(
            nacs::frontend::gpu_driver_default(gallium, loader, true, false),
            None,
            "GALLIUM_DRIVER={gallium:?} MESA_LOADER_DRIVER_OVERRIDE={loader:?} was named on purpose"
        );
    }
}

/// An empty value is an accident, not an intent — Mesa ignores it, so the
/// decision does too, rather than letting a stray `export` buy software
/// rendering.
#[cfg(feature = "frontend")]
#[test]
fn test_an_empty_driver_variable_is_not_a_choice() {
    assert_eq!(
        nacs::frontend::gpu_driver_default(Some(""), None, true, false),
        Some("d3d12")
    );
    assert_eq!(
        nacs::frontend::gpu_driver_default(Some("  "), Some(""), true, false),
        Some("d3d12")
    );
}

/// Every other machine keeps Mesa's own choice. A DRM node is an ordinary
/// Linux GPU stack the loader matches without help; no WSL node at all is not
/// this machine. Naming `d3d12` on either would be this shell inventing a
/// fact about hardware it cannot see.
#[cfg(feature = "frontend")]
#[test]
fn test_every_other_machine_keeps_mesas_choice() {
    // Native Linux: DRM node present.
    assert_eq!(nacs::frontend::gpu_driver_default(None, None, false, true), None);
    // WSL with a DRM node to match — the loader needs no help.
    assert_eq!(nacs::frontend::gpu_driver_default(None, None, true, true), None);
    // Neither node: not a machine this decision knows anything about.
    assert_eq!(nacs::frontend::gpu_driver_default(None, None, false, false), None);
}

// ---- the lens is the camera (the hoist's pin) -------------------------------
//
// `Camera::lens()` exists because `project` is a pure function of the stance
// and the point, so a loop over the sim surface recomputed yaw.sin_cos() and
// pitch.sin_cos() 265k times a frame to produce four numbers. Taking them once
// is a HOIST, not a second projection — and the only thing that keeps it one
// is this pin, because a drifted lens would not look wrong, it would look
// slightly rotated, which is exactly the class of error a viewport cannot
// report on itself.

/// Bitwise, not approximately: the hoisted arithmetic is the same sequence of
/// operations in the same order, so it must produce the same bits. An epsilon
/// here would let a genuine reordering pass while claiming the projection has
/// one home.
#[test]
fn test_the_lens_is_the_camera() {
    let stances = [
        (0.0, 0.0),
        (std::f64::consts::FRAC_PI_4, 0.615),
        (-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2),
        (2.59, -0.55),
        (4.07, 0.21),
        (1.85, 0.78),
        (-3.33, 1.35),
    ];
    let points = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [50.0, -25.0, 12.5],
        [-1234.5, 678.9, -0.001],
        [1e5, 1e-5, -1e3],
    ];
    for (yaw, pitch) in stances {
        for scale in [1.0, 0.001, 1000.0, 12.75] {
            for target in [[0.0; 3], [10.0, -20.0, 30.0]] {
                let cam = view::Camera { yaw, pitch, scale, target };
                let lens = cam.lens();
                for p in points {
                    assert_eq!(
                        cam.project(p),
                        lens.project(p),
                        "lens drifted from the camera at yaw {yaw} pitch {pitch} scale {scale} point {p:?}"
                    );
                }
            }
        }
    }
}

/// One lens, many points: the whole reason the hoist exists is that a caller
/// holds it across a loop, so the pin holds it across one too — a lens taken
/// before the loop must still answer for every point in it.
#[test]
fn test_one_lens_answers_for_a_whole_loop() {
    let cam = view::Camera { yaw: 1.11, pitch: -1.12, scale: 3.5, target: [1.0, 2.0, 3.0] };
    let lens = cam.lens();
    for i in 0..500 {
        let t = f64::from(i);
        let p = [t * 0.37 - 90.0, (t * 0.11).sin() * 40.0, (t * 0.05).cos() * 15.0];
        assert_eq!(cam.project(p), lens.project(p), "lens disagreed at point {i}");
    }
}

// ---- the clip matrix is the lens (slice 1's pin) ----------------------------
//
// The GPU pipeline needs the projection as sixteen floats so the mesh can be
// uploaded once and only the camera move. That is a restatement of the linear
// algebra — the trigonometry is still the lens's, taken once — and a
// restatement is exactly the kind of thing that drifts silently: a matrix
// slightly out of step with `project` does not draw a wrong viewport, it draws
// a viewport whose TOOLPATHS (still painted through `project`) no longer line
// up with the stock, by an amount too small to notice and too large to trust.
//
// Unlike the lens pin, this one carries an epsilon rather than demanding
// bitwise equality, and the difference is honest: the matrix folds the target
// into a translation column instead of subtracting it per point, so the
// operations genuinely differ in order and f32 rounding follows a different
// path. The tolerance is tight enough that a real algebraic error cannot hide
// under it.

/// For every stance, the matrix's normalized device coordinates are the
/// lens's screen coordinates divided by the half-viewport, and its depth is
/// the lens's depth mapped into [0, 1] about the sphere.
#[test]
fn test_the_clip_matrix_is_the_lens() {
    let (w, h, radius) = (1280.0f64, 720.0f64, 90.0f64);
    let stances = [
        (0.0, 0.0),
        (std::f64::consts::FRAC_PI_4, 0.615),
        (-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2),
        (2.59, -0.55),
        (4.07, 0.21),
        (-3.33, 1.35),
    ];
    let points = [
        [0.0, 0.0, 0.0],
        [50.0, -25.0, 12.5],
        [-40.0, 60.0, -20.0],
        [12.75, 12.75, 12.75],
        [-88.0, -3.5, 41.0],
    ];
    for (yaw, pitch) in stances {
        for scale in [1.0, 3.5, 0.25] {
            for target in [[0.0; 3], [10.0, -20.0, 30.0]] {
                let cam = view::Camera { yaw, pitch, scale, target };
                let m = cam.lens().clip_matrix(w, h, radius);
                for p in points {
                    let (sx, sy, depth) = cam.lens().project(p);
                    // Column-major: clip = M * (p, 1).
                    let col = |c: usize| [m[c * 4], m[c * 4 + 1], m[c * 4 + 2]];
                    let mut got = [0f64; 3];
                    for r in 0..3 {
                        got[r] = f64::from(col(0)[r]) * p[0]
                            + f64::from(col(1)[r]) * p[1]
                            + f64::from(col(2)[r]) * p[2]
                            + f64::from(col(3)[r]);
                    }
                    let want = [
                        f64::from(sx) / (w * 0.5),
                        f64::from(sy) / (h * 0.5),
                        0.5 + f64::from(depth) / (2.0 * radius * view::DEPTH_MARGIN),
                    ];
                    for a in 0..3 {
                        assert!(
                            (got[a] - want[a]).abs() < 1e-6,
                            "axis {a}: matrix {} vs lens {} at yaw {yaw} pitch {pitch} scale {scale} point {p:?}",
                            got[a],
                            want[a]
                        );
                    }
                }
            }
        }
    }
}

/// The depth range is the whole point of passing a radius: everything inside
/// the sphere must land in [0, 1], at every stance, or the depth test starts
/// clipping the stock it exists to order.
#[test]
fn test_the_clip_matrix_keeps_the_sphere_inside_the_depth_range() {
    let (w, h, radius) = (900.0f64, 900.0f64, 60.0f64);
    let center = [5.0, -7.0, 3.0];
    for (yaw, pitch) in [(0.0, 0.0), (1.11, -1.12), (2.59, 0.9), (5.4, 1.4)] {
        let cam = view::Camera { yaw, pitch, scale: 2.0, target: center };
        let m = cam.lens().clip_matrix(w, h, radius);
        // The sphere's extremes along each axis, plus its corners.
        for dx in [-1.0f64, 0.0, 1.0] {
            for dy in [-1.0f64, 0.0, 1.0] {
                for dz in [-1.0f64, 0.0, 1.0] {
                    let n = (dx * dx + dy * dy + dz * dz).sqrt();
                    if n == 0.0 {
                        continue;
                    }
                    let p = [
                        center[0] + radius * dx / n,
                        center[1] + radius * dy / n,
                        center[2] + radius * dz / n,
                    ];
                    let z = f64::from(m[2]) * p[0]
                        + f64::from(m[6]) * p[1]
                        + f64::from(m[10]) * p[2]
                        + f64::from(m[14]);
                    // Strictly inside, with the margin's room to spare: a
                    // vertex that merely touches the boundary is one f32
                    // rounding away from being clipped out of existence.
                    assert!(
                        z > 0.001 && z < 0.999,
                        "depth {z} reached the edge of [0,1] at yaw {yaw} pitch {pitch}"
                    );
                }
            }
        }
    }
}

// ---- the ground and the annotations (Phase 11, slice 0) ----------------------------

/// Every colour the window draws is chosen against one dark ground, and
/// every one is a `Theme` slot now (Phase 12, slice 0). Pinned as a
/// legibility fact, quantified over the WHOLE meaning table through
/// `meaning_slots` — a slot added to the struct joins this pin by joining
/// that list: each drawn colour is brighter than the ground it sits on by a
/// margin an eye can use, the map's ground is a step above the viewport's
/// (a sheet on the desk, not a hole in it), and the indicator is brighter
/// than both. The chrome half reads against its own panel the same way.
/// This is the built-in palette passing the floor slice 3's validator
/// holds every theme to.
#[test]
fn test_every_drawn_colour_reads_against_the_ground() {
    let t = Theme::default();
    // The law is a contrast RATIO now (2026-09-17, Phase 23 slice 4):
    // WCAG's, unsigned — a stroke at 3:1, text and the families at 4.5:1.
    for (name, c) in t.meaning_slots() {
        if name == "ground" || name == "map.ground" {
            continue;
        }
        let floor = if name.starts_with("family.") || name == "refusal" { theme::TEXT_CONTRAST_FLOOR } else { theme::STROKE_CONTRAST_FLOOR };
        assert!(theme::contrast(c, t.ground) >= floor, "{name} {c:?} must read against the ground {:?}: {:.2}:1", t.ground, theme::contrast(c, t.ground));
    }
    let (g, m, w) = (theme::relative_luminance(t.ground), theme::relative_luminance(t.map_ground), theme::relative_luminance(t.wire));
    assert!(m > g && m < w, "the map's ground sits between the viewport's and the quietest wire");
    let (ind, alpha) = t.indicator;
    assert!(theme::relative_luminance(ind) > theme::relative_luminance(t.select) * 0.9 && alpha > 128, "the indicator is light and mostly opaque");
    // The chrome: text against the panel it sits on, the border and the
    // scrollbar visible against it, the field's text on the field.
    for (name, c) in [("text", t.text), ("text.interactive", t.text_interactive), ("text.hover", t.text_hover), ("text.active", t.text_active), ("text.open", t.text_open)] {
        assert!(theme::contrast(c, t.panel_background) >= theme::TEXT_CONTRAST_FLOOR, "{name} {c:?} must read against the panel");
    }
    assert!(theme::contrast(t.border, t.panel_background) >= theme::STROKE_CONTRAST_FLOOR, "the border is visible against the panel");
    assert!(theme::contrast(t.scrollbar, t.panel_background) >= theme::STROKE_CONTRAST_FLOOR);
    assert!(theme::contrast(t.text_interactive, t.text_field) >= theme::TEXT_CONTRAST_FLOOR);
}

/// The contrast arithmetic is WCAG 2's: black on white is 21:1, a colour
/// against itself 1:1, either way round the same; the review's numbers
/// for the old dark theme reproduce (its `stock` at 1.96:1, its `wire`
/// at 2.37:1) — the measurement that drove the retune.
#[test]
fn test_the_contrast_ratio_is_wcag_and_unsigned() {
    assert!((theme::contrast([0, 0, 0], [255, 255, 255]) - 21.0).abs() < 1e-9);
    assert!((theme::contrast([255, 255, 255], [0, 0, 0]) - 21.0).abs() < 1e-9);
    assert!((theme::contrast([90, 90, 90], [90, 90, 90]) - 1.0).abs() < 1e-12);
    assert!((theme::relative_luminance([255, 255, 255]) - 1.0).abs() < 1e-9);
    assert_eq!(theme::relative_luminance([0, 0, 0]), 0.0);
    let old_ground = [24, 26, 30];
    assert!((theme::contrast([70, 74, 82], old_ground) - 1.96).abs() < 0.01, "the old stock");
    assert!((theme::contrast([82, 86, 94], old_ground) - 2.37).abs() < 0.01, "the old wire");
}

/// A light theme is legal now: the old signed delta failed every meaning
/// slot on a light ground at once and the policy laid the dark palette
/// over the light chrome. The same colours mirrored pass, and the
/// appearance rides with the chrome through the policy.
#[test]
fn test_a_light_ground_is_as_legal_as_a_dark_one() {
    let mut t = Theme::default();
    // A light palette by hand — mirroring the built-in does NOT keep the
    // ratios (the sRGB transfer is not symmetric about mid-grey), which is
    // itself a reason the law is a measured ratio and not a delta.
    let slots: &[(&str, [u8; 3])] = &[
        ("ground", [240, 240, 240]), ("map.ground", [226, 226, 226]),
        ("role.roughing", [150, 75, 0]), ("role.finishing", [0, 80, 170]), ("role.untagged", [60, 60, 70]),
        ("rapid", [120, 120, 130]), ("select", [130, 95, 0]), ("wire", [105, 105, 115]), ("stock", [95, 95, 105]),
        ("indicator", [30, 30, 40]), ("sim.base", [85, 85, 95]), ("removal", [0, 115, 45]),
        ("forgotten", [185, 30, 30]), ("violated", [160, 0, 130]), ("refusal", [190, 0, 0]),
        ("tool", [105, 105, 0]), ("fixture", [125, 75, 30]),
        ("family.engagement", [0, 95, 155]), ("family.deviation", [125, 75, 0]), ("family.entry", [175, 0, 65]),
        ("family.fixture", [115, 0, 155]), ("family.reach", [75, 65, 165]), ("family.envelope", [0, 85, 185]),
        ("family.containment", [0, 105, 75]), ("family.body_foul", [155, 45, 105]), ("family.field_body_foul", [95, 95, 0]),
        ("family.part_violation", [0, 115, 125]), ("family.decline", [85, 95, 105]),
        ("background", [250, 250, 250]), ("panel.background", [250, 250, 250]), ("border", [135, 135, 135]), ("border.hover", [80, 80, 80]),
        ("text", [60, 60, 60]), ("text.interactive", [40, 40, 40]), ("text.hover", [20, 20, 20]), ("text.active", [0, 0, 0]), ("text.open", [30, 30, 30]),
        ("element", [225, 225, 225]), ("element.hover", [215, 215, 215]), ("element.active", [205, 205, 205]), ("element.open", [235, 235, 235]), ("element.selected", [180, 205, 235]),
        ("text.field", [255, 255, 255]), ("scrollbar", [130, 130, 130]),
    ];
    for (n, c) in slots {
        assert!(t.set_slot(n, *c, None), "{n}");
    }
    t.appearance = theme::Appearance::Light;
    let findings = theme::validate(&t);
    let meaning: Vec<_> = findings.iter().filter(|f| f.grade == theme::ThemeGrade::Meaning).collect();
    assert!(meaning.is_empty(), "a light palette is legal — the old signed law failed all 26 slots at once: {meaning:?}");
    let (active, _) = theme::apply_policy(t.clone());
    assert_eq!(active, t, "worn whole");
    assert_eq!(active.appearance, theme::Appearance::Light);
    // And a rejected light theme keeps its appearance with its chrome.
    let mut bad = t.clone();
    bad.set_slot("role.roughing", bad.role_finishing, None);
    let (active, findings) = theme::apply_policy(bad);
    assert!(findings.iter().any(|f| f.grade == theme::ThemeGrade::Meaning));
    assert_eq!(active.appearance, theme::Appearance::Light);
    assert_eq!(active.role_roughing, Theme::default().role_roughing);
    assert_eq!(active.text, [60, 60, 60], "the light chrome stays");
}

/// The stock's other two voices against the roles: a forgotten cell and
/// a roughing pass in the same amber was the review's finding (ΔE 7.7);
/// the built-in clears the role floor now, and a theme that does not is
/// refused by name.
#[test]
fn test_forgotten_and_violated_are_not_a_roles_colour() {
    let t = Theme::default();
    for c in [t.forgotten, t.violated] {
        for r in [t.role_roughing, t.role_finishing, t.role_untagged] {
            assert!(theme::delta_e(c, r, None) >= theme::ROLE_DELTA_E_FLOOR, "{c:?} vs {r:?}: {:.1}", theme::delta_e(c, r, None));
        }
    }
    let mut bad = Theme::default();
    bad.set_slot("forgotten", bad.role_roughing, None);
    let findings = theme::validate(&bad);
    assert!(findings.iter().any(|f| f.grade == theme::ThemeGrade::Meaning && f.message.contains("`forgotten`") && f.message.contains("role.roughing")), "{findings:?}");
}

/// The eleven families under each eye are ADVISORY: the built-in has
/// close pairs under a dichromacy (measured: eleven hues cannot be
/// pairwise ΔE 18 under one), each named with its eye as a chrome note,
/// none a meaning violation — the row's word tells them apart.
#[test]
fn test_family_confusability_is_a_named_advisory_not_a_rejection() {
    let findings = theme::validate(&Theme::default());
    let family_notes: Vec<_> = findings.iter().filter(|f| f.message.contains("are close under")).collect();
    assert!(!family_notes.is_empty(), "the built-in has close family pairs under a dichromacy — measured");
    assert!(family_notes.iter().all(|f| f.grade == theme::ThemeGrade::Chrome));
    assert!(family_notes.iter().any(|f| f.message.contains("deuteranopia") && f.message.contains("family.reach") && f.message.contains("family.envelope")), "the review's pair: {family_notes:?}");
    assert!(findings.iter().all(|f| f.grade != theme::ThemeGrade::Meaning), "the built-in is still legal: {findings:?}");
}

/// The two new chrome slots and the appearance word: read from a Zed
/// file's own keys, defaulting to the built-in, never a meaning slot.
#[cfg(feature = "frontend")]
#[test]
fn test_the_text_field_the_scrollbar_and_the_appearance_are_read() {
    let text = r##"{"themes":[{"name":"x","appearance":"light","style":{"editor.background":"#ffffff","scrollbar.thumb.background":"#808080","panel.background":"#f0f0f0","text":"#202020"}}]}"##;
    let t = theme::theme_from_json(text).unwrap();
    assert_eq!(t.text_field, [255, 255, 255]);
    assert_eq!(t.scrollbar, [128, 128, 128]);
    assert_eq!(t.appearance, theme::Appearance::Light);
    assert!(t.chrome_slots().iter().any(|(n, _)| *n == "text.field") && t.chrome_slots().iter().any(|(n, _)| *n == "scrollbar"));
    assert!(t.meaning_slots().iter().all(|(n, _)| *n != "text.field" && *n != "scrollbar"));
    let dark = theme::theme_from_json(r##"{"themes":[{"name":"y","style":{}}]}"##).unwrap();
    assert_eq!((dark.appearance, dark.text_field, dark.scrollbar), (theme::Appearance::Dark, Theme::default().text_field, Theme::default().scrollbar));
}

/// Phase 59 slice 4 (forks 3A + 4A): `sh <line>` hands the line WHOLE to
/// the shell — its own spacing, quotes and pipe kept, one argument to the
/// shell's line flag — the system's shell by default, the workspace's
/// `shell <program>` when it names one, the log told which; an empty line
/// is refused by name, and an unknown first word is still refused: a
/// verb, not a fallback.
#[test]
fn test_sh_hands_the_line_whole_to_the_shell_and_the_shell_word_names_another() {
    let dir = std::path::Path::new("plate.cam");
    let nacs = std::path::Path::new("nacs.exe");
    let s = view::runner_command("sh  echo  \"a  b\" | sort ", dir, nacs, None, None).unwrap().unwrap();
    let (system, flag) = view::shell_program(None);
    assert_eq!(s.program, system);
    assert_eq!(s.args, vec![flag.to_string(), "echo  \"a  b\" | sort".to_string()], "the line after the verb, whole");
    assert_eq!(s.echo, "sh echo  \"a  b\" | sort", "the echo is the line as typed");
    assert_eq!(s.cwd, dir);
    assert_eq!(s.via.as_deref(), Some(format!("via {} {flag}", system.display()).as_str()), "the shell named under the echo");
    if cfg!(windows) {
        assert_eq!(flag, "/C");
        assert!(system.to_string_lossy().to_ascii_lowercase().ends_with("cmd.exe"), "{}", system.display());
    } else {
        assert_eq!((system.as_path(), flag), (std::path::Path::new("/bin/sh"), "-c"));
    }
    let b = view::runner_command("sh ls -la", dir, nacs, None, Some("bash")).unwrap().unwrap();
    assert_eq!((b.program.as_path(), b.args.clone()), (std::path::Path::new("bash"), vec!["-c".to_string(), "ls -la".to_string()]), "a declared shell, -c");
    assert_eq!(b.via.as_deref(), Some("via bash -c"));
    let c = view::runner_command("sh dir", dir, nacs, None, Some(if cfg!(windows) { "C:\\Windows\\System32\\cmd.exe" } else { "/mnt/c/Windows/System32/cmd.exe" })).unwrap().unwrap();
    assert_eq!(c.args[0], "/C", "cmd by any path takes /C");
    assert!(view::runner_command("sh", dir, nacs, None, None).unwrap_err().contains("sh takes a line"));
    assert!(view::runner_command("sh   ", dir, nacs, None, None).unwrap_err().contains("sh takes a line"));
    assert!(view::runner_command("echo hi", dir, nacs, None, None).unwrap_err().contains("`echo` is not a verb the runner knows"), "a verb, not a fallback");
    // The word in the workspace file: written when set, read back, absent otherwise.
    let mut ws = theme::Workspace::default();
    assert!(!ws.render().contains("shell"));
    ws.shell = Some("pwsh".to_string());
    assert!(ws.render().contains("shell pwsh\n"));
    assert_eq!(theme::Workspace::parse(&ws.render()), ws, "parse(render(x)) == x");
    assert_eq!(theme::Workspace::parse("shell  bash \n").shell.as_deref(), Some("bash"));
}

/// `ui.zoom` in the workspace file: written when set, read back
/// clamped to egui's band, absent when nothing was said.
#[test]
fn test_the_workspace_file_carries_the_zoom() {
    let mut ws = theme::Workspace::default();
    assert!(!ws.render().contains("ui.zoom"));
    ws.zoom = Some(1.25);
    assert!(ws.render().ends_with("ui.zoom 1.25\n"));
    assert_eq!(theme::Workspace::parse(&ws.render()), ws, "parse(render(x)) == x");
    assert_eq!(theme::Workspace::parse("ui.zoom 40\n").zoom, Some(theme::ZOOM_MAX), "a hand-typed 40 is clamped");
    assert_eq!(theme::Workspace::parse("ui.zoom nope\n").zoom, None);
    assert_eq!(theme::Workspace::parse("ui.zoom 0.1\n").zoom, Some(theme::ZOOM_MIN));
}

/// An annotation is a fact with a place: the camera goes to its anchor — the point
/// itself, or a segment's midpoint — and it projects through the SAME lens
/// every edge and toolpath point goes through, so an annotation at a toolpath's
/// coordinate lands on the toolpath's pixel, never beside it.
#[test]
fn test_a_mark_anchors_where_it_points_and_projects_through_the_one_lens() {
    let p = view::Annotation {
        shape: view::AnnotationShape::Point([10.0, 20.0, -3.0]),
        color: [1, 2, 3],
        label: "x".into(),
    };
    assert_eq!(p.anchor(), [10.0, 20.0, -3.0]);
    let s = view::Annotation {
        shape: view::AnnotationShape::Segment([0.0, 0.0, 0.0], [10.0, 20.0, -4.0]),
        color: [1, 2, 3],
        label: String::new(),
    };
    assert_eq!(s.anchor(), [5.0, 10.0, -2.0]);

    let cam = view::Camera::default_orbit();
    let lens = cam.lens();
    let direct = lens.project([10.0, 20.0, -3.0]);
    let through_mark = lens.project(p.anchor());
    assert_eq!(direct, through_mark, "one lens, one pixel");
    let (cx, cy, _) = cam.project([10.0, 20.0, -3.0]);
    assert_eq!((cx, cy), (direct.0, direct.1), "the camera and its lens agree (the hoist's identity)");
}

// ---- the findings as rows and annotations (Phase 11, lane A) -----------------------

use nacs::check_result;

/// The refusal's place reader — the ONE named exception to I7, because the
/// refusal is a `String` on the `--check=json` wire — pinned on the sentence
/// shapes it may meet: a `(x, y, z)` is a point, a `(x, y)` is lifted to the
/// stock's top, and a sentence with no numeric tuple names no place. Every
/// finding carries its place as a typed field and never comes here.
#[test]
fn test_the_refusal_s_place_is_the_one_thing_still_read_from_a_sentence() {
    let top = 10.0;
    // entry / body_foul / field_body_foul / a refusal: a 3-tuple.
    assert_eq!(
        view::refusal_place("the entry `G01 Z2.598 F200.000` meets standing material at (68.500, 40.000, -0.402) that no earlier op cleared", top),
        Some([68.5, 40.0, -0.402])
    );
    assert_eq!(
        view::refusal_place("T1's body 10.125mm above the tip sweeps to r 5.000mm and reaches standing stock at (48.819, 20.000, 9.839). Remove", top),
        Some([48.819, 20.0, 9.839])
    );
    // part_violation: a 3-tuple AFTER a parenthesised volume and the feature's kind.
    assert_eq!(
        view::refusal_place("'Post' (CylinderBoss): the sweep removed material — 6512 cells (~388.1 mm³) more than 0.707mm inside it read air, the lowest at (50.000, 50.000, -4.961), bounding box x 44.727..55.273", top),
        Some([50.0, 50.0, -4.961])
    );
    // fixture / deviation / engagement: a 2-tuple, lifted to the top.
    assert_eq!(
        view::refusal_place("T1's body 0.500mm above the tip reaches fixture 'ToeClamp' by 5.000mm near (48.000, 18.000) during 'Top'", top),
        Some([48.0, 18.0, top])
    );
    assert_eq!(
        view::refusal_place("corner shortfall 0.3107mm at (10.000, 20.000) — 5 corners the 1.000mm chamfer does not reach", top),
        Some([10.0, 20.0, top])
    );
    // containment / reach / decline / a cusp-budget engagement: no place.
    assert_eq!(view::refusal_place("'Blob' (Mesh) stands outside the stock: 10.000mm above the stock top z 0.000", top), None);
    assert_eq!(view::refusal_place("stock_to_leave (0.500mm) on 'Venturi' spends 76% of its budget on the pass's own cusps", top), None);
    assert_eq!(view::refusal_place("", top), None);
}

/// Every family has a colour that reads against the ground and is not
/// another family's, the error's is brighter than any of them, and the
/// pane's order names every family exactly once — a family added to the
/// engine that this list forgets fails here, not in a grey dot.
#[test]
fn test_every_family_has_its_own_legible_colour_and_a_place_in_the_order() {
    let t = Theme::default();
    let mut seen = Vec::new();
    for family in check_result::FAMILY_ORDER {
        let c = t.family_color(family);
        assert!(theme::contrast(c, t.ground) >= theme::TEXT_CONTRAST_FLOOR, "{family:?} {c:?} reads against the ground");
        assert!(!seen.contains(&c), "{family:?} shares a colour with another family");
        seen.push(c);
        assert!(check_result::FAMILY_ORDER.iter().filter(|f| **f == family).count() == 1);
    }
    assert_eq!(seen.len(), 11, "eleven families, eleven colours");
    assert!(theme::contrast(t.refusal, t.ground) >= theme::TEXT_CONTRAST_FLOOR);
    assert!(!seen.contains(&t.refusal));
}

/// ACCEPTANCE, the totality half (Phase 11 lane A, fork 8): over the whole
/// shipped corpus, every finding the check result carries is a row, and every row
/// either made an annotation or belongs to a family whose sentence names no place
/// (containment, reach, decline — about the declaration — or an engagement
/// cusp-budget line). Counts pinned as the corpus stands: 45 findings, 34
/// fixtures, and the families that MUST anchor always do.
#[test]
fn test_every_corpus_finding_is_a_row_and_anchors_or_says_why_not() {
    use nacs::parse::FindingFamily as F;
    let (mut rows_total, mut marks_total, mut fixtures_with) = (0usize, 0usize, 0usize);
    for entry in std::fs::read_dir(".").expect("repo root") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("cam") {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(path.join("main.part")) else { continue };
        let geom = std::fs::read_to_string(path.join("geometry.part")).ok();
        let Ok(part) = parse::parse_project_with_base(&src, geom.as_deref(), Some(&path)) else { continue };
        let v = check_result::check_result(&part);
        assert!(v.would_emit(), "{}: the shipped corpus emits", path.display());
        let (rows, annotations) = view::finding_rows(&v, part.stock.z, &Theme::default());
        // One row per finding — except a decline that repeats an earlier
        // one, subject and sentence, said once on screen (Phase 50 slice 7).
        let mut said: Vec<(&parse::FindingSubject, &str)> = Vec::new();
        let ordered: Vec<&parse::Finding> = check_result::FAMILY_ORDER
            .iter()
            .flat_map(|fam| v.findings.iter().filter(move |f| f.family == *fam))
            .filter(|f| {
                if f.family != F::Decline {
                    return true;
                }
                if said.iter().any(|(sub, m)| *sub == &f.subject && *m == f.message) {
                    return false;
                }
                said.push((&f.subject, &f.message));
                true
            })
            .collect();
        assert_eq!(rows.len(), ordered.len(), "{}: one row per finding, a repeated decline said once", path.display());
        if !rows.is_empty() {
            fixtures_with += 1;
        }
        for (row, f) in rows.iter().zip(ordered) {
            assert_eq!(row.text, f.message, "{}: the row is the sentence", path.display());
            assert!(row.head.starts_with(view::family_display(f.family)), "{}: the head is the family's word on screen: {}", path.display(), row.head);
            assert_eq!(row.unchecked, f.family == F::Decline);
            match f.family {
                F::Entry | F::BodyFoul | F::FieldBodyFoul | F::Fixture | F::Deviation | F::PartViolation => {
                    assert!(row.annotation.is_some(), "{}: a {:?} finding always names a place: {}", path.display(), f.family, f.message);
                }
                F::Containment | F::Reach | F::Decline => {
                    assert!(row.annotation.is_none(), "{}: a {:?} finding is about the declaration: {}", path.display(), f.family, f.message);
                }
                // Envelope carries the offending position as its typed place
                // when travel speaks; the ceiling and changer arms (slices
                // 2–3) will name none. Neither obligation is corpus-pinned
                // yet — no shipped fixture violates its own machine.
                F::Envelope => {}
                F::Engagement => {}
            }
            assert_eq!((row.place, row.at, row.tool), (f.place, f.at, f.tool), "the row carries the finding's typed fields");
            if let Some(mi) = row.annotation {
                let m = &annotations[mi];
                assert_eq!(m.color, row.color, "an annotation uses its row's colour");
                assert_eq!(Some(m.anchor()), f.place, "the annotation IS the typed place, never a parse");
            }
        }
        rows_total += rows.len();
        marks_total += annotations.len();
    }
    // 46 / 35 since Phase 26 slice 2: `pocket_islands.cam` ships with one
    // engagement advisory — the slot its second level cuts between islands.
    // Phase 27: `step.cam` (slice 2) and `pocket_floor_offset.cam` (slice 4) each
    // ship one engagement advisory; slice 3 moved the corpus's one keep-out
    // finding from `floor_clamped_offset.cam` to `floor_traverse_offset.cam`, net zero.
    // Phase 34 slice 1: `post_field.cam` ships with one engagement advisory
    // — the lanes between its posts, cut from solid.
    // Phase 38 slice 2: seven ops are mostly a slot and say so — the walk's
    // arm of the engagement family, on the three rail fixtures under spine,
    // `wrench_step_spine`'s box top and `walled_rails_ladder_offset`'s rest
    // floor. 49 → 56 rows, 38 → 43 fixtures.
    // Phase 41 slice 5: `tilted_bore.cam` ships one engagement advisory —
    // the pocket's third level cuts from solid under the constructed arc,
    // as every offset-family pocket of its size does. 56 → 57, 43 → 44.
    // Phase 49 slice 1: `rim_boss_offset.cam` ships one engagement advisory
    // — the offset family's own slot where the deepest level's lenses end,
    // between two bosses bitten out of the round's edge. 57 → 58, 44 → 45.
    // Phase 60 slice 1: `profile_pocket_arc.cam`'s Side plunged inside a
    // half-millimetre pocket nothing had cleared and the plunge taxonomy's
    // category 3 said so; the wall trace now plunges in air past the north
    // edge, and the advisory is gone. 58 → 57.
    // 2026-10-28: eleven revolved kinds and 29 fixtures deleted. 57 → 48.
    // 2026-11-03 (ROADMAP item 2, slice 5): `pocket_wall_lead.cam` is the
    // post pocket's floor fixture with the cavity wall's lead shaped, and
    // carries that fixture's slot advisory with it. 48 → 49, 36 → 37.
    // 2026-11-07 (ROADMAP item 3, slice 6): an op that cuts nothing says so;
    // `pocket_levels.cam`'s End runs at the floor in the air its Clear left,
    // and that is the fixture's first finding. 49 → 50, 37 → 38.
    assert_eq!(rows_total, 50, "the corpus's findings, as rows");
    // …and that advisory was the fixture's only finding. 45 → 44.
    assert_eq!(fixtures_with, 38); // 44 → 36 on 2026-10-28, with the deleted fixtures
    assert!(marks_total >= 30, "the anchored ones: {marks_total}");
}

/// ACCEPTANCE, the rejected half: the raised form-tool twin (the shipped
/// fixture with its Clear stripped — Phase 10 lane B's control) is REJECTED
/// by the entry restriction, and its check result opens as rows: the error on
/// top, in the error's colour, with its annotation at the standing stock the
/// text names — `(48.819, 20.000, 9.839)` — no emitted moves, and the parse-time
/// findings under it.
#[test]
fn test_a_refused_program_opens_on_its_refusal() {
    let src = std::fs::read_to_string("sphere_formtool.cam/main.part").unwrap();
    let raised = src.replace("run Clear(tool=T2, profile=TopDome, stepover=8.0, stepdown=5.0, stock_to_leave=0.5)\n", "");
    assert_ne!(raised, src);
    let part = parse::parse_project_with_base(&raised, None, Some(std::path::Path::new("sphere_formtool.cam"))).unwrap();
    let v = check_result::check_result(&part);
    assert!(!v.would_emit() && v.emitted_moves.is_none(), "rejected: no emitted_moves, no paths");
    let (rows, annotations) = view::finding_rows(&v, part.stock.z, &Theme::default());
    assert_eq!(rows[0].head, "REJECTED");
    assert_eq!(rows[0].color, Theme::default().refusal);
    assert!(rows[0].text.starts_with("Form on 'TopDome' with T1: the move `G01 Z-0.286 F200.000`"), "{}", rows[0].text);
    let m = &annotations[rows[0].annotation.expect("the error names its place")];
    assert_eq!(m.anchor(), [48.819, 20.0, 9.839]);
    assert_eq!(m.label, "rejected");
    assert!(rows.len() > 1, "the parse-time findings (the body foul the probe now sees) follow: {rows:?}");
    assert!(rows[1..].iter().any(|r| r.head.starts_with("Holder collision")), "{rows:?}");
    // The camera goes there.
    let mut cam = view::Camera::default_orbit();
    cam.look_at(m.anchor());
    assert_eq!(cam.target, [48.819, 20.0, 9.839]);
    assert_eq!(cam.project(m.anchor()).0, 0.0, "the anchor sits at screen centre");
}

// ---- the two stocks, and time (Phase 11, lane B) ------------------------------

/// Phase 26 slice 4: the picture of an islanded removal is the removal. The
/// pocket's declared stock draws its own outline AND the post's — four
/// boundaries, floor and ceiling of each — with no window code and no second
/// description: `Section::outlines` hands back the holes, and the terminal's
/// `removes:` line says the same thing in words.
#[test]
fn test_an_islanded_removal_draws_the_island_it_goes_around() {
    let p = load("pocket_post.cam");
    let line = nacs::outline::removal_line(&p, 0);
    assert!(line.starts_with("removes: polygon (") && line.ends_with(" pts) around 1 island z -5.000..0.000"), "{line}");
    let edges = view::removal_boundaries(&p, 1).expect("the islanded Clear has a closed form");
    assert_eq!(edges.len(), 4, "the pocket's outline and the post's, each at the floor and the ceiling");
    let post: Vec<_> = edges
        .iter()
        .filter(|w| w.iter().all(|pt| (((pt[0] - 50.0).powi(2) + (pt[1] - 50.0).powi(2)).sqrt() - 6.0).abs() < 1e-6))
        .collect();
    assert_eq!(post.len(), 2, "the post's circle, r 6 about (50, 50)");
    assert!(post.iter().any(|w| w.iter().all(|pt| (pt[2] + 5.0).abs() < 1e-6)), "at the floor");
    assert!(post.iter().any(|w| w.iter().all(|pt| pt[2].abs() < 1e-6)), "at the ceiling");
}

/// The declared stock's edges are `--outline`'s `removes:` line, drawn.
/// `sphere_clear.cam` after its Clear (k = 1): the moat's section is an
/// annulus to r 32.500 about (50, 40), so its outer boundary is a circle of that
/// radius, once on the floor (the equator plane, z 0) and once on the
/// ceiling (the stock's top, z 22); after the finish (k = 2) the dome's disc
/// and the swept tube's annulus join it, the tube's boundaries following its
/// envelope. At k = 0 nothing is declared gone, and a model that declines an
/// op declines the viewport with it, in its own words.
#[test]
fn test_the_declared_stock_draws_the_outline_s_own_line() {
    let p = load("sphere_clear.cam");
    let line = nacs::outline::removal_line(&p, 0);
    assert_eq!(line, "removes: annulus r 0.000..32.500 z 0.000..22.000");
    let edges = view::removal_boundaries(&p, 1).expect("the Clear has a closed form");
    assert_eq!(edges.len(), 2, "one boundary (r_inner is 0), at the floor and at the ceiling");
    for pt in &edges[0] {
        let rho = ((pt[0] - 50.0).powi(2) + (pt[1] - 40.0).powi(2)).sqrt();
        assert!((rho - 32.5).abs() < 1e-6, "the outer boundary: {pt:?}");
    }
    assert!(edges[0].iter().all(|pt| (pt[2] - 0.0).abs() < 1e-6), "the floor boundary rides the terraced floor's outer edge, at the equator plane");
    assert!(edges[1].iter().all(|pt| (pt[2] - 22.0).abs() < 1e-6), "the ceiling boundary at the stock's top");
    assert_eq!(edges[0][0], *edges[0].last().unwrap(), "closed");

    let after = view::removal_boundaries(&p, 2).expect("the finish has a closed form");
    assert!(after.len() > edges.len(), "the dome's disc and the tube's annulus join: {}", after.len());
    let tube_ceiling = after.iter().filter(|w| w.iter().any(|pt| pt[2] > 10.0 && pt[2] < 22.0 - 1e-6)).count();
    assert!(tube_ceiling >= 1, "the swept tube's ceiling follows its envelope, between the equator and the top");

    assert!(view::removal_boundaries(&p, 0).unwrap().is_empty(), "k = 0: nothing declared gone");
    let boundary = load("contour_pocket.cam");
    let err = view::removal_boundaries(&boundary, 1).expect_err("the boundary edge declines");
    assert!(err.contains("model incomplete") && err.contains("op [0]"), "{err}");
}

/// A band's boundaries sit on the MATERIAL side of its path at its two
/// distances, whichever way the path winds — found by the band's own
/// containment test, never by an offset sign. `profile_boss.cam`'s moat
/// bands about the boss: every boundary point is within the band's distance
/// span of the path and inside the boss's own containment of that band.
#[test]
fn test_a_band_s_rims_lie_on_the_material_side_at_its_distances() {
    let p = load("profile_boss.cam");
    let edges = view::removal_boundaries(&p, p.operations.len()).expect("bosses have their bands");
    assert!(!edges.is_empty());
    let model = nacs::stock::remaining_before(&p, p.operations.len());
    let mut bands = 0;
    for r in &model.removals {
        if let nacs::stock::Section::PathBand { path, material_inside, d_inner, d_outer } = &r.section {
            bands += 1;
            for boundary in r.section.outlines(&p.stock) {
                for q in &boundary {
                    let d = nacs::stock::dist_to_polygon(path, q.x, q.y);
                    assert!(d >= d_inner - 1e-3 && d <= d_outer + 1e-3, "a boundary point {d:.3} off the path for a {d_inner}..{d_outer} band");
                    if d > 1e-3 {
                        assert_eq!(nacs::stock::point_in_polygon(path, q.x, q.y), *material_inside, "on the material side");
                    }
                }
            }
        }
    }
    assert!(bands >= 2, "the moat family is bands: {bands}");
}

/// The swept stock at k = 0 is the untouched stock: the prefix field over no
/// ops draws a box the stock's size, and the field after every op is the
/// same surface `stock_surface` always drew.
#[test]
fn test_the_swept_stock_at_the_scrub_s_ends() {
    let p = load("plate.cam");
    let emitted_moves = gcode::program_with_moves(&p).unwrap().1;
    let stock = view::stock_surface_prefix(&p, &emitted_moves, 0, None, Some(2.0)).expect("a stock has a surface");
    let bb = view::stock_aabb(&p.stock);
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    for v in &stock.verts {
        for a in 0..3 {
            lo[a] = lo[a].min(f64::from(v[a]));
            hi[a] = hi[a].max(f64::from(v[a]));
        }
    }
    for a in 0..3 {
        assert!((lo[a] - bb[a]).abs() <= 2.0 && (hi[a] - bb[a + 3]).abs() <= 2.0, "axis {a}: the stock's box to within a cell: {lo:?} {hi:?} vs {bb:?}");
    }
    let all = view::stock_surface_prefix(&p, &emitted_moves, p.operations.len(), None, Some(2.0)).unwrap();
    let whole = view::stock_surface(&p, Some(2.0)).unwrap();
    assert_eq!(all.verts.len(), whole.verts.len(), "the prefix over every op is the whole");
    assert_eq!(all.tris, whole.tris);
}

// ---- the tool in its world (Phase 11, lane C) ----------------------------------

/// The refusal row is the one named I7 exception: its annotation is read
/// from its sentence (`refusal_place`, the material point `(48.819, 20.000,
/// 9.839)`), but it carries NO typed tip and no tool, so it stands none —
/// the cost of a refusal that is a `String` on the wire, recorded. The
/// body-foul row under it carries its tip and tool as fields, and the tool
/// stands there.
#[test]
fn test_the_refusal_row_is_the_one_named_exception() {
    let src = std::fs::read_to_string("sphere_formtool.cam/main.part").unwrap();
    let raised = src.replace("run Clear(tool=T2, profile=TopDome, stepover=8.0, stepdown=5.0, stock_to_leave=0.5)\n", "");
    let part = parse::parse_project_with_base(&raised, None, Some(std::path::Path::new("sphere_formtool.cam"))).unwrap();
    let v = check_result::check_result(&part);
    let (rows, annotations) = view::finding_rows(&v, part.stock.z, &Theme::default());
    assert_eq!(annotations[rows[0].annotation.unwrap()].anchor(), [48.819, 20.0, 9.839], "the annotation is still the material point");
    assert_eq!((rows[0].at, rows[0].tool), (None, None), "the refusal carries nothing typed");
    assert_eq!(view::tool_pose(&part, view::Selection::Finding(0), &rows, &[]), None, "and stands no tool");
    // The body-foul advisory under it carries its tip and tool as fields,
    // and the tool stands on the probe's ring.
    let (i, foul) = rows.iter().enumerate().find(|(_, r)| r.head.starts_with("Holder collision")).expect("the probe's row");
    let tip = foul.at.expect("the probe names its tip");
    assert_eq!(foul.tool, Some(1));
    assert!((tip[2] - 0.0).abs() < 1e-9 && ((tip[0] - 30.0).powi(2) + (tip[1] - 20.0).powi(2)).sqrt() > 10.0, "on the boundary ring at the equator: {tip:?}");
    assert_eq!(view::tool_pose(&part, view::Selection::Finding(i), &rows, &[]), Some((tip, 1)), "the tool stands where the field says");
    assert!(foul.place.is_some(), "and its place — the material point — is typed too");
}

/// I7 over the corpus: every finding whose family is about a BODY carries
/// its tip and tool as typed fields — the entry flag, both foul probes, the
/// fixture check — and every family that names a place carries it typed;
/// the declaration-only families carry none. Nothing is read from a
/// sentence: changing a message's wording changes nothing but the text.
#[test]
fn test_a_body_finding_carries_its_tip_and_tool_as_fields() {
    use nacs::parse::FindingFamily as F;
    let mut stands = 0;
    for entry in std::fs::read_dir(".").expect("repo root") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("cam") {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(path.join("main.part")) else { continue };
        let geom = std::fs::read_to_string(path.join("geometry.part")).ok();
        let Ok(part) = parse::parse_project_with_base(&src, geom.as_deref(), Some(&path)) else { continue };
        for f in check_result::check_result(&part).findings {
            match f.family {
                F::Entry | F::BodyFoul | F::FieldBodyFoul | F::Fixture => {
                    assert!(f.at.is_some(), "{}: {:?} carries its tip: {}", path.display(), f.family, f.message);
                    assert!(f.tool.is_some(), "{}: and its tool", path.display());
                    // The material point and the tip are two facts; an entry's
                    // plunge destination IS the material it meets, so they may
                    // coincide — both must exist.
                    assert!(f.place.is_some());
                    stands += 1;
                }
                F::Deviation | F::PartViolation => {
                    assert!(f.place.is_some(), "{}: {:?} carries its place: {}", path.display(), f.family, f.message);
                    assert_eq!((f.at, f.tool), (None, None), "{}: {}", path.display(), f.message);
                }
                // Envelope's travel arm carries the offending position as
                // its place; the ceiling arm names no place (an RPM happens
                // everywhere the op cuts). Neither carries a tip or tool
                // stand.
                F::Envelope => {
                    assert_eq!((f.at, f.tool), (None, None), "{}: {}", path.display(), f.message);
                }
                F::Containment | F::Reach | F::Decline | F::Engagement => {
                    assert_eq!((f.at, f.tool), (None, None), "{}: {}", path.display(), f.message);
                }
            }
        }
    }
    // Phase 60 slice 1: `profile_pocket_arc`'s Side plunges in air past the
    // blank, not inside an uncleared pocket — one entry finding fewer.
    // 2026-10-28: the deleted revolved fixtures took eight entry findings with them. 26 → 18.
    assert_eq!(stands, 18, "entry 15 + body_foul 2 + fixture 1 (no field foul ships on the corpus): {stands}");
}

/// The body is the one silhouette home drawn: `pseudosphere_funnel_holder`'s
/// T10 declares flutes to 3, a 10mm shank, an 8mm stickout, a 32mm nose for
/// 40mm and a 50mm body — so its rings read exactly `radius_at` at each
/// rung, the top is the holder body's top (48mm), and the body is declared.
/// `plate`'s T5 declares nothing: four radii, and the pane says so.
#[test]
fn test_the_tool_body_is_the_silhouette_s_own_ladder() {
    let p = load("spline_funnel_holder.cam");
    let sil = view::Silhouette::of(p.tools.get(&10).unwrap());
    let (top, declared) = view::tool_top(&sil);
    assert!(declared);
    assert!((top - 48.0).abs() < 1e-9, "nose face 8 + nose 40: {top}");
    let tip = [10.0, 20.0, -5.0];
    let edges = view::tool_body(&sil, tip);
    let rings: Vec<&Vec<[f64; 3]>> = edges.iter().filter(|w| w.first() == w.last()).collect();
    assert!(rings.len() >= 6, "tip, flute top, stickout (two each), top: {}", rings.len());
    for ring in &rings {
        let h = ring[0][2] - tip[2];
        let r = ((ring[0][0] - tip[0]).powi(2) + (ring[0][1] - tip[1]).powi(2)).sqrt();
        assert!((r - sil.radius_at(h)).abs() < 1e-6, "a ring at h {h:.3} has the silhouette's radius: {r} vs {}", sil.radius_at(h));
        assert_eq!(*ring.first().unwrap(), *ring.last().unwrap(), "closed");
    }
    let widest = rings.iter().map(|r| ((r[0][0] - tip[0]).powi(2) + (r[0][1] - tip[1]).powi(2)).sqrt()).fold(0.0f64, f64::max);
    assert!((widest - 25.0).abs() < 1e-6, "the holder body's radius: {widest}");
    let meridians: Vec<&Vec<[f64; 3]>> = edges.iter().filter(|w| w.first() != w.last()).collect();
    assert_eq!(meridians.len(), 4, "four meridians");
    assert!(meridians.iter().all(|m| m.len() >= rings.len()), "each meridian visits every level, the tip's r-0 level included");

    let plain = load("plate.cam");
    let sil = view::Silhouette::of(plain.tools.get(&5).unwrap());
    let (top, declared) = view::tool_top(&sil);
    assert!(!declared && (top - 20.0).abs() < 1e-9, "four radii of a dia-10 rod: {top}");
}

/// The fixture's keep-outs are their declared boxes: `floor_clamped`'s
/// `ToeClamp Box(w=14, l=24, h=25, x=48, y=18, z=0)` draws two rectangles
/// at z 0 and z 25 on those corners, and four uprights.
#[test]
fn test_the_fixture_draws_its_declared_box() {
    let p = load("floor_clamped_offset.cam");
    let edges = view::fixture_boundaries(&p);
    assert_eq!(edges.len(), 6, "two boundaries, four uprights");
    let boundaries: Vec<&Vec<[f64; 3]>> = edges.iter().filter(|w| w.len() == 5).collect();
    assert_eq!(boundaries.len(), 2);
    for boundary in boundaries {
        let xs: Vec<f64> = boundary.iter().map(|p| p[0]).collect();
        let ys: Vec<f64> = boundary.iter().map(|p| p[1]).collect();
        assert_eq!((xs.iter().cloned().fold(f64::MAX, f64::min), xs.iter().cloned().fold(f64::MIN, f64::max)), (48.0, 62.0));
        assert_eq!((ys.iter().cloned().fold(f64::MAX, f64::min), ys.iter().cloned().fold(f64::MIN, f64::max)), (18.0, 42.0));
        assert!(boundary.iter().all(|p| p[2] == 0.0) || boundary.iter().all(|p| p[2] == 25.0));
    }
    assert!(view::fixture_boundaries(&load("plate.cam")).is_empty(), "no fixture, no wires");

    // The deepest point of an op: plate's facing runs at z 0 everywhere, so
    // its first feed vertex is the deepest; the tool stands there.
    let emitted_moves = gcode::program_with_moves(&p).unwrap().1;
    let paths = view::op_paths(&p, &emitted_moves);
    let d = view::deepest_point(&paths[0]).expect("a facing has feed motion");
    let lowest = paths[0].polylines.iter().filter(|pl| !pl.rapid).flat_map(|pl| pl.pts.iter()).map(|p| p[2]).fold(f64::MAX, f64::min);
    assert_eq!(d[2], lowest, "the lowest feed vertex: {d:?}");
    assert!(d[2] < 0.0, "a facing cuts below the face it declares: {d:?}");
}

// ---- the help (Phase 11, lane D) ----------------------------------------------


// ---- the actions (Phase 12, slice 4) -------------------------------------------
//
// One inventory: ids and default keys unique, the palette's filter over
// it, the workspace file's rebind lines judged against it, the default
// construction (untested, by charter); everything the acts are MADE OF —
// the table, the filter, the judgment — is pinned here.

/// Ids unique (the palette's and the file's word for an act), default
/// keys unique (two actions on one key would race), every sentence
#[test]
fn test_the_action_inventory_is_a_directory_not_a_pile() {
    let mut ids = Vec::new();
    let mut keys = Vec::new();
    for a in view::ACTIONS {
        assert!(!ids.contains(&a.id), "`{}` appears twice", a.id);
        // An act may ship with NO default key (Phase 13, slice 3's pane
        // acts do): a key of none is not a binding, so it cannot collide
        // with another. Every key that IS one is still unique.
        if !a.key.is_empty() {
            assert!(!keys.contains(&a.key), "key `{}` is bound twice", a.key);
            keys.push(a.key);
        }
        assert!(!a.sentence.is_empty(), "`{}` has a sentence", a.id);
        ids.push(a.id);
    }
}

/// The palette's whole matching rule: empty lists everything in inventory
/// order, the filter is a case-insensitive substring over id and
/// sentence (and an exact key), and a miss is an empty list — fuzziness
/// stays out by name.
#[test]
fn test_the_palette_filter_is_a_substring_and_nothing_cleverer() {
    assert_eq!(view::palette_matches("").len(), view::ACTIONS.len());
    let cursor: Vec<&str> = view::palette_matches("cursor").iter().map(|a| a.id).collect();
    assert_eq!(cursor, ["cursor.back", "cursor.on", "cursor.block_back", "cursor.block_on"]);
    let hide: Vec<&str> = view::palette_matches("HIDE").iter().map(|a| a.id).collect();
    assert!(hide.contains(&"sidebar.toggle") && hide.contains(&"layer.outlines"), "case-insensitive, over sentences too: {hide:?}");
    assert_eq!(view::palette_matches("?").first().map(|a| a.id), Some("help"), "an exact key finds its action");
    assert!(view::palette_matches("unicorn").is_empty());
}

/// The file's `key` lines, judged: unknown ids dropped, the last line
/// for an id wins, known ids pass their key string through untouched.
#[test]
fn test_rebind_lines_are_judged_last_wins_unknown_dropped() {
    let overrides = vec![
        ("palette.open".to_string(), "k".to_string()),
        ("unicorn.fly".to_string(), "u".to_string()),
        ("palette.open".to_string(), "Space".to_string()),
    ];
    let resolved = view::resolve_keys(&overrides);
    assert_eq!(resolved, vec![("palette.open", "Space".to_string())]);
}

/// The widened workspace file (slice 4): theme, rebindings and the
/// x`, and a malformed extra line is dropped without touching the rest.
#[test]
fn test_the_workspace_file_carries_keys_and_theme() {
    let ws = theme::Workspace {
        layout: Layout::default(),
        theme: Some("gruvbox-dark".to_string()),
        zoom: None,
        project: None,
        shell: None,
        keys: vec![("palette.open".to_string(), "Space".to_string())],
    };
    assert_eq!(theme::Workspace::parse(&ws.render()), ws, "parse(render(x)) == x");
    // A bare `key` with no second word is dropped; the rest of the file
    // still lands.
    let text = format!("{}key palette.open\n", ws.render());
    let parsed = theme::Workspace::parse(&text);
    assert_eq!(parsed.keys, ws.keys, "the malformed line vanished, the good one stayed");
    // And the empty file is the default: no theme, no keys, no toolbar.
    assert_eq!(theme::Workspace::parse(""), theme::Workspace::default());
    // A `toolbar` line is a line older files wrote for a surface that no
    // longer exists: unknown, and dropped.
    assert_eq!(theme::Workspace::parse("toolbar help,mode.revert\n"), theme::Workspace::default());
}

// ---- the docks (Phase 12, slice 1) ---------------------------------------------
//
// The arrangement is a std model (`theme::Layout`) and `apply` is its only
// mutator, so the whole dock vocabulary is pinned headless: every pane has
// exactly one home under every action (assignment totality), the vacated
// dock keeps showing what it was showing, cycling wraps, switching opens,
// and no action can wedge a dock outside its size bounds. The shell only
// translates keys and clicks into these actions — what is pinned here is
// everything the docks can DO.





// ---- the workspace file (Phase 12, slice 2) ------------------------------------
//
// The arrangement survives the window as one hand-written key-value file:
// `parse(render(x)) == x`, and a mangled file dents the layout without
// ever losing a pane or blocking the window — every unreadable or unknown
// line falls silently to the default already in place, and totality holds
// against ANY input because panes are homed first-claim-wins with the
// unhomed falling to their default dock.




// ---- the theme's validator and file form (Phase 12, slice 3) -------------------
//
// The half with the correctness obligation: a theme is judged before it
// is active, a MEANING violation rejects the meaning slots (the built-in
// stands, the chrome applies), and the laws are std-only so the lean leg
// pins them — the reader behind `frontend` is pinned in the default leg
// against the shipped sample and the deliberately confusable fixture.

/// The simulation's arithmetic can be trusted or the judgments cannot:
/// each Machado matrix's rows sum to 1, so a pure grey is a fixed point
/// of every dichromacy — and the distance of a colour to itself is zero.
#[test]
fn test_the_cvd_matrices_fix_grey_and_the_measure_is_sane() {
    for cvd in theme::Cvd::ALL {
        for g in [[0, 0, 0], [128, 128, 128], [255, 255, 255]] {
            assert!(
                theme::delta_e(g, g, Some(cvd)) < 1e-9,
                "{}: a colour is itself",
                cvd.word()
            );
            // Grey in, grey out: simulate by measuring against the SAME
            // grey seen normally — a fixed point has no distance to it.
            assert!(
                theme::delta_e(g, g, Some(cvd)).abs() < 1e-9
                    && theme::delta_e([128, 128, 128], [128, 128, 128], Some(cvd)) < 1e-9
            );
        }
        // And the simulation moves a saturated red — it does something.
        assert!(theme::delta_e([255, 0, 0], [255, 0, 0], Some(cvd)) < 1e-9);
        let moved = {
            // red vs green collapse under the red-green dichromacies
            let d_normal = theme::delta_e([200, 60, 60], [60, 160, 60], None);
            let d_sim = theme::delta_e([200, 60, 60], [60, 160, 60], Some(cvd));
            (d_normal, d_sim)
        };
        if cvd != theme::Cvd::Tritanopia {
            assert!(moved.1 < moved.0, "{}: red and green draw together", cvd.word());
        }
    }
}

/// The built-in palette passes its own validator — the suite's standing
/// proof the stock colours are legal, and the guarantee that rejecting a
/// theme's meaning slots always has a legal fallback to stand.
#[test]
fn test_the_built_in_palette_passes_its_own_validator() {
    let findings = theme::validate(&Theme::default());
    // Legal means no MEANING finding; the family advisories under a
    // dichromacy are expected and named (their own pin).
    assert!(findings.iter().all(|f| f.grade == theme::ThemeGrade::Chrome), "the built-in is the law's first subject: {findings:?}");
    // With margin on the role floor, under every eye — a palette tweak
    // that lands near the floor should be a deliberate act, not a drift.
    for cvd in [None, Some(theme::Cvd::Protanopia), Some(theme::Cvd::Deuteranopia), Some(theme::Cvd::Tritanopia)] {
        let t = Theme::default();
        for (a, b) in [
            (t.role_roughing, t.role_finishing),
            (t.role_roughing, t.role_untagged),
            (t.role_finishing, t.role_untagged),
        ] {
            assert!(theme::delta_e(a, b, cvd) >= theme::ROLE_DELTA_E_FLOOR + 5.0);
        }
    }
}

/// The policy: a role pair confusable under a dichromacy — legible to
/// normal vision, which is exactly why the simulation must be the judge —
/// is a MEANING violation naming the eye; the theme keeps its chrome and
/// loses its viewport to the built-in. A chrome-only complaint changes
/// nothing but the record.
#[test]
fn test_role_confusability_is_refused_and_keeps_the_chrome() {
    let mut t = Theme::default();
    assert!(t.set_slot("role.roughing", [200, 120, 60], None));
    assert!(t.set_slot("role.finishing", [130, 150, 60], None));
    assert!(t.set_slot("background", [10, 10, 12], None));
    assert!(
        theme::delta_e([200, 120, 60], [130, 150, 60], None) >= theme::ROLE_DELTA_E_FLOOR,
        "the pair reads fine to normal vision — the trap the simulation exists for"
    );
    let findings = theme::validate(&t);
    assert!(
        findings.iter().any(|f| f.grade == theme::ThemeGrade::Meaning
            && f.message.contains("deuteranopia")),
        "the violation names the eye: {findings:?}"
    );
    let (active, _) = theme::apply_policy(t);
    assert_eq!(active.role_roughing, Theme::default().role_roughing, "the built-in stands");
    assert_eq!(active.background, [10, 10, 12], "the chrome half applies");

    // Chrome-only: text on its own panel, applied anyway.
    let mut t = Theme::default();
    assert!(t.set_slot("text", [40, 40, 40], None));
    let findings = theme::validate(&t);
    assert!(findings.iter().all(|f| f.grade == theme::ThemeGrade::Chrome), "{findings:?}");
    let (active, notes) = theme::apply_policy(t.clone());
    assert_eq!(active, t, "taste is the user's");
    assert!(!notes.is_empty(), "…but it is named");
}

/// Every name the two tables print is settable through the one door, and
/// reads back through the same table — a slot added to the struct without
/// joining `set_slot` fails here, not in a file that silently does
/// nothing. A made-up name is rejected.
#[test]
fn test_every_slot_name_is_settable_through_the_one_door() {
    let mut t = Theme::default();
    let names: Vec<&'static str> = t
        .meaning_slots()
        .into_iter()
        .chain(t.chrome_slots())
        .map(|(n, _)| n)
        .collect();
    for (i, name) in names.iter().enumerate() {
        let c = [i as u8, 2, 3];
        assert!(t.set_slot(name, c, Some(9)), "`{name}` is settable");
        let read = t
            .meaning_slots()
            .into_iter()
            .chain(t.chrome_slots())
            .find(|(n, _)| n == name)
            .unwrap()
            .1;
        assert_eq!(read, c, "`{name}` reads back");
    }
    assert_eq!(t.indicator.1, 9, "the indicator keeps its alpha");
    assert!(!t.set_slot("unicorn", [1, 2, 3], None), "a made-up name is rejected");
}

/// `#rrggbb` and `#rrggbbaa`, and nothing else.
#[test]
fn test_hex_parses_the_two_zed_forms() {
    assert_eq!(theme::parse_hex("#eba03c"), Some(([235, 160, 60], None)));
    assert_eq!(theme::parse_hex("#ebeef5be"), Some(([235, 238, 245], Some(190))));
    assert_eq!(theme::parse_hex(" #ffffff "), Some(([255, 255, 255], None)));
    for junk in ["", "#", "#fff", "#12345", "eba03c", "#gggggg", "#eba03c1"] {
        assert_eq!(theme::parse_hex(junk), None, "`{junk}` is not a colour");
    }
}

/// The reader (default leg): the SHIPPED sample is the built-in palette
/// written out as a file — one home, two renderings, pinned equal — and
/// an unmodified Zed-shape file with no `nacs` object is a valid theme
/// whose viewport speaks the stock palette.
#[cfg(feature = "frontend")]
#[test]
fn test_the_shipped_sample_is_the_built_in_written_out() {
    let text = std::fs::read_to_string("themes/nacs-dark.json").expect("the shipped sample");
    let t = theme::theme_from_json(&text).expect("the sample parses");
    assert_eq!(t, Theme::default(), "the sample IS the built-in, as a file");
    assert!(theme::validate(&t).iter().all(|f| f.grade == theme::ThemeGrade::Chrome), "legal: no meaning finding");

    let zed_only = r##"{"name":"minimal","themes":[{"name":"m","appearance":"dark",
        "style":{"background":"#101014","panel.background":"#16161a","text":"#d8d8e0"}}]}"##;
    let t = theme::theme_from_json(zed_only).expect("a pure Zed family parses");
    assert_eq!(t.background, [16, 16, 20]);
    assert_eq!(t.panel_background, [22, 22, 26]);
    assert_eq!(t.text, [216, 216, 224]);
    assert_eq!(t.role_roughing, Theme::default().role_roughing, "meaning slots stay stock");
    let (active, findings) = theme::apply_policy(t.clone());
    assert_eq!(active, t, "no meaning violation in a chrome-only theme");
    let _ = findings;
}

/// The reader (default leg): EVERY theme shipped in `themes/` is a theme
/// the window would use whole — it parses, it passes the validator, and
/// its `nacs` object names every meaning slot by hand. That last clause is
/// the anti-drift half: a shipped theme that omits a slot inherits the
/// built-in's colour for it and looks finished, so a slot added to the
/// table would silently leave every port half-dressed. This fails instead.
#[cfg(feature = "frontend")]
#[test]
fn test_every_shipped_theme_is_worn_whole() {
    let mut seen = 0;
    for entry in std::fs::read_dir("themes").expect("the themes dir") {
        let path = entry.expect("a themes entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        seen += 1;
        let name = path.display().to_string();
        let text = std::fs::read_to_string(&path).expect(&name);
        let candidate = theme::theme_from_json(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        let findings = theme::validate(&candidate);
        let meaning: Vec<_> = findings.iter().filter(|f| f.grade == theme::ThemeGrade::Meaning).collect();
        assert!(meaning.is_empty(), "{name} is shipped, so it is legal: {meaning:?}");
        let (active, _) = theme::apply_policy(candidate.clone());
        assert_eq!(active, candidate, "{name} is active whole — no half applies");
        for (slot, _) in candidate.meaning_slots() {
            assert!(
                text.contains(&format!("\"{slot}\"")),
                "{name} never names `{slot}` — it would silently use the built-in's colour there"
            );
        }
    }
    assert!(seen >= 2, "the sweep found {seen} theme(s) — it is not scanning an empty dir");
}

/// The shipped set holds a LIGHT face (Phase 24 slice 1): `nacs-light.json`
/// says `appearance: light`, passes the law with no meaning finding, and
/// comes through `apply_policy` whole and light — the acceptance sentence
/// Phase 23 pinned on a synthetic palette, now pinned on the file that
/// ships. And fork 3, as recommended: the same file with ONE meaning
/// fault wears its light chrome over the dark built-in's viewport, the
/// appearance riding with the chrome — a broken light file is a hybrid
/// the theme line names, never a silently dark window.
#[cfg(feature = "frontend")]
#[test]
fn test_the_shipped_set_holds_a_light_face() {
    let text = std::fs::read_to_string("themes/nacs-light.json").expect("the light face ships");
    let candidate = theme::theme_from_json(&text).expect("the light face parses");
    assert_eq!(candidate.appearance, theme::Appearance::Light, "the file says light");
    assert!(
        theme::relative_luminance(candidate.ground) > theme::relative_luminance(Theme::default().ground),
        "a light face has a lighter ground than the built-in"
    );
    let meaning: Vec<_> = theme::validate(&candidate).into_iter().filter(|f| f.grade == theme::ThemeGrade::Meaning).collect();
    assert!(meaning.is_empty(), "the light face is legal whole: {meaning:?}");
    let (active, _) = theme::apply_policy(candidate.clone());
    assert_eq!(active, candidate, "worn whole");
    assert_eq!(active.appearance, theme::Appearance::Light);

    // Fork 3: one meaning fault, and the fallback's face is the hybrid,
    // named — light chrome, dark built-in viewport, appearance kept.
    let mut bad = candidate.clone();
    bad.set_slot("role.roughing", bad.role_finishing, None);
    let (active, findings) = theme::apply_policy(bad);
    assert!(findings.iter().any(|f| f.grade == theme::ThemeGrade::Meaning), "the fault is a meaning finding");
    assert_eq!(active.appearance, theme::Appearance::Light, "the appearance rides with the chrome");
    assert_eq!(active.text, candidate.text, "the light chrome stays");
    assert_eq!(active.ground, Theme::default().ground, "the viewport is the built-in's");
    assert_eq!(active.role_roughing, Theme::default().role_roughing);
}

/// `--theme-check` says WHICH file it judged, first and once (Phase 24
/// slice 2, fork 4C) — by path the path as given, by NAME the copy under
/// the config dir the name resolved to. The finding this pins: a bare name
/// reads a copy, a copy goes stale, and the verdict must name the file it
/// is about. The config dir is pointed at a scratch dir through
/// `XDG_CONFIG_HOME`, so the test reads nothing of the user's.
#[cfg(all(feature = "frontend", not(windows)))]
#[test]
fn test_theme_check_names_the_file_it_judged() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_nacs"))
        .args(["--theme-check", "themes/nacs-light.json"])
        .output()
        .expect("run nacs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut lines = stdout.lines();
    assert_eq!(lines.next(), Some("judging themes/nacs-light.json"), "the path as given, first: {stdout}");
    assert!(stdout.lines().last().unwrap_or("").starts_with("theme ok"), "the verdict last: {stdout}");
    assert!(out.status.success());

    // By name: a scratch config dir holding a copy of the light face.
    let scratch = std::env::temp_dir().join(format!("nacs-theme-check-{}", std::process::id()));
    let themes = scratch.join("nacs").join("themes");
    std::fs::create_dir_all(&themes).expect("scratch themes dir");
    let copy = themes.join("scratch-light.json");
    std::fs::copy("themes/nacs-light.json", &copy).expect("copy the light face");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_nacs"))
        .env("XDG_CONFIG_HOME", &scratch)
        .args(["--theme-check", "scratch-light"])
        .output()
        .expect("run nacs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        stdout.lines().next(),
        Some(format!("judging {}", copy.display()).as_str()),
        "a bare name says the copy it resolved to: {stdout}"
    );
    assert!(out.status.success(), "{stdout}");
    let _ = std::fs::remove_dir_all(&scratch);
}

/// The reader (default leg): the deliberately confusable fixture — roles
/// that read fine to normal vision and collapse under deuteranopia — is
/// REJECTED by name, exactly as `--theme-check` would report it.
#[cfg(feature = "frontend")]
#[test]
fn test_the_confusable_fixture_is_refused_by_name() {
    let text = std::fs::read_to_string("tests/confusable_theme.json").expect("the fixture");
    let candidate = theme::theme_from_json(&text).expect("the fixture parses");
    let findings = theme::validate(&candidate);
    assert!(
        findings.iter().any(|f| f.grade == theme::ThemeGrade::Meaning
            && f.message.contains("deuteranopia")
            && f.message.contains("confusable")),
        "{findings:?}"
    );
    let (active, _) = theme::apply_policy(candidate);
    assert_eq!(active.role_roughing, Theme::default().role_roughing);
    assert_eq!(active.role_finishing, Theme::default().role_finishing);
}

/// The file lives in the user's config dir, never the project — a
/// churning chrome file inside a reviewable-text project is noise in
/// every review (fork 3C, rejected). `None` (no HOME at all) is a legal
/// answer: the window opens at the default and saves nowhere.
#[test]
fn test_the_workspace_lives_in_the_config_dir_not_the_project() {
    if let Some(p) = theme::workspace_path() {
        assert!(p.is_absolute(), "an absolute home, not a cwd-relative one: {}", p.display());
        assert!(
            p.ends_with(std::path::Path::new("nacs").join("workspace")),
            "…/nacs/workspace: {}",
            p.display()
        );
    }
}

// ---- the mode table (Phase 13, slice 0; retired 2026-09-04, Phase 21 slice 3) --
//
// Phase 13 opened on a measurement: ZERO statements on disk about what a
// person is doing when the window is open. The three modes were those
// statements; Phase 21 measured them again and found each was a (sidebar
// view, document) pair — the sidebar's and the center's own job — and
// retired them. What survives is above: one arrangement, `0` its escape,
// the diagnose stance a rule about the view at open.








// ---- the three named layouts (Phase 13, slice 2) --------------------------------
//
// Fork 1's ruling, derived. What is pinned here is the DERIVATION, not
// three arrangements someone liked: every layout satisfies its own mode's
// with-pairs by construction, the subject leads, a wall with nothing to
// say for the job stays shut — and the three that fall out are recorded
// exactly, so a later change to the table that moves them has to move
// this pin and say why.








/// The modes are retired (DESIGN.md §5.1, 2026-09-04): no act names one,
/// their keys `a`/`d`/`v` are free, and `0` returns the one arrangement
/// to its defaults — the palette finds it by its own words.
#[test]
fn test_the_modes_are_retired_and_their_keys_are_free() {
    for a in view::ACTIONS {
        assert!(!a.id.starts_with("mode."), "`{}` is a mode act", a.id);
        assert!(!matches!(a.key, "a" | "d" | "v"), "`{}` took a freed key", a.id);
    }
    let revert = view::action("layout.revert").expect("the escape exists");
    assert_eq!(revert.key, "0");
    assert!(view::palette_matches("defaults").iter().any(|m| m.id == "layout.revert"));
    assert!(view::action("mode.revert").is_none());
    let help = view::help_lines().join("\n");
    assert!(!help.contains("mode"), "the help says nothing about modes: {help}");
}

/// The one place the window chooses a view without being asked, and it
/// is the rule the window has had since Phase 11: a REJECTED project
/// opens with Problems showing. It is a rule about the view now, not a
/// mode: nothing else in the arrangement moves, and a clean project
/// opens exactly where it closed.
#[test]
fn test_a_refused_project_opens_on_problems_and_nothing_else_moves() {
    let mut kept = Layout::default();
    kept.apply(DockAction::ShowTab(SidebarTab::Review));
    kept.apply(DockAction::ToggleSidebar);
    kept.apply(DockAction::ShowDocument(Document::Program));
    kept.apply(DockAction::ResizeForm(500.0));
    assert_eq!(Layout::at_open(false, kept.clone()), kept, "a clean project opens where it closed");
    let opened = Layout::at_open(true, kept.clone());
    assert_eq!(opened.shown_tab(), Some(SidebarTab::Problems), "an error opens with Problems showing");
    let mut expected = kept;
    expected.apply(DockAction::ShowTab(SidebarTab::Problems));
    assert_eq!(opened, expected, "and nothing else moves");
}

// ---- the rail (Phase 13, slice 3) -----------------------------------------------
//
// The one surface whose job is discoverability, and the two properties
// that keep that honest: it is DERIVED from the inventories (so it cannot
// advertise a surface that does not exist, and cannot miss one that does),
// and it reads in the workflow order declared once in the mode table
// rather than in a fourth opinion of its own.





// ---- the ninth pane: the emitted program (Phase 13, slice 4) -------------------
//
// The phase's proof, and the house rule for what a pane may be at all: a
// view of something the terminal already prints. Two claims carry it —
// the pane's text IS `nacs`'s (one formatter, two voices), and every line
// knows which operation wrote it, which is what lets the pane highlight
// the op Phase 11's cursor is on. The attribution is DERIVED from the
// emission emitted moves, never re-parsed out of the dialect: a reader that
// re-derives the program's structure from its text is the second engine
// this project rejected once already.

/// One formatter, two voices — over the whole shipped corpus. The pane's
/// lines joined are byte-for-byte what `gcode::program` writes, and every
/// op's lines are exactly the moves the emitted moves handed out for it, in order,
/// in one contiguous run, with the header and the footer belonging to no
/// operation at all.
#[test]
fn test_the_program_document_is_the_program_nacs_writes() {
    let mut swept = 0usize;
    for entry in std::fs::read_dir(".").expect("repo root") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("cam") {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(path.join("main.part")) else { continue };
        let geom = std::fs::read_to_string(path.join("geometry.part")).ok();
        let Ok(part) = parse::parse_project_with_base(&src, geom.as_deref(), Some(&path)) else {
            continue;
        };
        let name = path.display();
        let (text, emitted_moves) = gcode::program_with_moves(&part).expect("the shipped corpus emits");
        let g = view::gcode_view(&part).expect("and so does the pane's view of it");
        swept += 1;

        // The claim the slice is FOR: the pane shows the program, not a
        // rendering of it.
        assert_eq!(g.text(), text, "{name}: the pane's text is the emitted program");
        assert_eq!(g.text(), gcode::program(&part).unwrap(), "{name}: and stdout's, too");
        assert_eq!(g.lines.len(), text.lines().count(), "{name}: one entry per line");

        // Every op's lines are the block the EMITTED MOVES says they are — its
        // tool change and title as well as its motion, which is the same
        // block the `n=` rung labels and a restart would replay. The
        // strings are only compared where the formatting boundary left
        // them alone; that it is a QUESTION is the finding (see the inch
        // pin below).
        let respelled = part.machine.as_ref().is_some_and(|m| !m.output_format_is_default());
        let mut previous_end: Option<usize> = None;
        for em in &emitted_moves {
            let span = g
                .span(em.op_idx)
                .unwrap_or_else(|| panic!("{name}: op {} has lines", em.op_idx));
            assert_eq!(span, em.lines, "{name}: op {}'s span is the emitted_moves's own", em.op_idx);
            assert!(span.len() >= em.moves.len(), "{name}: op {} holds its moves", em.op_idx);
            assert!(span.end <= g.lines.len(), "{name}: op {} lies in the text", em.op_idx);
            for line in &g.lines[span.clone()] {
                assert_eq!(line.op, Some(em.op_idx), "{name}: the block is contiguous");
            }
            // Its motion is the tail of the block, verbatim. The writer
            // stamps the `n=` label onto the block's FIRST line after the
            // emitted moves is taken, so the compare is modulo that one word.
            if !respelled {
                let tail = span.end - em.moves.len();
                for (line, mv) in g.lines[tail..span.end].iter().zip(&em.moves) {
                    assert_eq!(
                        gcode::strip_n_word(&line.text),
                        gcode::strip_n_word(mv),
                        "{name}: op {}'s line is the emitted_moves's move",
                        em.op_idx
                    );
                }
            }
            // The ops run in order and butt up against each other: one
            // op's block ends exactly where the next one's begins.
            if let Some(end) = previous_end {
                assert_eq!(span.start, end, "{name}: op {} follows the last", em.op_idx);
            }
            previous_end = Some(span.end);
        }

        // The header and the footer belong to no operation — which is why
        // a line number is not an op number, and why the model carries
        // the attribution instead of the pane counting lines.
        let attributed = g.lines.iter().filter(|l| l.op.is_some()).count();
        assert_eq!(
            attributed,
            emitted_moves.iter().map(|em| em.lines.len()).sum::<usize>(),
            "{name}: every attributed line belongs to exactly one op's block"
        );
        assert!(attributed < g.lines.len(), "{name}: the boilerplate is nobody's");
        assert_eq!(g.lines[0].op, None, "{name}: the program opens outside any op");
        assert_eq!(g.lines[0].text, "%", "{name}: on the dialect's own first line");
        assert_eq!(g.lines.last().unwrap().op, None, "{name}: and closes outside any op");
        // The two unattributed places told apart, and every line's seek
        // (Phase 52 slice 14): the header before operation 1, the trailer
        // after the last, an op's line after that op at its move; at
        // cursor 0 the header is lit and the trailer is not.
        let last = g.lines.len() - 1;
        let n = emitted_moves.len();
        assert_eq!(g.place(0), Some(view::Place::Header), "{name}");
        assert_eq!(g.place(last), Some(view::Place::Trailer), "{name}");
        assert_eq!(g.place(last + 1), None, "{name}: past the end");
        assert_eq!(g.seek(0), Some((0, None)), "{name}: a header click is before operation 1");
        assert_eq!(g.seek(last), Some((n, None)), "{name}: a trailer click is after the last op");
        let first_move = g.lines.iter().position(|l| l.block.is_some()).expect("a move");
        let op = g.lines[first_move].op.unwrap();
        assert_eq!(g.place(first_move), Some(view::Place::Op(op)), "{name}");
        assert_eq!(g.seek(first_move), Some((op + 1, g.lines[first_move].block)), "{name}: an op's line seeks after it at its move");
        assert!(g.lit(0, 0) && !g.lit(last, 0), "{name}: at cursor 0 the header is lit and the trailer is not");
        assert!(g.lit(first_move, op + 1) && !g.lit(0, op + 1), "{name}: at the op's cursor its lines are lit and the header is not");
        assert!(!g.lit(last, n), "{name}: the trailer is never lit");

        // An index past the end is no span, not a panic and not an empty
        // one — the pane asks this every frame with whatever `k` is.
        assert_eq!(g.span(emitted_moves.len()), None, "{name}: no op past the last");
    }
    assert!(swept > 40, "the corpus sweep really ran: {swept} projects");
}

/// The attribution survives the formatting boundary, which is the second
/// of the two reasons it could not be read off the text. `nacs` respells
/// every emitted line for a declared inch machine while the emitted moves stays in
/// the internal mm dialect, so an op's emitted moves and its printed lines
/// share no bytes at all — and the pane still knows whose lines they are,
/// because the emitted moves carries the RANGE and the respell rewrites each line
/// where it stands.
#[test]
fn test_the_attribution_survives_the_respell_that_the_strings_do_not() {
    let part = load("cylinder_bore_inch.cam");
    let (text, emitted_moves) = gcode::program_with_moves(&part).expect("the inch fixture emits");
    let g = view::gcode_view(&part).expect("and the pane sees it");
    assert_eq!(g.text(), text, "the pane shows the RESPELLED program, which is what nacs writes");

    let cutting = emitted_moves
        .iter()
        .find(|em| em.moves.iter().any(|m| m.starts_with("G01 X")))
        .expect("a cutting op");
    let printed: Vec<&str> =
        g.lines[cutting.lines.clone()].iter().map(|l| l.text.as_str()).collect();

    // The two spellings of one op, side by side — this is the whole
    // finding, and it is why a text match was never going to hold.
    assert!(printed.contains(&"G01 X2.5591 Y1.9685 F31.50"), "the text is in inches: {printed:?}");
    assert!(
        cutting.moves.contains(&"G01 X65.000 Y50.000 F800.000".to_string()),
        "and the emitted_moves is still in mm: {:?}",
        cutting.moves
    );
    for m in &cutting.moves {
        assert!(
            !printed.contains(&m.as_str()) || !m.contains('X'),
            "no motion line survives the respell unchanged: {m}"
        );
    }
    for line in &g.lines[cutting.lines.clone()] {
        assert_eq!(line.op, Some(cutting.op_idx), "every line of the block is still its op's");
    }
}

/// The cursor's op, and the pane's highlight, are one claim: `k` counts the
/// ops that have RUN, so the op it is on is the one it just finished —
/// nothing before the first, and the last op rather than one past it at
/// the end. The same arithmetic the outline's `past` uses, stated once
/// where the pane can read it.
#[test]
fn test_the_pane_highlights_the_op_the_scrub_is_on() {
    let part = load("plate.cam");
    let n = part.operations.len();
    assert!(n >= 2, "the fixture has ops to cursor through");
    let g = view::gcode_view(&part).expect("plate emits");

    assert_eq!(g.at_cursor(0), None, "before the first op, nothing is highlighted");
    for k in 1..=n {
        let op = g.at_cursor(k).expect("the cursor is on an op");
        assert_eq!(op, k - 1, "`k` ops have run, so the cursor is on the last of them");
        assert!(g.span(op).is_some(), "and that op has lines to highlight");
    }
    assert_eq!(g.at_cursor(n), Some(n - 1), "at the end it is the last op, not one past it");

    // The asymmetry, pinned as the CHOICE it is rather than left to be
    // rediscovered: the cursor is a boundary with `n + 1` positions and
    // the highlight is `n` ops plus nothing, so the map is a bijection —
    // total, every op reachable, and no position spare to spend on a
    // second un-highlighted end. This spends it at the bottom, because
    // the pane's question is "what cut this?" and not "what happens
    // next?". `k = n` is still "after the last op"; the pane says so in
    // words (the shell's label), and the model points at the op that
    // produced the state you are looking at.
    let states: Vec<Option<usize>> = (0..=n).map(|k| g.at_cursor(k)).collect();
    assert_eq!(states.len(), n + 1, "the slider's every position");
    for (i, a) in states.iter().enumerate() {
        assert!(!states[i + 1..].contains(a), "position {i} names a state no other position does");
    }
    assert_eq!(
        states.iter().filter(|s| s.is_none()).count(),
        1,
        "exactly one position highlights nothing"
    );
    assert_eq!(states[0], None, "and it is the bottom one — before anything ran");
    for op in 0..n {
        assert!(states.contains(&Some(op)), "op {op} is reachable by the cursor");
    }
}

/// A rejected program has no text, and the pane says the same thing the
/// terminal says in its place. This is the diagnose room's whole case for
/// the pane: the error IS what the emitter had to say about the
/// program, and it is a view of `gcode::program` either way.
#[test]
fn test_the_program_document_shows_the_refusal_the_terminal_prints() {
    let src = std::fs::read_to_string("sphere_formtool.cam/main.part").unwrap();
    let raised = src.replace(
        "run Clear(tool=T2, profile=TopDome, stepover=8.0, stepdown=5.0, stock_to_leave=0.5)\n",
        "",
    );
    assert_ne!(raised, src);
    let part =
        parse::parse_project_with_base(&raised, None, Some(std::path::Path::new("sphere_formtool.cam")))
            .unwrap();
    let error = view::gcode_view(&part).expect_err("the entry restriction rejects this program");
    assert_eq!(
        Err(error.clone()),
        gcode::program(&part),
        "the pane's error is the terminal's, word for word"
    );
    assert!(error.contains("the move `G01 Z-0.286 F200.000`"), "{error}");
}



// ---- the map floats again, and the latch comes back with it (slice 5) ---------
//
// Fork 6B. The plan map may be drawn over the viewport instead of taking
// dock space, because it is a view OF the viewport and so is the one pane
// that has to be visible at the same time as the thing it is a view of —
// the relation slice 0 proved the `with` table cannot state, since the
// viewport is not a pane. A `with` pair earns a split; this earns a float.
//
// The cost the roadmap named in advance, and the reason this section
// exists: as a dock pane the hit test is free by construction, and as an
// inset the map and the viewport share a rect, so a gesture has to be
// ASSIGNED. Phase 12 deleted a latch that lived in the shell and could not
// be pinned. This one is std-only state, and these are its pins.


/// The inset's geometry: a square in the bottom-right corner, a share of
/// the viewport's short side, clamped at both ends, always inside the
/// viewport with its margin — and NOTHING at all when the viewport cannot
/// hold it honestly, which is also when the latch has nothing to claim.
#[test]
fn test_the_inset_is_a_square_in_the_corner_and_nothing_when_it_cannot_be() {
    for (w, h) in [(1200.0_f32, 800.0_f32), (800.0, 1200.0), (400.0, 400.0), (3000.0, 2000.0)] {
        let r = view::map_inset_rect(w, h).unwrap_or_else(|| panic!("{w}x{h} holds a map"));
        let [x, y, iw, ih] = r;
        assert_eq!(iw, ih, "{w}x{h}: the map is square — the plan is not the window's shape");
        assert!(iw >= view::MAP_MIN_SIDE && iw <= view::MAP_MAX_SIDE, "{w}x{h}: clamped: {iw}");
        assert!(x >= 0.0 && y >= 0.0, "{w}x{h}: inside the viewport");
        assert!(x + iw <= w && y + ih <= h, "{w}x{h}: and inside it at the far corner");
        // Bottom-RIGHT, at the declared margin.
        assert!((w - (x + iw) - view::MAP_PAD).abs() < 1e-4, "{w}x{h}: pinned to the right edge");
        assert!((h - (y + ih) - view::MAP_PAD).abs() < 1e-4, "{w}x{h}: and the bottom one");
        // The hit test agrees with the rect it was given.
        assert!(view::in_rect(r, x + iw / 2.0, y + ih / 2.0), "the centre is in");
        assert!(!view::in_rect(r, x - 1.0, y + ih / 2.0), "a point left of it is out");
        assert!(!view::in_rect(r, x + iw / 2.0, y + ih + 1.0), "and below it");
    }
    // A window dragged narrow gets no map rather than a sliver of one.
    assert_eq!(view::map_inset_rect(2.0 * view::MAP_PAD, 500.0), None, "no room, no map");
    assert_eq!(view::map_inset_rect(0.0, 0.0), None);
    assert_eq!(view::map_inset_rect(f32::NAN, 100.0), None, "and a nonsense size is not a map");
}





/// The workspace file's `mode` line (and its older spelling `task`) is a
/// line from the modes era: unknown now, dropped like any other, and the
/// file written today never carries one.
#[test]
fn test_a_mode_line_is_unknown_and_drops() {
    assert_eq!(Layout::parse("task verify\n"), Layout::default());
    assert_eq!(Layout::parse("mode verify\n"), Layout::default());
    assert!(!Layout::default().render().contains("mode "));
}


/// A gesture is the map's exactly when it BEGAN on the inset — decided once
/// from the press origin, so a drag that crosses the map keeps orbiting and
/// a drag that leaves it keeps steering; and no inset means no map gesture.
#[test]
fn test_a_gesture_is_the_map_s_only_when_it_began_on_it() {
    let r = view::map_inset_rect(800.0, 600.0).expect("a viewport that holds the map");
    let (cx, cy) = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
    assert!(view::map_owns_gesture(Some(r), cx, cy));
    assert!(!view::map_owns_gesture(Some(r), r[0] - 1.0, cy), "a press beside it is the viewport's");
    assert!(!view::map_owns_gesture(None, cx, cy), "no inset, no map gesture");
}

/// I9: the layers are data — each has a word for the checkbox, a token for
/// the file, and an act the palette can name — and the default set is
/// today's picture: everything the viewport always drew on, the two
/// worker-built layers off until asked for.
#[test]
fn test_the_layers_default_to_today_s_picture_and_every_layer_has_a_word() {
    use nacs::theme::{Layer, Layers};
    let d = Layers::default();
    assert!(d.outlines && d.toolpaths && d.declared_stock && d.fixtures && d.tool && d.annotations);
    assert!(!d.simulated_stock && !d.rest_material, "a thread each: off until asked for");
    let mut labels: Vec<&str> = Layer::ALL.iter().map(|l| l.label()).collect();
    labels.sort();
    labels.dedup();
    assert_eq!(labels.len(), Layer::ALL.len(), "every label is its own");
    for l in Layer::ALL {
        assert_eq!(Layer::from_key(l.key()), Some(l), "the token round-trips");
        assert!(view::action(l.action_id()).is_some(), "{} has an act the palette can name", l.label());
        let mut on = Layers::default();
        on.set(l, !on.get(l));
        assert_ne!(on.get(l), d.get(l));
        on.toggle(l);
        assert_eq!(on, d, "toggle twice is the identity");
    }
    assert_eq!(Layers::from_on(&d.on()), d, "the ON list is the whole state");
    assert_eq!(Layers::from_on(&[]).on(), vec![]);
}

/// I8: the transport bar's label names where the cursor stands and what
/// just ran, and the cursor cannot pass the last op.
#[test]
fn test_the_transport_line_names_the_op_the_cursor_is_on() {
    let p = load("plate.cam");
    let n = p.operations.len();
    assert!(n > 0);
    assert_eq!(view::Transport::clamp(n + 5, n), n);
    assert_eq!(view::Transport { cursor: 0, count: n, block: None, blocks: 0 }.line(&p), format!("Shown before any operation — 0 of {n} run"));
    let last = view::Transport { cursor: n, count: n, block: None, blocks: 0 }.line(&p);
    assert!(last.starts_with(&format!("Shown through [{}] ", n - 1)) && last.ends_with(&format!("— {n} of {n} run")), "{last}");
    let first = view::Transport { cursor: 1, count: n, block: None, blocks: 0 }.line(&p);
    assert!(first.contains(&format!("'{}'", p.operations[0].feature_name)), "{first}");
    assert!(first.contains(&format!("T{}", p.operations[0].tool_num)), "{first}");
    assert_eq!(view::Transport { cursor: 0, count: 0, block: None, blocks: 0 }.line(&p), "No operations");
    // The bar and the program view agree on which op the cursor is ON.
    let g = view::gcode_view(&p).expect("plate emits");
    assert_eq!(g.at_cursor(n), Some(n - 1));
}

/// I10: the status line is one line, busy work speaks first, notes follow,
/// and it is empty when nothing is happening.
#[test]
fn test_the_status_line_is_one_line_and_busy_work_speaks_first() {
    use view::{status_line, unsent_note, StatusParts};
    assert_eq!(status_line(&StatusParts { busy: None, notes: &[] }), "");
    let s = status_line(&StatusParts { busy: Some("Simulating stock removal…"), notes: &["Could not load theme 'x'"] });
    assert!(s.starts_with("Simulating stock removal…"), "{s}");
    assert!(s.contains(" · Could not load theme 'x'"), "{s}");
    assert!(!status_line(&StatusParts { busy: None, notes: &["two\nlines"] }).contains('\n'));
    assert_eq!(status_line(&StatusParts { busy: None, notes: &[""] }), "", "an empty note is no note");
    // The selection is the line's OTHER segment (DESIGN.md §5.7, Phase 20
    // slice 0): the parts carry no selection field at all, so no caller
    // can join it into the notes' sentence.
    assert_eq!(unsent_note("stepover", "5"), "Unsent — stepover = 5: Enter writes it, Esc drops it");
}


/// I6: the form's card is a pure function of the selection, and the three
/// ways of selecting agree with what the viewport lights.
#[test]
fn test_the_form_card_is_a_function_of_the_selection() {
    use view::{Card, Selection};
    let p = load("plate.cam");
    let outline = nacs::outline::outline(&p);
    let v = check_result::check_result(&p);
    let (rows, annotations) = view::finding_rows(&v, p.stock.z, &Theme::default());
    let edges = view::feature_edges(&p);
    let paths = view::op_paths(&p, v.emitted_moves.as_deref().expect("plate emits"));
    assert_eq!(view::card(&p, &outline, Selection::None, &rows, &edges, &paths, &[], None), Card::Nothing);
    // An operation: its outline line, its removal line, its tool at the
    // deepest emitted point dropped to the tip.
    match view::card(&p, &outline, Selection::Operation(0), &rows, &edges, &paths, &[], None) {
        Card::Operation { line, removal, tool, .. } => {
            let op = outline.features.iter().flat_map(|f| f.ops.iter()).find(|o| o.idx == 0).expect("op 0 in the outline");
            assert_eq!(line, nacs::outline::op_line(&p, op));
            assert_eq!(removal, nacs::outline::removal_line(&p, 0));
            let card = tool.expect("op 0 has a tool");
            assert_eq!(card.number, p.operations[0].tool_num);
            let expected = view::tool_pose(&p, Selection::Operation(0), &rows, &paths).expect("a pose");
            assert_eq!((card.tip, card.number), expected);
            let path = paths.iter().find(|pa| pa.op_idx == 0).unwrap();
            let deepest = view::deepest_point(path).unwrap();
            assert!(card.tip[2] <= deepest[2], "the tip is the emitted point dropped by the silhouette");
        }
        other => panic!("an operation, not {other:?}"),
    }
    // A feature: its template, the same lines `nacs --template` prints.
    match view::card(&p, &outline, Selection::Feature(0), &rows, &edges, &paths, &[], None) {
        Card::Feature { name, kind, template, .. } => {
            assert_eq!(name, edges[0].name);
            assert_eq!(kind, edges[0].kind);
            assert_eq!(template, nacs::template::op_lines(&p, &edges[0].name));
        }
        other => panic!("a feature, not {other:?}"),
    }
    assert_eq!(view::tool_pose(&p, Selection::Feature(0), &rows, &paths), None, "a feature stands no tool");
    assert_eq!(view::selected_feature(Selection::Feature(0), &rows, &edges), Some(0));
    assert_eq!(view::selected_op(Selection::Feature(0), &rows), None);
    assert_eq!(view::selected_op(Selection::Operation(2), &rows), Some(2));
    assert_eq!(view::selection_anchor(Selection::Operation(2), &rows, &annotations), None, "selecting an op moves nothing");
    // Out of range is nothing, not a panic.
    assert_eq!(view::card(&p, &outline, Selection::Feature(999), &rows, &edges, &paths, &[], None), Card::Nothing);
    assert_eq!(view::card(&p, &outline, Selection::Finding(999), &rows, &edges, &paths, &[], None), Card::Nothing);
    // A tool (Phase 20 slice 1): its Outline row — the words `--outline`
    // prints — its body, its line's form; no tip, no op, no feature.
    match view::card(&p, &outline, Selection::Tool(5), &rows, &edges, &paths, &[], None) {
        Card::Tool { number, row, form, .. } => {
            assert_eq!(number, 5);
            assert_eq!(row, nacs::outline::tool_row(&outline.tools[0]));
            assert!(row.starts_with("T5 Endmill dia 10.000  runs [0] [1]"), "{row}");
            assert_eq!(form, view::statement_form(&p, &view::Statement::Tool(5)));
        }
        other => panic!("a tool, not {other:?}"),
    }
    assert_eq!(view::tool_pose(&p, Selection::Tool(5), &rows, &paths), None, "a declaration stands nowhere");
    assert_eq!(view::selected_op(Selection::Tool(5), &rows), None);
    assert_eq!(view::selected_feature(Selection::Tool(5), &rows, &edges), None);
    assert_eq!(view::selection_sentence(&p, &outline, Selection::Tool(5), &rows, &edges, &[]), nacs::outline::tool_row(&outline.tools[0]));
    assert_eq!(view::card(&p, &outline, Selection::Tool(99), &rows, &edges, &paths, &[], None), Card::Nothing, "an undeclared number is nothing");
}

/// One selection lights the op and the feature a problem names, moves the
/// camera to its annotation, and shows the problem's own words in the
/// inspector — all from the row's typed fields.
#[test]
fn test_one_selection_lights_the_op_and_the_feature_a_problem_names() {
    use view::{Card, Selection};
    // The first shipped fixture with a problem about an op that names a
    // place — the corpus has two dozen (the entry flags).
    let mut found = None;
    for entry in std::fs::read_dir(".").expect("repo root") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("cam") {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(path.join("main.part")) else { continue };
        let geom = std::fs::read_to_string(path.join("geometry.part")).ok();
        let Ok(p) = parse::parse_project_with_base(&src, geom.as_deref(), Some(&path)) else { continue };
        let v = check_result::check_result(&p);
        let (rows, _) = view::finding_rows(&v, p.stock.z, &Theme::default());
        if rows.iter().any(|r| r.op.is_some() && r.annotation.is_some()) {
            found = Some(p);
            break;
        }
    }
    let p = found.expect("a fixture with a problem about an op, with a place");
    let outline = nacs::outline::outline(&p);
    let v = check_result::check_result(&p);
    let (rows, annotations) = view::finding_rows(&v, p.stock.z, &Theme::default());
    let edges = view::feature_edges(&p);
    let paths = view::op_paths(&p, v.emitted_moves.as_deref().expect("emits"));
    let (i, row) = rows.iter().enumerate().find(|(_, r)| r.op.is_some() && r.annotation.is_some()).expect("a problem about an op, with a place");
    let sel = Selection::Finding(i);
    assert_eq!(view::selected_op(sel, &rows), row.op);
    assert_eq!(view::selected_feature(sel, &rows, &edges), edges.iter().position(|e| e.name == row.feature));
    assert_eq!(view::selection_anchor(sel, &rows, &annotations), Some(annotations[row.annotation.unwrap()].anchor()));
    assert_eq!(view::selection_sentence(&p, &outline, sel, &rows, &edges, &[]), row.head);
    match view::card(&p, &outline, sel, &rows, &edges, &paths, &[], None) {
        Card::Finding { head, text, op_line, feature, .. } => {
            assert_eq!(head, row.head);
            assert_eq!(text, row.text);
            assert!(op_line.is_some());
            assert_eq!(feature, row.feature);
        }
        other => panic!("a problem, not {other:?}"),
    }
}












/// The window's surfaces are data with a word each: the two sidebar views
/// and the two documents have a title, a file token that round-trips, an
/// act the palette can name, and a sentence — and every act in the
/// inventory has a help line.
#[test]
fn test_every_view_and_document_has_a_word_and_an_act() {
    for t in SidebarTab::ALL {
        assert_eq!(SidebarTab::from_key(t.key()), Some(t));
        let a = view::action(t.action_id()).unwrap_or_else(|| panic!("`{}` has an act", t.title()));
        assert!(a.sentence.contains(t.title()), "`{}` names the view it shows", a.id);
        assert!(a.key.is_empty(), "`{}` ships with no key of its own", a.id);
        assert!(view::palette_matches(t.title()).iter().any(|m| m.id == t.action_id()), "the palette finds `{}` by name", t.title());
    }
    for d in Document::ALL {
        assert_eq!(Document::from_key(d.key()), Some(d));
        let a = view::action(d.action_id()).unwrap_or_else(|| panic!("`{}` has an act", d.title()));
        assert!(a.sentence.contains(d.title()));
        assert!(view::palette_matches(d.title()).iter().any(|m| m.id == d.action_id()));
    }
    assert_eq!(SidebarTab::from_key("gcode"), None, "the program is not a sidebar view");
    assert_eq!(Document::from_key("findings"), None, "and the problems are not a document");
    let help = view::help_lines().join("\n");
    for t in SidebarTab::ALL {
        assert!(help.contains(t.title()), "the help names `{}`", t.title());
    }
    for d in Document::ALL {
        assert!(help.contains(d.title()), "the help names `{}`", d.title());
    }
    for a in view::ACTIONS {
        assert!(help.contains(a.sentence), "the help says what `{}` does", a.id);
    }
    for voice in ["--outline", "--check", "--rest-report", "--template", "--theme-check"] {
        assert!(help.contains(voice), "the help names the terminal voice `{voice}`");
    }
    assert!(help.contains("the tabs"), "and the tabs");
}

/// I13: no UI string uses the house vocabulary. Every sentence the window
/// can put on screen from std-only data — the help, the action sentences,
/// the mode sentences, the views' and documents' titles and sentences — is
/// swept for the words the spec retired. The shell's own literals are
/// untested by charter and are checked by eye against DESIGN.md §3.2.
#[test]
fn test_no_ui_string_speaks_the_house_vocabulary() {
    let banned = [
        "scrub", "stand", "stands", "worn", "wear", "wears", "picture", "wire", "wires", "rim",
        "rims", "blank", "latch", "grab", "tap", "tapped", "room", "wall", "cram", "crams",
        "verdict", "refusal", "refused", "refuses", "dock", "docks", "stack", "stacks",
    ];
    let mut strings: Vec<String> = view::help_lines();
    strings.extend(view::ACTIONS.iter().map(|a| a.sentence.to_string()));
    strings.extend(SidebarTab::ALL.iter().flat_map(|t| [t.title().to_string(), t.sentence().to_string()]));
    strings.extend(Document::ALL.iter().flat_map(|d| [d.title().to_string(), d.sentence().to_string()]));
    strings.extend(view::tabs().iter().map(|s| s.label.to_string()));
    for s in &strings {
        for w in s.split(|c: char| !c.is_alphanumeric()) {
            let w = w.to_ascii_lowercase();
            assert!(!banned.contains(&w.as_str()), "house vocabulary on screen: {w:?} in {s:?}");
        }
    }
}

/// The branch chip (DESIGN.md §5.3, Phase 52 slice 7): its composition is
/// pure and pinned; and on a machine with git, this repository's head
/// reads — a branch with a name. Without git the reader says why.
#[test]
fn test_the_branch_chip_names_the_project_and_the_head_when_there_is_one() {
    let ok = Ok(view::GitHead { branch: "main".into(), changed: 0 });
    assert_eq!(view::git_chip("plate.cam", Some(&ok)).0, "plate.cam · main");
    let dirty = Ok(view::GitHead { branch: "claude/x".into(), changed: 3 });
    let (text, hover) = view::git_chip(if cfg!(windows) { "C:\\work\\disc.cam" } else { "/work/disc.cam" }, Some(&dirty));
    assert_eq!(text, "disc.cam · claude/x*");
    assert!(hover.contains("3 paths changed"), "{hover}");
    assert_eq!(view::git_chip("plate.cam", None).0, "plate.cam");
    let none = Err("not a git repository".to_string());
    let (text, hover) = view::git_chip("plate.cam", Some(&none));
    assert_eq!(text, "plate.cam");
    assert!(hover.contains("not a git repository"), "{hover}");
    // The reader, on this repository, when git is in reach.
    let have_git = std::process::Command::new("git").arg("--version").output().is_ok_and(|o| o.status.success());
    if have_git {
        let head = view::git_head(std::path::Path::new("plate.cam")).expect("this repository has a head");
        assert!(!head.branch.is_empty());
    }
    let nowhere = std::env::temp_dir();
    // The system's temp directory is not a repository (or the reader says
    // it is — either way it answers, never panics).
    let _ = view::git_head(&nowhere);
}

/// The stock at the move (DESIGN.md §5.6, Phase 52 slice 6): the field at
/// move grain stamps the ops before whole and the cursor's op through the
/// block's move — through its last move it IS the op's whole field; through
/// its first feed move it holds fewer stamps and still builds a surface.
/// An op under control comp declines by name. Timed, for the entry.
#[test]
fn test_the_stock_at_a_move_is_the_prefix_through_that_move() {
    let part = parse::load_project("plate.cam").expect("plate.cam parses");
    let cr = check_result::check_result(&part);
    let emitted = cr.emitted_moves.expect("plate emits");
    let em0 = emitted.iter().find(|e| e.op_idx == 0).expect("op 0");
    let full = nacs::vox::VoxField::build_prefix(&part, 1, None).expect("the first op's field");
    let last = em0.moves.len() - 1;
    let through_last = nacs::vox::VoxField::build_prefix_moves(&part, &emitted, 0, Some(last), None).unwrap();
    assert_eq!(through_last.stamp_count(), full.stamp_count(), "through the last move is the whole op");
    assert!(through_last.complete());
    let first_feed = em0.moves.iter().position(|l| l.contains("G01")).expect("a feed move");
    let t0 = std::time::Instant::now();
    let early = nacs::vox::VoxField::build_prefix_moves(&part, &emitted, 0, Some(first_feed), None).unwrap();
    let t_field = t0.elapsed();
    assert!(early.stamp_count() >= 1 && early.stamp_count() < full.stamp_count(), "{} of {} stamps through the first feed move", early.stamp_count(), full.stamp_count());
    let t0 = std::time::Instant::now();
    let surface = view::stock_surface_prefix(&part, &emitted, 1, Some(first_feed), None);
    let t_surface = t0.elapsed();
    assert!(surface.is_ok(), "{:?}", surface.err());
    eprintln!("stock at a move (plate.cam, default cell): field {t_field:?}, field+surface {t_surface:?}");
    // No block: the op-grain prefix, unchanged.
    assert!(view::stock_surface_prefix(&part, &emitted, 1, None, None).is_ok());
    // A control-comp op stamps at move grain through the resolved move its
    // text's foot is on (slice 10): fewer stamps than the whole op, a
    // complete field, and the surface builds.
    // plate's Side is op 2 since Phase 53 slice 1 (the notch's Clear is 1).
    let em1 = emitted.iter().find(|e| e.op_idx == 2).expect("op 2");
    assert!(em1.resolved.is_some());
    let whole = nacs::vox::VoxField::build_prefix(&part, 3, None).unwrap();
    let first_cut = em1.moves.iter().position(|l| l.starts_with("G01") && l.contains("G41")).expect("the first compensated move");
    let (k, _) = view::resolved_foot(&emitted, 2, first_cut).expect("a foot");
    let partial = nacs::vox::VoxField::build_prefix_moves(&part, &emitted, 2, Some(k), None).unwrap();
    assert!(partial.complete(), "stamped at move grain, by the foot: {:?}", partial.incomplete_reasons_text());
    assert!(partial.stamp_count() < whole.stamp_count() && partial.stamp_count() >= 1, "{} of {}", partial.stamp_count(), whole.stamp_count());
    assert!(view::stock_surface_prefix(&part, &emitted, 3, Some(first_cut), None).is_ok());
}

/// The tool at the move (DESIGN.md §5.6, Phase 52 slice 5): `block_end` is
/// the end point of the move the block cursor names, the modal motion
/// replayed over the op's text moves; `block_pose` stands the tool there
/// only when that operation is the cursor's and the selection's; an op
/// under control comp declines by name (its text is the contour).
#[test]
fn test_the_tool_stands_at_the_end_of_the_move_the_block_names() {
    let part = parse::load_project("plate.cam").expect("plate.cam parses");
    let cr = nacs::check_result::check_result(&part);
    let emitted = cr.emitted_moves.expect("plate emits");
    let em0 = emitted.iter().find(|e| e.op_idx == 0).expect("op 0");
    assert!(em0.resolved.is_none(), "plate's first op is not under control comp");
    // Replay by hand to the first feed move and compare.
    let b = em0.moves.iter().position(|l| l.contains("G01")).expect("a feed move");
    let mut m = nacs::gcode::Motion { rapid: true, x: None, y: None, z: None, f: None };
    for line in &em0.moves[..=b] {
        nacs::gcode::apply_move(line, &mut m);
    }
    let end = view::block_end(&emitted, 0, b).expect("a position");
    assert_eq!(end, [m.x.unwrap(), m.y.unwrap(), m.z.unwrap()]);
    assert_eq!(view::block_end(&emitted, 0, em0.moves.len()), None, "past the op's last move");
    assert_eq!(view::block_end(&emitted, 99, 0), None, "no such op");
    let tool = part.operations[0].tool_num;
    let drop = view::Silhouette::of(&part.tools[&tool]).drop();
    // No selection in the question (Phase 52 slice 14): the block cursor
    // is the cursor's fact, and the pose follows it whatever is selected.
    assert_eq!(view::block_pose(&part, &emitted, 1, Some(b)), Some(([end[0], end[1], end[2] - drop], tool)));
    assert_eq!(view::block_pose(&part, &emitted, 1, None), None, "no block: the deepest point's turn");
    // The cursor on another op names THAT op's move — the selection used
    // to answer None here, which was the guard and not the cursor.
    let side_end = view::block_end(&emitted, 1, b).expect("op 1 (the notch's Clear since Phase 53 slice 1) has that many moves");
    let side_tool = part.operations[1].tool_num;
    let side_drop = view::Silhouette::of(&part.tools[&side_tool]).drop();
    assert_eq!(view::block_pose(&part, &emitted, 2, Some(b)), Some(([side_end[0], side_end[1], side_end[2] - side_drop], side_tool)), "the cursor on another op: that op's move");
    assert_eq!(view::block_pose(&part, &emitted, 2, Some(9_999)), None, "a move the op does not have");
    // A control-comp op answers with the FOOT (slice 10): plate's Side is
    // one. The parallel is the contour offset by r INTO the pocket, so at
    // the end of a text move the foot stands r off on a straight stretch
    // and, at a right-angle inside corner, at the miter — r·√2 off, which
    // is where the controller's compensated path turns; a reflex corner
    // (the L's, until Phase 53 slice 1 made the loop the notch's rectangle)
    // carries an ARC, flattened at CHORD_TOL_MM, its chord reading r within
    // one sagitta — the tolerance stays for that case. The Side is op 2
    // since slice 1, after the notch's Clear.
    let em1 = emitted.iter().find(|e| e.op_idx == 2).expect("op 2");
    assert!(em1.resolved.is_some(), "plate's Side is under control comp");
    let r = part.tools[&part.operations[2].tool_num].dia / 2.0;
    let mut checked = 0;
    for (b, line) in em1.moves.iter().enumerate() {
        if !line.starts_with("G01") || line.contains("G40") || line.contains('Z') {
            continue;
        }
        let text_end = {
            let mut m = nacs::gcode::Motion { rapid: true, x: None, y: None, z: None, f: None };
            for l in &em1.moves[..=b] {
                nacs::gcode::apply_move(l, &mut m);
            }
            [m.x.unwrap(), m.y.unwrap(), m.z.unwrap()]
        };
        let (k, foot) = view::resolved_foot(&emitted, 2, b).expect("a foot");
        assert!(k < em1.resolved.as_ref().unwrap().len());
        let d = ((foot[0] - text_end[0]).powi(2) + (foot[1] - text_end[1]).powi(2)).sqrt();
        let miter = r * std::f64::consts::SQRT_2;
        assert!((d - r).abs() <= CHORD_TOL_MM + 1e-6 || (d - miter).abs() < 1e-3, "move {b} {line:?}: the foot stands {d:.4} off the vertex — the radius is {r:.4} (within the chord tolerance on an arc), a right-angle miter {miter:.4}");
        assert!((foot[2] - text_end[2]).abs() < 1e-6, "at the cut's height");
        assert_eq!(view::block_end(&emitted, 2, b), Some(foot), "block_end answers with the foot under control comp");
        checked += 1;
    }
    assert!(checked >= 3, "the loop has several moves to check: {checked}");
    assert_eq!(view::resolved_foot(&emitted, 0, 0), None, "an engine-comp op has no foot to find — block_end's text arm answers");
}

/// The stock from a model (Phase 54 slice 5b, fork 3A): the runner's `stock`
/// verb is the sidecar's `import_stock.py` on this project; the standing
/// stock statement — a line or a block — is replaced by the proposal's
/// block with every other byte kept; a line that is not the stock, and a
/// proposal without one, refuse by name; the act and its doors stand.
#[test]
fn test_the_stock_from_a_model_replaces_the_stock_statement_and_nothing_else() {
    let dir = std::path::Path::new("plate.cam");
    let sidecar = std::path::Path::new("sidecar");
    let (python, args) = view::stock_command(sidecar, std::path::Path::new("tests/fixtures/l_plate.step"), dir, &view::ImportSetup { up: Some("z-".into()), solid: String::new(), allowance: String::new(), given: String::new(), origin: String::new() }).unwrap();
    assert!(python.to_string_lossy().contains("python"), "{}", python.display());
    assert!(args[0].ends_with("import_stock.py") && args[1].ends_with("l_plate.step") && args[2] == "-o" && args[4..] == ["--up".to_string(), "z-".to_string()], "{args:?}");
    // The setup unsaid proposes the blank 1 mm outside the model (slice 11).
    let (_, args) = view::stock_command(sidecar, std::path::Path::new("tests/fixtures/l_plate.step"), dir, &view::ImportSetup::default()).unwrap();
    assert_eq!(args[4..6], ["--allowance".to_string(), "1".to_string()], "{args:?}");
    assert!(view::stock_command(sidecar, std::path::Path::new("tests/fixtures/l_plate.stp.txt"), dir, &view::ImportSetup::default()).unwrap_err().contains("not a STEP file"));
    // The runner's verb builds the same spec, or says why not.
    let spec = view::runner_command("stock tests/fixtures/l_plate.step", dir, std::path::Path::new("nacs.exe"), Some(sidecar), None);
    match spec {
        // The STEP as typed is relative to the window's cwd; the process
        // runs in the project directory, so it is handed over absolute.
        Ok(Some(s)) => assert!(s.args[0].ends_with("import_stock.py") && std::path::Path::new(&s.args[1]).is_absolute() && s.args[1].ends_with("l_plate.step"), "{:?}", s.args),
        Ok(None) => panic!("a stock line is a run"),
        Err(e) => assert!(e.contains("venv is not set up"), "{e}"),
    }
    assert!(view::runner_command("stock", dir, std::path::Path::new("nacs.exe"), Some(sidecar), None).unwrap_err().contains("stock takes a STEP file"));
    // The replacement, on plate.cam's main.part: the Box line becomes the
    // block, every other line stays.
    let main = std::fs::read_to_string("plate.cam/main.part").unwrap();
    let p = parse::load_project("plate.cam").unwrap();
    let (_, line, _) = p.stock_line.clone().unwrap();
    // The proposal in plate.cam's own frame (top at z 0): a real proposal
    // carries the model's frame, and the features must agree with it — the
    // Target import's own contract.
    let proposal = "# from l_plate.step\nstock Profile(h=20.0, z=0.0\n    0.0 0.0\n    120.0 0.0\n    120.0 80.0\n    60.0 80.0\n    60.0 40.0\n    0.0 40.0\n)\n";
    let after = view::replace_stock_statement(&main, line, proposal).unwrap();
    let m: Vec<&str> = main.lines().collect();
    let a: Vec<&str> = after.lines().collect();
    assert_eq!(a.len(), m.len() + 8, "one line out, nine in (a comment, the opener, six points, the close)");
    assert_eq!(&a[..line], &m[..line], "the lines above stay");
    assert_eq!(a[line], "# from l_plate.step");
    assert_eq!(a[line + 1], "stock Profile(h=20.0, z=0.0");
    assert_eq!(a[line + 8], ")");
    assert_eq!(&a[line + 9..], &m[line + 1..], "the lines below stay");
    let q = parse::parse_project_with_base(&after, Some(&std::fs::read_to_string("plate.cam/geometry.part").unwrap()), Some(dir)).unwrap();
    assert!(matches!(q.stock.shape, parse::StockShape::Profile { .. }));
    // A block replaced by a line-long proposal: the block leaves whole.
    let back = view::replace_stock_statement(&after, line + 1, "stock Box(w=120, l=80, h=20, x=0, y=0, z=0)").unwrap();
    assert_eq!(back.lines().nth(line + 1), Some("stock Box(w=120, l=80, h=20, x=0, y=0, z=0)"));
    assert_eq!(back.lines().count(), m.len() + 1, "the comment line the proposal brought stays above");
    // Line 1 is `units mm`, a statement that is not the stock (line 0 is a
    // comment, which `statement_extent` refuses first).
    assert!(view::replace_stock_statement(&main, 1, proposal).unwrap_err().contains("is not the stock statement"));
    assert!(view::replace_stock_statement(&main, line, "# nothing here").unwrap_err().contains("carries no stock statement"));
    assert!(view::action("stock.new").is_some_and(|a| a.key.is_empty()));
    assert!(view::doors_of("stock.new").contains(&"menu"));
}

/// Git's one table (Phase 54 slice 3, fork 2A): the reading verbs and the
/// five that write run on the project's repository; `commit -m` takes the
/// words after it as the message and commits the project directory's
/// pathspec; `switch -c` makes a branch; what rewrites history or
/// discards work is refused by name, verb and flag alike; the five acts
/// are in the inventory, on the chip's menu and the corner menu.
#[test]
fn test_gits_verbs_are_one_table_and_nothing_here_rewrites_history() {
    let dir = std::path::Path::new("plate.cam");
    let d = dir.to_string_lossy().into_owned();
    let g = |words: &[&str]| view::git_spec(words, dir);
    assert_eq!(g(&["fetch"]).unwrap().args, vec!["-C".to_string(), d.clone(), "fetch".to_string()]);
    assert_eq!(g(&["push"]).unwrap().echo, "git push");
    assert_eq!(g(&["status", "--short"]).unwrap().args[2..], ["status".to_string(), "--short".to_string()]);
    let c = g(&["commit", "-m", "the", "notch", "is", "the", "void"]).unwrap();
    assert_eq!(c.args[2..], ["commit".to_string(), "-m".to_string(), "the notch is the void".to_string(), "--".to_string(), ".".to_string()]);
    assert_eq!(c.echo, "git commit -m the notch is the void");
    assert!(g(&["commit"]).unwrap_err().contains("takes -m <message>"));
    assert!(g(&["commit", "-m"]).unwrap_err().contains("takes a message after it"));
    assert_eq!(g(&["switch", "-c", "lane"]).unwrap().args[2..], ["switch".to_string(), "-c".to_string(), "lane".to_string()]);
    for refused in [&["push", "--force"][..], &["push", "-f"], &["commit", "--amend", "-m", "x"], &["branch", "-D", "x"], &["checkout", "--", "main.part"], &["reset", "--hard"], &["rebase", "main"], &["stash"], &["clean", "-fd"]] {
        let e = g(refused).unwrap_err();
        assert!(e.contains("refused by name") || e.contains("not a verb the window runs"), "{refused:?}: {e}");
    }
    assert!(g(&[]).unwrap_err().contains("git takes a verb"));
    // The typed line and the band share the table.
    let typed = view::runner_command("git push --force", dir, std::path::Path::new("nacs.exe"), None, None).unwrap_err();
    assert!(typed.contains("refused by name"), "{typed}");
    for (id, _) in view::GIT_DOORS {
        assert!(view::action(id).is_some_and(|a| a.key.is_empty()), "{id}");
        assert!(view::doors_of(id).contains(&"menu"), "{id} has a door in the corner menu");
    }
    assert!(view::git_chip("plate.cam", Some(&Ok(view::GitHead { branch: "main".into(), changed: 0 }))).1.contains("click for fetch"));
}

/// The runner's table (Phase 54 slice 2, fork 1A): `nacs <flags>` is this
/// binary on this project (the project appended unless the flag stands
/// alone), `git` the read-only verbs on the repository, `import` the
/// sidecar; the window's own verbs, the writing git verbs and anything
/// else refuse by name; an empty line is nothing; the `nacs` the runner
/// finds from a test binary is the one cargo built.
#[test]
fn test_the_runner_knows_the_houses_verbs_and_refuses_the_rest_by_name() {
    let dir = std::path::Path::new("plate.cam");
    let nacs = std::path::Path::new("nacs.exe");
    let run = |line: &str| view::runner_command(line, dir, nacs, None, None);
    let s = run("nacs --outline").unwrap().unwrap();
    assert_eq!((s.program.as_path(), s.cwd.as_path(), s.echo.as_str()), (nacs, dir, "nacs --outline"));
    assert_eq!(s.args, vec!["--outline".to_string(), dir.to_string_lossy().into_owned()]);
    assert_eq!(run("  nacs   --rest-report ").unwrap().unwrap().echo, "nacs --rest-report", "the echo is the words, once-spaced");
    assert_eq!(run("nacs --reference").unwrap().unwrap().args, vec!["--reference".to_string()], "a standalone flag takes no project");
    // Phase 56 slice 3 (fork 5A): a bare flag is nacs's, in every respect.
    let bare = run("--rest-report").unwrap().unwrap();
    let full = run("nacs --rest-report").unwrap().unwrap();
    assert_eq!((bare.program, bare.args, bare.cwd, bare.echo.as_str()), (full.program, full.args, full.cwd, "nacs --rest-report"), "the echo says the verb it took");
    assert!(run("--view").unwrap_err().contains("are not runs"), "the same refusals");
    assert_eq!(run("--reference").unwrap().unwrap().args, vec!["--reference".to_string()], "the same standalone flags");
    assert!(run("-o out.nc").unwrap_err().contains("`-o` is not a verb the runner knows"), "one dash is not read");
    assert!(run("nacs --view").unwrap_err().contains("are not runs"));
    assert!(run("nacs --outline --mcp").unwrap_err().contains("are not runs"));
    let g = run("git status --short").unwrap().unwrap();
    assert_eq!(g.program, std::path::PathBuf::from("git"));
    assert_eq!(g.args, vec!["-C".to_string(), dir.to_string_lossy().into_owned(), "status".to_string(), "--short".to_string()]);
    assert!(run("git push").is_ok(), "push is a verb since Phase 54 slice 3");
    assert!(run("git push --force").unwrap_err().contains("refused by name"));
    assert!(run("git").unwrap_err().contains("git takes a verb"));
    assert!(run("import part.step").unwrap_err().contains("no sidecar in reach"));
    assert!(run("import").unwrap_err().contains("import takes a STEP file"));
    assert!(run("frobnicate the thing").unwrap_err().contains("`frobnicate` is not a verb the runner knows"));
    assert_eq!(run("   ").unwrap(), None, "an empty line is nothing");
    // Cargo leaves an `nacs.exe` in `deps/` beside the hashed one and the
    // copy a level up: the first found on the way up is one of cargo's.
    let found = view::nacs_exe().expect("the nacs cargo built, beside this test binary's parent");
    let built = std::path::Path::new(env!("CARGO_BIN_EXE_nacs"));
    assert_eq!(found.file_name(), built.file_name(), "{}", found.display());
    assert!(found.is_file() && found.starts_with(built.parent().unwrap()), "{} is cargo's, under {}", found.display(), built.parent().unwrap().display());
    let mut log = view::OutputLog::default();
    log.append(1.0, "same");
    log.append(1.0, "same");
    log.append(1.0, "");
    let said: Vec<&str> = log.events.iter().map(|e| e.text.as_str()).collect();
    assert_eq!(said, vec!["  same", "  same"], "a run may say the same thing twice; an empty line is still nothing");
}

/// The Output log: each source posts its own sentence once, so a note
/// that stands while others come and go is one line; a source gone quiet
/// may say the same thing again; a job is one line with how long it took;
/// the cap drops the oldest; the export names level and source.
#[test]
fn test_the_output_log_is_one_line_an_event() {
    use view::Level;
    let texts = |log: &view::OutputLog| log.events.iter().map(|e| e.text.clone()).collect::<Vec<_>>();
    let mut log = view::OutputLog::default();
    log.say(0.4, Level::Info, "done", "");
    log.say(0.5, Level::Info, "done", "  ");
    assert!(log.events.is_empty(), "an empty text is no event");
    // A standing layer note, then a refusal beside it, then a job beside
    // both: three lines, where the joined status line used to post the
    // standing note again with every change.
    for t in [1.0, 1.1, 1.2] {
        log.say(t, Level::Refused, "simulation", "the model declines this op");
    }
    log.say(2.0, Level::Refused, "edit", "Nothing to undo");
    log.say(2.1, Level::Refused, "simulation", "the model declines this op");
    log.job(3.0, Some("Simulating stock removal…"));
    log.job(3.1, Some("Simulating stock removal…"));
    log.say(3.2, Level::Refused, "simulation", "the model declines this op");
    log.job(5.5, None);
    assert_eq!(texts(&log), vec!["the model declines this op", "Nothing to undo", "Simulating stock removal…"]);
    assert_eq!(log.events[2].took, Some(2.5), "the job's line carries how long it took");
    assert_eq!(view::OutputLog::render(&log.events[2]), "0:03  Simulating stock removal… (2.5 s)");
    assert_eq!(view::OutputLog::render(&log.events[1]), "0:02  Nothing to undo");
    // A source gone quiet forgets: the same refusal later is a new event.
    log.say(6.0, Level::Refused, "edit", "");
    log.say(62.0, Level::Refused, "edit", "Nothing to undo");
    assert_eq!(log.events.len(), 4);
    assert_eq!(view::OutputLog::render_line(62.0, "Nothing to undo"), "1:02  Nothing to undo");
    // A run's lines between do not make a standing sentence post again.
    log.event(63.0, Level::Info, "run", "$ nacs --outline");
    log.append(63.1, "stock  120.000 x 80.000");
    log.append(63.2, "exit 0");
    log.say(63.3, Level::Refused, "edit", "Nothing to undo");
    assert_eq!(log.events.len(), 7);
    let file = log.export("plate.cam", 1_794_492_185);
    let lines: Vec<&str> = file.lines().collect();
    assert_eq!(lines[0], "# nacs Output — plate.cam — window opened 2026-11-12 14:03:05 UTC");
    assert_eq!(lines[2], "0:02  refused  edit     Nothing to undo");
    assert_eq!(lines[3], "0:03  info     job      Simulating stock removal… (2.5 s)");
    assert_eq!(lines[6], "1:03  run      run        stock  120.000 x 80.000");
    assert_eq!(view::utc_stamp(0), "1970-01-01 00:00:00");
    assert_eq!(view::utc_stamp(951_782_400), "2000-02-29 00:00:00");
    assert_eq!(view::log_file_path(std::path::Path::new("jobs/plate.cam")), std::path::Path::new("jobs/plate.cam/plate.log"));
    let mut log = view::OutputLog::default();
    for i in 0..(view::OUTPUT_CAP + 5) {
        log.event(100.0 + i as f64, Level::Info, "done", &format!("note {i}"));
    }
    assert_eq!(log.events.len(), view::OUTPUT_CAP, "the cap holds");
    assert_eq!(log.events[0].text, "note 5", "the oldest left first");
    log.say(2000.0, Level::Refused, "edit", "x");
    log.clear();
    assert!(log.events.is_empty());
    log.say(2001.0, Level::Refused, "edit", "x");
    assert_eq!(log.events.len(), 1, "a cleared log posts the standing sentence again");
    let a = view::action("bottom.toggle").expect("the dock's act");
    assert_eq!(a.key, "3");
    assert!(view::action("output.clear").is_some_and(|a| a.key.is_empty()));
    assert!(view::doors_of("output.clear").contains(&"menu"), "a keyless act has its door");
    for id in ["output.copy", "output.save"] {
        assert!(view::action(id).is_some() && view::doors_of(id).contains(&"menu"), "{id}");
    }
    assert!(view::MENU.iter().any(|(_, rows)| rows.iter().any(|(id, label)| *id == "bottom.toggle" && *label == "Output")));
}

/// The loop's side as text (Phase 53 slice 2, fork 1A): `flip_side` swaps
/// the kind word on the opener and traverses the point lines the other
/// way, every other byte kept; twice is the identity; the flipped project
/// still parses (the winding law holds by construction); a kind without a
/// side and a line that is no statement are refused by name; the act is
/// in the inventory and on the feature's right-click menu.
#[test]
fn test_a_loops_side_flips_as_one_word_and_the_points_the_other_way() {
    let geom = std::fs::read_to_string("plate.cam/geometry.part").unwrap();
    let main = std::fs::read_to_string("plate.cam/main.part").unwrap();
    let p = parse::load_project("plate.cam").unwrap();
    let (_, file, first, _) = p.feature_lines.iter().find(|(n, ..)| n == "Notch").cloned().unwrap();
    assert_eq!(file, 1);
    let flipped = view::flip_side(&geom, first).unwrap();
    let g: Vec<&str> = geom.lines().collect();
    let f: Vec<&str> = flipped.lines().collect();
    assert_eq!(f.len(), g.len(), "no line added or lost");
    assert_eq!(f[first], g[first].replace("ProfilePocket(", "ProfileBoss("), "the kind word alone moves on the opener");
    let (_, last) = view::statement_extent(&geom, first).unwrap();
    let points: Vec<&str> = g[first + 1..last].to_vec();
    let mut reversed = points.clone();
    reversed.reverse();
    assert_eq!(&f[first + 1..last], &reversed[..], "the points traversed the other way");
    for i in (0..first).chain(last..g.len()) {
        assert_eq!(f[i], g[i], "line {} untouched", i + 1);
    }
    assert_eq!(flipped.ends_with(char::from(10u8)), geom.ends_with(char::from(10u8)));
    // Twice is the identity, byte for byte.
    assert_eq!(view::flip_side(&flipped, first).unwrap(), geom);
    // The winding law holds by construction (CCW for the pocket is CW for
    // the boss) — and on plate.cam the ONE PARSER still refuses the flip,
    // by the Clear's own law: a boss is roughed in Z levels and wants a
    // stepdown, which the notch's Clear does not carry. The shell says that
    // sentence and writes nothing; the ops that read a feature keep reading
    // it, and a flip changes the feature alone.
    let e = parse::parse_project_with_base(&main, Some(&flipped), Some(std::path::Path::new("plate.cam"))).unwrap_err();
    assert!(e.contains("'Notch' is a standing boss"), "{e}");
    // Where the ops read either side the same way, the flip lands:
    // profile_pocket.cam's Window has a Side alone.
    let pp = std::fs::read_to_string("profile_pocket.cam/main.part").unwrap();
    let w = parse::parse_project(&pp, None).unwrap();
    let (_, _, wline, _) = w.feature_lines.iter().find(|(n, ..)| n == "Window").cloned().unwrap();
    let pp_flipped = view::flip_side(&pp, wline).unwrap();
    let q = parse::parse_project(&pp_flipped, None).unwrap();
    assert!(matches!(q.features["Window"], parse::Feature::ProfileBoss { .. }), "the pocket is a boss, wound the other way, and the Side reads it");
    assert_eq!(view::flip_side(&pp_flipped, wline).unwrap(), pp);
    // Refused by name: a kind without a side, and a line that is no statement.
    let (_, _, plane, _) = p.feature_lines.iter().find(|(n, ..)| n == "TopFace").cloned().unwrap();
    let e = view::flip_side(&main, plane).unwrap_err();
    assert!(e.contains("'TopFace' is a Plane, which has no side to flip"), "{e}");
    assert!(view::flip_side(&geom, 0).unwrap_err().contains("not a statement"));
    assert_eq!(view::flipped_kind("ContourBoss"), Some("ContourPocket"));
    assert_eq!(view::flipped_kind("Sphere"), None);
    assert!(view::side_word("ProfilePocket").unwrap().contains("void inside"));
    assert!(view::side_word("ProfileBoss").unwrap().contains("standing material inside"));
    assert_eq!(view::side_word("Plane"), None);
    let a = view::action("feature.flip_side").expect("the act is in the inventory");
    assert!(a.key.is_empty(), "keyless: its doors are the card and the menu");
    assert!(view::context_actions(view::Selection::Feature(0)).iter().any(|(id, label)| *id == "feature.flip_side" && *label == "Flip side"));
    assert!(view::doors_of("feature.flip_side").contains(&"right-click menu"));
}

/// The band (DESIGN.md §4, §5.3, Phase 52 slice 2): two rows, one height,
/// and the sidebar's second row is the shown view's own line, derived.
#[test]
fn test_the_band_is_two_rows_and_the_views_line_is_derived() {
    assert_eq!(view::BAND_ROWS, 2);
    assert!(view::BAND_ROW_H > 0.0);
    let rows: Vec<view::FindingRow> = vec![];
    assert_eq!(view::view_line(SidebarTab::Outline, "plate.cam", 2, 2, 0, &rows, false, None), "plate.cam — 2 features, 2 operations");
    assert_eq!(view::view_line(SidebarTab::Outline, if cfg!(windows) { "C:\\work\\disc.cam" } else { "/work/disc.cam" }, 1, 1, 1, &rows, false, None), "disc.cam — 1 feature, 1 operation");
    assert_eq!(view::view_line(SidebarTab::Outline, "plate.cam", 0, 0, 0, &rows, false, None), "plate.cam — 0 features, no operations");
    // The Target group's row and its empty sentence (Phase 52 slice 4).
    assert_eq!(view::target_row("part.step", 7, 2), "part.step — 7 faces, 2 proposed");
    assert_eq!(view::target_row("one.step", 1, 0), "one.step — 1 face, 0 proposed");
    assert!(view::NO_TARGET.starts_with("No target"));
    assert_eq!(view::action("import.step").map(|a| a.key), Some(""), "the import is a keyless act");
    assert!(view::MENU.iter().any(|(_, rows)| rows.iter().any(|(id, _)| *id == "import.step")), "and the menu reaches it");
    assert_eq!(view::view_line(SidebarTab::Problems, "plate.cam", 2, 2, 0, &rows, false, None), "no problems");
    assert_eq!(view::view_line(SidebarTab::Problems, "plate.cam", 2, 2, 0, &rows, true, None), "1 problem", "a failed reload is a problem");
    assert_eq!(view::view_line(SidebarTab::Review, "plate.cam", 2, 2, 0, &rows, false, Some((2, 7))), "2 of 7 faces proposed — report.json");
    assert!(view::view_line(SidebarTab::Review, "plate.cam", 2, 2, 0, &rows, false, None).starts_with("no report"));
}

/// The tabs are the three views (DESIGN.md §5.2, amended 2026-10-17 — the
/// rail retired, Phase 52 slice 1; the Sequence folded, slice 4): the
/// sidebar's views in `SidebarTab::ALL`'s order, each saying one thing
/// once — and every tab's act is one the inventory knows, saying the
/// tab's own word and sentence. The Review's count rides the tab when a
/// report stands.
#[test]
fn test_the_tabs_are_the_three_views_and_nothing_twice() {
    let rail = view::tabs();
    assert_eq!(rail.iter().map(|s| s.tab).collect::<Vec<_>>(), SidebarTab::ALL.to_vec());
    let labels: Vec<&str> = rail.iter().map(|s| s.label).collect();
    assert_eq!(labels, ["Outline", "Problems", "Review"], "three words, rendered persistently (the Sequence folded into the Outline, Phase 52 slice 4)");
    let mut sorted = labels.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), labels.len(), "no word twice");
    let review = rail.iter().find(|s| s.tab == SidebarTab::Review).unwrap();
    assert_eq!(view::tab_label(review, 0, 0, Some((2, 7))), "Review (2/7)");
    assert_eq!(view::tab_label(review, 0, 0, None), "Review");
    assert_eq!(view::SIDEBAR_DOOR.0, "sidebar.toggle");
    assert!(view::action(view::SIDEBAR_DOOR.0).is_some(), "the shut sidebar's door is an act the inventory knows");
    for slot in &rail {
        let a = view::action(slot.action).unwrap_or_else(|| panic!("`{}` names an act the inventory knows", slot.label));
        assert_eq!(a.id, slot.tab.action_id());
        assert_eq!(slot.label, slot.tab.title());
        assert_eq!(slot.sentence, slot.tab.sentence());
    }
}

/// I1 and I2: the map is an overlay, and the bottom dock is empty BY
/// CONSTRUCTION — `Bottom` has no content field, so no line in the file
/// and no act can put anything there; a file that names the map homes
/// nothing, and the file never writes a map line.
#[test]
fn test_the_bottom_dock_is_empty_by_construction_and_the_map_is_an_overlay() {
    assert_eq!(SidebarTab::from_key("plan_map"), None);
    assert_eq!(Document::from_key("plan_map"), None);
    assert!(view::action("map.float").is_none());
    let l = Layout::parse("bottom.panes plan_map\nbottom.active plan_map\nmap inset\n");
    assert_eq!(l, Layout::default(), "a file that names the map changes nothing");
    let text = Layout::default().render();
    assert!(!text.contains("\nmap "), "the file never writes a map line");
    let bottom: Vec<&str> = text.lines().filter(|l| l.starts_with("bottom.")).collect();
    assert_eq!(bottom, vec!["bottom.open false", "bottom.height 200"], "two lines, and neither is content");
    assert!(!Layout::default().bottom.open, "the bottom dock is closed by default");
}

/// The center (DESIGN.md §5.3, amended 2026-09-04): both documents side
/// by side by default, either alone one act away, the program a document
/// and not a pane; the choice and the split's ratio round-trip through
/// the file, and the ratio clamps like a wall's size.
#[test]
fn test_the_center_shows_both_documents_by_default_and_either_alone() {
    let mut l = Layout::default();
    assert_eq!((l.center, l.split), (Center::Split, theme::SPLIT_DEFAULT));
    l.apply(DockAction::ShowDocument(Document::Program));
    assert_eq!(l.center, Center::Alone(Document::Program));
    assert!(l.render().contains("\ncenter program\n"));
    assert_eq!(Layout::parse(&l.render()), l);
    l.apply(DockAction::ShowSplit);
    assert_eq!(l.center, Center::Split);
    assert!(l.render().contains("\ncenter both\n"));
    l.apply(DockAction::ResizeSplit(0.3));
    assert_eq!(l.split, 0.3);
    assert!(l.render().contains("\ncenter.ratio 0.3\n"));
    assert_eq!(Layout::parse(&l.render()), l, "the ratio round-trips");
    l.apply(DockAction::ResizeSplit(0.01));
    assert_eq!(l.split, theme::SPLIT_MIN, "neither document can be dragged to a sliver");
    l.apply(DockAction::ResizeSplit(f32::NAN));
    assert_eq!(l.split, theme::SPLIT_DEFAULT);
    l.apply(DockAction::ShowDocument(Document::Viewport));
    assert_eq!(l.center, Center::Alone(Document::Viewport));
    assert_eq!(Layout::parse("center unicorn\n").center, Center::Split, "a made-up document is the default");
    assert_eq!(Layout::parse("center.ratio nope\n").split, theme::SPLIT_DEFAULT);
    assert_eq!(Layout::parse("center.ratio 2\n").split, theme::SPLIT_MAX, "a written ratio clamps like any Resize");
    assert_eq!(Center::from_key("both"), Some(Center::Split));
    assert_eq!(Center::from_key("gcode"), None);
    assert_eq!(view::action("center.split").map(|a| a.key), Some(""), "the split is a keyless act");
    for d in Document::ALL {
        assert!(view::action(d.action_id()).unwrap().sentence.contains("alone"), "a document's act shows it alone");
    }
}

/// I12: the dock keys are pure toggles, a view's act shows and never hides
/// (I3), and the acts that focused, cycled, moved and split are gone.
#[test]
fn test_the_dock_keys_are_pure_toggles_and_a_view_s_act_never_hides() {
    for id in ["sidebar.toggle", "form.toggle"] {
        assert!(view::action(id).is_some(), "{id} exists");
    }
    for id in ["dock.left", "dock.right", "dock.bottom", "pane.prev", "pane.next", "pane.move", "dock.split", "pane.project", "pane.gcode"] {
        assert!(view::action(id).is_none(), "{id} is gone");
    }
    let mut l = Layout::default();
    l.apply(DockAction::ToggleSidebar);
    assert!(!l.sidebar.open);
    l.apply(DockAction::ToggleSidebar);
    assert!(l.sidebar.open, "the same key, the same act, the other way");
    l.apply(DockAction::ToggleSidebar);
    l.apply(DockAction::ShowTab(SidebarTab::Problems));
    assert!(l.sidebar.open && l.sidebar.tab == SidebarTab::Problems, "a view's act opens the sidebar");
    l.apply(DockAction::ShowTab(SidebarTab::Problems));
    assert!(l.sidebar.open, "applied twice, still open — never a toggle");
    assert_eq!(l.shown_tab(), Some(SidebarTab::Problems));
    l.apply(DockAction::ToggleSidebar);
    assert_eq!(l.shown_tab(), None, "a collapsed sidebar shows no view");
}

/// Resize clamps at both walls, on every wall.
#[test]
fn test_resize_clamps_on_every_wall() {
    let mut l = Layout::default();
    l.apply(DockAction::ResizeSidebar(1.0));
    assert_eq!(l.sidebar.width, theme::DOCK_MIN_SIZE);
    l.apply(DockAction::ResizeSidebar(1e6));
    assert_eq!(l.sidebar.width, theme::DOCK_MAX_SIZE);
    l.apply(DockAction::ResizeForm(f32::NAN));
    assert_eq!(l.form.height, theme::DOCK_MIN_SIZE, "a nonsense height is the floor");
    l.apply(DockAction::ResizeBottom(1.0));
    assert_eq!(l.bottom.height, theme::DOCK_MIN_SIZE);
}

/// `parse(render(x)) == x` over the default, a worked-over arrangement and
/// a collapsed one; two saves of one state are byte-identical.
#[test]
fn test_the_workspace_file_round_trips_the_arrangement() {
    let mut worked = Layout::default();
    worked.apply(DockAction::ShowTab(SidebarTab::Problems));
    worked.apply(DockAction::ShowDocument(Document::Program));
    worked.apply(DockAction::ToggleForm);
    worked.apply(DockAction::ResizeSidebar(444.5));
    worked.apply(DockAction::ToggleBottom);
    worked.apply(DockAction::ResizeBottom(333.0));
    let mut shut = Layout::default();
    shut.apply(DockAction::ToggleSidebar);
    shut.apply(DockAction::ToggleForm);
    // The Layers pin (Phase 51 slice 2): additive — written only when on.
    let mut pinned = Layout::default();
    pinned.apply(DockAction::LayersPinned(true));
    assert!(pinned.render().contains("layers.pinned true\n"));
    assert!(!Layout::default().render().contains("layers.pinned"));
    // The delay (Phase 52 slice 11): a rest shorter than it is not an
    // open; the pin needs no rest; no pointer, no open.
    let d = nacs::view::LAYERS_HOVER_DELAY;
    assert!(nacs::view::layers_open(Some(d), false) && nacs::view::layers_open(Some(d + 1.0), false));
    assert!(!nacs::view::layers_open(Some(d / 2.0), false) && !nacs::view::layers_open(Some(0.0), false));
    assert!(nacs::view::layers_open(None, true) && nacs::view::layers_open(Some(0.0), true) && !nacs::view::layers_open(None, false));
    for candidate in [Layout::default(), worked, shut, pinned] {
        assert_eq!(Layout::parse(&candidate.render()), candidate, "parse(render(x)) == x");
    }
    assert_eq!(Layout::default().render(), Layout::default().render());
    let text = Layout::default().render();
    assert_eq!(
        text.lines().filter(|l| !l.starts_with('#')).collect::<Vec<_>>(),
        vec!["sidebar.tab outline", "sidebar.open true", "sidebar.width 320", "center both", "center.ratio 0.55", "form.open true", "form.height 360", "bottom.open false", "bottom.height 200"],
        "nine lines, one per fact, in a stated order"
    );
}

/// A workspace file from any earlier version of the window — panes that
/// moved, split and stacked; a map in the bottom dock; the program on the
/// right wall's tab — opens on the room it meant, through one alias table:
/// the sidebar shows the view the file named, the walls keep their sizes
/// and open flags, the program is a document, and every line about a home
/// or a split falls away as unknown.
#[test]
fn test_a_phase_13_workspace_file_still_opens_the_room_it_meant() {
    let text = "\
left.panes project,findings,declared_stock,tool,stock_sim,rest_cells,template
left.active findings
left.open true
left.size 444
left.split.panes template
left.ratio 0.3
right.panes gcode
right.active gcode
right.open false
right.size 500
bottom.panes plan_map
bottom.active plan_map
bottom.open true
bottom.size 150
map inset\n";
    let l = Layout::parse(text);
    assert_eq!(l.sidebar.tab, SidebarTab::Problems, "`left.active findings` is the Problems view");
    assert!(l.sidebar.open);
    assert_eq!(l.sidebar.width, 444.0, "`left.size` is the sidebar's width");
    assert_eq!(l.center, Center::Alone(Document::Program), "`right.active gcode` is the Program document, alone");
    assert!(!l.form.open, "`right.open` is the form dock's — the right wall's one flag that survived its retirement");
    assert_eq!(l.form.height, Layout::default().form.height, "`right.size` was the right wall's width — its width went to the center, and the line drops");
    assert!(l.bottom.open && l.bottom.height == 150.0, "`bottom.size` is the bottom's height");
    // A file from before the sidebar had a second view.
    let old = Layout::parse("left.active project\nleft.size 300\n");
    assert_eq!(old.sidebar.tab, SidebarTab::Outline);
    assert_eq!(old.sidebar.width, 300.0);
    assert_eq!(old.center, Center::Split, "and the center opens on the split, the default");
    // Legacy lines never come back out: the file is written in today's keys.
    let back = l.render();
    for key in ["left.", "right.", "bottom.size", ".panes", ".split.", "left.ratio", "map ", "inspector."] {
        assert!(!back.contains(key), "the file does not write `{key}`");
    }
}

/// The mangled file: unknown keys, a made-up view, an unparsable size, a
/// size below the floor, a bool that is not one. Every wall is where it
/// always is, and each broken line falls to its default alone.
#[test]
fn test_a_mangled_workspace_file_dents_the_layout_and_never_hides_a_view() {
    let text = "\
# a hand-mangled file
sidebar.tab unicorn
sidebar.width over_nine_thousand
bottom.height 1
gibberish line with no dot key
center program
inspector.open yes\n";
    let l = Layout::parse(text);
    assert_eq!(l.sidebar.tab, Layout::default().sidebar.tab, "a made-up view falls to the default");
    assert_eq!(l.sidebar.width, Layout::default().sidebar.width, "an unparsable size falls to the default");
    assert_eq!(l.bottom.height, theme::DOCK_MIN_SIZE, "a written size clamps like any Resize");
    assert_eq!(l.center, Center::Alone(Document::Program), "the good line landed");
    assert_eq!(l.form.open, Layout::default().form.open, "`yes` is not a bool — the default stands (and `inspector.open` reads as the form's)");
    assert_eq!(Layout::parse(""), Layout::default());
}

/// `0` returns the one arrangement to its defaults — a literal row,
/// field by field, nothing derived — from any state.
#[test]
fn test_revert_returns_the_arrangement_to_its_defaults() {
    use nacs::theme::{Bottom, FormDock, Layers, Sidebar};
    assert_eq!(
        Layout::default(),
        Layout {
            sidebar: Sidebar { tab: SidebarTab::Outline, open: true, width: 320.0 },
            center: Center::Split,
            split: theme::SPLIT_DEFAULT,
            form: FormDock { open: true, height: 360.0 },
            bottom: Bottom { open: false, height: 200.0 },
            layers: Layers::default(),
            program_numbers: true,
            program_minimap: true,
            layers_pinned: false,
            frame_native: false,
        },
        "the row, verbatim"
    );
    let mut l = Layout::default();
    l.apply(DockAction::ToggleSidebar);
    l.apply(DockAction::ResizeForm(700.0));
    l.apply(DockAction::ShowDocument(Document::Program));
    l.apply(DockAction::ShowTab(SidebarTab::Review));
    assert_ne!(l, Layout::default());
    l.apply(DockAction::Revert);
    assert_eq!(l, Layout::default());
}

/// A modes-era workspace file (2026-08-26 to 2026-09-04) — a `mode` line
/// and three stanzas — opens as the ONE arrangement: the unprefixed
/// (author) stanza wins, the `diagnose.`/`verify.` lines drop as unknown
/// along with the `mode` line, and a mangled line still dents one fact
/// alone. The file written back carries one arrangement and no `mode`.
#[test]
fn test_a_modes_era_file_reads_as_the_one_layout_with_the_author_stanza_winning() {
    let text = "\
# nacs workspace
mode verify

# author — write and see a part
sidebar.tab problems
sidebar.open true
sidebar.width 444.5
center viewport
inspector.open false
inspector.width 280
bottom.open false
bottom.height 200

# diagnose — find out why a check failed
diagnose.sidebar.tab outline
diagnose.sidebar.width nope
diagnose.layers toolpaths

# verify — check it before posting
verify.sidebar.tab problems
verify.center program
verify.inspector.open false
verify.layers outlines,toolpaths,removal_boundaries,stock_simulation,fixtures,tool
";
    let l = Layout::parse(text);
    let mut expected = Layout::default();
    expected.sidebar.tab = SidebarTab::Problems;
    expected.sidebar.width = 444.5;
    expected.form.open = false;
    expected.center = Center::Alone(Document::Viewport);
    assert_eq!(l, expected, "the author stanza, and only it");
    assert_eq!(l.center, Center::Alone(Document::Viewport), "the author stanza's `center viewport` — alone, as the file says; verify's `center program` did not win");
    let back = l.render();
    assert!(!back.contains("mode ") && !back.contains("diagnose.") && !back.contains("verify."), "{back}");
    assert_eq!(Layout::parse(&back), l, "and it round-trips in today's keys");
    // A file that only ever had one stanza — the Phase-12 era — is the
    // defaults with that file's edits.
    let old = Layout::parse("left.active findings\nleft.size 444\nright.open false\n");
    let mut expected = Layout::default();
    expected.sidebar.tab = SidebarTab::Problems;
    expected.sidebar.width = 444.0;
    expected.form.open = false;
    assert_eq!(old, expected);
}

/// The layers are part of the stored arrangement: the file carries
/// them additively (the default writes nothing, so an older file still
/// means what it meant), exactly the named ones are on, and an unknown
/// token is dropped like any other.
#[test]
fn test_the_workspace_file_carries_the_layers_additively() {
    use nacs::theme::{Layer, Layers};
    assert!(!Layout::default().render().contains("layers "), "the default writes no layers line");
    let mut l = Layout::default();
    l.apply(DockAction::Layer(Layer::SimulatedStock, true));
    l.apply(DockAction::Layer(Layer::Annotations, false));
    let text = l.render();
    assert!(text.contains("\nlayers outlines,toolpaths,removal_boundaries,stock_simulation,fixtures,tool\n"), "{text}");
    assert_eq!(Layout::parse(&text).layers, l.layers);
    assert_eq!(Layout::parse("layers toolpaths,unicorn\n").layers, Layers::from_on(&[Layer::Toolpaths]));
    assert_eq!(Layout::parse("sidebar.open true\n").layers, Layers::default(), "no line, the default");
}

/// I3: a tab click's destination never depends on prior state — a
/// sidebar view's act shows it and opens the sidebar, from any state, and
/// applied again it changes nothing. (A rail click, until 2026-10-17.)
#[test]
fn test_a_tab_click_shows_and_never_hides() {
    for start in [Layout::default(), {
        let mut l = Layout::default();
        l.apply(DockAction::ToggleSidebar);
        l.apply(DockAction::ShowDocument(Document::Program));
        l
    }] {
        for t in SidebarTab::ALL {
            let mut l = start.clone();
            l.apply(DockAction::ShowTab(t));
            assert!(l.sidebar.open && l.sidebar.tab == t, "shown and open, from {start:?}");
            let once = l.clone();
            l.apply(DockAction::ShowTab(t));
            assert_eq!(l, once, "again: nothing changes, nothing hides");
            assert_eq!(l.center, start.center, "and the center is not the tab's to touch");
        }
    }
}

// ---- Phase 16 slice 3: the block-grain sync -------------------------------
// (DESIGN.md §5.3 and §5.6 as amended 2026-08-28; DECISIONS.md "Phase 16
// opens: the clock-and-address DRAFT…", fork 4.)

/// The view model's block attribution IS the emission's correspondence:
/// over plate.cam, every op's move count matches its emission, `block_line`
/// round-trips through the attributed lines, and a line carrying a block
/// always carries its op.
#[test]
fn test_block_attribution_is_the_correspondence() {
    let src = std::fs::read_to_string("plate.cam/main.part").unwrap();
    let geom = std::fs::read_to_string("plate.cam/geometry.part").ok();
    let part = parse::parse_project_with_base(&src, geom.as_deref(), Some(std::path::Path::new("plate.cam"))).unwrap();
    let g = view::gcode_view(&part).expect("plate emits");
    let (_, emissions) = nacs::gcode::program_with_moves(&part).unwrap();
    for em in &emissions {
        assert_eq!(g.block_count(em.op_idx), em.moves.len(), "op {}'s move count", em.op_idx);
        for k in 0..em.moves.len() {
            assert_eq!(g.block_line(em.op_idx, k), Some(em.move_line(k)),
                "move {k} of op {} sits where the emission says", em.op_idx);
        }
    }
    for (i, line) in g.lines.iter().enumerate() {
        if line.block.is_some() {
            assert!(line.op.is_some(), "line {i}: a block always belongs to an op");
        }
    }
}

/// The block walk is total and ordered: stepping forward from the very top
/// visits every move of every op exactly once, in emission order, and
/// parks after the last op with the block cleared; the same walk backward
/// returns to the top. The step is pure, so the pin owns it whole.
#[test]
fn test_the_block_walk_visits_every_move_once_and_in_order() {
    let src = std::fs::read_to_string("plate.cam/main.part").unwrap();
    let geom = std::fs::read_to_string("plate.cam/geometry.part").ok();
    let part = parse::parse_project_with_base(&src, geom.as_deref(), Some(std::path::Path::new("plate.cam"))).unwrap();
    let g = view::gcode_view(&part).expect("plate emits");
    let n = part.operations.len();
    let total: usize = (0..n).map(|op| g.block_count(op)).sum();

    let (mut k, mut b) = (0usize, None);
    let mut visited: Vec<(usize, usize)> = Vec::new();
    loop {
        let (nk, nb) = view::block_step(&g, n, k, b, true);
        if (nk, nb) == (k, b) || nb.is_none() {
            k = nk;
            b = nb;
            break;
        }
        k = nk;
        b = nb;
        visited.push((k - 1, b.unwrap()));
        assert!(visited.len() <= total, "the walk never revisits");
    }
    assert_eq!((k, b), (n, None), "the walk parks after the last op, block cleared");
    assert_eq!(visited.len(), total, "every move visited exactly once");
    let mut lines: Vec<usize> = visited.iter().map(|(op, b)| g.block_line(*op, *b).unwrap()).collect();
    let sorted = { let mut s = lines.clone(); s.sort_unstable(); s };
    assert_eq!(lines, sorted, "and in program order");

    // Backward from the end returns to the very top through the same count.
    let (mut k, mut b) = (n, None);
    let mut steps = 0usize;
    loop {
        let (nk, nb) = view::block_step(&g, n, k, b, false);
        if (nk, nb) == (k, b) {
            break;
        }
        k = nk;
        b = nb;
        steps += 1;
        assert!(steps <= total + 1, "the backward walk terminates");
    }
    assert_eq!((k, b), (0, None), "backward parks before op 1");
    assert_eq!(steps, total + 1, "every move once, plus the final step off the top");
    lines.clear();
}

/// The transport's label nests the block inside the op — and without a
/// block it is yesterday's label byte for byte.
#[test]
fn test_the_transport_label_nests_the_block() {
    let src = std::fs::read_to_string("plate.cam/main.part").unwrap();
    let geom = std::fs::read_to_string("plate.cam/geometry.part").ok();
    let part = parse::parse_project_with_base(&src, geom.as_deref(), Some(std::path::Path::new("plate.cam"))).unwrap();
    let n = part.operations.len();
    let plain = view::Transport { cursor: 1, count: n, block: None, blocks: 0 }.line(&part);
    assert!(!plain.contains("move"), "no block, no block words: {plain}");
    let nested = view::Transport { cursor: 1, count: n, block: Some(4), blocks: 12 }.line(&part);
    assert!(nested.starts_with(&plain), "the block appends, never rewrites: {nested}");
    assert!(nested.ends_with("· move 5 of 12"), "{nested}");
    // The move row's own words (Phase 52 slice 13, fork 13c-A): present
    // at every cursor, saying what there is to walk or why nothing.
    let at = |cursor, block, blocks| view::Transport { cursor, count: n, block, blocks }.move_line();
    assert_eq!(at(0, None, 0), "no operation run — step on for its moves");
    assert_eq!(at(1, None, 0), "[0] emits no moves");
    assert_eq!(at(1, None, 12), "[0]: 12 moves");
    assert_eq!(at(1, Some(4), 12), "[0]: move 5 of 12");
    assert_eq!(view::Transport { cursor: 0, count: 0, block: None, blocks: 0 }.move_line(), "No operations");
}

// ---- Phase 17 lane B slice 2: the window follows the file (DESIGN.md §1) ----

/// The watch's one reading: the newest mtime over the files a project was
/// read from, `None` for an empty list or a file that cannot be stat'ed —
/// a vanished file is a change, and the reload that follows names it.
#[test]
fn test_newest_mtime_is_the_max_over_the_sources_and_none_when_one_is_gone() {
    // Off the `.cam` extension on purpose: the corpus scanners walk
    // `*.cam` dirs, and a temp project with that suffix has raced them
    // before (the check_tests convention).
    let dir = std::env::temp_dir().join(format!("nacs_watch_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("a.part");
    let b = dir.join("b.machine");
    std::fs::write(&a, "x").unwrap();
    std::fs::write(&b, "y").unwrap();
    let older = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
    let newer = older + std::time::Duration::from_secs(60);
    // Opened for WRITING: Windows refuses set_modified on a read-only handle
    // (ERROR_ACCESS_DENIED), Linux never minded.
    let writable = |p: &std::path::Path| std::fs::OpenOptions::new().write(true).open(p).unwrap();
    writable(&a).set_modified(older).unwrap();
    writable(&b).set_modified(newer).unwrap();
    assert_eq!(view::newest_mtime(&[]), None, "no sources, no reading");
    assert_eq!(view::newest_mtime(&[a.clone()]), Some(older));
    assert_eq!(view::newest_mtime(&[a.clone(), b.clone()]), Some(newer), "the newest file wins, whichever is listed first");
    assert_eq!(view::newest_mtime(&[b.clone(), a.clone()]), Some(newer));
    // Touch the older one past the newer: the reading moves — this is the
    // event the watch fires on.
    writable(&a).set_modified(newer + std::time::Duration::from_secs(60)).unwrap();
    assert_eq!(view::newest_mtime(&[a.clone(), b.clone()]), Some(newer + std::time::Duration::from_secs(60)));
    std::fs::remove_file(&b).unwrap();
    assert_eq!(view::newest_mtime(&[a.clone(), b.clone()]), None, "a vanished source is a change, read as None");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A project remembers every file it was read from, part files first and
/// then the externals in the order the parse opened them; a project parsed
/// from text alone remembers nothing — there is nothing to follow.
#[test]
fn test_a_loaded_project_lists_its_sources_part_files_first() {
    let p = parse::load_project("cylinder_bore_machine.cam").unwrap();
    let names: Vec<String> = p.sources.iter().map(|s| s.file_name().unwrap().to_string_lossy().to_string()).collect();
    assert_eq!(names, vec!["main.part", "fanuc.machine"], "the part file leads, the machine file it names follows");
    assert!(p.sources.iter().all(|s| s.exists()), "every source is a file on disk");
    // A geometry.part, when present, is the second part file.
    for dir in ["plate.cam", "cylinder_bore.cam"] {
        let p = parse::load_project(dir).unwrap();
        let names: Vec<String> = p.sources.iter().map(|s| s.file_name().unwrap().to_string_lossy().to_string()).collect();
        assert_eq!(names[0], "main.part", "{dir}");
        let has_geom = std::path::Path::new(dir).join("geometry.part").exists();
        assert_eq!(names.get(1) == Some(&"geometry.part".to_string()), has_geom, "{dir}: geometry.part listed iff present");
    }
    // Text alone: nothing to watch.
    let from_text = parse::parse_project("stock Box(x=50, y=50, z=10)\ntool T1 Endmill(dia=6, flutes=2)\n", None);
    if let Ok(p) = from_text {
        assert!(p.sources.is_empty());
    }
}

/// A reload is one more cursor move: the op cursor clamps to the new
/// count — and a cursor after the last operation stays after the new
/// last one — the block cursor clears by the nesting invariant, and an
/// index selection survives exactly when the new program still has that
/// index.
#[test]
fn test_after_reload_clamps_the_cursor_clears_the_block_and_keeps_only_a_selection_that_still_exists() {
    use view::Selection as S;
    // Same size: everything survives except the block.
    assert_eq!(view::after_reload(2, 3, S::Operation(1), 3, 4, 2, 0, &[]), (2, None, S::Operation(1)));
    assert_eq!(view::after_reload(3, 3, S::Feature(3), 3, 4, 2, 0, &[]), (3, None, S::Feature(3)));
    assert_eq!(view::after_reload(0, 3, S::Finding(1), 3, 4, 2, 0, &[]), (0, None, S::Finding(1)));
    // An op was ADDED (the live run's case): a cursor that stood after the
    // last op stands after the new last op, so the new op is run and drawn;
    // a cursor in the middle stays where it was.
    assert_eq!(view::after_reload(3, 3, S::None, 4, 4, 2, 0, &[]), (4, None, S::None), "at the end, stays at the end");
    assert_eq!(view::after_reload(1, 3, S::None, 4, 4, 2, 0, &[]), (1, None, S::None), "in the middle, stays put");
    assert_eq!(view::after_reload(0, 0, S::None, 1, 1, 0, 0, &[]), (1, None, S::None), "the first op of an empty program runs");
    // An op was removed: the cursor past the end clamps, the selection at
    // the vanished index becomes nothing, an earlier one stays.
    assert_eq!(view::after_reload(3, 3, S::Operation(2), 2, 4, 2, 0, &[]), (2, None, S::None));
    assert_eq!(view::after_reload(3, 3, S::Operation(1), 2, 4, 2, 0, &[]), (2, None, S::Operation(1)));
    assert_eq!(view::after_reload(2, 3, S::None, 2, 4, 2, 0, &[]), (2, None, S::None), "in the middle before, at the end now: clamped, not moved");
    // A feature and a finding likewise, each against its own count.
    assert_eq!(view::after_reload(1, 2, S::Feature(4), 2, 4, 2, 0, &[]), (1, None, S::None));
    assert_eq!(view::after_reload(1, 2, S::Finding(2), 2, 4, 2, 0, &[]), (1, None, S::None));
    assert_eq!(view::after_reload(1, 2, S::Finding(0), 2, 4, 0, 0, &[]), (1, None, S::None), "a program that became clean has no finding to keep");
    // The program was emptied: cursor 0, nothing selected.
    assert_eq!(view::after_reload(5, 5, S::Operation(0), 0, 0, 0, 0, &[]), (0, None, S::None));
    assert_eq!(view::after_reload(5, 0, S::None, 0, 0, 0, 0, &[]), (0, None, S::None));
    // A tool is kept by NUMBER (Phase 20 slice 1): the declaration is
    // still there or it is not; the count of tools says nothing about it.
    assert_eq!(view::after_reload(1, 2, S::Tool(5), 2, 4, 2, 0, &[1, 5]), (1, None, S::Tool(5)));
    assert_eq!(view::after_reload(1, 2, S::Tool(5), 2, 4, 2, 0, &[1, 2]), (1, None, S::None), "T5 was renumbered away");
}

/// The palette reaches the reload; no key does (DESIGN.md §9: no new
/// bindings), and the watch needs neither.
#[test]
fn test_the_project_reload_is_a_keyless_act_the_palette_finds() {
    let a = view::action("project.reload").expect("the act exists");
    assert_eq!(a.key, "", "keyless by DESIGN.md §9");
    let found: Vec<&str> = view::palette_matches("reload").iter().map(|a| a.id).collect();
    assert!(found.contains(&"project.reload") && found.contains(&"theme.reload"), "{found:?}");
}

// ---- Phase 17 lane B slice 3: the one write (DESIGN.md §1) ----

/// Suppression is the text the user could have typed: the `run` line
/// gains `# `, every other byte stays, and the program re-parses without
/// exactly that operation.
#[test]
fn test_suppress_line_comments_exactly_the_run_line_and_the_program_loses_exactly_that_op() {
    let dir = "plate.cam";
    let before = std::fs::read_to_string(format!("{dir}/main.part")).unwrap();
    let p = parse::load_project(dir).unwrap();
    assert_eq!(p.op_lines, vec![(0, 18), (0, 25), (0, 28)], "plate.cam's three run lines, 0-based, all in main.part");
    let (_, line) = p.op_lines[0];
    let after = view::suppress_line(&before, line).unwrap();
    let b: Vec<&str> = before.lines().collect();
    let a: Vec<&str> = after.lines().collect();
    assert_eq!(a.len(), b.len(), "no line added or lost");
    for i in 0..b.len() {
        if i == line {
            assert_eq!(a[i], format!("# {}", b[i]));
            assert!(b[i].starts_with("run End("), "{}", b[i]);
        } else {
            assert_eq!(a[i], b[i], "line {} untouched", i + 1);
        }
    }
    assert!(after.ends_with('\n') == before.ends_with('\n'), "the file's final newline is kept as it was");
    let geom = std::fs::read_to_string(format!("{dir}/geometry.part")).ok();
    let q = parse::parse_project_with_base(&after, geom.as_deref(), Some(std::path::Path::new(dir))).unwrap();
    assert_eq!(q.operations.len(), p.operations.len() - 1);
    assert_eq!(q.operations[0].op_type, p.operations[1].op_type, "the Clear remains, now first");
    assert_eq!(q.op_lines, vec![(0, 25), (0, 28)], "the surviving ops still know their lines");
    // Not a run line: refused by name, nothing written.
    let e = view::suppress_line(&before, 0).unwrap_err();
    assert!(e.contains("line 1 is not a run statement"), "{e}");
    let e = view::suppress_line(&before, 10_000).unwrap_err();
    assert!(e.contains("past the end"), "{e}");
    // A run line already commented is not a run line either — suppressing
    // twice is refused, not stacked.
    let e = view::suppress_line(&after, line).unwrap_err();
    assert!(e.contains("not a run statement"), "{e}");
}

/// A `run` line's file stamp: main.part's is file 0, and a `run` in
/// geometry.part is stamped file 1 — which is exactly what the parser
/// now reads to refuse it by name (Phase 20, fork 7: the operational
/// script lives in one file). A synthesized operation has no line, and
/// the table says so by being shorter.
#[test]
fn test_op_lines_name_the_file_and_stop_at_the_parsed_ops() {
    // The whole header (the header law, Phase 50 slice 1), so the run sits
    // at line 8 of main.part.
    let main_src = "stock Box(w=120, l=80, h=20, x=0, y=0, z=0)\nfeed 500\nplunge 200\nspindle 3000\nrapid_z 5\nclearance_z 25\ntool T5 Endmill(dia=10.0)\nfeature TopFace Plane(z=-0.5)\nrun End(tool=T5, surface=TopFace, stepover=6.0)\n";
    let geom_src = "# geometry with a run of its own\nfeature Top2 Plane(z=-0.5)\nrun End(tool=T5, surface=Top2, stepover=6.0)\n";
    let e = parse::parse_project(main_src, Some(geom_src)).unwrap_err();
    assert!(e.starts_with("geometry.part: line 3: a run line belongs in main.part"), "the stamp names the file and the line: {e}");
    let p = parse::parse_project(main_src, Some("feature Top2 Plane(z=-0.5)\n")).unwrap();
    assert_eq!(p.op_lines, vec![(0, 8)]);
    assert!(p.sources.is_empty(), "text alone: nothing on disk to write to");
    let mut q = p.clone();
    q.operations.push(q.operations[0].clone());
    assert_eq!(q.op_lines.get(2), None, "a synthesized op has no line, honestly");
}

/// Undo and redo are a stack of texts, never a command history: a new
/// write clears the redo stack, and each move carries the stamp the file
/// had after the rewrite.
#[test]
fn test_edit_history_is_a_stack_of_texts_and_a_new_write_clears_redo() {
    use std::time::{Duration, UNIX_EPOCH};
    let e = |n: u64| view::Edit { path: format!("f{n}").into(), before: format!("b{n}"), after: format!("a{n}"), stamp: Some(UNIX_EPOCH + Duration::from_secs(n)) };
    let mut h = view::EditHistory::default();
    assert!(h.to_undo().is_none() && h.to_redo().is_none());
    h.record(e(1));
    h.record(e(2));
    assert_eq!(h.depth(), (2, 0));
    assert_eq!(h.to_undo().unwrap().before, "b2");
    h.undone(Some(UNIX_EPOCH + Duration::from_secs(20)));
    assert_eq!(h.depth(), (1, 1));
    assert_eq!(h.to_redo().unwrap().after, "a2");
    assert_eq!(h.to_redo().unwrap().stamp, Some(UNIX_EPOCH + Duration::from_secs(20)), "the stamp is the file's after the undo's rewrite");
    h.redone(Some(UNIX_EPOCH + Duration::from_secs(30)));
    assert_eq!(h.depth(), (2, 0));
    assert_eq!(h.to_undo().unwrap().stamp, Some(UNIX_EPOCH + Duration::from_secs(30)));
    h.undone(None);
    h.undone(None);
    assert_eq!(h.depth(), (0, 2));
    h.record(e(3));
    assert_eq!(h.depth(), (1, 0), "a new write clears redo");
    h.undone(None);
    h.undone(None);
    assert_eq!(h.depth(), (0, 1), "undoing past the bottom is a no-op");
}

/// Two writes, two undos, two redos, through a real file with real
/// stamps: the "changed since" check is against the stamp this window LAST
/// left on the file, so the second undo is not refused because the first
/// one moved the mtime (Phase 43 slice 1). Another editor's write between
/// is still refused by name.
#[test]
fn test_undo_walks_the_whole_stack_through_a_real_file_and_a_foreign_write_is_refused() {
    let dir = std::env::temp_dir().join(format!("nacs-undo-stack-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.part");
    let stamp = |p: &std::path::Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    nacs::theme::write_atomically(&path, "v0").unwrap();
    let mut h = view::EditHistory::default();
    // Two writes, exactly as the shell's `set_field` makes them: read
    // before, write after, record with the stamp the file has after.
    for text in ["v1", "v2"] {
        std::thread::sleep(std::time::Duration::from_millis(25));
        let before = std::fs::read_to_string(&path).unwrap();
        nacs::theme::write_atomically(&path, text).unwrap();
        h.record(view::Edit { path: path.clone(), before, after: text.to_string(), stamp: stamp(&path) });
    }
    // Two undos: the second is the one the per-entry stamp refused.
    for expect in ["v1", "v0"] {
        std::thread::sleep(std::time::Duration::from_millis(25));
        let e = h.to_undo().unwrap();
        let (p, t) = (e.path.clone(), e.before.clone());
        let new = view::rewrite_if_unmoved(&p, &t, &h).unwrap_or_else(|r| panic!("undo to {expect} refused: {r}"));
        h.undone(new);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), expect);
    }
    assert_eq!(h.depth(), (0, 2));
    // Two redos, the mirror.
    for expect in ["v1", "v2"] {
        std::thread::sleep(std::time::Duration::from_millis(25));
        let e = h.to_redo().unwrap();
        let (p, t) = (e.path.clone(), e.after.clone());
        let new = view::rewrite_if_unmoved(&p, &t, &h).unwrap_or_else(|r| panic!("redo to {expect} refused: {r}"));
        h.redone(new);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), expect);
    }
    assert_eq!(h.depth(), (2, 0));
    // A foreign write moves the mtime: the undo is refused by name and the
    // file is left as the other editor wrote it.
    std::thread::sleep(std::time::Duration::from_millis(25));
    std::fs::write(&path, "theirs").unwrap();
    let e = h.to_undo().unwrap();
    let (p, t) = (e.path.clone(), e.before.clone());
    let err = view::rewrite_if_unmoved(&p, &t, &h).unwrap_err();
    assert!(err.contains("main.part changed since this window wrote it"), "{err}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "theirs");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The atomic write replaces the whole file and leaves no temp file
/// beside it.
#[test]
fn test_write_atomically_replaces_the_file_and_leaves_no_temp() {
    let dir = std::env::temp_dir().join(format!("nacs_write_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("main.part");
    std::fs::write(&f, "one\n").unwrap();
    theme::write_atomically(&f, "two\n").unwrap();
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "two\n");
    let names: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    assert_eq!(names, vec!["main.part"], "no temp file left behind");
    let e = theme::write_atomically(&dir.join("missing").join("x"), "").unwrap_err();
    assert!(e.contains("write") || e.contains("rename"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The write's acts: suppress keyless (the inspector's button reaches it),
/// undo and redo on the two keys the amendment named, colliding with none.
#[test]
fn test_the_write_acts_are_keyed_as_the_amendment_says() {
    assert_eq!(view::action("op.suppress").unwrap().key, "");
    assert_eq!(view::action("edit.undo").unwrap().key, "z");
    assert_eq!(view::action("edit.redo").unwrap().key, "y");
    let mut keys: Vec<&str> = view::ACTIONS.iter().map(|a| a.key).filter(|k| !k.is_empty()).collect();
    let n = keys.len();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), n, "no two acts share a key");
    assert!(view::palette_matches("suppress").iter().any(|a| a.id == "op.suppress"));
    assert!(view::palette_matches("undo").iter().any(|a| a.id == "edit.undo"));
}

// ---- Phase 17 lane A slice 3: the review (DESIGN.md §5.2, §6) ----

/// The second write as text: one blank line, the statement, a newline —
/// the shape a paste from `proposed.part` leaves — whatever the file ended
/// with, and the program re-parses with exactly one more feature.
#[test]
fn test_append_statement_pastes_the_way_proposed_part_reads_and_the_program_gains_one_feature() {
    let s = "feature F1_Top Plane(z=15.0)";
    assert_eq!(view::append_statement("", s), "feature F1_Top Plane(z=15.0)\n");
    assert_eq!(view::append_statement("a\n", s), "a\n\nfeature F1_Top Plane(z=15.0)\n");
    assert_eq!(view::append_statement("a", s), "a\n\nfeature F1_Top Plane(z=15.0)\n", "a missing final newline is supplied");
    assert_eq!(view::append_statement("a\n\n", s), "a\n\nfeature F1_Top Plane(z=15.0)\n", "an existing blank line is not doubled");
    assert_eq!(view::append_statement("a\n", "feature X Plane(z=1)\n\n"), "a\n\nfeature X Plane(z=1)\n", "the statement's own trailing whitespace is trimmed");
    let dir = "plate.cam";
    let before = std::fs::read_to_string(format!("{dir}/main.part")).unwrap();
    let geom = std::fs::read_to_string(format!("{dir}/geometry.part")).ok();
    let p = parse::parse_project_with_base(&before, geom.as_deref(), Some(std::path::Path::new(dir))).unwrap();
    let after = view::append_statement(&before, "feature Lid Plane(z=-1.0)");
    let q = parse::parse_project_with_base(&after, geom.as_deref(), Some(std::path::Path::new(dir))).unwrap();
    assert_eq!(q.features.len(), p.features.len() + 1);
    assert!(q.features.contains_key("Lid"));
    assert_eq!(q.operations.len(), p.operations.len(), "no operation moved");
    assert_eq!(view::feature_name_of("feature Lid Plane(z=-1.0)"), Some("Lid"));
    assert_eq!(view::feature_name_of("run End(tool=T1)"), None);
}

/// The review lights the proposal's own edges — the statement through the
/// one parser and the one edge path — and a statement that is not a
/// feature lights nothing, by name.
#[test]
fn test_proposal_edges_come_from_the_statement_through_the_one_parser() {
    let edges = view::proposal_edges("feature F1_Top Plane(z=15.0)").unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].name, "F1_Top");
    let edges = view::proposal_edges("feature F6_Bore CylinderBore(dia=10, extent=5, x=20, y=20)").unwrap();
    assert_eq!(edges[0].name, "F6_Bore");
    assert!(edges[0].polylines.iter().any(|l| l.len() > 8), "a bore's rim is a flattened circle");
    assert!(view::proposal_edges("run End(tool=T1, surface=Top)").is_err(), "not a feature: nothing to light");
    assert!(view::proposal_edges("feature Bad Nonsense(q=1)").is_err());
}

/// A review selection survives a reload exactly when its row still exists,
/// like the other three kinds.
#[test]
fn test_after_reload_keeps_a_review_row_that_still_exists() {
    use view::Selection as S;
    assert_eq!(view::after_reload(1, 3, S::Review(2), 3, 3, 3, 3, &[]), (1, None, S::Review(2)));
    assert_eq!(view::after_reload(1, 3, S::Review(2), 3, 3, 3, 2, &[]), (1, None, S::None));
    assert_eq!(view::after_reload(1, 3, S::Review(0), 3, 3, 3, 0, &[]), (1, None, S::None), "no report, no row");
}

/// The Review view has its tab and its act, and accept is keyless
/// like suppress — a write should not be one keystroke away.
#[test]
fn test_the_review_view_and_its_accept_are_acts_the_tabs_and_palette_reach() {
    assert!(SidebarTab::ALL.contains(&SidebarTab::Review));
    assert_eq!(SidebarTab::Review.title(), "Review");
    assert_eq!(SidebarTab::from_key("review"), Some(SidebarTab::Review));
    assert!(view::tabs().iter().any(|s| s.action == SidebarTab::Review.action_id()), "a Review tab");
    assert_eq!(view::action("review.accept").unwrap().key, "");
    assert!(view::palette_matches("accept").iter().any(|a| a.id == "review.accept"));
    // The inspector and the sentence read the review's rows.
    let row = view::ReviewRow { face: 3, id: "F4".into(), section: "Proposed".into(), head: "F4  plane".into(), text: "feature F4_Top Plane(z=1)\nplanar".into(), statement: Some("feature F4_Top Plane(z=1)".into()), refs: vec![], in_project: false, rejected: false, member_of: None };
    let p = parse::Part::default();
    let outline = nacs::outline::outline(&p);
    assert_eq!(view::selection_sentence(&p, &outline, view::Selection::Review(0), &[], &[], std::slice::from_ref(&row)), "F4  plane");
    match view::card(&p, &outline, view::Selection::Review(0), &[], &[], &[], std::slice::from_ref(&row), None) {
        view::Card::Review { statement, in_project, .. } => {
            assert_eq!(statement.as_deref(), Some("feature F4_Top Plane(z=1)"));
            assert!(!in_project);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(view::card(&p, &outline, view::Selection::Review(1), &[], &[], &[], std::slice::from_ref(&row), None), view::Card::Nothing);
}

/// The sidecar's proposed stock is the Review's first row (ROADMAP item 1,
/// slice 1): derived from `proposed_stock.part` and the program, never
/// remembered — accepted by this window or any editor, it reads in project.
#[test]
fn test_the_proposed_stock_is_a_review_row_derived_from_the_file() {
    let proposal = std::fs::read_to_string("tests/fixtures/l_plate.stock.golden.part").unwrap();
    let p = parse::Part::default();
    let s = view::stock_proposal(&proposal, &p).unwrap();
    assert_eq!(s.head, "Stock  Profile of 6 points");
    assert!(s.statement.starts_with("stock Profile(h=20.0, z=20.0\n    0.0 0.0\n"), "{}", s.statement);
    assert!(s.statement.ends_with("\n)"), "{}", s.statement);
    assert!(!s.statement.contains('#'), "the sidecar's comments are not the statement");
    assert!(s.text.starts_with("# Proposed stock from l_plate.step"), "the card shows the file's own words too");
    assert!(!s.in_project, "a default part's blank is not the model's");
    let accepted = parse::parse(&nacs::template::project_text_from_proposal(&proposal).unwrap()).unwrap();
    assert!(view::stock_proposal(&proposal, &accepted).unwrap().in_project, "the same blank, read off the program");
    assert!(view::stock_proposal("# nothing here\n", &p).is_err(), "a file with no statement is said, not shown");
    // The row wears the review card, Accept in its menu, under the Review tab.
    let outline = nacs::outline::outline(&p);
    match view::card(&p, &outline, view::Selection::ProposedStock, &[], &[], &[], &[], Some(&s)) {
        view::Card::Review { head, section, statement, in_project, .. } => {
            assert_eq!(head, "Stock  Profile of 6 points");
            assert_eq!(section, "Proposed stock");
            assert_eq!(statement.as_deref(), Some(s.statement.as_str()));
            assert!(!in_project);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(view::card(&p, &outline, view::Selection::ProposedStock, &[], &[], &[], &[], None), view::Card::Nothing);
    assert!(view::context_actions(view::Selection::ProposedStock).iter().any(|(id, _)| *id == "review.accept"));
    assert!(!view::context_actions(view::Selection::ProposedStock).iter().any(|(id, _)| *id == "review.reject"), "the stock is accepted or left; a feature row is rejected");
    assert!(view::context_actions(view::Selection::Review(0)).iter().any(|(id, label)| *id == "review.reject" && *label == "Reject"));
    assert_eq!(view::action("review.reject").unwrap().key, "");
    assert!(view::tab_shows(SidebarTab::Review, view::Selection::ProposedStock));
    assert!(!view::tab_shows(SidebarTab::Outline, view::Selection::ProposedStock));
    assert_eq!(view::selection_sentence(&p, &outline, view::Selection::ProposedStock, &[], &[], &[]), "Proposed stock");
}

/// A model that is not a prism gets the smallest blank it fits (ROADMAP
/// item 1, slice 2): one line, the row named by its size, the file's own
/// comment saying it is not the model's outline, `z=` read as the top.
#[test]
fn test_a_bounding_blank_proposal_is_one_line_named_by_its_size() {
    let proposal = std::fs::read_to_string("tests/fixtures/disc_step.stock.golden.part").unwrap();
    let p = parse::Part::default();
    let s = view::stock_proposal(&proposal, &p).unwrap();
    assert_eq!(s.head, "Stock  Cylinder ⌀120 × 30");
    assert_eq!(s.statement, "stock Cylinder(dia=120.0, h=30.0, x=0.0, y=0.0, z=30.0)");
    assert!(s.text.contains("the smallest blank the model fits"), "the card says it is not the model's own outline");
    assert!(!s.in_project);
    let accepted = parse::parse(&nacs::template::project_text_from_proposal(&proposal).unwrap()).unwrap();
    assert_eq!((accepted.stock.z, accepted.stock.h, accepted.stock.w), (30.0, 30.0, 120.0), "z= is the top");
    assert!(view::stock_proposal(&proposal, &accepted).unwrap().in_project, "the same blank, read off the program");
    let plate = std::fs::read_to_string("tests/fixtures/chamfer_plate.stock.golden.part").unwrap();
    assert_eq!(view::stock_proposal(&plate, &p).unwrap().head, "Stock  Box 100 × 80 × 28");
}

// ---- After Phase 17's first live run: restore, the inverse of suppress ----

/// Restore is suppress's exact inverse on the file: suppress a run line,
/// restore it, and the bytes are the original's — and the Suppressed
/// scan sees exactly the lines the file holds back, never the program's.
#[test]
fn test_restore_line_is_the_byte_inverse_of_suppress_and_the_scan_finds_only_suppressed_runs() {
    let dir = "plate.cam";
    let before = std::fs::read_to_string(format!("{dir}/main.part")).unwrap();
    let p = parse::load_project(dir).unwrap();
    let (_, line) = p.op_lines[2];
    assert!(view::suppressed_ops(&[&before]).is_empty(), "nothing suppressed in the shipped file");
    let suppressed = view::suppress_line(&before, line).unwrap();
    let found = view::suppressed_ops(&[&before, &suppressed]);
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].file, found[0].line), (1, line), "file index and line as the scan walks them");
    assert!(found[0].text.starts_with("run Side("), "{}", found[0].text);
    let restored = view::restore_line(&suppressed, line).unwrap();
    assert_eq!(restored, before, "the round trip is byte-identical");
    // A hand-typed `#run` (no space) restores too; leading indentation is kept.
    assert_eq!(view::restore_line("  #run End(tool=T1)\n", 0).unwrap(), "  run End(tool=T1)\n");
    assert_eq!(view::restore_line("#  run End(tool=T1)\n", 0).unwrap(), " run End(tool=T1)\n", "one space after the # is suppress's own; a second is the author's");
    // Refusals by name: a live run line, a comment that is not a run, past the end.
    let e = view::restore_line(&before, line).unwrap_err();
    assert!(e.contains("not a suppressed run statement"), "{e}");
    let e = view::restore_line("# a comment\n", 0).unwrap_err();
    assert!(e.contains("not a suppressed run statement"), "{e}");
    assert!(view::restore_line("x\n", 5).unwrap_err().contains("past the end"));
    // The scan ignores ordinary comments and live runs.
    assert!(view::suppressed_ops(&["# run-of-the-mill comment\nrun End(tool=T1)\n"]).is_empty());
    assert_eq!(view::suppressed_ops(&["#run End(tool=T1)\n"]).len(), 1);
}

// ---- Phase 18 slice 1: reorder (DESIGN.md §1 the fourth write, §5.2) ----

/// The swapped file emits what a hand-swapped file emits — pinned as
/// equality with an in-memory permutation of the parsed program — and a
/// swap is its own inverse on the bytes, comments between the two lines
/// included (plate.cam has two; a cut-and-paste hoisted them).
#[test]
fn test_swap_run_lines_reorders_the_program_and_emits_the_permutation() {
    for dir in ["chamfer.cam", "floor_restart.cam", "plate.cam"] {
        let before = std::fs::read_to_string(format!("{dir}/main.part")).unwrap();
        let geom = std::fs::read_to_string(format!("{dir}/geometry.part")).ok();
        let p = parse::load_project(dir).unwrap();
        let n = p.operations.len();
        assert!(n >= 2, "{dir}");
        for i in 0..n - 1 {
            let (fa, la) = p.op_lines[i];
            let (fb, lb) = p.op_lines[i + 1];
            assert_eq!(fa, fb, "{dir}: both in main.part");
            // Down: op i's line lands at op i+1's index.
            let down = view::swap_run_lines(&before, la, lb).unwrap();
            let q = parse::parse_project_with_base(&down, geom.as_deref(), Some(std::path::Path::new(dir))).unwrap();
            assert_eq!(q.operations.len(), n);
            let mut permuted = p.clone();
            permuted.operations.swap(i, i + 1);
            assert_eq!(q.operations[i], permuted.operations[i], "{dir}: op {i} after the move");
            assert_eq!(q.operations[i + 1], permuted.operations[i + 1]);
            assert_eq!(gcode::program(&q).unwrap(), gcode::program(&permuted).unwrap(), "{dir}: the moved file emits the permutation");
            // And back up: the bytes are the original's.
            let (_, la2) = q.op_lines[i + 1];
            let (_, lb2) = q.op_lines[i];
            assert_eq!(view::swap_run_lines(&down, la2, lb2).unwrap(), before, "{dir}: down then up is the identity");
        }
    }
    // Refusals by name.
    let src = "stock Box(w=120, l=80, h=20, x=0, y=0, z=0)\nrun End(tool=T5, surface=Top, stepover=6.0)\nrun End(tool=T5, surface=Top, stepover=6.0)";
    assert!(view::swap_run_lines(src, 1, 1).unwrap_err().contains("with itself"));
    assert!(view::swap_run_lines(src, 0, 1).unwrap_err().contains("not a run statement"));
    assert!(view::swap_run_lines(src, 1, 9).unwrap_err().contains("past the end"));
    // A last line without a newline moves up whole, and the file stays well-formed.
    let up = view::swap_run_lines(src, 2, 1).unwrap();
    assert_eq!(up.lines().count(), 3);
    assert!(up.lines().nth(1).unwrap().starts_with("run End"));
}

/// The refit door: the first frame fits once, and fullscreen changes the
/// viewport under that fit; `view.fit` asks for it again. Keyless until
/// 2026-09-17, when §9's ban was lifted for a named list — `f` and `Home`
/// now (the pin of the list is below, Phase 23 slice 1).
#[test]
fn test_the_fit_is_an_act_the_palette_finds() {
    assert_eq!(view::action("view.fit").unwrap().key, "f");
    assert!(view::palette_matches("fit").iter().any(|a| a.id == "view.fit"));
}

// ---- Phase 18 slice 2: renumber (DESIGN.md §1 the fifth write) ----

/// `set_key` is the one text edit under every field and numbering act:
/// set, append, remove — the rest of the line byte for byte, comment kept.
#[test]
fn test_set_key_rewrites_one_pair_and_keeps_every_other_byte() {
    let l = "run End(tool=T5, surface=Top, stepover=6.0)  # note\n";
    assert_eq!(view::set_key(l, "tool", Some("T1")).unwrap(), "run End(tool=T1, surface=Top, stepover=6.0)  # note\n");
    assert_eq!(view::set_key(l, "n", Some("10")).unwrap(), "run End(tool=T5, surface=Top, stepover=6.0, n=10)  # note\n");
    assert_eq!(view::set_key(l, "surface", None).unwrap(), "run End(tool=T5, stepover=6.0)  # note\n");
    assert_eq!(view::set_key("run End()\n", "n", Some("10")).unwrap(), "run End(n=10)\n");
    assert!(view::set_key(l, "n", None).unwrap_err().contains("no `n`"));
    assert!(view::set_key("feed 800\n", "n", Some("1")).unwrap_err().contains("no argument list"));
    // A list value hides no key: the comma inside [..] is not a separator.
    let c = "feature P ProfilePocket(points=[[0,0],[10,0]], extent=5)\n";
    assert_eq!(view::set_key(c, "extent", Some("7")).unwrap(), "feature P ProfilePocket(points=[[0,0],[10,0]], extent=7)\n");
}

/// Renumbering tools permutes the T, D and H words of the emitted program
/// — the tool change, the comp register, the length-offset register, all
/// derived from the number — and changes nothing else: on every shipped
/// fixture, the renumbered file's emission equals the original's with
/// every `T<n>`/`D<n>`/`H<n>` mapped.
#[test]
fn test_renumber_tools_permutes_the_t_d_and_h_words_and_nothing_else() {
    fn map_words(text: &str, map: &[(usize, usize)]) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::with_capacity(text.len());
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if matches!(c, 'T' | 'D' | 'H') && (i == 0 || !chars[i - 1].is_ascii_alphanumeric()) {
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_ascii_digit() {
                    j += 1;
                }
                if j > i + 1 && (j == chars.len() || !chars[j].is_ascii_alphanumeric()) {
                    let n: usize = chars[i + 1..j].iter().collect::<String>().parse().unwrap();
                    let m = map.iter().find(|(f, _)| *f == n).map(|(_, t)| *t).unwrap_or(n);
                    out.push(c);
                    out.push_str(&m.to_string());
                    i = j;
                    continue;
                }
            }
            out.push(c);
            i += 1;
        }
        out
    }
    let mut checked = 0;
    let mut changed = 0;
    for entry in std::fs::read_dir(".").unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().to_string();
        if !name.ends_with(".cam") {
            continue;
        }
        let before = std::fs::read_to_string(format!("{name}/main.part")).unwrap();
        let geom = std::fs::read_to_string(format!("{name}/geometry.part")).ok();
        let base = std::path::Path::new(&name);
        let Ok(p) = parse::parse_project_with_base(&before, geom.as_deref(), Some(base)) else { continue };
        let Ok(original) = gcode::program(&p) else { continue };
        let (after, map) = view::renumber_tools(&before).unwrap();
        let q = parse::parse_project_with_base(&after, geom.as_deref(), Some(base)).unwrap_or_else(|e| panic!("{name}: renumbered file fails to parse: {e}"));
        let renumbered = gcode::program(&q).unwrap();
        // The header's tool table is emitted in tool-NUMBER order, so a
        // renumbering reorders those lines too: canonicalize the table's
        // order on both sides; every other line stays where it is.
        fn table_sorted(text: &str) -> String {
            // An entry is its `(tool T…` line plus the `(  …` continuation
            // lines under it (a form tool's effective-diameter note).
            let lines: Vec<&str> = text.lines().collect();
            let Some(start) = lines.iter().position(|l| l.starts_with("(tool T")) else { return text.to_string() };
            let mut end = start;
            while end < lines.len() && (lines[end].starts_with("(tool T") || lines[end].starts_with("(  ")) {
                end += 1;
            }
            let mut entries: Vec<Vec<&str>> = Vec::new();
            for l in &lines[start..end] {
                if l.starts_with("(tool T") {
                    entries.push(vec![l]);
                } else if let Some(e) = entries.last_mut() {
                    e.push(l);
                }
            }
            entries.sort();
            let mut out: Vec<&str> = lines[..start].to_vec();
            for e in &entries {
                out.extend(e.iter().copied());
            }
            out.extend_from_slice(&lines[end..]);
            out.join("\n")
        }
        let expected = table_sorted(&map_words(&original, &map));
        let renumbered = table_sorted(&renumbered);
        if renumbered != expected {
            let (a, b): (Vec<&str>, Vec<&str>) = (expected.lines().collect(), renumbered.lines().collect());
            let k = a.iter().zip(b.iter()).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
            panic!(
                "{name}: the emission changed beyond its T/D/H words at line {} (map {map:?}):\n  expected: {}\n  got:      {}",
                k + 1,
                a.get(k).unwrap_or(&"<end>"),
                b.get(k).unwrap_or(&"<end>")
            );
        }
        assert_eq!(q.operations.len(), p.operations.len());
        assert_eq!(q.tools.len(), p.tools.len());
        checked += 1;
        if after != before {
            changed += 1;
            // The map is a bijection onto 1..=n in first-use order.
            let mut targets: Vec<usize> = map.iter().map(|(_, t)| *t).collect();
            targets.sort();
            assert_eq!(targets, (1..=map.len()).collect::<Vec<_>>(), "{name}");
        }
    }
    assert!(checked >= 80 && changed >= 10, "checked {checked}, changed {changed}");
    // The first-use order, on a fixture that declares out of order: plate uses T5 alone.
    let plate = std::fs::read_to_string("plate.cam/main.part").unwrap();
    let (after, map) = view::renumber_tools(&plate).unwrap();
    assert_eq!(map, vec![(5, 1)]);
    assert!(after.contains("tool T1 Endmill(dia=10.0)") && after.contains("run End(tool=T1,"));
    assert!(after.contains("# tool T<number> <name>(dia=<diameter>)"), "a comment is prose and stays");
    assert!(view::renumber_tools("run End(tool=T9, surface=Top)\n").unwrap_err().contains("T9"));

}
/// Live run (2026-08-30): two tool lines both declared T5 and renumber
/// rewrote both to T1 — faithful to the number-keyed emission, useless to
/// the author, whose first declaration was already silently shadowed. The
/// parser now refuses the duplicate at declaration; this pins the act's
/// own refusal, which guards the file even when it changed on disk after
/// the load.
#[test]
fn test_renumber_refuses_duplicate_tool_declarations() {
    let t = "tool T5 Endmill(dia=6.0)\ntool T5 Endmill(dia=10.0)\nrun End(tool=T5, surface=Top)\n";
    let e = view::renumber_tools(t).unwrap_err();
    assert!(e.contains("both declare T5"), "{e}");
}

/// Numbering by tens puts `n=10, 20, …` on every run line and changes
/// only the N words of the emission; stripping removes them all, and the
/// two are inverses up to the numbers the author chose.
#[test]
fn test_number_ops_and_strip_change_only_the_n_words() {
    let dir = "floor_restart.cam";
    let before = std::fs::read_to_string(format!("{dir}/main.part")).unwrap();
    let p = parse::load_project(dir).unwrap();
    let original = gcode::program(&p).unwrap();
    let numbered = view::number_ops(&before, 10).unwrap();
    let q = parse::parse_project(&numbered, None).unwrap();
    let emitted = gcode::program(&q).unwrap();
    fn bodies(t: &str) -> Vec<String> {
        t.lines().map(|l| gcode::strip_n_word(l).to_string()).collect()
    }
    assert_eq!(bodies(&emitted), bodies(&original), "only the N words moved");
    let rungs: Vec<&str> = emitted.lines().filter_map(|l| gcode::split_n_word(l).map(|(n, _)| n)).collect();
    assert_eq!(rungs, vec!["N10", "N20", "N30", "N40"], "{rungs:?}");
    assert_eq!(numbered.lines().filter(|l| l.contains(", n=")).count(), 4);
    let stripped = view::strip_op_numbers(&numbered).unwrap();
    assert!(!stripped.contains("n="));
    let r = parse::parse_project(&stripped, None).unwrap();
    assert_eq!(bodies(&gcode::program(&r).unwrap()), bodies(&original));
    assert!(gcode::program(&r).unwrap().lines().all(|l| gcode::split_n_word(l).is_none()));
    // Stripping the shipped file, whose rungs are hand-placed, and numbering it back are not the identity — the author's 10/15/30 are theirs.
    assert_ne!(view::number_ops(&view::strip_op_numbers(&before).unwrap(), 10).unwrap(), before);
    for id in ["tools.renumber", "ops.number", "ops.strip_numbers"] {
        assert_eq!(view::action(id).unwrap().key, "", "{id} is palette-only");
    }
}


// ---- Phase 18 slice 3: fields (DESIGN.md §1 the sixth write, §5.4) ----

/// The statement tables carry each line's own text, and the fields come
/// from that text — never from a rendered sentence (I7); geometry.part's
/// statements are stamped as file 1; a block statement offers no fields.
#[test]
fn test_statement_tables_carry_the_line_text_and_fields_come_from_it() {
    let p = parse::load_project("plate.cam").unwrap();
    assert_eq!(p.op_texts.len(), p.op_lines.len());
    assert_eq!(p.op_texts[0], "run End(tool=T5, surface=TopFace, stepover=6.0)");
    let f = p.feature_lines.iter().find(|(n, ..)| n == "TopFace").unwrap();
    assert_eq!((f.1, f.2, f.3.as_str()), (0, 16, "feature TopFace Plane(z=-0.5)"));
    let t = p.tool_lines.iter().find(|(n, ..)| *n == 5).unwrap();
    assert_eq!((t.1, t.2, t.3.as_str()), (0, 11, "tool T5 Endmill(dia=10.0)"));
    assert!(p.feature_lines.iter().any(|(_, file, ..)| *file == 1), "plate's geometry.part features are file 1");
    assert_eq!(
        view::fields_of(&p.op_texts[0]),
        vec![("tool".to_string(), "T5".to_string()), ("surface".to_string(), "TopFace".to_string()), ("stepover".to_string(), "6.0".to_string())]
    );
    assert_eq!(view::fields_of("feature P ProfilePocket(points=[[0,0],[10,0]], extent=5)").len(), 2, "a list value is one field");
    assert!(view::fields_of("feed 800").is_empty());
    // A block statement's first line has no `)`: since Phase 50 slice 6 its
    // OPENER's keys are fields, and set_key rewrites the opener alone.
    let c = parse::load_project("contour_boss.cam").unwrap();
    let crest = c.feature_lines.iter().find(|(n, ..)| n == "Crest").unwrap();
    assert_eq!(view::fields_of(&crest.3), vec![("extent".to_string(), "0".to_string())], "{}", crest.3);
    assert_eq!(view::set_key(&crest.3, "extent", Some("1")).unwrap(), crest.3.replace("extent=0", "extent=1"));
    // The inspector and the tool card carry the form (Phase 21 slice 1):
    // the present keys with the file's values, in the inventory's order.
    let outline = nacs::outline::outline(&p);
    let edges = view::feature_edges(&p);
    let paths = view::op_paths(&p, &[]);
    let present = |form: &Result<view::StatementForm, String>| -> Vec<(String, String)> {
        form.as_ref().unwrap().fields.iter().filter_map(|f| f.value.clone().map(|v| (f.key.clone(), v))).collect()
    };
    match view::card(&p, &outline, view::Selection::Operation(0), &[], &edges, &paths, &[], None) {
        view::Card::Operation { form, tool, .. } => {
            assert_eq!(present(&form), view::fields_of(&p.op_texts[0]));
            assert!(tool.is_none() || present(&tool.unwrap().form) == vec![("dia".to_string(), "10.0".to_string())]);
        }
        other => panic!("{other:?}"),
    }
    let ti = edges.iter().position(|e| e.name == "TopFace").unwrap();
    match view::card(&p, &outline, view::Selection::Feature(ti), &[], &edges, &paths, &[], None) {
        view::Card::Feature { form, .. } => assert_eq!(present(&form), vec![("z".to_string(), "-0.5".to_string())]),
        other => panic!("{other:?}"),
    }
}

/// The sixth write as a pure chain: set a key, splice the line, re-parse
/// the whole project — the value reads back through the program and
/// through `--outline`; a value the parser refuses yields the parser's
/// sentence and the chain stops before any write.
#[test]
fn test_a_field_round_trips_through_the_parser_and_the_outline_or_is_refused_by_the_parser() {
    let dir = "plate.cam";
    let before = std::fs::read_to_string(format!("{dir}/main.part")).unwrap();
    let geom = std::fs::read_to_string(format!("{dir}/geometry.part")).ok();
    let base = Some(std::path::Path::new(dir));
    let p = parse::parse_project_with_base(&before, geom.as_deref(), base).unwrap();
    // The operation: stepover 6.0 → 7.5.
    let (_, line) = p.op_lines[0];
    let new_line = view::set_key(&p.op_texts[0], "stepover", Some("7.5")).unwrap();
    let after = view::replace_line(&before, line, &new_line).unwrap();
    let q = parse::parse_project_with_base(&after, geom.as_deref(), base).unwrap();
    assert_eq!(q.operations[0].stepover, Some(7.5));
    assert_eq!(q.op_texts[0], new_line, "the table follows the file");
    // The outline does not print a stepover; it reads back through the program (above).
    assert_eq!(after.lines().count(), before.lines().count());
    assert_eq!(after.lines().filter(|l| l != &new_line.as_str()).count(), before.lines().count() - 1, "one line changed");
    // The feature: z −0.5 → −1.0.
    let (_, _, fl, ft) = p.feature_lines.iter().find(|(n, ..)| n == "TopFace").unwrap().clone();
    let after_f = view::replace_line(&before, fl, &view::set_key(&ft, "z", Some("-1.0")).unwrap()).unwrap();
    let qf = parse::parse_project_with_base(&after_f, geom.as_deref(), base).unwrap();
    assert!(matches!(qf.features.get("TopFace"), Some(parse::Feature::Plane { z }) if (*z + 1.0).abs() < 1e-12));
    assert!(nacs::outline::report(&qf).contains("z -1.000..0.000"), "the outline reads the new z back:\n{}", nacs::outline::report(&qf));
    // The tool: dia 10.0 → 8.0.
    let (_, _, tl, tt) = p.tool_lines.iter().find(|(n, ..)| *n == 5).unwrap().clone();
    let after_t = view::replace_line(&before, tl, &view::set_key(&tt, "dia", Some("8.0")).unwrap()).unwrap();
    let qt = parse::parse_project_with_base(&after_t, geom.as_deref(), base).unwrap();
    assert_eq!(qt.tools[&5].dia, 8.0);
    assert!(nacs::outline::report(&qt).contains("dia 8.000"), "the outline reads the new dia back");
    // Refused by the parser, before any write: the sentence names the line.
    let bad = view::replace_line(&before, line, &view::set_key(&p.op_texts[0], "stepover", Some("abc")).unwrap()).unwrap();
    let e = parse::parse_project_with_base(&bad, geom.as_deref(), base).unwrap_err();
    assert!(e.contains(&format!("line {}", line + 1)) || e.contains("stepover"), "{e}");
    // replace_line keeps every other byte and refuses past the end.
    assert!(view::replace_line("a\nb\n", 5, "x").unwrap_err().contains("past the end"));
    assert_eq!(view::replace_line("a\nb", 1, "x\n").unwrap(), "a\nx", "a last line without a newline stays without one");
}

/// Phase 20 slice 4: a reference field is a menu of what the project
/// declares — the outline's tool numbers, the declared feature names —
/// never a filter by kind, and nothing for a key that is not a
/// reference. The live run's lesson made permanent: on a one-tool
/// project the menu has one entry, and a CHANGED legal value lands.
#[test]
fn test_reference_fields_offer_the_declared_inventory_and_a_changed_value_lands() {
    let p = parse::load_project("floor_restart.cam").unwrap();
    assert_eq!(view::reference_options(&p, "tool"), Some(vec!["T1".to_string(), "T2".to_string()]));
    assert_eq!(view::reference_options(&p, "surface"), Some(vec!["Fin".to_string(), "Low".to_string(), "Mid".to_string(), "Top".to_string()]), "sorted, never HashMap order");
    assert_eq!(view::reference_options(&p, "profile"), view::reference_options(&p, "surface"), "the menu never filters by kind — the parser judges");
    assert_eq!(view::reference_options(&p, "stepover"), None);
    assert_eq!(view::reference_options(&p, "points"), None, "a list is several references; named out");
    let plate = parse::load_project("plate.cam").unwrap();
    assert_eq!(view::reference_options(&plate, "tool"), Some(vec!["T5".to_string()]), "one legal entry, and it shows");

    // A changed legal value lands: op 0's tool T1 → T2 through set_key,
    // re-parsed, and the program's op runs T2.
    let text = std::fs::read_to_string("floor_restart.cam/main.part").unwrap();
    let (_, line) = p.op_lines[0];
    // (T2 is 6mm and op 0's stepover is 6: the stepover's ceiling, Phase 43
    // slice 4, would refuse the tool change alone, as it should — so the
    // stepover moves with it, as a hand would move it.)
    let retooled = view::set_key(&p.op_texts[0], "tool", Some("T2")).unwrap();
    let new_line = view::set_key(&retooled, "stepover", Some("3")).unwrap();
    let after = view::replace_line(&text, line, &new_line).unwrap();
    let q = parse::parse_project(&after, None).unwrap();
    assert_eq!(q.operations[0].tool_num, 2);
    // A wrong-kind reference is the parser's refusal, not the menu's:
    // plate.cam's Finish on its pocket pointed at the Plane.
    let ptext = std::fs::read_to_string("plate.cam/main.part").unwrap();
    let (_, pl) = plate.op_lines[2];
    let redirected = view::replace_line(&ptext, pl, &view::set_key(&plate.op_texts[2], "surface", Some("TopFace")).unwrap()).unwrap();
    let err = parse::parse_project(&redirected, Some(&std::fs::read_to_string("plate.cam/geometry.part").unwrap())).unwrap_err();
    assert!(err.contains("TopFace") || err.contains("Plane"), "the parser's own sentence: {err}");
}

// ---- Phase 21 slice 1: the form is the spec (DESIGN.md §5.4) ----

/// The form offers every key the statement's kind TAKES, not only the
/// keys the line HAS: a tool declared without a holder shows `holder`
/// blank and optional; a run line shows the whole run whitelist with the
/// skeleton's keys required; a feature shows its spec's keys. The
/// inventory is read from the specs and the parser, and the values from
/// the line — a present key carries the file's value.
#[test]
fn test_statement_form_offers_every_key_the_kind_takes_with_the_line_values() {
    let p = parse::load_project("plate.cam").unwrap();
    // The tool: `tool T5 Endmill(dia=10.0)` — one present, the ladder blank.
    let tool = view::statement_form(&p, &view::Statement::Tool(5)).unwrap();
    assert_eq!(tool.head, "Endmill");
    let spec = nacs::template::tool_spec("Endmill").unwrap();
    assert_eq!(
        tool.fields.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(),
        nacs::template::tool_keys(spec).iter().map(|k| k.name).collect::<Vec<_>>(),
        "the spec's order, edit and creation one form"
    );
    let dia = tool.fields.iter().find(|f| f.key == "dia").unwrap();
    assert_eq!((dia.value.as_deref(), dia.required, dia.options.is_none()), (Some("10.0"), true, true));
    let holder = tool.fields.iter().find(|f| f.key == "holder").unwrap();
    assert_eq!((holder.value.as_deref(), holder.required), (None, false), "the live run's finding: a blank the line lacks");

    // The run: `run End(tool=T5, surface=TopFace, stepover=6.0)` — the
    // whole whitelist offered, the line's two references required (its
    // identity; the numbers are the parser's either-or questions), the
    // references as menus (Phase 20 slice 4), the rest blank.
    let run = view::statement_form(&p, &view::Statement::Op(0)).unwrap();
    assert_eq!(run.head, "End");
    let offered: Vec<&str> = run.fields.iter().map(|f| f.key.as_str()).collect();
    let expected: Vec<&str> = parse::RUN_KEYS.iter().copied().filter(|k| !matches!(*k, "profile" | "points")).collect();
    assert_eq!(offered, expected, "every whitelist key is offered in the parser's order, except the references the line does not carry");
    let required: Vec<&str> = run.fields.iter().filter(|f| f.required).map(|f| f.key.as_str()).collect();
    assert_eq!(required, vec!["tool", "surface"]);
    let stepover = run.fields.iter().find(|f| f.key == "stepover").unwrap();
    assert_eq!((stepover.value.as_deref(), stepover.required), (Some("6.0"), false), "the parser says an End needs it, and the parser will");
    let tool_field = run.fields.iter().find(|f| f.key == "tool").unwrap();
    assert_eq!((tool_field.value.as_deref(), tool_field.options.clone()), (Some("T5"), view::reference_options(&p, "tool")));
    assert!(run.fields.iter().all(|f| f.key != "profile" && f.key != "points"), "an End offers no second reference — the live run filled one and nothing read it");
    let spindle = run.fields.iter().find(|f| f.key == "spindle").unwrap();
    assert_eq!((spindle.value.as_deref(), spindle.required, spindle.options.is_none()), (None, false, true));

    // The feature: `feature TopFace Plane(z=-0.5)`.
    let plane = view::statement_form(&p, &view::Statement::Feature("TopFace".to_string())).unwrap();
    assert_eq!(plane.head, "Plane");
    assert_eq!(plane.fields.len(), 1);
    assert_eq!((plane.fields[0].key.as_str(), plane.fields[0].value.as_deref(), plane.fields[0].required), ("z", Some("-0.5"), true));
    // A CylinderBore offers its seat keys blank — every optional of every
    // single-line kind reachable from the window.
    let c = parse::load_project("cylinder_bore.cam").unwrap();
    let name = c.feature_lines[0].0.clone();
    let bore = view::statement_form(&c, &view::Statement::Feature(name)).unwrap();
    assert_eq!(bore.head, "CylinderBore");
    assert_eq!(bore.fields.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(), nacs::template::feature_spec("CylinderBore").unwrap().keys.iter().map(|k| k.name).collect::<Vec<_>>());
    assert!(bore.fields.iter().any(|f| f.key == "cbore_dia" && f.value.is_none() && !f.required));

    // Declines by name: a block statement (as today), an undeclared
    // tool, a feature that is not declared, an op past the end.
    let cb = parse::load_project("contour_boss.cam").unwrap();
    // A block kind's OPENER has a form since Phase 50 slice 6: the spec's
    // keys, the points not among them.
    let crest = view::statement_form(&cb, &view::Statement::Feature("Crest".to_string())).unwrap();
    assert_eq!(crest.head, "ContourBoss");
    assert!(crest.fields.iter().any(|f| f.key == "extent") && crest.fields.iter().all(|f| f.key != "points"));
    assert!(view::statement_form(&p, &view::Statement::Tool(99)).unwrap_err().contains("T99 is not declared"));
    assert!(view::statement_form(&p, &view::Statement::Feature("Nope".to_string())).unwrap_err().contains("not declared"));
    assert!(view::statement_form(&p, &view::Statement::Op(99)).unwrap_err().contains("no operation [99]"));
}

/// A kind word nobody knows never reaches a form: the parser refuses it
/// (Phase 36 slice 0), so every tool a form opens on has a spec.
#[test]
fn test_a_tool_of_an_unknown_kind_word_never_reaches_a_form() {
    let text = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\ntool T1 Endmil(dia=10, holder=BT30)\nfeature Top Plane(z=0)\nrun End(tool=T1, surface=Top, stepover=5)\n";
    let err = parse::parse_project(text, None).unwrap_err();
    assert!(err.contains("not a tool kind"), "{err}");
    // A visit's reference is its `points=[…]` list: required with the
    // tool; `cycle` is the parser's question.
    let text = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\ntool T1 Drill(dia=6)\nfeature Hole CylinderBore(dia=6, extent=5, x=10, y=10)\nrun Form(points=[Hole], tool=T1, cycle=Simple)\n";
    let p = parse::parse_project(text, None).unwrap();
    let form = view::statement_form(&p, &view::Statement::Op(0)).unwrap();
    assert_eq!(form.head, "Form");
    assert_eq!(form.fields.iter().filter(|f| f.required).map(|f| f.key.as_str()).collect::<Vec<_>>(), vec!["tool", "points"], "in the whitelist's order");
    assert_eq!(form.fields.len(), parse::RUN_KEYS.len() - 2, "the whitelist less the two references a visit to points does not carry");
}

/// What a draft writes: a value sets or appends; an emptied optional
/// removes; an emptied required key is refused by name before the parser
/// is asked; a key the form does not offer is refused too.
#[test]
fn test_field_write_appends_removes_and_refuses_to_clear_a_required_key() {
    let p = parse::load_project("plate.cam").unwrap();
    let run = view::statement_form(&p, &view::Statement::Op(0)).unwrap();
    assert_eq!(view::field_write(&run, "spindle", " 4000 ").unwrap(), Some("4000".to_string()), "trimmed, a value to append");
    assert_eq!(view::field_write(&run, "stepover", "7").unwrap(), Some("7".to_string()));
    assert_eq!(view::field_write(&run, "stepover", "  ").unwrap(), None, "a present optional cleared removes — and the parser will refuse an End without one, in its own sentence");
    assert_eq!(view::field_write(&run, "surface", "").unwrap_err(), "an End needs surface");
    assert_eq!(view::field_write(&run, "tool", "").unwrap_err(), "an End needs tool");
    assert!(view::field_write(&run, "spindle", "").unwrap_err().contains("already absent"), "nothing to remove");
    assert!(view::field_write(&run, "depth", "3").unwrap_err().contains("has no `depth`"));
    let tool = view::statement_form(&p, &view::Statement::Tool(5)).unwrap();
    assert_eq!(view::field_write(&tool, "dia", "").unwrap_err(), "an Endmill needs dia");
    // The parser's refusal, end to end: the stepover removed from the
    // text, the whole project re-parsed, the parser's own sentence.
    let text = std::fs::read_to_string("plate.cam/main.part").unwrap();
    let (_, line) = p.op_lines[0];
    let without = view::replace_line(&text, line, &view::set_key(&p.op_texts[0], "stepover", None).unwrap()).unwrap();
    let err = parse::parse_project(&without, Some(&std::fs::read_to_string("plate.cam/geometry.part").unwrap())).unwrap_err();
    assert!(err.contains("stepover"), "{err}");
}

/// The acceptance chain, as text: a tool made without a holder gains one
/// from the form and loses it again, the file each time the text the user
/// could have typed — and, after the removal, byte-identical to before.
/// Every optional of the tool's ladder makes the same round trip.
#[test]
fn test_a_tool_without_a_holder_grows_one_and_loses_it_again_byte_for_byte() {
    let before = std::fs::read_to_string("plate.cam/main.part").unwrap();
    let geom = std::fs::read_to_string("plate.cam/geometry.part").unwrap();
    let p = parse::parse_project(&before, Some(&geom)).unwrap();
    let (_, _, line, text) = p.tool_lines.iter().find(|(n, ..)| *n == 5).unwrap().clone();
    assert_eq!(text, "tool T5 Endmill(dia=10.0)");
    assert!(p.tools[&5].holder.is_none());
    let form = view::statement_form(&p, &view::Statement::Tool(5)).unwrap();
    // Grow: the form's blank given a value, appended by set_key.
    let value = view::field_write(&form, "holder", "BT30").unwrap();
    let grown_line = view::set_key(&text, "holder", value.as_deref()).unwrap();
    assert_eq!(grown_line, "tool T5 Endmill(dia=10.0, holder=BT30)");
    let grown = view::replace_line(&before, line, &grown_line).unwrap();
    let q = parse::parse_project(&grown, Some(&geom)).unwrap();
    assert_eq!(q.tools[&5].holder.as_deref(), Some("BT30"), "the program reads the holder");
    // Lose: the present optional cleared, removed by set_key(None).
    let form = view::statement_form(&q, &view::Statement::Tool(5)).unwrap();
    let value = view::field_write(&form, "holder", "").unwrap();
    assert_eq!(value, None);
    let shrunk_line = view::set_key(&grown_line, "holder", value.as_deref()).unwrap();
    let shrunk = view::replace_line(&grown, line, &shrunk_line).unwrap();
    assert_eq!(shrunk, before, "the round trip is the identity, byte for byte");
    // Every optional the kind takes makes the text trip — appended, then
    // removed, the file byte-identical — so the whole ladder is reachable
    // and removable from the window. Whether the ladder's rung stands
    // ALONE is the parser's (a shank needs a flute_len under it): it
    // either parses or refuses with a sentence naming the key, and the
    // form never pre-judges it.
    let sample = |key: &str| -> &str {
        match key {
            "holder" => "BT30",
            "coolant" => "flood",
            "flute_count" => "4",
            _ => "20",
        }
    };
    for f in form.fields.iter().filter(|f| !f.required) {
        let l = view::set_key(&text, &f.key, Some(sample(&f.key))).unwrap();
        let t = view::replace_line(&before, line, &l).unwrap();
        if let Err(e) = parse::parse_project(&t, Some(&geom)) {
            assert!(e.contains("T5"), "{}: the parser's own sentence, about this tool: {e}", f.key);
        }
        assert_eq!(view::replace_line(&t, line, &view::set_key(&l, &f.key, None).unwrap()).unwrap(), before);
    }
}

// ---- Phase 21 slice 2: the Sequence (DESIGN.md §5.2) — folded 2026-10-17 ----

/// The Sequence was the fourth sidebar view from 2026-09-04 until Phase 52
/// slice 4 folded it into the Outline's Operations group: three tabs now,
/// no `sidebar.sequence` act, and a workspace file that still says
/// `sidebar.tab sequence` opens on the Outline — the fall-to-default rule,
/// as a `mode` line did. The run order itself is unchanged:
/// `outline::sequence` is the Operations group's list.
#[test]
fn test_the_sequence_folded_into_the_outline_and_its_file_token_falls_to_the_default() {
    assert_eq!(SidebarTab::ALL.to_vec(), vec![SidebarTab::Outline, SidebarTab::Problems, SidebarTab::Review]);
    assert_eq!(SidebarTab::from_key("sequence"), None);
    assert!(view::action("sidebar.sequence").is_none());
    let mut l = Layout::default();
    l.apply(DockAction::ShowTab(SidebarTab::Problems));
    let text = l.render().replace("sidebar.tab problems", "sidebar.tab sequence");
    assert!(text.contains("sidebar.tab sequence"), "{text}");
    assert_eq!(Layout::parse(&text).sidebar.tab, SidebarTab::Outline, "an old file's sequence line falls to the Outline");
}

// ---- the UI/UX review's rider (DECISIONS.md "The UI/UX review intake…") ----

/// Esc closes what is on top and nothing beneath it: the palette (it holds
/// the keyboard), else the help, else the unsent drafts, else the creation
/// form. Before the review one Esc did all four at once — closing the help
/// dropped a form full of typed values, silently.
#[test]
fn test_esc_closes_the_topmost_transient_and_nothing_beneath_it() {
    use view::{dismiss_target, Dismiss};
    assert_eq!(dismiss_target(true, true, true, true), Dismiss::Palette);
    assert_eq!(dismiss_target(true, false, true, true), Dismiss::Help);
    assert_eq!(dismiss_target(false, false, true, true), Dismiss::Drafts);
    assert_eq!(dismiss_target(false, false, false, true), Dismiss::Form);
    assert_eq!(dismiss_target(false, false, false, false), Dismiss::Nothing);
    let sentence = view::action("dismiss").map(|a| a.sentence).unwrap_or_default();
    assert!(sentence.contains("on top"), "{sentence}");
}

/// The Program's scroll-to-line lands at the pitch `show_rows` lays rows
/// at — the row height PLUS the spacing. Built from the height alone the
/// offset fell short by one spacing per line: 85 rows at line 500.
#[test]
fn test_the_program_scroll_offset_uses_the_row_pitch_not_the_row_height() {
    use view::program_scroll_offset;
    // egui's pitch: (row + spacing) per line; the target row centred.
    assert_eq!(program_scroll_offset(14.5, 3.0, 500, 400.0), 17.5 * 500.0 - 200.0);
    // Never negative: the first rows sit at the top.
    assert_eq!(program_scroll_offset(14.5, 3.0, 0, 400.0), 0.0);
    assert_eq!(program_scroll_offset(14.5, 3.0, 3, 400.0), 0.0);
    // The old arithmetic, for the record of how far it was off.
    let old = 14.5 * 500.0 - 200.0;
    let short_by_rows = (program_scroll_offset(14.5, 3.0, 500, 400.0) - old) / 17.5;
    assert!(short_by_rows > 80.0, "{short_by_rows}");
}

/// A write says it was written, in which file, and the way back; an undo
/// and a redo say the same. Every write used to end in silence.
#[test]
fn test_a_write_leaves_a_note_that_names_the_file_and_the_way_back() {
    assert_eq!(view::written_note("main.part", "operation 3 suppressed"), "Written — main.part: operation 3 suppressed · z undoes");
    assert_eq!(view::undone_note("main.part", false), "Undone — main.part · y redoes");
    assert_eq!(view::undone_note("main.part", true), "Redone — main.part · z undoes");
    assert!(view::transient_shown(0.0));
    assert!(view::transient_shown(view::TRANSIENT_SECS - 0.1));
    assert!(!view::transient_shown(view::TRANSIENT_SECS));
}

/// Several fields typed in are several drafts, each named — a draft in
/// one field used to be dropped, unannounced, the moment another was typed.
#[test]
fn test_every_unsent_draft_is_named() {
    let one = vec![("stepover".to_string(), "5".to_string())];
    assert_eq!(view::unsent_notes(&one).as_deref(), Some("Unsent — stepover = 5: Enter writes it, Esc drops it"));
    let two = vec![("stepover".to_string(), "5".to_string()), ("feed".to_string(), "800".to_string())];
    assert_eq!(view::unsent_notes(&two).as_deref(), Some("Unsent — stepover = 5, feed = 800: Enter writes each, Esc drops them"));
    assert_eq!(view::unsent_notes(&[]), None);
}

/// The help and the palette render one inventory and say "no key" the
/// same way — `·`, never `()`.
#[test]
fn test_the_keys_column_says_no_key_one_way() {
    assert_eq!(view::key_column(""), "·");
    assert_eq!(view::key_column("p"), "p");
    let help = view::help_lines().join("\n");
    assert!(help.contains("  ·              suppress the selected operation"), "{help}");
    assert!(!help.contains("()"), "{help}");
}

/// The help names every sidebar view, describes the tabs (the rail until
/// 2026-10-17), and no act's sentence carries the house's
/// ordinal for a write (DESIGN.md I13).
#[test]
fn test_the_help_is_current_and_speaks_no_house_vocabulary() {
    let help = view::help_lines().join("\n");
    for view_word in ["Outline", "Problems", "Review"] {
        assert!(help.contains(&format!("  {view_word:<14} the sidebar")), "{view_word} missing: {help}");
    }
    assert!(!help.contains("click a tab"), "{help}");
    for a in view::ACTIONS {
        for house in ["seventh write", "eighth write", "ninth write", "wizard"] {
            assert!(!a.sentence.contains(house), "{}: {}", a.id, a.sentence);
        }
    }
}

/// A creation act opens the form's dock and never toggles it: a form
/// composed into a shut dock was live and invisible.
#[test]
fn test_open_form_opens_and_never_toggles() {
    let mut l = Layout::default();
    l.apply(DockAction::ToggleForm);
    assert!(!l.form.open);
    l.apply(DockAction::OpenForm);
    assert!(l.form.open);
    l.apply(DockAction::OpenForm);
    assert!(l.form.open, "a second open is not a close");
}

/// An empty reference inventory says so and names the door — a menu
/// over nothing was a dead end for a new project's first operation.
#[test]
fn test_an_empty_reference_inventory_names_its_door() {
    assert!(view::empty_inventory_note("tool").contains("New tool…"));
    assert!(view::empty_inventory_note("surface").contains("New feature…"));
    assert!(view::empty_inventory_note("profile").contains("New feature…"));
    // The inventory itself: a project with no tools offers an empty menu,
    // never `None` (that would make `tool` a text field).
    let p = parse::parse_project_with_base(
        "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\nstock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n",
        None,
        None,
    )
    .expect("a stock-only project parses");
    assert_eq!(view::reference_options(&p, "tool"), Some(vec![]));
}

/// Dimmed toolpaths stay legible: at the built-in theme every role,
/// composited on the ground at `DIMMED_ALPHA`, measures at least 3:1
/// (WCAG, the floor for a graphic). At the old 60 they measured ~1.5:1
/// and selecting one operation erased the rest of the program.
#[test]
fn test_dimmed_toolpaths_keep_three_to_one_on_the_ground() {
    fn lin(c: u8) -> f64 {
        let s = f64::from(c) / 255.0;
        if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    }
    fn lum(c: [u8; 3]) -> f64 {
        0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
    }
    fn over(fg: [u8; 3], bg: [u8; 3], a: u8) -> [u8; 3] {
        let t = f64::from(a) / 255.0;
        let mix = |f: u8, b: u8| (f64::from(b) + (f64::from(f) - f64::from(b)) * t).round() as u8;
        [mix(fg[0], bg[0]), mix(fg[1], bg[1]), mix(fg[2], bg[2])]
    }
    let t = Theme::default();
    for role in [OpRole::Roughing, OpRole::Finishing, OpRole::Untagged] {
        let (rgb, a) = view::feed_paint(&t, role, false, true);
        assert_eq!(a, view::DIMMED_ALPHA);
        let (l1, l2) = (lum(over(rgb, t.ground, a)), lum(t.ground));
        let ratio = (l1.max(l2) + 0.05) / (l1.min(l2) + 0.05);
        assert!(ratio >= 3.0, "{role:?} dimmed measures {ratio:.2}:1");
        let (_, old) = (rgb, 60u8);
        let (o1, o2) = (lum(over(rgb, t.ground, old)), lum(t.ground));
        let old_ratio = (o1.max(o2) + 0.05) / (o1.min(o2) + 0.05);
        assert!(old_ratio < 2.0, "the old alpha was legible after all: {old_ratio:.2}:1");
    }
    let (_, full) = view::feed_paint(&t, OpRole::Finishing, false, false);
    assert_eq!(full, 255);
}

// ---- Phase 22 slice 1: the stock is a statement ----------------------------
//
// The sixth selection kind (DESIGN.md §6, `Stock`): no index, a project
// has one. Its row is `outline::stock_row` — the words `--outline` prints —
// its card is the `stock` line's form over the parser's own whitelist
// (`parse::STOCK_KEYS`), and the write is the sixth write unchanged: one
// key on one line, the whole project re-parsed first.

/// The card, the sentence, the pose and the reload: a stock selection is
/// the Outline's row and the line's form, stands no tool, lights no op or
/// feature, and survives every reload (a project always has a stock).
#[test]
fn test_the_stock_is_a_selection_with_the_outlines_row_and_the_lines_form() {
    use view::{Card, Selection};
    let p = load("plate.cam");
    let outline = nacs::outline::outline(&p);
    let v = check_result::check_result(&p);
    let (rows, annotations) = view::finding_rows(&v, p.stock.z, &Theme::default());
    let edges = view::feature_edges(&p);
    let paths = view::op_paths(&p, v.emitted_moves.as_deref().expect("plate emits"));
    match view::card(&p, &outline, Selection::Stock, &rows, &edges, &paths, &[], None) {
        Card::Stock { row, form, .. } => {
            assert_eq!(row, nacs::outline::stock_row(&outline.stock));
            assert_eq!(row, "stock  120.000 x 80.000 x 20.000 mm  origin (0.000, 0.000)  top z 0.000");
            assert_eq!(row, nacs::outline::report(&p).lines().next().unwrap(), "the row is the report's first line, verbatim");
            assert_eq!(form, view::statement_form(&p, &view::Statement::Stock));
        }
        other => panic!("the stock, not {other:?}"),
    }
    assert_eq!(view::tool_pose(&p, Selection::Stock, &rows, &paths), None, "the stock stands no tool");
    assert_eq!(view::selected_op(Selection::Stock, &rows), None);
    assert_eq!(view::selected_feature(Selection::Stock, &rows, &edges), None);
    assert_eq!(view::selection_anchor(Selection::Stock, &rows, &annotations), None, "selecting the stock moves nothing");
    assert_eq!(view::selection_sentence(&p, &outline, Selection::Stock, &rows, &edges, &[]), nacs::outline::stock_row(&outline.stock));
    // A reload keeps it whatever the counts do — even a program with no
    // operation, feature or tool has a stock.
    assert_eq!(view::after_reload(2, 3, Selection::Stock, 0, 0, 0, 0, &[]), (0, None, Selection::Stock));
    assert_eq!(view::after_reload(1, 3, Selection::Stock, 5, 2, 1, 0, &[7]), (1, None, Selection::Stock));
}

/// The form is the parser's whitelist in the parser's order, the three
/// sides required (the parser refuses a Box without them), the origin,
/// the top datum and the material optional; the values are the line's.
/// A project parsed from text records its stock line too; a project
/// with no stock line declines by name rather than inventing one.
#[test]
fn test_the_stock_form_offers_the_parsers_stock_keys_with_the_lines_values() {
    let p = load("plate.cam");
    assert_eq!(p.stock_line, Some((0, 8, "stock Box(w=120, l=80, h=20, x=0, y=0, z=0)".to_string())), "main.part line 9, the text as typed");
    let form = view::statement_form(&p, &view::Statement::Stock).unwrap();
    assert_eq!(form.head, "Box");
    assert_eq!(form.fields.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(), parse::STOCK_KEYS.to_vec(), "the parser's whitelist, in its order");
    assert_eq!(form.fields.iter().filter(|f| f.required).map(|f| f.key.as_str()).collect::<Vec<_>>(), vec!["w", "l", "h"]);
    let h = form.fields.iter().find(|f| f.key == "h").unwrap();
    assert_eq!((h.value.as_deref(), h.required, h.options.is_none()), (Some("20"), true, true), "a number, not a menu");
    let material = form.fields.iter().find(|f| f.key == "material").unwrap();
    assert_eq!((material.value.as_deref(), material.required), (None, false), "absent on the line: a field the user can fill");
    assert_eq!(view::field_write(&form, "h", "").unwrap_err(), "a Box needs h");
    assert_eq!(view::field_write(&form, "material", "6061").unwrap(), Some("6061".to_string()));
    assert!(view::field_write(&form, "z", "").unwrap().is_none(), "a present optional emptied removes; the parser's default is 0");

    // A stock line in geometry.part is recorded against file 1, and it is
    // the line `stock` holds (the last parsed wins, as the parser reads).
    let main = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\ntool T1 Endmill(dia=10)\nrun End(tool=T1, surface=Top, stepover=5)\n";
    let geom = "# the blank\nstock Box(w=50, l=40, h=10)\nfeature Top Plane(z=0)\n";
    let p = parse::parse_project(main, Some(geom)).unwrap();
    assert_eq!(p.stock_line, Some((1, 1, "stock Box(w=50, l=40, h=10)".to_string())));
    assert_eq!((p.stock.w, p.stock.h), (50.0, 10.0));
    // No stock line at all: the default stock, and nothing to edit.
    let err = view::statement_form(&parse::Part::default(), &view::Statement::Stock).unwrap_err();
    assert!(err.contains("no line on disk"), "{err}");
}

/// The acceptance, end to end as the shell does it: a boss that stands
/// above the stock is a containment finding; its `z` and `h` written on
/// the stock line through `set_key` + `replace_line` — the text the user
/// could have typed, every other byte kept — re-parse to a project where
/// the boss is contained. And the parser's refusal reaches the form: a
/// side emptied is refused by the form, a side set to nothing a number by
/// the parser's own sentence.
#[test]
fn test_raising_the_stock_from_the_form_contains_the_boss_that_stood_above_it() {
    let text = "units mm\nfeed 800\nplunge 300\nrapid_z 40\nclearance_z 60\nspindle 3000\n\
# the blank, as delivered\nstock Box(w=100, l=80, h=20, x=0, y=0, z=0)\n\
tool T1 Ballmill(dia=10)\nfeature Post CylinderBoss(dia=20, extent=10, x=50, y=40, z=5)\n\
run Side(tool=T1, surface=Post, stepover=2)\n";
    let p = parse::parse_project(text, None).unwrap();
    let before = nacs::stock::containment_findings(&p);
    assert!(!before.is_empty(), "the boss's top at z 5 stands above the stock top z 0");
    let (_, line, stock_text) = p.stock_line.clone().unwrap();
    assert_eq!(line, 7);
    let form = view::statement_form(&p, &view::Statement::Stock).unwrap();
    // Two writes, as two Enters: the top datum, then the height.
    // The boss's `z` is its BASE (`stock.rs`'s boss arm) and it stands
    // `extent` above it, so the stock top must reach z 15.
    let z = view::field_write(&form, "z", "15").unwrap();
    let raised = view::replace_line(text, line, &view::set_key(&stock_text, "z", z.as_deref()).unwrap()).unwrap();
    let p2 = parse::parse_project(&raised, None).unwrap();
    let stock_text2 = p2.stock_line.clone().unwrap().2;
    let h = view::field_write(&view::statement_form(&p2, &view::Statement::Stock).unwrap(), "h", "35").unwrap();
    let raised = view::replace_line(&raised, line, &view::set_key(&stock_text2, "h", h.as_deref()).unwrap()).unwrap();
    assert_eq!(raised.lines().nth(7).unwrap(), "stock Box(w=100, l=80, h=35, x=0, y=0, z=15)", "the text the user could have typed");
    assert_eq!(raised.lines().nth(6).unwrap(), "# the blank, as delivered", "the comment above it stays");
    let p3 = parse::parse_project(&raised, None).unwrap();
    assert_eq!((p3.stock.h, p3.stock.z), (35.0, 15.0));
    assert!(nacs::stock::containment_findings(&p3).is_empty(), "contained: {:?}", nacs::stock::containment_findings(&p3));
    // The parser's own sentence for a value the form cannot judge.
    let bad = view::replace_line(&raised, line, &view::set_key(&p3.stock_line.clone().unwrap().2, "h", Some("tall")).unwrap()).unwrap();
    let err = parse::parse_project(&bad, None).unwrap_err();
    assert_eq!(err, "line 8: 'tall' is not a number", "the parser's sentence, the file untouched");
}

// ---- Phase 22 slice 2: delete, the tenth write --------------------------
//
// A statement removed from its file and nothing else (DESIGN.md §1): its
// line, or its block from the opener through the closing `)`; the
// comment above it stays; the whole project re-parsed with the lines gone
// decides what may go — with the parser's own sentence, never a second
// judge's.

/// The extent: a run, a tool, a stock and a single-line feature are one
/// line each; a block feature runs from its opener to the line that
/// closes it, by the parser's own rule (the first line whose code carries
/// a `)`); a comment or an empty line is not a statement; an unclosed
/// block is refused by name.
#[test]
fn test_statement_extent_is_one_line_or_the_block_the_parser_reads() {
    let main = std::fs::read_to_string("plate.cam/main.part").unwrap();
    let geom = std::fs::read_to_string("plate.cam/geometry.part").unwrap();
    let p = parse::load_project("plate.cam").unwrap();
    let (_, run) = p.op_lines[0];
    assert_eq!(view::statement_extent(&main, run), Ok((run, run)));
    let (_, _, tool, _) = p.tool_lines[0];
    assert_eq!(view::statement_extent(&main, tool), Ok((tool, tool)));
    assert_eq!(view::statement_extent(&main, p.stock_line.as_ref().unwrap().1), Ok((8, 8)));
    // geometry.part's ProfilePocket (the Notch since Phase 53 slice 1, on
    // the original L's corner since the erratum): the opener on line 7
    // after six comment lines, four points, the `)` on line 12.
    let (name, file, first, _) = p.feature_lines.iter().find(|(n, ..)| n == "Notch").cloned().unwrap();
    assert_eq!((name.as_str(), file, first), ("Notch", 1, 6));
    assert_eq!(view::statement_extent(&geom, first), Ok((6, 11)));
    assert_eq!(geom.lines().nth(11).unwrap().trim(), ")");
    // A comment, an empty line, past the end, an unclosed block.
    assert!(view::statement_extent(&main, 0).unwrap_err().contains("not a statement"));
    assert!(view::statement_extent(&geom, 0).unwrap_err().contains("not a statement"));
    assert!(view::statement_extent(&main, 10_000).unwrap_err().contains("past the end"));
    let unclosed = "feature P ProfilePocket(extent=1\n  0 0\n  1 0\n  1 1\n";
    assert!(view::statement_extent(unclosed, 0).unwrap_err().contains("no line closes"));
    // A block whose closing `)` shares a line with its last point — the
    // parser takes both; so does the extent.
    let tight = "# above\nfeature P ProfilePocket(extent=1\n  0 0\n  1 0\n  1 1)\nrun End(tool=T1, surface=P)\n";
    assert_eq!(view::statement_extent(tight, 1), Ok((1, 4)));
}

/// The bytes: the range leaves with its newlines and nothing else moves —
/// the comment above stays, the file's final newline is kept as it was,
/// and a last line without one goes whole.
#[test]
fn test_delete_lines_removes_exactly_the_range_and_keeps_every_other_byte() {
    let text = "a\n# above\nb\nc\nd";
    assert_eq!(view::delete_lines(text, 2, 2).unwrap(), "a\n# above\nc\nd");
    assert_eq!(view::delete_lines(text, 2, 3).unwrap(), "a\n# above\nd");
    assert_eq!(view::delete_lines(text, 4, 4).unwrap(), "a\n# above\nb\nc\n", "the last line, without its newline, goes whole");
    assert_eq!(view::delete_lines(text, 0, 0).unwrap(), "# above\nb\nc\nd");
    assert_eq!(view::delete_lines("a\nb\n", 1, 1).unwrap(), "a\n", "a final newline is kept as it was");
    assert!(view::delete_lines(text, 3, 2).unwrap_err().contains("inverted"));
    assert!(view::delete_lines(text, 0, 5).unwrap_err().contains("past the end"));
    // Round trip with the extent on a real file: the block gone, the
    // lines around it byte for byte.
    let geom = std::fs::read_to_string("plate.cam/geometry.part").unwrap();
    let (first, last) = view::statement_extent(&geom, 6).unwrap();
    let after = view::delete_lines(&geom, first, last).unwrap();
    assert_eq!(after, geom.lines().take(6).collect::<Vec<_>>().join("\n") + "\n", "the six comment lines stay");
}

/// End to end, as the shell does it: the lines gone, the whole project
/// re-parsed. An operation deleted is gone from the program and its
/// neighbours keep their lines; a feature something runs is the parser's
/// refusal ("referenced feature 'X' not defined"); a tool something runs
/// the same; a feature nothing runs goes, block and all; a tool nothing
/// runs goes.
#[test]
fn test_deleting_a_statement_reparses_and_the_parser_judges_what_may_go() {
    let dir = "contour_boss.cam";
    let main = std::fs::read_to_string(format!("{dir}/main.part")).unwrap();
    let p = parse::load_project(dir).unwrap();
    let base = Some(std::path::Path::new(dir));
    // The operation: run line 2 of 3 gone, the other two keep their text.
    let (_, line) = p.op_lines[1];
    let (a, b) = view::statement_extent(&main, line).unwrap();
    let after = view::delete_lines(&main, a, b).unwrap();
    let q = parse::parse_project_with_base(&after, None, base).unwrap();
    assert_eq!(q.operations.len(), 2);
    assert_eq!(q.op_texts, vec![p.op_texts[0].clone(), p.op_texts[2].clone()]);
    // The feature something runs: the parser's own sentence.
    let (_, _, first, _) = p.feature_lines[0].clone();
    let (a, b) = view::statement_extent(&main, first).unwrap();
    assert_eq!((a, b), (29, 34), "the ContourBoss block, opener through `)`");
    let after = view::delete_lines(&main, a, b).unwrap();
    let err = parse::parse_project_with_base(&after, None, base).unwrap_err();
    assert!(err.contains("referenced feature 'Crest' not defined"), "{err}");
    // The tool something runs: the same.
    let (_, _, tline, _) = p.tool_lines[0].clone();
    let after = view::delete_lines(&main, tline, tline).unwrap();
    let err = parse::parse_project_with_base(&after, None, base).unwrap_err();
    assert!(err.contains("referenced tool T5 not defined"), "{err}");
    // Nothing runs it: the block goes, the comment above it stays, the
    // project parses without the feature; the tool the same.
    let mut text = main.clone();
    for i in (0..p.operations.len()).rev() {
        let (_, l) = p.op_lines[i];
        text = view::delete_lines(&text, l, l).unwrap();
    }
    let no_runs = parse::parse_project_with_base(&text, None, base).unwrap();
    assert_eq!(no_runs.operations.len(), 0);
    let (_, _, first, _) = no_runs.feature_lines[0].clone();
    let (a, b) = view::statement_extent(&text, first).unwrap();
    let after = view::delete_lines(&text, a, b).unwrap();
    let r = parse::parse_project_with_base(&after, None, base).unwrap();
    assert!(r.features.is_empty());
    assert!(after.lines().nth(a - 1).unwrap().is_empty(), "the empty line above the block stays");
    assert!(after.contains("tool T5 Endmill(dia=10.0)"));
    let (_, _, tline, _) = r.tool_lines[0].clone();
    let after = view::delete_lines(&after, tline, tline).unwrap();
    let r = parse::parse_project_with_base(&after, None, base).unwrap();
    assert!(r.tools.is_empty());
}

// ---- Phase 23 slice 1: the keys (DESIGN.md §9's ban lifted for a named list) --

/// The named list, and nothing more: `f` and `Home` fit, `n` is a new
/// operation on the selected feature, `Ctrl+Z` / `Ctrl+Y` are undo's and
/// redo's second names. Every second binding names an act the inventory
/// knows; a bare second key collides with no act's own key; a chord is
/// spelled `Ctrl+<key>` and parses; the help and the palette print both
/// names in one column.
#[test]
fn test_the_named_keys_are_bound_and_the_second_names_collide_with_nothing() {
    assert_eq!(view::action("view.fit").unwrap().key, "f");
    assert_eq!(view::action("op.new").unwrap().key, "n");
    assert_eq!(view::also("view.fit"), Some("Home"));
    assert_eq!(view::also("edit.undo"), Some("Ctrl+Z"));
    assert_eq!(view::also("edit.redo"), Some("Ctrl+Y"));
    assert_eq!(view::also("op.suppress"), None, "a write that deletes an operation from the program keeps no key — nor a second one");
    assert_eq!(view::ALSO.len(), 3, "the list is named; a fourth entry is a DESIGN.md §9 amendment first");
    let keys: Vec<&str> = view::ACTIONS.iter().map(|a| a.key).filter(|k| !k.is_empty()).collect();
    for (id, binding) in view::ALSO {
        assert!(view::action(id).is_some(), "{id}: the second name belongs to no act");
        match view::chord_key(binding) {
            Some(k) => assert!(k.len() == 1 && k.chars().all(|c| c.is_ascii_uppercase()), "{binding}: a chord is Ctrl+<one letter>"),
            None => assert!(!keys.contains(binding), "{binding}: a bare second key collides with an act's own key"),
        }
    }
    assert_eq!(view::chord_key("Ctrl+Z"), Some("Z"));
    assert_eq!(view::chord_key("Home"), None);
    assert_eq!(view::key_words(view::action("view.fit").unwrap()), "f Home");
    assert_eq!(view::key_words(view::action("edit.undo").unwrap()), "z Ctrl+Z");
    assert_eq!(view::key_words(view::action("op.suppress").unwrap()), "·");
    let help = view::help_lines().join("\n");
    assert!(help.contains("  f Home         fit the scene"), "{help}");
    assert!(help.contains("  z Ctrl+Z       undo"), "{help}");
    assert!(help.contains("Ctrl+Z and Ctrl+Y are undo's and redo's own second names"), "the chord sentence names its two exceptions: {help}");
}

// ---- Phase 23 slice 2: the picture (DESIGN.md §5.5, §9's camera arithmetic admitted) --

/// Zoom about a screen point: the world point under the cursor stays
/// under it, at every stance; zoom about the centre is `zoom_by`; a
/// clamped zoom (nothing changed) moves nothing; a non-finite point is
/// a plain zoom.
#[test]
fn test_zoom_at_keeps_the_point_under_the_cursor_where_it_is() {
    for (yaw, pitch) in [(0.0, 0.0), (0.7, 0.4), (-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2), (2.1, -0.3)] {
        let mut cam = view::Camera { yaw, pitch, scale: 2.5, target: [10.0, -5.0, 3.0] };
        let (sx, sy) = (120.0, -45.0);
        let under = cam.unproject_screen(sx, sy);
        cam.zoom_at(sx, sy, 1.8);
        let after = cam.unproject_screen(sx, sy);
        for k in 0..3 {
            assert!((under[k] - after[k]).abs() < 1e-9, "stance ({yaw}, {pitch}) axis {k}: {under:?} vs {after:?}");
        }
        assert!((cam.scale - 4.5).abs() < 1e-12);
        // And the projection agrees: the world point lands back on the cursor.
        let (px, py, _) = cam.project(under);
        assert!((f64::from(px) - sx).abs() < 1e-3 && (f64::from(py) - sy).abs() < 1e-3, "{px} {py}");
    }
    let mut centre = view::Camera { yaw: 0.7, pitch: 0.4, scale: 2.0, target: [1.0, 2.0, 3.0] };
    centre.zoom_at(0.0, 0.0, 0.5);
    assert_eq!(centre.target, [1.0, 2.0, 3.0], "about the centre, the target stays");
    assert_eq!(centre.scale, 1.0);
    let mut pinned = view::Camera { yaw: 0.0, pitch: 0.0, scale: view::SCALE_MAX, target: [0.0; 3] };
    pinned.zoom_at(100.0, 100.0, 2.0);
    assert_eq!(pinned.target, [0.0; 3], "clamped: nothing changed, nothing slides");
    let mut nan = view::Camera { yaw: 0.0, pitch: 0.0, scale: 1.0, target: [0.0; 3] };
    nan.zoom_at(f64::NAN, 0.0, 2.0);
    assert_eq!((nan.scale, nan.target), (2.0, [0.0; 3]), "a point that is not one: a plain zoom");
}

/// The standard views keep the target and the scale and put the camera
/// at the axis stances the projection pins hold: plan is `Camera::plan`'s
/// stance (+X right, +Y up); front puts +X right and +Z up; right puts +Y
/// right and +Z up.
#[test]
fn test_the_standard_views_are_the_axis_stances_with_the_target_kept() {
    let mut cam = view::Camera { yaw: 0.7, pitch: 0.4, scale: 3.0, target: [10.0, 20.0, 30.0] };
    let at = |cam: &view::Camera, v: [f64; 3]| {
        let (x, y, _) = cam.project([cam.target[0] + v[0], cam.target[1] + v[1], cam.target[2] + v[2]]);
        (x.round() as i32, y.round() as i32)
    };
    cam.set_stance(view::Stance::Plan);
    assert_eq!((cam.yaw, cam.pitch), (view::Camera::plan().yaw, view::Camera::plan().pitch));
    assert_eq!((cam.scale, cam.target), (3.0, [10.0, 20.0, 30.0]));
    assert_eq!(at(&cam, [1.0, 0.0, 0.0]), (3, 0), "+X right");
    assert_eq!(at(&cam, [0.0, 1.0, 0.0]), (0, 3), "+Y up");
    cam.set_stance(view::Stance::Front);
    assert_eq!(at(&cam, [1.0, 0.0, 0.0]), (3, 0), "+X right");
    assert_eq!(at(&cam, [0.0, 0.0, 1.0]), (0, 3), "+Z up");
    assert_eq!(at(&cam, [0.0, 1.0, 0.0]), (0, 0), "+Y is the view direction");
    cam.set_stance(view::Stance::Right);
    assert_eq!(at(&cam, [0.0, 1.0, 0.0]), (3, 0), "+Y right");
    assert_eq!(at(&cam, [0.0, 0.0, 1.0]), (0, 3), "+Z up");
    assert_eq!(at(&cam, [1.0, 0.0, 0.0]), (0, 0), "+X is the view direction");
    assert_eq!((cam.scale, cam.target), (3.0, [10.0, 20.0, 30.0]), "the target and the scale never move");
    for id in ["view.plan", "view.front", "view.right"] {
        assert_eq!(view::action(id).unwrap().key, "", "{id} is keyless");
    }
}

/// The triad is the projection of the three unit axes, scale-free and
/// target-free: on the plan stance X points right and Y up with Z a dot;
/// on the front stance Z points up; every arrow is at most unit length.
#[test]
fn test_the_triad_is_the_projected_unit_axes() {
    let plan = view::Camera { yaw: view::Camera::plan().yaw, pitch: view::Camera::plan().pitch, scale: 40.0, target: [50.0, 60.0, 70.0] };
    let t = plan.triad();
    assert_eq!(t[0].2, "X");
    assert!((t[0].0 - 1.0).abs() < 1e-6 && t[0].1.abs() < 1e-6, "X right: {:?}", t[0]);
    assert!(t[1].0.abs() < 1e-6 && (t[1].1 - 1.0).abs() < 1e-6, "Y up: {:?}", t[1]);
    assert!(t[2].0.abs() < 1e-6 && t[2].1.abs() < 1e-6, "Z is a dot in plan: {:?}", t[2]);
    let mut front = plan;
    front.set_stance(view::Stance::Front);
    let t = front.triad();
    assert!((t[2].1 - 1.0).abs() < 1e-6, "Z up from the front: {:?}", t[2]);
    let tilted = view::Camera { yaw: 0.7, pitch: 0.4, scale: 9.0, target: [1.0; 3] };
    for (x, y, _) in tilted.triad() {
        assert!((x * x + y * y).sqrt() <= 1.0 + 1e-6);
    }
}

/// Framing an operation: its path's box centre, scale untouched; no path,
/// no point (the camera stays).
#[test]
fn test_frame_point_is_the_paths_centre_or_nothing() {
    let p = load("plate.cam");
    let v = check_result::check_result(&p);
    let paths = view::op_paths(&p, v.emitted_moves.as_deref().unwrap());
    let c = view::frame_point(&paths, 0).expect("op 0 emitted");
    let path = paths.iter().find(|pa| pa.op_idx == 0).unwrap();
    let all: Vec<[f64; 3]> = path.polylines.iter().flat_map(|pl| pl.pts.iter().copied()).collect();
    for k in 0..3 {
        let (lo, hi) = all.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), pt| (lo.min(pt[k]), hi.max(pt[k])));
        assert!((c[k] - (lo + hi) / 2.0).abs() < 1e-9);
    }
    assert_eq!(view::frame_point(&paths, 99), None);
    assert_eq!(view::frame_point(&[], 0), None);
}

/// The legend names the three roles beside the one colour home's
/// colours, in the derived order; the empty viewport says where the first
/// feature or operation comes from and is silent otherwise.
#[test]
fn test_the_legend_and_the_empty_viewport_note() {
    let theme = Theme::default();
    let legend = view::legend(&theme);
    assert_eq!(legend.iter().map(|(_, w)| *w).collect::<Vec<_>>(), vec!["roughing", "finishing", "untagged"]);
    assert_eq!(legend[0].0, theme.role_color(nacs::parse::OpRole::Roughing));
    assert_eq!(legend[1].0, theme.role_color(nacs::parse::OpRole::Finishing));
    assert!(view::empty_viewport_note(0, 0).unwrap().contains("New feature…"));
    assert!(view::empty_viewport_note(0, 2).unwrap().contains("New operation…"));
    assert_eq!(view::empty_viewport_note(3, 2), None);
    let help = view::help_lines().join("\n");
    assert!(help.contains("zoom about the cursor") && help.contains("axis triad") && help.contains("the legend"), "{help}");
}

// ---- Phase 23 slice 3: the Program's gutters, the rail's count (DESIGN.md §5.2, §5.3) --

/// The gutter and the strip: the number column's width, the visible span
/// at the pitch `show_rows` lays rows at, one slice per line down the
/// strip, and a click that centres the line under it.
#[test]
fn test_the_program_gutters_arithmetic() {
    assert_eq!(view::line_number_width(0), 2);
    assert_eq!(view::line_number_width(9), 2);
    assert_eq!(view::line_number_width(100), 3);
    assert_eq!(view::line_number_width(4321), 4);
    // 20px rows, 4px spacing → pitch 24; 240px visible → 10 rows (+1 partial).
    assert_eq!(view::program_visible_span(0.0, 20.0, 4.0, 240.0, 1000), 0..11);
    assert_eq!(view::program_visible_span(240.0, 20.0, 4.0, 240.0, 1000), 10..21);
    assert_eq!(view::program_visible_span(240.0, 20.0, 4.0, 240.0, 15), 10..15, "clamped to the document");
    assert_eq!(view::program_visible_span(99_999.0, 20.0, 4.0, 240.0, 15), 15..15, "past the end is empty, never a panic");
    assert_eq!(view::program_visible_span(-5.0, 20.0, 4.0, 240.0, 15), 0..11);
    // A thousand lines down a 200px strip: 0.2px each, contiguous.
    let (t0, b0) = view::minimap_row(0, 1000, 200.0);
    let (t1, b1) = view::minimap_row(1, 1000, 200.0);
    assert_eq!((t0, b0), (0.0, 0.2));
    assert_eq!((t1, b1), (0.2, 0.4));
    let (t, b) = view::minimap_row(999, 1000, 200.0);
    assert!((b - 200.0).abs() < 1e-4 && t < b);
    assert_eq!(view::minimap_row(3, 0, 200.0), (0.0, 0.0), "no lines, no rows");
    // A click at the strip's middle centres the middle line — the same
    // offset an Outline click would ask for that line.
    let n = 1000;
    let mid = view::minimap_scroll_to(100.0, 200.0, n, 20.0, 4.0, 240.0);
    assert_eq!(mid, view::program_scroll_offset(20.0, 4.0, 500, 240.0));
    assert_eq!(view::minimap_scroll_to(-50.0, 200.0, n, 20.0, 4.0, 240.0), 0.0, "above the strip: the top");
    assert_eq!(view::minimap_scroll_to(999.0, 200.0, n, 20.0, 4.0, 240.0), view::program_scroll_offset(20.0, 4.0, 999, 240.0), "below: the last line");
    assert_eq!(view::minimap_scroll_to(10.0, 200.0, 0, 20.0, 4.0, 240.0), 0.0);
}

/// The two toggles live in the workspace file like a layer: on by
/// default and written only when off, round-tripping, mutated only
/// through `DockAction`, and reverted with the rest.
#[test]
fn test_the_program_gutters_are_workspace_toggles() {
    let mut l = Layout::default();
    assert!(l.program_numbers && l.program_minimap);
    assert!(!l.render().contains("program."), "the defaults write nothing");
    l.apply(theme::DockAction::ProgramMinimap(false));
    assert!(l.render().contains("\nprogram.minimap false\n") && !l.render().contains("program.numbers"));
    assert_eq!(Layout::parse(&l.render()), l, "parse(render(x)) == x");
    l.apply(theme::DockAction::ProgramNumbers(false));
    assert_eq!(Layout::parse(&l.render()), l);
    assert_eq!(Layout::parse("program.numbers nope\n").program_numbers, true, "a nonsense value is the default");
    l.apply(theme::DockAction::Revert);
    assert!(l.program_numbers && l.program_minimap);
    for id in ["program.numbers", "program.minimap"] {
        assert_eq!(view::action(id).unwrap().key, "", "{id} is keyless");
    }
    let help = view::help_lines().join("\n");
    assert!(help.contains("line numbers on the left and a minimap on the right"), "{help}");
}

/// The rail says the count where the user looks, and a reload that turns
/// a project red says so on the status line — and only that transition.
#[test]
fn test_the_rail_counts_problems_and_a_reload_that_turns_red_says_so() {
    let rail = view::tabs();
    let problems = rail.iter().find(|s| s.tab == SidebarTab::Problems).unwrap();
    let outline = rail.iter().find(|s| s.tab == SidebarTab::Outline).unwrap();
    assert_eq!(view::tab_label(problems, 3, 0, None), "Problems (3)");
    assert_eq!(view::tab_label(problems, 0, 0, None), "Problems", "no count when there is nothing to count");
    assert_eq!(view::tab_label(outline, 3, 0, None), "Outline", "only the Problems slot counts");
    assert!(view::turned_red_note(false, true).unwrap().contains("Problems"));
    assert_eq!(view::turned_red_note(true, true), None);
    assert_eq!(view::turned_red_note(false, false), None);
    assert_eq!(view::turned_red_note(true, false), None, "turning green is the picture's own news");
}

// ---- the glass: the surface spike's rules (Phase 25 slice 1) --------------

fn offer(backend: &str, name: &str, kind: view::DeviceKind, modes: &[&str]) -> view::SurfaceOffer {
    view::SurfaceOffer { backend: backend.into(), name: name.into(), kind, alpha_modes: modes.iter().map(|m| m.to_string()).collect() }
}

/// The adapter the spike picks: wanting transparency, the first surface
/// that offers it, a discrete GPU first among those; not wanting it, or
/// when none offers it, the default's own preference — a discrete GPU,
/// else the first — so a machine whose surfaces are all opaque sees the
/// window it always saw. The shipped DX12 table (`[Opaque]` on a plain
/// HWND) is the second fixture here, by name.
#[test]
fn test_the_spike_prefers_a_surface_that_can_be_seen_through() {
    use view::DeviceKind::*;
    let dx12 = offer("Dx12", "NVIDIA GeForce RTX 5070 Ti", Discrete, &["Opaque"]);
    let vk = offer("Vulkan", "NVIDIA GeForce RTX 5070 Ti", Discrete, &["Opaque", "PreMultiplied"]);
    let gl = offer("Gl", "NVIDIA GeForce RTX 5070 Ti/PCIe/SSE2", Other, &["Opaque"]);
    let igpu_vk = offer("Vulkan", "Intel(R) UHD Graphics", Integrated, &["Opaque", "PostMultiplied"]);
    assert!(vk.transparent() && igpu_vk.transparent() && !dx12.transparent() && !gl.transparent());
    // Wanted and on offer: the see-through surface, the best device first.
    assert_eq!(view::pick_adapter(&[dx12.clone(), gl.clone(), igpu_vk.clone(), vk.clone()], true), Some(3));
    assert_eq!(view::pick_adapter(&[dx12.clone(), igpu_vk.clone()], true), Some(1), "a see-through iGPU beats an opaque dGPU when the window asked for glass");
    // Wanted and NOT on offer — the shipped stack's own case: the default
    // preference, the best-ranked device.
    assert_eq!(view::pick_adapter(&[gl.clone(), dx12.clone()], true), Some(1));
    // Not wanted: the default preference regardless of glass.
    assert_eq!(view::pick_adapter(&[igpu_vk.clone(), dx12.clone(), vk.clone()], false), Some(1));
    assert_eq!(view::pick_adapter(&[gl.clone()], false), Some(0));
    assert_eq!(view::pick_adapter(&[], true), None);
    // The WSL case that corrected the first draft of this rule: a CPU
    // Vulkan adapter (llvmpipe) offers PreMultiplied, the hardware is a GL
    // adapter of kind Other, and glass was NOT asked for — the hardware
    // wins, as the default would have chosen. Asked for, the CPU's glass
    // is the only glass and is taken, by the rule.
    let llvmpipe = offer("Vulkan", "llvmpipe (LLVM 15.0.6, 256 bits)", Cpu, &["Opaque", "PreMultiplied"]);
    let d3d12_gl = offer("Gl", "D3D12 (NVIDIA GeForce RTX 5070 Ti)", Other, &["Opaque"]);
    assert_eq!(view::pick_adapter(&[llvmpipe.clone(), d3d12_gl.clone()], false), Some(1));
    assert_eq!(view::pick_adapter(&[llvmpipe.clone(), d3d12_gl.clone()], true), Some(0));
    // Ties keep the first, as the default's enumeration order does.
    assert_eq!(view::pick_adapter(&[vk.clone(), dx12.clone()], false), Some(0));
}

/// The one sentence: every adapter, what its surface offered, which was
/// chosen, and the verdict in words — the measurement the DRAFT said the
/// shipped stack cannot state, stated from the user's launch.
#[test]
fn test_the_spike_says_what_every_surface_offered() {
    use view::DeviceKind::*;
    let dx12 = offer("Dx12", "RTX", Discrete, &["Opaque"]);
    let vk = offer("Vulkan", "RTX", Discrete, &["Opaque", "PreMultiplied"]);
    let s = view::surface_sentence(&[dx12.clone(), vk.clone()], Some(1), true);
    assert_eq!(s, "Transparency: RTX (Dx12) offers [Opaque]; RTX (Vulkan) offers [Opaque, PreMultiplied] — chosen; the window can be see-through");
    let s = view::surface_sentence(&[dx12.clone()], Some(0), true);
    assert!(s.ends_with("no surface offers transparency: the window is opaque, glass worn over black"), "{s}");
    let s = view::surface_sentence(&[dx12.clone()], Some(0), false);
    assert!(s.ends_with("transparency not asked for on this platform"), "{s}");
    // Not asked for, the verdict says so even when a surface could — the
    // WSL launch's own case.
    let s = view::surface_sentence(&[vk.clone()], Some(0), false);
    assert!(s.ends_with("transparency not asked for on this platform"), "{s}");
    let stranger = offer("Gl", "llvmpipe", Cpu, &[]);
    assert!(view::surface_sentence(&[stranger], Some(0), false).contains("llvmpipe (Gl) offers nothing"));
    assert_eq!(view::surface_sentence(&[], None, true), "Transparency: no adapter offered a surface");
}


// ---- the glass: the format and the law (Phase 25 slice 2) -----------------

/// Fork 2: a FILL keeps the alpha its file wrote, a stroke and a text
/// never do, `#rrggbb` is opaque, and Zed's `background.appearance` word
/// is read (absent or unknown = opaque). The snippet is the shape of every
/// Catppuccin blur variant: one `background` carrying the glass, every
/// pane `#00000000` over it.
#[cfg(feature = "frontend")]
#[test]
fn test_a_fill_keeps_its_alpha_and_a_stroke_drops_it() {
    let text = r##"{"themes":[{"name":"g","appearance":"dark","style":{
        "background.appearance":"blurred",
        "background":"#00000099","panel.background":"#00000000","editor.background":"#00000000",
        "element.background":"#181926","element.selected":"#363a4f4d","text":"#cad3f5ff","border":"#363a4f15"},
        "nacs":{"ground":"#000000d7","map.ground":"#0a0a0ad7","wire":"#62667080","indicator":"#ebeef5be"}}]}"##;
    let t = theme::theme_from_json(text).expect("parses");
    assert_eq!(t.background_appearance, theme::BackgroundAppearance::Blurred);
    assert_eq!(t.fill_alpha.background, 0x99);
    assert_eq!(t.fill_alpha.panel_background, 0, "a fully transparent pane is a legal fill");
    assert_eq!(t.fill_alpha.text_field, 0);
    assert_eq!(t.fill_alpha.element, 255, "#rrggbb is opaque");
    assert_eq!(t.fill_alpha.element_selected, 0x4d);
    assert_eq!(t.fill_alpha.ground, 0xd7);
    assert_eq!(t.fill_alpha.map_ground, 0xd7);
    assert_eq!(t.text, [0xca, 0xd3, 0xf5], "text keeps its colour");
    assert_eq!(t.wire, [0x62, 0x66, 0x70], "a stroke keeps its colour and has no alpha to keep");
    assert_eq!(t.border, [0x36, 0x3a, 0x4f]);
    assert_eq!(t.indicator, ([0xeb, 0xee, 0xf5], 0xbe), "the indicator's own alpha, as ever");
    assert!(t.translucent());
    // Absent word, opaque fills: the theme as it always read.
    let plain = theme::theme_from_json(r##"{"style":{"background":"#1b1b1b"}}"##).expect("parses");
    assert_eq!(plain.background_appearance, theme::BackgroundAppearance::Opaque);
    assert!(!plain.translucent());
    let odd = theme::theme_from_json(r##"{"style":{"background.appearance":"frosted"}}"##).expect("parses");
    assert_eq!(odd.background_appearance, theme::BackgroundAppearance::Opaque, "an unknown word is the unknown-line rule");
}

/// Fork 3: a translucent fill is judged at both desks, the floor holding
/// at both; the finding names the desk it failed on, or says the glass
/// sweeps across the mark's own brightness. The built-in's ground at the
/// user's Light alpha is the worked case: over a white desk it is
/// mid-grey, and the quiet strokes drown.
#[test]
fn test_a_translucent_fill_is_judged_at_both_desks() {
    // Opaque: no meaning finding, and no finding speaks of a desk.
    let t = Theme::default();
    let findings = theme::validate(&t);
    assert!(findings.iter().all(|f| f.grade != theme::ThemeGrade::Meaning));
    assert!(findings.iter().all(|f| !f.message.contains("desk")), "an opaque theme's findings are the old law's, byte for byte");

    // The desk arithmetic itself.
    assert_eq!(theme::over([0, 0, 0], 0x99, [255, 255, 255]), [102, 102, 102]);
    assert_eq!(theme::over([0, 0, 0], 255, [255, 255, 255]), [0, 0, 0]);
    assert_eq!(theme::stack_on([255, 255, 255], &[([0, 0, 0], 0x99), ([0, 0, 0], 0)]), [102, 102, 102], "a #00000000 pane changes nothing");
    let (ratio, _, desk) = theme::worst_contrast([255, 255, 255], &[([0, 0, 0], 255)]);
    assert!(desk.is_none() && (ratio - 21.0).abs() < 0.01, "an opaque stack is the plain contrast");

    // A translucent ground over an OPAQUE panel shows the panel, not the
    // desk — the stack is the shell's paint order — so nothing speaks of a
    // desk and the wire reads as it always did.
    let mut g = Theme::default();
    g.set_slot("ground", g.ground, Some(0x99));
    assert!(g.translucent());
    assert!(theme::validate(&g).iter().all(|f| !f.message.contains("desk")), "an opaque panel under the glass is still opaque");
    // The real shape of glass — Zed's blur files: the sheet `background`
    // at the user's Light alpha, the pane fully transparent, the ground at
    // the same alpha. Over a white desk the built-in's ground is mid-grey
    // and the quiet wire drowns; the finding names the desk.
    g.set_slot("background", [0, 0, 0], Some(0x99));
    g.set_slot("panel.background", [0, 0, 0], Some(0));
    let findings = theme::validate(&g);
    let wire = findings.iter().find(|f| f.message.starts_with("`wire`")).expect("the wire drowns on a white desk");
    assert_eq!(wire.grade, theme::ThemeGrade::Meaning);
    assert!(wire.message.contains("on a white desk"), "{}", wire.message);
    // A mark whose brightness lies inside what the glass sweeps meets it
    // at 1:1 on some desk, and the sentence says so.
    let (ratio, _, desk) = theme::worst_contrast([30, 30, 32], &[([0, 0, 0], 0x99), ([0, 0, 0], 0), ([24, 26, 30], 0x99)]);
    assert!((ratio - 1.0).abs() < 1e-9);
    assert_eq!(desk, Some("on some desk between black and white, where the glass meets its own brightness"));
    // A bright mark clears the black desk but not the white one: the
    // finding names the white desk and the ground as seen there.
    let (ratio, seen, desk) = theme::worst_contrast([160, 160, 160], &[([0, 0, 0], 0x99)]);
    assert_eq!(desk, Some("on a white desk"));
    assert_eq!(seen, [102, 102, 102]);
    assert!(ratio < 3.0 && ratio > 2.0, "{ratio}");
    // And a mark that clears both desks is legal glass.
    let (ratio, _, desk) = theme::worst_contrast([255, 255, 255], &[([0, 0, 0], 0x99)]);
    assert_eq!(desk, Some("on a white desk"));
    assert!(ratio > 4.5, "{ratio}");

    // The family's default glass (0xd7) on the same sheet: the roles clear
    // both desks; the quiet greys need the eye's re-tune, which is the law
    // speaking, not a bug.
    let mut d = g.clone();
    d.set_slot("ground", d.ground, Some(0xd7));
    let findings = theme::validate(&d);
    assert!(findings.iter().all(|f| !f.message.starts_with("`role.")), "{findings:?}");
}

/// The map's sheet-on-the-desk rule holds at both desks: a sheet that
/// lies on the ground's side of the wire on black can cross it on white.
#[test]
fn test_the_map_ground_rule_holds_at_both_desks() {
    let mut t = Theme::default();
    // The glass shape: sheet and pane see-through, ground translucent,
    // the map's sheet opaque and a little lighter than the ground: on a
    // black desk the sheet lies above the ground and below the wire; on a
    // white desk the ground brightens past the sheet.
    t.set_slot("background", [0, 0, 0], Some(0x99));
    t.set_slot("panel.background", [0, 0, 0], Some(0));
    t.set_slot("ground", [24, 26, 30], Some(0x99));
    t.set_slot("map.ground", [40, 42, 46], None);
    let findings = theme::validate(&t);
    let m = findings.iter().find(|f| f.message.starts_with("`map.ground`")).expect("the sheet sinks on a white desk");
    assert_eq!(m.grade, theme::ThemeGrade::Meaning);
    assert!(m.message.contains("on a white desk"), "{}", m.message);
}

/// The chrome advisories judge the panel's stack — the panel over the
/// background over the desk — so a `#00000000` pane is judged as the sheet
/// it shows: the user's own theme, text `#cad3f5` over `#00000099`, reads
/// 14:1 on a black desk and 3.83:1 on a white one — an advisory, named.
#[test]
fn test_text_on_a_transparent_pane_is_judged_on_the_sheet_below() {
    let mut t = Theme::default();
    t.set_slot("background", [0, 0, 0], Some(0x99));
    t.set_slot("panel.background", [0, 0, 0], Some(0));
    t.set_slot("text", [0xca, 0xd3, 0xf5], None);
    let findings = theme::validate(&t);
    let n = findings.iter().find(|f| f.message.starts_with("`text` [202, 211, 245]")).expect("the advisory");
    assert_eq!(n.grade, theme::ThemeGrade::Chrome);
    assert!(n.message.contains("on a white desk") && n.message.contains("3.8"), "{}", n.message);
}

/// Every shipped theme judges exactly as before: opaque fills, no desk in
/// any sentence, and `--theme-check`'s `judged:` line without the glass
/// clause; a translucent file gets the clause. Through the binary.
#[cfg(all(feature = "frontend", not(windows)))]
#[test]
fn test_theme_check_speaks_of_desks_only_for_glass() {
    let mut glass = 0;
    for entry in std::fs::read_dir("themes").expect("the themes dir") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let t = theme::theme_from_json(&std::fs::read_to_string(&path).expect("read")).expect("parses");
        if t.translucent() {
            // The glass file (slice 4) is judged at both desks and says
            // so; every other shipped theme is opaque and never speaks of
            // a desk — the old law, byte for byte.
            glass += 1;
            assert_eq!(t.background_appearance, theme::BackgroundAppearance::Blurred, "{}", path.display());
            continue;
        }
        assert!(theme::validate(&t).iter().all(|f| !f.message.contains("desk")), "{}", path.display());
    }
    assert_eq!(glass, 1, "exactly one shipped theme is glass");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_nacs")).args(["--theme-check", "themes/nacs-dark.json"]).output().expect("run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("both desks"), "{stdout}");
    let scratch = std::env::temp_dir().join(format!("nacs-glass-{}.json", std::process::id()));
    std::fs::write(&scratch, r##"{"style":{"background.appearance":"transparent","background":"#1b1b1bcc"}}"##).expect("write");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_nacs")).arg("--theme-check").arg(&scratch).output().expect("run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("the glass (`background.appearance: transparent`): every fill with an alpha judged at both desks, black and white"), "{stdout}");
    let _ = std::fs::remove_file(&scratch);
}

/// Fork 6: the status line's one sentence on the glass — nothing for an
/// opaque theme, nothing before the surface answers, nothing when it was
/// granted (the picture says it), and the fact when it was refused.
#[test]
fn test_the_glass_note_speaks_only_when_glass_was_refused() {
    use theme::BackgroundAppearance::*;
    use view::Blur;
    let over_black = Some(String::from("glass over black: the surface offers no transparency, so the theme's fills are worn over black"));
    assert_eq!(view::glass_note(Opaque, Some(false), &Blur::NotAsked), None);
    assert_eq!(view::glass_note(Blurred, None, &Blur::NotAsked), None);
    assert_eq!(view::glass_note(Transparent, Some(true), &Blur::NotAsked), None);
    assert_eq!(view::glass_note(Blurred, Some(true), &Blur::Applied), None, "granted glass, blurred: the picture says it");
    assert_eq!(view::glass_note(Transparent, Some(false), &Blur::NotAsked), over_black);
    assert_eq!(view::glass_note(Blurred, Some(false), &Blur::NotAsked), over_black);
    // Slice 3: glass granted, blur not — which of the two it got.
    assert_eq!(view::glass_note(Blurred, Some(true), &Blur::NotOnThisPlatform), Some(String::from("glass without blur: not on this platform")));
    assert_eq!(
        view::glass_note(Blurred, Some(true), &Blur::Refused(String::from("HRESULT 0x80070057"))),
        Some(String::from("glass without blur: the platform refused it — HRESULT 0x80070057"))
    );
}

/// The blur's one stderr sentence, each outcome in its own words.
#[test]
fn test_the_blur_sentence_names_each_outcome() {
    use view::Blur;
    assert_eq!(view::blur_sentence(&Blur::NotAsked), "Blur: not asked for");
    assert_eq!(view::blur_sentence(&Blur::NotOnThisPlatform), "Blur: not on this platform — the glass shows the desk unblurred");
    assert_eq!(view::blur_sentence(&Blur::Applied), "Blur: acrylic applied (Windows 11 system backdrop)");
    assert_eq!(view::blur_sentence(&Blur::Refused(String::from("why"))), "Blur: refused by the platform — why");
}

/// The port (Phase 25 slice 4, fork 4): Catppuccin Espresso (Blur)
/// [Light] ships worn whole — Zed's own chrome verbatim (the sheet
/// `background` at the user's Light alpha `99`, every pane `#00000000`),
/// the viewport's ground at the family's default `d7` because the law
/// has no 27-slot bijection at `99`, `blurred` so the window asks for the
/// backdrop, and every meaning slot legal on both desks.
#[cfg(feature = "frontend")]
#[test]
fn test_the_shipped_set_holds_a_glass_face() {
    let text = std::fs::read_to_string("themes/catppuccin-espresso-blur-light.json").expect("the glass face ships");
    let t = theme::theme_from_json(&text).expect("parses");
    assert_eq!(t.background_appearance, theme::BackgroundAppearance::Blurred);
    assert_eq!((t.background, t.fill_alpha.background), ([0, 0, 0], 0x99), "the sheet at the user's Light alpha, Zed's own");
    assert_eq!(t.fill_alpha.panel_background, 0, "every pane is the sheet");
    assert_eq!(t.fill_alpha.text_field, 0);
    assert_eq!((t.ground, t.fill_alpha.ground), ([0, 0, 0], 0xd7), "the drawing sheet at the family's default glass — by measurement, not taste");
    assert_eq!(t.fill_alpha.map_ground, 0xd7);
    assert!(t.translucent());
    let meaning: Vec<_> = theme::validate(&t).into_iter().filter(|f| f.grade == theme::ThemeGrade::Meaning).collect();
    assert!(meaning.is_empty(), "the glass face is legal on both desks: {meaning:?}");
    let (active, _) = theme::apply_policy(t.clone());
    assert_eq!(active, t, "worn whole");
    // The bijection: 27 meaning slots, 26 colours plus the indicator
    // sharing select's, as every port did.
    let mut colours: Vec<[u8; 3]> = t.meaning_slots().into_iter().filter(|(n, _)| *n != "indicator").map(|(_, c)| c).collect();
    assert_eq!(colours.len(), 27);
    colours.sort();
    colours.dedup();
    assert_eq!(colours.len(), 27, "no two meaning slots share a colour");
    assert_eq!(t.indicator.0, t.select, "the indicator is the selection's own colour, translucent");
    // And the user's own numbers: the chrome text is an advisory on a
    // white desk, named — true of the theme in Zed too.
    let text_note = theme::validate(&t).into_iter().find(|f| f.message.starts_with("`text` [202, 211, 245]")).expect("the advisory");
    assert_eq!(text_note.grade, theme::ThemeGrade::Chrome);
    assert!(text_note.message.contains("on a white desk"), "{}", text_note.message);
}

/// Phase 28 slice 5: the inspector's pattern table is `nacs --patterns`, word
/// for word — one function prints both — and the pin is one word written to
/// the run line by the text edit every field uses. Pinning zigzag on
/// `wrench_offset.cam`'s line yields exactly the shipped `wrench_zigzag.cam` program:
/// a picture that could not say what the CLI says would have nothing to pin.
#[test]
fn test_the_pattern_table_is_the_cli_report_and_a_pin_is_a_text_edit() {
    let p = load("wrench_offset.cam");
    let table = view::pattern_table(&p, 0).expect("an End on a Face works a region");
    let cli = nacs::compare::render(&p);
    for row in &table.rows {
        assert!(cli.contains(&format!("  {}\n", row.text)), "the window's row is the terminal's row: {}", row.text);
    }
    for land in &table.lands {
        assert!(cli.contains(&format!("  {land}\n")), "{land}");
    }
    assert!(cli.contains(&format!("  {}\n  {}\n", table.groups, table.header)), "the group line and the header, as the terminal prints them");
    assert!(cli.contains(table.note), "the base is said once, in both");
    // Phase 39 slice 2: the grid's cells ARE the terminal's row — the same
    // words, one cell per column (a spanning cell standing for several), and
    // the columns are the terminal's list.
    assert_eq!(table.columns, &nacs::compare::COLUMNS[..]);
    for row in &table.rows {
        assert_eq!(row.cells.iter().map(|c| c.span).sum::<usize>(), table.columns.len(), "{}: the cells span every column", row.word);
        let words: Vec<&str> = row.cells.iter().flat_map(|c| c.text.split_whitespace()).collect();
        let line: Vec<&str> = row.text.trim_start_matches(['*', ' ']).split_whitespace().collect();
        assert_eq!(words, line, "{}: the cells joined are the line", row.word);
    }
    assert_eq!(table.rows.iter().map(|r| (r.word, r.current, r.pinnable)).collect::<Vec<_>>(), [("offset", true, true), ("zigzag", false, true), ("oneway", false, true), ("spine", false, true), ("held", false, true)]);

    // The pin: `set_key` on the op's own line.
    let line = &p.op_texts[0];
    let pinned = view::set_key(line, "pattern", Some("zigzag")).unwrap();
    assert_eq!(pinned, "run End(tool=T5, surface=Top, stepover=5.0, pattern=zigzag)");
    let src = std::fs::read_to_string("wrench_offset.cam/main.part").unwrap().replace(line.as_str(), &pinned);
    let geom = std::fs::read_to_string("wrench_offset.cam/geometry.part").unwrap();
    let q = parse::parse_project(&src, Some(&geom)).unwrap();
    assert_eq!(
        nacs::gcode::program(&q).unwrap().trim(),
        std::fs::read_to_string("tests/wrench_zigzag.golden.gcode").unwrap().trim()
    );
    assert!(view::pattern_table(&q, 0).unwrap().rows[1].current, "and the table marks the pin");
    // Un-pinning is the same edit the other way.
    assert_eq!(view::set_key(&pinned, "pattern", None).unwrap(), "run End(tool=T5, surface=Top, stepover=5.0)");
    // …which is the default, not the line it started from: this fixture is pinned.
    assert_eq!(line, "run End(tool=T5, surface=Top, stepover=5.0, pattern=offset)");

    // A refused pattern is a row that cannot be pinned; an op that works no
    // region has no table.
    let post = parse::parse(&std::fs::read_to_string("pocket_post.cam/main.part").unwrap().replace("stepover=4.0)", "stepover=6.0, pattern=zigzag)")).unwrap();
    let t = view::pattern_table(&post, 0).unwrap();
    assert!(!t.rows[0].pinnable && t.rows[0].text.contains("refused — "));
    let sphere = load("sphere.cam");
    assert!((0..sphere.operations.len()).all(|i| view::pattern_table(&sphere, i).is_none()));
}

/// Phase 32: the inspector's card READS that line from the outline — it does
/// not ask the stock model, which is the same bug one call site over (the
/// card is built every frame the inspector is open, and `walled_rails_offset.cam`
/// fell to 7 fps again the moment an op was selected). Pinned by a sentinel:
/// what the outline says is what the card shows.
#[test]
fn test_the_cards_removal_line_comes_from_the_outline_not_the_model() {
    let src = std::fs::read_to_string("walled_rails_offset.cam/main.part").unwrap();
    let geom = std::fs::read_to_string("walled_rails_offset.cam/geometry.part").unwrap();
    let p = parse::parse_project(&src, Some(&geom)).unwrap();
    let mut o = nacs::outline::outline(&p);
    o.removals[0] = "SENTINEL — from the outline".to_string();
    let card = view::card(&p, &o, view::Selection::Operation(0), &[], &[], &[], &[], None);
    match card {
        view::Card::Operation { removal, .. } => assert_eq!(removal, "SENTINEL — from the outline"),
        other => panic!("an operation selects an operation card: {other:?}"),
    }
}

/// Phase 32: the outline carries each op's removal line, taken ONCE with the
/// rest of it, and it is word for word what `removal_line` says. The window
/// reads it from here — it asked `removal_line` every frame until a Face's
/// removal began costing a coverage grid, and `walled_rails_offset.cam` fell to 7
/// fps (`NACS_FRAME_BENCH` measured the outline pane at 124 ms a frame).
#[test]
fn test_the_outline_carries_every_removal_line_once() {
    for dir in ["walled_rails_offset.cam", "plate.cam", "curved_rails_spine.cam", "pocket_post.cam"] {
        let src = std::fs::read_to_string(format!("{dir}/main.part")).unwrap();
        let geom = std::fs::read_to_string(format!("{dir}/geometry.part")).ok();
        let p = parse::parse_project(&src, geom.as_deref()).unwrap();
        let o = nacs::outline::outline(&p);
        assert_eq!(o.removals.len(), p.operations.len(), "{dir}");
        for (i, line) in o.removals.iter().enumerate() {
            assert_eq!(line, &nacs::outline::removal_line(&p, i), "{dir} op {i}");
        }
    }
}

// ---- Phase 41 slice 1: the focus, and the floor's kinds drawn ------------
// (`DECISIONS.md` → "Phase 41 opens on the ruling, and slice 1…")

/// The focus is an ACT with its own key, beside the fit; selection framing
/// is untouched — it centres (`frame_point`) and never zooms, by the
/// user's ruling of 2026-10-04.
#[test]
fn test_the_focus_is_an_act_with_its_own_key_and_framing_still_only_centres() {
    assert_eq!(view::action("view.focus").unwrap().key, ".");
    assert_eq!(view::action("view.fit").unwrap().key, "f");
    assert!(view::palette_matches("focus").iter().any(|a| a.id == "view.focus"));
    let help = view::help_lines().join("\n");
    assert!(help.contains("  .              focus the selection"), "{help}");
    assert!(help.contains("the floor's edges"), "{help}");
}

/// The tween: starts where it is, ends where it goes, turns the SHORT way
/// round, and zooms in log space — the midpoint of 1→100 is 10.
#[test]
fn test_the_camera_tween_starts_here_ends_there_turns_the_short_way_and_zooms_in_log_space() {
    let from = view::Camera { yaw: 3.0, pitch: 0.2, scale: 1.0, target: [0.0, 0.0, 0.0] };
    let to = view::Camera { yaw: -3.0, pitch: 1.0, scale: 100.0, target: [10.0, 20.0, 30.0] };
    let at0 = from.toward(&to, 0.0);
    assert_eq!((at0.yaw, at0.pitch, at0.scale, at0.target), (from.yaw, from.pitch, from.scale, from.target));
    let at1 = from.toward(&to, 1.0);
    assert_eq!((at1.yaw, at1.pitch, at1.scale, at1.target), (to.yaw, to.pitch, to.scale, to.target));
    let mid = from.toward(&to, 0.5);
    // 3.0 → −3.0 is 0.283 rad the short way, through ±π — not 6 rad back.
    assert!((mid.yaw - std::f64::consts::PI).abs() < 1e-9, "{}", mid.yaw);
    assert!((mid.pitch - 0.6).abs() < 1e-12);
    assert!((mid.scale - 10.0).abs() < 1e-9, "{}", mid.scale);
    assert_eq!(mid.target, [5.0, 10.0, 15.0]);
    // Out of range clamps; a non-finite fraction arrives.
    assert_eq!(from.toward(&to, 7.0).scale, to.scale);
    assert_eq!(from.toward(&to, -1.0).scale, from.scale);
    assert_eq!(from.toward(&to, f64::NAN).scale, to.scale);
    // The ease is smoothstep: still at both ends, half way at the middle.
    assert_eq!(view::ease(0.0), 0.0);
    assert_eq!(view::ease(1.0), 1.0);
    assert_eq!(view::ease(0.5), 0.5);
    assert!(view::ease(0.25) < 0.25 && view::ease(0.75) > 0.75);
}

/// The focused camera keeps the user's yaw, tilts to the focus pitch, and
/// fits the box the way the fit does.
#[test]
fn test_focused_on_keeps_yaw_tilts_and_fits_the_box() {
    let cam = view::Camera { yaw: 0.3, pitch: 0.1, scale: 7.0, target: [1.0, 1.0, 1.0] };
    let bb = [0.0, 0.0, -10.0, 40.0, 30.0, 0.0];
    let f = cam.focused_on(bb, 800.0, 600.0);
    assert_eq!(f.yaw, 0.3);
    assert_eq!(f.pitch, view::FOCUS_PITCH);
    assert_eq!(f.target, [20.0, 15.0, -5.0]);
    let mut fitted = cam;
    fitted.fit_to_aabb(bb, 800.0, 600.0);
    assert_eq!(f.scale, fitted.scale);
    assert!(view::FOCUS_PITCH > view::Camera::default_orbit().pitch && view::FOCUS_PITCH < std::f64::consts::FRAC_PI_2);
}

/// What the focus fits: the selected feature's edges; an operation's
/// feature's edges (its paths fold in when there are any); nothing for
/// nothing — the shell then fits the scene.
#[test]
fn test_focus_aabb_is_the_selection_s_box_and_none_for_nothing() {
    let p = load("step.cam");
    let edges = view::feature_edges(&p);
    let bb = view::focus_aabb(&p, view::Selection::Feature(0), &[], &edges, &[]).unwrap();
    let want = view::edges_aabb(&edges[0..1], [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY]);
    assert_eq!(bb, want);
    // The op works `Step`, so with no paths its box is the feature's.
    assert_eq!(view::focus_aabb(&p, view::Selection::Operation(0), &[], &edges, &[]), Some(want));
    assert_eq!(view::focus_aabb(&p, view::Selection::None, &[], &edges, &[]), None);
    assert_eq!(view::focus_aabb(&p, view::Selection::Stock, &[], &edges, &[]), None);
}

/// The floor's kinds as strokes: the step's clipped outline is one WALL
/// run (the pocket's own wall at x=60) and one OPEN run of three edges
/// joined across the loop's seam — what `region::of` says, drawn, at the
/// floor's height.
#[test]
fn test_the_step_s_floor_is_one_wall_run_and_one_open_run_at_the_floor_s_height() {
    let p = load("step.cam");
    let edges = view::feature_edges(&p);
    assert_eq!(view::region_op(&p, view::Selection::Operation(0), &[], &edges), Some(0));
    assert_eq!(view::region_op(&p, view::Selection::Feature(0), &[], &edges), Some(0), "a feature's floor is its first op's");
    assert_eq!(view::region_op(&p, view::Selection::None, &[], &edges), None);
    let strokes = view::region_strokes(&p, 0, &edges);
    assert_eq!(strokes.len(), 2, "{strokes:?}");
    let wall = strokes.iter().find(|s| s.kind == view::EdgeStroke::Wall).expect("a wall run");
    let open = strokes.iter().find(|s| s.kind == view::EdgeStroke::Open).expect("an open run");
    assert_eq!(wall.pts.len(), 2);
    assert!(wall.pts.iter().all(|q| (q[0] - 60.0).abs() < 1e-9), "{:?}", wall.pts);
    assert_eq!(open.pts.len(), 4, "three open edges, one run: {:?}", open.pts);
    for s in &strokes {
        assert!(s.pts.iter().all(|q| (q[2] + 5.0).abs() < 1e-9), "at the floor z = -5: {:?}", s.pts);
    }
}

/// Islands are closed WALL loops inside the outer; a bridged mouth is a
/// dotted ring at the FLOOR's height, read from the feature's own drawn
/// edges and never restated.
#[test]
fn test_islands_are_closed_wall_loops_and_a_bridged_mouth_is_a_ring_at_the_floor() {
    let p = load("pocket_islands.cam");
    let edges = view::feature_edges(&p);
    let op = (0..p.operations.len()).find(|&i| !view::region_strokes(&p, i, &edges).is_empty()).expect("a floor op");
    let strokes = view::region_strokes(&p, op, &edges);
    assert!(strokes.iter().all(|s| s.kind == view::EdgeStroke::Wall), "a closed pocket with posts has no air: {strokes:?}");
    let closed = strokes.iter().filter(|s| s.pts.first() == s.pts.last()).count();
    assert!(closed >= 4, "the outer and three posts, each a closed loop: {closed}");

    let faced = parse::parse_project_with_base(
        "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
         stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\ntool T5 Endmill(dia=10.0)\n\
         feature Deck Plane(z=-2)\nfeature Hole CylinderBore(dia=12, extent=10, x=50, y=40)\n\
         run End(tool=T5, surface=Deck, stepover=6.0)\n",
        None,
        None,
    )
    .unwrap();
    let edges = view::feature_edges(&faced);
    let strokes = view::region_strokes(&faced, 0, &edges);
    let bridged: Vec<_> = strokes.iter().filter(|s| s.kind == view::EdgeStroke::Bridged).collect();
    assert_eq!(bridged.len(), 1, "one mouth, drawn once though the bore has two rings: {strokes:?}");
    let ring = &bridged[0].pts;
    assert_eq!(ring.first(), ring.last());
    assert!(ring.iter().all(|q| (q[2] + 2.0).abs() < 1e-9 && ((q[0] - 50.0).hypot(q[1] - 40.0) - 6.0).abs() < 1e-6), "{ring:?}");
    let open: Vec<_> = strokes.iter().filter(|s| s.kind == view::EdgeStroke::Open).collect();
    assert_eq!(open.len(), 1, "a plane's four edges are all air, one run");
    assert_eq!(open[0].pts.len(), 5);
}

/// The default orbit agrees with the sheet (Phase 43 slice 3): +X images
/// to the right, +Y to the right and UP, +Z up — so the plan minimap's
/// drag moves the picture the way the hand moves and the `front` stance
/// looks from the same side the default does. The first hand found the
/// old default standing at +X +Y, with +X running left against a map whose
/// X runs right.
#[test]
fn test_the_default_orbit_stands_front_right_of_the_sheet() {
    let cam = view::Camera::default_orbit();
    let lens = cam.lens();
    let o = lens.project([0.0, 0.0, 0.0]);
    let x = lens.project([1.0, 0.0, 0.0]);
    let y = lens.project([0.0, 1.0, 0.0]);
    let z = lens.project([0.0, 0.0, 1.0]);
    assert!(x.0 > o.0 && x.1 < o.1, "+X runs right and down: {x:?}");
    assert!(y.0 > o.0 && y.1 > o.1, "+Y runs right and up: {y:?}");
    assert!((z.0 - o.0).abs() < 1e-6 && z.1 > o.1, "+Z runs straight up: {z:?}");
    // The front stance and the default look from the same side of the
    // part: both put +Y INTO the screen (farther from the viewer).
    let mut front = view::Camera::default_orbit();
    front.set_stance(view::Stance::Front);
    let into = |c: &view::Camera| (c.lens().project([0.0, 1.0, 0.0]).2 - c.lens().project([0.0; 3]).2).signum();
    assert_eq!(into(&cam), into(&front), "the default and the front stance look from the same side of the part");
    // A drag on the plan map moves the target in world XY (the shell's
    // rule); at the default orbit that moves the picture WITH the hand:
    // a target pushed +X puts the scene's origin to the left on screen,
    // which is what dragging the map rightward should do.
    let mut moved = view::Camera::default_orbit();
    moved.target[0] += 10.0;
    assert!(moved.lens().project([0.0; 3]).0 < lens.project([0.0; 3]).0);
}

/// A stepover's grip has a ceiling (Phase 43 slice 4): a hair under the
/// tool's diameter, where `validate` refuses it — the drag clamps there,
/// the clamped value still parses, and the grip says why. A key with no
/// ceiling clamps nowhere.
#[test]
fn test_a_stepover_grip_stops_under_the_tools_diameter_and_says_so() {
    let p = parse::parse_project_with_base(
        "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
         stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\ntool T5 Endmill(dia=10.0)\n\
         feature Deck Plane(z=-2)\n\
         run End(tool=T5, surface=Deck, stepover=4.0)\n",
        None,
        None,
    )
    .unwrap();
    let edges = view::feature_edges(&p);
    let hs = view::handles(&p, view::Selection::Operation(0), &[], &edges);
    let step = hs.iter().find(|h| h.key == "stepover").unwrap();
    assert!((step.max - 9.999).abs() < 1e-9, "a hair under T5's diameter: {}", step.max);
    assert_eq!(step.stop.as_deref(), Some("stepover stops under T5's diameter (10mm)"));
    // Plan view at scale 4: the inward normal of the plane's first edge is
    // +Y (the blank's bottom edge, CCW), 4 px per mm; a drag of 400px
    // along it asks for +100mm and gets the ceiling.
    let mut cam = view::Camera::plan();
    cam.scale = 4.0;
    let (ax, ay) = (step.axis[0], step.axis[1]);
    let v = view::handle_drag(step, &cam, (0.0, 0.0), (400.0 * ax, 400.0 * ay)).unwrap();
    assert!((v - 9.999).abs() < 1e-9, "clamped at the ceiling: {v}");
    assert_eq!(view::handle_stop(step, v), Some("stepover stops under T5's diameter (10mm)"));
    assert_eq!(view::handle_stop(step, 4.5), None, "under the ceiling nothing is said");
    assert_eq!(view::handle_text(v), "9.999");
    // The clamped text is what the parser accepts: one hair under refuses
    // nothing, the diameter itself is refused.
    let src = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
         stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\ntool T5 Endmill(dia=10.0)\n\
         feature Deck Plane(z=-2)\n";
    assert!(parse::parse_project_with_base(&format!("{src}run End(tool=T5, surface=Deck, stepover=9.999)\n"), None, None).is_ok());
    let err = parse::parse_project_with_base(&format!("{src}run End(tool=T5, surface=Deck, stepover=10)\n"), None, None).unwrap_err();
    assert!(err.contains("uncut strip") && err.contains("reduce stepover"), "{err}");
    // No ceiling on an allowance.
    let skin = hs.iter().find(|h| h.key == "stock_to_leave");
    assert!(skin.is_none() || skin.unwrap().max == f64::INFINITY);
}

/// ROADMAP item 3 slice 3: the Engagement layer draws the immersion field
/// within a budget of squares. The plate's face under a ⌀2 is 153,600
/// cells; drawn, it is squares of 3 × 3 cells, each the worst band of the
/// cells under it, and the layer's line says so. A field under the budget
/// is drawn as itself and the line says nothing more.
#[test]
fn test_the_engagement_layer_draws_the_field_within_a_budget_the_worst_of_each_square() {
    assert_eq!(view::immersion_draw_step(100, 100, 20_000), 1);
    assert_eq!(view::immersion_draw_step(480, 320, 20_000), 3, "sqrt(153600 / 20000) rounds up to 3");
    assert_eq!(view::immersion_drawn_note(1), None);
    assert_eq!(view::immersion_drawn_note(3).as_deref(), Some("drawn 3×3 cells a square, the worst of each"));
    let main = std::fs::read_to_string("plate.cam/main.part").unwrap().replace("tool T5 Endmill(dia=10.0)", "tool T5 Endmill(dia=2.0)").replace("stepover=6.0", "stepover=1.5").replace("stepover=4.0", "stepover=1.0");
    let geom = std::fs::read_to_string("plate.cam/geometry.part").unwrap();
    let p = parse::parse_project_with_base(&main, Some(&geom), None).unwrap();
    let f = nacs::engagement::immersion(&p, 0).expect("the face's field");
    assert_eq!((f.nx, f.ny), (480, 320), "an eighth of the diameter over 120 × 80");
    let b = view::immersion_blocks(&f);
    assert_eq!(b.step, 3);
    assert!((b.side - 0.75).abs() < 1e-12);
    assert!(b.blocks.len() <= view::IMMERSION_DRAW_BUDGET, "{} squares", b.blocks.len());
    assert!(b.blocks.len() > 10_000, "most of the face has a value: {} squares", b.blocks.len());
    // Each square's band is the worst of the cells under it.
    let rank = |band: nacs::engagement::Band| match band {
        nacs::engagement::Band::Within => 0,
        nacs::engagement::Band::Above => 1,
        nacs::engagement::Band::Past90 => 2,
    };
    for (x, y, band) in b.blocks.iter().step_by(97) {
        let worst = f
            .cells()
            .filter(|(cx, cy, _)| (cx - x).abs() <= b.side / 2.0 && (cy - y).abs() <= b.side / 2.0)
            .map(|(_, _, t)| rank(f.band(t)))
            .max()
            .expect("a square has a cell under it");
        assert_eq!(rank(*band), worst, "the square at ({x}, {y})");
    }
    // Under the budget, the field is drawn as itself.
    let ten = parse::load_project("plate.cam").unwrap();
    let f = nacs::engagement::immersion(&ten, 0).unwrap();
    let b = view::immersion_blocks(&f);
    assert_eq!(b.step, 1);
    assert_eq!(b.blocks.len(), f.cells().count());
}

/// ROADMAP item 3 slice 2: the stepover grip's floor is a twentieth of the
/// tool's diameter (it was 0.001 mm, and a drag through it froze the
/// window for a minute planning the offset family), said on the status
/// line the way the ceiling is.
#[test]
fn test_a_stepover_grip_stops_at_a_twentieth_of_the_diameter_and_says_so() {
    assert!((view::stepover_floor(10.0) - 0.5).abs() < 1e-12);
    assert!((view::stepover_floor(2.0) - 0.1).abs() < 1e-12);
    let p = parse::parse_project_with_base(
        "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
         stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\ntool T5 Endmill(dia=10.0)\n\
         feature Deck Plane(z=-2)\n\
         run End(tool=T5, surface=Deck, stepover=4.0)\n",
        None,
        None,
    )
    .unwrap();
    let edges = view::feature_edges(&p);
    let hs = view::handles(&p, view::Selection::Operation(0), &[], &edges);
    let step = hs.iter().find(|h| h.key == "stepover").unwrap();
    assert!((step.min - 0.5).abs() < 1e-12, "a twentieth of T5's diameter: {}", step.min);
    let floor = "stepover stops at a twentieth of T5's diameter (0.5mm) — a finer one takes minutes to plan; type it if you mean it";
    assert_eq!(step.floor_stop.as_deref(), Some(floor));
    let mut cam = view::Camera::plan();
    cam.scale = 4.0;
    let (ax, ay) = (step.axis[0], step.axis[1]);
    // Dragged 400 px back along its axis the grip asks for −96 mm and
    // gets the floor.
    let v = view::handle_drag(step, &cam, (0.0, 0.0), (-400.0 * ax, -400.0 * ay)).unwrap();
    assert!((v - 0.5).abs() < 1e-12, "clamped at the floor: {v}");
    assert_eq!(view::handle_stop(step, v), Some(floor));
    assert_eq!(view::handle_stop(step, 4.0), None, "between the floor and the ceiling nothing is said");
    assert_eq!(view::handle_text(v), "0.5");
}

// ---- Phase 41 slice 4: the handles ----------------------------------------
// (`DECISIONS.md` → "Phase 41 slice 4…")

/// A bore's handles sit where their keys mean: the diameter's on the rim
/// at +X (two millimetres of value per millimetre of grip), the extent's
/// at the bottom pointing down, the centre's at the centre; an operation's
/// stepover a radius plus itself in from the first wall. Only keys the
/// line carries have a grip, and a block statement has none.
#[test]
fn test_handles_sit_where_their_keys_mean_and_only_for_keys_the_line_carries() {
    let p = parse::parse_project_with_base(
        "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
         stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\ntool T5 Endmill(dia=10.0)\n\
         feature Hole CylinderBore(dia=12, extent=10, x=50, y=40)\n\
         feature Deck ProfilePocket(extent=5\n    10 10\n    90 10\n    90 70\n    10 70\n)\n\
         run Clear(tool=T5, profile=Deck, stepover=4.0, stock_to_leave=0.5)\n",
        None,
        None,
    )
    .unwrap();
    let edges = view::feature_edges(&p);
    let hole = edges.iter().position(|e| e.name == "Hole").unwrap();
    let hs = view::handles(&p, view::Selection::Feature(hole), &[], &edges);
    let keys: Vec<&str> = hs.iter().map(|h| h.key.as_str()).collect();
    assert_eq!(keys, ["dia", "extent", "x", "y"]);
    let dia = hs.iter().find(|h| h.key == "dia").unwrap();
    assert_eq!((dia.at, dia.axis, dia.per_mm, dia.value), ([56.0, 40.0, 0.0], [1.0, 0.0, 0.0], 2.0, 12.0));
    let ext = hs.iter().find(|h| h.key == "extent").unwrap();
    assert_eq!((ext.at, ext.axis, ext.value), ([56.0, 40.0, -10.0], [0.0, 0.0, -1.0], 10.0));
    assert!(hs.iter().all(|h| h.statement == view::Statement::Feature("Hole".to_string())));
    // The block statement: since Phase 50 slice 6 its OPENER's keys get
    // grips (extent at the first vertex, z at the rim when carried) and
    // nothing else does — the points stay the file's.
    let deck = edges.iter().position(|e| e.name == "Deck").unwrap();
    let deck_keys: Vec<String> = view::handles(&p, view::Selection::Feature(deck), &[], &edges).iter().map(|h| h.key.clone()).collect();
    assert!(!deck_keys.is_empty() && deck_keys.iter().all(|k| k == "extent" || k == "z"), "{deck_keys:?}");
    // The operation: the allowance in from the first wall, the stepover a
    // radius plus the allowance plus itself in — along the inward normal
    // of the pocket's first edge (10,10)→(90,10), which is +Y.
    let hs = view::handles(&p, view::Selection::Operation(0), &[], &edges);
    let keys: Vec<&str> = hs.iter().map(|h| h.key.as_str()).collect();
    assert_eq!(keys, ["stock_to_leave", "stepover"]);
    let skin = &hs[0];
    assert_eq!((skin.at, skin.axis, skin.value, skin.min), ([50.0, 10.5, -5.0], [0.0, 1.0, 0.0], 0.5, 0.0));
    let step = &hs[1];
    assert_eq!((step.at, step.value), ([50.0, 19.5, -5.0], 4.0));
    assert!(step.min > 0.0, "a stepover stays positive");
    assert!(view::handles(&p, view::Selection::None, &[], &edges).is_empty());
}

/// The drag: travel projected onto the axis as the camera draws it, in
/// mm, times the handle's scale, floored at its minimum; nothing when the
/// axis looks at the viewer. The text is what a hand would type.
#[test]
fn test_a_drag_drafts_along_the_axis_as_drawn_and_the_text_is_what_a_hand_types() {
    let h = view::Handle {
        statement: view::Statement::Feature("Hole".into()),
        key: "dia".into(),
        at: [56.0, 40.0, 0.0],
        axis: [1.0, 0.0, 0.0],
        per_mm: 2.0,
        value: 12.0,
        min: 0.001,
        max: f64::INFINITY,
        stop: None,
        floor_stop: None,
    };
    // Plan view at scale 4: +X runs right across the screen, 4 px per mm.
    let mut cam = view::Camera::plan();
    cam.scale = 4.0;
    // Twelve pixels right is 3 mm of grip, 6 mm of diameter.
    assert_eq!(view::handle_drag(&h, &cam, (0.0, 0.0), (12.0, 0.0)), Some(18.0));
    // Travel across the axis moves nothing.
    assert_eq!(view::handle_drag(&h, &cam, (0.0, 0.0), (0.0, 40.0)), Some(12.0));
    // Dragged far left: the floor holds.
    assert_eq!(view::handle_drag(&h, &cam, (0.0, 0.0), (-400.0, 0.0)), Some(0.001));
    // An axis looking at the viewer drafts nothing: Z under the plan view.
    let z = view::Handle { axis: [0.0, 0.0, 1.0], ..h.clone() };
    assert_eq!(view::handle_drag(&z, &cam, (0.0, 0.0), (10.0, 10.0)), None);
    // The pick: within tolerance of the grip's projection, the nearest.
    let far = view::Handle { at: [156.0, 40.0, 0.0], ..h.clone() };
    let (gx, gy, _) = cam.project(h.at);
    assert_eq!(view::pick_handle(&[far.clone(), h.clone()], &cam, f64::from(gx) + 3.0, f64::from(gy), 8.0), Some(1));
    assert_eq!(view::pick_handle(&[far, h], &cam, f64::from(gx) + 30.0, f64::from(gy), 8.0), None);
    assert_eq!(view::handle_text(12.5), "12.5");
    assert_eq!(view::handle_text(4.0), "4");
    assert_eq!(view::handle_text(0.25), "0.25");
    assert_eq!(view::handle_text(0.0004), "0");
    assert_eq!(view::handle_text(-3.1), "-3.1");
    let help = view::help_lines().join("\n");
    assert!(help.contains("drag a grip"), "{help}");
}

/// Phase 45 slice 2: a bore's grips sit on its RIM. With `z` on the line the
/// diameter's grip is on the rim at the declared height, the extent's one
/// extent under it, and the centre gains a `z` grip at the bottom (the
/// centre convention); without `z` there is no `z` grip, because a grip is
/// a key the line carries.
#[test]
fn test_sunken_bore_grips_sit_on_its_rim() {
    let src = |z: &str| {
        format!(
            "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
             stock Box(w=100, l=100, h=20, x=0, y=0, z=0)\ntool T5 Endmill(dia=10.0)\n\
             feature Bore CylinderBore(dia=30, extent=6, x=50, y=50{z})\n"
        )
    };
    let sunken = parse::parse_project_with_base(&src(", z=-8"), None, None).unwrap();
    let edges = view::feature_edges(&sunken);
    let hs = view::handles(&sunken, view::Selection::Feature(0), &[], &edges);
    let at = |key: &str| hs.iter().find(|h| h.key == key).unwrap_or_else(|| panic!("no {key} grip: {hs:?}")).at;
    assert_eq!(at("dia"), [65.0, 50.0, -8.0], "the diameter's grip on the rim");
    assert_eq!(at("extent"), [65.0, 50.0, -14.0], "the extent's grip one extent under the rim");
    assert_eq!(at("z"), [50.0, 50.0, -14.0], "the rim's grip at the bottom, the centre convention");
    assert_eq!(hs.iter().find(|h| h.key == "z").unwrap().value, -8.0, "the grip's value is the key's");

    let top = parse::parse_project_with_base(&src(""), None, None).unwrap();
    let edges = view::feature_edges(&top);
    let hs = view::handles(&top, view::Selection::Feature(0), &[], &edges);
    assert!(hs.iter().all(|h| h.key != "z"), "no `z` on the line, no `z` grip: {hs:?}");
    assert_eq!(hs.iter().find(|h| h.key == "dia").unwrap().at, [65.0, 50.0, 0.0], "omitted, the rim is the stock top");
}

// ---- Phase 50 slice 2: the way out (DESIGN.md §5.3, §1) ----

/// The program file is the project's own name beside its `main.part`:
/// `plate.cam/plate.nc`, a project given by its `main.part` its directory,
/// a directory without the `.cam` its name as it is.
#[test]
fn test_the_program_file_is_named_for_the_project_beside_it() {
    use std::path::{Path, PathBuf};
    let v = |s: &str| nacs::view::program_file_path(Path::new(s));
    assert_eq!(v("plate.cam"), PathBuf::from("plate.cam").join("plate.nc"));
    assert_eq!(v("some/where/wrench_spine.cam"), PathBuf::from("some/where/wrench_spine.cam").join("wrench_spine.nc"));
    assert_eq!(v("plate.cam/main.part"), PathBuf::from("plate.cam").join("plate.nc"));
    assert_eq!(v("jobs/bracket"), PathBuf::from("jobs/bracket").join("bracket.nc"));
    assert_eq!(nacs::view::program_text("%\nM30\n%"), "%\nM30\n%\n", "stdout's one trailing newline");
}

/// The write lands the program's text beside the project, byte for byte
/// what stdout carries, and the watch does not notice: the `.nc` is not a
/// file the parse opened, so `newest_mtime` over the sources is unmoved.
#[test]
fn test_writing_the_program_beside_the_project_leaves_the_watch_quiet() {
    let dir = std::env::temp_dir().join(format!("nacs_program_write_{}.cam", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let main = dir.join("main.part");
    std::fs::write(&main, std::fs::read_to_string("plate.cam/main.part").expect("read")).expect("write main");
    std::fs::write(dir.join("geometry.part"), std::fs::read_to_string("plate.cam/geometry.part").expect("read")).expect("write geometry");
    let sources = vec![main.clone(), dir.join("geometry.part")];
    let before = nacs::view::newest_mtime(&sources).expect("a stamp");
    let part = nacs::parse::load_project(dir.to_str().unwrap()).expect("parses");
    let g = nacs::gcode::program(&part).expect("emits");
    std::thread::sleep(std::time::Duration::from_millis(20));
    let path = nacs::view::write_program(&dir, &g).expect("written");
    assert_eq!(path, dir.join(format!("nacs_program_write_{}.nc", std::process::id())));
    assert_eq!(std::fs::read_to_string(&path).expect("read back"), format!("{g}\n"));
    assert_eq!(nacs::view::newest_mtime(&sources).expect("a stamp"), before, "the watch polls the sources, and a .nc is not one");
    // The view's text is the program's, so the window's write is the CLI's.
    assert_eq!(nacs::view::gcode_view(&part).expect("a view").text(), g);
    assert_eq!(nacs::view::program_written_note("plate.nc", 120), "Written — plate.nc: the program, 120 lines");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Both doors are keyless acts (DESIGN.md §9) the palette finds under
/// "program", beside the gutters' toggles.
#[test]
fn test_copy_and_write_program_are_keyless_acts_the_palette_finds() {
    for id in ["program.copy", "program.write"] {
        let a = nacs::view::action(id).expect("the act exists");
        assert_eq!(a.key, "", "keyless by DESIGN.md §9");
    }
    let found: Vec<&str> = nacs::view::palette_matches("program").iter().map(|a| a.id).collect();
    assert!(found.contains(&"program.copy") && found.contains(&"program.write"), "{found:?}");
    let help = nacs::view::help_lines().join("\n");
    assert!(help.contains("program.write") || help.contains("write the program"), "the help names the door: {help}");
}

// ---- Phase 50 slice 3: the way in (DESIGN.md §1, §5.4) ----

/// The workspace file remembers the last project: `project <path>`,
/// round-tripped, absent when never opened.
#[test]
fn test_the_workspace_file_remembers_the_last_project() {
    let ws = theme::Workspace { project: Some("C:/jobs/plate.cam".to_string()), ..theme::Workspace::default() };
    assert!(ws.render().contains("project C:/jobs/plate.cam\n"));
    assert_eq!(theme::Workspace::parse(&ws.render()), ws, "parse(render(x)) == x");
    assert_eq!(theme::Workspace::default().project, None);
    assert!(!theme::Workspace::default().render().contains("project"));
}

/// The header is a statement of six lines (DESIGN.md §5.4, Phase 50 slice
/// 3): the form reads each line's value, a missing line is a blank, every
/// one required; a write rewrites the line in place or inserts before the
/// stock line.
#[test]
fn test_the_header_is_a_form_and_its_write_lands_in_place_or_before_the_stock() {
    let text = "units mm\nfeed 800\nplunge 300\n# the blank\nstock Box(w=100, l=80, h=20, x=0, y=0, z=0)\nspindle 3000\nrapid_z 5\nclearance_z 25\n";
    let p = parse::parse_project(text, None).unwrap();
    let form = view::statement_form(&p, &view::Statement::Header).unwrap();
    assert_eq!(form.head, "header");
    let keys: Vec<&str> = form.fields.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys, vec!["units", "feed", "plunge", "spindle", "rapid_z", "clearance_z"]);
    assert!(form.fields.iter().all(|f| f.required && f.options.is_none()));
    assert_eq!(form.fields[1].value.as_deref(), Some("800"));
    assert_eq!(view::header_write_plan(&p, "feed"), Some(1));
    assert_eq!(view::header_write_plan(&p, "spindle"), Some(5), "a header line after the stock is still that line");
    // A value typed is the new line, in place.
    let after = view::replace_line(text, 1, &view::header_line("feed", " 1200 ")).unwrap();
    assert!(after.starts_with("units mm\nfeed 1200\nplunge 300\n"));
    parse::parse_project(&after, None).expect("still a program");
    // A line the file lacks is inserted before the stock line.
    let short = text.replace("feed 800\n", "");
    parse::parse_project(&short, None).expect("no operation runs, so the header law has nothing to judge yet");
    let mut p2 = parse::Part::default();
    p2.stock_line = Some((0, 3, "stock Box(w=1, l=1, h=1)".to_string()));
    assert_eq!(view::header_insert_at(&p2), 3);
    let inserted = view::insert_line(&short, 3, &view::header_line("feed", "800"));
    assert_eq!(inserted, "units mm\nplunge 300\n# the blank\nfeed 800\nstock Box(w=100, l=80, h=20, x=0, y=0, z=0)\nspindle 3000\nrapid_z 5\nclearance_z 25\n");
    parse::parse_project(&inserted, None).expect("whole again");
    assert_eq!(view::insert_line("", 0, "units mm"), "units mm\n");
    assert_eq!(view::insert_line("a\nb", 9, "c"), "a\nb\nc");
    assert_eq!(view::header_value("feed 800   # fast"), "800");
    // An emptied header field is refused by the form, as any required key.
    assert!(view::field_write(&form, "feed", "  ").unwrap_err().contains("needs feed"));
    // The empty stance's form has nothing to say about it: no project, no form.
    assert!(view::statement_form(&parse::Part::default(), &view::Statement::Header).is_ok(), "six blanks");
}

/// A drop opens a project: a directory as itself, a `.part` file as its
/// directory, anything else ignored; the first that is one wins.
#[test]
fn test_a_dropped_folder_or_part_file_names_the_project() {
    let dir = std::env::temp_dir().join(format!("nacs_drop_{}.cam", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    let main = dir.join("main.part");
    std::fs::write(&main, "units mm\n").expect("write");
    let other = dir.join("notes.txt");
    std::fs::write(&other, "x").expect("write");
    assert_eq!(view::dropped_project(&[other.clone(), dir.clone()]), Some(dir.clone()));
    assert_eq!(view::dropped_project(&[main.clone()]), Some(dir.clone()));
    assert_eq!(view::dropped_project(&[other.clone()]), None);
    assert_eq!(view::dropped_project(&[]), None);
    // The empty stance: a part that names the files the watch follows and
    // declares nothing, and the sentence the viewport says.
    let empty_dir = std::env::temp_dir().join(format!("nacs_empty_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&empty_dir);
    let p = view::empty_project_part(&empty_dir);
    assert_eq!(p.sources, vec![empty_dir.join("main.part")]);
    assert!(p.features.is_empty() && p.operations.is_empty());
    let note = view::stance_note(&p, Some("cannot read main.part")).unwrap();
    assert!(note.starts_with(&format!("{} has no main.part yet", empty_dir.file_name().unwrap().to_string_lossy())), "{note}");
    let parsed_dir = view::empty_project_part(&main);
    assert_eq!(parsed_dir.sources, vec![main.clone()], "a main.part path is its directory");
    assert!(view::stance_note(&parsed_dir, Some("parse error: line 1: nothing")).unwrap().starts_with("this project does not parse"));
    assert_eq!(view::stance_note(&parsed_dir, None), None);
    // Every view model holds on an empty part — nothing to draw, nothing to fit, no panic.
    // An empty part still EMITS — the header and the footer, no operation —
    // and the Program document says so ("no operations").
    assert!(view::gcode_view(&p).is_ok());
    assert!(view::feature_edges(&p).is_empty());
    let bb = view::edges_aabb(&view::feature_edges(&p), view::scene_aabb(&p.stock, &[]));
    assert!(bb.iter().all(|v| v.is_finite()));
    let _ = nacs::outline::outline(&p);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `project.new` is a keyless act the palette finds; the project text it
/// writes is `template::project_text`'s, with every default visible.
#[test]
fn test_new_project_is_a_keyless_act_and_its_text_shows_every_default() {
    let a = view::action("project.new").expect("the act exists");
    assert_eq!(a.key, "", "keyless by DESIGN.md §9");
    assert!(view::palette_matches("new project").iter().any(|a| a.id == "project.new") || view::palette_matches("project").iter().any(|a| a.id == "project.new"));
    let keys = nacs::template::project_keys("Box").unwrap();
    assert_eq!(keys.iter().map(|(k, ..)| *k).collect::<Vec<_>>(), vec!["w", "l", "h", "x", "y", "z", "units", "feed", "plunge", "spindle", "rapid_z", "clearance_z"]);
    assert_eq!(keys.iter().find(|(k, ..)| *k == "rapid_z").unwrap().2, "top + 5");
    assert!(nacs::template::project_keys("Sphere").unwrap_err().contains("Box or Cylinder"));
    let answers: Vec<(String, String)> = [("w", "100"), ("l", "60"), ("h", "12")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let text = nacs::template::project_text("Box", &answers).unwrap();
    assert!(text.contains("units mm\nfeed 800\nplunge 300\nspindle 3000\nrapid_z 5.000\nclearance_z 25.000\n\nstock Box(w=100, l=60, h=12)\n"), "{text}");
    let p = parse::parse(&text).expect("the one parser accepts what this writes");
    assert_eq!(p.global_rapid_z, 5.0);
    // The planes follow the top; `top + n` is legal on the command line too.
    let (kind, more) = nacs::template::parse_init_spec("Cylinder dia=300 h=65 z=65 units=mm rapid_z=top+10").unwrap();
    assert_eq!(kind, "Cylinder");
    let text = nacs::template::project_text(&kind, &more).unwrap();
    assert!(text.contains("rapid_z 75.000\nclearance_z 90.000\n\nstock Cylinder(dia=300, h=65, z=65)\n"), "{text}");
    parse::parse(&text).expect("parses");
    // Refusals by name.
    assert!(nacs::template::project_text("Box", &[]).unwrap_err().contains("needs w"));
    let bad: Vec<(String, String)> = vec![("w".into(), "1".into()), ("l".into(), "1".into()), ("h".into(), "1".into()), ("depth".into(), "3".into())];
    assert!(nacs::template::project_text("Box", &bad).unwrap_err().contains("`depth` is not a key"));
    let bad: Vec<(String, String)> = vec![("w".into(), "1".into()), ("l".into(), "1".into()), ("h".into(), "1".into()), ("units".into(), "cm".into())];
    assert!(nacs::template::project_text("Box", &bad).unwrap_err().contains("mm or inch"));
    assert!(nacs::template::parse_init_spec("Box w").unwrap_err().contains("not key=value"));
    assert!(nacs::template::parse_init_spec("   ").unwrap_err().contains("empty"));
}

// ---- Phase 50 slice 4: the doors (DESIGN.md §5.1, reopened) ----

/// Every door names an act the inventory has, the hover carries the key
/// where one exists, and the three strips cover what the audit found
/// keyboard-only: fit, focus, the stances, undo, redo, help, the palette.
#[test]
fn test_every_door_is_an_act_and_its_hover_names_the_key() {
    let strips: [(&str, &[(&str, &str, &str)]); 3] = [("view", view::VIEW_STRIP), ("transport", view::TRANSPORT_DOORS), ("status", view::STATUS_DOORS)];
    for (name, strip) in strips {
        // The transport strip is empty since Phase 52 slice 13: the bar is
        // the cursor's alone, and the way back lives on the status line.
        assert!(!strip.is_empty() || name == "transport", "{name}");
        for (id, label, what) in strip {
            let a = view::action(id).unwrap_or_else(|| panic!("{name}: `{id}` is not an act"));
            assert!(!label.is_empty() && !what.is_empty());
            let hover = view::door_hover(id, what);
            assert!(hover.starts_with(what), "{hover}");
            if a.key.is_empty() {
                assert_eq!(hover, *what, "a keyless act's hover is its sentence alone");
            } else {
                assert!(hover.contains(&format!("({}", view::key_words(a).trim())), "{hover} should name {}", a.key);
            }
        }
    }
    let ids: Vec<&str> = view::VIEW_STRIP.iter().map(|(id, ..)| *id).collect();
    assert_eq!(ids, vec!["view.fit", "view.focus", "view.plan", "view.front", "view.right"]);
    assert!(view::TRANSPORT_DOORS.is_empty(), "the transport bar holds the cursor and nothing else (Phase 52 slice 13)");
    assert_eq!(view::STATUS_DOORS.iter().map(|(id, ..)| *id).collect::<Vec<_>>(), vec!["edit.undo", "edit.redo", "help", "palette.open"]);
    assert_eq!(view::doors_of("cursor.block_on"), vec!["transport"], "the move grain's doors are on the transport bar");
    let fit = view::door_hover("view.fit", "fit the scene");
    assert!(fit.starts_with("fit the scene  (") && fit.contains('f') && fit.contains("Home"), "{fit}");
    assert_eq!(view::door_hover("view.plan", "look straight down"), "look straight down");
    let undo = view::door_hover("edit.undo", "undo the last write");
    assert!(undo.starts_with("undo the last write  (") && undo.contains("Ctrl+Z"), "{undo}");
}

// ---- Phase 50 slice 5: the forms speak machining (DESIGN.md §5.4) ----

/// Every word an enum menu offers is a word the parser accepts on a run
/// line, and the keys that are numbers or references offer no menu.
#[test]
fn test_enum_menus_mirror_the_parser_word_for_word() {
    let base = std::fs::read_to_string("profile_pocket_clear.cam/main.part").expect("read");
    for (key, host) in [("pattern", "Clear"), ("comp", "Side"), ("compress", "Side"), ("strategy", "Clear"), ("coolant", "Clear")] {
        let words = view::enum_options(key).unwrap_or_else(|| panic!("{key} is a menu"));
        assert!(words.len() >= 2, "{key}");
        for w in &words {
            let line = if host == "Clear" {
                format!("run Clear(tool=T5, profile=PocketProfile, stepover=4.0, {key}={w})")
            } else {
                format!("run Side(tool=T5, surface=PocketProfile, {key}={w})")
            };
            let src = base.lines().filter(|l| !l.starts_with("run ")).chain(std::iter::once(line.as_str())).collect::<Vec<_>>().join("\n");
            let r = parse::parse(&src);
            // A word the parser knows never yields "unknown … '<word>'";
            // any other refusal (a coolant the machine has no word for)
            // is the machine's business, not the menu's.
            if let Err(e) = &r {
                assert!(!e.contains(&format!("'{w}'")) || e.contains("has no word on this machine"), "{key}={w}: {e}");
            }
        }
    }
    let cycle = view::enum_options("cycle").unwrap();
    assert_eq!(cycle, vec!["Simple", "Peck"]);
    assert_eq!(view::enum_options("pattern").unwrap(), vec!["offset", "zigzag", "oneway", "spine", "held"]);
    for key in ["stepover", "tool", "surface", "dia", "extent", "feed", "n"] {
        assert!(view::enum_options(key).is_none(), "{key} is not a menu");
    }
    // The form carries them: a run line's pattern is a menu, its stepover a number.
    let p = parse::parse(&base).unwrap();
    let form = view::statement_form(&p, &view::Statement::Op(0)).unwrap();
    let pattern = form.fields.iter().find(|f| f.key == "pattern").expect("pattern is a run key");
    assert_eq!(pattern.options.as_deref(), Some(&["offset".to_string(), "zigzag".to_string(), "oneway".to_string(), "spine".to_string(), "held".to_string()][..]));
    assert!(form.fields.iter().find(|f| f.key == "stepover").unwrap().options.is_none());
    assert!(form.fields.iter().find(|f| f.key == "tool").unwrap().options.is_some(), "a reference is still a menu");
}

/// A key's unit beside its label: lengths in the file's unit, feeds per
/// minute, rpm, degrees, nothing for words and counts.
#[test]
fn test_field_labels_say_the_unit() {
    assert_eq!(view::field_label("stepover", true), "stepover (mm) =");
    assert_eq!(view::field_label("dia", false), "dia (in) =");
    assert_eq!(view::field_label("feed", true), "feed (mm/min) =");
    assert_eq!(view::field_label("plunge", false), "plunge (in/min) =");
    assert_eq!(view::field_label("spindle", true), "spindle (rpm) =");
    assert_eq!(view::field_label("chamfer_angle", true), "chamfer_angle (°) =");
    assert_eq!(view::field_label("tilt", true), "tilt (°) =");
    for key in ["pattern", "tool", "surface", "cycle", "n", "flute_count", "file", "units", "coolant"] {
        assert_eq!(view::field_label(key, true), format!("{key} ="), "{key} carries no unit");
    }
}

/// The creation menu's groups cover every variant exactly once; a new
/// feature's name is suggested from the variant and the project; a new
/// operation's stepover is suggested from the tool.
#[test]
fn test_grouped_kinds_cover_every_variant_once_and_names_are_suggested() {
    let mut seen: Vec<&str> = nacs::template::FEATURE_GROUPS.iter().flat_map(|(_, vs)| vs.iter().copied()).collect();
    let mut all: Vec<&str> = nacs::template::FEATURE_SPECS.iter().map(|s| s.variant).collect();
    seen.sort_unstable();
    all.sort_unstable();
    assert_eq!(seen, all, "every variant in exactly one group, no stranger");
    let base = std::fs::read_to_string("profile_pocket_clear.cam/main.part").expect("read");
    let p = parse::parse(&base).unwrap();
    assert_eq!(nacs::template::suggested_name(&p, "ProfilePocket"), "Pocket1");
    assert_eq!(nacs::template::suggested_name(&p, "CylinderBore"), "Hole1");
    assert_eq!(nacs::template::suggested_name(&p, "Sphere"), "Sphere1");
    let taken = base.replace("feature PocketProfile ProfilePocket(", "feature Pocket1 ProfilePocket(").replace("profile=PocketProfile", "profile=Pocket1");
    let q = parse::parse(&taken).unwrap();
    assert_eq!(nacs::template::suggested_name(&q, "ProfilePocket"), "Pocket2", "the lowest number the project does not use");
    assert_eq!(nacs::template::suggested_stepover(&p, "T5").as_deref(), Some("5"), "half a ⌀10");
    assert_eq!(nacs::template::suggested_stepover(&p, "T9"), None, "no such tool");
    assert_eq!(nacs::template::suggested_stepover(&p, "x"), None);
}

// ---- Phase 50 slice 6: block features (DESIGN.md §1, §5.4, §5.5) ----

/// A block's OPENER carries fields like any single-line statement: read
/// by `fields_of`, rewritten by `set_key`, the points below it untouched.
#[test]
fn test_a_block_opener_has_fields_and_a_key_write_leaves_its_points_alone() {
    let opener = "feature P ProfilePocket(extent=5.0";
    assert_eq!(view::fields_of(opener), vec![("extent".to_string(), "5.0".to_string())]);
    assert_eq!(view::set_key(opener, "extent", Some("8")).unwrap(), "feature P ProfilePocket(extent=8");
    assert_eq!(view::set_key(opener, "z", Some("-2")).unwrap(), "feature P ProfilePocket(extent=5.0, z=-2");
    assert_eq!(view::set_key("feature P ProfilePocket(extent=5.0 # the void", "extent", Some("8")).unwrap(), "feature P ProfilePocket(extent=8 # the void");
    assert!(view::set_key("feature P ProfilePocket(extent=5.0", "z", None).unwrap_err().contains("no `z`"));
    // Through the project: the whole write, the points as they were.
    let text = std::fs::read_to_string("profile_pocket_clear.cam/main.part").expect("read");
    let p = parse::parse(&text).unwrap();
    let (_, line, _) = p.feature_lines.iter().find(|(n, ..)| n == "PocketProfile").map(|(_, f, l, t)| (*f, *l, t.clone())).unwrap();
    let after = view::replace_line(&text, line, &view::set_key(text.lines().nth(line).unwrap(), "extent", Some("8")).unwrap()).unwrap();
    let q = parse::parse(&after).expect("still a program");
    match &q.features["PocketProfile"] {
        parse::Feature::ProfilePocket { extent, points, .. } => {
            assert_eq!(*extent, 8.0);
            assert_eq!(points.len(), 4, "the points below the opener are untouched");
        }
        other => panic!("{other:?}"),
    }
    // The form: the opener's keys, no points field, the block editor still its own thing.
    let form = view::statement_form(&p, &view::Statement::Feature("PocketProfile".to_string())).unwrap();
    assert_eq!(form.head, "ProfilePocket");
    let keys: Vec<&str> = form.fields.iter().map(|f| f.key.as_str()).collect();
    assert!(keys.contains(&"extent") && keys.contains(&"z") && !keys.contains(&"points"), "{keys:?}");
    assert_eq!(form.fields.iter().find(|f| f.key == "extent").unwrap().value.as_deref(), Some("5.0"));
    assert_eq!(form.fields.iter().find(|f| f.key == "z").unwrap().value, None, "a blank the write appends");
    // A Face's z is a field too.
    let face = std::fs::read_to_string("cottage_interior_offset.cam/main.part").expect("read");
    let fp = parse::parse(&face).unwrap();
    let ff = view::statement_form(&fp, &view::Statement::Feature("Floor".to_string())).unwrap();
    assert_eq!(ff.fields.iter().find(|f| f.key == "z").unwrap().value.as_deref(), Some("-6"));
}

/// A pocket and a boss get grips on their opener's keys at the first
/// vertex: the extent at the floor pointing down, the rim's z when the
/// line carries it; a Face its z; the points stay the file's.
#[test]
fn test_a_profile_gets_an_extent_grip_at_its_first_vertex() {
    let text = std::fs::read_to_string("profile_pocket_clear.cam/main.part").expect("read");
    let p = parse::parse(&text).unwrap();
    let edges = view::feature_edges(&p);
    let i = edges.iter().position(|e| e.name == "PocketProfile").expect("drawn");
    let hs = view::handles(&p, view::Selection::Feature(i), &[], &edges);
    let extent = hs.iter().find(|h| h.key == "extent").expect("the extent grip");
    assert_eq!(extent.at, [20.0, 20.0, -5.0], "the first vertex, at the floor");
    assert_eq!(extent.axis, [0.0, 0.0, -1.0]);
    assert_eq!(extent.value, 5.0);
    assert!(hs.iter().all(|h| h.key != "z"), "no z on the line, no z grip — the form is where one is first typed");
    let with_z = text.replace("ProfilePocket(extent=5.0", "ProfilePocket(z=-1, extent=5.0");
    let pz = parse::parse(&with_z).unwrap();
    let hz = view::handles(&pz, view::Selection::Feature(i), &[], &view::feature_edges(&pz));
    assert_eq!(hz.iter().find(|h| h.key == "z").expect("z grip").at, [20.0, 20.0, -1.0]);
    assert_eq!(hz.iter().find(|h| h.key == "extent").unwrap().at, [20.0, 20.0, -6.0], "the floor follows the rim");
}

/// A rectangle or a circle from numbers, wound the way the kind wants;
/// a typed loop the wrong way round is the form's to reverse and say.
#[test]
fn test_a_rectangle_or_circle_from_numbers_winds_as_the_kind_wants() {
    use nacs::template::{circle_points, rectangle_points, winding_ccw};
    assert_eq!(winding_ccw("ProfilePocket"), Some(true));
    assert_eq!(winding_ccw("ProfileBoss"), Some(false));
    assert_eq!(winding_ccw("Face"), None);
    assert_eq!(rectangle_points("40", "20", "30", "20", true).unwrap(), "30,20;70,20;70,40;30,40");
    assert_eq!(rectangle_points("40", "20", "30", "20", false).unwrap(), "30,40;70,40;70,20;30,20");
    assert_eq!(rectangle_points("10", "10", "", "", true).unwrap(), "0,0;10,0;10,10;0,10", "a blank corner is the origin");
    assert!(rectangle_points("", "10", "", "", true).unwrap_err().contains("w is needed"));
    assert!(rectangle_points("x", "10", "", "", true).unwrap_err().contains("not a number"));
    assert!(rectangle_points("0", "10", "", "", true).unwrap_err().contains("positive"));
    let c = circle_points("20", "50", "50", 8, true).unwrap();
    assert_eq!(c.split(';').count(), 8);
    assert!(c.starts_with("60,50;57.071,57.071;"), "{c}");
    assert_eq!(circle_points("20", "0", "0", 3, true).unwrap().split(';').count(), 8, "never fewer than eight chords");
    assert_eq!(view::reverse_points("1,2;3,4;5,6"), "5,6;3,4;1,2");
    assert_eq!(view::reverse_points(" 1,2 ; 3,4 ;"), "3,4;1,2");
    // Each lands as the parser's own loop, wound right: a pocket CCW, a boss CW.
    let header = "units mm\nfeed 800\nplunge 300\nspindle 3000\nrapid_z 5\nclearance_z 25\nstock Box(w=100, l=60, h=12)\n";
    for (variant, ccw) in [("ProfilePocket", true), ("ProfileBoss", false)] {
        let pts = rectangle_points("40", "20", "30", "20", ccw).unwrap();
        let answers = vec![("extent".to_string(), "6".to_string()), ("points".to_string(), pts)];
        let statement = nacs::template::feature_statement(variant, "Q", &answers).unwrap();
        let p = parse::parse(&format!("{header}{statement}\n")).unwrap_or_else(|e| panic!("{variant}: {e}"));
        assert!(p.features.contains_key("Q"));
        // And the wrong way round is the winding refusal the form answers by reversing.
        let wrong = vec![("extent".to_string(), "6".to_string()), ("points".to_string(), view::reverse_points(&answers[1].1))];
        let statement = nacs::template::feature_statement(variant, "Q", &wrong).unwrap();
        let e = parse::parse(&format!("{header}{statement}\n")).unwrap_err();
        assert!(e.contains("winds"), "{e}");
    }
}

// ---- Phase 50 slice 7: the checker's words (DESIGN.md §5.2) ----

/// Every family has a word on screen that is not the wire's identifier;
/// the wire's spelling is unchanged.
#[test]
fn test_every_family_has_a_display_word_and_the_wire_keeps_its_own() {
    for family in nacs::check_result::FAMILY_ORDER {
        let shown = view::family_display(family);
        let wire = nacs::check_result::family_word(family);
        assert!(!shown.is_empty() && !shown.contains('_'), "{shown}");
        assert!(wire.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "the wire's spelling stands: {wire}");
    }
    assert_eq!(view::family_display(parse::FindingFamily::BodyFoul), "Holder collision");
    assert_eq!(view::family_display(parse::FindingFamily::Decline), "Not checked");
    assert_eq!(view::family_display(parse::FindingFamily::Containment), "Outside stock");
    assert_eq!(nacs::check_result::family_word(parse::FindingFamily::BodyFoul), "body_foul");
}

/// The rows lead with the display word, a decline is counted apart and
/// said once, and the heading says both counts.
#[test]
fn test_declines_are_counted_apart_and_said_once() {
    let theme = Theme::default();
    let mk = |family: parse::FindingFamily, index: usize, message: &str| parse::Finding {
        family,
        subject: parse::FindingSubject::Op { index, feature: "Rim".to_string() },
        message: message.to_string(),
        place: None,
        at: None,
        tool: None,
        about: None,
    };
    let cr = nacs::check_result::CheckResult {
        refusal: None,
        findings: vec![
            mk(parse::FindingFamily::Containment, 0, "stands outside the stock"),
            mk(parse::FindingFamily::Decline, 1, "no closed-form removal"),
            mk(parse::FindingFamily::Decline, 1, "no closed-form removal"),
            mk(parse::FindingFamily::Decline, 2, "another"),
        ],
        emitted_moves: None,
    };
    let (rows, _) = view::finding_rows(&cr, 0.0, &theme);
    assert_eq!(rows.len(), 3, "the repeated decline is said once");
    assert!(rows[0].head.starts_with("Outside stock · op [0] 'Rim'"), "{}", rows[0].head);
    assert!(rows[1].head.starts_with("Not checked · op [1] 'Rim'"), "{}", rows[1].head);
    assert!(!rows[0].unchecked && rows[1].unchecked && rows[2].unchecked);
    assert_eq!(view::problem_counts(&rows), (1, 2));
    assert_eq!(view::problems_heading(&rows, false), "Problems (1 · 2 unchecked)");
    assert_eq!(view::problems_heading(&rows, true), "Problems (2 · 2 unchecked)");
    assert_eq!(view::problems_heading(&rows[1..], false), "Problems (2 unchecked)");
    assert_eq!(view::problems_heading(&rows[..1], false), "Problems (1)");
    assert_eq!(view::problems_heading(&[], false), "Problems");
    let slot = view::tabs().into_iter().find(|s| s.tab == SidebarTab::Problems).unwrap();
    assert_eq!(view::tab_label(&slot, 1, 2, None), "Problems (1 · 2 unchecked)");
    assert_eq!(view::tab_label(&slot, 0, 0, None), "Problems");
    assert_eq!(view::tab_label(&slot, 0, 3, None), "Problems (3 unchecked)");
    // A rejected program is the first row and a problem.
    let rejected = nacs::check_result::CheckResult { refusal: Some("error: too small".to_string()), findings: vec![], emitted_moves: None };
    let (rows, _) = view::finding_rows(&rejected, 0.0, &theme);
    assert_eq!(rows[0].head, "REJECTED");
    assert_eq!(view::problem_counts(&rows), (1, 0));
}

/// A parse error's place, read off the parser's own prefix, names the
/// file and the line the pane offers to an editor.
#[test]
fn test_a_parse_error_names_its_file_and_line_for_the_launcher() {
    assert_eq!(view::parse_error_place("line 27: unknown operation 'Clar'"), Some(("main.part", 27)));
    assert_eq!(view::parse_error_place("parse error: line 3: 'abc' is not a number"), Some(("main.part", 3)));
    assert_eq!(view::parse_error_place("Not reloaded — geometry.part: line 12: a run line belongs in main.part"), Some(("geometry.part", 12)));
    assert_eq!(view::parse_error_place("referenced tool T9 not defined (main.part line 4) — declared: T5"), None, "no prefix, no line");
    assert_eq!(view::parse_error_place("line x: nope"), None);
}

// ---- Phase 50 slice 8: the two reports in the window, and the reference ----

/// The clock line is the CLI's own numbers, said short; a declined rapid
/// total is said by name; a rejected program has no clock.
#[test]
fn test_the_clock_line_is_the_time_report_said_short() {
    assert_eq!(view::mmss(0.0), "0 s");
    assert_eq!(view::mmss(18.4), "18 s");
    assert_eq!(view::mmss(252.0), "4 m 12 s");
    assert_eq!(view::mmss(3720.0), "1 h 02 m");
    let p = parse::parse(&std::fs::read_to_string("plate.cam/main.part").unwrap().replace("main.part", "main.part")).ok();
    let src = std::fs::read_to_string("profile_pocket_clear.cam/main.part").unwrap();
    let p = p.or_else(|| parse::parse(&src).ok()).unwrap();
    let report = nacs::cut_time::time_report(&p);
    let line = view::clock_line(&report);
    let r = report.as_ref().unwrap();
    assert!(line.starts_with(&format!("cut {}", view::mmss(r.cutting_s()))), "{line}");
    if r.declined() {
        assert!(line.contains("rapids uncounted"), "{line}");
    } else {
        assert!(line.contains(&format!("total {}", view::mmss(r.cutting_s() + r.rapid_s()))), "{line}");
    }
    assert_eq!(view::clock_line(&Err("too small".to_string())), "no clock — the program is rejected");
}

/// The Uncut layer's verdict says what the scan found, in the report's words.
#[test]
fn test_the_uncut_layers_verdict_is_the_rest_reports_sentence() {
    let none = view::RestCellsView { cell: 0.5, forgotten: vec![], violated: vec![] };
    assert_eq!(view::rest_verdict(&none), "uncut (declared removed, never swept): none · gouged (swept where nothing was declared): none");
    let some = view::RestCellsView { cell: 1.0, forgotten: vec![[0.0; 3]; 12], violated: vec![[0.0; 3]] };
    assert_eq!(view::rest_verdict(&some), "uncut (declared removed, never swept): 12 cells (~12.0 mm³) · gouged (swept where nothing was declared): 1 cells (~1.0 mm³)");
}

/// The reference is generated from the tables and the checked-in file is
/// pinned to it: every variant, kind, run key and act appears, and
/// `REFERENCE.md` equals the generator byte for byte.
#[test]
fn test_the_reference_is_generated_and_the_checked_in_file_matches() {
    let text = view::reference_text();
    for spec in nacs::template::FEATURE_SPECS {
        assert!(text.contains(&format!("**{}**", spec.variant)), "{} is in the reference", spec.variant);
    }
    for spec in nacs::template::TOOL_SPECS {
        assert!(text.contains(&format!("**{}**", spec.kind)), "{} is in the reference", spec.kind);
    }
    for k in parse::RUN_KEYS {
        assert!(text.contains(&format!("- `{k}=`")), "{k} is in the reference");
    }
    for a in view::ACTIONS {
        assert!(text.contains(&format!("- `{}` — ", a.id)), "{} is in the reference", a.id);
    }
    assert!(text.contains("one of `offset`, `zigzag`, `oneway`, `spine`"));
    assert!(text.contains("- `stepover=` (mm)"));
    let file = std::fs::read_to_string("REFERENCE.md").expect("REFERENCE.md is checked in");
    assert_eq!(file.replace("\r\n", "\n"), text, "REFERENCE.md drifted — regenerate: nacs --reference > REFERENCE.md");
}

// ---- Phase 50 slice 9: the third door (DESIGN.md §5.2, §5.5, §6) ----

/// Every menu entry is an act the inventory has; each selection kind has
/// a menu; the two new moves are keyless.
#[test]
fn test_every_context_action_is_an_act() {
    let kinds = [
        view::Selection::None,
        view::Selection::Operation(0),
        view::Selection::Feature(0),
        view::Selection::Tool(1),
        view::Selection::Stock,
        view::Selection::Finding(0),
        view::Selection::Review(0),
    ];
    for sel in kinds {
        let menu = view::context_actions(sel);
        assert!(!menu.is_empty(), "{sel:?}");
        for (id, label) in menu {
            assert!(view::action(id).is_some(), "{sel:?}: `{id}` is not an act");
            assert!(!label.is_empty());
        }
    }
    let op: Vec<&str> = view::context_actions(view::Selection::Operation(2)).iter().map(|(id, _)| *id).collect();
    assert_eq!(op, vec!["form.open", "view.focus", "op.suppress", "op.move_top", "op.move_bottom", "statement.delete"]);
    // Edit… (Phase 51 slice 3) leads every menu over a thing with a form,
    // and is absent where there is none to edit.
    for sel in [view::Selection::Operation(0), view::Selection::Feature(0), view::Selection::Tool(1), view::Selection::Stock] {
        assert_eq!(view::context_actions(sel)[0], ("form.open", "Edit…"), "{sel:?}");
    }
    for sel in [view::Selection::None, view::Selection::Finding(0), view::Selection::Review(0)] {
        assert!(!view::context_actions(sel).iter().any(|(id, _)| *id == "form.open"), "{sel:?}");
    }
    assert_eq!(view::action("form.open").expect("the act exists").key, "", "keyless: the menu and the selection reach it");
    for id in ["op.move_top", "op.move_bottom"] {
        assert_eq!(view::action(id).expect("the act exists").key, "", "keyless by DESIGN.md §9");
    }
}

/// A move to the top or the bottom is the swap composed: the run order
/// changes, every other line stays, a comment between runs is not hoisted.
#[test]
fn test_move_to_top_or_bottom_is_repeated_swaps() {
    let text = "units mm\nfeed 800\nplunge 300\nspindle 3000\nrapid_z 5\nclearance_z 25\nstock Box(w=100, l=60, h=12)\ntool T1 Endmill(dia=6)\nfeature A Plane(z=-1)\nfeature B Plane(z=-2)\nfeature C Plane(z=-3)\nrun End(tool=T1, surface=A, stepover=3)\n# between one and two\nrun End(tool=T1, surface=B, stepover=3)\nrun End(tool=T1, surface=C, stepover=3)\n";
    let p = parse::parse(text).unwrap();
    assert_eq!(p.op_lines.len(), 3);
    let top = view::move_run_line(text, &p.op_lines, 2, true).unwrap();
    let q = parse::parse(&top).unwrap();
    let order: Vec<&str> = q.operations.iter().map(|o| o.feature_name.as_str()).collect();
    assert_eq!(order, vec!["C", "A", "B"]);
    assert_eq!(top.lines().nth(12), Some("# between one and two"), "the comment stays where it was");
    let bottom = view::move_run_line(text, &p.op_lines, 0, false).unwrap();
    let q = parse::parse(&bottom).unwrap();
    let order: Vec<&str> = q.operations.iter().map(|o| o.feature_name.as_str()).collect();
    assert_eq!(order, vec!["B", "C", "A"]);
    assert!(view::move_run_line(text, &p.op_lines, 0, true).unwrap_err().contains("already at the top"));
    assert!(view::move_run_line(text, &p.op_lines, 2, false).unwrap_err().contains("already at the bottom"));
    assert!(view::move_run_line(text, &p.op_lines, 9, true).unwrap_err().contains("not in the program"));
}

// ---- Phase 50 slice 10: the import act ----

/// The sidecar's command is built and judged before anything runs: the
/// interpreter where the venv puts it, the script, the STEP, the output
/// directory with its trailing separator, the up axis; a missing venv
/// names the setup; a file that is not a STEP, or an axis that is not
/// one, refuse by name.
#[test]
fn test_the_import_command_is_judged_before_it_runs() {
    use nacs::view::{import_command, is_step, sidecar_python};
    let scratch = std::env::temp_dir().join(format!("nacs_import_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    let sidecar = scratch.join("sidecar");
    std::fs::create_dir_all(&sidecar).unwrap();
    std::fs::write(sidecar.join("import_step.py"), "# stand-in").unwrap();
    let step = scratch.join("part.STP");
    std::fs::write(&step, "ISO-10303-21;").unwrap();
    let dir = scratch.join("job.cam");
    assert!(is_step(&step) && is_step(std::path::Path::new("a/b.step")) && !is_step(std::path::Path::new("a/b.part")));
    let none = view::ImportSetup::default();
    let e = import_command(&sidecar, &step, &dir, &none).unwrap_err();
    assert!(e.contains("venv is not set up") && e.contains("setup."), "{e}");
    let python = sidecar_python(&sidecar);
    assert!(python.ends_with(if cfg!(windows) { "Scripts\\python.exe" } else { "bin/python" }), "{}", python.display());
    std::fs::create_dir_all(python.parent().unwrap()).unwrap();
    std::fs::write(&python, "").unwrap();
    let (prog, args) = import_command(&sidecar, &step, &dir, &view::ImportSetup { up: Some("y+".into()), solid: String::new(), allowance: String::new(), given: String::new(), origin: String::new() }).unwrap();
    assert_eq!(prog, python);
    assert_eq!(args[0], sidecar.join("import_step.py").to_string_lossy());
    assert_eq!(args[1], step.to_string_lossy());
    assert_eq!(args[2], "-o");
    assert!(args[3].ends_with(std::path::MAIN_SEPARATOR), "the sidecar wants the directory with its separator: {}", args[3]);
    assert_eq!(&args[4..], ["--up", "y+"]);
    assert!(import_command(&sidecar, &step, &dir, &view::ImportSetup { up: Some("up".into()), solid: String::new(), allowance: String::new(), given: String::new(), origin: String::new() }).unwrap_err().contains("not an axis"));
    assert!(import_command(&sidecar, &scratch.join("x.part"), &dir, &none).unwrap_err().contains("not a STEP file"));
    assert!(import_command(&sidecar, &scratch.join("missing.step"), &dir, &none).unwrap_err().contains("cannot read"));
    // The setup's two words (ROADMAP item 1, slice 4): the solid as S2 or 2,
    // refused when it is not a solid's number; the runner's line spells both.
    let none = String::new();
    let both = view::ImportSetup { up: Some("y+".into()), solid: " S2 ".into(), allowance: none.clone(), given: String::new(), origin: String::new() };
    assert_eq!(both.args().unwrap(), ["--up", "y+", "--solid", "2"]);
    assert_eq!(view::ImportSetup { up: None, solid: "1".into(), allowance: none.clone(), given: String::new(), origin: String::new() }.args().unwrap(), ["--solid", "1"]);
    assert!(view::ImportSetup { up: None, solid: "0".into(), allowance: none.clone(), given: String::new(), origin: String::new() }.args().unwrap_err().contains("not a solid's number"));
    assert!(view::ImportSetup { up: None, solid: "two".into(), allowance: none.clone(), given: String::new(), origin: String::new() }.args().unwrap_err().contains("not a solid's number"));
    assert_eq!(view::ImportSetup::from_words(["x.step", "solid=2", "up=y+"].into_iter()), view::ImportSetup { up: Some("y+".into()), solid: "2".into(), ..Default::default() });
    let (_, args) = import_command(&sidecar, &step, &dir, &both).unwrap();
    assert_eq!(&args[4..], ["--up", "y+", "--solid", "2"]);
    // The third word (slice 11): the allowance, 1 mm unless typed, a size
    // in mm or nothing, `0` for a blank cut to size.
    assert_eq!(view::ImportSetup::default().args().unwrap(), ["--allowance", "1", "--origin", "mid,mid,max"]);
    assert_eq!(view::ImportSetup { allowance: " 0.5 ".into(), ..Default::default() }.args().unwrap()[..2], ["--allowance", "0.5"]);
    assert_eq!(view::ImportSetup::from_words(["x.step", "allowance=0"].into_iter()).args().unwrap()[..2], ["--allowance", "0"]);
    assert!(view::ImportSetup { allowance: "-1".into(), ..Default::default() }.args().unwrap_err().contains("not a size in mm"));
    assert!(view::ImportSetup { allowance: "some".into(), ..Default::default() }.args().unwrap_err().contains("not a size in mm"));
    // The fourth word (slice 12): the program before this one. Its blank
    // turned as up= says is handed to the sidecar as --blank, computed from
    // the project's own stock through the validator's turn; the allowance
    // word is not sent beside it. A quarter turn, a missing project and a
    // Profile blank are refused by name.
    let front = std::fs::canonicalize("through_plate.cam").unwrap();
    let front_s = front.to_string_lossy().to_string();
    let given = view::ImportSetup::from_words(["x.step", &format!("given={front_s}")].into_iter());
    assert_eq!(given.given, front_s);
    assert_eq!(given.args().unwrap(), Vec::<String>::new(), "the given blank carries its own allowance");
    assert_eq!(given.given_args(&dir).unwrap(), ["--given", &front_s, "--blank", "Box(w=90, l=50, h=12, x=-45, y=-25, z=12)"]);
    let turned = view::ImportSetup { up: Some("z-".into()), given: front_s.clone(), ..Default::default() };
    assert_eq!(turned.given_args(&dir).unwrap(), ["--given", &front_s, "--blank", "Box(w=90, l=50, h=12, x=-45, y=-25, z=0)"]);
    let (_, args) = import_command(&sidecar, &step, &dir, &turned).unwrap();
    assert_eq!(&args[4..], ["--up", "z-", "--given", &front_s, "--blank", "Box(w=90, l=50, h=12, x=-45, y=-25, z=0)"]);
    assert!(view::ImportSetup { up: Some("x+".into()), given: front_s.clone(), ..Default::default() }.given_args(&dir).unwrap_err().contains("quarter turn"));
    assert!(view::ImportSetup { given: "nowhere.cam".into(), ..Default::default() }.given_args(&dir).unwrap_err().contains("no such project"));
    assert!(view::ImportSetup { given: std::fs::canonicalize("stock_profile.cam").unwrap().to_string_lossy().to_string(), ..Default::default() }.given_args(&dir).unwrap_err().contains("Profile over"));
    // A relative path is found against the project's directory, as the
    // parser finds a `given from` path.
    let sibling = view::ImportSetup { given: format!("../{}", front.file_name().unwrap().to_string_lossy()), ..Default::default() };
    let args = sibling.given_args(&front.parent().unwrap().join("through_plate_back.cam")).unwrap();
    assert_eq!(args[3], "Box(w=90, l=50, h=12, x=-45, y=-25, z=12)");
    // The stock row reads a proposal carrying the given from line: the
    // statement is both lines, the head names the earlier project, and the
    // earlier project need not be in reach of this process to read the row.
    let proposal = "# the earlier program's blank turned over\nstock Cylinder(dia=120.0, h=30.0, x=0.0, y=0.0, z=0.0)\ngiven from ../front.cam up=z-\n";
    let row = view::stock_proposal(proposal, &parse::Part::default()).unwrap();
    assert_eq!(row.head, "Stock  Cylinder ⌀120 × 30, given from ../front.cam");
    assert_eq!(row.statement, "stock Cylinder(dia=120.0, h=30.0, x=0.0, y=0.0, z=0.0)\ngiven from ../front.cam up=z-");
    assert!(nacs::view::import_hint("job.cam").contains("nacs --import <model.step> job.cam"));
    let _ = std::fs::remove_dir_all(&scratch);
}

/// Duplicate proposals — the same statement with another name — point at
/// the first; `review.accept_all` is a keyless act.
#[test]
fn test_duplicate_proposals_point_at_the_first() {
    let row = |id: &str, statement: Option<&str>| view::ReviewRow {
        face: 0,
        id: id.to_string(),
        section: "Proposed".to_string(),
        head: id.to_string(),
        text: String::new(),
        statement: statement.map(String::from),
        refs: vec![],
        in_project: false,
        rejected: false,
        member_of: None,
    };
    let rows = vec![
        row("F1", Some("feature F1_Top Plane(z=65.0)")),
        row("F2", Some("feature F2_Bore CylinderBore(dia=14.0, extent=12.0, x=0.0, y=6.0)")),
        row("F3", Some("feature F3_Top Plane(z=65.0)")),
        row("F4", None),
        row("F5", Some("feature F5_Top Plane(z=65.0)")),
    ];
    assert_eq!(view::duplicate_proposals(&rows), vec![None, None, Some(0), None, Some(0)]);
    assert_eq!(view::action("review.accept_all").expect("the act exists").key, "");
}

// ---- Phase 51 slice 3: the form's doors (DESIGN.md §5.4, I14) ----

/// `ShutForm` shuts and never toggles, as `OpenForm` opens and never
/// toggles; and `tab_shows` is the rule a tab change reads: a tab shows
/// the selection its rows could have made, and nothing selected is never
/// a stale card.
#[test]
fn test_shut_form_never_toggles_and_a_tab_shows_what_its_rows_can_make() {
    let mut l = Layout::default();
    assert!(l.form.open);
    l.apply(DockAction::ShutForm);
    assert!(!l.form.open);
    l.apply(DockAction::ShutForm);
    assert!(!l.form.open, "shut stays shut");
    l.apply(DockAction::OpenForm);
    assert!(l.form.open);
    use view::Selection as S;
    for tab in SidebarTab::ALL {
        assert!(view::tab_shows(tab, S::None), "{tab:?}: nothing selected is not stale");
    }
    assert!(view::tab_shows(SidebarTab::Outline, S::Operation(0)), "the Outline's Operations group makes an operation's card");
    assert!(!view::tab_shows(SidebarTab::Problems, S::Operation(0)) && !view::tab_shows(SidebarTab::Review, S::Operation(0)));
    assert!(view::tab_shows(SidebarTab::Outline, S::Feature(0)) && !view::tab_shows(SidebarTab::Problems, S::Feature(0)));
    assert!(view::tab_shows(SidebarTab::Outline, S::Stock) && view::tab_shows(SidebarTab::Outline, S::Tool(1)));
    assert!(view::tab_shows(SidebarTab::Problems, S::Finding(0)) && !view::tab_shows(SidebarTab::Outline, S::Finding(0)));
    assert!(view::tab_shows(SidebarTab::Review, S::Review(0)) && !view::tab_shows(SidebarTab::Outline, S::Review(0)));
}

// ---- Phase 51 slice 4: the corner menu (DESIGN.md §5.1, reopened 2026-10-17) ----

/// Every menu row is an act; the job's verbs lead; each submenu the
/// contract names exists; a state row checks against the layout and a
/// verb row does not; and the "an act nobody finds" lint, extended: every
/// KEYLESS act has a door somewhere the mouse can reach — a strip, the
/// menu, or a right-click menu — so no act is palette-only.
#[test]
fn test_the_menu_is_acts_all_the_way_down_and_no_keyless_act_is_palette_only() {
    assert_eq!(view::MENU[0].0, "", "the job's verbs lead, untitled");
    let titles: Vec<&str> = view::MENU.iter().map(|(t, _)| *t).filter(|t| !t.is_empty()).collect();
    assert_eq!(titles, vec!["File", "Git", "Edit", "View", "Window", "Help"], "Git since Phase 54 slice 3");
    for (title, rows) in view::MENU {
        assert!(!rows.is_empty(), "{title}");
        for (id, label) in rows.iter() {
            assert!(view::action(id).is_some(), "{title}: `{id}` is not an act");
            assert!(!label.is_empty());
        }
    }
    let l = Layout::default();
    assert_eq!(view::menu_check("sidebar.toggle", &l), Some(true));
    assert_eq!(view::menu_check("form.toggle", &l), Some(true));
    assert_eq!(view::menu_check("sidebar.outline", &l), Some(true));
    assert_eq!(view::menu_check("sidebar.problems", &l), Some(false));
    assert_eq!(view::menu_check("center.split", &l), Some(true));
    assert_eq!(view::menu_check("document.program", &l), Some(false));
    assert_eq!(view::menu_check("layer.stock_simulation", &l), Some(false));
    assert_eq!(view::menu_check("layer.toolpaths", &l), Some(true));
    assert_eq!(view::menu_check("view.fit", &l), None, "a verb has no check");
    let mut shut = l.clone();
    shut.apply(DockAction::ToggleSidebar);
    assert_eq!(view::menu_check("sidebar.toggle", &shut), Some(false));
    assert_eq!(view::menu_check("sidebar.outline", &shut), Some(false), "a tab is not showing while the sidebar is shut");
    for a in view::ACTIONS {
        if a.key.is_empty() {
            assert!(!view::doors_of(a.id).is_empty(), "`{}` is keyless and has no door — a palette-only act is an act nobody finds", a.id);
        }
    }
    let text = view::reference_text();
    assert!(text.contains("## The menu"));
    assert!(text.contains("- Write program (.nc) — `program.write`"));
}

// ---- Phase 51 slice 5: the theme's home (DESIGN.md §5.1, §5.7) ----

/// The shipped themes are the repo's `themes/` folder, embedded: every
/// string is its file byte for byte (line endings aside), and the list is
/// the folder's listing — a file added without an embed, or an embed
/// without a file, fails here. The menu lists the built-in first, the
/// shipped, then the folder's extras; a path reads as a file; the glass
/// note speaks only across the launch-time line.
#[test]
fn test_the_shipped_themes_are_the_themes_folder_and_the_menu_lists_them() {
    let mut on_disk: Vec<String> = std::fs::read_dir("themes")
        .expect("themes/")
        .flatten()
        .filter_map(|e| e.file_name().to_str().and_then(|f| f.strip_suffix(".json")).map(str::to_string))
        .collect();
    on_disk.sort();
    let shipped: Vec<String> = theme::SHIPPED_THEMES.iter().map(|(n, _)| n.to_string()).collect();
    assert_eq!(shipped, on_disk, "the embed is the folder");
    for (name, json) in theme::SHIPPED_THEMES {
        let file = std::fs::read_to_string(format!("themes/{name}.json")).expect(name);
        assert_eq!(json.replace("\r\n", "\n"), file.replace("\r\n", "\n"), "{name} embedded byte for byte");
    }
    // The JSON reader lives behind `frontend` (serde_json); the embed and
    // the folder pin above hold in the lean leg too.
    #[cfg(feature = "frontend")]
    for (name, json) in theme::SHIPPED_THEMES {
        let t = theme::theme_from_json(json).unwrap_or_else(|e| panic!("{name}: {e}"));
        let (_, findings) = theme::apply_policy(t);
        assert!(!findings.iter().any(|f| f.grade == theme::ThemeGrade::Meaning), "{name} ships clean");
    }
    assert!(matches!(theme::theme_source("themes/nacs-light.json"), Some(theme::ThemeSource::File(_))), "a path is a file");
    match theme::theme_source("tokyo-night") {
        Some(theme::ThemeSource::Shipped(n, _)) => assert_eq!(n, "tokyo-night"),
        Some(theme::ThemeSource::File(p)) => assert!(p.is_file(), "a folder copy wins only when it exists: {}", p.display()),
        None => panic!("a shipped name always resolves"),
    }
    match theme::theme_source("no-such-theme") {
        Some(theme::ThemeSource::File(p)) => assert!(p.ends_with("no-such-theme.json"), "{}", p.display()),
        other => panic!("a bare name found nowhere names the file it would have been: {other:?}"),
    }
    let rows = view::theme_menu(&["a", "b"], &["b".to_string(), "c".to_string()], Some("b"));
    let labels: Vec<&str> = rows.iter().map(|(l, _, _)| l.as_str()).collect();
    assert_eq!(labels, vec!["Default (built-in)", "a", "b", "c"], "built-in, shipped, the folder's extras once");
    assert_eq!(rows.iter().filter(|(_, _, on)| *on).count(), 1);
    assert!(rows[2].2 && rows[2].1.as_deref() == Some("b"));
    assert!(rows[0].1.is_none());
    use theme::BackgroundAppearance::*;
    assert_eq!(view::theme_pick_note(false, Opaque), None);
    assert_eq!(view::theme_pick_note(true, Blurred), None);
    assert!(view::theme_pick_note(false, Blurred).unwrap().contains("next launch"));
    assert!(view::theme_pick_note(true, Opaque).unwrap().contains("next launch"));
    assert!(view::reference_text().contains("**Window ▸ Theme**"));
}

/// Phase 56 slice 4 (fork 4A): a main.part from a proposed stock — the
/// template's header with its mm defaults read against the proposal's
/// top (the planes above the model's blank), then the proposal's lines as
/// the sidecar wrote them; the parser takes the text; `project_text`'s
/// own header is byte-identical to what it rendered before the header
/// moved into its own home.
#[test]
fn test_a_project_is_made_from_a_proposed_stock() {
    use nacs::template;
    let proposal = "# Proposed stock from l_plate.step by sidecar/import_stock.py\nstock Profile(h=20.0, z=20.0\n    0.0 0.0\n    120.0 0.0\n    120.0 80.0\n    60.0 80.0\n    60.0 40.0\n    0.0 40.0\n)\n";
    let text = template::project_text_from_proposal(proposal).expect("a project from the proposal");
    assert!(text.starts_with("# written by nacs --init / the window's New project"), "{text}");
    assert!(text.contains("\nunits mm\nfeed 800\nplunge 300\nspindle 3000\nrapid_z 25.000\nclearance_z 45.000\n\n# Proposed stock"), "the planes stand above the model's top of 20: {text}");
    assert!(text.ends_with("    0.0 40.0\n)\n"), "the proposal's lines as written: {text}");
    let part = nacs::parse::parse(&text).expect("the parser takes it");
    assert_eq!((part.stock.w, part.stock.l, part.stock.h), (120.0, 80.0, 20.0));
    assert!(template::project_text_from_proposal("# only a comment\n").unwrap_err().contains("no stock statement"));
    assert!(template::project_text_from_proposal("units mm\n").unwrap_err().contains("not a stock"));
    // No `z=`: the top is 0, the planes the template's own.
    let flat = template::project_text_from_proposal("stock Box(w=10, l=10, h=5)\n").unwrap();
    assert!(flat.contains("rapid_z 5.000\nclearance_z 25.000\n\nstock Box(w=10, l=10, h=5)\n"), "{flat}");
    // The one header: `project_text` renders exactly what it did.
    let boxed = template::project_text("Box", &[("w".to_string(), "100".to_string()), ("l".to_string(), "60".to_string()), ("h".to_string(), "12".to_string())]).unwrap();
    assert_eq!(boxed, "# written by nacs --init / the window's New project: every value here was shown before it was written — change any of them.\nunits mm\nfeed 800\nplunge 300\nspindle 3000\nrapid_z 5.000\nclearance_z 25.000\n\nstock Box(w=100, l=60, h=12)\n");
}

/// Phase 57 slice 1 (forks 0A, 1A, 3A): the window's own frame. The
/// caption acts are acts with the band as their door and a row on the
/// Window menu; the maximize glyph says restore while maximized; the frame
/// word rides the workspace file only when it is the system's; the frame's
/// sentence says whose frame and what the platform did.
#[test]
fn test_the_windows_own_frame_is_acts_a_word_and_a_sentence() {
    for (id, glyph, _) in view::CAPTION_DOORS {
        let a = view::action(id).unwrap_or_else(|| panic!("{id} is an act"));
        assert!(a.key.is_empty(), "{id} is keyless: the band and the menu are its doors");
        let doors = view::doors_of(id);
        assert!(doors.contains(&"band") && doors.contains(&"menu"), "{id}'s doors: {doors:?}");
        assert!(!glyph.is_empty());
    }
    assert!(view::doors_of("window.frame").contains(&"menu"), "the switch is on the Window menu");
    assert_eq!(view::caption_glyph("window.maximize", false), "🗖");
    assert_eq!(view::caption_glyph("window.maximize", true), "🗗", "maximized: the glyph says restore");
    assert_eq!(view::caption_glyph("window.close", true), "🗙");
    assert_eq!(view::caption_glyph("window.minimize", false), "🗕");
    let mut ws = theme::Workspace::default();
    assert!(!ws.render().contains("frame "), "the window's own frame is the default and is not written");
    ws.layout.frame_native = true;
    assert!(ws.render().contains("\nframe native\n"));
    assert_eq!(theme::Workspace::parse(&ws.render()), ws, "the word round-trips");
    assert!(!theme::Workspace::parse("frame own\n").layout.frame_native);
    assert!(view::frame_sentence(true, &view::FrameDwm::NotAsked).starts_with("Frame: the system's"));
    assert_eq!(view::frame_sentence(false, &view::FrameDwm::Applied { border: [0x36, 0x3a, 0x4f] }), "Frame: the window's own — the band is the title bar; corners rounded, border #363a4f (Windows 11 DWM)");
    assert!(view::frame_sentence(false, &view::FrameDwm::Refused("x".into())).contains("refused by the platform — x"));
    assert!(view::frame_sentence(false, &view::FrameDwm::NotOnThisPlatform).contains("no corners or border"));
}

/// Phase 57 slice 2 (fork 2A): the edges. The one hit test names the edge
/// under a point — corners first, then sides, nothing inside the margin
/// or outside the window — and the eight rects the shell registers agree
/// with it everywhere they stand, the corners' squares taking the two
/// sides they end.
#[test]
fn test_the_window_edges_are_eight_zones_one_hit_test_agrees_with() {
    use view::{resize_edge, Edge, RESIZE_MARGIN};
    let r = [100.0, 50.0, 700.0, 450.0];
    let m = RESIZE_MARGIN;
    assert_eq!(resize_edge([400.0, 250.0], r, m), None, "inside");
    assert_eq!(resize_edge([99.0, 250.0], r, m), None, "outside");
    assert_eq!(resize_edge([100.0, 250.0], r, m), Some(Edge::West));
    assert_eq!(resize_edge([700.0, 250.0], r, m), Some(Edge::East));
    assert_eq!(resize_edge([400.0, 50.0], r, m), Some(Edge::North));
    assert_eq!(resize_edge([400.0, 450.0], r, m), Some(Edge::South));
    assert_eq!(resize_edge([100.0, 50.0], r, m), Some(Edge::NorthWest));
    assert_eq!(resize_edge([700.0, 50.0], r, m), Some(Edge::NorthEast));
    assert_eq!(resize_edge([100.0, 450.0], r, m), Some(Edge::SouthWest));
    assert_eq!(resize_edge([700.0, 450.0], r, m), Some(Edge::SouthEast));
    assert_eq!(resize_edge([100.0 + m, 50.0 + m], r, m), Some(Edge::NorthWest), "the margin's last point is still the edge");
    assert_eq!(resize_edge([100.0 + m + 0.5, 250.0], r, m), None, "one past the margin is inside");
    // The rects agree with the hit test: every point in an edge's rect
    // names that edge, with a corner's square beating the sides it ends
    // (the shell registers the corners after the sides, so they win).
    for edge in Edge::ALL {
        let [x0, y0, x1, y1] = edge.rect(r, m);
        assert!(x0 < x1 && y0 < y1, "{edge:?} has a rect");
        for (x, y) in [(x0, y0), (x1, y1), ((x0 + x1) / 2.0, (y0 + y1) / 2.0)] {
            let hit = resize_edge([x, y], r, m).unwrap_or_else(|| panic!("{edge:?} rect's point ({x}, {y}) is an edge"));
            let corner = matches!(hit, Edge::NorthEast | Edge::NorthWest | Edge::SouthEast | Edge::SouthWest);
            assert!(hit == edge || corner, "{edge:?} rect's point ({x}, {y}) hits {hit:?}");
        }
    }
}

/// Phase 59 nudge 2: Output's line's history — ↑ walks back from the
/// newest, ↓ forward and past the newest the draft comes back; an empty
/// line and a repeat of the last are not remembered; a new line ends a
/// walk; the memory is bounded.
#[test]
fn test_output_line_history_walks_back_and_forward_and_keeps_the_draft() {
    let mut h = view::LineHistory::default();
    assert_eq!(h.back("x"), None, "nothing to recall yet");
    assert_eq!(h.forward(), None);
    h.remember("sh echo one");
    h.remember("  ");
    h.remember("sh echo two");
    h.remember("sh echo two");
    assert_eq!(h.len(), 2, "an empty line and a repeat of the last are not remembered");
    assert_eq!(h.back("sh e").as_deref(), Some("sh echo two"));
    assert_eq!(h.back("").as_deref(), Some("sh echo one"));
    assert_eq!(h.back(""), None, "the oldest stands");
    assert_eq!(h.forward().as_deref(), Some("sh echo two"));
    assert_eq!(h.forward().as_deref(), Some("sh e"), "past the newest the draft comes back");
    assert_eq!(h.forward(), None, "and no walk stands");
    h.remember("git status");
    assert_eq!(h.back("").as_deref(), Some("git status"));
    h.remember("sh dir");
    assert_eq!(h.forward(), None, "a new line ends the walk");
    assert_eq!(h.back("").as_deref(), Some("sh dir"));
    let mut h = view::LineHistory::default();
    for i in 0..(view::HISTORY_CAP + 5) {
        h.remember(&format!("sh echo {i}"));
    }
    assert_eq!(h.len(), view::HISTORY_CAP, "bounded");
    assert_eq!(h.back("").as_deref(), Some(format!("sh echo {}", view::HISTORY_CAP + 4).as_str()), "the newest kept");
}

// ---- Phase 61 slice 3: a composed operation lands after the cursor ----------
// The user, 2026-10-25: "new operations are to land after the cursor." The
// cursor is the op cursor — how many operations have run — so position `k` is
// straight AFTER operation `k − 1`'s run line; at the end it is the append it
// always was.

#[test]
fn test_a_run_line_lands_after_the_cursor_and_the_program_reads_it_there() {
    let text = std::fs::read_to_string("plate.cam/main.part").unwrap();
    let geom = std::fs::read_to_string("plate.cam/geometry.part").unwrap();
    let part = nacs::parse::parse_project(&text, Some(&geom)).unwrap();
    assert!(part.operations.len() >= 3, "plate.cam runs End, Clear, Side");
    let line = "run Side(tool=T5, surface=Notch, comp=control)";
    // After the first operation: second in the run order, the comments that
    // introduce the old second operation still above IT.
    let after = view::insert_run_line_at(&text, &part.op_lines, 1, line);
    let again = nacs::parse::parse_project(&after, Some(&geom)).unwrap();
    assert_eq!(again.operations.len(), part.operations.len() + 1);
    assert_eq!(again.operations[1].op_type, nacs::parse::OpType::Side, "landed at position 1");
    assert_eq!(again.operations[0].op_type, part.operations[0].op_type);
    assert_eq!(again.operations[2].op_type, part.operations[1].op_type, "the old second is now third");
    // At zero: before everything.
    let top = view::insert_run_line_at(&text, &part.op_lines, 0, line);
    let top = nacs::parse::parse_project(&top, Some(&geom)).unwrap();
    assert_eq!(top.operations[0].op_type, nacs::parse::OpType::Side);
    // At the end — the cursor's default — it is the append.
    let end = view::insert_run_line_at(&text, &part.op_lines, part.operations.len(), line);
    assert_eq!(end, view::append_statement(&text, line));
    // A cursor past the end is the same.
    assert_eq!(view::insert_run_line_at(&text, &part.op_lines, 99, line), end);
}

#[test]
fn test_a_run_line_lands_directly_below_the_previous_run_line_not_below_its_comments() {
    let text = "a\nrun One\n# about two\nrun Two\n";
    let op_lines = [(0usize, 1usize), (0, 3)];
    let after = view::insert_run_line_at(text, &op_lines, 1, "run New");
    assert_eq!(after, "a\nrun One\nrun New\n# about two\nrun Two\n", "the comment stays with the operation it introduces");
    assert_eq!(view::insert_run_line_at(text, &op_lines, 0, "run New"), "a\nrun New\nrun One\n# about two\nrun Two\n");
}

// ---- Phase 61 slice 3: Clear around it… ---------------------------------------

#[test]
fn test_clear_around_proposes_the_plane_at_a_hostless_bosss_base_and_refuses_the_rest() {
    let load = |dir: &str| nacs::parse::load_project(dir).unwrap();
    // A boss standing in the blank with nothing declared around it.
    let p = load("cylinder_boss.cam");
    let boss = p.features.iter().find(|(_, f)| matches!(f, nacs::parse::Feature::CylinderBoss { .. })).map(|(n, _)| n.clone()).expect("a boss");
    let (name, z) = view::clear_around_plane(&p, &boss).expect("a hostless boss gets a proposal");
    assert_eq!(name, "Floor1", "the door's Plane is a floor by construction — not the skim the form guesses a project's first Plane to be (Phase 61 nudge 1)");
    assert!(z < p.stock.z, "the base is below the stock top: {z}");
    // A boss that already stands in a host is its island: refused by name.
    let p = load("plate_floated.cam");
    let err = view::clear_around_plane(&p, "Plate").expect_err("Plate stands in Floor");
    assert!(err.contains("already stands in 'Floor'"), "{err}");
    // Not a boss at all.
    let err = view::clear_around_plane(&p, "Floor").expect_err("a Plane is not a boss");
    assert!(err.contains("is not a boss standing in the blank"), "{err}");
    // The act is in the inventory and the menu carries it on a feature.
    assert!(view::action("feature.clear_around").is_some());
    assert!(view::context_actions(view::Selection::Feature(0)).iter().any(|(id, l)| *id == "feature.clear_around" && *l == "Clear around it…"));
}

#[test]
fn test_a_second_plane_is_suggested_as_a_floor_and_the_first_as_the_skim() {
    let p = nacs::parse::load_project("plate_floated.cam").unwrap();
    assert!(p.features.values().any(|f| matches!(f, nacs::parse::Feature::Plane { .. })));
    assert_eq!(nacs::template::suggested_name(&p, "Plane"), "Floor1");
    let empty = nacs::parse::parse("units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\nstock Box(w=10, l=10, h=5, x=0, y=0, z=0)\n").unwrap();
    assert_eq!(nacs::template::suggested_name(&empty, "Plane"), "Top1");
}

/// Every single-line noun has grips, no two grips of one selection share
/// a place on screen at any of the four stances, and a centre's grips sit
/// at the feature's reach along the negative axes (Phase 43 slice 2 — the
/// first hand found `x`, `y` and `z` on one point, with the first pushed
/// winning every tie, and the elliptic pair with no grips at all).
#[test]
fn test_no_two_grips_share_a_place_and_every_single_line_noun_has_them() {
    let p = parse::parse_project_with_base(
        "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
         stock Box(w=200, l=160, h=40, x=0, y=0, z=0)\ntool T5 Endmill(dia=10.0)\n\
         feature Hole CylinderBore(dia=12, extent=10, x=20, y=20)\n\
         feature Post CylinderBoss(dia=12, extent=10, x=60, y=20, z=-20)\n\
         feature Ball Sphere(dia=20, x=100, y=20, z=-20)\n\
         feature Peak Cone(dia=20, height=15, x=140, y=20, z=-20)\n\
         feature Ring Torus(major_dia=30, minor_dia=8, x=180, y=20, z=-20)\n\
         feature Drain TiltedBore(dia=12, extent=10, x=175, y=130, tilt=20, heading=90)\n",
        None,
        None,
    )
    .unwrap();
    let edges = view::feature_edges(&p);
    let bb = view::edges_aabb(&edges, view::stock_aabb(&p.stock));
    let mut stances = vec![view::Camera::default_orbit()];
    for s in [view::Stance::Plan, view::Stance::Front, view::Stance::Right] {
        let mut c = view::Camera::default_orbit();
        c.set_stance(s);
        stances.push(c);
    }
    for cam in &mut stances {
        cam.fit_to_aabb(bb, 1200.0, 800.0);
    }
    for (i, e) in edges.iter().enumerate() {
        let hs = view::handles(&p, view::Selection::Feature(i), &[], &edges);
        assert!(!hs.is_empty(), "{} has grips", e.name);
        let keys: Vec<&str> = hs.iter().map(|h| h.key.as_str()).collect();
        assert!(keys.contains(&"x") && keys.contains(&"y"), "{}: a centre has x and y grips: {keys:?}", e.name);
        assert!(!keys.iter().any(|k| ["rotation", "tilt", "heading"].contains(k)), "{}: an angle has no grip: {keys:?}", e.name);
        for cam in &stances {
            // A grip whose axis looks at the viewer is not drawn in that
            // stance (it could not be dragged), so only the drawn ones
            // are held apart.
            let pts: Vec<(&str, (f64, f64))> = hs.iter().filter_map(|h| view::handle_view_point(h, cam).map(|q| (h.key.as_str(), q))).collect();
            for a in 0..pts.len() {
                for b in (a + 1)..pts.len() {
                    let d = (pts[a].1 .0 - pts[b].1 .0).hypot(pts[a].1 .1 - pts[b].1 .1);
                    assert!(d > 8.0, "{}: {} and {} are {d:.1}px apart at yaw {:.2} pitch {:.2}", e.name, pts[a].0, pts[b].0, cam.yaw, cam.pitch);
                }
            }
        }
        // On the plan every grip but a vertical one is drawn; on the Right
        // stance an X-axis grip is not.
        assert!(hs.iter().filter(|h| h.axis[2] == 0.0).all(|h| view::handle_view_point(h, &stances[1]).is_some()), "{}", e.name);
        assert!(hs.iter().filter(|h| h.axis == [1.0, 0.0, 0.0]).all(|h| view::handle_view_point(h, &stances[3]).is_none()), "{}", e.name);
    }
    // A centre's grips: `x` on the rim at −X, `y` on the rim at −Y, at the
    // centre's height; the pick lands on each where it is drawn.
    let hole = edges.iter().position(|e| e.name == "Hole").unwrap();
    let hs = view::handles(&p, view::Selection::Feature(hole), &[], &edges);
    let x = hs.iter().find(|h| h.key == "x").unwrap();
    let y = hs.iter().find(|h| h.key == "y").unwrap();
    assert!((x.at[0] - 14.0).abs() < 1e-6 && x.at[1] == 20.0 && x.at[2] == 0.0, "{x:?}");
    assert!((y.at[1] - 14.0).abs() < 1e-6 && y.at[0] == 20.0 && y.at[2] == 0.0, "{y:?}");
    let plan = &stances[1];
    let lens = plan.lens();
    for h in [x, y] {
        let (gx, gy) = view::handle_view_point(h, plan).unwrap();
        assert_eq!(view::pick_handle(&hs, plan, gx, gy, 8.0).map(|k| hs[k].key.as_str()), Some(h.key.as_str()), "{} is grabbable", h.key);
    }
    // A grip is drawn at its point's image.
    let dia = hs.iter().find(|h| h.key == "dia").unwrap();
    let (dx, dy, _) = lens.project(dia.at);
    assert_eq!(view::handle_view_point(dia, plan), Some((f64::from(dx), f64::from(dy))));
    // The boss: its diameter on the rim at its BASE, its extent at the top.
    let post = edges.iter().position(|e| e.name == "Post").unwrap();
    let hs = view::handles(&p, view::Selection::Feature(post), &[], &edges);
    let dia = hs.iter().find(|h| h.key == "dia").unwrap();
    let ext = hs.iter().find(|h| h.key == "extent").unwrap();
    assert_eq!((dia.at, ext.at, ext.axis), ([66.0, 20.0, -20.0], [66.0, 20.0, -10.0], [0.0, 0.0, 1.0]));
}

/// The Problems line carries the check's account (ROADMAP item 2, slice 7):
/// the same numbers `--check` prints, so the empty pane says what ran.
#[test]
fn test_the_problems_line_says_how_many_checks_ran_and_could_not() {
    let p = parse::load_project("plate.cam").unwrap();
    let v = check_result::check_result(&p);
    let acc = check_result::account(&p, &v);
    let (ran, _, declined) = check_result::account_counts(&acc);
    assert!(ran >= 5 && declined == 0, "{ran} ran, {declined} could not");
    assert_eq!(view::problems_line(&[], false, &acc), format!("no problems · {ran} checks ran, 0 could not"));
    assert_eq!(view::problems_line(&[], true, &acc), format!("1 problem · {ran} checks ran, 0 could not"));
    // The rim wire's interior is undeclared: the entry restriction declines twice.
    let p = parse::load_project("contour_pocket.cam").unwrap();
    let v = check_result::check_result(&p);
    let acc = check_result::account(&p, &v);
    let theme = theme::Theme::default();
    let (rows, _) = view::finding_rows(&v, p.stock.z, &theme);
    assert!(view::problems_line(&rows, false, &acc).ends_with(" could not") && view::problems_line(&rows, false, &acc).contains(", 2 could not"), "{}", view::problems_line(&rows, false, &acc));
}

/// The Problems pane's own words: a row carries the gist of a long
/// sentence, cut at a word, and the whole of a short one; the account
/// names each family as the rows do; a finding's mark in the viewport
/// wears the row's word, not the wire's.
#[test]
fn test_the_problems_pane_speaks_the_rows_words() {
    assert_eq!(view::finding_gist("short and whole"), "short and whole");
    let long = "T10's body 8.156mm above the tip sweeps to r 16.000mm and reaches material that outlives 'Vortex' at (66.000, 40.000, -9.844) — the tip is on the surface, the rest of the tool is not in air; remove that stock in an earlier op";
    let gist = view::finding_gist(long);
    assert!(gist.ends_with('…') && gist.chars().count() <= 151 && long.starts_with(gist.trim_end_matches('…')), "{gist}");
    assert!(!gist.trim_end_matches('…').ends_with(' '), "cut at a word: {gist:?}");
    let p = load("spline_funnel_holder.cam");
    let cr = nacs::check_result::check_result(&p);
    let (summary, lines) = view::account_lines(&nacs::check_result::account(&p, &cr));
    assert_eq!(summary, "6 checks ran, 4 had nothing to judge");
    assert!(lines.contains(&"Holder collision: ran — 1 finding".to_string()), "{lines:?}");
    assert!(lines.iter().all(|l| !l.starts_with("body_foul") && !l.starts_with("field_body_foul")), "{lines:?}");
    let (rows, marks) = view::finding_rows(&cr, p.stock.z, &nacs::theme::Theme::default());
    let row = rows.iter().find(|r| r.head.starts_with("Holder collision")).expect("the body foul's row");
    assert_eq!(marks[row.annotation.expect("it names a place")].label, "Holder collision");
    assert!(rows.iter().all(|r| !r.text.contains("ROADMAP") && !r.text.contains("Advisory: `entry gate` rejects")), "a finding is not a note to self");
}

/// The palette lists acts by a short label, one row each: every act has
/// one that fits the column, no two acts share one, and a row is the
/// label and then the key. The sentence is the row's hover.
#[test]
fn test_every_act_has_a_short_label_for_the_palette() {
    let mut seen: Vec<&str> = Vec::new();
    for a in view::ACTIONS {
        let label = view::action_label(a);
        assert!(!label.is_empty() && label.chars().count() <= view::PALETTE_LABEL_W, "{}: {label:?} fits the column", a.id);
        assert!(!seen.contains(&label), "{}: {label:?} names one act", a.id);
        seen.push(label);
    }
    let help = view::action("help").unwrap();
    assert_eq!(view::palette_row(help), format!("{:<28}  ?", "Help"));
    assert_eq!(view::palette_matches("clear output").first().map(|a| a.id), Some("output.clear"), "the label is searched");
}

/// The job guide: the first step not done is the next one and carries the
/// act that moves it on; an empty folder starts at the model; a project
/// whose holes have no drill stops at Tools and names it.
#[test]
fn the_job_steps_name_the_next_thing_to_do() {
    use nacs::view::{job_steps, JobFacts, StepState};
    let empty = nacs::view::empty_project_part(std::path::Path::new("nowhere.cam"));
    let none = JobFacts { refusal: Some("no main.part yet".to_string()), ..JobFacts::default() };
    let steps = job_steps(&empty, &none);
    assert_eq!(steps.len(), 7);
    assert_eq!((steps[0].name, steps[0].state), ("Model", StepState::Next));
    assert_eq!(steps[0].act.as_ref().map(|a| a.1), Some("import.step"));
    assert!(steps[1..].iter().all(|s| s.state == StepState::Later && s.act.is_none()), "one next step, the rest wait");

    // A model imported, its stock and three features still in Review.
    let steps = job_steps(&empty, &JobFacts { model: Some("m.step".to_string()), stock_waiting: true, waiting: 3, ..none.clone() });
    assert_eq!(steps[0].state, StepState::Done);
    assert_eq!((steps[2].state, steps[2].act.as_ref().map(|a| a.1)), (StepState::Next, Some("review.accept")));
    assert_eq!((steps[1].name, steps[1].state, steps[1].act.as_ref().map(|a| a.1)), ("Origin", StepState::Done, Some("import.step")), "the origin can still be changed");

    // plate.cam is a finished project: only the program is left to write.
    let plate = nacs::parse::load_project("plate.cam").unwrap();
    let on_disk = JobFacts { has_file: true, ..JobFacts::default() };
    let steps = job_steps(&plate, &on_disk);
    assert!(steps[..6].iter().all(|s| s.state == StepState::Done), "{steps:?}");
    assert_eq!(steps[6].act.as_ref().map(|a| a.1), Some("program.write"));
    // Written, every step is done.
    let steps = job_steps(&plate, &JobFacts { program_file: Some("plate.nc".to_string()), ..on_disk.clone() });
    assert!(steps.iter().all(|s| s.state == StepState::Done), "{steps:?}");

    // A hole with no drill keeps Tools open and says which tool; a chamfer
    // with no chamfer mill does not, and is named on the done step.
    let notes = vec![
        "H1 waits for a tool: no Drill(dia=3) and no Endmill under 3 is declared".to_string(),
        "H1's chamfer waits for a ChamferMill(angle=90): none is declared".to_string(),
    ];
    let steps = job_steps(&plate, &JobFacts { notes: notes.clone(), ..on_disk.clone() });
    assert_eq!((steps[4].name, steps[4].state, steps[4].act.as_ref().map(|a| a.1)), ("Tools", StepState::Next, Some("tool.new")));
    assert!(steps[4].says.contains("Drill(dia=3) or Endmill under 3"), "{}", steps[4].says);
    let steps = job_steps(&plate, &JobFacts { notes: notes[1..].to_vec(), ..on_disk.clone() });
    assert_eq!((steps[4].state, steps[4].act.as_ref().map(|a| a.1)), (StepState::Done, Some("tool.new")), "done, and the tool can still be added");
    assert!(steps[4].says.contains("ChamferMill(angle=90)"), "{}", steps[4].says);

    // With that tool declared the scheduler has a line to add, so
    // Operations opens again.
    let steps = job_steps(&plate, &JobFacts { to_schedule: 1, ..on_disk });
    assert_eq!((steps[5].name, steps[5].state, steps[5].act.as_ref().map(|a| a.1)), ("Operations", StepState::Next, Some("schedule")));
}

#[test]
fn a_proposal_is_drawn_before_it_is_accepted() {
    let stock = "# Proposed stock\nstock Box(w=100.0, l=80.0, h=28.0, x=0.0, y=0.0, z=28.0)\n";
    let hole = "feature H1 CylinderBore(dia=6.0, extent=10.0, x=20, y=20, z=28.0)".to_string();
    let bad = "feature Nope Nothing(dia=1)".to_string();
    let p = nacs::view::proposal_preview(stock, &[hole, bad], true).expect("the proposed blank parses");
    assert_eq!(p.stock_bb, [0.0, 0.0, 0.0, 100.0, 80.0, 28.0]);
    // The statement the parser refuses is left out; the other is drawn.
    assert_eq!(p.edges.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), vec!["H1"]);
    assert!(!p.edges[0].polylines.is_empty());
    assert!(nacs::view::proposal_preview("not a stock", &[], true).is_none());
    // No path is no project: nothing to watch, and the window says so.
    let none = nacs::view::empty_project_part(std::path::Path::new(""));
    assert!(none.sources.is_empty());
    assert!(nacs::view::stance_note(&none, Some(nacs::view::NO_PROJECT)).is_some_and(|n| n.starts_with("No project open")));
    assert_eq!(nacs::view::home_for_step(std::path::Path::new("a/bracket.step")), std::path::Path::new("a/bracket.cam"));
}

#[test]
fn an_allowance_can_give_each_side_of_the_blank_its_own() {
    use nacs::view::{allowance_from, allowance_parts, allowance_sides, ImportSetup};
    // top, bottom, x-, x+, y-, y+: the number is the sides and the top, never the bottom.
    assert_eq!(allowance_sides("1").unwrap(), [1.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
    assert_eq!(allowance_sides(" 1, bottom=3 ,x+=10").unwrap(), [1.0, 3.0, 1.0, 10.0, 1.0, 1.0]);
    assert_eq!(allowance_sides("top=2").unwrap(), [2.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    for bad in ["1,side=2", "1,2", "1,x+=-1", "1,x+=2,x+=3", "x+"] {
        assert!(allowance_sides(bad).unwrap_err().contains("not a size in mm"), "{bad}");
    }
    // The window's seven fields and the one spelling are the same thing.
    let (base, own) = allowance_parts("1,bottom=3,x+=10");
    assert_eq!((base.as_str(), own[1].as_str(), own[3].as_str(), own[0].as_str()), ("1", "3", "10", ""));
    assert_eq!(allowance_from(&base, &own), "1,bottom=3,x+=10");
    let args = ImportSetup { allowance: " 1 , x-=4".to_string(), ..Default::default() }.args().unwrap();
    assert!(args.windows(2).any(|w| w == ["--allowance", "1,x-=4"]), "{args:?}");
    assert!(ImportSetup { allowance: "1,left=4".to_string(), ..Default::default() }.args().is_err());
}

#[test]
fn an_imports_origin_is_three_words_and_a_sentence() {
    use nacs::view::{origin_says, origin_words, ImportSetup};
    // Unless told, the stock's center and its top.
    let args = ImportSetup::default().args().unwrap();
    assert!(args.windows(2).any(|w| w == ["--origin", "mid,mid,max"]), "{args:?}");
    assert_eq!(origin_says("mid,mid,max"), "X0 Y0 at the stock's center, Z0 at the stock's top");
    assert_eq!(origin_says("model,model,model"), "X0 Y0 Z0 as modeled");
    // Any point can be typed: a number is a model coordinate.
    assert_eq!(origin_words("12.5, part-min ,max").unwrap(), ["12.5", "part-min", "max"]);
    assert_eq!(origin_says("12.5,part-min,part-max"), "X0 Y0 at x 12.5, y part-min, Z0 at the part's top");
    assert!(origin_words("mid,mid").is_err() && origin_words("mid,mid,up").is_err());
    assert!(ImportSetup { origin: "mid,top,max".to_string(), ..Default::default() }.args().is_err());
    // A second setup keeps the origin its given blank is written in.
    let second = ImportSetup::from_words(["origin=min,min,max", "given=../front.cam"].into_iter());
    assert_eq!(second.origin, "min,min,max");
    assert!(!second.args().unwrap().iter().any(|a| a == "--origin"));
}
