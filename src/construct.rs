//! Boundary construction against projected references — Phase 41 slice 3
//! (`DECISIONS.md` → "Phase 41 slice 3…"; the T5 entry "Boundary construction
//! against projected references…").
//!
//! The case that motivated it: a pocket's boundary needed an arc through two
//! known points TANGENT to a neighbour's footprint, and another CAM package
//! had no way to say it. The circle through two points tangent to a curve is
//! Apollonius' point-point-curve case: its centre lies on the two points'
//! perpendicular bisector, so the problem is one-dimensional — a parameter
//! `s` along that line — and against a LINE or a CIRCLE it is a quadratic in
//! `s`, closed form, up to two answers each. Every footprint this engine
//! declares is lines and circles (a profile's edges, a bore's or boss's
//! circle, the blank's four sides), so nothing here is iterated.
//!
//! **What comes out is `.part` text**: the arc from the first point to the
//! second, through the tangent point, chorded at the one chord tolerance —
//! exactly how every curved fixture in the corpus is authored today. The
//! grammar gains no arc; the file holds what it always held, points. The
//! author pastes the lines into a loop, and the parser is the only judge.
//!
//! **A reference is DERIVED, never saved**: the plan footprint of a declared
//! feature (or the blank), pushed out by whatever the caller asks — a tool's
//! radius plus an allowance, typically — read from the features' own fields.
//! Nothing here is a noun; 3D informs the 2D construction through its
//! footprint and never joins it.

use crate::math::Portable as _;
use crate::parse::{Feature, Part, Pt};
use crate::CHORD_TOL_MM;

/// Two exactly-computed reals that should agree, do, to this.
const EPS: f64 = 1e-9;

/// A curve the arc may be tangent to, in plan.
#[derive(Clone, Debug, PartialEq)]
pub enum Reference {
    /// A segment from `a` to `b`: tangency counts only where the tangent
    /// point lies within it.
    Segment(Pt, Pt),
    /// A circle: centre and radius.
    Circle { cx: f64, cy: f64, r: f64 },
}

/// One answer: the circle, where it touches, and the arc's sense from the
/// first point to the second through the touch.
#[derive(Clone, Debug, PartialEq)]
pub struct Arc {
    pub cx: f64,
    pub cy: f64,
    pub r: f64,
    /// Where the circle touches the reference.
    pub tx: f64,
    pub ty: f64,
    /// Counter-clockwise from the first point to the second.
    pub ccw: bool,
    /// Which reference (index into the list handed in) it touches.
    pub reference: usize,
}

fn sub(a: Pt, b: Pt) -> (f64, f64) {
    (a.x - b.x, a.y - b.y)
}

fn quadratic(a: f64, b: f64, c: f64) -> Vec<f64> {
    if a.abs() < EPS {
        return if b.abs() < EPS { Vec::new() } else { vec![-c / b] };
    }
    let d = b * b - 4.0 * a * c;
    if d < -EPS {
        return Vec::new();
    }
    let sq = d.max(0.0).sqrt();
    let mut out = vec![(-b - sq) / (2.0 * a), (-b + sq) / (2.0 * a)];
    out.dedup_by(|x, y| (*x - *y).abs() < EPS);
    out
}

/// The arcs through `a` and `b` tangent to one of `refs`, every solution,
/// smallest radius first. The centre is `m + s·n` for the midpoint `m` and
/// the unit normal `n` of `ab`, the radius `sqrt(h² + s²)` for the half
/// chord `h`; against a line `|dist(centre, line)| = r` and against a
/// circle `|centre − q| = r ± ρ` each square to a quadratic in `s`.
pub fn arcs_through(a: Pt, b: Pt, refs: &[Reference]) -> Result<Vec<Arc>, String> {
    let (dx, dy) = sub(b, a);
    let chord = dx.phypot(dy);
    if chord < EPS {
        return Err("the two points coincide — an arc needs two distinct points".to_string());
    }
    let h = chord / 2.0;
    let (mx, my) = ((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    let (nx, ny) = (-dy / chord, dx / chord);
    let mut out = Vec::new();
    for (k, rf) in refs.iter().enumerate() {
        match rf {
            Reference::Segment(p, q) => {
                let (ex, ey) = sub(*q, *p);
                let len = ex.phypot(ey);
                if len < EPS {
                    continue;
                }
                let (ux, uy) = (ex / len, ey / len);
                // Signed distance from the centre to the line: cross(c − p, u)
                // = alpha + beta·s.
                let alpha = (mx - p.x) * uy - (my - p.y) * ux;
                let beta = nx * uy - ny * ux;
                // (alpha + beta s)² = h² + s²
                for s in quadratic(beta * beta - 1.0, 2.0 * alpha * beta, alpha * alpha - h * h) {
                    let (cx, cy) = (mx + s * nx, my + s * ny);
                    let r = (h * h + s * s).sqrt();
                    // The foot of the perpendicular: must lie within the segment.
                    let t = ((cx - p.x) * ux + (cy - p.y) * uy) / len;
                    if !(-EPS..=1.0 + EPS).contains(&t) {
                        continue;
                    }
                    let (tx, ty) = (p.x + t * len * ux, p.y + t * len * uy);
                    out.push(arc(a, b, cx, cy, r, tx, ty, k));
                }
            }
            Reference::Circle { cx: qx, cy: qy, r: rho } => {
                let (qx, qy, rho) = (*qx, *qy, *rho);
                // |c − q|² = |m − q|² + 2 s n·(m − q) + s² ; r² = h² + s²
                // external: |c − q| = r + ρ ; internal: |c − q| = |r − ρ|
                // both: (|c − q|² − r² − ρ²)² = 4 ρ² r²  →  (gamma + delta s)² = 4ρ²(h² + s²)
                let (wx, wy) = (mx - qx, my - qy);
                let gamma = wx * wx + wy * wy - h * h - rho * rho;
                let delta = 2.0 * (nx * wx + ny * wy);
                for s in quadratic(delta * delta - 4.0 * rho * rho, 2.0 * gamma * delta, gamma * gamma - 4.0 * rho * rho * h * h) {
                    let (cx, cy) = (mx + s * nx, my + s * ny);
                    let r = (h * h + s * s).sqrt();
                    let d = (cx - qx).phypot(cy - qy);
                    if d < EPS {
                        continue;
                    }
                    // Which tangency this root is: the touch lies on the
                    // line of centres, on the far side of q for an
                    // external touch and the near side for an internal one.
                    let external = (d - (r + rho)).abs() < 1e-6 * (1.0 + r + rho);
                    let internal = (d - (r - rho).abs()).abs() < 1e-6 * (1.0 + r + rho);
                    if !external && !internal {
                        continue;
                    }
                    // Externally, and internally with the arc's circle
                    // INSIDE the reference, the touch is on q's far side
                    // along the line of centres; internally with the arc
                    // WRAPPING the reference it is on the near side.
                    let sign = if internal && r > rho { -1.0 } else { 1.0 };
                    let (tx, ty) = (qx + sign * rho * (cx - qx) / d, qy + sign * rho * (cy - qy) / d);
                    out.push(arc(a, b, cx, cy, r, tx, ty, k));
                }
            }
        }
    }
    // The VERTEX arm (Phase 42 slice 1; the intake's fifth premise
    // correction). A circle tangent to a convex corner AT ITS VERTEX is
    // tangent to neither chord — the foot of its perpendicular lies within
    // no segment — so the segment arm above misses every tangency that
    // falls on a chorded loop's vertex: the envelope's extreme point at
    // `offset=0` answered "no circle" while `offset=0.001` answered two
    // arcs, because a positive offset rounds the vertex into chords. Where
    // two references meet end to start, the circle through the two points
    // AND the vertex is the candidate — linear in `s`, the vertex being a
    // third point on the circle — kept when its radial direction at the
    // vertex lies within the vertex's normal cone, which is when the circle
    // sits on one side of both edges there: touching the corner from
    // outside, or wrapping it (and, at a reflex vertex, sitting inside it).
    //
    // On a CHORDED curve every vertex is a small corner, and the circle
    // through a vertex beside the chord the segment arm touched is the same
    // answer a few microns over — so a vertex answer is kept only where no
    // segment answer agrees with it within the chord tolerance (the
    // reference cannot tell two circles apart that close), and the segment
    // arm's answers stand exactly as they did.
    let mut at_vertices = Vec::new();
    for (i, rf) in refs.iter().enumerate() {
        let Reference::Segment(p0, v) = rf else { continue };
        for (j, other) in refs.iter().enumerate() {
            let Reference::Segment(q0, q1) = other else { continue };
            if i == j || (q0.x - v.x).abs() > EPS || (q0.y - v.y).abs() > EPS {
                continue;
            }
            // The two edges, as unit directions AWAY from the vertex.
            let away = |q: Pt| {
                let (ex, ey) = sub(q, *v);
                let len = ex.phypot(ey);
                (len >= EPS).then(|| (ex / len, ey / len))
            };
            let (Some(e1), Some(e2)) = (away(*p0), away(*q1)) else { continue };
            // |m + s n − v|² = h² + s²: the s² terms cancel, leaving a line.
            let (wx, wy) = (mx - v.x, my - v.y);
            let nw = nx * wx + ny * wy;
            if nw.abs() < EPS {
                continue;
            }
            let s = (h * h - wx * wx - wy * wy) / (2.0 * nw);
            let (cx, cy) = (mx + s * nx, my + s * ny);
            let r = (h * h + s * s).sqrt();
            let (ux, uy) = ((cx - v.x) / r, (cy - v.y) / r);
            let (d1, d2) = (ux * e1.0 + uy * e1.1, ux * e2.0 + uy * e2.1);
            // Both edges leave the vertex on the same side of the tangent:
            // away from the centre (the circle outside the corner) or toward
            // it (the circle around it).
            if (d1 <= EPS && d2 <= EPS) || (d1 >= -EPS && d2 >= -EPS) {
                at_vertices.push(arc(a, b, cx, cy, r, v.x, v.y, i));
            }
        }
    }
    let same = |p: &Arc, q: &Arc| (p.cx - q.cx).abs() < CHORD_TOL_MM && (p.cy - q.cy).abs() < CHORD_TOL_MM && (p.r - q.r).abs() < CHORD_TOL_MM;
    for v in at_vertices {
        if !out.iter().any(|o| same(o, &v)) {
            out.push(v);
        }
    }
    out.sort_by(|p, q| p.r.partial_cmp(&q.r).unwrap_or(std::cmp::Ordering::Equal));
    out.dedup_by(|p, q| (p.cx - q.cx).abs() < 1e-7 && (p.cy - q.cy).abs() < 1e-7 && (p.r - q.r).abs() < 1e-7);
    Ok(out)
}

/// The arc from `a` to `b` on the circle that passes THROUGH the touch:
/// the sense is whichever of the two arcs between `a` and `b` contains it.
#[allow(clippy::too_many_arguments)]
fn arc(a: Pt, b: Pt, cx: f64, cy: f64, r: f64, tx: f64, ty: f64, reference: usize) -> Arc {
    let ang = |x: f64, y: f64| (y - cy).patan2(x - cx);
    let (ta, tb, tt) = (ang(a.x, a.y), ang(b.x, b.y), ang(tx, ty));
    let ccw_span = (tb - ta).rem_euclid(std::f64::consts::TAU);
    let to_touch = (tt - ta).rem_euclid(std::f64::consts::TAU);
    Arc { cx, cy, r, tx, ty, ccw: to_touch <= ccw_span, reference }
}

impl Arc {
    /// The arc's points from the first point to the second inclusive,
    /// chorded so no chord's sagitta exceeds the one chord tolerance — the
    /// corpus's own authoring rule for a curve.
    pub fn points(&self, a: Pt, b: Pt) -> Vec<Pt> {
        let ang = |p: Pt| (p.y - self.cy).patan2(p.x - self.cx);
        let (ta, tb) = (ang(a), ang(b));
        let span = if self.ccw { (tb - ta).rem_euclid(std::f64::consts::TAU) } else { -((ta - tb).rem_euclid(std::f64::consts::TAU)) };
        let step_max = 2.0 * (1.0 - CHORD_TOL_MM / self.r).clamp(-1.0, 1.0).pacos();
        let n = ((span.abs() / step_max).ceil() as usize).max(1);
        let mut out = Vec::with_capacity(n + 1);
        out.push(a);
        for k in 1..n {
            let t = ta + span * k as f64 / n as f64;
            out.push(Pt { x: self.cx + self.r * t.pcos(), y: self.cy + self.r * t.psin() });
        }
        out.push(b);
        out
    }

    /// The angle the arc turns through, radians.
    pub fn sweep(&self, a: Pt, b: Pt) -> f64 {
        let ang = |p: Pt| (p.y - self.cy).patan2(p.x - self.cx);
        let (ta, tb) = (ang(a), ang(b));
        if self.ccw { (tb - ta).rem_euclid(std::f64::consts::TAU) } else { (ta - tb).rem_euclid(std::f64::consts::TAU) }
    }
}

/// The plan footprint of a declared feature — or of the blank, named
/// `stock` — as references, pushed out by `offset` (mm): a bore's or a
/// boss's circle grows by it; a profile's or face's edges move out by it
/// (the offsetting crate, every wall at once, the outer loop); the blank's
/// four sides move IN by it, since the material is inside them. A kind
/// with no plan footprint to offer declines by name.
pub fn references_of(p: &Part, name: &str, offset: f64) -> Result<Vec<Reference>, String> {
    if name == "stock" {
        let s = &p.stock;
        // A round blank's reference is its circle pulled IN by the offset
        // (Phase 46 slice 1) — the analytic primitive, never its chords.
        if let crate::parse::StockShape::Cylinder { dia } = s.shape {
            let (cx, cy) = s.axis();
            if dia / 2.0 - offset <= EPS {
                return Err(format!("an offset of {}mm leaves nothing of the stock", crate::gcode::f(offset)));
            }
            return Ok(vec![Reference::Circle { cx, cy, r: dia / 2.0 - offset }]);
        }
        let (x0, y0, x1, y1) = (s.x + offset, s.y + offset, s.x + s.w - offset, s.y + s.l - offset);
        if !(x0 < x1 && y0 < y1) {
            return Err(format!("an offset of {}mm leaves nothing of the stock", crate::gcode::f(offset)));
        }
        let c = |x: f64, y: f64| Pt { x, y };
        return Ok(vec![
            Reference::Segment(c(x0, y0), c(x1, y0)),
            Reference::Segment(c(x1, y0), c(x1, y1)),
            Reference::Segment(c(x1, y1), c(x0, y1)),
            Reference::Segment(c(x0, y1), c(x0, y0)),
        ]);
    }
    let f = p.features.get(name).ok_or_else(|| format!("no feature named '{name}' (and `stock` names the blank)"))?;
    let circle = |cx: f64, cy: f64, r: f64| Ok(vec![Reference::Circle { cx, cy, r: r + offset }]);
    let loop_refs = |points: &[Pt]| -> Result<Vec<Reference>, String> {
        let loops: Vec<Vec<Pt>> = if offset.abs() < EPS {
            vec![points.to_vec()]
        } else {
            // The crate's sign is inward-positive on a CCW loop; a reference
            // pushed OUT by a positive offset is the CCW loop offset by its
            // negative. The shoelace sign decides the winding, as
            // `ops::profile` decides it for a wire.
            let area: f64 = (0..points.len()).map(|i| {
                let (p, q) = (points[i], points[(i + 1) % points.len()]);
                p.x * q.y - q.x * p.y
            }).sum();
            let ccw: Vec<Pt> = if area < 0.0 { points.iter().rev().copied().collect() } else { points.to_vec() };
            crate::offset::offset_closed_polyline(&ccw, -offset)
        };
        let mut out = Vec::new();
        for lp in loops {
            for i in 0..lp.len() {
                out.push(Reference::Segment(lp[i], lp[(i + 1) % lp.len()]));
            }
        }
        if out.is_empty() {
            return Err(format!("an offset of {}mm leaves nothing of '{name}'", crate::gcode::f(offset)));
        }
        Ok(out)
    };
    match f {
        Feature::CylinderBore { dia, x, y, .. } | Feature::CylinderBoss { dia, x, y, .. } => circle(*x, *y, dia / 2.0),
        // The intruder with an axis (Phase 41 slice 5): its plan envelope
        // over the depth it spans, two lines and two half-ellipses, read as
        // the chorded loop every curved footprint in the corpus is — so the
        // elliptical caps are segments to this solve, and nothing here is
        // iterated (the DRAFT's fork 3, chorded).
        Feature::TiltedBore { dia, extent, x, y, tilt, heading } => {
            loop_refs(&crate::parse::derive_tilted_bore_envelope(*dia, *extent, *x, *y, *tilt, *heading))
        }
        Feature::Sphere { dia, x, y, .. }
        | Feature::Cone { dia, x, y, .. } => circle(*x, *y, dia / 2.0),
        Feature::Torus { major_dia, minor_dia, x, y, .. } => circle(*x, *y, (major_dia + minor_dia) / 2.0),
        Feature::ProfilePocket { points, .. } | Feature::ProfileBoss { points, .. } => loop_refs(points),
        Feature::Face { loops, .. } => loop_refs(&loops[0]),
        other => Err(format!("'{name}' is a {} — no plan footprint to build against yet", other.kind())),
    }
}

/// The construction's terminal voice: every solution named, then the
/// picked one's points as loop lines — the file's own shape, one point per
/// line, the first and last being the two points given.
pub fn report(p: &Part, a: Pt, b: Pt, reference: &str, offset: f64, pick: usize) -> Result<String, String> {
    let refs = references_of(p, reference, offset)?;
    let arcs = arcs_through(a, b, &refs)?;
    if arcs.is_empty() {
        return Err(format!(
            "no circle through ({}, {}) and ({}, {}) is tangent to '{reference}'{}",
            crate::gcode::f(a.x),
            crate::gcode::f(a.y),
            crate::gcode::f(b.x),
            crate::gcode::f(b.y),
            if offset.abs() < EPS { String::new() } else { format!(" pushed out {}mm", crate::gcode::f(offset)) }
        ));
    }
    if pick == 0 || pick > arcs.len() {
        return Err(format!("pick={pick} — there are {} arcs, numbered from 1", arcs.len()));
    }
    let mut out = String::new();
    out.push_str(&format!(
        "# {} arc{} through ({}, {}) and ({}, {}) tangent to '{reference}'{}:\n",
        arcs.len(),
        if arcs.len() == 1 { "" } else { "s" },
        crate::gcode::f(a.x),
        crate::gcode::f(a.y),
        crate::gcode::f(b.x),
        crate::gcode::f(b.y),
        if offset.abs() < EPS { String::new() } else { format!(" pushed out {}mm", crate::gcode::f(offset)) }
    ));
    for (i, arc) in arcs.iter().enumerate() {
        out.push_str(&format!(
            "#   [{}] R {} centre ({}, {}) touching at ({}, {}), {} {:.0}°{}\n",
            i + 1,
            crate::gcode::f(arc.r),
            crate::gcode::f(arc.cx),
            crate::gcode::f(arc.cy),
            crate::gcode::f(arc.tx),
            crate::gcode::f(arc.ty),
            if arc.ccw { "counter-clockwise" } else { "clockwise" },
            arc.sweep(a, b).to_degrees(),
            if i + 1 == pick { "  ← pick" } else { "" }
        ));
    }
    let arc = &arcs[pick - 1];
    let pts = arc.points(a, b);
    out.push_str(&format!("# [{pick}] as loop points, chorded to {}mm ({} points):\n", crate::gcode::f(CHORD_TOL_MM), pts.len()));
    for q in &pts {
        out.push_str(&format!("    {} {}\n", crate::gcode::f(q.x), crate::gcode::f(q.y)));
    }
    Ok(out)
}

/// The CLI's reading of `--arc`'s one argument: `x1,y1 x2,y2 tangent=<name>
/// [offset=<mm>] [pick=<n>]`.
pub fn parse_request(spec: &str) -> Result<(Pt, Pt, String, f64, usize), String> {
    let shape = "the shape: 'x1,y1 x2,y2 tangent=<feature|stock> [offset=<mm>] [pick=<n>]'";
    let mut toks = spec.split_whitespace();
    let point = |tok: Option<&str>| -> Result<Pt, String> {
        let t = tok.ok_or_else(|| format!("two points are needed — {shape}"))?;
        let (x, y) = t.split_once(',').ok_or_else(|| format!("'{t}' is not a point (x,y) — {shape}"))?;
        Ok(Pt { x: x.parse().map_err(|_| format!("'{x}' is not a number"))?, y: y.parse().map_err(|_| format!("'{y}' is not a number"))? })
    };
    let a = point(toks.next())?;
    let b = point(toks.next())?;
    let (mut reference, mut offset, mut pick) = (None, 0.0, 1usize);
    for tok in toks {
        let Some((k, v)) = tok.split_once('=') else { return Err(format!("'{tok}' is not key=value — {shape}")) };
        match k {
            "tangent" => reference = Some(v.to_string()),
            "offset" => offset = v.parse().map_err(|_| format!("offset '{v}' is not a number"))?,
            "pick" => pick = v.parse().map_err(|_| format!("pick '{v}' is not a count from 1"))?,
            _ => return Err(format!("'{k}' is not a key here — tangent, offset, pick")),
        }
    }
    let reference = reference.ok_or_else(|| format!("tangent=<feature|stock> is needed — {shape}"))?;
    Ok((a, b, reference, offset, pick))
}
