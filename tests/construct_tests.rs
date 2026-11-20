// ---- boundary construction against projected references (Phase 41 slice 3) --
//
// `DECISIONS.md` → "Phase 41 slice 3…". The circle through two points tangent
// to a line or a circle is Apollonius' point-point-curve case, a quadratic
// along the points' bisector — closed form, every solution returned. These
// pins hold the hand-checkable cases: a line at a known distance (the circle
// through two points on a horizontal chord tangent to a horizontal line is
// the one whose radius is the line's height), a circle touched externally and
// internally, the arc's points chorded at the one tolerance and ending on the
// two points given, the footprints a reference is read from, the pushed-out
// reference, and the terminal's voice.

use nacs::construct::{self, Arc, Reference};
use nacs::parse::{self, Pt};
use nacs::CHORD_TOL_MM;

fn pt(x: f64, y: f64) -> Pt {
    Pt { x, y }
}

fn load(dir: &str) -> parse::Part {
    let main_src = std::fs::read_to_string(format!("{dir}/main.part")).unwrap_or_else(|_| panic!("read {dir}/main.part"));
    let geom = std::fs::read_to_string(format!("{dir}/geometry.part")).ok();
    parse::parse_project_with_base(&main_src, geom.as_deref(), Some(std::path::Path::new(dir))).unwrap_or_else(|e| panic!("parse {dir}: {e}"))
}

fn on_circle(a: &Arc, p: Pt) -> bool {
    ((p.x - a.cx).hypot(p.y - a.cy) - a.r).abs() < 1e-9
}

/// Two points on y = 0 at x = ±3, tangent to the line y = 5: the centre is
/// on x = 0 at height c with c² + 9 = (5 − c)² → c = 1.6, R = 3.4 — and the
/// mirror below, tangent from the other side is NOT a solution to this line
/// (one line, one side the circle can sit on while passing through both
/// points above it... the far root c = 1.6 is the only real one here);
/// against y = −5 the symmetric answer. Both ends land exactly on the
/// points given; every chord's sagitta is within the tolerance.
#[test]
fn test_tangent_to_a_line_is_the_textbook_circle_and_the_arc_lands_on_its_points() {
    let (a, b) = (pt(-3.0, 0.0), pt(3.0, 0.0));
    let line = Reference::Segment(pt(-100.0, 5.0), pt(100.0, 5.0));
    let arcs = construct::arcs_through(a, b, &[line]).unwrap();
    assert_eq!(arcs.len(), 1, "{arcs:?}");
    let arc = &arcs[0];
    assert!((arc.cx).abs() < 1e-9 && (arc.cy - 1.6).abs() < 1e-9 && (arc.r - 3.4).abs() < 1e-9, "{arc:?}");
    assert!((arc.tx).abs() < 1e-9 && (arc.ty - 5.0).abs() < 1e-9, "touching the line directly above the centre");
    assert!(on_circle(arc, a) && on_circle(arc, b));
    // The arc runs from a to b THROUGH the touch — over the top, so
    // clockwise from (−3, 0) to (3, 0).
    assert!(!arc.ccw, "{arc:?}");
    let pts = arc.points(a, b);
    assert_eq!(pts[0], a);
    assert_eq!(*pts.last().unwrap(), b);
    assert!(pts.len() > 10, "a 3.4 radius through 220° needs many chords at 0.01: {}", pts.len());
    for w in pts.windows(2) {
        let (p, q) = (w[0], w[1]);
        let half = (q.x - p.x).hypot(q.y - p.y) / 2.0;
        let sagitta = arc.r - (arc.r * arc.r - half * half).max(0.0).sqrt();
        assert!(sagitta <= CHORD_TOL_MM + 1e-9, "sagitta {sagitta}");
        assert!(on_circle(arc, q));
    }
    // The touch itself is on the arc: a point of the chorded path lies within a chord of it.
    assert!(pts.iter().any(|p| (p.x - arc.tx).hypot(p.y - arc.ty) < 0.3));
    // A tangency whose foot falls outside the SEGMENT is not one.
    let short = Reference::Segment(pt(10.0, 5.0), pt(20.0, 5.0));
    assert!(construct::arcs_through(a, b, &[short]).unwrap().is_empty());
    // Coincident points refuse by name.
    assert!(construct::arcs_through(a, a, &[]).unwrap_err().contains("coincide"));
}

/// Tangent to a circle: two points on y = 0 at x = ±4 and a dia-6 circle
/// centred (0, 8). Externally tangent: |c − q| = R + 3; internally: the
/// arc wraps the circle, |c − q| = R − 3. Both are returned, smallest
/// radius first, the touch on the line of centres, the arc through it.
#[test]
fn test_tangent_to_a_circle_gives_the_external_and_internal_answers() {
    let (a, b) = (pt(-4.0, 0.0), pt(4.0, 0.0));
    let circle = Reference::Circle { cx: 0.0, cy: 8.0, r: 3.0 };
    let arcs = construct::arcs_through(a, b, &[circle]).unwrap();
    assert_eq!(arcs.len(), 2, "{arcs:?}");
    for arc in &arcs {
        assert!(on_circle(arc, a) && on_circle(arc, b), "{arc:?}");
        let d = (arc.cx).hypot(arc.cy - 8.0);
        let external = (d - (arc.r + 3.0)).abs() < 1e-7;
        let internal = (d - (arc.r - 3.0).abs()).abs() < 1e-7;
        assert!(external || internal, "{arc:?}: |c−q| = {d}");
        // The touch is on the reference circle AND on the arc's circle.
        assert!(((arc.tx).hypot(arc.ty - 8.0) - 3.0).abs() < 1e-7, "{arc:?}");
        assert!(on_circle(arc, pt(arc.tx, arc.ty)));
        // And the arc passes through it.
        let pts = arc.points(a, b);
        assert!(pts.iter().any(|p| (p.x - arc.tx).hypot(p.y - arc.ty) < 0.5), "{arc:?}");
    }
    assert!(arcs[0].r <= arcs[1].r, "smallest first");
    // The two touches are on opposite sides of the small circle: one below
    // it (the arc bulges up to meet it), one above (the arc wraps it).
    let ys: Vec<f64> = arcs.iter().map(|a| a.ty).collect();
    assert!(ys.iter().any(|y| *y < 8.0) && ys.iter().any(|y| *y > 8.0), "{ys:?}");
}

/// The references a declaration offers: a bore is its circle, grown by the
/// offset; a profile is its edges, pushed out; the blank is its four sides,
/// pushed in; a dome is its base circle; a kind with no footprint declines.
#[test]
fn test_references_are_read_from_the_declarations_and_pushed_out() {
    let p = load("pocket_islands.cam");
    let post = construct::references_of(&p, "PostA", 0.0).unwrap();
    assert_eq!(post, vec![Reference::Circle { cx: 45.0, cy: 60.0, r: 6.0 }]);
    let grown = construct::references_of(&p, "PostA", 5.5).unwrap();
    assert_eq!(grown, vec![Reference::Circle { cx: 45.0, cy: 60.0, r: 11.5 }]);
    let cavity = construct::references_of(&p, "Cavity", 0.0).unwrap();
    assert_eq!(cavity.len(), 4, "a rectangle's four edges: {cavity:?}");
    let pushed = construct::references_of(&p, "Cavity", 5.0).unwrap();
    // Pushed OUT by 5: every segment endpoint lies on the 15..125 × 15..105
    // box (the offsetter rounds the corners into arcs; their chords' ends
    // still lie on the pushed rectangle's sides... or on its corner arcs,
    // outside the original).
    for r in &pushed {
        let Reference::Segment(a, b) = r else { panic!("{r:?}") };
        for q in [a, b] {
            assert!(q.x <= 20.0 + 1e-6 || q.x >= 120.0 - 1e-6 || q.y <= 20.0 + 1e-6 || q.y >= 100.0 - 1e-6, "outside the authored loop: {q:?}");
        }
    }
    let stock = construct::references_of(&p, "stock", 5.0).unwrap();
    assert_eq!(stock.len(), 4);
    let Reference::Segment(a, _) = &stock[0] else { panic!() };
    assert_eq!(*a, pt(5.0, 5.0), "the blank's sides move IN");
    assert!(construct::references_of(&p, "Nobody", 0.0).unwrap_err().contains("no feature named 'Nobody'"));
    let p = load("sphere.cam");
    let name = p.features.keys().next().unwrap().clone();
    let dome = construct::references_of(&p, &name, 0.0).unwrap();
    assert!(matches!(dome[0], Reference::Circle { .. }), "a sphere's footprint is its equator");
}

/// The terminal's voice: every arc named with its index, the pick's points
/// as loop lines whose first and last are the two points given — the
/// motivating case on the shipped corpus: an arc from two points on the
/// cavity's floor, tangent to the post pushed out by the tool's radius.
#[test]
fn test_the_report_names_every_arc_and_prints_the_pick_as_loop_lines() {
    let p = load("pocket_islands.cam");
    let (a, b) = (pt(30.0, 70.0), pt(60.0, 70.0));
    let r = construct::report(&p, a, b, "PostA", 5.0, 1).unwrap();
    let lines: Vec<&str> = r.lines().collect();
    assert!(lines[0].starts_with("# 2 arcs through (30.000, 70.000) and (60.000, 70.000) tangent to 'PostA' pushed out 5.000mm:"), "{}", lines[0]);
    assert!(lines[1].starts_with("#   [1] R ") && lines[1].ends_with("← pick"), "{}", lines[1]);
    assert!(lines[2].starts_with("#   [2] R "), "{}", lines[2]);
    assert!(lines[3].starts_with("# [1] as loop points, chorded to 0.010mm ("), "{}", lines[3]);
    let pts: Vec<&str> = lines[4..].to_vec();
    assert_eq!(pts[0], "    30.000 70.000");
    assert_eq!(*pts.last().unwrap(), "    60.000 70.000");
    assert!(pts.len() > 3);
    // The second pick is the other arc; a pick past the end refuses by name.
    assert!(construct::report(&p, a, b, "PostA", 5.0, 2).unwrap().lines().nth(2).unwrap().ends_with("← pick"));
    assert!(construct::report(&p, a, b, "PostA", 5.0, 3).unwrap_err().contains("there are 2 arcs"));
    // No tangent arc: one point inside the post's circle and one outside —
    // a circle through both must CROSS the reference, twice, and cannot
    // touch it.
    let e = construct::report(&p, pt(45.0, 62.0), pt(80.0, 60.0), "PostA", 0.0, 1).unwrap_err();
    assert!(e.contains("no circle through"), "{e}");
    // The request's reading.
    let (a, b, name, off, pick) = construct::parse_request("30,70 60,70 tangent=PostA offset=5 pick=2").unwrap();
    assert_eq!((a, b, name.as_str(), off, pick), (pt(30.0, 70.0), pt(60.0, 70.0), "PostA", 5.0, 2));
    let (_, _, _, off, pick) = construct::parse_request("0,0 1,1 tangent=stock").unwrap();
    assert_eq!((off, pick), (0.0, 1));
    assert!(construct::parse_request("0,0 1,1").unwrap_err().contains("tangent="));
    assert!(construct::parse_request("0,0 tangent=stock").unwrap_err().contains("is not a point"));
    assert!(construct::parse_request("0,0").unwrap_err().contains("two points"));
    assert!(construct::parse_request("0,0 1,1 tangent=stock side=left").unwrap_err().contains("'side' is not a key"));
}

/// The vertex arm (Phase 42 slice 1): a diamond's top vertex (0, 5) and the
/// points (−3, 8), (3, 8). The circle through all three is centred (0, 8)
/// with radius 3 — tangent to the corner at its vertex and to neither edge
/// (its distance from either edge's line is 2.12) — so the segment arm
/// alone answers nothing there. The arc runs from the first point to the
/// second THROUGH the vertex: counter-clockwise, under the centre.
#[test]
fn test_a_circle_tangent_at_a_vertex_is_found_from_the_vertex_arm() {
    let diamond = [pt(-5.0, 0.0), pt(0.0, -5.0), pt(5.0, 0.0), pt(0.0, 5.0)];
    let refs: Vec<Reference> = (0..4).map(|i| Reference::Segment(diamond[i], diamond[(i + 1) % 4])).collect();
    let (a, b) = (pt(-3.0, 8.0), pt(3.0, 8.0));
    let arcs = construct::arcs_through(a, b, &refs).unwrap();
    let top = arcs.iter().find(|c| (c.tx).abs() < 1e-9 && (c.ty - 5.0).abs() < 1e-9).unwrap_or_else(|| panic!("{arcs:?}"));
    assert!((top.cx).abs() < 1e-9 && (top.cy - 8.0).abs() < 1e-9 && (top.r - 3.0).abs() < 1e-9, "{top:?}");
    assert!(top.ccw, "{top:?}");
    assert!(on_circle(top, a) && on_circle(top, b));
    assert_eq!(top.reference, 2, "the segment that ENDS at the vertex");
    // A circle CROSSING a corner is no tangency: the same points against a
    // corner at (0, 5) whose edges leave it one toward the centre's side of
    // the tangent and one away — up-left and down-right — answer nothing at
    // the vertex, and the bent edges' own lines are not tangent either.
    let bent = [Reference::Segment(pt(-5.0, 9.0), pt(0.0, 5.0)), Reference::Segment(pt(0.0, 5.0), pt(5.0, 1.0))];
    let arcs = construct::arcs_through(a, b, &bent).unwrap();
    assert!(arcs.iter().all(|c| (c.tx, c.ty) != (0.0, 5.0)), "{arcs:?}");
}
