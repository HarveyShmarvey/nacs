use crate::math::Portable as _;
use crate::parse::Pt;
use cavalier_contours::polyline::{seg_arc_radius_and_center, BooleanOp, Polyline, PlineVertex, PlineSource};
use cavalier_contours::shape_algorithms::{Shape, ShapeOffsetOptions};

/// A vertex of an offset polyline, keeping the `bulge` that `cavalier_contours`
/// puts on the segment *leaving* this vertex. `bulge == 0.0` is a straight
/// segment; nonzero is a circular arc (`tan(theta/4)`). Callers that only emit
/// straight moves must check for arcs rather than dropping the bulge silently.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OffsetVertex {
    pub x: f64,
    pub y: f64,
    pub bulge: f64,
}

/// Center of the circular arc on the segment *leaving* `v1` toward `v2` — the
/// segment whose `bulge` is `v1.bulge` (see `OffsetVertex`). Reuses
/// `cavalier_contours`' own `seg_arc_radius_and_center` (the same routine the
/// offsetter uses internally) rather than re-deriving the bulge geometry, and
/// hands back a plain `(f64, f64)` so the crate's `Vector2` never leaks past
/// this module. Only meaningful for an arc segment (`v1.bulge != 0.0`); the
/// underlying helper's behavior is undefined for a zero bulge, so callers must
/// gate on the bulge first.
pub fn arc_center(v1: &OffsetVertex, v2: &OffsetVertex) -> (f64, f64) {
    let (_, c) = seg_arc_radius_and_center(
        PlineVertex::new(v1.x, v1.y, v1.bulge),
        PlineVertex::new(v2.x, v2.y, 0.0),
    );
    (c.x, c.y)
}

/// Offsets a closed 2D polyline, preserving each segment's `bulge`. This is the
/// arc-aware sibling of `offset_closed_polyline`; the plain version is a thin
/// wrapper that discards the bulge for callers that don't need it.
pub fn offset_closed_polyline_verts(points: &[Pt], distance: f64) -> Vec<Vec<OffsetVertex>> {
    if points.is_empty() {
        return Vec::new();
    }

    let mut pline = Polyline::new();
    pline.is_closed = true;
    for pt in points {
        pline.vertex_data.push(PlineVertex::new(pt.x, pt.y, 0.0));
    }

    pline
        .parallel_offset(distance)
        .into_iter()
        .map(|op| {
            op.vertex_data
                .iter()
                .map(|v| OffsetVertex { x: v.x, y: v.y, bulge: v.bulge })
                .collect()
        })
        .collect()
}

/// Offsets an OPEN polyline — a wall chain between two open edges (Phase 60
/// slice 1) — by `distance`, the sign the closed sibling reads (positive is left
/// of travel). Each result is an open polyline; the ends are not capped.
pub fn offset_open_polyline_verts(points: &[Pt], distance: f64) -> Vec<Vec<OffsetVertex>> {
    if points.len() < 2 {
        return Vec::new();
    }
    let mut pline = Polyline::new();
    pline.is_closed = false;
    for pt in points {
        pline.vertex_data.push(PlineVertex::new(pt.x, pt.y, 0.0));
    }
    pline
        .parallel_offset(distance)
        .into_iter()
        .map(|op| op.vertex_data.iter().map(|v| OffsetVertex { x: v.x, y: v.y, bulge: v.bulge }).collect())
        .collect()
}

/// Is `q` inside this closed offset loop — arcs and all? The offsetting
/// crate's own winding number, exact over bulge arcs (Phase 37 slice 1): the
/// engagement band used to carry a hand-written ray-cast-and-flip for this,
/// and it flipped on the wrong side of every arc's chord.
pub fn loop_contains(lp: &[OffsetVertex], q: (f64, f64)) -> bool {
    let mut pline = Polyline::new();
    pline.is_closed = true;
    for v in lp {
        pline.vertex_data.push(PlineVertex::new(v.x, v.y, v.bulge));
    }
    pline.winding_number(cavalier_contours::core::math::Vector2::new(q.0, q.1)) != 0
}

/// Chord-flatten a closed offset loop ALONG its arcs, never across them.
///
/// `offset_closed_polyline` keeps only the vertices, so a fillet arc becomes
/// the chord between its ends — and an offset's arc is always centred on the
/// corner it rounds, so that chord always passes NEARER the corner than the
/// arc does. Around a boss (an outward offset, Phase 10 lane A's moat family)
/// the corner is the boss's own. Inside a pocket (an inward offset) the only
/// corners that grow an arc are the ones that point INTO the pocket, and they
/// are the part's too. Either way a tool riding the chord at the wall level
/// cuts the corner it was rounding. (This comment used to call the inward
/// chord "the silent direction". It was not: `DECISIONS.md` → "Phase 37 slice
/// 0…".) So a loop a tool will ride is flattened here, sampling each arc at
/// the chord tolerance exactly as the waterline rings are
/// (`circle_segments`'s own law, `CHORD_TOL_MM`).
pub fn flatten_loop(lp: &[OffsetVertex]) -> Vec<Pt> {
    let n = lp.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let v1 = &lp[i];
        let v2 = &lp[(i + 1) % n];
        out.push(Pt { x: v1.x, y: v1.y });
        if v1.bulge.abs() <= 1e-12 {
            continue;
        }
        let (cx, cy) = arc_center(v1, v2);
        let rad = ((v1.x - cx).powi(2) + (v1.y - cy).powi(2)).sqrt();
        if rad <= 1e-12 {
            continue;
        }
        // bulge = tan(sweep / 4), signed by direction.
        let sweep = 4.0 * v1.bulge.patan();
        let ratio = (1.0 - crate::CHORD_TOL_MM / rad).clamp(-1.0, 1.0);
        let theta = 2.0 * ratio.pacos();
        let steps = ((sweep.abs() / theta).ceil() as usize).max(1);
        let a1 = (v1.y - cy).patan2(v1.x - cx);
        for k in 1..steps {
            let a = a1 + sweep * k as f64 / steps as f64;
            out.push(Pt { x: cx + rad * a.pcos(), y: cy + rad * a.psin() });
        }
    }
    out
}

/// A standing outline a cleared region goes around. A round boss goes to the
/// offsetter as a TRUE circle — two half-circle arcs — so its parallel loops
/// come back as circles and are flattened once, at emission, at the chord
/// tolerance; a chorded circle would come back as hundreds of slivers of arc,
/// one per chord corner.
#[derive(Clone, Debug, PartialEq)]
pub enum Island {
    Circle { x: f64, y: f64, r: f64 },
    Loop(Vec<Pt>),
}

/// One level of a region-with-islands family: the loops parallel to the outer
/// wall (CCW) and the loops parallel to the islands (CW), as the offsetter
/// hands them back — already wound the way each climbs with M03.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapeLevel {
    pub ccw: Vec<Vec<OffsetVertex>>,
    pub cw: Vec<Vec<OffsetVertex>>,
}

impl ShapeLevel {
    pub fn is_empty(&self) -> bool {
        self.ccw.is_empty() && self.cw.is_empty()
    }
}

/// The region between an outer wall and its islands, eroded by `distance` from
/// EVERY wall at once (Phase 26 slice 2; `DECISIONS.md` → "Phase 26 opens: the
/// island DRAFT…", fork 1A). `cavalier_contours`' shape offset — CCW outers,
/// CW holes — which merges an island's loop into the wall's where a tool
/// cannot pass between them and splits the region where it pinches; both are
/// read by the caller as COUNTS, never patched here. `outer` may be either
/// winding; it is normalised, as the islands are. The crate's types stay
/// behind this module.
pub fn offset_shape(outer: &[Pt], islands: &[Island], distance: f64) -> ShapeLevel {
    if outer.len() < 3 {
        return ShapeLevel::default();
    }
    let area: f64 = (0..outer.len())
        .map(|i| {
            let (a, b) = (outer[i], outer[(i + 1) % outer.len()]);
            a.x * b.y - b.x * a.y
        })
        .sum();
    let mut plines: Vec<Polyline<f64>> = Vec::with_capacity(islands.len() + 1);
    let closed = |pts: &mut dyn Iterator<Item = Pt>| {
        let mut pl = Polyline::new();
        pl.is_closed = true;
        for q in pts {
            pl.vertex_data.push(PlineVertex::new(q.x, q.y, 0.0));
        }
        pl
    };
    plines.push(if area >= 0.0 { closed(&mut outer.iter().copied()) } else { closed(&mut outer.iter().rev().copied()) });
    for island in islands {
        plines.push(match island {
            // Bulge -1 is a CW half-circle; two of them are the hole.
            Island::Circle { x, y, r } => {
                let mut pl = Polyline::new();
                pl.is_closed = true;
                pl.vertex_data.push(PlineVertex::new(x - r, *y, -1.0));
                pl.vertex_data.push(PlineVertex::new(x + r, *y, -1.0));
                pl
            }
            Island::Loop(pts) => {
                let a: f64 = (0..pts.len())
                    .map(|i| {
                        let (p0, p1) = (pts[i], pts[(i + 1) % pts.len()]);
                        p0.x * p1.y - p1.x * p0.y
                    })
                    .sum();
                if a <= 0.0 {
                    closed(&mut pts.iter().copied())
                } else {
                    closed(&mut pts.iter().rev().copied())
                }
            }
        });
    }
    let shape = Shape::from_plines(plines).parallel_offset(distance, ShapeOffsetOptions::default());
    let verts = |pl: &Polyline<f64>| pl.vertex_data.iter().map(|v| OffsetVertex { x: v.x, y: v.y, bulge: v.bulge }).collect();
    ShapeLevel {
        ccw: shape.ccw_plines.iter().map(|ip| verts(&ip.polyline)).collect(),
        cw: shape.cw_plines.iter().map(|ip| verts(&ip.polyline)).collect(),
    }
}

/// The part of one closed polygon that lies inside another — a cavity's
/// outline clipped to the blank (Phase 27, fork 2A). `cavalier_contours`'
/// polyline boolean, in the ledger since Phase 2 and unused until now. Both
/// windings are accepted; the pieces come back CCW, largest first, so a caller
/// that expects one piece takes the first and one that does not can see the
/// rest.
pub fn clip_to(subject: &[Pt], window: &[Pt]) -> Vec<Vec<Pt>> {
    let closed_ccw = |pts: &[Pt]| {
        let area: f64 = (0..pts.len())
            .map(|i| {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                a.x * b.y - b.x * a.y
            })
            .sum();
        let mut pl = Polyline::new();
        pl.is_closed = true;
        let mut push = |q: &Pt| pl.vertex_data.push(PlineVertex::new(q.x, q.y, 0.0));
        if area >= 0.0 {
            pts.iter().for_each(&mut push);
        } else {
            pts.iter().rev().for_each(&mut push);
        }
        pl
    };
    if subject.len() < 3 || window.len() < 3 {
        return Vec::new();
    }
    let result = closed_ccw(subject).boolean(&closed_ccw(window), BooleanOp::And);
    let mut pieces: Vec<(f64, Vec<Pt>)> = result
        .pos_plines
        .iter()
        .map(|r| (r.pline.area().abs(), r.pline.vertex_data.iter().map(|v| Pt { x: v.x, y: v.y }).collect::<Vec<Pt>>()))
        .filter(|(_, pts)| pts.len() >= 3)
        .collect();
    pieces.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    pieces.into_iter().map(|(_, pts)| crate::ops::clear::make_ccw(&pts)).collect()
}

/// Two overlapping closed polygons as one — the crate's boolean, the other way
/// (Phase 27 slice 2: a region pushed out across its open edges). `None` when
/// the union is not one simple piece, which a caller must treat as "could not".
pub fn union(a: &[Pt], b: &[Pt]) -> Option<Vec<Pt>> {
    let closed = |pts: &[Pt]| {
        let ccw = crate::ops::clear::make_ccw(pts);
        let mut pl = Polyline::new();
        pl.is_closed = true;
        ccw.iter().for_each(|q| pl.vertex_data.push(PlineVertex::new(q.x, q.y, 0.0)));
        pl
    };
    let result = closed(a).boolean(&closed(b), BooleanOp::Or);
    if result.pos_plines.len() != 1 || !result.neg_plines.is_empty() {
        return None;
    }
    let pts: Vec<Pt> = result.pos_plines[0].pline.vertex_data.iter().map(|v| Pt { x: v.x, y: v.y }).collect();
    Some(crate::ops::clear::make_ccw(&pts))
}

/// One closed polygon with another cut out of it — the largest piece left, CCW
/// (Phase 27 slice 3: a keep-out that straddles a region's boundary is a bite
/// out of that boundary, not an island inside it). `None` when nothing simple
/// is left.
pub fn subtract(a: &[Pt], b: &[Pt]) -> Option<Vec<Pt>> {
    let closed = |pts: &[Pt]| {
        let ccw = crate::ops::clear::make_ccw(pts);
        let mut pl = Polyline::new();
        pl.is_closed = true;
        ccw.iter().for_each(|q| pl.vertex_data.push(PlineVertex::new(q.x, q.y, 0.0)));
        pl
    };
    let result = closed(a).boolean(&closed(b), BooleanOp::Not);
    if !result.neg_plines.is_empty() {
        return None;
    }
    result
        .pos_plines
        .iter()
        .map(|r| (r.pline.area().abs(), r.pline.vertex_data.iter().map(|v| Pt { x: v.x, y: v.y }).collect::<Vec<Pt>>()))
        .filter(|(_, pts)| pts.len() >= 3)
        .max_by(|x, y| x.0.partial_cmp(&y.0).unwrap())
        .map(|(_, pts)| crate::ops::clear::make_ccw(&pts))
}

/// One closed polygon with another cut out of it — EVERY piece left, CCW,
/// largest first (Phase 33: a rest stretch less what stands, where a fin can
/// split one stretch in two and `subtract`'s largest piece would drop half).
/// A `b` wholly inside `a` is a hole, not a cut, and the caller's to keep: it
/// comes back as `a` untouched.
pub fn subtract_all(a: &[Pt], b: &[Pt]) -> Vec<Vec<Pt>> {
    let closed = |pts: &[Pt]| {
        let ccw = crate::ops::clear::make_ccw(pts);
        let mut pl = Polyline::new();
        pl.is_closed = true;
        ccw.iter().for_each(|q| pl.vertex_data.push(PlineVertex::new(q.x, q.y, 0.0)));
        pl
    };
    let result = closed(a).boolean(&closed(b), BooleanOp::Not);
    let mut pieces: Vec<(f64, Vec<Pt>)> = result
        .pos_plines
        .iter()
        .map(|r| (r.pline.area().abs(), r.pline.vertex_data.iter().map(|v| Pt { x: v.x, y: v.y }).collect::<Vec<Pt>>()))
        .filter(|(_, pts)| pts.len() >= 3)
        .collect();
    pieces.sort_by(|x, y| y.0.partial_cmp(&x.0).unwrap());
    pieces.into_iter().map(|(_, pts)| crate::ops::clear::make_ccw(&pts)).collect()
}

/// The whole family, OUTERMOST FIRST: levels from `d0` inward in `stepover`
/// steps until the region is gone. One home for the emitter and the
/// engagement primitive, which must walk the same loops or price the wrong cut.
pub fn shape_levels(outer: &[Pt], islands: &[Island], d0: f64, stepover: f64) -> Vec<(f64, ShapeLevel)> {
    let mut levels = Vec::new();
    if stepover <= 0.0 {
        return levels;
    }
    let mut d = d0;
    loop {
        let level = offset_shape(outer, islands, d);
        if level.is_empty() {
            return levels;
        }
        levels.push((d, level));
        d += stepover;
    }
}

/// Offsets a closed 2D polyline by a distance — positive is inward for a
/// counter-clockwise polygon — and hands back each loop's VERTICES ONLY: an
/// arc comes back as the chord between its ends. That is right for asking
/// whether an offset is empty, and wrong for any loop a tool will ride or a
/// model will claim: those take `offset_closed_polyline_verts` and
/// `flatten_loop`.
pub fn offset_closed_polyline(points: &[Pt], distance: f64) -> Vec<Vec<Pt>> {
    offset_closed_polyline_verts(points, distance)
        .into_iter()
        .map(|poly| poly.into_iter().map(|v| Pt { x: v.x, y: v.y }).collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(a: f64, b: f64) -> Vec<Pt> {
        vec![Pt { x: a, y: a }, Pt { x: b, y: a }, Pt { x: b, y: b }, Pt { x: a, y: b }]
    }

    /// The DRAFT's spike, as a pin: `pocket_post.cam`'s own geometry. The
    /// wall's loop and the island's come back separately until the region
    /// between them is thinner than two steps, then as four corners.
    #[test]
    fn test_a_post_in_a_square_erodes_to_a_ring_then_four_corners() {
        let post = [Island::Circle { x: 50.0, y: 50.0, r: 6.0 }];
        let l0 = offset_shape(&square(20.0, 80.0), &post, 5.0);
        assert_eq!((l0.ccw.len(), l0.cw.len()), (1, 1));
        // The island's loop is a true circle of radius 6 + 5, wound CW.
        let c = &l0.cw[0];
        assert_eq!(c.len(), 2);
        assert!(c.iter().all(|v| (v.bulge + 1.0).abs() < 1e-9), "CW half-circles: {c:?}");
        assert!(((c[0].x - c[1].x).abs() - 22.0).abs() < 1e-6);
        let l2 = offset_shape(&square(20.0, 80.0), &post, 13.0);
        assert_eq!((l2.ccw.len(), l2.cw.len()), (4, 0));
        assert!(offset_shape(&square(20.0, 80.0), &post, 17.0).is_empty());
        // Either winding of the wall or the island is the same region.
        let cw_wall: Vec<Pt> = square(20.0, 80.0).into_iter().rev().collect();
        assert_eq!(offset_shape(&cw_wall, &post, 5.0), l0);
    }

    /// A tool that cannot pass between an island and the wall is a COUNT: the
    /// island's loop has merged into the wall's at the very first level.
    #[test]
    fn test_a_web_is_a_missing_island_loop() {
        let l = offset_shape(&square(20.0, 80.0), &[Island::Circle { x: 34.0, y: 50.0, r: 6.0 }], 5.0);
        assert_eq!((l.ccw.len(), l.cw.len()), (1, 0));
        // With no island at all the shape offset is the polyline offset's loop.
        let plain = offset_shape(&square(20.0, 80.0), &[], 5.0);
        assert_eq!((plain.ccw.len(), plain.cw.len()), (1, 0));
        assert_eq!(plain.ccw[0].len(), 4);
    }

    #[test]
    fn test_simple_square_offset() {
        // Create a 10x10 CCW square: (0,0), (10,0), (10,10), (0,10)
        let pts = vec![
            Pt { x: 0.0, y: 0.0 },
            Pt { x: 10.0, y: 0.0 },
            Pt { x: 10.0, y: 10.0 },
            Pt { x: 0.0, y: 10.0 },
        ];

        // Offset inward by 1.0 (CCW square -> positive distance offsets inward)
        let offset = offset_closed_polyline(&pts, 1.0);
        assert_eq!(offset.len(), 1);
        
        let poly = &offset[0];
        assert_eq!(poly.len(), 4);
        
        // Assert the offset polygon vertices are at 1.0 and 9.0
        for pt in poly {
            assert!(pt.x >= 0.99 && pt.x <= 9.01);
            assert!(pt.y >= 0.99 && pt.y <= 9.01);
        }
    }

    #[test]
    fn test_arc_center_semicircle() {
        // A CCW half-circle from (0,0) to (1,0) has bulge 1.0 (tan(pi/4)); its
        // center is the chord midpoint (0.5, 0). Mirrors the crate's own
        // seg_arc_radius_and_center doctest, through our (f64, f64) wrapper.
        let v1 = OffsetVertex { x: 0.0, y: 0.0, bulge: 1.0 };
        let v2 = OffsetVertex { x: 1.0, y: 0.0, bulge: 0.0 };
        let (cx, cy) = arc_center(&v1, &v2);
        assert!((cx - 0.5).abs() < 1e-9, "center x was {cx}");
        assert!((cy - 0.0).abs() < 1e-9, "center y was {cy}");
    }

    #[test]
    fn test_arc_center_quarter_circle_sign() {
        // A shallow CCW quarter arc from (1,0) to (0,1), bulge tan(pi/8), curves
        // around the origin — center at (0,0). The negated bulge (CW) keeps the
        // same endpoints but bulges the other way, so its center flips to (1,1).
        let b = (std::f64::consts::PI / 8.0).ptan();
        let ccw = arc_center(
            &OffsetVertex { x: 1.0, y: 0.0, bulge: b },
            &OffsetVertex { x: 0.0, y: 1.0, bulge: 0.0 },
        );
        assert!((ccw.0).abs() < 1e-9 && (ccw.1).abs() < 1e-9, "ccw center {ccw:?}");
        let cw = arc_center(
            &OffsetVertex { x: 1.0, y: 0.0, bulge: -b },
            &OffsetVertex { x: 0.0, y: 1.0, bulge: 0.0 },
        );
        assert!((cw.0 - 1.0).abs() < 1e-9 && (cw.1 - 1.0).abs() < 1e-9, "cw center {cw:?}");
    }
}
