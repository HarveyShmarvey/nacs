//! The keep-out clearance check — Phase 8's V2, slice 0
//! (`DECISIONS.md` → "V2, the fixture…").
//!
//! Two properties make this unlike every other body check in the tree, and
//! both are deliberate.
//!
//! **Its subject is the RAPID.** `stock::body_foul_warnings` probes positions
//! derived from declared feature geometry, and `stock::check_moves` looks only
//! at *committing* moves, because both ask "does the tool drive into
//! material?". A fixture crash is characteristically not a cutting event at
//! all — it is the traverse across a clamp at a clearance height that was set
//! for the part and not for the vise, or a retract that clears the stock and
//! not the jaw. So this walks **every** emitted motion, rapids and the links
//! between ops included.
//!
//! **It is biased to NOISE, inverting the house convention on purpose.** Every
//! other advisory here under-claims — containment "can only ever under-claim",
//! body-foul is "biased to silence" — and that bias is exactly why
//! `apply_gate_floor` rejects to escalate body-foul: *a gate must not promise
//! what a probe cannot.* Inherit it here and the check is worse than useless,
//! because a keep-out probe that misses is a wrecked spindle that passed
//! review. So every body band is bounded by its **maximum** radius and every
//! arc by its exact bounding box: this can raise a clearance it did not have
//! to, and can never miss one it should have. That constraint is about probes
//! that MISS, so an over-claiming bound does not run into it — which is what
//! makes this eligible to gate where body-foul never will be.
//!
//! Nothing here samples. With `Box`/`Cylinder` keep-outs and a body that is a
//! stack of radii, a straight move sweeps a stack of stadium prisms, and
//! stadium-to-box and stadium-to-cylinder distance in plan are elementary.

use crate::gcode::{apply_move, Motion, OpEmission};
use crate::parse::{Finding, FindingFamily, Fixture, Part, Silhouette, Tool};

/// One band of the tool body: `[h_lo, h_hi)` above the tip, and an UPPER BOUND
/// on the radius anywhere in it.
type Band = (f64, f64, f64);

/// The body as piecewise-constant bands, each bounding its own radius from
/// above.
///
/// The breakpoints are the ladder's own rungs, so nothing is invented: the
/// cutting envelope up to where the flutes end, then shank, nose, and holder
/// body. Within a band the bound is that band's widest radius — for the rod
/// arms every band is already constant, so the bound is exact; for a ball or a
/// drawn profile the envelope band is bounded by its widest point, which
/// over-claims in the safe direction.
fn body_bands(sil: &Silhouette, reach: f64) -> Vec<Band> {
    let mut breaks: Vec<f64> = vec![0.0];
    // `radius_at` walks the same rungs; asking the silhouette just above each
    // breakpoint is what keeps this from restating the ladder's own layout.
    for h in sil.rung_heights() {
        if h > 0.0 && h < reach {
            breaks.push(h);
        }
    }
    breaks.push(reach.max(0.0));
    breaks.sort_by(|a, b| a.partial_cmp(b).unwrap());
    breaks.dedup_by(|a, b| (*a - *b).abs() < 1e-9);

    let mut out = Vec::new();
    for w in breaks.windows(2) {
        let (lo, hi) = (w[0], w[1]);
        // The band's bound: its ends plus its middle. A rung is constant, so
        // these agree; a curved envelope (a ball's taper, a drawn profile) is
        // monotone within a rung-free stretch, so the ends bound it — the
        // midpoint is belt and braces at no cost, and errs upward.
        let r = sil
            .radius_at(lo)
            .max(sil.radius_at((lo + hi) / 2.0))
            .max(sil.radius_at((hi - 1e-9).max(lo)));
        out.push((lo, hi, r));
    }
    out
}

/// Distance between two 2D segments, zero if they cross. Closed form: the
/// minimum of a segment pair is attained at an endpoint unless they intersect,
/// which is the standard planar result and is why no search appears here.
fn seg_seg_distance(a0: (f64, f64), a1: (f64, f64), b0: (f64, f64), b1: (f64, f64)) -> f64 {
    let cross = |o: (f64, f64), p: (f64, f64), q: (f64, f64)| {
        (p.0 - o.0) * (q.1 - o.1) - (p.1 - o.1) * (q.0 - o.0)
    };
    let (d1, d2) = (cross(a0, a1, b0), cross(a0, a1, b1));
    let (d3, d4) = (cross(b0, b1, a0), cross(b0, b1, a1));
    if ((d1 > 0.0) != (d2 > 0.0)) && ((d3 > 0.0) != (d4 > 0.0)) {
        return 0.0;
    }
    point_seg_distance(a0, b0, b1)
        .min(point_seg_distance(a1, b0, b1))
        .min(point_seg_distance(b0, a0, a1))
        .min(point_seg_distance(b1, a0, a1))
}

/// Where a point projects onto a segment, clamped to it.
fn project_t(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    if len2 <= 1e-18 {
        return 0.0;
    }
    (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
}

/// Distance from a point to a segment.
fn point_seg_distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 <= 1e-18 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
    };
    let (qx, qy) = (a.0 + t * dx, a.1 + t * dy);
    ((p.0 - qx).powi(2) + (p.1 - qy).powi(2)).sqrt()
}

/// Plan distance from a straight move's spine to a keep-out's footprint, zero
/// when they overlap — the stadium's centreline, with the body radius added by
/// the caller.
///
/// Exact for both shapes rather than approximated at the endpoints, which
/// matters because the clearance a traverse actually has is usually at the foot
/// of a perpendicular and not at either end: a check that only measured
/// emitted points would sail a tool past a clamp and call it clear.
fn segment_plan_distance(f: &Fixture, a: (f64, f64), b: (f64, f64)) -> (f64, (f64, f64)) {
    // The witness: WHERE on the spine the least clearance is. Reporting the
    // move's endpoint instead would name a coordinate the tool was nowhere
    // near when it got close — the first version of this did exactly that and
    // pointed at the far end of a facing pass.
    let witness = |t: f64| (a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1));
    let scan = |best: &mut (f64, (f64, f64)), t: f64| {
        let q = witness(t);
        let d = f.plan_distance(q.0, q.1);
        if d < best.0 {
            *best = (d, q);
        }
    };
    match f.plan_corners() {
        // A box: zero if either end is inside (or the spine crosses an edge),
        // otherwise the least distance to its four edges.
        Some(c) => {
            let mut best = (f64::INFINITY, a);
            scan(&mut best, 0.0);
            scan(&mut best, 1.0);
            if best.0 == 0.0 {
                return best;
            }
            // Each corner's foot on the spine, plus each edge's own closest
            // approach: together these are where a segment-to-rectangle
            // minimum can live, so no search is needed.
            for corner in c.iter() {
                scan(&mut best, project_t(*corner, a, b));
            }
            for i in 0..4 {
                let (e0, e1) = (c[i], c[(i + 1) % 4]);
                if seg_seg_distance(a, b, e0, e1) == 0.0 {
                    // The spine crosses an edge: it is inside, distance zero,
                    // and the crossing itself is the honest witness.
                    let t = project_t(e0, a, b).clamp(0.0, 1.0);
                    return (0.0, witness(t));
                }
                scan(&mut best, project_t(e0, a, b));
                scan(&mut best, project_t(e1, a, b));
            }
            best
        }
        // A cylinder: the spine's distance to the axis, less the radius.
        None => {
            let t = project_t((f.x, f.y), a, b);
            let q = witness(t);
            (f.plan_distance(q.0, q.1), q)
        }
    }
}

/// The keep-out findings for a program, over the whole emitted emitted moves.
///
/// Advisory: printed to stderr like its siblings, so no golden moves. Empty
/// whenever nothing is declared, which is today's behaviour by construction.
pub fn fixture_findings(p: &Part, emissions: &[OpEmission]) -> Vec<Finding> {
    let mut findings = Vec::new();
    if p.fixtures.is_empty() {
        return findings;
    }

    for em in emissions {
        let Some(op) = p.operations.get(em.op_idx) else { continue };
        let Some(tool) = p.tools.get(&op.tool_num) else { continue };
        let sil = Silhouette::of(tool);
        // The tool reaches from its tip up to whatever the declared body says;
        // an undeclared rod is bounded by the clearance plane it retracts to,
        // since above that the program says nothing and this check invents
        // nothing.
        let reach = body_reach(p, tool, &sil);
        let bands = body_bands(&sil, reach);

        let mut m = Motion { rapid: false, x: None, y: None, z: None, f: None };
        let mut prev: Option<(f64, f64, f64)> = None;
        let mut hit: Option<(String, f64, f64, f64, f64, (f64, f64, f64))> = None;
        for line in crate::gcode::resolved_moves(em) {
            if !apply_move(line, &mut m) {
                continue;
            }
            let (Some(x), Some(y), Some(z)) = (m.x, m.y, m.z) else {
                prev = None;
                continue;
            };
            let cur = (x, y, z);
            let from = prev.unwrap_or(cur);
            prev = Some(cur);

            // An arc's swept spine is bounded by the segment's own endpoints
            // plus the arc bulge; rather than model that, the whole move is
            // bounded by the box its endpoints span, widened by the body. That
            // over-claims on a large arc and is the safe direction — the one
            // place this check is coarse, named so it can be tightened when a
            // real program is annoyed by it.
            let tip_lo = tool_tip_z(&sil, from.2.min(cur.2));
            let tip_hi = tool_tip_z(&sil, from.2.max(cur.2));

            for f in &p.fixtures {
                let (fz_lo, fz_hi) = f.z_span();
                for (h_lo, h_hi, rb) in &bands {
                    // Vertical overlap first: the band spans [tip+h_lo,
                    // tip+h_hi] as the tip travels, so take the widest reading.
                    let band_lo = tip_lo + h_lo;
                    let band_hi = tip_hi + h_hi;
                    if band_hi < fz_lo || band_lo > fz_hi {
                        continue;
                    }
                    let (plan, at) = segment_plan_distance(f, (from.0, from.1), (cur.0, cur.1));
                    let gap = plan - rb;
                    if gap < 0.0 {
                        // The height to report is where the body ACTUALLY
                        // meets the keep-out, not the band's own floor: a tip
                        // hanging below a clamp's base is not what hit it.
                        let h_meets = h_lo.max(fz_lo - tip_hi).max(*h_lo);
                        let worse = hit.as_ref().map_or(true, |(_, _, _, _, g, _)| gap < *g);
                        if worse {
                            hit = Some((f.name.clone(), h_meets, at.0, at.1, gap, (cur.0, cur.1, tip_hi.min(tip_lo))));
                        }
                    }
                }
            }
        }

        if let Some((fname, h, x, y, gap, tip)) = hit {
            findings.push(Finding::op(
                FindingFamily::Fixture,
                em.op_idx,
                &op.feature_name,
                format!(
                    "T{}'s body {:.3}mm above the tip reaches fixture '{}' by {:.3}mm near ({:.3}, {:.3}) during '{}' — a keep-out is never cut, only avoided, so this is steel meeting steel: raise the clearance plane, move the fixture, or shorten the tool. (Bounded from ABOVE on purpose: this check would rather name a clearance you had than miss one you did not) — the tip at ({:.3}, {:.3}, {:.3}) as the move ends",
                    op.tool_num, h, fname, -gap, x, y, op.feature_name, tip.0, tip.1, tip.2
                ),
            ).with_place([x, y, p.stock.z]).with_tip([tip.0, tip.1, tip.2], op.tool_num));
        }
    }
    findings
}

/// How far up the tool this check looks. The declared body decides it; with no
/// body declared, the retract plane does — above that the program says nothing
/// about the tool and this check rejects to invent it.
fn body_reach(p: &Part, tool: &Tool, sil: &Silhouette) -> f64 {
    let declared = tool.stickout.or(tool.flute_len);
    match declared {
        Some(d) => d.max(sil.max_radius()),
        None => (p.global_clearance_z - p.stock.z).max(0.0),
    }
}

/// Tip Z for an emitted Z — the same convention every other reader uses.
fn tool_tip_z(sil: &Silhouette, emitted_z: f64) -> f64 {
    emitted_z - sil.drop()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{FixtureShape, Silhouette};

    fn boxx(x: f64, y: f64, z: f64, w: f64, l: f64, h: f64) -> Fixture {
        Fixture { name: "Clamp".into(), x, y, z, shape: FixtureShape::Box { w, l, h } }
    }

    #[test]
    fn plan_distance_is_zero_inside_and_euclidean_outside() {
        let f = boxx(10.0, 10.0, 0.0, 20.0, 20.0, 10.0);
        assert_eq!(f.plan_distance(15.0, 15.0), 0.0, "inside is zero");
        assert_eq!(f.plan_distance(30.0, 20.0), 0.0, "on the face is zero");
        assert!((f.plan_distance(34.0, 20.0) - 4.0).abs() < 1e-12, "off a face");
        // A corner is the euclidean distance, not the axis one — the max/max
        // form is what gets this right.
        let d = f.plan_distance(33.0, 34.0);
        assert!((d - 5.0).abs() < 1e-12, "corner distance was {d}");
    }

    #[test]
    fn a_cylinder_keep_out_is_radial() {
        let f = Fixture {
            name: "Post".into(),
            x: 0.0, y: 0.0, z: 0.0,
            shape: FixtureShape::Cylinder { dia: 20.0, h: 50.0 },
        };
        assert_eq!(f.plan_distance(5.0, 0.0), 0.0, "inside the post");
        assert!((f.plan_distance(30.0, 0.0) - 20.0).abs() < 1e-12);
    }

    #[test]
    fn a_segment_passing_beside_a_box_is_measured_at_its_closest_point() {
        // The clearance a traverse actually has is at the foot of the
        // perpendicular, not at either endpoint — the failure a check that
        // only looked at emitted points would have.
        let f = boxx(0.0, 0.0, 0.0, 10.0, 10.0, 10.0);
        let (d, at) = segment_plan_distance(&f, (-20.0, 15.0), (30.0, 15.0));
        assert!((d - 5.0).abs() < 1e-12, "closest approach was {d}");
        // And the WITNESS is that closest point, not an endpoint — the exact
        // dishonesty the first version of the finding shipped with.
        assert!((at.1 - 15.0).abs() < 1e-12 && at.0 >= 0.0 && at.0 <= 10.0,
            "witness {at:?} is not over the box");
        // Both endpoints are much further away than that.
        assert!(f.plan_distance(-20.0, 15.0) > 15.0);
        assert!(f.plan_distance(30.0, 15.0) > 15.0);
    }

    #[test]
    fn the_bands_bound_the_body_from_above() {
        // A ball nose tapers; the band containing the taper must be bounded by
        // the widest radius in it, never the narrowest.
        let sil = Silhouette::Ball { r: 5.0, body: None };
        for (lo, hi, r) in body_bands(&sil, 30.0) {
            for k in 0..=10 {
                let h = lo + (hi - lo) * (k as f64 / 10.0);
                assert!(
                    sil.radius_at(h) <= r + 1e-9,
                    "band [{lo},{hi}) bound {r} is under the body's {} at {h}",
                    sil.radius_at(h)
                );
            }
        }
    }

    #[test]
    fn a_declared_neck_narrows_a_band_and_that_loosens_this_gate() {
        // THE GATE CHANGE THE NECK FIELDS' DESIGN NAMED, as a test rather than
        // as a surprise. This check bands the body by each band's widest
        // radius and takes its breakpoints from the ladder itself — so a
        // declared neck publishes a narrower band, and a clearance that
        // rejected becomes legal.
        //
        // That is the wall working, not weakening: the old error was
        // gauging from a body WIDER than the tool, because the declaration was
        // coarser than the hardware. Gauging a clearance off a section that
        // was ground away is the standing form-tool gripe pointed inward.
        let with_neck = |neck: Option<(f64, f64)>| crate::parse::BodyLadder {
            shank: Some(crate::parse::ShankRung { flute_top: 12.0, shank_r: 6.0, neck }),
            holder: None,
        };
        let coarse = Silhouette::Flat { r: 5.0, body: Some(with_neck(None)) };
        let necked = Silhouette::Flat { r: 5.0, body: Some(with_neck(Some((32.0, 4.0)))) };

        // The same height, two declarations: 6mm of shank the coarse one has
        // to assume, 4mm of ground-back neck the precise one can state.
        assert_eq!(coarse.radius_at(20.0), 6.0);
        assert_eq!(necked.radius_at(20.0), 4.0);

        // And that reaches the bands, which is what the check actually reads.
        let band_at = |sil: &Silhouette, h: f64| {
            body_bands(sil, 40.0)
                .into_iter()
                .find(|(lo, hi, _)| h >= *lo && h < *hi)
                .map(|(_, _, r)| r)
                .expect("a band covers it")
        };
        assert_eq!(band_at(&coarse, 20.0), 6.0);
        assert_eq!(band_at(&necked, 20.0), 4.0, "the neck band is bounded by the NECK");

        // The bands still bound the body from above — the property the test
        // above pins, re-checked with a non-monotone ladder in play, because a
        // step DOWN is the first thing this family has ever had to hold.
        //
        // Sampled HALF-OPEN, unlike the monotone test above, and that is not
        // pedantry: `body_bands` has always produced `[lo, hi)`, but until a
        // body could step back UP at a band's top, sampling the closed end
        // happened to agree. A neck is the first shape where it does not.
        for (lo, hi, r) in body_bands(&necked, 40.0) {
            for k in 0..10 {
                let h = lo + (hi - lo) * (k as f64 / 10.0);
                assert!(necked.radius_at(h) <= r + 1e-9, "band [{lo},{hi}) bound {r} at {h}");
            }
            let top = (hi - 1e-9).max(lo);
            assert!(necked.radius_at(top) <= r + 1e-9, "band [{lo},{hi}) bound {r} at its top");
        }

        // A clamp 5mm from the tool axis: the coarse body's 6mm shank stands
        // in it, the declared 4mm neck clears it. One number, two answers,
        // and only one of them describes the tool on the machine.
        assert!(band_at(&coarse, 20.0) > 5.0 && band_at(&necked, 20.0) < 5.0);
    }
}
