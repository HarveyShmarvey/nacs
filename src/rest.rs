use crate::math::Portable as _;
// ---- the rest floor: what an earlier op conceded, as floors of their own ----
// Phase 33 slice 1 (`DECISIONS.md` → "Phase 33 slice 1: the rest floor…").
// A `Floor` with `strategy=rest` cuts only the floor an earlier op left
// standing — the stretches its concession named — instead of re-facing the
// whole floor to reach them. Nothing here is declared: the stretches are the
// stock model's own hold-back (`stock::conceded_holes`), each one less what
// stands becomes a floor with the host's height, and every reader — the
// emitter, the removal, the entry check's overhang — takes those floors
// through the machinery it already has, as the `Face` each one is.

use crate::parse::{Feature, Operation, Part, Pt};

/// A stretch piece smaller than this is where a stretch's outline and a
/// standing outline disagree by their chording — `cover`'s own threshold.
const LEAST_PIECE: f64 = crate::cover::LEAST_STRETCH;

/// This op's place in the program: by identity, or — for a copy a consumer
/// made of it — the first op on the same floor with the same tool and word.
pub(crate) fn index_of(p: &Part, op: &Operation) -> Option<usize> {
    p.operations.iter().position(|o| std::ptr::eq(o, op)).or_else(|| {
        p.operations
            .iter()
            .position(|o| o.feature_name == op.feature_name && o.tool_num == op.tool_num && o.op_type == op.op_type && o.strategy == op.strategy)
    })
}

/// Does this op cut the rest floor?
pub fn is_rest(op: &Operation) -> bool {
    op.op_type == crate::parse::OpType::End && op.strategy == Some(crate::parse::ClearStrategy::Rest)
}

/// What stands on this op's floor before it runs: the stretches the most
/// recent EARLIER op on the same floor held back. That op is the one that
/// last worked it, so what it conceded is what is left — and when it was a
/// rest op itself, what IT conceded (the recursion needs no rule of its own).
pub fn standing(p: &Part, op: &Operation) -> Result<(usize, Vec<Vec<Pt>>), String> {
    let idx = index_of(p, op).expect("the op is in the program");
    let before = p.operations[..idx]
        .iter()
        .enumerate()
        .rev()
        .find(|(_, o)| o.feature_name == op.feature_name && crate::region::of(p, o).is_some());
    let Some((k, prev)) = before else {
        return Err(format!(
            "End on '{}' with T{}: strategy=rest cuts what an earlier op left standing on this floor, and no earlier op works it — drop strategy=rest to face the whole floor",
            op.feature_name, op.tool_num
        ));
    };
    let held = crate::stock::conceded_holes(p, prev);
    if held.is_empty() {
        return Err(format!(
            "End on '{}' with T{}: strategy=rest — op [{}] ({} with T{}) left none of this floor standing, so there is nothing to cut. Drop this op, or drop strategy=rest to face the floor again",
            op.feature_name,
            op.tool_num,
            k,
            crate::stock::op_word(prev),
            prev.tool_num
        ));
    }
    Ok((k, held))
}

/// One floor of the rest: the part's copy of the host replaced by a `Face`
/// at the host's height whose loops are this piece, the op's copy without
/// the word (it faces that `Face` as it faces any), and what the op's tool
/// reaches of it.
pub struct Piece {
    pub part: Part,
    pub op: Operation,
    pub coverage: crate::cover::Coverage,
    /// Whether the tool's centre fits anywhere in it — the offset family's
    /// allowed region, the one home for "can a pass be laid here".
    cut: bool,
}

impl Piece {
    /// A piece no pass enters at all is conceded whole: nothing of it is
    /// cut, nothing of it is claimed, and it is held back exactly as drawn.
    pub fn is_cut(&self) -> bool {
        self.cut
    }

    pub fn outline(&self) -> &[Pt] {
        match self.part.features.get(&self.op.feature_name) {
            Some(Feature::Face { loops, .. }) => &loops[0],
            _ => unreachable!("rest::pieces makes Faces"),
        }
    }
}

/// The rest floor as floors, one `Piece` per piece. Edges against what
/// stands are WALLs because a declared neighbour stands beyond them (Phase
/// 29's derivation); edges against the earlier op's cut are OPEN because
/// nothing does — the stock model says it is gone.
///
/// `cover=` on a rest op (slice 3) is a share of the WHOLE rest floor, judged
/// once by `concession` below, so each piece is faced as if it had conceded
/// whatever it cannot reach (`cover=0`): the note and the check are the op's,
/// never a piece's.
pub fn pieces(p: &Part, op: &Operation) -> Result<Vec<Piece>, String> {
    let (_, held) = standing(p, op)?;
    let host = p.features.get(&op.feature_name).expect("validate: the feature exists");
    let (z, floor, voids) = match host {
        Feature::Face { z, loops } => (*z, loops[0].clone(), loops[1..].to_vec()),
        Feature::Plane { z } => (*z, crate::region::blank_of(p), Vec::new()),
        _ => unreachable!("validate: strategy=rest faces a Face or a Plane"),
    };
    let r = p.tools.get(&op.tool_num).expect("validate: the tool exists").dia / 2.0;
    // What stands on the floor: every declared neighbour above it, and the
    // host's own inner loops — occupied or void, neither is floor.
    let mut stands: Vec<Vec<Pt>> = crate::stock::declared_neighbours_above(p, &op.feature_name, z + 1e-3).into_iter().map(|(_, o)| o).collect();
    stands.extend(voids);
    let mut out = Vec::new();
    for outline in &held {
        for (outer, holes) in less_standing(crate::offset::clip_to(outline, &floor), &stands) {
            let mut loops = vec![simplified(&outer)];
            loops.extend(holes.into_iter().map(|h| cw(&h)));
            let mut q = p.clone();
            q.features.insert(op.feature_name.clone(), Feature::Face { z, loops });
            let mut o = op.clone();
            o.strategy = None;
            o.cover = Some(0.0);
            let region = crate::region::of(&q, &o).ok_or_else(|| format!("End on '{}': a rest floor lies outside the stock", op.feature_name))?;
            let mut coverage = crate::ops::clear::coverage(&q, &o, &region, r, 0.0)?;
            // Cut when a pass fits AND reaches floor: a 5mm tool fits in the
            // air past a 3mm slot's mouth and reaches nothing of the slot,
            // and the passes it would lay there are air. (The grid counts
            // specks under half a square millimetre as reached; with nothing
            // really reached, none of it is.)
            let fits = !crate::ops::clear::allowed_loops(&q, &o, &region, r, 0.0)?.0.is_empty();
            let cut = fits && coverage.reachable_mm2 >= LEAST_PIECE;
            if !cut {
                coverage.reachable_mm2 = 0.0;
            }
            out.push(Piece { part: q, op: o, coverage, cut });
        }
    }
    Ok(out)
}

/// The rest floor's coverage, whole: every piece's floor and reach summed,
/// every stretch listed, largest first.
pub fn coverage(pieces: &[Piece]) -> crate::cover::Coverage {
    let mut stretches: Vec<crate::cover::Stretch> = pieces.iter().flat_map(|pc| pc.coverage.stretches.iter().cloned()).collect();
    stretches.sort_by(|a, b| b.area_mm2.partial_cmp(&a.area_mm2).unwrap());
    crate::cover::Coverage {
        floor_mm2: pieces.iter().map(|pc| pc.coverage.floor_mm2).sum(),
        reachable_mm2: pieces.iter().map(|pc| pc.coverage.reachable_mm2).sum(),
        stretches,
    }
}

/// What the rest op holds back (slice 3, the recursion): a cut piece's own
/// stretches, and a piece no pass enters, whole.
pub fn held_back(pieces: &[Piece]) -> Vec<Vec<Pt>> {
    pieces
        .iter()
        .flat_map(|pc| if pc.is_cut() { pc.coverage.stretches.iter().map(|s| s.outline.clone()).collect() } else { vec![pc.outline().to_vec()] })
        .collect()
}

/// The rest op's concession, judged over its whole floor: the refusal naming
/// every stretch it cannot reach and the op that would (slice 2), or the
/// note `cover=` earns and the line after it.
pub fn concession(p: &Part, op: &Operation, pieces: &[Piece]) -> Result<Vec<String>, String> {
    let c = coverage(pieces);
    if c.stretches.is_empty() {
        return Ok(Vec::new());
    }
    let next = crate::cover::next_step(p, op, &c);
    let dia = p.tools.get(&op.tool_num).expect("validate: the tool exists").dia;
    match op.cover {
        None => Err(crate::cover::refusal(op, dia, &c, next.as_ref())),
        Some(asked) => {
            let mut notes = vec![crate::cover::concession_note(op, asked, &c)?];
            notes.extend(next.map(|n| n.note()));
            Ok(notes)
        }
    }
}

/// The `--outline` line: what the rest op works, or why it works nothing.
pub fn line(p: &Part, op: &Operation) -> String {
    let (k, pieces) = match standing(p, op).and_then(|(k, _)| Ok((k, pieces(p, op)?))) {
        Ok(v) => v,
        Err(e) => return format!("rest floor: none — {e}"),
    };
    let area: f64 = pieces.iter().map(|pc| pc.coverage.floor_mm2).sum();
    format!(
        "rest floor: {} floor{} op [{}] left standing, {:.1}mm²",
        pieces.len(),
        if pieces.len() == 1 { "" } else { "s" },
        k,
        area
    )
}

/// Each piece less every standing outline: a cut where one crosses it, a hole
/// where one stands wholly inside it, and slivers dropped. Judged by the AREA
/// they share, not by crossings: a traced stretch meets a fin exactly at the
/// fin's corner, which no proper-crossing test sees.
fn less_standing(pieces: Vec<Vec<Pt>>, stands: &[Vec<Pt>]) -> Vec<(Vec<Pt>, Vec<Vec<Pt>>)> {
    let shared = |a: &[Pt], b: &[Pt]| crate::offset::clip_to(a, b).iter().map(|l| area(l)).sum::<f64>();
    let mut out: Vec<(Vec<Pt>, Vec<Vec<Pt>>)> = pieces.into_iter().map(|pc| (pc, Vec::new())).collect();
    for s in stands {
        out = out
            .into_iter()
            .flat_map(|(pc, holes)| {
                let over = shared(s, &pc);
                if over < LEAST_PIECE {
                    return vec![(pc, holes)];
                }
                if over >= area(&pc) - LEAST_PIECE {
                    return Vec::new(); // the piece stands under it whole
                }
                let cut = crate::offset::subtract_all(&pc, s);
                if cut.len() == 1 && (area(&cut[0]) - area(&pc)).abs() < LEAST_PIECE {
                    // Nothing came off, yet they share its area: it stands
                    // wholly inside the piece — a hole.
                    let mut holes = holes;
                    holes.push(s.clone());
                    return vec![(pc, holes)];
                }
                cut.into_iter()
                    .map(|q| {
                        let kept = holes.iter().filter(|h| shared(h, &q) > LEAST_PIECE).cloned().collect();
                        (q, kept)
                    })
                    .collect()
            })
            .collect();
    }
    out.retain(|(pc, _)| area(pc) >= LEAST_PIECE);
    out
}

/// A traced outline, thinned to the one geometric tolerance: the marching
/// squares hand back a vertex every fifth of a millimetre, many of them
/// nearly collinear, and the offsetter splits the OUTWARD offset of such a
/// loop into pieces — the swath of the gap between the Disc and WallWest
/// came back as two loops and the larger missed the pinch. Douglas–Peucker at
/// `CHORD_TOL_MM`, on a closed loop split at its two farthest-apart vertices.
fn simplified(l: &[Pt]) -> Vec<Pt> {
    fn dp(pts: &[Pt], tol: f64, out: &mut Vec<Pt>) {
        let (a, b) = (pts[0], pts[pts.len() - 1]);
        let (k, d) = pts[1..pts.len() - 1]
            .iter()
            .enumerate()
            .map(|(i, q)| (i + 1, crate::stock::dist_to_polygon(&[a, b], q.x, q.y)))
            .fold((0, 0.0), |m, x| if x.1 > m.1 { x } else { m });
        if d > tol {
            dp(&pts[..=k], tol, out);
            dp(&pts[k..], tol, out);
        } else {
            out.push(a);
        }
    }
    if l.len() < 4 {
        return l.to_vec();
    }
    let far = (1..l.len()).max_by(|&i, &j| {
        let d = |k: usize| (l[k].x - l[0].x).phypot(l[k].y - l[0].y);
        d(i).partial_cmp(&d(j)).unwrap()
    }).unwrap();
    let mut out = Vec::new();
    let mut ring: Vec<Pt> = l.to_vec();
    ring.push(l[0]);
    dp(&ring[..=far], crate::CHORD_TOL_MM, &mut out);
    dp(&ring[far..], crate::CHORD_TOL_MM, &mut out);
    out
}

fn area(l: &[Pt]) -> f64 {
    (0..l.len()).map(|i| l[i].x * l[(i + 1) % l.len()].y - l[(i + 1) % l.len()].x * l[i].y).sum::<f64>().abs() / 2.0
}

fn cw(l: &[Pt]) -> Vec<Pt> {
    let mut v = crate::ops::clear::make_ccw(l);
    v.reverse();
    v
}
