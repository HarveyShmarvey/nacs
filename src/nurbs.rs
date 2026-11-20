// ---- nurbs: analytic b-spline meridian for a surface of revolution ----------
// The *analytic* NURBS reader (ROADMAP Phase 4 follow-up; DECISIONS.md "Analytic
// spline-of-revolution reader…"). The complement of the camp-3 mesh path: where
// `import_step_mesh.py` tessellates a freeform revolved face to a triangle soup
// and accepts the stair-step error, this reads the face's b-spline *meridian*
// as an exact curve and machines the surface of revolution through the R2
// pointwise-contact machinery (ops::finish::finish_spline_revolve) with NO
// tessellation error — the same regime as the catenoid / pseudosphere / vase,
// with the named analytic meridian generalized to an arbitrary (rational)
// b-spline.
//
// A meridian is a curve (r, z) in the half-plane, revolved about the feature's
// vertical axis. Like STL vertex data (see mesh.rs), the curve's control-point
// coordinates are defined to be millimetres — the parse layer scales a
// SplineRevolve feature's *placement* offsets with the project units but never
// the meridian itself.
//
// std-only, no crate: de Boor evaluation plus derivatives via the derivative
// control-point recurrence (a b-spline of degree p differentiates to one of
// degree p-1 with poles p·ΔP/Δknot), then the rational quotient rule. Chosen
// over Piegl & Tiller's DersBasisFuns for the same reason mesh.rs hand-rolls
// its STL reader: fewer moving parts to get an index wrong in.
//
// The module also carries `BSplineSurface` — the general (non-revolved) NURBS
// patch (DECISIONS.md "the general NURBS patch…", slice 1): a tensor-product
// (rational) b-spline surface in R³, camp 3's analytic input. It shares the
// same de Boor core (the evaluators below are generic over the homogeneous
// point width: 3 for the meridian's (r, z, w), 4 for the patch's (x, y, z, w)),
// and machines through ops::finish's raster drop-cutter — NOT the R2 revolved
// family, which is defined by its waterline circle. Patch control points are
// mm by definition, exactly like the meridian's.

/// A (rational) b-spline curve in the meridian half-plane: control points are
/// (r, z) in millimetres, `weights` is per-pole (all 1.0 for a non-rational
/// b-spline), `knots` is the full (flattened) clamped knot vector of length
/// `poles.len() + degree + 1`.
#[derive(Clone, Debug, PartialEq)]
pub struct BSplineCurve {
    pub degree: usize,
    pub knots: Vec<f64>,
    pub poles: Vec<(f64, f64)>,
    pub weights: Vec<f64>,
}

impl BSplineCurve {
    /// Build and validate a curve. `weights` = None means non-rational (all
    /// weights 1). Enforces the structural invariants a well-formed b-spline
    /// needs so evaluation can never index out of range or divide by a
    /// zero-width knot span it shouldn't.
    pub fn new(
        degree: usize,
        knots: Vec<f64>,
        poles: Vec<(f64, f64)>,
        weights: Option<Vec<f64>>,
    ) -> Result<Self, String> {
        if degree < 1 {
            return Err(format!("b-spline degree must be >= 1, got {}", degree));
        }
        if poles.len() < degree + 1 {
            return Err(format!(
                "b-spline of degree {} needs at least {} control points, got {}",
                degree,
                degree + 1,
                poles.len()
            ));
        }
        let expected = poles.len() + degree + 1;
        if knots.len() != expected {
            return Err(format!(
                "b-spline knot vector must have {} entries (poles {} + degree {} + 1), got {}",
                expected,
                poles.len(),
                degree,
                knots.len()
            ));
        }
        for w in knots.windows(2) {
            if w[1] < w[0] {
                return Err(format!(
                    "b-spline knot vector must be non-decreasing, found {} after {}",
                    w[1], w[0]
                ));
            }
        }
        if knots[knots.len() - 1 - degree] <= knots[degree] {
            return Err("b-spline knot vector spans an empty parameter domain".to_string());
        }
        let weights = match weights {
            None => vec![1.0; poles.len()],
            Some(w) => {
                if w.len() != poles.len() {
                    return Err(format!(
                        "b-spline has {} weights but {} control points",
                        w.len(),
                        poles.len()
                    ));
                }
                if let Some(bad) = w.iter().find(|&&x| !(x > 0.0)) {
                    return Err(format!("b-spline weights must be positive, found {}", bad));
                }
                w
            }
        };
        Ok(BSplineCurve { degree, knots, poles, weights })
    }

    /// The clamped parameter domain [u0, u1] the curve is defined over.
    pub fn domain(&self) -> (f64, f64) {
        (self.knots[self.degree], self.knots[self.knots.len() - 1 - self.degree])
    }

    /// The distinct knot values across the domain, u0 and u1 included — one
    /// entry per polynomial-span boundary. The guard scans sample per span from
    /// this, so their density is tied to the curve's own complexity: refining
    /// the knot vector refines the scan with it, and a many-knot meridian
    /// cannot hide a feature between samples the way a fixed whole-domain
    /// count could.
    pub fn distinct_knots(&self) -> Vec<f64> {
        distinct_knots_of(self.degree, &self.knots)
    }

    /// Signed curvature at u, oriented so a **positive** value means the
    /// meridian is concave toward the up-facing (tool-side) normal — the
    /// orientation the finishing CL offset uses. The tangent-rotated normal
    /// N_plus = (−z', r')/|C'| is that outward normal up to the nz >= 0 flip
    /// the CL offset applies (N_plus.z = r'/|C'| < 0 would be flipped upward),
    /// so the plain signed curvature (r'·z'' − z'·r'')/|C'|³ is corrected by
    /// the same flip. Returns None where the parameter speed vanishes (a fully
    /// repeated pole) and no tangent direction exists. One home for the sign
    /// convention: the gouge guard and the scallop march both read this.
    ///
    /// This is the CAP's home. The funnel reads
    /// `curvature_toward_void_normal` below: on a mouth-first cavity meridian
    /// (r' <= 0) the two agree wherever r' < 0 (the flip here fires and lands
    /// on the same sign), but on a vertical wall stretch (r' == 0, legal in
    /// both envelopes) the nz >= 0 tie-break cannot distinguish the sides and
    /// resolves the CAP's way — so the cavity gets its own oriented home
    /// rather than a convention that is right except on the walls.
    pub fn curvature_toward_up_normal(&self, u: f64) -> Option<f64> {
        let kappa = self.signed_curvature(u)?;
        let (_, (dr, _)) = self.eval_d1(u);
        Some(if dr < 0.0 { -kappa } else { kappa })
    }

    /// The same signed curvature read against the funnel's into-void normal
    /// (parse::spline_revolve_funnel_cl_point). That normal is the plain left
    /// normal negated, so the oriented curvature is the plain signed curvature
    /// negated — unconditionally, no tie-break: a hemispherical bowl's plain
    /// −1/R becomes +1/R (positive = concave toward the descending tool, the
    /// same reading as the cap's convention), and it stays exact on a
    /// vertical wall stretch where the cap rule's flip would not fire. One
    /// home for the funnel side: its gouge guard and scallop march both read
    /// this.
    pub fn curvature_toward_void_normal(&self, u: f64) -> Option<f64> {
        Some(-self.signed_curvature(u)?)
    }

    // The plain signed curvature κ = (r'·z'' − z'·r'')/|C'|³ — positive
    // toward the left normal (−z', r')/|C'|. The single formula both oriented
    // homes above apply their side's sign to. None where the parameter speed
    // vanishes (a fully repeated pole) and no tangent direction exists.
    fn signed_curvature(&self, u: f64) -> Option<f64> {
        let (_, (dr, dz), (d2r, d2z)) = self.eval_d2(u);
        let speed = (dr * dr + dz * dz).sqrt();
        if speed < 1e-9 {
            return None;
        }
        Some((dr * d2z - dz * d2r) / speed.powi(3))
    }

    /// The point C(u) = (r, z) on the meridian.
    pub fn eval(&self, u: f64) -> (f64, f64) {
        let d = self.rational_derivs(u, 0);
        (d[0].0, d[0].1)
    }

    /// The point and first derivative: (C, C') where C' = (dr/du, dz/du).
    pub fn eval_d1(&self, u: f64) -> ((f64, f64), (f64, f64)) {
        let d = self.rational_derivs(u, 1);
        (d[0], d[1])
    }

    /// The point and first two derivatives: (C, C', C''). C'' powers the
    /// curvature the finishing gouge guard reads.
    pub fn eval_d2(&self, u: f64) -> ((f64, f64), (f64, f64), (f64, f64)) {
        let d = self.rational_derivs(u, 2);
        (d[0], d[1], d[2])
    }

    /// Reject a meridian a top-down ball finish cannot reach as a cap. The
    /// machinable envelope (the same one the sphere / paraboloid / vase share)
    /// is: stored **apex-first**, z non-increasing and r non-decreasing from
    /// the first pole (apex, near the axis, at the top) to the last (base rim).
    /// r turning back inward as z descends is an *undercut* — a wider ring
    /// above shadows it from a tool coming down the axis — and z reversing means
    /// the surface is not single-valued in height; both are rejected here rather
    /// than machined into a gouging or self-shadowing path. The reader emits the
    /// canonical apex-first order, so a reversed curve is an authoring error and
    /// says so.
    ///
    /// Two tiers. A meridian whose *control polygon* is monotone the right way
    /// is accepted exactly, no sampling: with the constructor's positive
    /// weights the variation-diminishing property holds — the curve crosses any
    /// line r = const (or z = const) no more often than its polygon does, so a
    /// monotone polygon PROVES a monotone curve. Only a wiggly-polygon curve
    /// (whose curve may still be monotone — the polygon exaggerates) falls to
    /// the dense scan, which samples per knot span so knot refinement cannot
    /// hide a reversal between samples; that accept is a dense check, not a
    /// proof, and the doc says so honestly.
    pub fn check_machinable_cap(&self, feature_name: &str) -> Result<(), String> {
        // A small tolerance absorbs spline wiggle at the float level without
        // letting a real reversal through.
        let tol = 1e-6;

        // Tier 1: the exact accept.
        if self
            .poles
            .windows(2)
            .all(|w| w[1].0 >= w[0].0 - tol && w[1].1 <= w[0].1 + tol)
        {
            return Ok(());
        }

        // Tier 2: dense per-span scan.
        const SAMPLES_PER_SPAN: usize = 200;
        let spans = self.distinct_knots();
        let (mut pr, mut pz) = self.eval(spans[0]);
        for pair in spans.windows(2) {
            for k in 1..=SAMPLES_PER_SPAN {
                let u = pair[0] + (pair[1] - pair[0]) * (k as f64 / SAMPLES_PER_SPAN as f64);
                let (r, z) = self.eval(u);
                if z > pz + tol {
                    return Err(format!(
                        "SplineRevolve feature '{}': the meridian rises (z increases from {:.4} to {:.4}) — a surface of revolution machined top-down must descend from apex to base; store the meridian apex-first (apex on the axis, at the top)",
                        feature_name, pz, z
                    ));
                }
                if r < pr - tol {
                    return Err(format!(
                        "SplineRevolve feature '{}': the meridian turns back inward (radius shrinks from {:.4} to {:.4} as it descends) — that is an undercut a tool coming down the axis cannot reach; the machinable envelope is radius non-decreasing from apex to base",
                        feature_name, pr, r
                    ));
                }
                pr = r;
                pz = z;
            }
        }
        Ok(())
    }

    /// Reject a meridian a top-down ball finish cannot reach as a cavity — the
    /// mirror of `check_machinable_cap`. The funnel envelope is: stored
    /// **mouth-first**, z non-increasing and r non-increasing from the first
    /// pole (the mouth rim, widest, at the top) to the last (the tip). r
    /// growing back outward as z descends is the cavity's *undercut* — the
    /// narrower rim above shadows it from a tool coming down the axis — and z
    /// reversing again means not single-valued in height. The two envelopes
    /// are provably disjoint on any curve with depth (a cap widens descending,
    /// a funnel narrows), so which validator runs is the feature variant's
    /// choice, never a heuristic.
    ///
    /// Two cavity-specific preconditions come first: the mouth must be an open
    /// circle (first pole off the axis — a cavity is entered through its
    /// mouth), and the meridian must actually descend (a zero-depth cavity has
    /// nothing to machine). Then the same two tiers as the cap: an exact
    /// variation-diminishing accept when the control polygon is monotone the
    /// right way, else the dense per-knot-span scan (a dense check, not a
    /// proof — same honesty note as the cap's).
    pub fn check_machinable_funnel(&self, feature_name: &str) -> Result<(), String> {
        let tol = 1e-6;

        let (r_mouth, z_mouth) = self.eval(self.domain().0);
        let (_, z_tip) = self.eval(self.domain().1);
        if r_mouth <= tol {
            return Err(format!(
                "SplineRevolveFunnel feature '{}': the meridian's first pole sits on the axis — a cavity needs an open mouth circle of positive radius, stored mouth-first (mouth rim widest, at the top)",
                feature_name
            ));
        }
        if z_mouth - z_tip <= tol {
            return Err(format!(
                "SplineRevolveFunnel feature '{}': the meridian has no depth (z spans {:.4} to {:.4}) — a zero-depth cavity has nothing to machine",
                feature_name, z_mouth, z_tip
            ));
        }

        // Tier 1: the exact accept (variation-diminishing, as in the cap).
        if self
            .poles
            .windows(2)
            .all(|w| w[1].0 <= w[0].0 + tol && w[1].1 <= w[0].1 + tol)
        {
            return Ok(());
        }

        // Tier 2: dense per-span scan.
        const SAMPLES_PER_SPAN: usize = 200;
        let spans = self.distinct_knots();
        let (mut pr, mut pz) = self.eval(spans[0]);
        for pair in spans.windows(2) {
            for k in 1..=SAMPLES_PER_SPAN {
                let u = pair[0] + (pair[1] - pair[0]) * (k as f64 / SAMPLES_PER_SPAN as f64);
                let (r, z) = self.eval(u);
                if z > pz + tol {
                    return Err(format!(
                        "SplineRevolveFunnel feature '{}': the meridian rises (z increases from {:.4} to {:.4}) — a cavity machined top-down must descend from mouth to tip; store the meridian mouth-first (mouth rim widest, at the top)",
                        feature_name, pz, z
                    ));
                }
                if r > pr + tol {
                    return Err(format!(
                        "SplineRevolveFunnel feature '{}': the meridian widens (radius grows from {:.4} to {:.4} as it descends) — a cavity that widens below its mouth is an undercut the narrower rim above shadows from a tool coming down the axis; the machinable envelope is radius non-increasing from mouth to tip",
                        feature_name, pr, r
                    ));
                }
                pr = r;
                pz = z;
            }
        }
        Ok(())
    }

    // Rational curve derivatives up to order `n` (n <= 2), dehomogenized. The
    // homogeneous curve Cw(u) = (w·r, w·z, w) is a non-rational b-spline in R^3;
    // its derivatives Aders[k] come from the derivative control-point recurrence,
    // and the rational quotient rule turns them into the (r, z) derivatives:
    //   C   = A / w
    //   C'  = (A'  - w'·C) / w
    //   C'' = (A'' - 2·w'·C' - w''·C) / w
    // (Piegl & Tiller A4.2 specialized to k <= 2.)
    fn rational_derivs(&self, u: f64, n: usize) -> Vec<(f64, f64)> {
        // Homogeneous control points (w·r, w·z, w).
        let cptsw: Vec<[f64; 3]> = self
            .poles
            .iter()
            .zip(&self.weights)
            .map(|(&(r, z), &w)| [w * r, w * z, w])
            .collect();

        let aders = homo_derivs(self.degree, &self.knots, &cptsw, u, n);

        let w0 = aders[0][2];
        let c0 = (aders[0][0] / w0, aders[0][1] / w0);
        let mut out = vec![c0];
        if n >= 1 {
            let w1 = aders[1][2];
            let c1 = (
                (aders[1][0] - w1 * c0.0) / w0,
                (aders[1][1] - w1 * c0.1) / w0,
            );
            out.push(c1);
            if n >= 2 {
                let w2 = aders[2][2];
                let c2 = (
                    (aders[2][0] - 2.0 * w1 * c1.0 - w2 * c0.0) / w0,
                    (aders[2][1] - 2.0 * w1 * c1.1 - w2 * c0.1) / w0,
                );
                out.push(c2);
            }
        }
        out
    }
}

// The distinct knot values across a clamped knot vector's domain, endpoints
// included — the span-boundary list both the curve's and the surface's
// per-span scans read (one implementation, two callers).
fn distinct_knots_of(degree: usize, knots: &[f64]) -> Vec<f64> {
    let (u0, u1) = (knots[degree], knots[knots.len() - 1 - degree]);
    let tol = (u1 - u0) * 1e-12;
    let mut out = vec![u0];
    for &k in &knots[degree..=knots.len() - 1 - degree] {
        if k > *out.last().unwrap() + tol && k <= u1 {
            out.push(k);
        }
    }
    out
}

// Derivatives 0..=n of a homogeneous b-spline curve at u — the shared loop
// under both the meridian's rational_derivs (width 3) and the surface's
// tensor evaluation (width 4): repeatedly form the derivative curve via the
// control-point recurrence and de Boor it.
fn homo_derivs<const N: usize>(
    degree: usize,
    knots: &[f64],
    cpts: &[[f64; N]],
    u: f64,
    n: usize,
) -> Vec<[f64; N]> {
    let mut aders: Vec<[f64; N]> = Vec::with_capacity(n + 1);
    let mut degree = degree;
    let mut knots = knots.to_vec();
    let mut cpts = cpts.to_vec();
    for k in 0..=n {
        if k == 0 {
            aders.push(deboor(degree, &knots, &cpts, u));
        } else if degree == 0 {
            // Differentiating past the polynomial degree gives zero.
            aders.push([0.0; N]);
        } else {
            let (dq, dk, dc) = derivative_cpts(degree, &knots, &cpts);
            degree = dq;
            knots = dk;
            cpts = dc;
            aders.push(deboor(degree, &knots, &cpts, u));
        }
    }
    aders
}

// The knot span index i such that knots[i] <= u < knots[i+1], clamped to the
// valid control-point range [degree, n_last] (Piegl & Tiller A2.1). n_last is
// the index of the last control point.
fn find_span(n_last: usize, degree: usize, knots: &[f64], u: f64) -> usize {
    if u >= knots[n_last + 1] {
        return n_last;
    }
    if u <= knots[degree] {
        return degree;
    }
    let mut low = degree;
    let mut high = n_last + 1;
    let mut mid = (low + high) / 2;
    while u < knots[mid] || u >= knots[mid + 1] {
        if u < knots[mid] {
            high = mid;
        } else {
            low = mid;
        }
        mid = (low + high) / 2;
    }
    mid
}

// de Boor evaluation of a degree-`degree` b-spline with the given knot vector
// and homogeneous control points (any width — 3 for the meridian, 4 for the
// patch), at parameter u. Degree 0 returns the control point owning the span
// (a piecewise-constant curve).
fn deboor<const N: usize>(degree: usize, knots: &[f64], cpts: &[[f64; N]], u: f64) -> [f64; N] {
    let n_last = cpts.len() - 1;
    let span = find_span(n_last, degree, knots, u);
    if degree == 0 {
        return cpts[span];
    }
    // d[j] starts as the (degree+1) control points influencing this span.
    let mut d: Vec<[f64; N]> = (0..=degree).map(|j| cpts[span - degree + j]).collect();
    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let i = span - degree + j;
            let denom = knots[i + degree + 1 - r] - knots[i];
            let alpha = if denom.abs() < f64::EPSILON {
                0.0
            } else {
                (u - knots[i]) / denom
            };
            for c in 0..N {
                d[j][c] = (1.0 - alpha) * d[j - 1][c] + alpha * d[j][c];
            }
        }
    }
    d[degree]
}

// The control points, degree, and knot vector of the derivative curve: a
// b-spline of degree p differentiates to one of degree p-1 with control points
// Q_i = p·(P_{i+1} - P_i)/(knots[i+p+1] - knots[i+1]) and the knot vector with
// its first and last entries dropped (Piegl & Tiller Eq. 3.7). A zero-width
// denominator (a fully-multiple knot) contributes a zero control point, the
// standard 0/0 -> 0 convention.
fn derivative_cpts<const N: usize>(
    degree: usize,
    knots: &[f64],
    cpts: &[[f64; N]],
) -> (usize, Vec<f64>, Vec<[f64; N]>) {
    let p = degree as f64;
    let mut q = Vec::with_capacity(cpts.len() - 1);
    for i in 0..cpts.len() - 1 {
        let denom = knots[i + degree + 1] - knots[i + 1];
        let scale = if denom.abs() < f64::EPSILON { 0.0 } else { p / denom };
        let mut d = [0.0; N];
        for c in 0..N {
            d[c] = scale * (cpts[i + 1][c] - cpts[i][c]);
        }
        q.push(d);
    }
    let dk = knots[1..knots.len() - 1].to_vec();
    (degree - 1, dk, q)
}

/// Read a `.spline` meridian file: a plain-text, diffable description of the
/// b-spline the reader (import_step_nurbs.py) emits and a human reviews. Line
/// oriented, `#` comments and blank lines ignored:
///
/// ```text
/// degree 3
/// knots 0 0 0 0 0.5 1 1 1 1
/// pole 0 12
/// pole 6 11
/// ...
/// weights 1 1 1 1 1      # optional; absent => non-rational
/// ```
///
/// The same "not a new seam" spirit as Mesh(file=…)/FormTool(profile=…): the
/// core reads this text directly, so the reader is just a text emitter.
pub fn read_spline(path: &std::path::Path) -> Result<BSplineCurve, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    parse_spline(&text)
}

/// The `.spline` grammar, split out so it is unit-testable without a file.
pub fn parse_spline(text: &str) -> Result<BSplineCurve, String> {
    let mut degree: Option<usize> = None;
    let mut knots: Option<Vec<f64>> = None;
    let mut weights: Option<Vec<f64>> = None;
    let mut poles: Vec<(f64, f64)> = Vec::new();

    let parse_f = |t: &str| -> Result<f64, String> {
        t.parse::<f64>().map_err(|_| format!("bad number '{}'", t))
    };

    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let tok: Vec<&str> = line.split_whitespace().collect();
        match tok[0] {
            "degree" => {
                if tok.len() != 2 {
                    return Err(format!("line {}: 'degree' takes one integer", lineno + 1));
                }
                degree = Some(
                    tok[1]
                        .parse::<usize>()
                        .map_err(|_| format!("line {}: bad degree '{}'", lineno + 1, tok[1]))?,
                );
            }
            "knots" => {
                let mut ks = Vec::with_capacity(tok.len() - 1);
                for t in &tok[1..] {
                    ks.push(parse_f(t).map_err(|e| format!("line {}: {}", lineno + 1, e))?);
                }
                knots = Some(ks);
            }
            "weights" => {
                let mut ws = Vec::with_capacity(tok.len() - 1);
                for t in &tok[1..] {
                    ws.push(parse_f(t).map_err(|e| format!("line {}: {}", lineno + 1, e))?);
                }
                weights = Some(ws);
            }
            "pole" => {
                if tok.len() != 3 {
                    return Err(format!(
                        "line {}: 'pole' takes two numbers (r z)",
                        lineno + 1
                    ));
                }
                let r = parse_f(tok[1]).map_err(|e| format!("line {}: {}", lineno + 1, e))?;
                let z = parse_f(tok[2]).map_err(|e| format!("line {}: {}", lineno + 1, e))?;
                if r < 0.0 {
                    return Err(format!(
                        "line {}: pole radius r must be >= 0 (a meridian is a half-plane curve), got {}",
                        lineno + 1,
                        r
                    ));
                }
                poles.push((r, z));
            }
            other => {
                return Err(format!("line {}: unknown spline keyword '{}'", lineno + 1, other));
            }
        }
    }

    let degree = degree.ok_or_else(|| "spline file missing a 'degree' line".to_string())?;
    let knots = knots.ok_or_else(|| "spline file missing a 'knots' line".to_string())?;
    if poles.is_empty() {
        return Err("spline file has no 'pole' lines".to_string());
    }
    BSplineCurve::new(degree, knots, poles, weights)
}

// ---- the general NURBS patch ------------------------------------------------

/// A tensor-product (rational) b-spline surface in R³ — the general
/// (non-revolved) NURBS patch (DECISIONS.md "the general NURBS patch…").
/// Control points are `[x, y, z]` in millimetres; `poles` is the control net
/// stored **row-major**: `poles[iu * n_v + iv]`, `iu` in `0..n_u` along the u
/// direction, `iv` in `0..n_v` along v. `weights` matches it entry for entry
/// (all 1.0 for a non-rational patch). Each direction carries its own degree
/// and full clamped knot vector, validated exactly as the curve's.
#[derive(Clone, Debug, PartialEq)]
pub struct BSplineSurface {
    pub degree_u: usize,
    pub degree_v: usize,
    pub knots_u: Vec<f64>,
    pub knots_v: Vec<f64>,
    pub poles: Vec<[f64; 3]>,
    pub weights: Vec<f64>,
    pub n_u: usize,
    pub n_v: usize,
}

// Per-direction structural validation, shared by both of the surface's
// directions: the same invariants BSplineCurve::new enforces, with the
// direction named so an error points at the offending knot vector. Returns
// the control-point count the knot vector implies.
fn check_direction(dir: &str, degree: usize, knots: &[f64]) -> Result<usize, String> {
    if degree < 1 {
        return Err(format!("b-spline surface degree_{} must be >= 1, got {}", dir, degree));
    }
    if knots.len() < 2 * (degree + 1) {
        return Err(format!(
            "b-spline surface knots_{} must have at least {} entries for degree {}, got {}",
            dir,
            2 * (degree + 1),
            degree,
            knots.len()
        ));
    }
    for w in knots.windows(2) {
        if w[1] < w[0] {
            return Err(format!(
                "b-spline surface knots_{} must be non-decreasing, found {} after {}",
                dir, w[1], w[0]
            ));
        }
    }
    if knots[knots.len() - 1 - degree] <= knots[degree] {
        return Err(format!(
            "b-spline surface knots_{} spans an empty parameter domain",
            dir
        ));
    }
    Ok(knots.len() - degree - 1)
}

impl BSplineSurface {
    /// Build and validate a surface. The control-net dimensions are implied by
    /// the knot vectors (`n = knots.len() - degree - 1` per direction, the same
    /// identity the curve enforces), so a wrong pole count is caught against
    /// them. `weights` = None means non-rational (all weights 1).
    pub fn new(
        degree_u: usize,
        degree_v: usize,
        knots_u: Vec<f64>,
        knots_v: Vec<f64>,
        poles: Vec<[f64; 3]>,
        weights: Option<Vec<f64>>,
    ) -> Result<Self, String> {
        let n_u = check_direction("u", degree_u, &knots_u)?;
        let n_v = check_direction("v", degree_v, &knots_v)?;
        if poles.len() != n_u * n_v {
            return Err(format!(
                "b-spline surface control net must have {} poles ({} x {} from the knot vectors), got {}",
                n_u * n_v,
                n_u,
                n_v,
                poles.len()
            ));
        }
        let weights = match weights {
            None => vec![1.0; poles.len()],
            Some(w) => {
                if w.len() != poles.len() {
                    return Err(format!(
                        "b-spline surface has {} weights but {} control points",
                        w.len(),
                        poles.len()
                    ));
                }
                if let Some(bad) = w.iter().find(|&&x| !(x > 0.0)) {
                    return Err(format!("b-spline surface weights must be positive, found {}", bad));
                }
                w
            }
        };
        Ok(BSplineSurface { degree_u, degree_v, knots_u, knots_v, poles, weights, n_u, n_v })
    }

    /// The clamped parameter domain [u0, u1] of the u direction.
    pub fn domain_u(&self) -> (f64, f64) {
        (self.knots_u[self.degree_u], self.knots_u[self.knots_u.len() - 1 - self.degree_u])
    }

    /// The clamped parameter domain [v0, v1] of the v direction.
    pub fn domain_v(&self) -> (f64, f64) {
        (self.knots_v[self.degree_v], self.knots_v[self.knots_v.len() - 1 - self.degree_v])
    }

    /// Distinct u knots, domain endpoints included — the u-direction
    /// span-boundary list (the curve's `distinct_knots`, per direction). The
    /// drop-cutter's seed grid is laid out per span from these, so seed
    /// density is tied to the patch's own complexity exactly as the guard
    /// scans' density is tied to the meridian's.
    pub fn distinct_knots_u(&self) -> Vec<f64> {
        distinct_knots_of(self.degree_u, &self.knots_u)
    }

    /// Distinct v knots, domain endpoints included.
    pub fn distinct_knots_v(&self) -> Vec<f64> {
        distinct_knots_of(self.degree_v, &self.knots_v)
    }

    /// The point S(u, v) on the patch.
    pub fn eval(&self, u: f64, v: f64) -> [f64; 3] {
        self.rational_partials(u, v, 0)[0][0]
    }

    /// Point and first partials: (S, S_u, S_v). S_u x S_v is the (unnormalized)
    /// surface normal.
    pub fn eval_d1(&self, u: f64, v: f64) -> ([f64; 3], [f64; 3], [f64; 3]) {
        let d = self.rational_partials(u, v, 1);
        (d[0][0], d[1][0], d[0][1])
    }

    /// Point, first, and second partials: (S, S_u, S_v, S_uu, S_uv, S_vv) —
    /// the drop-cutter's Newton polish consumes all six.
    #[allow(clippy::type_complexity)]
    pub fn eval_d2(
        &self,
        u: f64,
        v: f64,
    ) -> ([f64; 3], [f64; 3], [f64; 3], [f64; 3], [f64; 3], [f64; 3]) {
        let d = self.rational_partials(u, v, 2);
        (d[0][0], d[1][0], d[0][1], d[2][0], d[1][1], d[0][2])
    }

    // Cartesian partials ∂^{a+b}S/∂u^a∂v^b for a + b <= n (n <= 2), as
    // out[a][b]. Tensor evaluation: v-derivatives of each pole row's
    // homogeneous curve first, then u-derivatives of the curves those values
    // form — the same homo_derivs both times — then the bivariate rational
    // quotient rule (Piegl & Tiller A4.4 specialized to order 2):
    //   S    = A00 / w
    //   S_u  = (A10 - w10·S) / w                       (S_v symmetric)
    //   S_uu = (A20 - 2·w10·S_u - w20·S) / w           (S_vv symmetric)
    //   S_uv = (A11 - w10·S_v - w01·S_u - w11·S) / w
    fn rational_partials(&self, u: f64, v: f64, n: usize) -> Vec<Vec<[f64; 3]>> {
        // rows[iu][b] = b-th v-derivative of pole row iu's homogeneous curve.
        let rows: Vec<Vec<[f64; 4]>> = (0..self.n_u)
            .map(|iu| {
                let row: Vec<[f64; 4]> = (0..self.n_v)
                    .map(|iv| {
                        let i = iu * self.n_v + iv;
                        let w = self.weights[i];
                        let p = self.poles[i];
                        [w * p[0], w * p[1], w * p[2], w]
                    })
                    .collect();
                homo_derivs(self.degree_v, &self.knots_v, &row, v, n)
            })
            .collect();

        // a[a][b] = ∂^{a+b}H/∂u^a∂v^b of the homogeneous surface.
        let mut ha: Vec<Vec<[f64; 4]>> = vec![vec![[0.0; 4]; n + 1]; n + 1];
        for b in 0..=n {
            let ctrl: Vec<[f64; 4]> = (0..self.n_u).map(|iu| rows[iu][b]).collect();
            let ders = homo_derivs(self.degree_u, &self.knots_u, &ctrl, u, n - b);
            for (a, d) in ders.into_iter().enumerate() {
                ha[a][b] = d;
            }
        }

        let xyz = |h: [f64; 4]| [h[0], h[1], h[2]];
        let comb = |h: [f64; 4], terms: &[(f64, [f64; 3])], w0: f64| {
            let mut out = xyz(h);
            for &(c, t) in terms {
                for k in 0..3 {
                    out[k] -= c * t[k];
                }
            }
            for o in &mut out {
                *o /= w0;
            }
            out
        };

        let w0 = ha[0][0][3];
        let s = comb(ha[0][0], &[], w0);
        let mut out = vec![vec![[0.0; 3]; n + 1]; n + 1];
        out[0][0] = s;
        if n >= 1 {
            let (w10, w01) = (ha[1][0][3], ha[0][1][3]);
            let su = comb(ha[1][0], &[(w10, s)], w0);
            let sv = comb(ha[0][1], &[(w01, s)], w0);
            out[1][0] = su;
            out[0][1] = sv;
            if n >= 2 {
                let (w20, w11, w02) = (ha[2][0][3], ha[1][1][3], ha[0][2][3]);
                out[2][0] = comb(ha[2][0], &[(2.0 * w10, su), (w20, s)], w0);
                out[0][2] = comb(ha[0][2], &[(2.0 * w01, sv), (w02, s)], w0);
                out[1][1] = comb(ha[1][1], &[(w10, sv), (w01, su), (w11, s)], w0);
            }
        }
        out
    }

    /// An upper bound on |S_u| over the whole patch — the derived half of the
    /// drop-cutter's seed-capture argument (DECISIONS.md "the general NURBS
    /// patch…": the seed density is *derived* from the control net, never
    /// guessed). With positive weights the convex-hull property gives, for
    /// the homogeneous surface H = (A, w) and its u-derivative net:
    ///   |A_u| <= D_A   (max |xyz| over the u-derivative net's entries)
    ///   |w_u| <= D_w   (max |w| over the same entries)
    ///   |S|   <= M     (S is a convex combination of the poles)
    ///   w     >= w_min (w is a convex combination of the weights)
    /// so |S_u| = |(A_u - w_u·S)/w| <= (D_A + D_w·M) / w_min. Non-rational
    /// collapses to the classic hull bound D_A exactly (D_w = 0, w_min = 1).
    /// Conservative, not tight — its consumer divides a capture spacing by it,
    /// so slack only ever adds seeds.
    pub fn deriv_bound_u(&self) -> f64 {
        self.deriv_bound(true)
    }

    /// The v-direction counterpart of `deriv_bound_u`.
    pub fn deriv_bound_v(&self) -> f64 {
        self.deriv_bound(false)
    }

    fn deriv_bound(&self, along_u: bool) -> f64 {
        let (degree, knots, outer, inner, stride_outer, stride_inner) = if along_u {
            // Differentiate along u: one derivative net per v column.
            (self.degree_u, &self.knots_u, self.n_v, self.n_u, 1, self.n_v)
        } else {
            (self.degree_v, &self.knots_v, self.n_u, self.n_v, self.n_v, 1)
        };
        let mut d_a: f64 = 0.0;
        let mut d_w: f64 = 0.0;
        for j in 0..outer {
            let line: Vec<[f64; 4]> = (0..inner)
                .map(|i| {
                    let idx = i * stride_inner + j * stride_outer;
                    let w = self.weights[idx];
                    let p = self.poles[idx];
                    [w * p[0], w * p[1], w * p[2], w]
                })
                .collect();
            let (_, _, q) = derivative_cpts(degree, knots, &line);
            for e in q {
                d_a = d_a.max((e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt());
                d_w = d_w.max(e[3].abs());
            }
        }
        let m = self
            .poles
            .iter()
            .map(|p| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt())
            .fold(0.0, f64::max);
        let w_min = self.weights.iter().cloned().fold(f64::INFINITY, f64::min);
        (d_a + d_w * m) / w_min
    }
}

/// Read a `.patch` file: the surface counterpart of `.spline`, emitted by the
/// reader (import_step_nurbs.py) and reviewed by a human. Line oriented, `#`
/// comments and blank lines ignored:
///
/// ```text
/// degree_u 3
/// degree_v 3
/// knots_u 0 0 0 0 1 1 1 1
/// knots_v 0 0 0 0 1 1 1 1
/// pole 0 0 0          # row-major: n_u rows of n_v poles (x y z, mm)
/// ...
/// weights 1 1 ...     # optional; absent => non-rational; row-major
/// ```
///
/// The net dimensions are implied by the knot vectors (no explicit counts to
/// drift out of sync); a wrong pole count is caught by the constructor.
pub fn read_patch(path: &std::path::Path) -> Result<BSplineSurface, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    parse_patch(&text)
}

/// The `.patch` grammar, split out so it is unit-testable without a file.
pub fn parse_patch(text: &str) -> Result<BSplineSurface, String> {
    let mut degree_u: Option<usize> = None;
    let mut degree_v: Option<usize> = None;
    let mut knots_u: Option<Vec<f64>> = None;
    let mut knots_v: Option<Vec<f64>> = None;
    let mut weights: Option<Vec<f64>> = None;
    let mut poles: Vec<[f64; 3]> = Vec::new();

    let parse_f = |t: &str| -> Result<f64, String> {
        t.parse::<f64>().map_err(|_| format!("bad number '{}'", t))
    };
    let parse_list = |tok: &[&str], lineno: usize| -> Result<Vec<f64>, String> {
        tok.iter()
            .map(|t| parse_f(t).map_err(|e| format!("line {}: {}", lineno + 1, e)))
            .collect()
    };

    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let tok: Vec<&str> = line.split_whitespace().collect();
        match tok[0] {
            "degree_u" | "degree_v" => {
                if tok.len() != 2 {
                    return Err(format!("line {}: '{}' takes one integer", lineno + 1, tok[0]));
                }
                let d = tok[1]
                    .parse::<usize>()
                    .map_err(|_| format!("line {}: bad degree '{}'", lineno + 1, tok[1]))?;
                if tok[0] == "degree_u" {
                    degree_u = Some(d);
                } else {
                    degree_v = Some(d);
                }
            }
            "knots_u" => knots_u = Some(parse_list(&tok[1..], lineno)?),
            "knots_v" => knots_v = Some(parse_list(&tok[1..], lineno)?),
            "weights" => weights = Some(parse_list(&tok[1..], lineno)?),
            "pole" => {
                if tok.len() != 4 {
                    return Err(format!(
                        "line {}: 'pole' takes three numbers (x y z)",
                        lineno + 1
                    ));
                }
                let x = parse_f(tok[1]).map_err(|e| format!("line {}: {}", lineno + 1, e))?;
                let y = parse_f(tok[2]).map_err(|e| format!("line {}: {}", lineno + 1, e))?;
                let z = parse_f(tok[3]).map_err(|e| format!("line {}: {}", lineno + 1, e))?;
                poles.push([x, y, z]);
            }
            other => {
                return Err(format!("line {}: unknown patch keyword '{}'", lineno + 1, other));
            }
        }
    }

    let degree_u = degree_u.ok_or_else(|| "patch file missing a 'degree_u' line".to_string())?;
    let degree_v = degree_v.ok_or_else(|| "patch file missing a 'degree_v' line".to_string())?;
    let knots_u = knots_u.ok_or_else(|| "patch file missing a 'knots_u' line".to_string())?;
    let knots_v = knots_v.ok_or_else(|| "patch file missing a 'knots_v' line".to_string())?;
    if poles.is_empty() {
        return Err("patch file has no 'pole' lines".to_string());
    }
    BSplineSurface::new(degree_u, degree_v, knots_u, knots_v, poles, weights)
}

#[cfg(test)]
mod tests {
    use crate::math::Portable as _;
    use super::*;

    fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
        ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
    }

    #[test]
    fn degree1_is_the_control_polyline() {
        // A degree-1 clamped b-spline interpolates its control points linearly:
        // C(u) is exactly the polyline. Poles (0,0)-(2,4)-(6,4), uniform knots.
        let c = BSplineCurve::new(
            1,
            vec![0.0, 0.0, 0.5, 1.0, 1.0],
            vec![(0.0, 0.0), (2.0, 4.0), (6.0, 4.0)],
            None,
        )
        .unwrap();
        assert!(dist(c.eval(0.0), (0.0, 0.0)) < 1e-12);
        assert!(dist(c.eval(0.5), (2.0, 4.0)) < 1e-12);
        assert!(dist(c.eval(1.0), (6.0, 4.0)) < 1e-12);
        // Midpoint of the first segment.
        assert!(dist(c.eval(0.25), (1.0, 2.0)) < 1e-12);
        // Midpoint of the second segment.
        assert!(dist(c.eval(0.75), (4.0, 4.0)) < 1e-12);
    }

    // A rational quadratic Bézier with weights (1, cos45, 1) over the control
    // triangle (R,0)-(R,R)-(0,R) traces the exact quarter circle of radius R.
    fn quarter_circle(r: f64) -> BSplineCurve {
        let w = (std::f64::consts::FRAC_PI_4).pcos(); // √2/2
        BSplineCurve::new(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![(r, 0.0), (r, r), (0.0, r)],
            Some(vec![1.0, w, 1.0]),
        )
        .unwrap()
    }

    #[test]
    fn rational_quarter_circle_is_exact() {
        let r = 7.0;
        let c = quarter_circle(r);
        for k in 0..=20 {
            let u = k as f64 / 20.0;
            let (pr, pz) = c.eval(u);
            let radius = (pr * pr + pz * pz).sqrt();
            assert!((radius - r).abs() < 1e-9, "u={} radius={}", u, radius);
        }
        // Endpoints and the symmetric midpoint (R/√2, R/√2).
        assert!(dist(c.eval(0.0), (r, 0.0)) < 1e-12);
        assert!(dist(c.eval(1.0), (0.0, r)) < 1e-12);
        let m = r / 2.0f64.sqrt();
        assert!(dist(c.eval(0.5), (m, m)) < 1e-9);
    }

    #[test]
    fn first_derivative_matches_finite_difference() {
        // Cubic non-rational spline; compare analytic C' to a central
        // difference at interior parameters.
        let c = BSplineCurve::new(
            3,
            vec![0.0, 0.0, 0.0, 0.0, 0.4, 1.0, 1.0, 1.0, 1.0],
            vec![(0.0, 12.0), (6.0, 11.0), (11.0, 8.0), (13.0, 3.0), (12.0, 0.0)],
            None,
        )
        .unwrap();
        let h = 1e-6;
        for k in 1..10 {
            let u = k as f64 / 10.0;
            let (_, d1) = c.eval_d1(u);
            let fp = c.eval(u + h);
            let fm = c.eval(u - h);
            let fd = ((fp.0 - fm.0) / (2.0 * h), (fp.1 - fm.1) / (2.0 * h));
            assert!((d1.0 - fd.0).abs() < 1e-4, "u={} dr {} vs {}", u, d1.0, fd.0);
            assert!((d1.1 - fd.1).abs() < 1e-4, "u={} dz {} vs {}", u, d1.1, fd.1);
        }
    }

    #[test]
    fn quarter_circle_curvature_is_one_over_r() {
        // Signed curvature κ = (r'·z'' − z'·r'')/|C'|³ has magnitude 1/R on a
        // circle of radius R — the value the finishing gouge guard reads.
        let r = 5.0;
        let c = quarter_circle(r);
        for k in 1..10 {
            let u = k as f64 / 10.0;
            let (_, d1, d2) = c.eval_d2(u);
            let speed2 = d1.0 * d1.0 + d1.1 * d1.1;
            let kappa = (d1.0 * d2.1 - d1.1 * d2.0) / speed2.ppowf(1.5);
            assert!((kappa.abs() - 1.0 / r).abs() < 1e-7, "u={} kappa={}", u, kappa);
        }
    }

    #[test]
    fn parse_spline_round_trips() {
        let text = "\
# a small cap meridian
degree 3
knots 0 0 0 0 0.4 1 1 1 1
pole 0 12
pole 6 11
pole 11 8
pole 13 3
pole 12 0
";
        let c = parse_spline(text).unwrap();
        assert_eq!(c.degree, 3);
        assert_eq!(c.poles.len(), 5);
        assert_eq!(c.weights, vec![1.0; 5]);
        assert!(dist(c.eval(0.0), (0.0, 12.0)) < 1e-12);
        assert!(dist(c.eval(1.0), (12.0, 0.0)) < 1e-12);
    }

    #[test]
    fn machinable_cap_accepts_monotone_and_refuses_undercut() {
        // The spline_cap fixture meridian: monotone r up, z down — a machinable
        // cap.
        let good = BSplineCurve::new(
            3,
            vec![0.0, 0.0, 0.0, 0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0, 1.0, 1.0, 1.0],
            vec![(0.0, 20.0), (3.0, 19.0), (7.0, 16.0), (10.0, 11.0), (12.0, 5.0), (13.0, 0.0)],
            None,
        )
        .unwrap();
        assert!(good.check_machinable_cap("Cap").is_ok());

        // A meridian that bulges out then tucks back in as it descends: the
        // tuck is an undercut a top-down tool cannot reach.
        let undercut = BSplineCurve::new(
            3,
            vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0],
            vec![(0.0, 20.0), (10.0, 14.0), (13.0, 9.0), (8.0, 4.0), (5.0, 0.0)],
            None,
        )
        .unwrap();
        let err = undercut.check_machinable_cap("Bulge").unwrap_err();
        assert!(err.contains("undercut"), "undercut error should say why: {err}");

        // A meridian that rises again (not single-valued in height).
        let rises = BSplineCurve::new(
            1,
            vec![0.0, 0.0, 0.5, 1.0, 1.0],
            vec![(0.0, 10.0), (5.0, 0.0), (8.0, 6.0)],
            None,
        )
        .unwrap();
        assert!(rises.check_machinable_cap("Wave").is_err());
    }

    #[test]
    fn machinable_cap_catches_a_reversal_hidden_in_a_narrow_knot_span() {
        // A degree-1 meridian that descends monotonically except for a tiny
        // inward turn (an undercut) confined to a knot span 1e-4 wide — narrow
        // enough to slip between the samples of a fixed whole-domain scan
        // (400 uniform samples are 2.5e-3 apart), and with the radius fully
        // recovered by the next span so the neighbouring samples look monotone.
        // The per-span scan samples inside every span, so it must catch it.
        let sneaky = BSplineCurve::new(
            1,
            vec![0.0, 0.0, 0.5, 0.5001, 0.7, 1.0, 1.0],
            vec![(0.0, 20.0), (10.0, 10.0), (9.5, 9.9), (10.5, 9.8), (13.0, 0.0)],
            None,
        )
        .unwrap();
        let err = sneaky.check_machinable_cap("Sneaky").unwrap_err();
        assert!(err.contains("undercut"), "the narrow-span reversal must be caught: {err}");
    }

    #[test]
    fn monotone_polygon_is_accepted_exactly() {
        // Tier-1 accept: the polygon is monotone (r up, z down), so the
        // variation-diminishing property proves the curve monotone with no
        // sampling at all — including across many knot spans.
        let c = BSplineCurve::new(
            3,
            vec![0.0, 0.0, 0.0, 0.0, 0.2, 0.4, 0.6, 0.8, 1.0, 1.0, 1.0, 1.0],
            vec![
                (0.0, 20.0), (2.0, 19.0), (4.0, 16.0), (6.0, 12.0),
                (8.0, 8.0), (10.0, 5.0), (12.0, 2.0), (13.0, 0.0),
            ],
            None,
        )
        .unwrap();
        assert!(c.check_machinable_cap("Cap").is_ok());
    }

    #[test]
    fn curvature_toward_up_normal_signs() {
        // The sign convention the gouge guard and scallop march share: on the
        // apex-first quarter circle (0,R)-(R,R)-(R,0) — a dome, convex toward
        // the tool above it — the oriented curvature is NEGATIVE with
        // magnitude 1/R. On the arc centred at (20,20) from (10,20) down to
        // (20,10) — a cove holding the tool, concave toward it — it is
        // POSITIVE with magnitude 1/R.
        let w = (std::f64::consts::FRAC_PI_4).pcos();
        let dome = BSplineCurve::new(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![(0.0, 8.0), (8.0, 8.0), (8.0, 0.0)],
            Some(vec![1.0, w, 1.0]),
        )
        .unwrap();
        let cove = BSplineCurve::new(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![(10.0, 20.0), (10.0, 10.0), (20.0, 10.0)],
            Some(vec![1.0, w, 1.0]),
        )
        .unwrap();
        for k in 1..10 {
            let u = k as f64 / 10.0;
            let kd = dome.curvature_toward_up_normal(u).unwrap();
            assert!(kd < 0.0 && (kd.abs() - 1.0 / 8.0).abs() < 1e-7, "dome u={u}: {kd}");
            let kc = cove.curvature_toward_up_normal(u).unwrap();
            assert!(kc > 0.0 && (kc - 1.0 / 10.0).abs() < 1e-7, "cove u={u}: {kc}");
        }
    }

    #[test]
    fn parse_spline_rejects_malformed() {
        // Knot vector length mismatch.
        assert!(parse_spline("degree 3\nknots 0 0 1 1\npole 0 0\npole 1 1\n").is_err());
        // Negative radius.
        assert!(parse_spline("degree 1\nknots 0 0 1 1\npole -1 0\npole 1 1\n").is_err());
        // Missing degree.
        assert!(parse_spline("knots 0 0 1 1\npole 0 0\npole 1 1\n").is_err());
    }

    #[test]
    fn machinable_funnel_accepts_monotone_and_refuses_widening() {
        // A mouth-first cavity meridian: r down, z down — a machinable funnel.
        let good = BSplineCurve::new(
            3,
            vec![0.0, 0.0, 0.0, 0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0, 1.0, 1.0, 1.0],
            vec![(13.0, 0.0), (12.0, -3.0), (10.0, -8.0), (7.0, -13.0), (3.0, -16.0), (1.0, -18.0)],
            None,
        )
        .unwrap();
        assert!(good.check_machinable_funnel("Funnel").is_ok());

        // A cavity that necks in then belly-widens below the mouth: the belly
        // is the cavity's undercut (the narrower rim above shadows it).
        let widens = BSplineCurve::new(
            3,
            vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0],
            vec![(13.0, 0.0), (8.0, -4.0), (7.0, -9.0), (12.0, -14.0), (11.0, -18.0)],
            None,
        )
        .unwrap();
        let err = widens.check_machinable_funnel("Belly").unwrap_err();
        assert!(err.contains("undercut"), "widening error should say why: {err}");

        // A meridian that rises again (not single-valued in height).
        let rises = BSplineCurve::new(
            1,
            vec![0.0, 0.0, 0.5, 1.0, 1.0],
            vec![(13.0, 0.0), (8.0, -10.0), (5.0, -4.0)],
            None,
        )
        .unwrap();
        assert!(rises.check_machinable_funnel("Wave").is_err());
    }

    #[test]
    fn machinable_funnel_refuses_zero_mouth_and_zero_depth() {
        // First pole on the axis: a cavity with no open mouth circle. The
        // preconditions run before the tiers, so even this (cap-shaped,
        // monotone the wrong way) curve gets the mouth message.
        let closed = BSplineCurve::new(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![(0.0, 0.0), (10.0, -10.0)],
            None,
        )
        .unwrap();
        let err = closed.check_machinable_funnel("Sealed").unwrap_err();
        assert!(err.contains("open mouth"), "zero-mouth error should say why: {err}");

        // A flat ring at one height: no depth, nothing to machine.
        let flat = BSplineCurve::new(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![(13.0, 0.0), (5.0, 0.0)],
            None,
        )
        .unwrap();
        let err = flat.check_machinable_funnel("Flat").unwrap_err();
        assert!(err.contains("no depth"), "zero-depth error should say why: {err}");
    }

    #[test]
    fn machinable_funnel_catches_a_widening_hidden_in_a_narrow_knot_span() {
        // The cap's sneaky-reversal test mirrored: a tiny outward bulge (the
        // cavity's undercut) confined to a 1e-4-wide knot span, radius fully
        // recovered by the next span. The per-span scan must catch it.
        let sneaky = BSplineCurve::new(
            1,
            vec![0.0, 0.0, 0.5, 0.5001, 0.7, 1.0, 1.0],
            vec![(13.0, 0.0), (8.0, -10.0), (8.5, -10.1), (7.5, -10.2), (3.0, -20.0)],
            None,
        )
        .unwrap();
        let err = sneaky.check_machinable_funnel("Sneaky").unwrap_err();
        assert!(err.contains("undercut"), "the narrow-span widening must be caught: {err}");
    }

    #[test]
    fn funnel_monotone_polygon_is_accepted_exactly() {
        // Tier-1 accept, funnel side: the polygon is monotone (r down, z down)
        // across many knot spans, so variation-diminishing proves the curve.
        let c = BSplineCurve::new(
            3,
            vec![0.0, 0.0, 0.0, 0.0, 0.2, 0.4, 0.6, 0.8, 1.0, 1.0, 1.0, 1.0],
            vec![
                (13.0, 0.0), (12.0, -2.0), (10.0, -5.0), (8.0, -8.0),
                (6.0, -12.0), (4.0, -16.0), (2.0, -19.0), (1.0, -20.0),
            ],
            None,
        )
        .unwrap();
        assert!(c.check_machinable_funnel("Funnel").is_ok());
    }

    #[test]
    fn curvature_toward_up_normal_signs_hold_from_the_void_side() {
        // The same one-home convention read from the funnel side (the flip is
        // orientation-agnostic — see the method comment). The mouth-first
        // quarter circle (R,0)-(R,−R)-(0,−R) centred at the origin is a
        // hemispherical BOWL: it holds the descending tool, concave toward it,
        // so the oriented curvature is POSITIVE with magnitude 1/R (the plain
        // signed curvature is −1/R; r' < 0 fires the flip). A wall that bulges
        // into the void — the arc centred at (20,−10) from (20,0) to
        // (10,−10) — is convex toward the tool: NEGATIVE, magnitude 1/10.
        let w = (std::f64::consts::FRAC_PI_4).pcos();
        let bowl = BSplineCurve::new(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![(10.0, 0.0), (10.0, -10.0), (0.0, -10.0)],
            Some(vec![1.0, w, 1.0]),
        )
        .unwrap();
        let bulge = BSplineCurve::new(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![(20.0, 0.0), (10.0, 0.0), (10.0, -10.0)],
            Some(vec![1.0, w, 1.0]),
        )
        .unwrap();
        for k in 1..10 {
            let u = k as f64 / 10.0;
            let kb = bowl.curvature_toward_up_normal(u).unwrap();
            assert!(kb > 0.0 && (kb - 1.0 / 10.0).abs() < 1e-7, "bowl u={u}: {kb}");
            let kw = bulge.curvature_toward_up_normal(u).unwrap();
            assert!(kw < 0.0 && (kw.abs() - 1.0 / 10.0).abs() < 1e-7, "bulge u={u}: {kw}");
        }
    }

    // ---- the general NURBS patch --------------------------------------------

    fn dist3(a: [f64; 3], b: [f64; 3]) -> f64 {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    }

    #[test]
    fn bilinear_patch_interpolates_corners_and_center() {
        // Degree 1x1 with a 2x2 net is the bilinear interpolant of its
        // corners: exact at the corners and the (0.5, 0.5) average.
        let s = BSplineSurface::new(
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
            vec![
                [0.0, 0.0, 0.0],
                [0.0, 10.0, 2.0],
                [10.0, 0.0, 4.0],
                [10.0, 10.0, 6.0],
            ],
            None,
        )
        .unwrap();
        assert!(dist3(s.eval(0.0, 0.0), [0.0, 0.0, 0.0]) < 1e-12);
        assert!(dist3(s.eval(0.0, 1.0), [0.0, 10.0, 2.0]) < 1e-12);
        assert!(dist3(s.eval(1.0, 0.0), [10.0, 0.0, 4.0]) < 1e-12);
        assert!(dist3(s.eval(1.0, 1.0), [10.0, 10.0, 6.0]) < 1e-12);
        assert!(dist3(s.eval(0.5, 0.5), [5.0, 5.0, 3.0]) < 1e-12);
    }

    // The exact sphere octant: the rational quarter circle revolved a quarter
    // turn — the tensor product of two exact-arc directions, weights
    // w_ij = w_i * w_j. u walks the meridian (equator to pole), v the
    // rotation. The surface anchor test in the same mold as the curve's
    // rational_quarter_circle_is_exact.
    fn sphere_octant(r: f64) -> BSplineSurface {
        let c = (std::f64::consts::FRAC_PI_4).pcos(); // √2/2
        let xs = [r, r, 0.0]; // meridian radius poles (equator-first)
        let zs = [0.0, r, r]; // meridian height poles
        let wu = [1.0, c, 1.0];
        let wv = [1.0, c, 1.0];
        let mut poles = Vec::new();
        let mut weights = Vec::new();
        for i in 0..3 {
            // Rotate the meridian pole (xs[i], 0, zs[i]) a quarter turn: the
            // arc poles in the plane z = zs[i] are (x,0) -> (x,x) -> (0,x).
            let ring = [[xs[i], 0.0, zs[i]], [xs[i], xs[i], zs[i]], [0.0, xs[i], zs[i]]];
            for j in 0..3 {
                poles.push(ring[j]);
                weights.push(wu[i] * wv[j]);
            }
        }
        BSplineSurface::new(
            2,
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            poles,
            Some(weights),
        )
        .unwrap()
    }

    #[test]
    fn rational_sphere_octant_is_exact() {
        let r = 9.0;
        let s = sphere_octant(r);
        for ku in 0..=10 {
            for kv in 0..=10 {
                let p = s.eval(ku as f64 / 10.0, kv as f64 / 10.0);
                let radius = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
                assert!((radius - r).abs() < 1e-9, "u={} v={} radius={}", ku, kv, radius);
            }
        }
        // Corners: equator start, equator quarter-turn, pole.
        assert!(dist3(s.eval(0.0, 0.0), [r, 0.0, 0.0]) < 1e-12);
        assert!(dist3(s.eval(0.0, 1.0), [0.0, r, 0.0]) < 1e-12);
        assert!(dist3(s.eval(1.0, 0.0), [0.0, 0.0, r]) < 1e-9);
    }

    #[test]
    fn surface_partials_match_finite_differences() {
        // A rational bicubic with an uneven net and non-uniform weights:
        // compare all five partials to central differences at interior points.
        let s = BSplineSurface::new(
            3,
            3,
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            (0..16)
                .map(|i| {
                    let (iu, iv) = (i / 4, i % 4);
                    [
                        8.0 * iu as f64 + iv as f64,
                        8.0 * iv as f64 - 0.5 * iu as f64,
                        ((iu * iv) as f64).psin() * 5.0 + iu as f64,
                    ]
                })
                .collect(),
            Some((0..16).map(|i| 1.0 + 0.1 * ((i % 5) as f64)).collect()),
        )
        .unwrap();
        let h = 1e-5;
        for ku in 1..5 {
            for kv in 1..5 {
                let (u, v) = (ku as f64 / 5.0, kv as f64 / 5.0);
                let (_, su, sv, suu, suv, svv) = s.eval_d2(u, v);
                for k in 0..3 {
                    let fd_u = (s.eval(u + h, v)[k] - s.eval(u - h, v)[k]) / (2.0 * h);
                    let fd_v = (s.eval(u, v + h)[k] - s.eval(u, v - h)[k]) / (2.0 * h);
                    assert!((su[k] - fd_u).abs() < 1e-4, "S_u[{k}] {} vs {}", su[k], fd_u);
                    assert!((sv[k] - fd_v).abs() < 1e-4, "S_v[{k}] {} vs {}", sv[k], fd_v);
                    let fd_uu =
                        (s.eval(u + h, v)[k] - 2.0 * s.eval(u, v)[k] + s.eval(u - h, v)[k]) / (h * h);
                    let fd_vv =
                        (s.eval(u, v + h)[k] - 2.0 * s.eval(u, v)[k] + s.eval(u, v - h)[k]) / (h * h);
                    let fd_uv = (s.eval(u + h, v + h)[k] - s.eval(u + h, v - h)[k]
                        - s.eval(u - h, v + h)[k]
                        + s.eval(u - h, v - h)[k])
                        / (4.0 * h * h);
                    assert!((suu[k] - fd_uu).abs() < 1e-2, "S_uu[{k}] {} vs {}", suu[k], fd_uu);
                    assert!((svv[k] - fd_vv).abs() < 1e-2, "S_vv[{k}] {} vs {}", svv[k], fd_vv);
                    assert!((suv[k] - fd_uv).abs() < 1e-2, "S_uv[{k}] {} vs {}", suv[k], fd_uv);
                }
            }
        }
    }

    #[test]
    fn deriv_bound_actually_bounds() {
        // The capture argument's premise, checked: |S_u| and |S_v| sampled
        // densely never exceed the hull bounds — on the rational octant (where
        // the weight terms matter) and on a non-rational bicubic.
        let s = sphere_octant(7.0);
        let (bu, bv) = (s.deriv_bound_u(), s.deriv_bound_v());
        for ku in 0..=40 {
            for kv in 0..=40 {
                let (_, su, sv) = s.eval_d1(ku as f64 / 40.0, kv as f64 / 40.0);
                let (nu, nv) = (
                    (su[0] * su[0] + su[1] * su[1] + su[2] * su[2]).sqrt(),
                    (sv[0] * sv[0] + sv[1] * sv[1] + sv[2] * sv[2]).sqrt(),
                );
                assert!(nu <= bu + 1e-9, "|S_u|={} exceeds bound {}", nu, bu);
                assert!(nv <= bv + 1e-9, "|S_v|={} exceeds bound {}", nv, bv);
            }
        }
    }

    #[test]
    fn parse_patch_round_trips() {
        let text = "\
# a flat 2x2 bilinear sheet
degree_u 1
degree_v 1
knots_u 0 0 1 1
knots_v 0 0 1 1
pole 0 0 5
pole 0 10 5
pole 10 0 5
pole 10 10 5
";
        let s = parse_patch(text).unwrap();
        assert_eq!((s.degree_u, s.degree_v, s.n_u, s.n_v), (1, 1, 2, 2));
        assert_eq!(s.weights, vec![1.0; 4]);
        assert!(dist3(s.eval(0.5, 0.5), [5.0, 5.0, 5.0]) < 1e-12);
    }

    #[test]
    fn parse_patch_rejects_malformed() {
        // Pole count disagreeing with the knot-implied net.
        assert!(parse_patch(
            "degree_u 1\ndegree_v 1\nknots_u 0 0 1 1\nknots_v 0 0 1 1\npole 0 0 0\npole 1 0 0\npole 0 1 0\n"
        )
        .is_err());
        // Missing degree_v.
        assert!(parse_patch("degree_u 1\nknots_u 0 0 1 1\nknots_v 0 0 1 1\npole 0 0 0\n").is_err());
        // Decreasing knot vector.
        assert!(parse_patch(
            "degree_u 1\ndegree_v 1\nknots_u 0 1 0 1\nknots_v 0 0 1 1\npole 0 0 0\npole 1 0 0\npole 0 1 0\npole 1 1 0\n"
        )
        .is_err());
        // A pole line with the meridian's two-number shape (the .spline
        // grammar) must say the patch wants three.
        let err = parse_patch("degree_u 1\ndegree_v 1\nknots_u 0 0 1 1\nknots_v 0 0 1 1\npole 0 0\n")
            .unwrap_err();
        assert!(err.contains("three numbers"), "{err}");
    }
}
