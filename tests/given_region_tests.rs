// ---- the region grows into given air (Phase 42 slice 3) ----------------------
//
// `DECISIONS.md` → "Phase 42 slice 3…", fork 3A of the Phase 42 DRAFT: the
// pocket as intended does not exist; a larger void containing it does.
// `region::of` unions in the LENS of every given void whose floor piece
// straddles the authored outline — derived from containment, as islands
// are, never drawn. The lens's edge is a WALL, the authored outline's
// stretch inside the lens is dropped, the junction is a re-entrant corner
// the tool fillets. Wholly inside stays BRIDGED; apart is nothing; a void
// not given is a wall as ever. These pins hold the growth by geometry, the
// edge kinds, the two unchanged cases, the emitters reading the grown
// floor, the certificate standing behind it, and the outline's voice.

use nacs::parse::{self, Pt};
use nacs::{gcode, outline, region, stock};

const HEAD: &str = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\ntool T1 Endmill(dia=4.0)\n";
const DRAIN: &str = "feature Drain TiltedBore(dia=12, extent=20, x=70, y=40, tilt=30, heading=180)\n";
/// A pocket 8 deep whose right wall at x = 66 crosses the bore: at the floor
/// the lens (sections over depths 0..8, eroded and put back) spans x
/// 63.2..72.2 about y = 40, so the wall straddles it.
const STRADDLING: &str = "feature Pocket ProfilePocket(extent=8\n    30 20\n    66 20\n    66 60\n    30 60\n)\n";
const RUN: &str = "run Clear(tool=T1, profile=Pocket, stepover=2)\n";

fn program(lines: &str) -> parse::Part {
    parse::parse(&format!("{HEAD}{lines}")).unwrap_or_else(|e| panic!("must parse: {e}"))
}

fn bbox(lp: &[Pt]) -> (f64, f64, f64, f64) {
    lp.iter().fold((f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY), |b, q| {
        (b.0.min(q.x), b.1.min(q.y), b.2.max(q.x), b.3.max(q.y))
    })
}

#[test]
fn test_a_given_void_straddling_the_wall_grows_the_floor_into_its_lens() {
    let p = program(&format!("{DRAIN}given Drain\n{STRADDLING}{RUN}"));
    let r = region::of(&p, &p.operations[0]).unwrap();
    assert_eq!(r.grown, ["Drain"]);
    let lens = &stock::given_air(&p, &p.operations[0])[0];
    let (lx0, ly0, lx1, ly1) = bbox(&lens.floor[0]);
    assert!(lx0 < 66.0 && lx1 > 66.0, "the lens straddles the wall: {lx0}..{lx1}");
    // The outer grew exactly to the lens's far extreme and no further; the
    // pocket's own extremes are untouched.
    let (x0, y0, x1, y1) = bbox(&r.outer);
    assert!((x0 - 30.0).abs() < 1e-9 && (y0 - 20.0).abs() < 1e-9 && (y1 - 60.0).abs() < 1e-9, "{x0} {y0} {y1}");
    assert!((x1 - lx1).abs() < 1e-6, "{x1} vs {lx1}");
    assert!(ly0 > 20.0 && ly1 < 60.0, "the lens pokes through the right wall only");
    // Every edge is a wall: the pocket's, and the lens's (the body would
    // touch the bore there). The authored wall's stretch inside the lens is
    // gone — no vertex of the outer lies strictly inside the lens.
    assert_eq!(r.open_edges(), 0);
    assert!(r.edges.iter().all(|k| *k == region::EdgeKind::Wall));
    for q in &r.outer {
        let inside = stock::point_in_polygon(&lens.floor[0], q.x, q.y) && stock::dist_to_polygon(&lens.floor[0], q.x, q.y) > 1e-6;
        assert!(!inside, "{q:?} lies inside the lens");
    }
    // The grown part of the outer runs along the lens's own edge.
    let beyond: Vec<&Pt> = r.outer.iter().filter(|q| q.x > 66.0 + 1e-6).collect();
    assert!(beyond.len() >= 8, "{}", beyond.len());
    assert!(beyond.iter().all(|q| stock::dist_to_polygon(&lens.floor[0], q.x, q.y) < 1e-6), "on the lens's edge");
    assert!(!r.bridged().contains(&"Drain"), "straddling, not bridged");
    assert!(!r.is_plain());
    assert!(r.line(4.0).contains("grown into the given air of Drain"), "{}", r.line(4.0));
}

#[test]
fn test_not_given_the_same_wall_is_a_wall_and_wholly_inside_stays_bridged() {
    // Not given: the bore's air is forbidden, the outline is the region.
    let p = program(&format!("{DRAIN}{STRADDLING}{RUN}"));
    let r = region::of(&p, &p.operations[0]).unwrap();
    assert!(r.grown.is_empty());
    assert_eq!(r.outer.len(), 4);
    assert!(r.is_plain());
    // Given but wholly inside the floor and deeper: BRIDGED, unchanged.
    let p = program(&format!(
        "{DRAIN}given Drain\nfeature Pocket ProfilePocket(extent=8\n    40 20\n    90 20\n    90 60\n    40 60\n)\n{RUN}"
    ));
    let r = region::of(&p, &p.operations[0]).unwrap();
    assert!(r.grown.is_empty());
    assert_eq!(r.bridged(), ["Drain"]);
    assert_eq!(r.outer.len(), 4);
    // Given and apart: nothing.
    let p = program(&format!("{DRAIN}given Drain\nfeature Pocket ProfilePocket(extent=8\n    10 20\n    50 20\n    50 60\n    10 60\n)\n{RUN}"));
    let r = region::of(&p, &p.operations[0]).unwrap();
    assert!(r.grown.is_empty() && r.outer.len() == 4 && r.is_plain());
}

#[test]
fn test_a_given_column_void_grows_the_floor_too_and_a_face_takes_the_lens_edge_as_a_wall() {
    // A given pocket, shallower than this one, straddling its wall: a lens
    // of its own (its outline eroded and put back — the outline itself for a
    // rod), unioned in.
    let p = program(&format!(
        "feature Dish ProfilePocket(extent=12\n    60 30\n    90 30\n    90 50\n    60 50\n)\ngiven Dish\n{STRADDLING}{RUN}"
    ));
    let r = region::of(&p, &p.operations[0]).unwrap();
    assert_eq!(r.grown, ["Dish"]);
    let (_, _, x1, _) = bbox(&r.outer);
    assert!((x1 - 90.0).abs() < 1e-6, "{x1}");
    assert!(r.edges.iter().all(|k| *k == region::EdgeKind::Wall));
    // A Face host: its own edges open (the silhouette), the lens's a wall.
    let p = program(&format!(
        "{DRAIN}given Drain\nfeature Top Face(z=-8\n    loop\n        30 20\n        66 20\n        66 60\n        30 60\n)\nrun End(tool=T1, surface=Top, stepover=2)\n"
    ));
    let r = region::of(&p, &p.operations[0]).unwrap();
    assert_eq!(r.grown, ["Drain"]);
    let lens = &stock::given_air(&p, &p.operations[0])[0];
    for (i, k) in r.edges.iter().enumerate() {
        let (a, b) = (r.outer[i], r.outer[(i + 1) % r.outer.len()]);
        let mid = Pt { x: (a.x + b.x) / 2.0, y: (a.y + b.y) / 2.0 };
        let on_lens = stock::dist_to_polygon(&lens.floor[0], mid.x, mid.y) < 1e-6 && mid.x > 66.0;
        assert_eq!(*k == region::EdgeKind::Wall, on_lens, "edge {i} at {mid:?}");
    }
}

#[test]
fn test_the_emitters_cut_the_grown_floor_and_the_certificate_stands_behind_it() {
    let p = program(&format!("{DRAIN}given Drain\n{STRADDLING}{RUN}"));
    let g = gcode::program(&p).unwrap();
    assert!(g.contains("(grown into given air: Drain)\n"), "{}", &g[..600]);
    let lens = &stock::given_air(&p, &p.operations[0])[0];
    let (_, _, lx1, _) = bbox(&lens.floor[0]);
    let xs: Vec<f64> = g
        .lines()
        .filter(|l| l.starts_with("G01") || l.starts_with("G00"))
        .filter_map(|l| l.split_whitespace().find(|w| w.starts_with('X')).and_then(|w| w[1..].parse::<f64>().ok()))
        .collect();
    let max_x = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    // Past the authored wall by more than a radius, never past the lens
    // less a radius.
    assert!(max_x > 66.0 + 2.0 && max_x <= lx1 - 2.0 + 1e-6, "{max_x} vs {lx1}");
    // The same program with the pocket's floor finished by an End reads the
    // same region.
    let p = program(&format!("{DRAIN}given Drain\n{STRADDLING}{RUN}run End(tool=T1, surface=Pocket, stepover=2)\n"));
    let g = gcode::program(&p).unwrap();
    assert_eq!(g.matches("(grown into given air: Drain)\n").count(), 2, "{g}");
    // The stock model: nothing extra is claimed removed — the grown part is
    // the given air already — and the moves are blessed (the program came
    // back at all).
    let after = stock::remaining_after(&p, 0);
    assert!(after.complete);
    assert!(!after.material_at(68.0, 40.0, -4.0), "the bore's air");
    assert!(after.material_at(68.0, 48.0, -4.0), "the tube wall beside the lens");
}

#[test]
fn test_the_outline_says_the_floor_grew_and_lists_the_void_as_shaping() {
    let p = program(&format!("{DRAIN}given Drain\n{STRADDLING}{RUN}"));
    let o = outline::outline(&p);
    let text = outline::render(&p, &o);
    assert!(text.contains("grown into the given air of Drain"), "{text}");
    assert!(text.contains("\nindirectly used (no operation names it; it shapes what one cuts):\n  Drain  TiltedBore  given air grown into by [0]\n"), "{text}");
    assert!(text.contains("\ngiven (air before this program starts — cut on another setup):\n  Drain  TiltedBore\n"), "{text}");
}
