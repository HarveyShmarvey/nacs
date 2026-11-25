use crate::math::Portable as _;
// view: the window's view model, std-only. Everything the window DRAWS is
// derived here so the drawing code has nothing to derive: the frontend is a
// thin consumer of this module plus `outline::ProjectOutline` and must never
// walk `Part` or the emitted program itself — a renderer that re-derives
// geometry is a second engine. Deliberately eframe-free: every type is std
// (`[f64; 3]`, `[u8; 3]`, tuples), so it compiles and its pins run under
// `--no-default-features`. The camera is ORTHOGRAPHIC by choice (the
// machinist's drawing convention: zoom is a scale, the projection a pure
// linear map with exact test expectations); there is no depth buffer behind
// the painter, and the stock surface does not need one (DECISIONS.md).

use crate::gcode::{self, Motion, OpEmission};
use crate::outline::ProjectOutline;
use crate::parse::{
    self, derive_contour_top, derive_op_role, derive_z_cut, ChamferLeg, Feature, OpRole, Part,
    Stock,
};
pub use crate::parse::Silhouette;
use crate::theme::{Center, Document, Layer, Layout, SidebarTab, Theme};
use crate::vox::VoxField;

/// One run of connected emitted motion at a single rapid/feed state.
pub struct Polyline {
    pub rapid: bool,
    pub pts: Vec<[f64; 3]>,
}

/// One operation's drawable motion. `op_idx` is the SAME index
/// `outline::OpNode.idx` and `gcode::OpEmission.op_idx` carry — the join key
/// (outline.rs documents it): a click in the outline panel selects paths by
/// this index, never by position in any list.
pub struct OpPath {
    pub op_idx: usize,
    pub role: OpRole,
    pub polylines: Vec<Polyline>,
}

/// Build the drawable paths from the emission emitted moves. Control-comp ops draw the
/// RESOLVED centerline (`gcode::resolved_moves` — the motion the machine
/// makes); arcs flatten against `CHORD_TOL_MM`, the single tolerance home.
/// Moves the modal walk cannot place (the program's opening retracts, before
/// X/Y/Z are all established) are skipped, exactly as the voxel walker skips
/// them — they carry no drawable geometry.
pub fn op_paths(p: &Part, emitted_moves: &[OpEmission]) -> Vec<OpPath> {
    let mut out: Vec<OpPath> = Vec::new();
    // Modal state carries across ops, as in every other emitted moves reader: exact,
    // because each op re-establishes its coordinates on entry.
    let mut m = Motion { rapid: true, x: None, y: None, z: None, f: None };

    for em in emitted_moves {
        let op = &p.operations[em.op_idx];
        let mut path = OpPath { op_idx: em.op_idx, role: derive_op_role(op), polylines: Vec::new() };

        for line in gcode::resolved_moves(em) {
            let before = m;
            let arc = gcode::arc_words(line);
            if !gcode::apply_move(line, &mut m) {
                continue;
            }
            let (to, from) = match ((m.x, m.y, m.z), (before.x, before.y, before.z)) {
                ((Some(tx), Some(ty), Some(tz)), (Some(fx), Some(fy), Some(fz))) => {
                    ([tx, ty, tz], [fx, fy, fz])
                }
                _ => continue,
            };

            if let Some((i, j, cw)) = arc {
                // The dialect's arcs are constant-Z (`gcode::arc` writes no Z
                // word); a Z-carrying arc is not a drawable class any more
                // than it is a stampable one — skipped here, rejected loudly
                // by the voxel walker, which is the voice that owns errors.
                if (to[2] - from[2]).abs() > gcode::PATH_EPS {
                    continue;
                }
                let ag = gcode::arc_geom(from[0], from[1], to[0], to[1], i, j, cw);
                if ag.ra <= gcode::PATH_EPS {
                    continue;
                }
                let pts = gcode::arc_chords(&ag, from, to);
                append(&mut path.polylines, m.rapid, &pts);
            } else {
                append(&mut path.polylines, m.rapid, &[from, to]);
            }
        }
        out.push(path);
    }
    out
}

/// Extend the op's last polyline when the new run continues it at the same
/// rapid/feed state; open a new one otherwise. Emitted motion is sequential,
/// so "continues" is exact endpoint equality, not a tolerance question.
fn append(polylines: &mut Vec<Polyline>, rapid: bool, pts: &[[f64; 3]]) {
    if let Some(last) = polylines.last_mut() {
        if last.rapid == rapid && last.pts.last() == pts.first() {
            last.pts.extend_from_slice(&pts[1..]);
            return;
        }
    }
    polylines.push(Polyline { rapid, pts: pts.to_vec() });
}


// ---- the orthographic orbit camera ------------------------------------------

/// Orbit state: yaw around world +Z (the tool axis), pitch toward top-down,
/// a uniform mm→screen scale, and the world point at screen center.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    /// Radians around +Z. At 0 the viewer stands on +X looking toward −X —
    /// the YZ silhouette, with +Y running right across the screen.
    pub yaw: f64,
    /// Radians; 0 is a side view (screen up = +Z), π/2 is straight down.
    pub pitch: f64,
    /// Screen units per millimeter.
    pub scale: f64,
    pub target: [f64; 3],
}

/// A `Camera` with its trigonometry taken, from `Camera::lens()`. Hold one
/// for the length of a loop and project through it; hold one across a camera
/// CHANGE and you are drawing last frame's stance, which is why this is a
/// short-lived value rather than anything cached on the camera itself.
#[derive(Clone, Copy, Debug)]
pub struct Lens {
    sy: f64,
    cy: f64,
    sp: f64,
    cp: f64,
    scale: f64,
    target: [f64; 3],
}

impl Lens {
    /// The projection itself — the ONE home for this arithmetic, which
    /// `Camera::project` calls through after taking the stance per point.
    /// See `Camera::project` for what the returned triple means.
    pub fn project(&self, p: [f64; 3]) -> (f32, f32, f32) {
        let (px, py, pz) = (p[0] - self.target[0], p[1] - self.target[1], p[2] - self.target[2]);
        let x_cam = -self.sy * px + self.cy * py;
        let h = self.cy * px + self.sy * py; // horizontal reach along the view direction
        let y_cam = -self.sp * h + self.cp * pz;
        let depth = -(self.cp * h + self.sp * pz);
        ((x_cam * self.scale) as f32, (y_cam * self.scale) as f32, depth as f32)
    }
}

/// How much wider than the scene's bounding sphere the pipeline's depth range
/// is cut, so a vertex at the extreme cannot round out of [0, 1] and vanish.
pub const DEPTH_MARGIN: f64 = 1.02;

impl Lens {
    /// The same projection as a 4x4 world-to-clip matrix, column-major for
    /// WGSL — what a GPU needs so the mesh can be uploaded ONCE and the
    /// camera arrive as sixteen floats a frame (DECISIONS.md → "the field is
    /// already undercut-capable…", slice 1).
    ///
    /// `w` and `h` are the viewport in POINTS, whose borders egui defines as
    /// the [-1, +1] of normalized device coordinates; `radius` is a bounding
    /// sphere of the drawn scene, which sets the depth range. A sphere is
    /// orientation-free, so no orbit can push geometry out of the depth
    /// range — the argument `fit_to_aabb` already makes for the fit.
    ///
    /// Screen y is NOT flipped here as it is for the painter: normalized
    /// device coordinates run +y UP, which is the projection's own
    /// convention, so the flip the viewport applies is the painter's business
    /// and not the pipeline's.
    pub fn clip_matrix(&self, w: f64, h: f64, radius: f64) -> [f32; 16] {
        let kx = self.scale / (w * 0.5);
        let ky = self.scale / (h * 0.5);
        // Depth to [0, 1] with 0 nearest — the wgpu convention, and the
        // reason the pipeline compares Less against a buffer cleared to 1.
        //
        // The margin is not decoration. The depth row is a UNIT vector, so a
        // point exactly on the bounding sphere maps to exactly 0 or exactly 1
        // — and a vertex that rounds a hair past either end is clipped away,
        // which reads as a hole in the stock rather than as an error. The
        // margin is applied HERE rather than asked of callers, because a
        // caller who forgets loses geometry silently at the one stance where
        // the scene is widest.
        let kz = 1.0 / (2.0 * radius.max(1e-6) * DEPTH_MARGIN);
        // The three rows ARE `project`'s three lines, times those scales; the
        // trigonometry is the lens's, taken once, so this restates the linear
        // algebra but never the stance. `test_the_clip_matrix_is_the_lens`
        // holds the two together.
        let r0 = [kx * -self.sy, kx * self.cy, 0.0];
        let r1 = [ky * -self.sp * self.cy, ky * -self.sp * self.sy, ky * self.cp];
        let r2 = [kz * -self.cp * self.cy, kz * -self.cp * self.sy, kz * -self.sp];
        let t = self.target;
        let dot = |r: [f64; 3]| r[0] * t[0] + r[1] * t[1] + r[2] * t[2];
        // The target is folded into the translation column rather than
        // subtracted per vertex, which is the whole point of a matrix.
        let (d0, d1, d2) = (-dot(r0), -dot(r1), 0.5 - dot(r2));
        [
            r0[0] as f32, r1[0] as f32, r2[0] as f32, 0.0,
            r0[1] as f32, r1[1] as f32, r2[1] as f32, 0.0,
            r0[2] as f32, r1[2] as f32, r2[2] as f32, 0.0,
            d0 as f32,    d1 as f32,    d2 as f32,    1.0,
        ]
    }
}

/// The zoom floor and ceiling, in screen units per millimetre. The floor is
/// the far side of "the whole shop fits on screen"; the ceiling puts a single
/// micron across a screen, and keeps a metre-scale coordinate nine orders
/// inside `f32::MAX` — the projection's own type — so no reachable zoom can
/// manufacture an infinity.
pub const SCALE_MIN: f64 = 1e-3;
pub const SCALE_MAX: f64 = 1e6;

impl Camera {
    /// The default stance: a quarter turn of yaw and a machinist's ~35° tilt —
    /// an isometric-flavored view where X, Y, and Z all read at a glance.
    /// The viewer stands at the +X −Y corner, FRONT-RIGHT of the drawing
    /// sheet (Phase 43 slice 3): +X runs right-and-down across the screen,
    /// +Y right-and-up, +Z up — the way the plan minimap reads (+X right,
    /// +Y up) and the way the `front` stance looks (from −Y). At the old
    /// +π/4 the viewer stood at +X +Y looking back, +X ran LEFT, and a drag
    /// on the map moved the picture against the hand.
    pub fn default_orbit() -> Camera {
        Camera { yaw: -std::f64::consts::FRAC_PI_4, pitch: 0.615, scale: 1.0, target: [0.0; 3] }
    }

    /// The drawing-sheet PLAN stance: straight down, +X right across the
    /// screen, +Y up it — the orientation every print's plan view uses. At
    /// yaw −π/2 the projection's basis rows become exactly (+X, +Y), which
    /// the plan-stance pin in `tests/view_tests.rs` holds. The minimap is
    /// this camera, fitted and never orbited — a stance, not a second
    /// projection code path.
    pub fn plan() -> Camera {
        Camera {
            yaw: -std::f64::consts::FRAC_PI_2,
            pitch: std::f64::consts::FRAC_PI_2,
            scale: 1.0,
            target: [0.0; 3],
        }
    }

    /// Project a world point to (screen_x, screen_y, depth), +y UP — the
    /// caller flips for a y-down surface. TOTAL: orthographic projection has
    /// no behind-camera case, so there is nothing to be `Option` about.
    /// `depth` grows away from the viewer (painter's-algorithm material for a
    /// later slice; unused by the wireframe painter today) — hence the
    /// negation: (right, up, right×up) is a right-handed basis whose third
    /// axis points OUT of the screen, so away-from-viewer is its negative.
    pub fn project(&self, p: [f64; 3]) -> (f32, f32, f32) {
        self.lens().project(p)
    }

    /// The stance with its trigonometry already taken — the same projection,
    /// hoisted, for callers that project MANY points at one camera.
    ///
    /// `project` is a pure function of the stance and the point, so a loop
    /// over a heightfield recomputes `yaw.psin_cos()` and `pitch.psin_cos()`
    /// per point and throws them away: the sim surface asks for 265k
    /// projections a frame (53k centroids to sort, then four corners each),
    /// which is 530k sin_cos calls to produce four distinct numbers. Taking
    /// them once made the sim layer cost 7.16 ms rather than what it did.
    ///
    /// This is a hoist, not a second projection: `Camera::project` is this
    /// function with the stance taken per call, so there remains exactly ONE
    /// place where the arithmetic lives, and `test_the_lens_is_the_camera`
    /// pins the two BITWISE equal across stances rather than trusting the
    /// reading.
    pub fn lens(&self) -> Lens {
        let (sy, cy) = self.yaw.psin_cos();
        let (sp, cp) = self.pitch.psin_cos();
        Lens { sy, cy, sp, cp, scale: self.scale, target: self.target }
    }

    /// The world direction that runs RIGHT across the screen — the same basis
    /// row `project()` uses, exposed so panning can move the target along the
    /// screen plane without the frontend restating the trigonometry.
    pub fn right(&self) -> [f64; 3] {
        let (sy, cy) = self.yaw.psin_cos();
        [-sy, cy, 0.0]
    }

    /// The world direction that runs UP the screen — `project()`'s second row.
    pub fn up(&self) -> [f64; 3] {
        let (sy, cy) = self.yaw.psin_cos();
        let (sp, cp) = self.pitch.psin_cos();
        [-sp * cy, -sp * sy, cp]
    }

    /// Center the target and set the scale so the AABB's bounding sphere
    /// fits a viewport of `w x h` screen units with a margin, regardless of
    /// orientation (a sphere's silhouette is orientation-free — the fit
    /// cannot be invalidated by orbiting).
    ///
    /// Positive-finite only, the guard `plan_fit` already carries and this
    /// sibling did not: a viewport with a zero side (a first frame laid out
    /// before the panel has width, a window opened collapsed) computed a
    /// scale of ZERO here and latched it, and a zero scale is not a small
    /// viewport — it is a division by zero in `unproject_screen`, which
    /// `viewport_footprint` calls four times a frame for the minimap's
    /// indicator. Non-finite world corners reach the painter from there, and
    /// non-finite coordinates do not draw small; they draw as slivers across
    /// the frame.
    pub fn fit_to_aabb(&mut self, aabb: [f64; 6], w: f32, h: f32) {
        self.target = aabb_center(aabb);
        let r = ((aabb[3] - aabb[0]).powi(2) + (aabb[4] - aabb[1]).powi(2) + (aabb[5] - aabb[2]).powi(2))
            .sqrt()
            / 2.0;
        let s = 0.45 * f64::from(w.min(h)) / r;
        if r > 0.0 && s.is_finite() && s > 0.0 {
            self.scale = s;
        }
    }

    /// Zoom by a multiplicative factor, clamped to [`SCALE_MIN`,
    /// `SCALE_MAX`]. The clamp lives here rather than at the scroll handler
    /// because the bound is a property of the PROJECTION, not of the input
    /// device: `project` returns `f32`, so a scale that carries an ordinary
    /// millimetre past `f32::MAX` hands the painter infinities, and a scale
    /// that reaches zero divides by zero in `unproject_screen` and the pan.
    /// Both ends produce the same artifact — non-finite vertices, drawn as
    /// slivers running off the frame — from a camera the geometry modules
    /// have no way to reject.
    /// Put a world point at the screen's centre, stance and scale unchanged —
    /// what "go there" means for a chosen annotation.
    pub fn look_at(&mut self, p: [f64; 3]) {
        self.target = p;
    }

    pub fn zoom_by(&mut self, factor: f64) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        self.scale = (self.scale * factor).clamp(SCALE_MIN, SCALE_MAX);
    }

    /// The plan stance fitted to the AABB's XY FOOTPRINT — the minimap's fit.
    /// Deliberately not `fit_to_aabb`: that fit buys orientation-proofness
    /// with the bounding sphere, and a camera that never orbits pays the
    /// shrink without the protection — a tall part must not shrink its own
    /// plan. Degenerate footprints (a zero-extent axis) fall back to the
    /// other axis, or to unit scale when both are points.
    pub fn plan_fit(aabb: [f64; 6], w: f32, h: f32) -> Camera {
        let mut cam = Camera::plan();
        cam.target = aabb_center(aabb);
        let (bw, bl) = (aabb[3] - aabb[0], aabb[4] - aabb[1]);
        let sx = if bw > 0.0 { f64::from(w) / bw } else { f64::INFINITY };
        let sy = if bl > 0.0 { f64::from(h) / bl } else { f64::INFINITY };
        let s = 0.9 * sx.min(sy); // the tight axis fills 90% of its span
        // Positive-finite only: a degenerate footprint (or a zero-size map)
        // keeps unit scale rather than poisoning `unproject_screen` with a
        // division by zero.
        if s.is_finite() && s > 0.0 {
            cam.scale = s;
        }
        cam
    }

    /// The world point on the camera plane (the plane through `target`
    /// normal to the view direction) that projects to screen `(sx, sy)` —
    /// `project`'s inverse restricted to that plane, built on
    /// `right()`/`up()`. Those are the same basis rows `project()` spells
    /// inline (kept inline there for the per-point hot path); the
    /// round-trip pin in `tests/view_tests.rs` is the lock that keeps the
    /// two spellings one. The plane is where depth reads 0, which is what a
    /// screen point *means* in an orthographic view: the whole depth ray
    /// projects there, and the plane point is its anchor.
    pub fn unproject_screen(&self, sx: f64, sy: f64) -> [f64; 3] {
        let (r, u) = (self.right(), self.up());
        let mut p = self.target;
        for k in 0..3 {
            p[k] += r[k] * (sx / self.scale) + u[k] * (sy / self.scale);
        }
        p
    }

    /// Zoom about a screen point (`sx`, `sy` from the viewport's centre,
    /// +y up): the world point under the cursor stays under it. The
    /// scale changes as `zoom_by` does (same clamp); the target moves
    /// along the camera plane by what the point would otherwise have
    /// slid — the CAD-trained hand's expectation, and the one camera
    /// arithmetic DESIGN.md §9 admitted (2026-09-17). Pure and pinned.
    pub fn zoom_at(&mut self, sx: f64, sy: f64, factor: f64) {
        let before = self.scale;
        self.zoom_by(factor);
        if self.scale == before || !sx.is_finite() || !sy.is_finite() {
            return;
        }
        let (r, u) = (self.right(), self.up());
        let slide = 1.0 / before - 1.0 / self.scale;
        for k in 0..3 {
            self.target[k] += (r[k] * sx + u[k] * sy) * slide;
        }
    }

    /// Turn the view by a drag: `dyaw` about world +Z, `dpitch` toward
    /// straight down. A turntable stops the tilt at straight down and
    /// straight up; a free one goes on over the top. `pivot` is the world
    /// point that stays where it is on the screen; `None` is the target,
    /// the middle of the viewport.
    pub fn orbit(&mut self, dyaw: f64, dpitch: f64, style: crate::theme::Orbit, pivot: Option<[f64; 3]>) {
        use std::f64::consts::{FRAC_PI_2, PI, TAU};
        // The target as the pivot sees it, in the screen's own axes: kept
        // through the turn, it leaves the pivot where it was drawn.
        let basis = |c: &Camera| {
            let (r, u) = (c.right(), c.up());
            [r, u, [r[1] * u[2] - r[2] * u[1], r[2] * u[0] - r[0] * u[2], r[0] * u[1] - r[1] * u[0]]]
        };
        let held = pivot.map(|c| (c, basis(self).map(|b| (0..3).map(|k| (self.target[k] - c[k]) * b[k]).sum::<f64>())));
        self.yaw += dyaw;
        self.pitch = match style {
            crate::theme::Orbit::Turntable => (self.pitch + dpitch).clamp(-FRAC_PI_2, FRAC_PI_2),
            // Kept within one turn, so a focus never unwinds several.
            crate::theme::Orbit::Free => (self.pitch + dpitch + PI).rem_euclid(TAU) - PI,
        };
        if let Some((c, along)) = held {
            let b = basis(self);
            for k in 0..3 {
                self.target[k] = c[k] + along[0] * b[0][k] + along[1] * b[1][k] + along[2] * b[2][k];
            }
        }
    }

    /// A standard view (DESIGN.md §5.5, 2026-09-17): the stance's yaw and
    /// pitch, the target and the scale kept — where you were looking, from
    /// a drawing's direction. Plan is `Camera::plan`'s stance (+X right, +Y
    /// up, straight down); front looks along +Y from −Y (+X right, +Z up);
    /// right looks along −X from +X (+Y right, +Z up) — the axis views the
    /// projection pins hold.
    pub fn set_stance(&mut self, stance: Stance) {
        let (yaw, pitch) = match stance {
            Stance::Plan => (-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2),
            Stance::Front => (-std::f64::consts::FRAC_PI_2, 0.0),
            Stance::Right => (0.0, 0.0),
        };
        self.yaw = yaw;
        self.pitch = pitch;
    }

    /// The axis triad: where world +X, +Y and +Z point on the screen under
    /// this stance, as unit-length screen directions (+y up) — `project`
    /// of the three unit vectors from the origin, scale-free. An axis
    /// looking straight at the viewer projects to nothing and reads as a
    /// dot: its direction is `(0, 0)` and its length 0, which the drawing
    /// shows as a point rather than inventing a direction.
    pub fn triad(&self) -> [(f32, f32, &'static str); 3] {
        let mut cam = *self;
        cam.scale = 1.0;
        cam.target = [0.0; 3];
        let axis = |v: [f64; 3], name: &'static str| {
            let (x, y, _) = cam.project(v);
            (x, y, name)
        };
        [axis([1.0, 0.0, 0.0], "X"), axis([0.0, 1.0, 0.0], "Y"), axis([0.0, 0.0, 1.0], "Z")]
    }
}

// ---- the focus: an act, not a side effect (Phase 41 slice 1) -----------------
//
// `DECISIONS.md` → "Phase 41 opens on the ruling…". Selection framing CENTRES
// and never zooms (Phase 23 fork 4, re-affirmed 2026-10-04 by the user:
// "keep it centered only"); the focus is its own key. It puts the camera at
// an oblique look down the tool axis, fitted to what is selected, and it
// gets there by a tween the shell steps — every frame of which is this
// pure arithmetic, so the move cannot land anywhere the pins do not reach.

/// The focused stance's pitch: steeper than the default orbit's machinist
/// tilt (0.615) so a floor reads as a floor, shy of straight down so the
/// walls still have height. Yaw is KEPT — the user's turn of the part is
/// theirs; only the tilt and the fit are the act's.
pub const FOCUS_PITCH: f64 = 1.0;

/// How long the move takes, in seconds. Short enough to read as one
/// gesture, long enough that the eye tracks what moved where.
pub const FOCUS_SECS: f64 = 0.35;

/// The ease: smoothstep. Zero velocity at both ends — a camera that
/// arrives at speed overshoots in the eye even when it stops exactly.
pub fn ease(t: f64) -> f64 {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 1.0 };
    t * t * (3.0 - 2.0 * t)
}

impl Camera {
    /// This camera focused on `aabb`: yaw kept, pitch `FOCUS_PITCH`, the
    /// box fitted as `fit_to_aabb` fits (the bounding sphere, so the fit
    /// holds at the new pitch and at any later orbit).
    pub fn focused_on(&self, aabb: [f64; 6], w: f32, h: f32) -> Camera {
        let mut cam = *self;
        cam.pitch = FOCUS_PITCH;
        cam.fit_to_aabb(aabb, w, h);
        cam
    }

    /// The camera `t` of the way from `self` to `to`, `t` in [0, 1]: yaw
    /// along the SHORTER arc (a focus from yaw 3.1 to −3.1 turns a tenth
    /// of a radian, not six), pitch and target linear, and the scale in
    /// LOG space — a zoom from 1 to 100 passes 10 at the midpoint, which
    /// is the only midpoint that looks like half a zoom. `t` outside the
    /// range is clamped; a non-finite `t` arrives.
    pub fn toward(&self, to: &Camera, t: f64) -> Camera {
        let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 1.0 };
        let tau = std::f64::consts::TAU;
        let mut dyaw = (to.yaw - self.yaw) % tau;
        if dyaw > std::f64::consts::PI {
            dyaw -= tau;
        } else if dyaw < -std::f64::consts::PI {
            dyaw += tau;
        }
        let lerp = |a: f64, b: f64| a + (b - a) * t;
        let mut target = [0.0; 3];
        for k in 0..3 {
            target[k] = lerp(self.target[k], to.target[k]);
        }
        Camera {
            yaw: if t >= 1.0 { to.yaw } else { self.yaw + dyaw * t },
            pitch: lerp(self.pitch, to.pitch),
            scale: if t >= 1.0 { to.scale } else { (lerp(self.scale.pln(), to.scale.pln())).pexp() },
            target,
        }
    }
}

/// What the focus fits: the selected feature's edges, or the selected
/// operation's path together with the edges of the feature it works (a
/// problem's row resolves to its op, then its feature, as the selection
/// helpers already read it). `None` when nothing selected has a place —
/// the shell then fits the scene, so the key always does something.
pub fn focus_aabb(p: &Part, sel: Selection, rows: &[FindingRow], edges: &[FeatureEdges], paths: &[OpPath]) -> Option<[f64; 6]> {
    let empty = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    let mut bb = empty;
    if let Some(wi) = selected_feature(sel, rows, edges) {
        bb = edges_aabb(&edges[wi..=wi], bb);
    }
    if let Some(op) = selected_op(sel, rows) {
        // The features the op names (`outline::op_features`, the one
        // reading of an op's references) — their edges fold in, so an op
        // on a dome frames the dome and not only the rings it has cut.
        if let Some(o) = p.operations.get(op) {
            for name in crate::outline::op_features(o) {
                if let Some(e) = edges.iter().find(|e| e.name == name) {
                    bb = edges_aabb(std::slice::from_ref(e), bb);
                }
            }
        }
        for path in paths.iter().filter(|p| p.op_idx == op) {
            for pl in &path.polylines {
                for p in &pl.pts {
                    for k in 0..3 {
                        bb[k] = bb[k].min(p[k]);
                        bb[k + 3] = bb[k + 3].max(p[k]);
                    }
                }
            }
        }
    }
    (bb[0] <= bb[3]).then_some(bb)
}

/// The three drawing directions a key can put the camera at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stance {
    Plan,
    Front,
    Right,
}

/// Where the camera looks when an operation is selected (DESIGN.md §6, the
/// one side effect, grown 2026-09-17): the centre of the op's emitted
/// path's box, scale untouched — the path is brought to the middle of the
/// screen, never zoomed to, because a zoom nobody asked for is the camera
/// jump a hand cannot predict. `None` for an op with no path (nothing
/// emitted, a rejected project) — the camera stays.
pub fn frame_point(paths: &[OpPath], op: usize) -> Option<[f64; 3]> {
    let path = paths.iter().find(|p| p.op_idx == op)?;
    let mut bb = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    let mut any = false;
    for pl in &path.polylines {
        for p in &pl.pts {
            any = true;
            for k in 0..3 {
                bb[k] = bb[k].min(p[k]);
                bb[k + 3] = bb[k + 3].max(p[k]);
            }
        }
    }
    any.then(|| aabb_center(bb))
}

/// The box's center — one home for the arithmetic both fits share.
pub fn aabb_center(aabb: [f64; 6]) -> [f64; 3] {
    [(aabb[0] + aabb[3]) / 2.0, (aabb[1] + aabb[4]) / 2.0, (aabb[2] + aabb[5]) / 2.0]
}

/// The main camera's visible region, as four world corners — the screen
/// rectangle's corners unprojected onto the camera plane, in closed-quad
/// order. On the minimap this quad IS the viewport indicator, and its plan
/// footprint degenerates honestly: an edge-on view (pitch 0) covers a
/// SEGMENT of the XY plane, so the drawn quad collapses to one.
pub fn viewport_footprint(cam: &Camera, w: f32, h: f32) -> [[f64; 3]; 4] {
    let (hw, hh) = (f64::from(w) / 2.0, f64::from(h) / 2.0);
    [
        cam.unproject_screen(-hw, -hh),
        cam.unproject_screen(hw, -hh),
        cam.unproject_screen(hw, hh),
        cam.unproject_screen(-hw, hh),
    ]
}

/// What the MAP should actually draw for the viewport: the footprint clipped
/// to the stock's XY rectangle — which is the stock's own rectangle when the
/// view holds all of it — or `None` when the view is aimed off the stock
/// entirely. (Until 2026-08-29 a view holding the whole stock drew nothing;
/// at fullscreen, where the once-only fit leaves the whole part in view,
/// that read as the indicator vanishing — the frame lying ON the part is
/// the honest "everything is in view", and it is what a person expects to
/// see clamp to the map's edge.)
///
/// `viewport_footprint` above is the honest region and stays the pinned home,
/// but drawn raw it is the wrong INDICATOR. Zoomed out, the region is larger
/// than the map and only its far edges cross the frame — a bright rotated
/// quad slicing over the part, saying "cropped" at the one moment nothing is.
/// Clipped, the indicator only ever draws a boundary lying ON the part, which
/// is the question a map is asked: where does what I am looking at end?
///
/// The degenerate case keeps its meaning: an edge-on view (pitch 0) covers a
/// SEGMENT of the XY plane, and clipping a segment to the rectangle is still
/// that segment, trimmed.
pub fn viewport_indicator(cam: &Camera, w: f32, h: f32, aabb: [f64; 6]) -> Option<Vec<[f64; 3]>> {
    let fp = viewport_footprint(cam, w, h);
    let quad: Vec<[f64; 2]> = fp.iter().map(|p| [p[0], p[1]]).collect();
    let (x0, y0, x1, y1) = (aabb[0], aabb[1], aabb[3], aabb[4]);
    // A view holding every corner of the stock clips to the stock's own
    // rectangle below — the frame lying on the part — and is drawn, not
    // skipped: the clip is exact either way.

    // Sutherland-Hodgman against the four half-planes of the stock's
    // rectangle — the window is convex, so clipping edge by edge is exact and
    // the order does not matter.
    let mut poly = quad;
    for (axis, limit, keep_below) in
        [(0usize, x1, true), (0, x0, false), (1usize, y1, true), (1, y0, false)]
    {
        let inside = |p: &[f64; 2]| if keep_below { p[axis] <= limit } else { p[axis] >= limit };
        let mut next: Vec<[f64; 2]> = Vec::with_capacity(poly.len() + 1);
        for k in 0..poly.len() {
            let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
            let (ain, bin) = (inside(&a), inside(&b));
            if ain {
                next.push(a);
            }
            if ain != bin {
                let d = b[axis] - a[axis];
                // Only an edge that CROSSES the limit reaches here, so the
                // span along `axis` is non-zero and t is finite.
                if d != 0.0 {
                    let t = (limit - a[axis]) / d;
                    next.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
                }
            }
        }
        poly = next;
        if poly.is_empty() {
            return None; // the view looks entirely off the stock
        }
    }
    // Drawn on the stock's top face, where the map reads everything else.
    Some(poly.into_iter().map(|p| [p[0], p[1], aabb[5]]).collect())
}

// ---- shared scene facts -----------------------------------------------------

/// The stock's box: `stock.z` is the TOP datum — the stock hangs below it
/// (DECISIONS.md "Vertical occupancy is `extent`…"; `stock::material_at` is
/// the authority this restates). Drawing it the other way up puts the stock
/// floating above the table, which is why this helper exists and is pinned.
pub fn stock_aabb(s: &Stock) -> [f64; 6] {
    [s.x, s.y, s.z - s.h, s.x + s.w, s.y + s.l, s.z]
}

/// The scene's fit box: the stock plus everything any path visits (rapids
/// included — a rapid plane above the stock is part of what "fit" means).
pub fn scene_aabb(s: &Stock, paths: &[OpPath]) -> [f64; 6] {
    let mut bb = stock_aabb(s);
    for path in paths {
        for pl in &path.polylines {
            for p in &pl.pts {
                bb[0] = bb[0].min(p[0]);
                bb[1] = bb[1].min(p[1]);
                bb[2] = bb[2].min(p[2]);
                bb[3] = bb[3].max(p[0]);
                bb[4] = bb[4].max(p[1]);
                bb[5] = bb[5].max(p[2]);
            }
        }
    }
    bb
}

// ---- the colours: slots in `theme`, paint rules here ------------------------
//
// Every colour this module used to name — the roles, the ground, the annotations'
// families, the stocks' — is a named slot on `theme::Theme` now, with `Theme::default()` carrying the exact values that stood
// here. What stays in `view` is the paint RULES that combine slots with
// state (`feed_paint`), and the presentation numbers; a view model that
// bakes a colour takes the `Theme` it reads as a parameter, so the window
// and any future CLI-side map read the same slot through the same rule.

/// The feed-motion paint rule — one home for its two presentations (the
/// main view and the map, which must read identically): the selected op
/// uses the `select` slot, everything else its role's slot, dimmed to the
/// returned alpha when some OTHER op holds the selection.
pub fn feed_paint(theme: &Theme, role: OpRole, selected: bool, dimmed: bool) -> ([u8; 3], u8) {
    let rgb = if selected { theme.select } else { theme.role_color(role) };
    (rgb, if dimmed { DIMMED_ALPHA } else { 255 })
}

/// The alpha a toolpath is drawn at while another operation is selected.
///
/// A presentation number, but a measured one: at 60 the dimmed roles
/// composited on the built-in ground measured 1.5–1.6:1 (WCAG), below
/// the noise floor of most panels, so selecting one operation erased the
/// rest of the program; at 160 the dimmest role (untagged) measures 3.1:1
/// and the other two 3.9–4.0:1 (`DECISIONS.md` → "The UI/UX review
/// intake…"), pinned.
pub const DIMMED_ALPHA: u8 = 160;

// ---- the annotations --------------------------------------------------------
//
// Presentation NUMBERS (stroke widths, the cross size of an annotation) stay in
// the shell; MEANINGS (what is drawn in what colour) are theme slots.

/// Where an annotation sits: a point, or a segment between two.
#[derive(Clone, Debug, PartialEq)]
pub enum AnnotationShape {
    Point([f64; 3]),
    Segment([f64; 3], [f64; 3]),
}

/// **A fact with a place** — the seam every pane draws through.
/// A finding's coordinate, a removal's boundary, a tool's tip: anything the
/// engine can state that has somewhere to be in the part's space becomes a
/// annotation, and the shell paints annotations one way (a cross or a line, a label)
/// without knowing what kind of fact it is. The colour is a SLOT chosen by
/// the view model that made the annotation (a family's colour, a stock's), never
/// by the shell; the label is the fact's own sentence, or its head.
#[derive(Clone, Debug, PartialEq)]
pub struct Annotation {
    pub shape: AnnotationShape,
    pub color: [u8; 3],
    pub label: String,
}

impl Annotation {
    /// The point a camera goes to when the annotation is chosen: the point itself,
    /// or a segment's midpoint.
    pub fn anchor(&self) -> [f64; 3] {
        match &self.shape {
            AnnotationShape::Point(p) => *p,
            AnnotationShape::Segment(a, b) => {
                [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0]
            }
        }
    }
}

// ---- the findings, as rows and annotations ----------------------------------
//
// The window's pane is a view of the check's check result — the SAME `Finding`
// structs `--check=json` serialises, never re-derived — and a finding whose
// sentence names a place becomes an annotation at it. The place is read out of the
// sentence because that is where every producer puts it today (the entry
// flag's "at (x, y, z)", the probes' contact point, the fixture's "near (x,
// y)", the violation's "the lowest at (x, y, z)"), and the original DRAFT
// blessed reading it there (fork 2A, "where its message carries a place").
// A structured `at` on `Finding` is the obvious successor the day a second
// consumer wants it; until then one reader, one rule, pinned on every
// family's real sentence.

/// The ONE place in the window that reads structure out of prose, and the
/// one named exception to DESIGN.md I7: the refusal. Every finding carries
/// its place as a typed field now; the refusal is a `String` on the
/// `--check=json` wire and in `gcode::program`'s `Err`, so a typed refusal
/// is a public-API change and stays a follow-up. Until then its annotation
/// is read from its sentence: the first `(x, y, z)` as a point, else the
/// first `(x, y)` lifted to `lift_z` (the stock's top), else nothing.
/// Numbers only: `(Mesh)` and `(~388.1 mm³)` are not places. Called on
/// the refusal row and nowhere else.
pub fn refusal_place(sentence: &str, lift_z: f64) -> Option<[f64; 3]> {
    let mut pair: Option<[f64; 3]> = None;
    let bytes = sentence.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'(' {
            if let Some(close) = sentence[i + 1..].find(')') {
                let inner = &sentence[i + 1..i + 1 + close];
                let nums: Vec<Option<f64>> =
                    inner.split(',').map(|s| s.trim().parse::<f64>().ok()).collect();
                if nums.iter().all(Option::is_some) {
                    match nums.len() {
                        3 => return Some([nums[0].unwrap(), nums[1].unwrap(), nums[2].unwrap()]),
                        2 if pair.is_none() => pair = Some([nums[0].unwrap(), nums[1].unwrap(), lift_z]),
                        _ => {}
                    }
                }
                i += close + 1;
            }
        }
        i += 1;
    }
    pair
}

/// One row of the findings pane: the family's word and the subject as the
/// head, the sentence as the body, the family's colour, and the annotation it
/// made (an index into the annotations list) when its sentence named a place.
/// `op` is the subject op's index when it has one — the row selects that op's
/// toolpath when clicked, the same selection the outline pane makes.
#[derive(Clone, Debug, PartialEq)]
pub struct FindingRow {
    pub head: String,
    pub text: String,
    pub color: [u8; 3],
    pub annotation: Option<usize>,
    pub op: Option<usize>,
    pub feature: String,
    /// The finding's typed place, tip and tool (`parse::Finding`'s own
    /// fields, copied) — what the annotation and the tool position derive
    /// from. Never read out of `text`.
    pub place: Option<[f64; 3]>,
    pub at: Option<[f64; 3]>,
    pub tool: Option<usize>,
    /// A check that could not run (the `decline` family, Phase 50 slice
    /// 7): counted apart from the problems, never in the red badge.
    pub unchecked: bool,
}

/// The pane and its annotations, from the check result: the error first when there is
/// one, then every finding grouped by family in `FAMILY_ORDER`, each family's
/// findings in the order the pipeline produced them. Marks are returned
/// beside the rows so the shell paints one list and indexes it from the other.
pub fn finding_rows(
    v: &crate::check_result::CheckResult,
    lift_z: f64,
    theme: &Theme,
) -> (Vec<FindingRow>, Vec<Annotation>) {
    let mut rows = Vec::new();
    let mut annotations = Vec::new();
    let mut push = |head: String, text: &str, color: [u8; 3], op: Option<usize>, feature: String, label: &str, place: Option<[f64; 3]>, at: Option<[f64; 3]>, tool: Option<usize>, unchecked: bool| {
        let annotation = place.map(|p| {
            annotations.push(Annotation { shape: AnnotationShape::Point(p), color, label: label.to_string() });
            annotations.len() - 1
        });
        rows.push(FindingRow { head, text: text.to_string(), color, annotation, op, feature, place, at, tool, unchecked });
    };
    // A window with no project open has nothing rejected.
    if let Some(r) = v.refusal.as_ref().filter(|r| !r.starts_with(NO_PROJECT)) {
        let text = r.strip_prefix("error: ").unwrap_or(r);
        // The one row whose place is read from its sentence (`refusal_place`).
        push("REJECTED".to_string(), text, theme.refusal, None, String::new(), "rejected", refusal_place(text, lift_z), None, None, false);
    }
    for family in crate::check_result::FAMILY_ORDER {
        // The word on screen is the machinist's (Phase 50 slice 7), on
        // the row and on its mark in the viewport alike: a mark labelled
        // `body_foul` beside a row headed "Holder collision" was two
        // names for one thing. The wire's word stays on the wire.
        let word = family_display(family);
        let label = word;
        let unchecked = family == parse::FindingFamily::Decline;
        let mut said: Vec<(&parse::FindingSubject, &str)> = Vec::new();
        for f in v.findings.iter().filter(|f| f.family == family) {
            // A decline that repeats an earlier one, subject and sentence,
            // is said once on screen (the wire still carries both).
            if unchecked {
                if said.iter().any(|(s, m)| *s == &f.subject && *m == f.message) {
                    continue;
                }
                said.push((&f.subject, &f.message));
            }
            let (head, op, feature) = match &f.subject {
                parse::FindingSubject::Op { index, feature } => {
                    (format!("{word} · op [{}] '{}'", index, feature), Some(*index), feature.clone())
                }
                parse::FindingSubject::Feature { name } => {
                    (format!("{word} · '{}'", name), None, name.clone())
                }
            };
            push(head, &f.message, theme.family_color(family), op, feature, label, f.place, f.at, f.tool, unchecked);
        }
    }
    (rows, annotations)
}

// ---- the feature wireframe --------------------------------------------------
//
// One match arm per declared `Feature` variant — the per-variant geometry
// slice 0 deferred with "written once, when drawn". This is that home, and
// the later click-a-face slice hit-tests the same struct. The rule of the
// match: every number comes from the variant's OWN homes — the
// `parse::*_cl_point` family at `r_off = 0` IS the nominal surface, the
// parameter ranges and z-datum arithmetic mirror `stock::removals_for`'s
// reviewed arms, circles flatten through `ops::finish::circle_segments`
// (the one chord-tolerance home) — and the tiny meridians with no home
// (sphere, cone, torus) are restated inline under the Cone precedent
// ("none worth inventing for a formula this size").
//
// What is drawn is the DECLARED surface — which equals the machinable band
// by construction: the non-undercut constructor rule means no variant
// declares more (a Sphere is its dome, a catenoid stops at its waist), so
// there is no full solid in the model to draw and no per-variant judgment
// call to make.
//
// Density vs tolerance, split deliberately: parallels are circles, and
// circles have a tolerance home, so their point counts come from
// `circle_segments`. Meridian COUNT and points-per-meridian are presentation
// density — a drawing choice, not a precision claim — and live in the named
// constants below.

/// How many meridians a revolved surface shows.
pub const EDGE_MERIDIANS: usize = 12;
/// Points sampled along one meridian.
pub const EDGE_MERIDIAN_PTS: usize = 33;
/// Parallels per revolved surface, endpoints included — so every named end
/// circle (equator, mouth, waist, boundary, base) is drawn and pinnable. ODD on
/// purpose: an odd count also lands the parameter MIDPOINT, which is itself
/// a named circle on the symmetric bands (the torus crown).
pub const EDGE_PARALLELS: usize = 5;

/// A parallel whose radius sits at or under this is a pole/apex — skipped,
/// because the meridians already meet there and a zero-radius ring is a
/// degenerate triple point.
const EDGE_POLE_EPS: f64 = 1e-9;

/// One feature's drawable geometry. Joined to the outline panel by NAME —
/// the features' join key, as `op_idx` is the ops' (`outline::FeatureNode`).
pub struct FeatureEdges {
    pub name: String,
    pub kind: &'static str,
    pub polylines: Vec<Vec<[f64; 3]>>,
}

/// A closed circle in a Z plane, flattened at the shared chord tolerance.
/// `circle_pts`' contract: it emits from the first step through 2π, so the
/// angle-0 point is prepended here to close the loop from its true start.
fn ring(x: f64, y: f64, rho: f64, z: f64) -> Vec<[f64; 3]> {
    let mut pts = vec![[x + rho, y, z]];
    pts.extend(crate::ops::finish::circle_pts(x, y, rho).into_iter().map(|(px, py)| [px, py, z]));
    pts
}

/// A closed nominal ellipse ring (`ellipse_offset_pts` at `r_signed = 0`, the
/// promoted home — rotation arm included). Same closing contract as `ring`.
fn ellipse_ring(x: f64, y: f64, a: f64, b: f64, rot: Option<(f64, f64)>, z: f64) -> Vec<[f64; 3]> {
    let mut pts = vec![match rot {
        None => [x + a, y, z],
        Some((c, s)) => [x + a * c, y + a * s, z],
    }];
    pts.extend(
        crate::ops::finish::ellipse_offset_pts(x, y, a, b, 0.0, rot)
            .into_iter()
            .map(|(px, py)| [px, py, z]),
    );
    pts
}

/// The revolved-surface builder: meridians × parallels for a nominal
/// meridian `t -> (rho, z)` about the vertical axis through `(x, y)`.
fn revolve(
    x: f64,
    y: f64,
    t0: f64,
    t1: f64,
    meridian: impl Fn(f64) -> (f64, f64),
) -> Vec<Vec<[f64; 3]>> {
    let mut out = Vec::new();
    for k in 0..EDGE_MERIDIANS {
        let az = 2.0 * std::f64::consts::PI * (k as f64) / (EDGE_MERIDIANS as f64);
        let (ca, sa) = (az.pcos(), az.psin());
        let mut pts = Vec::with_capacity(EDGE_MERIDIAN_PTS);
        for i in 0..EDGE_MERIDIAN_PTS {
            let t = t0 + (t1 - t0) * (i as f64) / ((EDGE_MERIDIAN_PTS - 1) as f64);
            let (rho, z) = meridian(t);
            pts.push([x + rho * ca, y + rho * sa, z]);
        }
        out.push(pts);
    }
    for j in 0..EDGE_PARALLELS {
        let t = t0 + (t1 - t0) * (j as f64) / ((EDGE_PARALLELS - 1) as f64);
        let (rho, z) = meridian(t);
        if rho > EDGE_POLE_EPS {
            out.push(ring(x, y, rho, z));
        }
    }
    out
}

/// Two stacked loops plus vertical ties at shared vertices — the prismatic
/// vocabulary. Ties are strided so a densely-authored contour doesn't become
/// a picket fence.
fn loops_and_ties(top: Vec<[f64; 3]>, bottom: Vec<[f64; 3]>) -> Vec<Vec<[f64; 3]>> {
    const MAX_TIES: usize = 16;
    let n = top.len().saturating_sub(1); // both loops are closed; skip the repeat
    let stride = n.div_ceil(MAX_TIES).max(1);
    let mut out = Vec::new();
    for i in (0..n).step_by(stride) {
        out.push(vec![top[i], bottom[i]]);
    }
    out.push(top);
    out.push(bottom);
    out
}

/// A closed loop from authored 2D points at a Z plane.
fn closed_loop(points: &[parse::Pt], z: f64) -> Vec<[f64; 3]> {
    let mut pts: Vec<[f64; 3]> = points.iter().map(|p| [p.x, p.y, z]).collect();
    if let Some(&first) = pts.first() {
        pts.push(first);
    }
    pts
}

/// The circular pair's four vertical ties, at the quarter azimuths.
fn quarter_ties(x: f64, y: f64, r: f64, z_top: f64, z_bot: f64) -> Vec<Vec<[f64; 3]>> {
    [(r, 0.0), (0.0, r), (-r, 0.0), (0.0, -r)]
        .iter()
        .map(|(dx, dy)| vec![[x + dx, y + dy, z_top], [x + dx, y + dy, z_bot]])
        .collect()
}

/// Decoration circles on a circular-edged feature, TOOL-FREE only: the
/// circles the declaration alone pins. The named residue (the csink frustum
/// floor, an angle-absent chamfer's second circle, every polyline-edge
/// decoration) needs the tool or the offsetter and is deliberately not drawn
/// — a picture must not guess (DECISIONS.md, the slice-2 entry).
fn decoration_rings(p: &Part, feature: &Feature) -> Vec<Vec<[f64; 3]>> {
    let mut out = Vec::new();

    if let Some(er) = parse::derive_edge_round(p, feature) {
        if let Some((x, y, wall_r)) = er.circle() {
            // The two tangency circles: on the wall one radius below the
            // face, and on the face one radius into the material.
            out.push(ring(x, y, wall_r, er.centre_z(0.0)));
            out.push(ring(x, y, er.centre(0.0, 0.0).0, er.z_top));
        }
    }
    if let Some(ec) = parse::derive_chamfer(p, feature) {
        if let Some((x, y, wall_r)) = ec.circle() {
            let inward = if ec.boss { -1.0 } else { 1.0 };
            // The leg the print declared pins its own circle; a declared
            // angle pins the other leg through the cone relation.
            let half = ec.spec.angle.map(|a| (a / 2.0).to_radians());
            let (depth, width) = match (ec.spec.leg, half) {
                (ChamferLeg::Depth(d), Some(h)) => (Some(d), Some(d * h.ptan())),
                (ChamferLeg::Depth(d), None) => (Some(d), None),
                (ChamferLeg::Width(w), Some(h)) => (Some(w / h.ptan()), Some(w)),
                (ChamferLeg::Width(w), None) => (None, Some(w)),
            };
            if let Some(d) = depth {
                out.push(ring(x, y, wall_r, ec.z_top - d));
            }
            if let Some(w) = width {
                out.push(ring(x, y, wall_r + inward * w, ec.z_top));
            }
        }
    }
    // The mouth treatments sit at the RIM (Phase 45 slice 2): the stock top
    // unless the bore says `z`, through the one home.
    if let Feature::CylinderBore { x, y, z, csink: Some(spec), .. } = feature {
        // The seat circle is the print's callout; the frustum floor needs
        // the TOOL's ground cone (its relieved tip flat) — named residue.
        out.push(ring(*x, *y, spec.seat_dia() / 2.0, derive_contour_top(p.stock.z, *z)));
    }
    if let Feature::CylinderBore { x, y, z, cbore: Some(cb), .. } = feature {
        let rim = derive_contour_top(p.stock.z, *z);
        out.push(ring(*x, *y, cb.dia / 2.0, rim));
        out.push(ring(*x, *y, cb.dia / 2.0, derive_z_cut(rim, cb.depth)));
    }
    out
}

/// Every declared feature's wireframe, name-sorted (`Part::features` is a
/// `HashMap` — the outline's own determinism argument).
pub fn feature_edges(p: &Part) -> Vec<FeatureEdges> {
    let mut names: Vec<&String> = p.features.keys().collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let feature = &p.features[name];
            let mut polylines = edge_polylines(p, feature);
            polylines.extend(decoration_rings(p, feature));
            FeatureEdges { name: name.clone(), kind: feature.kind(), polylines }
        })
        .collect()
}

/// The per-variant match. Exhaustive on purpose — a new `Feature` variant
/// stops compiling here instead of silently not drawing (the
/// `theme::Theme::role_color` rule). Parameter ranges and datums mirror
/// `stock::removals_for`'s arms.
fn edge_polylines(p: &Part, feature: &Feature) -> Vec<Vec<[f64; 3]>> {
    use std::f64::consts::{FRAC_PI_2, PI};
    let stock_top = p.stock.z;
    match feature {
        // The finished face, drawn across the stock's footprint — the plane's
        // own span is genuinely zero, so a rectangle is the whole truth.
        Feature::Plane { z } => vec![closed_loop(&crate::parse::derive_blank(&p.stock), *z)],

        // The bounded face: its loops, each closed, at its own height — a face
        // has no extent to tie down to.
        Feature::Face { z, loops } => loops.iter().map(|l| closed_loop(l, *z)).collect(),

        // The polyline pair: BOTH variants are top-down (`z` is the TOP datum
        // in each — parse.rs's derive_edge_round states it; the boss does NOT
        // follow CylinderBoss's base-up convention).
        Feature::ProfilePocket { points, z, extent, .. }
        | Feature::ProfileBoss { points, z, extent, .. } => {
            let top = derive_contour_top(stock_top, *z);
            loops_and_ties(closed_loop(points, top), closed_loop(points, top - extent))
        }

        // The 3D-wire pair: each point carries its own Z; the wall spans
        // [pt.z - extent, pt.z]. `extent == 0` is legal — a trim edge, one
        // edge, no second loop.
        Feature::ContourPocket { points, extent } | Feature::ContourBoss { points, extent } => {
            let mut top: Vec<[f64; 3]> = points.iter().map(|p3| [p3.x, p3.y, p3.z]).collect();
            if let Some(&first) = top.first() {
                top.push(first);
            }
            if *extent == 0.0 {
                return vec![top];
            }
            let bottom: Vec<[f64; 3]> =
                top.iter().map(|pt| [pt[0], pt[1], pt[2] - extent]).collect();
            loops_and_ties(top, bottom)
        }

        // The bore: its rim ring at the RIM (Phase 45 slice 2 — the stock
        // top unless `z` says otherwise, through the pocket's home), the
        // floor ring one extent below, and the quarter ties between them.
        Feature::CylinderBore { dia, extent, x, y, z, .. } => {
            let rim = derive_contour_top(stock_top, *z);
            let (r, z_bot) = (dia / 2.0, derive_z_cut(rim, *extent));
            let mut out = vec![ring(*x, *y, r, rim), ring(*x, *y, r, z_bot)];
            out.extend(quarter_ties(*x, *y, r, rim, z_bot));
            out
        }
        // The tilted bore (Phase 41 slice 5): its two horizontal sections
        // are congruent ellipses — semi-axis r/cos(tilt) along the heading,
        // r across it — the lower one displaced along the heading by
        // extent·tan(tilt); the ties between them at the quarter points are
        // generators, straight because the sections are translates. The
        // displacement is `derive_tilted_axis` scaled to the depth, never
        // restated trigonometry.
        Feature::TiltedBore { dia, extent, x, y, tilt, heading } => {
            let z_bot = derive_z_cut(stock_top, *extent);
            let r = dia / 2.0;
            let t = tilt.to_radians();
            let a = r / t.pcos();
            let rot = Some((heading.to_radians().pcos(), heading.to_radians().psin()));
            let d = crate::parse::derive_tilted_axis(*tilt, *heading);
            let k = extent / -d[2];
            let (x1, y1) = (x + d[0] * k, y + d[1] * k);
            let mut out = vec![ellipse_ring(*x, *y, a, r, rot, stock_top), ellipse_ring(x1, y1, a, r, rot, z_bot)];
            for q in 0..4 {
                let phi = std::f64::consts::FRAC_PI_2 * q as f64;
                let (px, py) = crate::ops::finish::ellipse_offset_pt(*x, *y, a, r, 0.0, phi, rot);
                out.push(vec![[px, py, stock_top], [px + d[0] * k, py + d[1] * k, z_bot]]);
            }
            out
        }
        Feature::CylinderBoss { dia, extent, x, y, z, .. } => {
            let r = dia / 2.0;
            let mut out = vec![ring(*x, *y, r, z + extent), ring(*x, *y, r, *z)];
            out.extend(quarter_ties(*x, *y, r, z + extent, *z));
            out
        }

        // ---- the revolved surfaces: the nominal meridian (r_off = 0) over
        // the same ranges the stock model walks.
        Feature::Sphere { dia, x, y, z } => {
            // The dome above the equator — the declared machinable half. No
            // meridian home exists (the Cone precedent); the polar form is
            // two lines.
            let r = dia / 2.0;
            revolve(*x, *y, 0.0, FRAC_PI_2, |phi| (r * phi.psin(), z + r * phi.pcos()))
        }
        Feature::Cone { dia, height, x, y, z } => {
            let (a, h) = (dia / 2.0, *height);
            revolve(*x, *y, 0.0, a, |rho| (rho, z + h * (1.0 - rho / a)))
        }
        Feature::Torus { major_dia, minor_dia, x, y, z } => {
            let (rm, rt) = (major_dia / 2.0, minor_dia / 2.0);
            // Upper tube half: outer equator -> crown -> inner equator.
            revolve(*x, *y, 0.0, PI, |phi| (rm + rt * phi.pcos(), z + rt * phi.psin()))
        }
        // The four cavity revolves read their meridian from the one home
        // (`parse::cavity_meridian`) — the wall is that home at `r_ball = 0`,
        // which is what "the nominal surface" means here. Each arm keeps its
        // own parameter DIRECTION, because the edge's point order is its own
        // business and the home only states which end is the mouth.
        Feature::SplineRevolve { meridian, x, y, z, .. } => {
            let (u0, u1) = meridian.domain();
            revolve(*x, *y, u0, u1, |u| {
                let (rho, z_rel) = meridian.eval(u);
                (rho, z + z_rel)
            })
        }
        Feature::SplineRevolveFunnel { x, y, .. } => {
            let m = parse::cavity_meridian(feature, stock_top).expect("a spline funnel is a cavity");
            revolve(*x, *y, m.mouth, m.floor, |u| m.at(u, 0.0))
        }

        // ---- the camp-3 pair: geometry in file frame, placement applied
        // here (the finish_mesh / finish_nurbs_patch precedent — ops shift up
        // front, and the wireframe has no op to do it for it).
        Feature::Mesh { triangles, x, y, z, .. } => {
            let mut seen: std::collections::HashSet<([u64; 3], [u64; 3])> =
                std::collections::HashSet::new();
            let key = |v: &[f64; 3]| [v[0].to_bits(), v[1].to_bits(), v[2].to_bits()];
            let mut out = Vec::new();
            for tri in triangles {
                for (ai, bi) in [(0, 1), (1, 2), (2, 0)] {
                    let (va, vb) = (&tri.v[ai], &tri.v[bi]);
                    let (ka, kb) = (key(va), key(vb));
                    let edge = if ka <= kb { (ka, kb) } else { (kb, ka) };
                    if seen.insert(edge) {
                        out.push(vec![
                            [va[0] + x, va[1] + y, va[2] + z],
                            [vb[0] + x, vb[1] + y, vb[2] + z],
                        ]);
                    }
                }
            }
            out
        }
        Feature::NurbsPatch { surface, x, y, z, .. } => {
            let us = densify(surface.distinct_knots_u(), 8);
            let vs = densify(surface.distinct_knots_v(), 8);
            let (v0, v1) = surface.domain_v();
            let (u0, u1) = surface.domain_u();
            let mut out = Vec::new();
            for &u in &us {
                out.push(iso_line(surface, *x, *y, *z, |s| (u, v0 + (v1 - v0) * s)));
            }
            for &v in &vs {
                out.push(iso_line(surface, *x, *y, *z, |s| (u0 + (u1 - u0) * s, v)));
            }
            out
        }
    }
}

/// One iso-parameter line over a patch, sampled at meridian density, with the
/// placement offset added per evaluated point (translation commutes — no
/// need to clone-and-shift the surface as the op does).
fn iso_line(
    surface: &crate::nurbs::BSplineSurface,
    x: f64,
    y: f64,
    z: f64,
    param: impl Fn(f64) -> (f64, f64),
) -> Vec<[f64; 3]> {
    (0..EDGE_MERIDIAN_PTS)
        .map(|i| {
            let s = i as f64 / (EDGE_MERIDIAN_PTS - 1) as f64;
            let (u, v) = param(s);
            let pt = surface.eval(u, v);
            [pt[0] + x, pt[1] + y, pt[2] + z]
        })
        .collect()
}

/// Grow a knot list to at least `min_n` values by inserting span midpoints —
/// a pure-Bézier patch has only boundary knots, and a grid of two lines per
/// direction would draw an empty rectangle.
fn densify(mut vals: Vec<f64>, min_n: usize) -> Vec<f64> {
    while vals.len() < min_n {
        let mut next = Vec::with_capacity(vals.len() * 2 - 1);
        for w in vals.windows(2) {
            next.push(w[0]);
            next.push((w[0] + w[1]) / 2.0);
        }
        next.push(*vals.last().expect("a knot vector has endpoints"));
        vals = next;
    }
    vals
}

// ---- the stock simulation surface -------------------------------------------
//
// ROADMAP item 3: see the swept-volume truth. The drawable form rests on the
// field's own lemma: when every stamp reaches forever upward
// (`VoxField::upward_closed`), each XY column's remaining material is ONE
// interval from the stock's bottom — the machined stock is exactly a
// heightfield. And a heightfield needs no depth buffer: its quads, drawn
// far-to-near, occlude themselves correctly under the painter's algorithm,
// which is why the depth-tested pipeline slice 1 expected here never
// arrives. When the lemma fails (a truncated form-cutter silhouette) or the
// field is incomplete, the surface DECLINES with the reason named — the rest
// report's "no claim" rule, drawn: an incomplete field over-states material,
// and a viewport of an over-statement shows stock that is not there.

/// The one fixed WORLD light the machined surface is lit by, canted off
/// vertical so floors and walls separate. World-anchored on purpose: orbiting
/// reads like walking around a lit part rather than carrying a headlamp.
/// Read by the sim pipeline's shader, which lights a vertex normal on the
/// GPU. It stays a Rust constant rather than a literal in the WGSL because
/// the shading law's inputs are a fact about how this project draws stock,
/// and `test_the_shared_light_separates_floor_from_wall` pins them here —
/// the shader itself is shell, and untested like the rest of it.
pub const SIM_LIGHT: [f64; 3] = [-0.30, -0.36, 0.88];
/// The ambient floor that keeps a wall facing away from the light legible
/// instead of black. Same two consumers, same reason.
pub const SIM_AMBIENT: f64 = 0.30;

/// The machined stock as a GPU-ready mesh: interleaved position+normal
/// vertices, flat indices, and the bounding sphere the depth range is cut
/// from. `id` is the FIELD's identity (`vox::VoxField::id`), so an upload can
/// be skipped exactly when the surface provably has not changed.
#[derive(Debug)]
pub struct StockSurface {
    /// Position xyz then normal xyz, per vertex — one buffer, one stride.
    pub verts: Vec<[f32; 6]>,
    pub tris: Vec<u32>,
    pub center: [f64; 3],
    pub radius: f64,
    pub id: u64,
}

impl StockSurface {
    /// The depth-range radius for a camera aimed at `target`. Depth is
    /// measured from the target, so the bound has to follow it as the view
    /// pans — but it does NOT need re-measuring against every vertex: the
    /// mesh's own sphere plus the distance to the target bounds it in O(1),
    /// conservatively, and being generous costs nothing a 32-bit depth buffer
    /// can notice.
    pub fn depth_radius(&self, target: [f64; 3]) -> f64 {
        let d = ((self.center[0] - target[0]).powi(2)
            + (self.center[1] - target[1]).powi(2)
            + (self.center[2] - target[2]).powi(2))
        .sqrt();
        (d + self.radius).max(1e-6)
    }
}

/// The machined stock, extracted as a surface the depth buffer can order —
/// `vox::VoxField::iso_surface` with this module's color law applied by the
/// shader rather than baked per quad (DECISIONS.md → "the field is already
/// undercut-capable…", slice 1).
///
/// **There is no upward-closure error, and its absence is the point.** The
/// heightfield this replaced declined a truncated form-cutter field because
/// "a column may hold material above a void and a HEIGHTFIELD would draw it
/// wrong" — a sentence about a representation that no longer exists. The two
/// errors here are about the FIELD's trustworthiness, and they are the
/// heightfield's own, unchanged: an incomplete sweep makes no claim, and a
/// stock thinner than a cell holds no sample to read.
pub fn stock_surface(p: &Part, cell: Option<f64>) -> Result<StockSurface, String> {
    let field = VoxField::build_with_cell(p, cell)?;
    stock_surface_of(field)
}

/// The swept stock after the first `k` ops — the field's own prefix
/// builder, the one the rest report's attribution reads, so the cursor's
/// surface at `k` is the same field the report would build for op `k`.
/// `k == 0` is the untouched stock. With `block = Some(b)` (DESIGN.md
/// §5.6, Phase 52 slice 6) the surface is the stock WHILE the cursor's
/// operation — op `k-1`, the highlighted one — stands at its move `b`:
/// the ops before it whole, that op through the move
/// (`VoxField::build_prefix_moves`).
pub fn stock_surface_prefix(p: &Part, emitted_moves: &[OpEmission], k: usize, block: Option<usize>, cell: Option<f64>) -> Result<StockSurface, String> {
    let field = match (block, k.checked_sub(1)) {
        (Some(b), Some(on)) => {
            // The walker's bound is an index into the channel it walks —
            // the resolved one under control comp, where the text's move
            // `b` becomes the resolved move its foot is on (slice 10).
            let through = match emitted_moves.iter().find(|em| em.op_idx == on) {
                Some(em) if em.resolved.is_some() => resolved_foot(emitted_moves, on, b).map(|(r, _)| r),
                _ => Some(b),
            };
            match through {
                Some(r) => VoxField::build_prefix_moves(p, emitted_moves, on, Some(r), cell)?,
                // No foot to stand on: the stock before the op, whole.
                None => VoxField::build_prefix_from_emissions(p, emitted_moves, on, cell)?,
            }
        }
        _ => VoxField::build_prefix_from_emissions(p, emitted_moves, k, cell)?,
    };
    stock_surface_of(field)
}

fn stock_surface_of(field: VoxField) -> Result<StockSurface, String> {
    if !field.complete() {
        return Err(format!(
            "Stock simulation unavailable — the sweep is incomplete: {}",
            field.incomplete_reasons.join("; ")
        ));
    }
    let iso = field.iso_surface();
    if iso.verts.is_empty() {
        return Err(String::from(
            "stock sim declined — the field holds no surface to draw: the stock may be thinner than one cell, so no cell centre lies inside it to sample",
        ));
    }
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    for v in &iso.verts {
        for a in 0..3 {
            lo[a] = lo[a].min(v[a]);
            hi[a] = hi[a].max(v[a]);
        }
    }
    let center = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
    let radius = iso
        .verts
        .iter()
        .map(|v| {
            ((v[0] - center[0]).powi(2) + (v[1] - center[1]).powi(2) + (v[2] - center[2]).powi(2))
                .sqrt()
        })
        .fold(0.0f64, f64::max);

    let verts = iso
        .verts
        .iter()
        .zip(iso.normals.iter())
        .map(|(p, n)| {
            [p[0] as f32, p[1] as f32, p[2] as f32, n[0] as f32, n[1] as f32, n[2] as f32]
        })
        .collect();
    let tris = iso.tris.iter().flat_map(|t| t.iter().copied()).collect();
    Ok(StockSurface { verts, tris, center, radius, id: field.id() })
}

// ---- the two stocks, and time -----------------------------------------------
//
// The viewport had one stock — the field's — and the engine has two truths it
// reconciles in the rest report. The DECLARED stock is drawn as boundaries: each
// removal's plan boundaries lifted to its floor and to its ceiling, the moat, the
// band, the tube and the island as what the declaration says is gone after
// the first `k` ops (`stock::remaining_before(p, k)`). The report's cells
// ride the same `k`, from the report's own scan. Everything here is a view of
// something the terminal prints: `--outline`'s `removes:` line per op and
// `--rest-report`'s counts.

/// The removal edges after the first `k` ops: every section boundary of every
/// removal, once at the floor and once at the ceiling (each point at the
/// surface's own height there, so a swept tube's boundaries follow its envelope).
/// `Err` with the model's reason when it is incomplete over those ops — a
/// declined model draws nothing rather than half a viewport.
pub fn removal_boundaries(p: &Part, k: usize) -> Result<Vec<Vec<[f64; 3]>>, String> {
    let k = k.min(p.operations.len());
    let model = crate::stock::remaining_before(p, k);
    if !model.complete {
        let missing: Vec<String> = crate::stock::incomplete_ops(p)
            .into_iter()
            .filter(|(i, _)| *i < k)
            .map(|(i, why)| format!("op [{}] — {}", i, why))
            .collect();
        return Err(format!("declared stock declined — model incomplete: {}", missing.join("; ")));
    }
    let mut out = Vec::new();
    for r in &model.removals {
        for boundary in r.section.outlines(&p.stock) {
            if boundary.len() < 2 {
                continue;
            }
            let mut floor: Vec<[f64; 3]> = boundary.iter().map(|q| [q.x, q.y, r.floor_z(q.x, q.y)]).collect();
            let mut ceil: Vec<[f64; 3]> = boundary.iter().map(|q| [q.x, q.y, r.ceiling_z(q.x, q.y)]).collect();
            floor.push(floor[0]);
            ceil.push(ceil[0]);
            out.push(floor);
            out.push(ceil);
        }
    }
    Ok(out)
}

/// The rest report's cells after `k` ops — `vox::rest_cells`, the report's
/// own scan, passed through so the shell reads one module.
pub fn rest_cells(p: &Part, emitted_moves: &[OpEmission], k: usize) -> Result<crate::vox::RestCells, String> {
    crate::vox::rest_cells(p, emitted_moves, k, None)
}

/// The cells as the shell holds them: the report's lists and its cell size.
pub struct RestCellsView {
    pub cell: f64,
    pub forgotten: Vec<[f64; 3]>,
    pub violated: Vec<[f64; 3]>,
}

impl RestCellsView {
    pub fn of(c: crate::vox::RestCells) -> RestCellsView {
        RestCellsView { cell: c.cell, forgotten: c.forgotten, violated: c.violated }
    }
}

// ---- the tool in its world --------------------------------------------------
//
// Nothing in the window was the cutter. A finding about a body names a
// tool, a height above its tip and a point in material — and since this
// lane the sentence names the TIP too (`the tip at (x, y, z)`, appended by
// every producer that knows it: the entry flag, the error, both foul
// probes, the fixture check), because a body can only honestly stand on an
// emitted or probed tip, never on the material point it met. The body
// itself is `Silhouette::radius_at` sampled at the tip, at every rung
// height the one ladder home names, and at the top — rings and four
// meridians, a wireframe solid of revolution. The fixture's clamps are
// their declared boxes and cylinders.

/// How tall the drawn body is: the highest rung the declaration names (a
/// holder's body, its nose, the flute top, a profile's top vertex), else —
/// an undeclared body, today's semi-infinite rod — four radii, which is a
/// presentation height and is labelled as one by the pane.
pub fn tool_top(sil: &parse::Silhouette) -> (f64, bool) {
    let top = sil.rung_heights().into_iter().fold(f64::NEG_INFINITY, f64::max);
    if top.is_finite() && top > 0.0 {
        (top, true)
    } else {
        (4.0 * sil.max_radius(), false)
    }
}

/// The body as edges, standing with its tip at `tip`: a ring at the tip, at
/// every rung height, at a few heights inside a ball's curve, and at the
/// top — each of radius `radius_at(h)` — and four meridians joining them.
pub fn tool_body(sil: &parse::Silhouette, tip: [f64; 3]) -> Vec<Vec<[f64; 3]>> {
    let (top, _) = tool_top(sil);
    let mut hs: Vec<f64> = vec![0.0];
    hs.extend(sil.rung_heights().into_iter().filter(|h| *h > 0.0 && *h < top));
    if let parse::Silhouette::Ball { r, .. } = sil {
        for k in 1..4 {
            let h = r * k as f64 / 4.0;
            if h < top {
                hs.push(h);
            }
        }
    }
    hs.push(top);
    hs.sort_by(|a, b| a.partial_cmp(b).expect("finite heights"));
    hs.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    // A radial step (a shoulder) is two rings at one height: sample a hair
    // below and above each rung so the step is drawn as a step.
    let mut levels: Vec<f64> = Vec::new();
    for h in hs {
        if h > 0.0 && h < top {
            levels.push(h - 1e-6);
            levels.push(h + 1e-6);
        } else {
            levels.push(h);
        }
    }
    let mut out = Vec::new();
    let rings: Vec<(f64, f64)> = levels.iter().map(|h| (*h, sil.radius_at(*h))).collect();
    for (h, r) in &rings {
        if *r > 1e-9 {
            let mut ring: Vec<[f64; 3]> = crate::ops::finish::circle_pts(tip[0], tip[1], *r)
                .into_iter()
                .map(|(x, y)| [x, y, tip[2] + h])
                .collect();
            ring.push(ring[0]);
            out.push(ring);
        }
    }
    for k in 0..4 {
        let a = std::f64::consts::FRAC_PI_2 * k as f64;
        out.push(rings.iter().map(|(h, r)| [tip[0] + r * a.pcos(), tip[1] + r * a.psin(), tip[2] + h]).collect());
    }
    out
}

/// The fixture's keep-outs as boundaries: a box's twelve edges, a cylinder's two
/// boundaries and four uprights.
pub fn fixture_boundaries(p: &Part) -> Vec<Vec<[f64; 3]>> {
    let mut out = Vec::new();
    for f in &p.fixtures {
        let (z0, z1) = f.z_span();
        match f.shape {
            parse::FixtureShape::Box { w, l, .. } => {
                let c = [(f.x, f.y), (f.x + w, f.y), (f.x + w, f.y + l), (f.x, f.y + l)];
                for z in [z0, z1] {
                    let mut loop_: Vec<[f64; 3]> = c.iter().map(|(x, y)| [*x, *y, z]).collect();
                    loop_.push(loop_[0]);
                    out.push(loop_);
                }
                for (x, y) in c {
                    out.push(vec![[x, y, z0], [x, y, z1]]);
                }
            }
            parse::FixtureShape::Cylinder { dia, .. } => {
                let r = dia / 2.0;
                for z in [z0, z1] {
                    let mut ring: Vec<[f64; 3]> =
                        crate::ops::finish::circle_pts(f.x, f.y, r).into_iter().map(|(x, y)| [x, y, z]).collect();
                    ring.push(ring[0]);
                    out.push(ring);
                }
                for k in 0..4 {
                    let a = std::f64::consts::FRAC_PI_2 * k as f64;
                    out.push(vec![[f.x + r * a.pcos(), f.y + r * a.psin(), z0], [f.x + r * a.pcos(), f.y + r * a.psin(), z1]]);
                }
            }
        }
    }
    out
}

/// An op's deepest emitted point — the lowest feed-motion vertex, the
/// first on a tie — where the tool stands when an op is selected and no
/// finding names a tip. The emitted coordinate, not the tip: the shell
/// drops by the silhouette's `drop()`.
pub fn deepest_point(path: &OpPath) -> Option<[f64; 3]> {
    let mut best: Option<[f64; 3]> = None;
    for pl in path.polylines.iter().filter(|pl| !pl.rapid) {
        for p in &pl.pts {
            if best.map_or(true, |b| p[2] < b[2]) {
                best = Some(*p);
            }
        }
    }
    best
}


// ---- the selection (DESIGN.md I6) ---------------------------------------------
//
// One value, one writer (the shell's `set_selection`), everything else
// derived: which op lights, which feature lights, where the tool stands,
// where the camera goes, what the inspector shows. Nothing here reads a
// sentence: a finding's place, tip and tool are its typed fields.

/// What is selected: nothing, an operation (by its `run` index), a feature
/// (by its index into the drawn edges), or a problem (by its row).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Selection {
    #[default]
    None,
    Operation(usize),
    Feature(usize),
    Finding(usize),
    /// A row of the review (DESIGN.md §5.2, §6): an index into the
    /// `ReviewRow`s built from the report, which the report's order fixes.
    Review(usize),
    /// A declared tool (DESIGN.md §5.2's Tools heading, §6), by its NUMBER
    /// — a declaration is named by its number in the file, so the
    /// selection survives a reload exactly when that number is still
    /// declared, whatever moved around it.
    Tool(usize),
    /// The stock (DESIGN.md §6, Phase 22 slice 1): no index — a project
    /// has one — so it survives every reload.
    Stock,
    /// The sidecar's proposed stock (`proposed_stock.part` beside the
    /// project), the Review's first row: one file, so no index.
    ProposedStock,
}

/// The newest modification time over every file a project was read from
/// (`Part::sources`) — the watch's one reading (DESIGN.md §1, the window
/// follows the file). `None` when the list is empty or any file cannot be
/// stat'ed: a vanished file is a change too, and the reload that follows
/// names it.
pub fn newest_mtime(sources: &[std::path::PathBuf]) -> Option<std::time::SystemTime> {
    if sources.is_empty() {
        return None;
    }
    let mut newest: Option<std::time::SystemTime> = None;
    for s in sources {
        let m = std::fs::metadata(s).and_then(|m| m.modified()).ok()?;
        newest = Some(newest.map_or(m, |n| n.max(m)));
    }
    newest
}

/// The cursor and the selection after a reload — "one more cursor move"
/// (DESIGN.md §1): the op cursor clamps to the new op count — and a
/// cursor that stood AFTER THE LAST operation stands after the new last
/// one, so an operation just created is run, drawn and above the line
/// (the live run's finding: the newest op looked as if it had no
/// toolpath) — the block cursor clears by the nesting invariant (the op
/// under it may not be the same op), and an index selection survives
/// exactly when the new program still has that index — the operation,
/// feature or finding at it may differ, which is the honest reading of
/// "the file changed under you".
pub fn after_reload(
    op_cursor: usize,
    n_ops_before: usize,
    sel: Selection,
    n_ops: usize,
    n_features: usize,
    n_findings: usize,
    n_review: usize,
    tool_numbers: &[usize],
) -> (usize, Option<usize>, Selection) {
    let sel = match sel {
        Selection::Operation(i) if i < n_ops => sel,
        Selection::Feature(i) if i < n_features => sel,
        Selection::Finding(i) if i < n_findings => sel,
        Selection::Review(i) if i < n_review => sel,
        // A tool is kept by NUMBER: the declaration either still exists or
        // it does not, and an index would be a claim about order the file
        // never made.
        Selection::Tool(n) if tool_numbers.contains(&n) => sel,
        // A project always has a stock, so the stock stays selected.
        Selection::Stock => sel,
        // The proposed stock is a file beside the project, not a row of
        // the report; the shell drops the selection when the file goes.
        Selection::ProposedStock => sel,
        _ => Selection::None,
    };
    let k = if op_cursor >= n_ops_before { n_ops } else { op_cursor.min(n_ops) };
    (k, None, sel)
}

/// The one write, as text (DESIGN.md §1): the `run` line at `line`
/// (0-based) gains a leading `#` — the comment the language already has,
/// the exact text the user could have typed — and every other byte stays.
/// Refuses by name when the line is not a `run` statement: the program the
/// window shows was parsed from a file that has since changed, and the
/// reload will catch up before any write does.
pub fn suppress_line(text: &str, line: usize) -> Result<String, String> {
    let mut out = String::with_capacity(text.len() + 2);
    let mut found = false;
    for (i, l) in text.split_inclusive('\n').enumerate() {
        if i == line {
            let body = l.split('#').next().unwrap_or("").trim_start();
            if !(body.starts_with("run ") || body.starts_with("run(")) {
                return Err(format!("line {} is not a run statement: {}", line + 1, l.trim_end()));
            }
            out.push('#');
            out.push(' ');
            found = true;
        }
        out.push_str(l);
    }
    if !found {
        return Err(format!("line {} is past the end of the file ({} lines)", line + 1, text.lines().count()));
    }
    Ok(out)
}

/// A `# run …` line in a project source file — an operation the file holds
/// back (DESIGN.md §5.2, the Outline's Suppressed heading). Read from the
/// text, never the program, which does not contain it.
#[derive(Clone, Debug, PartialEq)]
pub struct SuppressedOp {
    /// Index into `Part::sources`.
    pub file: usize,
    /// 0-based line.
    pub line: usize,
    /// The `run …` text without its `#`, as the outline shows it.
    pub text: String,
}

fn suppressed_run(l: &str) -> Option<&str> {
    let body = l.trim_start().strip_prefix('#')?.trim_start();
    (body.starts_with("run ") || body.starts_with("run(")).then_some(body.trim_end())
}

/// Every suppressed operation in the project's source texts, in file then
/// line order.
pub fn suppressed_ops(texts: &[&str]) -> Vec<SuppressedOp> {
    let mut out = Vec::new();
    for (file, text) in texts.iter().enumerate() {
        for (line, l) in text.lines().enumerate() {
            if let Some(run) = suppressed_run(l) {
                out.push(SuppressedOp { file, line, text: run.to_string() });
            }
        }
    }
    out
}

/// The third write as text (DESIGN.md §1): the `#` — and the one space
/// `suppress_line` put after it — leave the line, every other byte stays.
/// The exact inverse of `suppress_line` on its own output; on a hand-typed
/// `#run …` it strips the `#` alone. Refuses by name when the line is not
/// a suppressed run statement.
pub fn restore_line(text: &str, line: usize) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut found = false;
    for (i, l) in text.split_inclusive('\n').enumerate() {
        if i == line {
            if suppressed_run(l).is_none() {
                return Err(format!("line {} is not a suppressed run statement: {}", line + 1, l.trim_end()));
            }
            let lead_len = l.len() - l.trim_start().len();
            let (lead, rest) = l.split_at(lead_len);
            let rest = rest.strip_prefix('#').unwrap_or(rest);
            let rest = rest.strip_prefix(' ').unwrap_or(rest);
            out.push_str(lead);
            out.push_str(rest);
            found = true;
        } else {
            out.push_str(l);
        }
    }
    if !found {
        return Err(format!("line {} is past the end of the file ({} lines)", line + 1, text.lines().count()));
    }
    Ok(out)
}

/// The fourth write as text (DESIGN.md §1): two `run` lines exchange
/// places — every other line keeps its position and its bytes. A swap and
/// not a cut-and-paste, because the paste is not its own inverse: moving a
/// line past a comment and back hoists the comment (found on `plate.cam`,
/// whose two `run`s have two comment lines between them), while a swap
/// leaves the comments where the author put them and undoes itself. Both
/// indices must be `run` lines (the program's order is its `run` lines'
/// order, so exchanging two of them is a reorder and nothing else);
/// refused by name otherwise, and when they are the same line.
pub fn swap_run_lines(text: &str, a: usize, b: usize) -> Result<String, String> {
    if a == b {
        return Err(format!("line {} would swap with itself", a + 1));
    }
    let mut lines: Vec<String> = text.split_inclusive('\n').map(str::to_string).collect();
    for i in [a, b] {
        let Some(l) = lines.get(i) else {
            return Err(format!("line {} is past the end of the file ({} lines)", i + 1, lines.len()));
        };
        let body = l.split('#').next().unwrap_or("").trim_start();
        if !(body.starts_with("run ") || body.starts_with("run(")) {
            return Err(format!("line {} is not a run statement: {}", i + 1, l.trim_end()));
        }
    }
    // A last line without its newline would glue onto its successor once
    // it moves up; give it the newline the file's other lines have.
    for i in [a, b] {
        if !lines[i].ends_with('\n') {
            lines[i].push('\n');
        }
    }
    lines.swap(a, b);
    Ok(lines.concat())
}

// ---- the tenth write: delete (Phase 22 slice 2) ------------------------------
//
// A statement removed from its file and nothing else (DESIGN.md §1): a
// single-line statement its line, a block statement its opener through
// its closing `)` — the parser's own block rule, read here and never
// restated: an opener whose code has a `(` and no `)` reads point lines
// until the first line whose code carries a `)`. A comment above the
// statement stays (the swap lesson: a write that eats a neighbour is not
// its own inverse). Which statements may go is the PARSER's question — the
// shell re-parses the whole project in memory with the lines gone, and a
// feature something runs or a tool something runs is refused with the
// parser's sentence, never a second judge's.

/// The lines a statement spans, from its first: `(first, last)` inclusive,
/// 0-based. A single-line statement is `(first, first)`; a block
/// statement runs to the line that closes it. Refused by name when
/// `first` is past the end, is not a statement (empty or a comment), or
/// opens a block no line closes.
pub fn statement_extent(text: &str, first: usize) -> Result<(usize, usize), String> {
    let lines: Vec<&str> = text.lines().collect();
    let Some(head) = lines.get(first) else {
        return Err(format!("line {} is past the end of the file ({} lines)", first + 1, lines.len()));
    };
    let code = head.split('#').next().unwrap_or("").trim();
    if code.is_empty() {
        return Err(format!("line {} is not a statement: {}", first + 1, head.trim_end()));
    }
    // A `stock Profile(` block too (Phase 54 slice 5a/5b): the same shape.
    let opens_block = (code.starts_with("feature ") || code.starts_with("stock ")) && code.contains('(') && !code.contains(')');
    if !opens_block {
        return Ok((first, first));
    }
    for (i, l) in lines.iter().enumerate().skip(first + 1) {
        if l.split('#').next().unwrap_or("").contains(')') {
            return Ok((first, i));
        }
    }
    Err(format!("line {} opens a block no line closes: {}", first + 1, head.trim_end()))
}

/// The tenth write as text (DESIGN.md §1): lines `first..=last` leave the
/// file with their newlines, every other byte stays. A last line without
/// its newline is removed whole; the line before it then ends the file
/// with the newline it already had. Refused by name past the end and
/// when the range is inverted.
pub fn delete_lines(text: &str, first: usize, last: usize) -> Result<String, String> {
    if last < first {
        return Err(format!("lines {}..{} are inverted", first + 1, last + 1));
    }
    let n = text.lines().count();
    if last >= n {
        return Err(format!("line {} is past the end of the file ({} lines)", last + 1, n));
    }
    let mut out = String::with_capacity(text.len());
    for (i, l) in text.split_inclusive('\n').enumerate() {
        if i < first || i > last {
            out.push_str(l);
        }
    }
    Ok(out)
}

/// How many events the Output log keeps: the oldest leave first. The log
/// is written to disk only when the user saves it.
pub const OUTPUT_CAP: usize = 1000;

/// How long a refusal stands on the status line before it leaves for the
/// log: long enough to read twice, short enough that an old "Nothing to
/// undo" is not still there an hour later.
pub const REFUSAL_SECS: f64 = 8.0;

/// What kind of thing an event is: something that happened, something
/// that was refused or failed, or a line a run on Output's line printed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Refused,
    Run,
}

impl Level {
    pub fn word(self) -> &'static str {
        match self {
            Level::Info => "info",
            Level::Refused => "refused",
            Level::Run => "run",
        }
    }
}

/// One line of the Output log: when (seconds since the window opened),
/// what kind, who said it, the sentence, and for a background job that
/// has ended, how long it took.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub t: f64,
    pub level: Level,
    pub source: &'static str,
    pub text: String,
    pub took: Option<f64>,
    serial: u64,
}

/// The Output log: what happened in this window, one event a line. Each
/// source (a write, the project's reload, the theme, a run) posts its own
/// sentence once, when it first appears, so a note that stands while
/// other things come and go is one line and not one per change beside it.
/// A background job is one line too, with how long it took once it ends.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OutputLog {
    pub events: Vec<Event>,
    /// What each source last said and is still saying.
    standing: Vec<(&'static str, String)>,
    /// The job running now: its text, when it began, its event's serial.
    job: Option<(String, f64, u64)>,
    serial: u64,
}

impl OutputLog {
    fn push(&mut self, t: f64, level: Level, source: &'static str, text: String) -> u64 {
        self.serial += 1;
        self.events.push(Event { t, level, source, text, took: None, serial: self.serial });
        if self.events.len() > OUTPUT_CAP {
            let drop = self.events.len() - OUTPUT_CAP;
            self.events.drain(..drop);
        }
        self.serial
    }

    /// What `source` is saying this frame: posted when it differs from
    /// what that source last said; an empty text means the source has
    /// gone quiet, so the same sentence later is a new event.
    pub fn say(&mut self, t: f64, level: Level, source: &'static str, text: &str) {
        let text = text.trim();
        let at = self.standing.iter().position(|(s, _)| *s == source);
        if text.is_empty() {
            if let Some(i) = at {
                self.standing.remove(i);
            }
            return;
        }
        match at {
            Some(i) if self.standing[i].1 == text => return,
            Some(i) => self.standing[i].1 = text.to_string(),
            None => self.standing.push((source, text.to_string())),
        }
        self.push(t, level, source, text.to_string());
    }

    /// Something that happened once: always a line.
    pub fn event(&mut self, t: f64, level: Level, source: &'static str, text: &str) {
        if !text.trim().is_empty() {
            self.push(t, level, source, text.trim().to_string());
        }
    }

    /// The background job this frame, or none. A job that starts is a
    /// line; when it ends (or another takes its place) that line gets how
    /// long it took.
    pub fn job(&mut self, t: f64, text: Option<&str>) {
        if self.job.as_ref().map(|(s, _, _)| s.as_str()) == text {
            return;
        }
        if let Some((_, since, serial)) = self.job.take() {
            if let Some(e) = self.events.iter_mut().find(|e| e.serial == serial) {
                e.took = Some((t - since).max(0.0));
            }
        }
        if let Some(text) = text {
            let serial = self.push(t, Level::Info, "job", text.to_string());
            self.job = Some((text.to_string(), t, serial));
        }
    }

    /// A run's own line, as it came, indented under the echoed command. A
    /// run may say the same thing twice; an empty line is skipped.
    pub fn append(&mut self, t: f64, text: &str) {
        if !text.trim().is_empty() {
            self.push(t, Level::Run, "run", format!("  {}", text.trim_end()));
        }
    }

    /// The log emptied (the dock's *Clear*). What a source is still
    /// saying is forgotten too, so a standing note is posted again.
    pub fn clear(&mut self) {
        self.events.clear();
        self.standing.clear();
        self.job = None;
    }

    /// One line as the dock prints it: `m:ss  sentence`.
    pub fn render_line(t: f64, text: &str) -> String {
        let s = t.max(0.0) as u64;
        format!("{}:{:02}  {text}", s / 60, s % 60)
    }

    /// An event as the dock prints it: its line, and how long a finished
    /// job took when that was long enough to notice.
    pub fn render(e: &Event) -> String {
        let line = OutputLog::render_line(e.t, &e.text);
        match e.took {
            Some(d) if d >= 0.5 => format!("{line} ({d:.1} s)"),
            _ => line,
        }
    }

    /// The whole log as a text file: a header naming the project and when
    /// the window opened, then `m:ss  level  source  sentence` a line.
    pub fn export(&self, project: &str, opened_unix: u64) -> String {
        let mut out = format!("# nacs Output — {project} — window opened {} UTC\n", utc_stamp(opened_unix));
        for e in &self.events {
            let s = e.t.max(0.0) as u64;
            let took = match e.took {
                Some(d) if d >= 0.5 => format!(" ({d:.1} s)"),
                _ => String::new(),
            };
            out.push_str(&format!("{}:{:02}  {:<7}  {:<7}  {}{took}\n", s / 60, s % 60, e.level.word(), e.source, e.text));
        }
        out
    }
}

/// Seconds since 1970 as `YYYY-MM-DD HH:MM:SS`, UTC. The civil-date
/// arithmetic is Howard Hinnant's `civil_from_days`; std has no calendar
/// and a date crate for one header line is not worth a dependency.
pub fn utc_stamp(unix: u64) -> String {
    let (days, rem) = ((unix / 86_400) as i64, unix % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", rem / 3600, (rem % 3600) / 60, rem % 60)
}

/// The Output log as a file beside the project: `<project>/<name>.log`,
/// named as the program file is.
pub fn log_file_path(project: &std::path::Path) -> std::path::PathBuf {
    program_file_path(project).with_extension("log")
}

/// Output's line remembers what was entered (Phase 59 nudge 2, the
/// user's walk of 2026-10-23: "being able to press up arrow to recall
/// previous entries could be very handy"): the lines, oldest first, and
/// where a walk with ↑ and ↓ stands in them. ↑ walks back from the
/// newest; ↓ walks forward, and past the newest the line being typed
/// when ↑ was first pressed comes back. Every line Enter took is
/// remembered, a refused one too, so a typo comes back to fix. The
/// window's own memory, bounded as the log is and written to no file:
/// the line is not a terminal, and a history file would be a second
/// thing beside the workspace file to explain.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LineHistory {
    entries: Vec<String>,
    /// Where a walk stands, `None` when the line is the user's own.
    at: Option<usize>,
    /// What the line said when the walk began.
    draft: String,
}

/// How many lines Output's line remembers.
pub const HISTORY_CAP: usize = 200;

impl LineHistory {
    /// A line entered: remembered unless empty or the same as the last
    /// one remembered (Enter twice on one line is one line to recall);
    /// any walk ends.
    pub fn remember(&mut self, line: &str) {
        let line = line.trim();
        self.at = None;
        self.draft.clear();
        if line.is_empty() || self.entries.last().is_some_and(|l| l == line) {
            return;
        }
        self.entries.push(line.to_string());
        if self.entries.len() > HISTORY_CAP {
            let drop = self.entries.len() - HISTORY_CAP;
            self.entries.drain(..drop);
        }
    }

    /// ↑ with `current` on the line: the next older line, or `None` at
    /// the oldest (the line stands). The first step keeps `current` as
    /// the draft ↓ brings back.
    pub fn back(&mut self, current: &str) -> Option<String> {
        let next = match self.at {
            None => {
                if self.entries.is_empty() {
                    return None;
                }
                self.draft = current.to_string();
                self.entries.len() - 1
            }
            Some(0) => return None,
            Some(i) => i - 1,
        };
        self.at = Some(next);
        Some(self.entries[next].clone())
    }

    /// ↓: the next newer line; past the newest, the draft back and the
    /// walk over. `None` when no walk stands.
    pub fn forward(&mut self) -> Option<String> {
        let i = self.at?;
        if i + 1 < self.entries.len() {
            self.at = Some(i + 1);
            Some(self.entries[i + 1].clone())
        } else {
            self.at = None;
            Some(std::mem::take(&mut self.draft))
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Git's verbs the window runs (DESIGN.md §5.3, Phase 54 slice 3, fork
/// 2A): the reading ones — status, log, diff, show, branch (listing),
/// remote — and five that write: fetch, pull, push, commit, switch (a
/// branch created with `switch -c`). What they may never do, by name:
/// no `--force`/`-f`, no `--amend`, no `--hard`, no `-D`; no checkout
/// (`checkout -- <path>` discards work), reset, rebase, stash, merge,
/// cherry-pick, clean, rm — history is never rewritten and no file is
/// discarded from here. `commit` takes `-m` and the words after it ARE
/// the message (the runner has no shell and no quotes), committed with
/// pathspec `.` — the project directory's tracked changes.
pub fn git_spec(words: &[&str], project_dir: &std::path::Path) -> Result<RunSpec, String> {
    let Some((&sub, rest)) = words.split_first() else {
        return Err("git takes a verb: status, log, diff, show, branch, remote, fetch, pull, push, commit -m <message>, switch <branch>".to_string());
    };
    let read = matches!(sub, "status" | "log" | "diff" | "show" | "branch" | "remote");
    let write = matches!(sub, "fetch" | "pull" | "push" | "commit" | "switch");
    if !read && !write {
        return Err(format!("git {sub} is not a verb the window runs — status, log, diff, show, branch and remote read; fetch, pull, push, commit and switch write; history is never rewritten and no file is discarded from here"));
    }
    for a in rest {
        if matches!(*a, "--force" | "-f" | "--force-with-lease" | "--amend" | "--hard" | "-D" | "--delete") {
            return Err(format!("git {sub} {a} is refused by name — nothing here rewrites history or discards work"));
        }
    }
    let dir = project_dir.to_string_lossy().into_owned();
    let mut args = vec!["-C".to_string(), dir];
    if sub == "commit" {
        let Some(m) = rest.iter().position(|a| *a == "-m") else {
            return Err("git commit takes -m <message> — the words after -m are the message".to_string());
        };
        let message = rest[m + 1..].join(" ");
        if message.trim().is_empty() {
            return Err("git commit -m takes a message after it".to_string());
        }
        args.extend(["commit".to_string(), "-m".to_string(), message, "--".to_string(), ".".to_string()]);
    } else {
        args.push(sub.to_string());
        args.extend(rest.iter().map(|s| s.to_string()));
    }
    let echo = std::iter::once("git").chain(words.iter().copied()).collect::<Vec<_>>().join(" ");
    Ok(RunSpec { program: std::path::PathBuf::from("git"), args, cwd: project_dir.to_path_buf(), echo, via: None })
}

/// What the runner will run (DESIGN.md §5.8, Phase 54 slice 2, fork 1A):
/// a program, its arguments, the directory it runs in, and the line as
/// the log echoes it. Never a shell: the words are split on whitespace
/// and handed to the process as they are.
#[derive(Clone, Debug, PartialEq)]
pub struct RunSpec {
    pub program: std::path::PathBuf,
    pub args: Vec<String>,
    pub cwd: std::path::PathBuf,
    pub echo: String,
    /// A line the log says under the echo before the run's own (Phase 59
    /// slice 4): `sh` names the shell and the flag it handed the line to.
    pub via: Option<String>,
}

/// The shell `sh` hands a line to (Phase 59 slice 4, fork 4A): the
/// workspace's `shell <program>` when it names one, else the system's —
/// `%COMSPEC%` (else `cmd.exe`) on Windows, `/bin/sh` elsewhere — and the
/// flag that takes a line whole: `/C` when the program is cmd, `-c` for
/// every other.
pub fn shell_program(shell: Option<&str>) -> (std::path::PathBuf, &'static str) {
    let program = match shell {
        Some(s) => std::path::PathBuf::from(s),
        None if cfg!(windows) => std::path::PathBuf::from(std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())),
        None => std::path::PathBuf::from("/bin/sh"),
    };
    let stem = program.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let flag = if stem == "cmd" { "/C" } else { "-c" };
    (program, flag)
}

/// The `nacs` the window is, for the runner: this executable when it is
/// `nacs`, else (a test binary under `target/debug/deps`) the `nacs`
/// beside its parent — the harness's own case, said rather than guessed.
pub fn nacs_exe() -> Result<std::path::PathBuf, String> {
    let me = std::env::current_exe().map_err(|e| format!("no path to this executable: {e}"))?;
    let name = format!("nacs{}", std::env::consts::EXE_SUFFIX);
    if me.file_name().is_some_and(|n| n == name.as_str()) {
        return Ok(me);
    }
    let mut dir = me.parent();
    while let Some(d) = dir {
        let candidate = d.join(&name);
        if candidate.is_file() {
            return Ok(candidate);
        }
        dir = d.parent();
    }
    Err(format!("no {name} beside {}", me.display()))
}

/// The verbs the runner knows, by the line's first word (fork 1A: the
/// house's own verbs as processes, not a shell). `nacs <flags>` runs this
/// binary on this project (the project's path appended; `--reference`,
/// `--help` and `--init` take none; `--view` and `--mcp` are refused — a
/// window inside the window is not a run); `import <file.step> [up=…] [solid=N] [allowance=…] [origin=…] [given=…]`
/// runs the sidecar as the drop does; `git <read-only verb>` runs git on
/// the project's repository — status, log, diff, show, branch, remote,
/// fetch — the writing verbs are slice 3's. Anything else is refused by
/// name, and an empty line is nothing. A line whose first word starts
/// with `--` is `nacs`'s (Phase 56 slice 3, fork 5A): every flag the
/// runner knows belongs to this binary, and Phase 54's walk typed the
/// hint's flag without the hint's verb; the echo says the verb it took.
/// One dash is not read — `-o <file>` is nacs's too, but a lone dash is
/// as likely a slip as a verb. `sh <line>` (Phase 59 slice 4, forks 3A +
/// 4A) hands everything after the verb WHOLE to the shell —
/// `shell_program`'s, from the workspace's `shell` word or the system's
/// — as one argument; a verb, not a fallback, so the refusal stands.
pub fn runner_command(line: &str, project_dir: &std::path::Path, nacs: &std::path::Path, sidecar: Option<&std::path::Path>, shell: Option<&str>) -> Result<Option<RunSpec>, String> {
    let mut words: Vec<&str> = line.split_whitespace().collect();
    if words.first().is_some_and(|w| w.starts_with("--")) {
        words.insert(0, "nacs");
    }
    let Some((&verb, rest)) = words.split_first() else { return Ok(None) };
    let echo = words.join(" ");
    let spec = match verb {
        "nacs" => {
            if rest.iter().any(|a| *a == "--view" || *a == "--mcp") {
                return Err("nacs --view and nacs --mcp are not runs — this window is the one, and the agent server speaks on stdio".to_string());
            }
            let mut args: Vec<String> = rest.iter().map(|s| s.to_string()).collect();
            let standalone = rest.iter().any(|a| matches!(*a, "--reference" | "--help" | "--init"));
            if !standalone {
                args.push(project_dir.to_string_lossy().into_owned());
            }
            RunSpec { program: nacs.to_path_buf(), args, cwd: project_dir.to_path_buf(), echo, via: None }
        }
        // The stock proposal (slice 5b): the sidecar's `import_stock.py`,
        // its `proposed_stock.part` accepted by the shell when the run
        // exits 0.
        "stock" => {
            let Some(step) = rest.first() else { return Err("stock takes a STEP file: stock <file.step> [up=z-] [solid=N] [allowance=1] [given=../front.cam] — the sidecar proposes the blank".to_string()) };
            let setup = ImportSetup::from_words(rest.iter().copied());
            let Some(sidecar) = sidecar else { return Err("no sidecar in reach: run the window from the repo root, or set NACS_SIDECAR to the sidecar directory".to_string()) };
            let (python, args) = stock_command(sidecar, &absolute(std::path::Path::new(step)), project_dir, &setup)?;
            RunSpec { program: python, args, cwd: project_dir.to_path_buf(), echo, via: None }
        }
        "import" => {
            let Some(step) = rest.first() else { return Err("import takes a STEP file: import <file.step> [up=z-] [solid=N] [allowance=1] [origin=mid,mid,max] [given=../front.cam]".to_string()) };
            let setup = ImportSetup::from_words(rest.iter().copied());
            let Some(sidecar) = sidecar else { return Err("no sidecar in reach: run the window from the repo root, or set NACS_SIDECAR to the sidecar directory".to_string()) };
            let (python, args) = import_command(sidecar, &absolute(std::path::Path::new(step)), project_dir, &setup)?;
            RunSpec { program: python, args, cwd: project_dir.to_path_buf(), echo, via: None }
        }
        // Git through the one table (slice 3): the band's verbs and the
        // typed line are the same words with the same limits.
        "git" => git_spec(rest, project_dir)?,
        // A line handed WHOLE to the shell (Phase 59 slice 4): everything
        // after the verb, as typed — its spacing, quotes and pipes — is
        // one argument to `<shell> -c` (`/C` for cmd); the shell splits,
        // quotes and globs it, never the runner. The echo is the line as
        // typed too, since the words were not split.
        "sh" => {
            let after = line.trim_start().strip_prefix("sh").unwrap_or("").trim();
            if after.is_empty() {
                return Err("sh takes a line: sh <command…> — handed whole to the shell".to_string());
            }
            let (program, flag) = shell_program(shell);
            let via = format!("via {} {flag}", program.display());
            RunSpec { program, args: vec![flag.to_string(), after.to_string()], cwd: project_dir.to_path_buf(), echo: format!("sh {after}"), via: Some(via) }
        }
        other => return Err(format!("`{other}` is not a verb the runner knows — nacs <flags>, import <file.step>, stock <file.step>, git <verb>, sh <line>")),
    };
    Ok(Some(spec))
}

/// The kind a loop's side flips to (Phase 53 slice 2, fork 1A): a pocket
/// (a void inside the loop) becomes a boss (standing material inside) and
/// back, for the planar loop and the 3D wire alike. A kind without a side
/// — a Plane, a bore, a Face, a revolve — has none.
pub fn flipped_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "ProfilePocket" => Some("ProfileBoss"),
        "ProfileBoss" => Some("ProfilePocket"),
        "ContourPocket" => Some("ContourBoss"),
        "ContourBoss" => Some("ContourPocket"),
        _ => None,
    }
}

/// The side, said on the card beside the kind: where the material is.
pub fn side_word(kind: &str) -> Option<&'static str> {
    match kind {
        "ProfilePocket" | "ContourPocket" => Some("a void inside the loop — the material outside it"),
        "ProfileBoss" | "ContourBoss" => Some("standing material inside the loop — the void outside it"),
        _ => None,
    }
}

/// A loop's side flipped, as text (Phase 53 slice 2, fork 1A): the kind
/// word on the opener swapped and the point lines traversed the other way
/// — one statement, every other byte kept, comment and blank lines among
/// the points standing where they were. The parser's winding law then
/// holds by construction: the loop that was CCW for the pocket is CW for
/// the boss. Refused by name for a kind without a side, a statement with
/// no point lines of its own, and a closing `)` that shares a line with a
/// point. What a flip does NOT do is add the pocket a boss needs around it
/// or the clear a notch needs — the ops that read the feature keep
/// reading it, and `--outline` says what stands.
pub fn flip_side(text: &str, first: usize) -> Result<String, String> {
    let (first, last) = statement_extent(text, first)?;
    let lines: Vec<&str> = text.split_inclusive(NEWLINE).collect();
    let opener = lines[first];
    let code = opener.split('#').next().unwrap_or("");
    let mut words = code.split_whitespace();
    let (Some("feature"), Some(name), Some(rest)) = (words.next(), words.next(), words.next()) else {
        return Err(format!("line {} is not a feature statement: {}", first + 1, opener.trim_end()));
    };
    let kind = rest.split('(').next().unwrap_or("");
    let Some(to) = flipped_kind(kind) else {
        return Err(format!("'{name}' is a {kind}, which has no side to flip — only a ProfilePocket/ProfileBoss or a ContourPocket/ContourBoss loop has one"));
    };
    if last == first {
        return Err(format!("'{name}' declares no points on lines of its own — nothing to traverse the other way"));
    }
    if lines[last].split('#').next().unwrap_or("").trim() != ")" {
        return Err(format!("'{name}''s closing ) shares line {} with a point — put it on a line of its own to flip the side", last + 1));
    }
    let needle = format!("{kind}(");
    let new_opener = opener.replacen(&needle, &format!("{to}("), 1);
    let middle = &lines[first + 1..last];
    let carrying: Vec<usize> = middle.iter().enumerate().filter(|(_, l)| !l.split('#').next().unwrap_or("").trim().is_empty()).map(|(i, _)| i).collect();
    let mut out: Vec<&str> = middle.to_vec();
    for (slot, &src) in carrying.iter().zip(carrying.iter().rev()) {
        out[*slot] = middle[src];
    }
    let mut text_out = String::with_capacity(text.len());
    text_out.extend(lines[..first].iter().copied());
    text_out.push_str(&new_opener);
    text_out.extend(out.iter().copied());
    text_out.extend(lines[last..].iter().copied());
    Ok(text_out)
}

const NEWLINE: char = 10u8 as char;

/// The writes a PIN makes (Phase 62 slice 1): the pattern word — and,
/// because `held` refuses a stepover and derives its own while every other
/// word needs one, the stepover dropped on the way INTO `held` and written
/// back on the way OUT at the spacing the held op was cutting at, its
/// ceiling dropped with it. One edit through the one write path: the
/// pair validates only together (Phase 62 slice 3).
pub fn pin_writes(p: &Part, idx: usize, word: &str) -> Vec<(&'static str, Option<String>)> {
    let mut out = vec![("pattern", Some(word.to_string()))];
    let Some(op) = p.operations.get(idx) else { return out };
    let was_held = op.pattern == Some(crate::parse::Pattern::Held);
    if word == crate::parse::Pattern::Held.word() {
        if op.stepover.is_some() {
            out.push(("stepover", None));
        }
    } else if was_held {
        if let Some(s) = crate::parse::derive_stepover(p, op) {
            out.push(("stepover", Some(crate::gcode::f(s).trim_end_matches('0').trim_end_matches('.').to_string())));
        }
        if op.engagement.is_some() {
            out.push(("engagement", None));
        }
    }
    out
}

/// The inspector's pattern table for one op (Phase 28 slice 5; `DECISIONS.md`
/// → "Phase 28 slice 5: the window…"): `nacs --patterns`' rows for that op,
/// word for word — `compare::row_line` prints both — with what the window
/// needs beside each: whether it is what the op cuts today, and whether it
/// can be PINNED (a pattern the emitter refuses cannot). `None` for an op that
/// works no region. The pin itself is `set_key(line, "pattern", Some(word))`
/// through the window's one field write — a panel edit is a text edit the
/// author could have typed (Phase 18's law).
///
/// It runs the real emitter once per pattern, so the shell asks for it when
/// the selection or the project changes, never per frame.
#[derive(Clone, Debug, PartialEq)]
pub struct PatternTable {
    /// The lines above the header: the floor's shape, then its lands.
    pub lands: Vec<String>,
    /// The base every `%` stands on, said once (`compare::BASE_NOTE`).
    pub note: &'static str,
    /// The group line over the header: COVER, COST, WHERE, LOAD.
    pub groups: String,
    pub header: String,
    /// The columns the window lays out (Phase 39 slice 2): `compare::COLUMNS`,
    /// the terminal's own list — group, label, which edge a cell sits on.
    pub columns: &'static [crate::compare::Column],
    pub rows: Vec<PatternRow>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PatternRow {
    pub word: &'static str,
    pub current: bool,
    pub pinnable: bool,
    /// The terminal's line for this row (`compare::row_line`).
    pub text: String,
    /// The same row as cells, one per column or spanning several
    /// (`compare::cells`) — what the window's grid lays out. `text` is these
    /// joined at the terminal's widths, so the picture and the terminal say
    /// the same words by construction.
    pub cells: Vec<crate::compare::Cell>,
}

pub fn pattern_table(p: &Part, op_idx: usize) -> Option<PatternTable> {
    let report = crate::compare::op_report(p, op_idx)?;
    Some(PatternTable {
        lands: report
            .shape
            .iter()
            .map(|s| crate::compare::shape_line(s, report.tool_dia))
            .chain(report.lands.iter().enumerate().map(|(k, l)| crate::compare::land_line(k, l, report.tool_dia)))
            .collect(),
        note: crate::compare::BASE_NOTE,
        groups: crate::compare::group_line(),
        header: crate::compare::header(),
        columns: &crate::compare::COLUMNS,
        rows: report
            .rows
            .iter()
            .map(|row| PatternRow { word: row.pattern.word(), current: row.current, pinnable: row.outcome.is_ok(), text: crate::compare::row_line(row), cells: crate::compare::cells(row) })
            .collect(),
    })
}

/// One `key=value` on one statement line, rewritten — the text edit under
/// every field and every numbering act (DESIGN.md §1). The line is
/// `<head> <Kind>(k=v, k=v)…`; the key is found at bracket depth 1 so a
/// `[…]` list value cannot hide one. `Some(v)` sets or appends (after the
/// last pair, with `, `), `None` removes the pair and its separator.
/// Refuses by name when the line has no argument list, or when asked to
/// remove a key the line does not carry. A trailing `#` comment on the
/// line is kept.
pub fn set_key(line: &str, key: &str, value: Option<&str>) -> Result<String, String> {
    let (code, comment) = match line.find('#') {
        Some(i) => (&line[..i], &line[i..]),
        None => (line, ""),
    };
    let open = code.find('(').ok_or_else(|| format!("no argument list on: {}", line.trim_end()))?;
    // A block's opener (Phase 50 slice 6) closes on a later line: its
    // arguments run to the end of this one, and the rewrite closes nothing.
    let (close, closes) = match code.rfind(')') {
        Some(c) => (c, true),
        None => (code.trim_end().len(), false),
    };
    if close < open {
        return Err(format!("malformed argument list on: {}", line.trim_end()));
    }
    let args = &code[open + 1..close];
    let mut pairs: Vec<&str> = Vec::new();
    let (mut depth, mut start) = (0usize, 0usize);
    for (i, c) in args.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                pairs.push(&args[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if !args[start..].trim().is_empty() || !pairs.is_empty() {
        pairs.push(&args[start..]);
    }
    let is_key = |p: &str| p.split('=').next().map(str::trim) == Some(key);
    let mut out: Vec<String> = Vec::new();
    let mut found = false;
    for p in &pairs {
        if is_key(p) {
            found = true;
            if let Some(v) = value {
                let lead = &p[..p.len() - p.trim_start().len()];
                out.push(format!("{lead}{key}={v}"));
            }
        } else {
            out.push((*p).to_string());
        }
    }
    match (found, value) {
        (false, Some(v)) => out.push(if out.is_empty() { format!("{key}={v}") } else { format!(" {key}={v}") }),
        (false, None) => return Err(format!("no `{key}` on: {}", line.trim_end())),
        _ => {}
    }
    // An opener keeps the whitespace between its arguments and its comment.
    let tail = if closes { format!("){}", &code[close + 1..]) } else { code[close..].to_string() };
    Ok(format!("{}({}{}{}", &code[..open], out.join(","), tail, comment))
}

/// The `key=value` pairs of one statement line, in the line's order —
/// what the inspector's fields show (DESIGN.md §5.4). Empty when the line
/// has no argument list or its arguments span lines (a block statement:
/// the first line has no `)`), which is how a block declines fields.
pub fn fields_of(line: &str) -> Vec<(String, String)> {
    let code = line.split('#').next().unwrap_or("");
    let Some(open) = code.find('(') else { return Vec::new() };
    // A block's OPENER (Phase 50 slice 6): no `)` on the line — the
    // arguments run to its end and the points follow below. Its keys are
    // fields like any other line's; the points are not.
    let args = match code.rfind(')') {
        Some(close) if close >= open => &code[open + 1..close],
        Some(_) => return Vec::new(),
        None => code[open + 1..].trim_end(),
    };
    let mut out = Vec::new();
    let (mut depth, mut start) = (0usize, 0usize);
    let mut push = |s: &str| {
        if let Some((k, v)) = s.split_once('=') {
            out.push((k.trim().to_string(), v.trim().to_string()));
        }
    };
    for (i, c) in args.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                push(&args[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    push(&args[start..]);
    out
}

/// One line of a file replaced, every other byte kept — the splice under
/// the sixth write. The new line takes the old one's newline.
pub fn replace_line(text: &str, line: usize, new_line: &str) -> Result<String, String> {
    let mut out = String::with_capacity(text.len() + new_line.len());
    let mut found = false;
    for (i, l) in text.split_inclusive('\n').enumerate() {
        if i == line {
            out.push_str(new_line.trim_end_matches('\n'));
            if l.ends_with('\n') {
                out.push('\n');
            }
            found = true;
        } else {
            out.push_str(l);
        }
    }
    if !found {
        return Err(format!("line {} is past the end of the file ({} lines)", line + 1, text.lines().count()));
    }
    Ok(out)
}

fn is_run_line(l: &str) -> bool {
    let body = l.split('#').next().unwrap_or("").trim_start();
    body.starts_with("run ") || body.starts_with("run(")
}

fn tool_ref(l: &str) -> Option<usize> {
    let code = l.split('#').next().unwrap_or("");
    let i = code.find("tool")?;
    let rest = code[i + 4..].trim_start().strip_prefix('=')?.trim_start();
    let t = rest.strip_prefix('T')?;
    let digits: String = t.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn tool_decl(l: &str) -> Option<usize> {
    let code = l.split('#').next().unwrap_or("").trim_start();
    let rest = code.strip_prefix("tool ")?.trim_start();
    let t = rest.strip_prefix('T')?;
    let digits: String = t.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

/// The fifth write, first policy (DESIGN.md §1): tools renumbered
/// `T1..Tn` in the order the `run` lines first use them, unused
/// declarations after in declaration order; declarations and references
/// rewritten together, every other byte kept. The map comes back with the
/// text so a caller can check the emission through it. A reference to an
/// undeclared tool is refused by name (the parser would too — before it
/// does, so nothing is written).
pub fn renumber_tools(text: &str) -> Result<(String, Vec<(usize, usize)>), String> {
    let declared: Vec<usize> = text.lines().filter_map(tool_decl).collect();
    for (i, t) in declared.iter().enumerate() {
        if declared[..i].contains(t) {
            return Err(format!(
                "two tool lines both declare T{t} — a renumber that keeps them together keeps the shadowing, and one that splits them cannot know which line a run line's T{t} means. Give each tool its own number first"
            ));
        }
    }
    let mut order: Vec<usize> = Vec::new();
    for l in text.lines().filter(|l| is_run_line(l)) {
        if let Some(t) = tool_ref(l) {
            if !declared.contains(&t) {
                return Err(format!("run line references T{t}, which no tool line declares"));
            }
            if !order.contains(&t) {
                order.push(t);
            }
        }
    }
    for t in &declared {
        if !order.contains(t) {
            order.push(*t);
        }
    }
    let map: Vec<(usize, usize)> = order.iter().enumerate().map(|(i, &t)| (t, i + 1)).collect();
    let to = |t: usize| map.iter().find(|(from, _)| *from == t).map(|(_, n)| *n).unwrap_or(t);
    let mut out = String::with_capacity(text.len());
    for l in text.split_inclusive('\n') {
        if let Some(t) = tool_decl(l) {
            let n = to(t);
            let i = l.find(&format!("T{t}")).unwrap();
            out.push_str(&l[..i]);
            out.push_str(&format!("T{n}"));
            out.push_str(&l[i + 1 + t.to_string().len()..]);
        } else if is_run_line(l) && tool_ref(l).is_some() {
            let t = tool_ref(l).unwrap();
            out.push_str(&set_key(l, "tool", Some(&format!("T{}", to(t))))?);
        } else {
            out.push_str(l);
        }
    }
    Ok((out, map))
}

/// The fifth write, second and third policies: `n=` on every `run` line
/// by tens in program order, or every `n=` removed. Every other byte kept.
pub fn number_ops(text: &str, by: usize) -> Result<String, String> {
    let mut out = String::with_capacity(text.len() + 8 * text.lines().count());
    let mut k = 0;
    for l in text.split_inclusive('\n') {
        if is_run_line(l) {
            k += 1;
            out.push_str(&set_key(l, "n", Some(&(k * by).to_string()))?);
        } else {
            out.push_str(l);
        }
    }
    Ok(out)
}

pub fn strip_op_numbers(text: &str) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    for l in text.split_inclusive('\n') {
        if is_run_line(l) && set_key(l, "n", None).is_ok() {
            out.push_str(&set_key(l, "n", None)?);
        } else {
            out.push_str(l);
        }
    }
    Ok(out)
}

/// One write this window made: which file, the text it replaced and the
/// text it wrote, and the file's modification time right after — the
/// stamp an undo or redo checks so a file that moved under it is refused
/// by name rather than clobbered.
#[derive(Clone, Debug, PartialEq)]
pub struct Edit {
    pub path: std::path::PathBuf,
    pub before: String,
    pub after: String,
    pub stamp: Option<std::time::SystemTime>,
}

/// Undo and redo as a stack of the file texts this window wrote — never a
/// command history (DESIGN.md §1). A new write clears the redo stack, as
/// every text editor's does.
///
/// `last` is the stamp of the LAST write this window made to each file —
/// moved by every write, undo and redo — and it, not an entry's own
/// stamp, is what "the file has changed since" is judged against. An
/// entry's stamp is the mtime after ITS write; once the entry above it
/// has been undone the file's mtime is the undo's, so judging the second
/// undo by its own entry's stamp refused it every time (Phase 43 slice 1,
/// the first hand's finding).
#[derive(Default, Debug)]
pub struct EditHistory {
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    last: std::collections::HashMap<std::path::PathBuf, Option<std::time::SystemTime>>,
}

impl EditHistory {
    pub fn record(&mut self, e: Edit) {
        self.last.insert(e.path.clone(), e.stamp);
        self.undo.push(e);
        self.redo.clear();
    }
    /// The stamp this window last left on `path` — by a write, an undo or
    /// a redo — or `None` if it never wrote it.
    pub fn last_stamp(&self, path: &std::path::Path) -> Option<std::time::SystemTime> {
        self.last.get(path).copied().flatten()
    }
    /// The write an undo would reverse, if any — the caller rewrites
    /// `before`, then calls `undone` with the new stamp.
    pub fn to_undo(&self) -> Option<&Edit> {
        self.undo.last()
    }
    pub fn to_redo(&self) -> Option<&Edit> {
        self.redo.last()
    }
    pub fn undone(&mut self, stamp: Option<std::time::SystemTime>) {
        if let Some(mut e) = self.undo.pop() {
            e.stamp = stamp;
            self.last.insert(e.path.clone(), stamp);
            self.redo.push(e);
        }
    }
    pub fn redone(&mut self, stamp: Option<std::time::SystemTime>) {
        if let Some(mut e) = self.redo.pop() {
            e.stamp = stamp;
            self.last.insert(e.path.clone(), stamp);
            self.undo.push(e);
        }
    }
    pub fn depth(&self) -> (usize, usize) {
        (self.undo.len(), self.redo.len())
    }
}

/// Rewrite `path` with `text` only if its modification time is still the
/// one this window last left on it (`EditHistory::last_stamp`); the new
/// stamp comes back for `undone`/`redone`. Refused by name otherwise — a
/// stale undo that clobbers someone else's save is the one thing worse
/// than no undo. Here rather than in the shell so a test can drive two
/// writes, two undos and two redos through a real file.
pub fn rewrite_if_unmoved(path: &std::path::Path, text: &str, history: &EditHistory) -> Result<Option<std::time::SystemTime>, String> {
    let now = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    if now != history.last_stamp(path) {
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        return Err(format!("{name} changed since this window wrote it"));
    }
    crate::theme::write_atomically(path, text)?;
    Ok(std::fs::metadata(path).and_then(|m| m.modified()).ok())
}

/// The op the selection lights: the operation itself, or the op a problem
/// is about.
pub fn selected_op(sel: Selection, rows: &[FindingRow]) -> Option<usize> {
    match sel {
        Selection::Operation(i) => Some(i),
        Selection::Finding(i) => rows.get(i).and_then(|r| r.op),
        _ => None,
    }
}

/// The feature the selection lights: the feature itself, or the one a
/// problem names, found by name among the drawn edges.
pub fn selected_feature(sel: Selection, rows: &[FindingRow], edges: &[FeatureEdges]) -> Option<usize> {
    match sel {
        Selection::Feature(i) if i < edges.len() => Some(i),
        Selection::Finding(i) => rows.get(i).and_then(|r| edges.iter().position(|e| e.name == r.feature)),
        _ => None,
    }
}

/// Where the tool stands, derived: at a problem's typed tip with its typed
/// tool; a problem that names none falls through to its op; an operation
/// stands at its deepest emitted point dropped by the silhouette to the
/// tip; a feature stands no tool.
pub fn tool_pose(p: &Part, sel: Selection, rows: &[FindingRow], paths: &[OpPath]) -> Option<([f64; 3], usize)> {
    if let Selection::Finding(i) = sel {
        if let Some(r) = rows.get(i) {
            if let (Some(at), Some(t)) = (r.at, r.tool) {
                return Some((at, t));
            }
        }
    }
    let idx = selected_op(sel, rows)?;
    let op = p.operations.get(idx)?;
    let tool = p.tools.get(&op.tool_num)?;
    let path = paths.iter().find(|pa| pa.op_idx == idx)?;
    let e = deepest_point(path)?;
    let drop = Silhouette::of(tool).drop();
    Some(([e[0], e[1], e[2] - drop], op.tool_num))
}

/// Where the tool stands after move `b` of op `op` (DESIGN.md §5.6, Phase
/// 52 slice 5): the END point of that move as emitted — the modal motion
/// replayed from the program's start through `gcode::apply_move` over the
/// op's TEXT moves, which are what the block cursor and the Program's
/// lines name (`GcodeLine.block` is an index into `OpEmission.moves`), so
/// a move that carries only a Z word still has its X and Y. `None` when
/// the op or the move is not there, when the position is not yet fully
/// known. For an op under control comp — its text the CONTOUR the
/// controller offsets, its resolved channel the tool centre with no
/// move-for-move map to the text — the answer is the FOOT (`resolved_foot`,
/// Phase 52 slice 10): the resolved channel's point nearest the text move's
/// end, which is where the centre stands when the controller has offset
/// that vertex; never the text's own point, which would stand a radius off.
pub fn block_end(emitted_moves: &[OpEmission], op: usize, b: usize) -> Option<[f64; 3]> {
    if emitted_moves.iter().any(|em| em.op_idx == op && em.resolved.is_some()) {
        return resolved_foot(emitted_moves, op, b).map(|(_, foot)| foot);
    }
    text_end(emitted_moves, op, b)
}

/// The end of TEXT move `b` of op `op`: the modal motion replayed over
/// every op's text to that move.
fn text_end(emitted_moves: &[OpEmission], op: usize, b: usize) -> Option<[f64; 3]> {
    let mut m = gcode::Motion { rapid: true, x: None, y: None, z: None, f: None };
    for em in emitted_moves {
        if em.op_idx == op {
            if b >= em.moves.len() {
                return None;
            }
            for line in &em.moves[..=b] {
                gcode::apply_move(line, &mut m);
            }
            return match (m.x, m.y, m.z) {
                (Some(x), Some(y), Some(z)) => Some([x, y, z]),
                _ => None,
            };
        }
        for line in &em.moves {
            gcode::apply_move(line, &mut m);
        }
    }
    None
}

/// The per-move map under control comp (DESIGN.md §5.6, Phase 52 slice
/// 10): for an op with a resolved channel, the RESOLVED move whose path
/// passes nearest the end of TEXT move `b`, and the point on it — the foot
/// of the contour vertex on the tool-centre parallel. The reading: the
/// resolved channel is the text offset by the radius (the emitter's own
/// composition law), so when the controller has offset a vertex the centre
/// stands at the parallel's point nearest it — on a straight stretch the
/// normal's foot, one radius off; at an inside corner the parallel's
/// miter, where the compensated path turns (`r·√2` for a right angle); at
/// an outside corner the parallel's arc. Both channels replayed from the
/// program's start, arcs flattened
/// as `op_paths` flattens them, the distance in three dimensions so a
/// rapid at clearance never wins over the cut beneath it. `None` for an op
/// with no resolved channel (the text IS the centre: `block_end` answers)
/// or when either position is not yet known.
pub fn resolved_foot(emitted_moves: &[OpEmission], op: usize, b: usize) -> Option<(usize, [f64; 3])> {
    let target = text_end(emitted_moves, op, b)?;
    let mut m = gcode::Motion { rapid: true, x: None, y: None, z: None, f: None };
    let mut best: Option<(usize, [f64; 3], f64)> = None;
    for em in emitted_moves {
        if em.op_idx != op {
            for line in gcode::resolved_moves(em) {
                gcode::apply_move(line, &mut m);
            }
            continue;
        }
        em.resolved.as_ref()?;
        for (k, line) in gcode::resolved_moves(em).iter().enumerate() {
            let before = m;
            let arc = gcode::arc_words(line);
            if !gcode::apply_move(line, &mut m) {
                continue;
            }
            let (to, from) = match ((m.x, m.y, m.z), (before.x, before.y, before.z)) {
                ((Some(tx), Some(ty), Some(tz)), (Some(fx), Some(fy), Some(fz))) => ([tx, ty, tz], [fx, fy, fz]),
                _ => continue,
            };
            let pts: Vec<[f64; 3]> = match arc {
                Some((i, j, cw)) if (to[2] - from[2]).abs() <= gcode::PATH_EPS => {
                    let ag = gcode::arc_geom(from[0], from[1], to[0], to[1], i, j, cw);
                    if ag.ra <= gcode::PATH_EPS { vec![from, to] } else { gcode::arc_chords(&ag, from, to) }
                }
                _ => vec![from, to],
            };
            for w in pts.windows(2) {
                let (foot, d) = nearest_on_segment(w[0], w[1], target);
                if best.map_or(true, |(_, _, bd)| d < bd) {
                    best = Some((k, foot, d));
                }
            }
        }
        return best.map(|(k, foot, _)| (k, foot));
    }
    None
}

/// The point of segment `a`–`b` nearest `p`, and its distance.
fn nearest_on_segment(a: [f64; 3], b: [f64; 3], p: [f64; 3]) -> ([f64; 3], f64) {
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ap = [p[0] - a[0], p[1] - a[1], p[2] - a[2]];
    let len2 = ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2];
    let s = if len2 <= 0.0 { 0.0 } else { ((ap[0] * ab[0] + ap[1] * ab[1] + ap[2] * ab[2]) / len2).clamp(0.0, 1.0) };
    let foot = [a[0] + ab[0] * s, a[1] + ab[1] * s, a[2] + ab[2] * s];
    let d = ((p[0] - foot[0]).powi(2) + (p[1] - foot[1]).powi(2) + (p[2] - foot[2]).powi(2)).sqrt();
    (foot, d)
}

/// The tool's pose at the block cursor (DESIGN.md §5.6, Phase 52 slice 5):
/// with a block standing on the operation the cursor is on AND that
/// operation selected, the tool stands at the move's end (`block_end`),
/// dropped by the silhouette to the tip as `tool_pose` drops the deepest
/// point. `None` otherwise — the shell then asks `tool_pose`.
pub fn block_pose(p: &Part, emitted_moves: &[OpEmission], k: usize, block: Option<usize>) -> Option<([f64; 3], usize)> {
    let b = block?;
    let on = k.checked_sub(1)?;
    // No selection guard (Phase 52 slice 14): the block cursor is a fact
    // of the cursor, not of the selection — a viewport click that selects
    // a feature leaves the tool where the move put it, as the stock stays.
    let op = p.operations.get(on)?;
    let tool = p.tools.get(&op.tool_num)?;
    let e = block_end(emitted_moves, on, b)?;
    let drop = Silhouette::of(tool).drop();
    Some(([e[0], e[1], e[2] - drop], op.tool_num))
}

/// Where the camera goes on a fresh selection, if anywhere: a problem's
/// annotation. Selecting an op or a feature moves nothing.
pub fn selection_anchor(sel: Selection, rows: &[FindingRow], annotations: &[Annotation]) -> Option<[f64; 3]> {
    let Selection::Finding(i) = sel else { return None };
    rows.get(i)?.annotation.and_then(|mi| annotations.get(mi)).map(|a| a.anchor())
}

/// The op's outline line, by index.
fn op_line_at(p: &Part, outline: &ProjectOutline, idx: usize) -> Option<String> {
    outline.features.iter().flat_map(|f| f.ops.iter()).find(|op| op.idx == idx).map(|op| crate::outline::op_line(p, op))
}

/// The status line's selection part.
pub fn selection_sentence(p: &Part, outline: &ProjectOutline, sel: Selection, rows: &[FindingRow], edges: &[FeatureEdges], review: &[ReviewRow]) -> String {
    match sel {
        Selection::None => String::new(),
        Selection::Operation(i) => op_line_at(p, outline, i).unwrap_or_default(),
        Selection::Feature(i) => edges.get(i).map(|e| format!("{}  {}", e.name, e.kind)).unwrap_or_default(),
        Selection::Finding(i) => rows.get(i).map(|r| r.head.clone()).unwrap_or_default(),
        Selection::Review(i) => review.get(i).map(|r| r.head.clone()).unwrap_or_default(),
        Selection::ProposedStock => "Proposed stock".to_string(),
        Selection::Tool(n) => outline.tools.iter().find(|t| t.num == n).map(crate::outline::tool_row).unwrap_or_default(),
        Selection::Stock => crate::outline::stock_row(&outline.stock),
    }
}

/// One row of the review (DESIGN.md §5.2): a face of the sidecar's report,
/// as the pane lists it and the inspector describes it. Std-only — the
/// report itself is JSON and lives behind the features that own
/// `serde_json`; `review::rows` builds these from it.
#[derive(Clone, Debug, PartialEq)]
pub struct ReviewRow {
    /// Index into the report's `faces`.
    pub face: usize,
    pub id: String,
    /// The pane's section this row sits under: "Proposed", or
    /// "Residue · row 1.2 · plane_oblique".
    pub section: String,
    /// The row's one line: id, surface, and for a grouped proposal its group.
    pub head: String,
    /// The statement (a proposal) or the reason (the residue), verbatim.
    pub text: String,
    /// The proposal's statement, when there is one to accept.
    pub statement: Option<String>,
    /// The faces or features the reason names — the contract's `refs`.
    pub refs: Vec<String>,
    /// Whether the project's text rejects this proposal (ROADMAP item 1,
    /// slice 3): a `# rejected F7` line names it or a member of its group.
    /// Accept all skips it; the row and the CLI say so.
    pub rejected: bool,
    /// The statement's feature name is already declared in the project:
    /// accepted (by this window or any editor), derived from the program,
    /// never remembered.
    pub in_project: bool,
    /// A grouped proposal's member that is not its primary: the block is
    /// the group's, accepted through any member.
    pub member_of: Option<String>,
}

/// The name a `feature NAME …` statement declares — the second word. The
/// one string rule the review needs from the grammar, so a row can say
/// "in project" without a parse per row.
pub fn feature_name_of(statement: &str) -> Option<&str> {
    let mut words = statement.split_whitespace();
    (words.next()? == "feature").then(|| words.next()).flatten()
}

/// The proposal's own edges: the statement parsed through the one parser
/// into a transient program and drawn through the one edge path — what the
/// review lights in the viewport (DESIGN.md §5.2), never a STEP face.
pub fn proposal_edges(statement: &str) -> Result<Vec<FeatureEdges>, String> {
    let mut p = Part::default();
    crate::parse::parse_into(statement, &mut p, None)?;
    if p.features.is_empty() {
        return Err("the statement declares no feature".to_string());
    }
    Ok(feature_edges(&p))
}

/// The second write as text (DESIGN.md §1): the statement appended to the
/// project's main text, after one blank line, newline-terminated — the
/// shape `proposed.part` uses between statements, so the file reads as if
/// the person had pasted from it.
pub fn append_statement(text: &str, statement: &str) -> String {
    let mut out = String::with_capacity(text.len() + statement.len() + 3);
    out.push_str(text);
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str(statement.trim_end());
    out.push('\n');
    out
}

/// The proposal behind *Clear around it…* (Phase 61 slice 3): a boss that
/// stands in the blank with no host — nothing declared around it to be cleared
/// — gets the floor whose wall is the stock, a `Plane` at its base, named
/// a floor (`Floor1`, the lowest number free): it IS one by construction,
/// whatever the project's other Planes are called (Phase 61 nudge 1). `(name, z)`; refused by name for
/// a feature that is not a boss and for one that already stands in a host
/// (Clear that, the boss is its island), and for one that stands from the
/// stock top down to nothing above it. The form shows it; the parser judges
/// it on Create; nothing here writes.
pub fn clear_around_plane(p: &Part, feature: &str) -> Result<(String, f64), String> {
    let f = p.features.get(feature).ok_or_else(|| format!("'{feature}' is not declared"))?;
    let Some(z) = crate::stock::standing_base_z(p, f) else {
        return Err(format!("'{feature}' is not a boss standing in the blank — a Clear around it needs a CylinderBoss, an EllipticBoss or a ProfileBoss"));
    };
    if let Some(host) = crate::stock::host_of(p, feature) {
        return Err(format!("'{feature}' already stands in '{host}' — Clear that, and the boss is its island"));
    }
    if z >= p.stock.z - 1e-9 {
        return Err(format!("'{feature}' stands from the stock top — there is nothing above it to clear"));
    }
    Ok((crate::template::suggested_name_from(p, "Floor"), z))
}

/// A composed `run` line landed at position `at` in the run order (Phase 61
/// slice 3; the user, 2026-10-25: "new operations are to land after the
/// cursor"): straight AFTER the run line of the operation before it, so a
/// comment that introduces the operation after stays above that one, and
/// BEFORE the first run line when `at` is 0. `op_lines` is the program's
/// `(file, line)` per operation, 0-based as `swap_run_lines` reads them. At
/// or past the last operation — the cursor's default, "every op has run" —
/// it is `append_statement`, as it has always been; so is a program whose
/// run lines live in two files, where the destination is the main text's
/// end and nothing is guessed.
pub fn insert_run_line_at(text: &str, op_lines: &[(usize, usize)], at: usize, statement: &str) -> String {
    let n = op_lines.len();
    let one_file = op_lines.iter().all(|(f, _)| *f == 0);
    if at >= n || !one_file {
        return append_statement(text, statement);
    }
    // The line the statement goes BEFORE: the next run line's start, or —
    // for a position past the first — the line after the previous run line.
    let before_line = if at == 0 { op_lines[0].1 } else { op_lines[at - 1].1 + 1 };
    let mut lines: Vec<String> = text.split_inclusive('\n').map(str::to_string).collect();
    if before_line > lines.len() {
        return append_statement(text, statement);
    }
    // A previous run line with no newline of its own (the last line of the
    // file) cannot be followed by an insertion here: it is the append case.
    if before_line > 0 && !lines[before_line - 1].ends_with('\n') {
        return append_statement(text, statement);
    }
    lines.insert(before_line, format!("{}\n", statement.trim_end()));
    lines.concat()
}

/// What a reference key may point at (DESIGN.md §5.4, Phase 20 slice 4):
/// `tool` — every declared number as `T<n>`, in number order; `surface`
/// and `profile` — every declared feature name, sorted. `None` for a key
/// that is not a reference (a number, a kind, a list). The menu is the
/// inventory `--outline` prints and NEVER a filter by kind: which
/// features a `profile=` accepts is `validate()`'s to say, with its own
/// sentence, and a menu that pre-judged it would be the parser restated.
pub fn reference_options(p: &Part, key: &str) -> Option<Vec<String>> {
    match key {
        "tool" => {
            let mut nums: Vec<usize> = p.tools.keys().copied().collect();
            nums.sort_unstable();
            Some(nums.into_iter().map(|n| format!("T{n}")).collect())
        }
        "surface" | "profile" => {
            let mut names: Vec<String> = p.features.keys().cloned().collect();
            names.sort();
            Some(names)
        }
        _ => None,
    }
}

/// Whether the second part file declares features. A project that keeps
/// features in `geometry.part` gets its new ones there too.
pub fn second_file_declares_features(p: &crate::parse::Part) -> bool {
    p.feature_lines.iter().any(|(_, file, _, _)| *file == 1)
}

// ---- the form: the spec is the inventory (Phase 21 slice 1) ------------------
//
// Edit and creation are ONE form (DESIGN.md §5.4). Phase 18's fields were
// the keys the line HAS — the right cut then, and Phase 20's live run
// found its limit: a tool declared without a holder could never grow one,
// because a key the line lacks was "a template question, not a field".
// The template's questions exist now, so the form offers every key the
// statement's kind TAKES, the absent ones blank, and its inventory is the
// one the wizard and the parser already share — `template::TOOL_SPECS` by
// the line's kind word, `FEATURE_SPECS` by its variant, `parse::RUN_KEYS`
// for a run line — read, never restated (the mirror is pinned in
// `tests/wizard_tests.rs`). Required is what the spec requires, or the
// references a run line carries. The line's bytes stay the truth: a present
// key shows the file's value, `set_key` is the one edit under every field
// (a blank given a value appends, a value cleared removes), and a block
// statement still declines.

/// One statement of the project, by the index its home keeps: an
/// operation by run order, a feature by name, a tool by number, the
/// stock by itself (a project has one).
#[derive(Clone, Debug, PartialEq)]
pub enum Statement {
    Op(usize),
    Feature(String),
    Tool(usize),
    Stock,
    /// The header's six words as one statement (Phase 50 slice 3):
    /// `units`, `feed`, `plunge`, `spindle`, `rapid_z`, `clearance_z`.
    Header,
}

/// One field of the form: its key, the file's value (`None` for a key
/// the line does not carry — a blank), whether the statement is legal
/// without it, and the menu for a reference key (`reference_options`).
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub key: String,
    pub value: Option<String>,
    pub required: bool,
    pub options: Option<Vec<String>>,
}

/// The form: the head word a refusal names (`Floor`, `Endmill`,
/// `CylinderBore`) and the fields in the inventory's own order, the
/// spec's — the order the creation form asks in, so edit and creation
/// read the same way. A key the line carries that no inventory names
/// (a tool of a kind word the parser never judged) follows, optional.
#[derive(Clone, Debug, PartialEq)]
pub struct StatementForm {
    pub head: String,
    pub fields: Vec<Field>,
}

/// The op word of a run line, off the text — the form's head.
fn run_word(line: &str) -> String {
    let code = line.split('#').next().unwrap_or("");
    code.split('(').next().unwrap_or("").split_whitespace().nth(1).unwrap_or("").to_string()
}

/// The keys a run line is created ON — its identity (`fill_skeleton`
/// refuses to answer them): the tool and the feature reference.
const RUN_REFERENCES: [&str; 4] = ["tool", "surface", "profile", "points"];

/// The form for one statement (DESIGN.md §5.4): the inventory with the
/// line's values, required as the spec says (a tool, a feature) or as
/// the line's own references say (a run). Declines by name for a
/// statement with no line, a block statement (its opener and points are
/// edited in the file, as today), and an undeclared tool.
pub fn statement_form(p: &Part, target: &Statement) -> Result<StatementForm, String> {
    let (line, head, inventory): (String, String, Vec<(String, bool)>) = match target {
        Statement::Op(i) => {
            let line = p.op_texts.get(*i).cloned().ok_or_else(|| format!("no operation [{}]", i))?;
            // Required is the references the line CARRIES — the line's
            // identity — and nothing else: the DRAFT said the skeleton's
            // keys, and the corpus said no (`sphere_scallop.cam`'s Finish
            // carries `scallop=` where the skeleton spells `stepover=`,
            // and the parser takes either). Which numbers a verb needs is
            // the parser's either-or matrix, and a form that pre-judged it
            // would be the parser restated in a widget.
            // The reference keys offered are the ones the line CARRIES: a
            // Floor has no `profile=` row, a Drill no `surface=`. A second
            // reference is a key the verb never reads and the parser accepts
            // silently — the live run filled the blank menus on a Drill and
            // nothing read them. Every other whitelist key is offered.
            let present = fields_of(&line);
            let carries = |k: &str| present.iter().any(|(pk, _)| pk == k);
            let inventory = crate::parse::RUN_KEYS
                .iter()
                .filter(|k| !RUN_REFERENCES.contains(k) || carries(k))
                .map(|k| (k.to_string(), RUN_REFERENCES.contains(k)))
                .collect();
            let word = run_word(&line);
            (line, word, inventory)
        }
        Statement::Feature(name) => {
            let line = p.feature_lines.iter().find(|(n, ..)| n == name).map(|(_, _, _, t)| t.clone()).ok_or_else(|| format!("'{name}' is not declared"))?;
            let feature = p.features.get(name).ok_or_else(|| format!("'{name}' is not declared"))?;
            let spec = crate::template::spec_for(feature);
            // A block kind's OPENER has fields (Phase 50 slice 6): the spec's
            // keys; the points below it are not a field — a block editor is
            // still its own thing.
            (line, spec.variant.to_string(), spec.keys.iter().map(|k| (k.name.to_string(), k.required)).collect())
        }
        Statement::Tool(n) => {
            let line = p.tool_lines.iter().find(|(t, ..)| t == n).map(|(_, _, _, t)| t.clone()).ok_or_else(|| format!("T{n} is not declared"))?;
            let kind = line.split('#').next().unwrap_or("").split('(').next().unwrap_or("").split_whitespace().nth(2).unwrap_or("").to_string();
            // The parser judges the kind word (Phase 36 slice 0), so a
            // declared tool always has a spec; the default is defensive.
            let inventory = crate::template::tool_spec(&kind)
                .map(|spec| crate::template::tool_keys(spec).iter().map(|k| (k.name.to_string(), k.required)).collect())
                .unwrap_or_default();
            (line, kind, inventory)
        }
        // The stock (Phase 22 slice 1): one line, its whitelist the
        // parser's own (`STOCK_KEYS`), the three sides required — the
        // parser refuses a Box without them — the origin, the top datum
        // and the material optional. A project with no stock line has
        // the default stock and nothing to edit.
        Statement::Header => {
            // The header (Phase 50 slice 3): six lines, one value each, in
            // the order a file reads — a line the file lacks is a blank the
            // write inserts before the stock line. Every one required: the
            // header law needs every one.
            let on_line = |k: &str| p.header_lines.iter().find(|(n, f, ..)| n == k && *f == 0).map(|(_, _, _, t)| header_value(t));
            let mut fields: Vec<Field> = crate::parse::HEADER_KEYS
                .iter()
                .map(|k| Field { key: k.to_string(), value: on_line(k), required: true, options: None })
                .collect();
            // The register the program selects, first: it is the one a
            // setup changes. Its line is optional, and the field shows G54
            // when the file does not write one.
            fields.insert(0, Field { key: "work_offset".to_string(), value: on_line("work_offset").or_else(|| Some("G54".to_string())), required: false, options: None });
            return Ok(StatementForm { head: "header".to_string(), fields });
        }
        Statement::Stock => {
            let line = p.stock_line.as_ref().map(|(_, _, t)| t.clone()).ok_or_else(|| "the stock has no line on disk — this project declares no stock".to_string())?;
            // The kind and its keys are the shape's (Phase 46 slice 1): the
            // Cylinder's list is the parser's own too.
            let (kind, keys, required): (&str, &[&str], &[&str]) = match p.stock.shape {
                crate::parse::StockShape::Box => ("Box", &crate::parse::STOCK_KEYS, &["w", "l", "h"]),
                crate::parse::StockShape::Cylinder { .. } => ("Cylinder", &crate::parse::STOCK_CYLINDER_KEYS, &["dia", "h"]),
                crate::parse::StockShape::Profile { .. } => ("Profile", &crate::parse::STOCK_PROFILE_KEYS, &["h"]),
            };
            let inventory = keys.iter().map(|k| (k.to_string(), required.contains(k))).collect();
            (line, kind.to_string(), inventory)
        }
    };
    let present = fields_of(&line);
    let mut fields: Vec<Field> = inventory
        .iter()
        .map(|(key, required)| Field {
            key: key.clone(),
            value: present.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()),
            required: *required,
            // A reference is a menu of the inventory; an enum key a menu over
            // the parser's list (DESIGN.md §5.4, 2026-10-15).
            options: reference_options(p, key).or_else(|| enum_options(key)),
        })
        .collect();
    for (key, value) in present {
        if !inventory.iter().any(|(k, _)| *k == key) {
            fields.push(Field { options: reference_options(p, &key).or_else(|| enum_options(&key)), key, value: Some(value), required: false });
        }
    }
    Ok(StatementForm { head, fields })
}

/// What a field's draft writes (DESIGN.md §5.4): a value sets or appends
/// the key (`set_key(Some)`); an emptied optional removes it
/// (`set_key(None)`); an emptied required key is refused by name — "an
/// End needs surface", "an Endmill needs dia" — before the parser is
/// asked. Pure, so the pins reach it.
pub fn field_write(form: &StatementForm, key: &str, draft: &str) -> Result<Option<String>, String> {
    let draft = draft.trim();
    let field = form.fields.iter().find(|f| f.key == key).ok_or_else(|| format!("{} has no `{key}`", crate::parse::with_article(&form.head)))?;
    if !draft.is_empty() {
        return Ok(Some(draft.to_string()));
    }
    if field.required {
        return Err(format!("{} needs {key}", crate::parse::with_article(&form.head)));
    }
    if field.value.is_none() {
        return Err(format!("`{key}` is already absent"));
    }
    Ok(None)
}

/// The tool as the form's card describes it: which, where its tip is, how
/// much body is declared.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCard {
    pub number: usize,
    pub label: String,
    pub tip: [f64; 3],
    pub body_top: f64,
    pub declared: bool,
    /// The `tool` line's form (DESIGN.md §5.4, Phase 21 slice 1); a
    /// refusal by name for a tool the program does not declare.
    pub form: Result<StatementForm, String>,
}

fn tool_card(p: &Part, pose: Option<([f64; 3], usize)>) -> Option<ToolCard> {
    let (tip, t) = pose?;
    let tool = p.tools.get(&t);
    let label = tool.map(|tool| crate::outline::tool_label(t, Some(tool))).unwrap_or_else(|| format!("T{t}"));
    let (body_top, declared) = tool.map(|tool| tool_top(&Silhouette::of(tool))).unwrap_or((0.0, false));
    let form = statement_form(p, &Statement::Tool(t));
    Some(ToolCard { number: t, label, tip, body_top, declared, form })
}

/// What the form shows — its CARD, a pure function of the selection
/// (DESIGN.md §5.4): nothing, an operation with its tool, a feature with
/// its template, a problem with its op, feature and tool position, a
/// review row, a declared tool — each carrying its statement's form.
#[derive(Clone, Debug, PartialEq)]
pub enum Card {
    Nothing,
    /// `form` is the statement's form (Phase 21 slice 1) — every key the
    /// kind takes, the absent ones blank — or a refusal by name.
    Operation { line: String, removal: String, tool: Option<ToolCard>, form: Result<StatementForm, String> },
    Feature { name: String, kind: String, template: Result<Vec<String>, String>, form: Result<StatementForm, String> },
    Finding { head: String, text: String, color: [u8; 3], op_line: Option<String>, feature: String, tool: Option<ToolCard> },
    /// A review row (DESIGN.md §5.4): `statement` is `Some` for a proposal
    /// — the second write's text — and `in_project` says whether it is
    /// already declared, so the button is an offer or a fact.
    Review { head: String, section: String, text: String, statement: Option<String>, refs: Vec<String>, in_project: bool, member_of: Option<String> },
    /// A declared tool (DESIGN.md §5.4, Phase 20 slice 1): its Outline row,
    /// its body as declared, and its `tool` line's form. No tip — a
    /// declaration stands nowhere until an operation runs it.
    Tool { number: usize, row: String, body_top: f64, declared: bool, form: Result<StatementForm, String> },
    /// The stock (DESIGN.md §5.4, Phase 22 slice 1): its Outline row —
    /// the words `--outline` prints — and its `stock` line's form.
    Stock { row: String, form: Result<StatementForm, String>, header: Result<StatementForm, String> },
}

pub fn card(p: &Part, outline: &ProjectOutline, sel: Selection, rows: &[FindingRow], edges: &[FeatureEdges], paths: &[OpPath], review: &[ReviewRow], stock: Option<&StockProposal>) -> Card {
    match sel {
        Selection::None => Card::Nothing,
        // The proposed stock wears the review card: the text it will write
        // in monospace, the sidecar's comments small, one Accept.
        Selection::ProposedStock => match stock {
            Some(s) => Card::Review {
                head: s.head.clone(),
                section: "Proposed stock".to_string(),
                text: s.text.clone(),
                statement: Some(s.statement.clone()),
                refs: Vec::new(),
                in_project: s.in_project,
                member_of: None,
            },
            None => Card::Nothing,
        },
        Selection::Review(i) => match review.get(i) {
            Some(r) => Card::Review {
                head: r.head.clone(),
                section: r.section.clone(),
                text: r.text.clone(),
                statement: r.statement.clone(),
                refs: r.refs.clone(),
                in_project: r.in_project,
                member_of: r.member_of.clone(),
            },
            None => Card::Nothing,
        },
        Selection::Operation(i) => match op_line_at(p, outline, i) {
            Some(line) => Card::Operation {
                line,
                // From the outline, taken once when it was built — never per
                // call: the stock model's answer can cost a coverage grid
                // (Phase 32), and this card is built every frame the
                // inspector is open.
                removal: outline.removals.get(i).cloned().unwrap_or_default(),
                tool: tool_card(p, tool_pose(p, sel, rows, paths)),
                form: statement_form(p, &Statement::Op(i)),
            },
            None => Card::Nothing,
        },
        Selection::Feature(i) => match edges.get(i) {
            Some(e) => Card::Feature {
                name: e.name.clone(),
                kind: e.kind.to_string(),
                template: crate::template::op_lines(p, &e.name),
                form: statement_form(p, &Statement::Feature(e.name.clone())),
            },
            None => Card::Nothing,
        },
        Selection::Finding(i) => match rows.get(i) {
            Some(r) => Card::Finding {
                head: r.head.clone(),
                text: r.text.clone(),
                color: r.color,
                op_line: r.op.and_then(|idx| op_line_at(p, outline, idx)),
                feature: r.feature.clone(),
                tool: tool_card(p, tool_pose(p, sel, rows, paths)),
            },
            None => Card::Nothing,
        },
        Selection::Tool(n) => match (outline.tools.iter().find(|t| t.num == n), p.tools.get(&n)) {
            (Some(node), Some(tool)) => {
                let (body_top, declared) = tool_top(&Silhouette::of(tool));
                Card::Tool {
                    number: n,
                    row: crate::outline::tool_row(node),
                    body_top,
                    declared,
                    form: statement_form(p, &Statement::Tool(n)),
                }
            }
            _ => Card::Nothing,
        },
        Selection::Stock => Card::Stock {
            row: crate::outline::stock_row(&outline.stock),
            form: statement_form(p, &Statement::Stock),
            header: statement_form(p, &Statement::Header),
        },
    }
}

// ---- the emitted program, as a document -------------------------------------
//
// A view of `gcode::program`, which is the house rule for what a view may
// be — *a view of something the
// terminal already prints*. No engine change; not one byte of the program
// moves, and the pin below is the whole guarantee: the document's text IS what
// `nacs` writes, one formatter and two voices.
//
// What the model adds over the string is ATTRIBUTION: which operation each
// line was emitted for, so the document can highlight the op the cursor is on.
// That is exactly as fine as the op cursor, which counts OPS (`k` = how
// many have run) — and no finer, deliberately. The block-level sync the
// window-layout spec asks for (cursor a move, highlight its block; click a
// block, seek the sim) needs a MOVE→line correspondence and a finer cursor
// state than this project has, and it stays its own item rather than being
// half-built here.
//
// The attribution comes off the EMITTED MOVES, and the build measured why it has to.
// The obvious cheap version — match each op's emitted moves against the
// text and call the rest boilerplate — is wrong twice over on the shipped
// corpus: a TOOL CHANGE between two ops means their moves are not one
// contiguous run (`sphere_scallop.cam`), and the formatting boundary
// respells the emitted text while the emitted moves deliberately stays in the
// internal dialect, so for a declared inch machine the strings do not
// match at all. Both are the same mistake in different clothes, and it is
// the one this module's own charter names at the top: re-deriving the
// program's structure from its text is a second engine. So the emitter's
// own anchor — `op_start`, the line where an op's contribution begins,
// which the `n=` restart ladder already labels — rides out on
// `OpEmission::lines`, and this reads it.

/// One line of the emitted program, and the operation it belongs to.
#[derive(Clone, Debug, PartialEq)]
pub struct GcodeLine {
    pub text: String,
    /// The op index into `Part::operations`, or `None` for the header the
    /// program opens with and the footer it closes on — which belong to
    /// no operation at all, and are the reason a line number is not an op
    /// number.
    pub op: Option<usize>,
    /// Which of its op's MOVES this line is (Phase 16 slice 3, DESIGN.md
    /// §5.3): move `b` of the op, off `OpEmission.moves_start` — the
    /// move→line correspondence, read here and never recovered from the
    /// text. `None` for an op's header lines (title, tool change, a bare
    /// S) and for every line that belongs to no op.
    pub block: Option<usize>,
}

/// The emitted program with every line attributed.
#[derive(Clone, Debug, PartialEq)]
pub struct GcodeView {
    pub lines: Vec<GcodeLine>,
    /// Each op's own block of lines. A program at several work offsets
    /// holds the op again for each further part; those lines are not here.
    pub own: Vec<(usize, std::ops::Range<usize>)>,
}

impl GcodeView {
    /// The program back, byte for byte — `gcode::program`'s own string.
    /// The pin over the corpus is what makes this an identity rather than
    /// a hope.
    pub fn text(&self) -> String {
        self.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n")
    }

    /// The half-open line range one op emitted, or `None` for an op that
    /// emitted nothing (which the writer never produces today) and for an
    /// index past the end. In a program at several work offsets it is the
    /// first part's block; the op's lines for the other parts follow later.
    pub fn span(&self, op: usize) -> Option<std::ops::Range<usize>> {
        self.own.iter().find(|(i, _)| *i == op).map(|(_, lines)| lines.clone()).filter(|l| !l.is_empty())
    }

    /// The op the cursor is ON, given the cursor's count — how many ops have
    /// run. The one it just finished, so `k = 0` (nothing has run) is
    /// nothing, and `k = n` is the last op rather than one past it.
    ///
    /// **The asymmetry is a choice, and it is forced.** The cursor has
    /// `n + 1` positions because it is a BOUNDARY — a cursor between ops
    /// — and the highlight has `n` ops plus "nothing", which is also
    /// `n + 1`. The map is a bijection, so it is total and every op is
    /// reachable; what it cannot do is spend an un-highlighted position
    /// at BOTH ends. This spends it at the bottom.
    ///
    /// It spends it there because of the question the pane is for. At
    /// `k = 3` the viewport shows the part after three ops and the thing
    /// you want picked out is what CUT it — op 3, the one that just ran.
    /// The fork not taken is to highlight the PENDING op instead
    /// (`k = 0` → op 1, `k = n` → nothing, "the program is finished"),
    /// which is symmetric the other way and answers "what happens next"
    /// — the wrong question for a pane whose room is diagnose.
    ///
    /// So `k = n` IS "after the last op"; it is not a position the
    /// slider lacks. What was missing was the pane saying so, which is a
    /// label and not a model (the shell's `program_document`).
    pub fn at_cursor(&self, k: usize) -> Option<usize> {
        k.checked_sub(1)
    }

    /// How many moves op `op` emitted — the block cursor's bound.
    pub fn block_count(&self, op: usize) -> usize {
        // The highest move number, not a count of lines: a program at
        // several work offsets holds each move once per part.
        self.lines.iter().filter(|l| l.op == Some(op)).filter_map(|l| l.block).max().map_or(0, |b| b + 1)
    }

    /// The program line holding move `b` of op `op`, if both exist.
    pub fn block_line(&self, op: usize, b: usize) -> Option<usize> {
        self.lines.iter().position(|l| l.op == Some(op) && l.block == Some(b))
    }

    /// Where line `i` stands (Phase 52 slice 14, fork 14A): in an op, in
    /// the header before the first op, or in the trailer after the last —
    /// the two unattributed places told apart, so the header can be the
    /// cursor's at 0 without the trailer lighting with it. The ops' spans
    /// tile the middle (pinned over the corpus), so every unattributed line
    /// is one or the other. `None` past the end.
    pub fn place(&self, i: usize) -> Option<Place> {
        let line = self.lines.get(i)?;
        if let Some(op) = line.op {
            return Some(Place::Op(op));
        }
        let first = self.lines.iter().position(|l| l.op.is_some());
        Some(match first {
            Some(f) if i < f => Place::Header,
            None => Place::Header,
            _ => Place::Trailer,
        })
    }

    /// Where a click on line `i` puts the cursor (Phase 52 slice 14): the
    /// transport's pair — a header line before operation 1, an op's line
    /// after that op with the move it is, a trailer line after the last op.
    /// Every line seeks; the header and the trailer used to seek nothing.
    pub fn seek(&self, i: usize) -> Option<(usize, Option<usize>)> {
        Some(match self.place(i)? {
            Place::Header => (0, None),
            Place::Op(op) => (op + 1, self.lines[i].block),
            Place::Trailer => (self.lines.iter().filter_map(|l| l.op).max().map_or(0, |m| m + 1), None),
        })
    }

    /// Whether line `i` wears the cursor's highlight at `k` ops run: the
    /// facing op's lines; at `k = 0` the header's — and never the
    /// trailer's, which shared the header's `None` until slice 14.
    pub fn lit(&self, i: usize, k: usize) -> bool {
        matches!((self.at_cursor(k), self.place(i)), (Some(o), Some(Place::Op(p))) if o == p) || matches!((self.at_cursor(k), self.place(i)), (None, Some(Place::Header)))
    }
}

/// Where a program line stands: the header, an operation, the trailer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Header,
    Op(usize),
    Trailer,
}

/// One step of the block walk (Phase 16 slice 3, DESIGN.md §5.6): within
/// the highlighted op the cursor moves one emitted move; past either end it
/// carries the OP cursor over the boundary and lands on the neighbouring
/// op's nearest move — so a held `}` walks the whole program move by move.
/// Input and output are the transport's own pair (`k` = ops run, the block
/// within op `k-1`); ops that emitted nothing are stepped across rather
/// than parked on. Pure so the pins reach it; the shell only applies it.
pub fn block_step(view: &GcodeView, n_ops: usize, k: usize, block: Option<usize>, forward: bool) -> (usize, Option<usize>) {
    if n_ops == 0 {
        return (0, None);
    }
    if forward {
        // Into the current op's next move, else the next op with moves.
        if let Some(op) = k.checked_sub(1) {
            let next = block.map_or(0, |b| b + 1);
            if next < view.block_count(op) {
                return (k, Some(next));
            }
        }
        let start = k.max(1).min(n_ops);
        for op in (if k == 0 { 0 } else { start })..n_ops {
            if view.block_count(op) > 0 {
                return (op + 1, Some(0));
            }
        }
        (n_ops, None)
    } else {
        if let Some(op) = k.checked_sub(1) {
            match block {
                Some(b) if b > 0 => return (k, Some(b - 1)),
                // No block yet: land on this op's own last move first.
                None if view.block_count(op) > 0 => return (k, Some(view.block_count(op) - 1)),
                _ => {}
            }
            for prev in (0..op).rev() {
                if view.block_count(prev) > 0 {
                    return (prev + 1, Some(view.block_count(prev) - 1));
                }
            }
        }
        (0, None)
    }
}

/// The program and its attribution, or the engine's own error — which
/// is also what the terminal prints, and so is also what the pane shows.
pub fn gcode_view(p: &Part) -> Result<GcodeView, String> {
    let (text, emitted_moves) = gcode::program_with_moves(p)?;
    Ok(attribute(&text, &emitted_moves))
}

fn attribute(text: &str, emitted_moves: &[OpEmission]) -> GcodeView {
    let text: Vec<&str> = text.lines().collect();
    // A range the text does not hold is dropped rather than trusted: this
    // module reads the emitter, it does not police it, and a pane that
    // shows the program without a highlight is degraded where one that
    // panics is broken.
    let mut of: Vec<Option<usize>> = vec![None; text.len()];
    let mut block: Vec<Option<usize>> = vec![None; text.len()];
    for em in emitted_moves {
        // A program at several work offsets holds the op once per register.
        for (lines, moves_start) in em.copies() {
            for slot in of.iter_mut().take(lines.end.min(text.len())).skip(lines.start) {
                *slot = Some(em.op_idx);
            }
            for k in 0..em.moves.len() {
                if let Some(slot) = block.get_mut(moves_start + k) {
                    *slot = Some(k);
                }
            }
        }
    }
    GcodeView {
        // A range the text does not hold is cut to it, as the lines are.
        own: emitted_moves.iter().map(|em| (em.op_idx, em.lines.start.min(text.len())..em.lines.end.min(text.len()))).collect(),
        lines: text
            .into_iter()
            .zip(of.into_iter().zip(block))
            .map(|(text, (op, block))| GcodeLine { text: text.to_string(), op, block })
            .collect(),
    }
}

// ---- the map's inset -----------------------------------------------------------
//
// The plan map is an OVERLAY over the viewport (DESIGN.md I2): a view of the
// viewport, drawn in its corner, never a pane. It has one home — the inset
// rect below — and one input path: a gesture is the map's exactly when it
// began on the inset, decided once from the press origin.

/// The inset's share of the viewport's short side, its floor and ceiling,
/// and its margin from the corner.
pub const MAP_FRACTION: f32 = 0.28;
pub const MAP_MIN_SIDE: f32 = 140.0;
pub const MAP_MAX_SIDE: f32 = 260.0;
pub const MAP_PAD: f32 = 12.0;

/// Where the inset sits in a viewport `w` x `h`, as `[x, y, w, h]` with
/// the origin at the viewport's top-left: a square in the BOTTOM-RIGHT
/// corner, a share of the short side, clamped at both ends and never
/// wider than the padding leaves.
///
/// `None` when the viewport cannot hold it honestly — a window dragged
/// narrow gets no map rather than a sliver of one, and the viewport keeps
/// every gesture.
pub fn map_inset_rect(w: f32, h: f32) -> Option<[f32; 4]> {
    // BOTH sides, not their `min`: `f32::min` ignores NaN, so a NaN width
    // beside a sane height yields a finite `short` and a rect whose x is
    // NaN. The pin caught exactly that.
    if !w.is_finite() || !h.is_finite() {
        return None;
    }
    let short = w.min(h);
    let side = (short * MAP_FRACTION)
        .clamp(MAP_MIN_SIDE, MAP_MAX_SIDE)
        .min(short - 2.0 * MAP_PAD);
    if side <= 0.0 {
        return None;
    }
    Some([w - MAP_PAD - side, h - MAP_PAD - side, side, side])
}

/// Whether a point is inside `[x, y, w, h]` — the inset's hit test.
pub fn in_rect(r: [f32; 4], x: f32, y: f32) -> bool {
    x >= r[0] && x <= r[0] + r[2] && y >= r[1] && y <= r[1] + r[3]
}

/// Whether a gesture that BEGAN at `(x, y)` (viewport-relative) belongs to
/// the map. Asked once, at the frame a gesture starts, and the answer is
/// kept for the gesture's life — so a drag begun on the viewport keeps
/// orbiting when it crosses the map, and a drag begun on the map keeps
/// steering when it leaves. Kept, not re-asked, because egui's
/// `press_origin` is per-pointer and a chorded second button mid-gesture
/// reports a moved origin. No inset, no map gesture.
pub fn map_owns_gesture(inset: Option<[f32; 4]>, x: f32, y: f32) -> bool {
    inset.is_some_and(|r| in_rect(r, x, y))
}

// ---- the help ---------------------------------------------------------------

// ---- the actions ------------------------------------------------------------
//
// ONE inventory of everything the window can be told to do — the keys,
// so an act cannot exist that the palette does not list, and a key cannot
// answer that the help does not name. The shell's `act(id)` is the one
// dispatcher; this table is the one directory. Custom `key <id> <key>`
// lines in the workspace file re-bind an id (judged here by
// `resolve_keys`, since the shell must not trust raw file strings), and

/// One thing the window can do: its id (the palette's and the workspace
/// file's word), its default key (a name `egui::Key::from_name`
/// understands), and its sentence (the help's and the palette's line).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Action {
    pub id: &'static str,
    pub key: &'static str,
    pub sentence: &'static str,
}

/// Every action, in the order the help prints them.
pub const ACTIONS: &[Action] = &[
    Action { id: "help", key: "?", sentence: "this help" },
    Action { id: "palette.open", key: "p", sentence: "the quick-action palette — type to filter, Enter runs" },
    Action { id: "dismiss", key: "Esc", sentence: "close what is on top: the palette, else the help, else an unsent draft, else the creation form" },
    Action { id: "cursor.back", key: "[", sentence: "the op cursor: one operation back" },
    Action { id: "cursor.on", key: "]", sentence: "the op cursor: one operation on" },
    Action { id: "cursor.block_back", key: "{", sentence: "the block cursor: one move back (past the top, the op before)" },
    Action { id: "cursor.block_on", key: "}", sentence: "the block cursor: one move on (past the end, the op after)" },
    // Pure toggles (DESIGN.md I12): one key, one behaviour, no
    // focus-if-unfocused.
    Action { id: "sidebar.toggle", key: "1", sentence: "show or hide the sidebar" },
    Action { id: "form.toggle", key: "2", sentence: "show or hide the form" },
    Action { id: "bottom.toggle", key: "3", sentence: "show or hide Output, under the viewport — what happened in this window, one event a line, with its time" },
    // The window's own frame (Phase 57 slice 1, fork 0A): the band is the
    // title bar, and these are its caption buttons and the frame's switch.
    Action { id: "window.minimize", key: "", sentence: "minimize the window to the taskbar" },
    Action { id: "window.maximize", key: "", sentence: "maximize the window, or restore it when it is maximized — a double-click on the band does the same" },
    Action { id: "window.close", key: "", sentence: "close the window" },
    Action { id: "window.frame", key: "", sentence: "switch between the window's own frame, where the band is the title bar, and the system's; remembered in the workspace file" },
    Action { id: "output.clear", key: "", sentence: "empty the Output log" },
    Action { id: "output.copy", key: "", sentence: "copy the Output log — every line with its time, kind and source" },
    Action { id: "output.save", key: "", sentence: "write the Output log beside the project as <name>.log — plain text, a line an event" },
    // Stop (Phase 59 slice 5, fork 5A): for any run, since a `sh` line can
    // be endless where `nacs --outline` cannot.
    Action { id: "output.stop", key: "", sentence: "stop the run on Output's line — its process is killed, and the log says so" },
    // The form's door IN (DESIGN.md §5.4, Phase 51 slice 3): the right-click
    // menu's Edit… — open, never a toggle, on the standing selection.
    Action { id: "form.open", key: "", sentence: "show the form on the selection — its card and its fields (the right-click menu's Edit…)" },
    // The one arrangement's escape (the modes retired 2026-09-04 — their
    // `a`/`d`/`v` keys are free): back to the defaults in one act.
    Action { id: "layout.revert", key: "0", sentence: "return the arrangement to its defaults" },
    Action { id: "theme.reload", key: "r", sentence: "reload the active theme file now" },
    // `f` (and Home, `ALSO`) since 2026-09-17 — §9's ban lifted for a
    // named list (DESIGN.md §9, Phase 23 slice 1): the first frame fits
    // once, and a fit is an act a hand reaches ten times an hour.
    Action { id: "options", key: "", sentence: "the options page: how a drag turns the view, what it turns about, and how much of the job guide shows" },
    Action { id: "view.fit", key: "f", sentence: "fit the scene to the viewport again" },
    // `.` since 2026-10-04 (DESIGN.md §9's named list grows by one, Phase
    // 41 slice 1): the focus is an ACT — selection framing centres and
    // never zooms, by the user's ruling — and a key, because it is reached
    // as often as the fit.
    Action { id: "view.focus", key: ".", sentence: "focus the selection — an oblique look down the tool axis, fitted to the selected feature or operation; with nothing selected, the scene" },
    // The standard views (DESIGN.md §5.5, 2026-09-17): keyless — the
    // palette and the viewport's own words reach them; the target and the
    // scale stay, only the direction changes.
    Action { id: "view.plan", key: "", sentence: "look straight down — the plan view, +X right, +Y up" },
    Action { id: "view.front", key: "", sentence: "look from the front — +X right, +Z up" },
    Action { id: "view.right", key: "", sentence: "look from the right — +Y right, +Z up" },
    // The Program's two gutters (DESIGN.md §5.3, 2026-09-17): each a
    // toggle, saved in the workspace file like a layer.
    Action { id: "program.numbers", key: "", sentence: "show or hide the Program's line numbers" },
    Action { id: "program.minimap", key: "", sentence: "show or hide the Program's minimap" },
    // The way out (DESIGN.md §5.3, Phase 50 slice 2): the whole program,
    // copied or written beside the project. Keyless, as every write is.
    Action { id: "program.copy", key: "", sentence: "copy the whole program — the text nacs writes to stdout" },
    Action { id: "program.write", key: "", sentence: "write the program beside the project as <name>.nc — the text nacs -o writes — and the setup sheet beside it as <name>.setup.txt" },
    Action { id: "sheet.copy", key: "", sentence: "copy the setup sheet: the stock, the origin and its register, the tools to load, the run time — the text nacs --sheet prints" },
    Action { id: "origin.register", key: "", sentence: "choose the work offset the program selects: open the stock's card, where the header's work_offset takes G54 to G59, or several to run a part in each" },
    // The way in (DESIGN.md §1, Phase 50 slice 3): a main.part from nothing.
    Action { id: "project.new", key: "", sentence: "new project — write a main.part here, a stock and the header, every value shown first (nacs --init)" },
    // The third door (DESIGN.md §5.2, Phase 50 slice 9): the fourth write's swap, composed.
    Action { id: "op.move_top", key: "", sentence: "move the selected operation to the top of the run order — its run line swapped up past each neighbour; z undoes" },
    Action { id: "op.move_bottom", key: "", sentence: "move the selected operation to the bottom of the run order — its run line swapped down past each neighbour; z undoes" },
    // The import act (Phase 50 slice 10): every proposal at once.
    Action { id: "review.accept_all", key: "", sentence: "accept every proposal not yet in the project — appended to main.part in one write, duplicates skipped; z undoes" },
    Action { id: "schedule", key: "", sentence: "schedule operations — append a run line for every declared feature no run names yet, in shop order, the tools picked from the project's own tool lines (nacs --schedule prints the same text); z undoes" },
    // Keyless by DESIGN.md §9 (no new bindings): the palette reaches it,
    // and the watch reaches it every 500ms without being asked.
    Action { id: "project.reload", key: "", sentence: "reload the project files now" },
    Action { id: "stock.new", key: "", sentence: "new stock from a STEP file — `stock <file.step>` on the Output line: the sidecar proposes the stock's outline and height, accepted into main.part; z undoes" },
    Action { id: "git.fetch", key: "", sentence: "git fetch — the remote's news, nothing moved" },
    Action { id: "git.pull", key: "", sentence: "git pull — the branch brought up to its remote" },
    Action { id: "git.push", key: "", sentence: "git push — the branch sent to its remote; never forced" },
    Action { id: "git.commit", key: "", sentence: "git commit — the message typed on the Output line, the project directory's tracked changes committed" },
    Action { id: "git.branch", key: "", sentence: "git switch — a branch typed on the Output line, switched to; -c makes it" },
    // The one write (DESIGN.md §1) and its undo. Suppress is keyless — the
    // inspector's button reaches it, and a key that deletes an operation
    // from the program should not be one keystroke from the cursor keys.
    Action { id: "op.suppress", key: "", sentence: "suppress the selected operation — comment out its run line" },
    // The tenth write (DESIGN.md §1): keyless for the same reason, from
    // the card's button; the parser judges what may go, z undoes.
    Action { id: "statement.delete", key: "", sentence: "delete the selected operation, feature or tool — its line or its block leaves the file; z undoes" },
    Action { id: "feature.flip_side", key: "", sentence: "flip the selected loop's side — ProfilePocket ↔ ProfileBoss (ContourPocket ↔ ContourBoss): the kind word swapped and the points traversed the other way, one write; z undoes" },
    // Phase 61 slice 3: on a boss that stands in the blank with nothing
    // around it, the floor whose wall is the stock — a Plane at the boss's
    // base — opens as a reviewed proposal, and the Clear on it follows.
    Action { id: "feature.clear_around", key: "", sentence: "clear around the selected boss: propose the Plane at its base (the floor the stock bounds), then the Clear on it — reviewed in the form, nothing written until Create; z undoes" },
    Action { id: "edit.undo", key: "z", sentence: "undo this window's last write to the project" },
    Action { id: "edit.redo", key: "y", sentence: "redo the write undone last" },
    // The fifth write's three policies (DESIGN.md §1): whole-program
    // transforms, palette-only, no dialog.
    Action { id: "tools.renumber", key: "", sentence: "renumber tools T1..Tn in first-use order, declarations and run lines together" },
    Action { id: "ops.number", key: "", sentence: "number every operation's run line by tens (n=10, 20, …)" },
    Action { id: "ops.strip_numbers", key: "", sentence: "strip every n= from the run lines" },
    Action { id: "feature.new", key: "", sentence: "new feature — compose a feature statement in the form (nacs --new)" },
    Action { id: "tool.new", key: "", sentence: "new tool — compose a tool statement in the form, the number prefilled with the lowest undeclared T" },
    Action { id: "op.new", key: "n", sentence: "new operation on the selected feature — its template lines are the menu, their blanks the fields" },
    // One act per sidebar view and per center document (DESIGN.md §5.1,
    // §5.3): the rail's and the tab strips' clicks, and what the palette
    // names. Keyless — a click reaches them.
    Action { id: SidebarTab::Outline.action_id(), key: "", sentence: "show the Outline in the sidebar" },
    // The target (Phase 52 slice 4, DESIGN.md §5.2): the model being made.
    // Keyless; the Outline's Target header, the menu and the palette reach
    // it; a drop on the window is the same import without the field.
    Action { id: "import.step", key: "", sentence: "import a STEP file as the target — name the file in the Outline's Target group, or drop it on the window (nacs --import)" },
    Action { id: SidebarTab::Problems.action_id(), key: "", sentence: "show the Problems in the sidebar" },
    Action { id: SidebarTab::Review.action_id(), key: "", sentence: "show the Review in the sidebar" },
    Action { id: "review.accept", key: "", sentence: "accept the selected proposal into the project — append its statement, as edited on its card, to main.part" },
    Action { id: "review.reject", key: "", sentence: "reject the selected proposal — append a `# rejected F7` line to main.part, so the row says so and Accept all skips it; on a rejected proposal, take that line out again; z undoes" },
    Action { id: Document::Viewport.action_id(), key: "", sentence: "show the Viewport alone in the center" },
    Action { id: Document::Program.action_id(), key: "", sentence: "show the Program alone in the center" },
    // The split (DESIGN.md §5.3, amended 2026-09-04): both documents side
    // by side, the default; keyless like the document acts.
    Action { id: "center.split", key: "", sentence: "show the Viewport and the Program side by side in the center" },
    // One act per viewport layer (DESIGN.md I9): the checkbox's twin, so
    // the palette can name what the widget toggles.
    Action { id: Layer::Outlines.action_id(), key: "", sentence: "show or hide the Outlines layer" },
    Action { id: Layer::Toolpaths.action_id(), key: "", sentence: "show or hide the Toolpaths layer" },
    Action { id: Layer::DeclaredStock.action_id(), key: "", sentence: "show or hide the Removal boundaries layer" },
    Action { id: Layer::SimulatedStock.action_id(), key: "", sentence: "show or hide the Stock simulation layer" },
    Action { id: Layer::RestMaterial.action_id(), key: "", sentence: "show or hide the Uncut material layer" },
    Action { id: Layer::Fixtures.action_id(), key: "", sentence: "show or hide the Fixtures layer" },
    Action { id: Layer::Tool.action_id(), key: "", sentence: "show or hide the Tool layer" },
    Action { id: Layer::Annotations.action_id(), key: "", sentence: "show or hide the Annotations layer" },
    Action { id: Layer::Engagement.action_id(), key: "", sentence: "show or hide the Engagement layer — what the tool would meet on the selected floor, before a path exists" },
];

/// The action an id names, if the inventory knows it.
pub fn action(id: &str) -> Option<&'static Action> {
    ACTIONS.iter().find(|a| a.id == id)
}

/// The palette's filter: a case-insensitive substring over id, sentence
/// and key — an empty query lists everything, in inventory order. This is
/// the whole matching rule; fuzziness stays out by name.
pub fn palette_matches(query: &str) -> Vec<&'static Action> {
    let q = query.to_lowercase();
    ACTIONS
        .iter()
        .filter(|a| {
            q.is_empty()
                || action_label(a).to_lowercase().contains(&q)
                || a.id.to_lowercase().contains(&q)
                || a.sentence.to_lowercase().contains(&q)
                || a.key.to_lowercase() == q
        })
        .collect()
}

/// The few acts no menu names, by a short label of their own.
const UNLISTED_LABELS: &[(&str, &str)] = &[
    ("dismiss", "Close what is on top"),
    ("cursor.back", "Operation back"),
    ("cursor.on", "Operation on"),
    ("cursor.block_back", "Move back"),
    ("cursor.block_on", "Move on"),
];

/// An act's short name: the label its menu row wears, else its
/// right-click row's, else its own from the table above. The palette
/// lists acts by this; the sentence is the hover.
pub fn action_label(a: &Action) -> &'static str {
    let menu = MENU.iter().flat_map(|(_, rows)| rows.iter()).find(|(id, _)| *id == a.id).map(|(_, l)| *l);
    let kinds = [Selection::Operation(0), Selection::Feature(0), Selection::Tool(1), Selection::Stock, Selection::Finding(0), Selection::Review(0)];
    let context = || kinds.into_iter().flat_map(context_actions).find(|(id, _)| *id == a.id).map(|(_, l)| l);
    let own = || UNLISTED_LABELS.iter().find(|(id, _)| *id == a.id).map(|(_, l)| *l);
    menu.or_else(context).or_else(own).unwrap_or(a.sentence)
}

/// How wide the palette's label column is, in characters.
pub const PALETTE_LABEL_W: usize = 28;

/// A palette row: the label in its column, then the key.
pub fn palette_row(a: &Action) -> String {
    format!("{:<w$}  {}", action_label(a), key_words(a).trim(), w = PALETTE_LABEL_W).trim_end().to_string()
}

/// A second binding for an act (DESIGN.md §9, the named list of
/// 2026-09-17): a bare key name (`Home`) rides the keymap beside the
/// act's own key; a chord (`Ctrl+Z`) is matched with its modifier held —
/// the one place a chord fires an act, now that a chord is never a bare
/// key. Every id here is one `ACTIONS` knows (pinned), and a bare key
/// here collides with no default key (pinned).
pub const ALSO: &[(&str, &str)] = &[
    ("view.fit", "Home"),
    ("edit.undo", "Ctrl+Z"),
    ("edit.redo", "Ctrl+Y"),
];

/// An act's second binding, or none.
pub fn also(id: &str) -> Option<&'static str> {
    ALSO.iter().find(|(i, _)| *i == id).map(|(_, k)| *k)
}

/// A chord's parts: `Ctrl+Z` → the key name `Z` under the command
/// modifier (Ctrl, or Cmd on a Mac). `None` for a bare key.
pub fn chord_key(binding: &str) -> Option<&str> {
    binding.strip_prefix("Ctrl+")
}

/// The keys column's words for an act: its key (or `·`) and its second
/// binding when it has one — `f Home`, `z Ctrl+Z` — one rendering for
/// the help and the palette.
pub fn key_words(a: &Action) -> String {
    match also(a.id) {
        Some(more) => format!("{} {more}", key_column(a.key)),
        None => key_column(a.key).to_string(),
    }
}

/// The workspace file's `key` lines, judged: only ids the inventory knows
/// survive (an unknown id is the unknown-line rule again), the LAST line
/// for an id wins (a config file reads top to bottom), and the key string
/// passes through — the shell resolves it against the keyboard it has.
pub fn resolve_keys(overrides: &[(String, String)]) -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, String)> = Vec::new();
    for (id, key) in overrides {
        if let Some(a) = action(id) {
            out.retain(|(kept, _)| *kept != a.id);
            out.push((a.id, key.clone()));
        }
    }
    out
}

// ---- the tabs ---------------------------------------------------------------
//
// The sidebar's views as TABS on its first row (DESIGN.md §5.2, amended
// 2026-10-17 — Phase 52 slice 1). They were the RAIL, a fixed column on
// the far left, from Phase 13 until the user read the window's top edge
// at once and found a 92-pixel strip that was mostly air carrying four
// words the pane's heading repeated; the tab strip had been dropped on
// 2026-09-04 as "a redundant second control once the rail had persistent
// labels", and with the tabs in the pane the rail is the redundant one.
// What the rail was for, the tabs keep, and both are pins:
//
//   DERIVED. Every tab comes from one inventory — `SidebarTab::ALL` —
//   and every tab's act is an id `ACTIONS` knows (`ShowTab`, never a
//   toggle: I3, the control renamed). A view added to the enum appears
//   without this file being touched.
//
//   ONE THING ONCE. The word is on the tab and nowhere else: the heading
//   row that repeated it is gone, and the count the heading carried
//   (`Problems (3)`, `Review (2/7)`) is on the tab (`tab_label`).
//
//   WORDS, NOT TOKENS. Every tab carries its label rendered persistently.
//
// The menu (`MENU_GLYPH`) sits at the row's left end — still the window's
// upper-left corner — and the shut sidebar keeps that cell: the glyph and
// `SIDEBAR_DOOR`, so the sidebar is one click from shut (I14).

/// The band (DESIGN.md §4, §5.3 — Phase 52 slice 2): every column draws
/// this many rows of this height at its top and nothing else above its
/// body, so the window's top edge is one height everywhere — the
/// sidebar's tabs and its view's line, the center's three acts and each
/// document's own strip. One constant; the harness pins it by rects.
pub const BAND_ROWS: usize = 2;
pub const BAND_ROW_H: f32 = 24.0;

/// The sidebar's second row (DESIGN.md §5.2, 2026-10-17): the shown view's
/// own line — the project's name with its counts and the run order's
/// place under the Outline, the findings' count under Problems (what the
/// heading row used to say), the report's under Review. Derived, so it
/// cannot disagree with the rows beneath it.
pub fn view_line(tab: SidebarTab, project_path: &str, n_features: usize, n_ops: usize, k: usize, rows: &[FindingRow], reload_failed: bool, review: Option<(usize, usize)>) -> String {
    let name = std::path::Path::new(project_path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| project_path.to_string());
    match tab {
        // The cursor's place is the transport bar's to say, once; this
        // line used to say it again, and so did the Program's row.
        SidebarTab::Outline => {
            let _ = k;
            match n_ops {
                0 => format!("{name} — {n_features} {}, no operations", plural(n_features, "feature")),
                n => format!("{name} — {n_features} {}, {n} {}", plural(n_features, "feature"), plural(n, "operation")),
            }
        }
        SidebarTab::Problems => {
            let (p, u) = problem_counts(rows);
            match (p + usize::from(reload_failed), u) {
                (0, 0) => "no problems".to_string(),
                (p, 0) => format!("{p} {}", plural(p, "problem")),
                (0, u) => format!("{u} unchecked"),
                (p, u) => format!("{p} {} · {u} unchecked", plural(p, "problem")),
            }
        }
        SidebarTab::Review => match review {
            Some((w, n)) => format!("{n} {} proposed, {w} waiting — report.json", plural(n, "feature")),
            None => "no report — drop a STEP file on the window".to_string(),
        },
    }
}

/// The Target group's row (DESIGN.md §5.2, Phase 52 slice 4): the model
/// being made — the report's source file and its totals, the words the
/// Review's heading carried.
pub fn target_row(source: &str, faces_total: usize, features: usize) -> String {
    format!("{source} — {faces_total} {}, {features} {} proposed", plural(faces_total, "face"), plural(features, "feature"))
}

/// The Target group with no report (DESIGN.md §5.2): where one comes from.
pub const NO_TARGET: &str = "No target — New target… imports a STEP, or declare features by hand";

fn plural(n: usize, word: &str) -> String {
    if n == 1 { word.to_string() } else { format!("{word}s") }
}

/// The menu's button, one glyph: `λ`, the project's own letter (Phase 52
/// fork 10, ruled 2026-10-17 — "I like the lambda logo assuming the font
/// supports it"; the glyph lint in `tests/shell_tests.rs` is the proof
/// that it does). `☰` was the placeholder from Phase 51 slice 4.
pub const MENU_GLYPH: &str = "λ";

/// The shut sidebar's door back (I14; DESIGN.md §5.1 as retired): one chip
/// beside the menu, `(action id, label, what)` as every door table is.
pub const SIDEBAR_DOOR: (&str, &str, &str) = ("sidebar.toggle", "Sidebar", "open the sidebar");

/// One tab: the sidebar view it shows, the WORD it draws (rendered
/// persistently — a token with meaning available only on hover is
/// unlearnable), the sentence a hover adds, and the action id a click
/// runs. Everything the shell needs, so the shell decides nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct Tab {
    pub tab: SidebarTab,
    pub label: &'static str,
    pub sentence: &'static str,
    pub action: &'static str,
}

/// The legend row (DESIGN.md §5.5, 2026-09-17): each role's colour beside
/// its word, in the order the roles are derived — so a roughing pass and a
/// finishing pass are named on screen beside their colours. Read from the
/// one colour home (`role_color`) and the role's own `label`, never a
/// second table.
pub fn legend(theme: &Theme) -> Vec<([u8; 3], &'static str)> {
    [OpRole::Roughing, OpRole::Finishing, OpRole::Untagged]
        .into_iter()
        .map(|r| (theme.role_color(r), r.label()))
        .collect()
}

/// How far along a job step is. The first step not done is `Next`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StepState {
    Done,
    Next,
    Later,
}

/// One step of the job, in the order a part is programmed: what it is
/// called, where it stands, one sentence, and the act that moves it on
/// (the button's words and the act's id).
#[derive(Clone, Debug, PartialEq)]
pub struct JobStep {
    pub name: &'static str,
    pub state: StepState,
    pub says: String,
    pub act: Option<(String, &'static str)>,
}

/// The proposals in the Review that an Accept all would write.
pub fn proposals_waiting(rows: &[ReviewRow], dups: &[Option<usize>]) -> usize {
    rows.iter().enumerate().filter(|(i, r)| r.statement.is_some() && !r.in_project && !r.rejected && dups.get(*i).copied().flatten().is_none()).count()
}

/// The Review's two counts: the proposed features still waiting, and all
/// the features proposed. A face that is only a member of one is not one.
pub fn review_counts(rows: &[ReviewRow], dups: &[Option<usize>]) -> (usize, usize) {
    (proposals_waiting(rows, dups), rows.iter().filter(|r| r.statement.is_some() && r.member_of.is_none()).count())
}

/// What the window or the CLI knows beside the `Part` when it asks for the
/// job's steps.
#[derive(Clone, Debug, Default)]
pub struct JobFacts {
    /// main.part is on disk.
    pub has_file: bool,
    /// The imported model's file name.
    pub model: Option<String>,
    /// A proposed blank is not yet accepted.
    pub stock_waiting: bool,
    /// The count from `proposals_waiting`.
    pub waiting: usize,
    /// The scheduler's notes (`schedule::for_project`): what waits for a tool.
    pub notes: Vec<String>,
    /// Why no program is generated, when none is.
    pub refusal: Option<String>,
    /// The written program's file name, when it is on disk and not older
    /// than the project's text.
    pub program_file: Option<String>,
    /// How many run lines the scheduler would add now.
    pub to_schedule: usize,
    /// The origin the model was imported with, as the report says it.
    pub origin: Option<String>,
}

/// The tools the scheduler's notes ask for: first those a whole feature
/// waits for, then those only a chamfer, a seat or an edge round waits for.
/// A note reads "<what> waits for a <tool>: <why>".
pub fn tools_wanted(notes: &[String]) -> (Vec<String>, Vec<String>) {
    let (mut needed, mut extra) = (Vec::new(), Vec::new());
    for note in notes {
        let Some((what, rest)) = note.split_once(" waits for ") else { continue };
        let rest = rest.trim_start_matches("an ").trim_start_matches("a ");
        let (tool, why) = rest.split_once(": ").unwrap_or((rest, ""));
        // "a tool: no Drill(dia=3) and no Endmill under 3 is declared"
        let tool = if tool == "tool" { why.trim_end_matches(" is declared").trim_start_matches("no ").replace(" and no ", " or ") } else { tool.to_string() };
        let list = if what.contains("'s ") { &mut extra } else { &mut needed };
        if !list.contains(&tool) {
            list.push(tool);
        }
    }
    (needed, extra)
}

/// The tool form a wanted tool opens on: the kind's place in
/// `template::TOOL_SPECS`, and the one size the job names for it.
/// "Drill(dia=3) or Endmill under 3" is a Drill with dia 3; "Endmill" is an
/// Endmill with nothing filled.
pub fn wanted_tool_form(wanted: &str) -> Option<(usize, Option<(String, String)>)> {
    let kind_len = wanted.find(|c: char| !c.is_alphanumeric()).unwrap_or(wanted.len());
    let kind = crate::template::TOOL_SPECS.iter().position(|s| s.kind == &wanted[..kind_len])?;
    let size = wanted[kind_len..]
        .strip_prefix('(')
        .and_then(|rest| rest.split_once(')'))
        .and_then(|(pair, _)| pair.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()));
    Some((kind, size))
}

/// The job as seven steps, model to program, so the window can say what
/// comes next.
pub fn job_steps(part: &Part, facts: &JobFacts) -> Vec<JobStep> {
    let JobFacts { has_file, stock_waiting, waiting, .. } = *facts;
    let model = facts.model.as_deref();
    let refusal = facts.refusal.as_deref();
    let (needed, extra) = tools_wanted(&facts.notes);
    let features = part.features.len();
    let unscheduled = part.features.keys().filter(|name| !crate::schedule::consumed(part, name)).count();
    let register = registers_say(part);
    let several = gcode::registers(part).len() > 1;
    let plural =|n: usize, one: &str, many: &str| if n == 1 { format!("1 {one}") } else { format!("{n} {many}") };
    // (name, done, what a done step says, what an open one says, its act)
    let mut rows: Vec<(&'static str, bool, String, String, Option<(String, &'static str)>)> = vec![
        (
            "Model",
            model.is_some() || features > 0,
            model.map(str::to_string).unwrap_or_else(|| "none imported; the features are declared by hand".to_string()),
            "import a STEP file, or drop one on the window; the form asks which way is up and where the origin is".to_string(),
            Some(("Import STEP…".to_string(), "import.step")),
        ),
        (
            "Origin",
            model.is_some() || features > 0,
            if facts.origin.is_some() || model.is_some() || several || has_file {
                format!("{}; the program selects {register}", origin_place(facts))
            } else {
                origin_place(facts)
            },
            String::new(),
            Some(("Change the origin…".to_string(), "import.step")),
        ),
        if stock_waiting {
            ("Stock", false, String::new(), "a blank is proposed from the model; its text is on the card below".to_string(), Some(("Accept the proposed stock".to_string(), "review.accept")))
        } else {
            ("Stock", has_file, crate::outline::stock_row(&part.stock).trim_start_matches("stock").trim().to_string(), "declare the blank the part is cut from".to_string(), Some(("New project…".to_string(), "project.new")))
        },
        if waiting > 0 {
            ("Features", false, String::new(), format!("{} proposed, listed under Review: accept them all, or pick one and accept it on its card", plural(waiting, "feature is", "features are")), Some((format!("Accept all ({waiting})"), "review.accept_all")))
        } else {
            ("Features", features > 0, plural(features, "feature", "features"), "declare what is to be cut".to_string(), Some(("New feature…".to_string(), "feature.new")))
        },
        (
            "Tools",
            !part.tools.is_empty() && needed.is_empty(),
            if extra.is_empty() { plural(part.tools.len(), "tool", "tools") } else { format!("{}; left uncut for want of: {}", plural(part.tools.len(), "tool", "tools"), extra.join(", ")) },
            if needed.is_empty() { "no cutter is declared, and an operation needs one".to_string() } else { format!("the features still need: {}", needed.join("; ")) },
            Some(("New tool…".to_string(), "tool.new")),
        ),
        (
            "Operations",
            !part.operations.is_empty() && unscheduled == 0 && facts.to_schedule == 0,
            plural(part.operations.len(), "operation", "operations"),
            if unscheduled > 0 {
                format!("{} no operation yet", plural(unscheduled, "feature has", "features have"))
            } else {
                format!("{} can be added with the tools now declared", plural(facts.to_schedule, "operation", "operations"))
            },
            Some(("Schedule operations".to_string(), "schedule")),
        ),
    ];
    rows.push(match (refusal, &facts.program_file) {
        (Some(why), _) => ("Program", false, String::new(), format!("refused: {why}"), Some(("Show the problems".to_string(), "sidebar.problems"))),
        (None, Some(file)) => ("Program", true, format!("{file} is written"), String::new(), None),
        (None, None) => ("Program", false, String::new(), format!("ready to write beside the project as a .nc file; it selects {register} (File > Work offset… changes that)"), Some(("Write program".to_string(), "program.write"))),
    });
    let mut next_given = false;
    rows.into_iter()
        .map(|(name, done, done_says, open_says, act)| {
            if done {
                // A done Tools step that still leaves a chamfer or a round
                // uncut keeps its button: the tool can be added at any time.
                // So does the origin while nothing is accepted yet: importing
                // again with another origin costs nothing then.
                let act = act.filter(|_| (name == "Tools" && !extra.is_empty()) || (name == "Origin" && stock_waiting && model.is_some()));
                JobStep { name, state: StepState::Done, says: done_says, act }
            } else if !next_given {
                next_given = true;
                JobStep { name, state: StepState::Next, says: open_says, act }
            } else {
                JobStep { name, state: StepState::Later, says: String::new(), act: None }
            }
        })
        .collect()
}

/// The program's register, or all of them when it runs at several. The
/// origin is the point the machine holds there.
fn registers_say(part: &Part) -> String {
    let registers = gcode::registers(part);
    match registers.len() {
        1 => registers[0].clone(),
        n => format!("{} in turn, {n} parts a run", registers.join(", ")),
    }
}

/// Where the origin is on the part, as the import says it.
fn origin_place(facts: &JobFacts) -> String {
    match (&facts.origin, &facts.model) {
        (Some(o), _) => origin_says(o),
        (None, Some(_)) => "X0 Y0 Z0 as modeled".to_string(),
        (None, None) => "where the features are written from".to_string(),
    }
}

/// The setup sheet: what the machine needs before the program runs. The
/// blank to clamp, where to set the register, the tools to load, and how
/// long it cuts. Rows of (heading, text); `nacs --sheet` prints them and
/// the finished job guide shows them.
pub fn setup_sheet(part: &Part, facts: &JobFacts, clock: &Result<crate::cut_time::TimeReport, String>) -> Vec<(&'static str, String)> {
    let mut rows = vec![
        ("Program", facts.program_file.clone().unwrap_or_else(|| "not written yet".to_string())),
        ("Stock", crate::outline::stock_row(&part.stock).trim_start_matches("stock").trim().to_string()),
        ("Origin", format!("{}: {}", registers_say(part), origin_place(facts))),
    ];
    // Only the tools an operation runs: a declared tool nothing uses is
    // not loaded.
    let tools: Vec<String> = crate::outline::outline(part).tools.iter().filter(|t| !t.ops.is_empty()).map(|t| t.label.clone()).collect();
    rows.extend(tools.into_iter().map(|t| ("Tool", t)));
    let n = part.operations.len();
    rows.push(("Run", format!("{n} operation{}; {}", if n == 1 { "" } else { "s" }, clock_line(clock))));
    rows
}

/// The setup sheet as text, one row a line.
pub fn setup_sheet_text(rows: &[(&'static str, String)]) -> String {
    rows.iter().map(|(head, says)| format!("{head:<9}{says}\n")).collect()
}

/// The job steps as text, one a line: what `nacs --job` prints.
pub fn job_report(steps: &[JobStep]) -> String {
    let mut out = String::new();
    for (i, s) in steps.iter().enumerate() {
        let state = match s.state {
            StepState::Done => "done",
            StepState::Next => "next",
            StepState::Later => "later",
        };
        let act = s.act.as_ref().map(|(label, id)| format!("  [{label}: {id}]")).unwrap_or_default();
        out.push_str(format!("{} {:<11}{:<6}{}{}", i + 1, s.name, state, s.says, act).trim_end());
        out.push('\n');
    }
    out
}

/// The empty viewport's sentence (DESIGN.md §5.5, 2026-09-17): a project
/// with no operation draws a stock box and nothing else, and says where
/// the first operation comes from. `None` while there is something to draw.
pub fn empty_viewport_note(n_ops: usize, n_features: usize) -> Option<&'static str> {
    match (n_ops, n_features) {
        (0, 0) => Some("no features yet — the Outline's New feature… declares one"),
        (0, _) => Some("no operations yet — New operation… on a feature in the Outline schedules one"),
        _ => None,
    }
}

/// The tabs, left to right: the sidebar's views (DESIGN.md §5.2) in
/// `SidebarTab::ALL`'s order, each saying one thing once.
pub fn tabs() -> Vec<Tab> {
    SidebarTab::ALL
        .into_iter()
        .map(|t| Tab { tab: t, label: t.title(), sentence: t.sentence(), action: t.action_id() })
        .collect()
}

/// What the window can show, each beside the terminal voice it is a view
/// of, and everything it can be told to do — the `?` overlay's lines. The
/// keys section is GENERATED from `ACTIONS`, so an action without a help
/// line cannot exist; the views section is pinned to `theme::{SidebarTab, Document}`.
/// One line per layer for the help.
fn layer_help(l: Layer) -> &'static str {
    match l {
        Layer::Outlines => "the declared features' edges",
        Layer::Toolpaths => "the emitted motion through the op cursor",
        Layer::DeclaredStock => "the removals' boundaries at their floors and ceilings",
        Layer::SimulatedStock => "the swept stock after the ops that have run (built on a worker thread)",
        Layer::RestMaterial => "forgotten · violated cells, the rest report's own scan (built on a worker thread)",
        Layer::Fixtures => "the fixture's keep-outs",
        Layer::Tool => "the declared body at a problem's tip, at the block cursor's move, or at the selected op's deepest point",
        Layer::Annotations => "a cross or a line wherever a problem names a place",
        Layer::Engagement => "the immersion field over the selected floor — how much of the tool's circle is in material at each place its centre may be, before this op cuts: faint within the stepover, brighter above it, brightest past 90°",
    }
}

/// The op cursor's arithmetic and the transport bar's text, one home
/// (DESIGN.md I8): `cursor` is how many ops have run, `count` how many
/// there are.
pub struct Transport {
    pub cursor: usize,
    pub count: usize,
    /// The block cursor nested inside the op cursor (Phase 16 slice 3,
    /// DESIGN.md §5.6): which of the highlighted op's moves is active, and
    /// how many it has. `None` is today's whole-op grain, and any change
    /// of the op cursor clears it — the nesting invariant.
    pub block: Option<usize>,
    pub blocks: usize,
}

impl Transport {
    /// The cursor cannot pass the last op.
    pub fn clamp(cursor: usize, count: usize) -> usize {
        cursor.min(count)
    }

    /// The bar's label, the one place the cursor is said: what the
    /// viewport and the stock are shown through, by the operation's own
    /// name, and how many have run. "Shown through", because a selected
    /// operation's card may be open on a different one.
    pub fn line(&self, p: &Part) -> String {
        let n = self.count;
        if n == 0 {
            return "No operations".to_string();
        }
        let k = Transport::clamp(self.cursor, n);
        if k == 0 {
            return format!("Shown before any operation — 0 of {n} run");
        }
        let op = &p.operations[k - 1];
        let block = match self.block {
            Some(b) => format!(" · move {} of {}", b + 1, self.blocks),
            None => String::new(),
        };
        format!("Shown through [{}] {:?} on '{}' with T{} — {k} of {n} run{block}", k - 1, op.op_type, op.feature_name, op.tool_num)
    }

    /// The move row's own label: the same operation by the same name and
    /// where the move cursor stands in it. How to name a move is the
    /// slider's hover (`MOVE_ROW_HINT`), not a standing instruction.
    pub fn move_line(&self) -> String {
        let n = self.count;
        if n == 0 {
            return "No operations".to_string();
        }
        let k = Transport::clamp(self.cursor, n);
        if k == 0 {
            return "no operation run — step on for its moves".to_string();
        }
        let i = k - 1;
        match (self.block, self.blocks) {
            (_, 0) => format!("[{i}] emits no moves"),
            (None, m) => format!("[{i}]: {m} moves"),
            (Some(b), m) => format!("[{i}]: move {} of {m}", b + 1),
        }
    }
}

/// How a move is named, said on the move row's hover.
pub const MOVE_ROW_HINT: &str = "the move within the operation shown through — click a line in the Program, drag this, or press { and }; the tool stands at the move's end";

/// What the status line's LEFT segment is made of (DESIGN.md I10, §5.7):
/// busy work speaks first, then any notes (a theme that would not load,
/// a build that failed, a write refused, a draft unsent). What is
/// selected is the line's other segment, drawn at its right and never
/// joined into this string — a refusal that read as one sentence with
/// the selection was Phase 19's live-run finding (Phase 20 slice 0).
pub struct StatusParts<'a> {
    pub busy: Option<&'a str>,
    pub notes: &'a [&'a str],
}

/// The status line's notes: one line, empty when nothing is happening.
pub fn status_line(s: &StatusParts) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if let Some(b) = s.busy {
        parts.push(b);
    }
    parts.extend(s.notes.iter().copied().filter(|n| !n.is_empty()));
    parts.join(" · ").replace('\n', " ")
}

/// The unsent-draft note (DESIGN.md §5.7): a field typed in and not
/// committed is said, so a click away never leaves a draft standing
/// unannounced.
pub fn unsent_note(key: &str, draft: &str) -> String {
    format!("Unsent — {key} = {draft}: Enter writes it, Esc drops it")
}

/// The unsent note over every field being typed in: one draft says it
/// as `unsent_note` does; several name each, so a draft left in one
/// field while another is typed in is never dropped unannounced.
pub fn unsent_notes(drafts: &[(String, String)]) -> Option<String> {
    match drafts {
        [] => None,
        [(key, draft)] => Some(unsent_note(key, draft)),
        many => {
            let list: Vec<String> = many.iter().map(|(k, d)| format!("{k} = {d}")).collect();
            Some(format!("Unsent — {}: Enter writes each, Esc drops them", list.join(", ")))
        }
    }
}

// ---- the import act (Phase 50 slice 10) --------------------------------------

/// The sidecar's interpreter, where `python -m venv` puts it on each
/// platform — the one table the CLI, the window, the hint and the tests
/// read (`sidecar/README.md`'s own words).
pub fn sidecar_python(sidecar_dir: &std::path::Path) -> std::path::PathBuf {
    if cfg!(windows) {
        sidecar_dir.join(".venv").join("Scripts").join("python.exe")
    } else {
        sidecar_dir.join(".venv").join("bin").join("python")
    }
}

/// Where the sidecar is: `NACS_SIDECAR` when set, else `sidecar/` under
/// the working directory (the repo's layout, which every document
/// assumes). `None` when neither holds `import_step.py`.
pub fn sidecar_dir() -> Option<std::path::PathBuf> {
    let candidates = [std::env::var_os("NACS_SIDECAR").map(std::path::PathBuf::from), Some(std::path::PathBuf::from("sidecar"))];
    candidates.into_iter().flatten().find(|d| d.join("import_step.py").is_file())
}

/// A path as typed on the runner's line, made absolute against the
/// window's own working directory. The run's working directory is the
/// PROJECT directory, so a relative path handed to the process as typed
/// is judged in one directory and opened in another (the Phase 56 walk:
/// `stock tests/fixtures/l_plate.step` passed `is_file` at the repo root
/// and the sidecar, run inside `l-plate.cam/`, could not open it) — the
/// same trap the sidecar's own path fell into in slice 4.
pub fn absolute(path: &std::path::Path) -> std::path::PathBuf {
    if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir().map(|c| c.join(path)).unwrap_or_else(|_| path.to_path_buf()) }
}

/// Whether a dropped or named file is a STEP model.
pub fn is_step(path: &std::path::Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "step" | "stp"))
}

/// The sidecar's command for a STEP into a project directory —
/// `(interpreter, args)` — built and judged BEFORE anything runs: a
/// missing sidecar or venv is a sentence that names the setup, never a
/// spawn error. `up` is the model's up axis as the sidecar spells it.
/// The import's setup (ROADMAP item 1, slice 4): which MODEL axis points
/// up on the machine, which solid is the part when the file holds
/// several, and (slice 11) how far the blank stands outside the model —
/// the three words the sidecar takes (`--up`, `--solid`, `--allowance`),
/// kept as the field holds them and turned into arguments once, here. An
/// empty solid is the one solid; `S2` and `2` are the same answer. The
/// allowance is 1 mm unless typed: the stock proposal grows by it on the
/// sides and the top, and the top face and the rim become passes; `0` is a
/// blank cut to size, which is what the sidecar does on its own. A side
/// can have its own after the number (`1,bottom=3,x+=10`): that is where
/// the part sits in the stock. The
/// fourth word (slice 12) is the program before this one, `given=`, the
/// path as the project's `given from` line will name it: the blank is then
/// that program's turned over, computed here from its stock and handed to
/// the sidecar as `--blank`, and the allowance is that blank's own.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportSetup {
    pub up: Option<String>,
    pub solid: String,
    pub allowance: String,
    pub given: String,
    /// The point that becomes X0 Y0 Z0, as the sidecar's `--origin` takes
    /// it: `x,y,z`, each `model`, a number, `min`/`mid`/`max` of the blank
    /// or `part-min`/`part-mid`/`part-max` of the model. On a second setup
    /// (`given`) the blank is the given one and `model` keeps the first
    /// setup's zero on that axis.
    pub origin: String,
}

impl Default for ImportSetup {
    fn default() -> Self {
        ImportSetup { up: None, solid: String::new(), allowance: "1".to_string(), given: String::new(), origin: ORIGIN_DEFAULT.to_string() }
    }
}

/// The sides of the blank an allowance can name, in the sidecar's order.
pub const MARGIN_KEYS: [&str; 6] = ["top", "bottom", "x-", "x+", "y-", "y+"];

/// An allowance as its fields hold it: the number for the sides and the
/// top, then each side's own (empty where it takes the first; the bottom's
/// empty is 0). `1,bottom=3,x+=10` is the spelling. Text only: a number
/// half typed comes back as typed.
pub fn allowance_parts(text: &str) -> (String, [String; 6]) {
    let mut base = String::new();
    let mut own: [String; 6] = Default::default();
    for (i, word) in text.split(',').map(str::trim).enumerate() {
        match word.rsplit_once('=') {
            Some((key, val)) => {
                if let Some(k) = MARGIN_KEYS.iter().position(|m| *m == key.trim()) {
                    own[k] = val.trim().to_string();
                }
            }
            None if i == 0 => base = word.to_string(),
            None => {}
        }
    }
    (base, own)
}

/// The fields back into the one spelling the sidecar reads.
pub fn allowance_from(base: &str, own: &[String; 6]) -> String {
    let clean = |t: &str| t.trim().replace([',', '='], "");
    let mut words: Vec<String> = Vec::new();
    if !clean(base).is_empty() {
        words.push(clean(base));
    }
    words.extend(MARGIN_KEYS.iter().zip(own).filter(|(_, v)| !clean(v).is_empty()).map(|(k, v)| format!("{k}={}", clean(v))));
    words.join(",")
}

/// How far the blank stands outside the model on each side, in
/// `MARGIN_KEYS` order, or why the allowance is not one.
pub fn allowance_sides(text: &str) -> Result<[f64; 6], String> {
    let bad = || format!("allowance `{}` is not a size in mm — 1, 0.5, or 0 for a blank cut to size; then top=, bottom=, x-=, x+=, y-= or y+= for a side of its own, as in 1,bottom=3", text.trim());
    let size = |t: &str| t.parse::<f64>().ok().filter(|a| a.is_finite() && *a >= 0.0);
    let mut base = 0.0;
    let mut own: [Option<f64>; 6] = [None; 6];
    for (i, word) in text.split(',').map(str::trim).enumerate() {
        match word.rsplit_once('=') {
            Some((key, val)) => {
                let k = MARGIN_KEYS.iter().position(|m| *m == key.trim()).filter(|k| own[*k].is_none()).ok_or_else(bad)?;
                own[k] = Some(size(val.trim()).ok_or_else(bad)?);
            }
            None if i == 0 => base = size(word).ok_or_else(bad)?,
            None => return Err(bad()),
        }
    }
    Ok(std::array::from_fn(|k| own[k].unwrap_or(if k == 1 { 0.0 } else { base })))
}

/// The origin an import takes unless told: the blank's centre in plan and
/// its top, the point a probing routine finds first.
pub const ORIGIN_DEFAULT: &str = "mid,mid,max";

/// The ready answers for X0 Y0: what the window says, then the x and y words.
pub const ORIGIN_XY: [(&str, &str, &str); 6] = [
    ("the stock's center", "mid", "mid"),
    ("the stock's X- Y- corner", "min", "min"),
    ("the stock's X+ Y- corner", "max", "min"),
    ("the stock's X- Y+ corner", "min", "max"),
    ("the stock's X+ Y+ corner", "max", "max"),
    ("as modeled", "model", "model"),
];

/// The ready answers for Z0.
pub const ORIGIN_Z: [(&str, &str); 4] = [("the stock's top", "max"), ("the part's top", "part-max"), ("the bottom", "min"), ("as modeled", "model")];

/// An origin's three words, or why it is not one.
pub fn origin_words(origin: &str) -> Result<[String; 3], String> {
    let parts: Vec<String> = origin.split(',').map(|p| p.trim().to_string()).collect();
    let ok = |p: &str| p == "model" || ["min", "mid", "max"].contains(&p.strip_prefix("part-").unwrap_or(p)) || p.parse::<f64>().is_ok_and(f64::is_finite);
    match <[String; 3]>::try_from(parts) {
        Ok(words) if words.iter().all(|w| ok(w)) => Ok(words),
        _ => Err(format!("origin `{origin}` is not x,y,z with each of model, a number, min, mid, max, part-min, part-mid or part-max — mid,mid,max is the stock's center and top")),
    }
}

/// An origin in a sentence: "X0 Y0 at the stock's center, Z0 at the stock's
/// top". Words with no ready answer are said as they are written.
pub fn origin_says(origin: &str) -> String {
    let Ok([x, y, z]) = origin_words(origin) else { return format!("origin {origin}") };
    let xy = ORIGIN_XY.iter().find(|(_, a, b)| *a == x && *b == y).map(|(says, ..)| says.to_string()).unwrap_or_else(|| format!("x {x}, y {y}"));
    let z = ORIGIN_Z.iter().find(|(_, a)| *a == z).map(|(says, _)| says.to_string()).unwrap_or_else(|| format!("z {z}"));
    if x == "model" && y == "model" && z == "as modeled" {
        return "X0 Y0 Z0 as modeled".to_string();
    }
    format!("X0 Y0 at {xy}, Z0 at {z}")
}

/// The six answers to "which way is up": the model's axis that points up
/// on the machine, `z+` meaning as modeled.
pub const UP_AXES: [&str; 6] = ["z+", "z-", "x+", "x-", "y+", "y-"];

impl ImportSetup {
    pub fn args(&self) -> Result<Vec<String>, String> {
        let mut out = Vec::new();
        if let Some(axis) = &self.up {
            if !UP_AXES.contains(&axis.as_str()) {
                return Err(format!("up=`{axis}` is not an axis — x+, x-, y+, y-, z+ or z-"));
            }
            out.extend(["--up".to_string(), axis.clone()]);
        }
        let solid = self.solid.trim();
        if !solid.is_empty() {
            let n = solid.trim_start_matches(['S', 's']).parse::<u32>().ok().filter(|n| *n >= 1).ok_or_else(|| format!("solid `{solid}` is not a solid's number — S1, S2… or 1, 2…"))?;
            out.extend(["--solid".to_string(), n.to_string()]);
        }
        let allowance = self.allowance.trim();
        if !allowance.is_empty() {
            allowance_sides(allowance)?;
            // A given blank carries its own allowance: the word is not sent.
            if self.given.trim().is_empty() {
                let (base, own) = allowance_parts(allowance);
                out.extend(["--allowance".to_string(), allowance_from(&base, &own)]);
            }
        }
        let origin = self.origin.trim();
        if !origin.is_empty() {
            // A second setup takes the word too: it is read on the given
            // blank, and the `given from` line says where the first zero went.
            out.extend(["--origin".to_string(), origin_words(origin)?.join(",")]);
        }
        Ok(out)
    }

    /// The setup as the runner's line spells it: `up=y+ solid=2 allowance=0 given=../front.cam`.
    pub fn from_words<'a>(words: impl Iterator<Item = &'a str>) -> ImportSetup {
        let mut s = ImportSetup::default();
        for w in words {
            if let Some(a) = w.strip_prefix("up=") {
                s.up = Some(a.to_string());
            } else if let Some(n) = w.strip_prefix("solid=") {
                s.solid = n.to_string();
            } else if let Some(a) = w.strip_prefix("allowance=") {
                s.allowance = a.to_string();
            } else if let Some(g) = w.strip_prefix("given=") {
                s.given = g.to_string();
            } else if let Some(o) = w.strip_prefix("origin=") {
                s.origin = o.to_string();
            }
        }
        s
    }

    /// The sidecar's two words for the second setup (slice 12), or none
    /// when no program is given: `--given <project as written> --blank
    /// <that program's stock turned as up= says>`. The project is found as
    /// the parser finds a `given from` path — as written, then against the
    /// project's directory — and loaded through the one `load_project`
    /// home, so a program that does not stand on its own is refused with
    /// its own sentence. The turn is `up=`'s word, z+ or z- (a quarter turn
    /// is no turn a program reads, `UpAxis::from_word`'s own refusal), and
    /// the turned stock is `stock::turn_stock`, the validator's own, so the
    /// line this writes is the line the validator will accept. A Profile
    /// blank has an outline to mirror, which this does not write.
    pub fn given_args(&self, project_dir: &std::path::Path) -> Result<Vec<String>, String> {
        let given = self.given.trim();
        if given.is_empty() {
            return Ok(Vec::new());
        }
        let up = parse::UpAxis::from_word(self.up.as_deref().unwrap_or("z+")).map_err(|e| format!("given={given} with {e}"))?;
        let written = std::path::Path::new(given);
        let resolved = if written.exists() { written.to_path_buf() } else { project_dir.join(written) };
        if !resolved.exists() {
            return Err(format!("given=`{given}`: no such project — looked as written and against {}", project_dir.display()));
        }
        let part = parse::load_project(&resolved.to_string_lossy()).map_err(|e| {
            let (parse::LoadError::Io(e) | parse::LoadError::Parse(e)) = e;
            format!("given=`{given}`: the earlier program does not stand on its own — {e}")
        })?;
        if let parse::StockShape::Profile { points } = &part.stock.shape {
            return Err(format!(
                "given=`{given}`: the earlier program's blank is a Profile over {} points, whose outline turned over this door does not write — declare the turned outline by hand",
                points.len()
            ));
        }
        let turned = crate::stock::turn_stock(&part.stock, up);
        let mut out = vec!["--given".to_string(), given.to_string(), "--blank".to_string(), crate::stock::stock_words_row(&turned)];
        // The model point the earlier program zeroed at, from its import's
        // report: the same model is read here, and must be found in the
        // frame the blank is written in. No report, or none with an
        // origin, is a program written at the model's own zero.
        let earlier_zero = std::fs::read_to_string(resolved.join("report.json")).ok().and_then(|text| report_origin_at(&text));
        if let Some([x, y, z]) = earlier_zero {
            out.extend(["--given-origin".to_string(), format!("{x},{y},{z}")]);
        }
        Ok(out)
    }
}

/// A report's `origin_at`, the model point its import zeroed at. Read as
/// text: the report's own reader is the window's, and this door is the
/// lean build's too.
pub fn report_origin_at(report: &str) -> Option<[f64; 3]> {
    let after = report.split_once("\"origin_at\"")?.1;
    let inside = after.split_once('[')?.1.split_once(']')?.0;
    let nums: Vec<f64> = inside.split(',').map(|n| n.trim().parse::<f64>().ok()).collect::<Option<_>>()?;
    <[f64; 3]>::try_from(nums).ok()
}

pub fn import_command(sidecar: &std::path::Path, step: &std::path::Path, project_dir: &std::path::Path, setup: &ImportSetup) -> Result<(std::path::PathBuf, Vec<String>), String> {
    if !is_step(step) {
        return Err(format!("{} is not a STEP file (.step / .stp)", step.display()));
    }
    if !step.is_file() {
        return Err(format!("cannot read {}", step.display()));
    }
    let python = sidecar_python(sidecar);
    if !python.is_file() {
        return Err(format!(
            "the sidecar's venv is not set up — {} is missing. Once: {} (Python 3.11, about 1.5 GB; sidecar/README.md)",
            python.display(),
            if cfg!(windows) { "sidecar\\setup.ps1" } else { "sidecar/setup.sh" }
        ));
    }
    let setup_args = setup.args()?;
    let given_args = setup.given_args(project_dir)?;
    let mut args = vec![
        sidecar.join("import_step.py").to_string_lossy().to_string(),
        step.to_string_lossy().to_string(),
        "-o".to_string(),
        format!("{}{}", project_dir.display(), std::path::MAIN_SEPARATOR),
    ];
    args.extend(setup_args);
    args.extend(given_args);
    Ok((python, args))
}

/// Run the import, waiting: the sidecar's own last lines on success (its
/// summary — "120 faces: 40 proposed, 80 unsupported"), its stderr as
/// the sentence on failure. The sidecar never touches the network; this
/// spawns it and nothing else.
pub fn run_import(sidecar: &std::path::Path, step: &std::path::Path, project_dir: &std::path::Path, setup: &ImportSetup) -> Result<String, String> {
    let (python, args) = import_command(sidecar, step, project_dir, setup)?;
    let out = std::process::Command::new(&python).args(&args).output().map_err(|e| format!("could not run {}: {e}", python.display()))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if out.status.success() {
        Ok(if stdout.is_empty() { stderr } else { stdout })
    } else {
        Err(format!("the sidecar refused: {}", if stderr.is_empty() { stdout } else { stderr }))
    }
}

/// The sidecar's stock proposal (Phase 54 slice 5b, fork 3A): the python
/// and the arguments that run `import_stock.py` on a STEP into the
/// project directory — the same venv, the same setup words, the same
/// refusals as the import (`import_command`'s shape).
pub fn stock_command(sidecar: &std::path::Path, step: &std::path::Path, project_dir: &std::path::Path, setup: &ImportSetup) -> Result<(std::path::PathBuf, Vec<String>), String> {
    let (python, _) = import_command(sidecar, step, project_dir, setup)?;
    let mut args = vec![sidecar.join("import_stock.py").to_string_lossy().into_owned(), step.to_string_lossy().into_owned(), "-o".to_string(), project_dir.to_string_lossy().into_owned()];
    args.extend(setup.args()?);
    args.extend(setup.given_args(project_dir)?);
    Ok((python, args))
}

/// Run the stock proposal, waiting. `Ok` is the sidecar's one line when it
/// wrote `proposed_stock.part`; `Err` is its sentence when it declined (a
/// model that is not a prism) or could not run. A decline is not an import
/// failure: the features still proposed, the blank is declared by hand.
pub fn run_stock(sidecar: &std::path::Path, step: &std::path::Path, project_dir: &std::path::Path, setup: &ImportSetup) -> Result<String, String> {
    let (python, args) = stock_command(sidecar, step, project_dir, setup)?;
    let out = std::process::Command::new(&python).args(&args).output().map_err(|e| format!("could not run {}: {e}", python.display()))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if out.status.success() {
        Ok(if stdout.is_empty() { stderr } else { stdout })
    } else {
        Err(if stderr.is_empty() { stdout } else { stderr }.trim_start_matches("declined: ").to_string())
    }
}

/// The sidecar's proposed stock as the Review shows it (ROADMAP item 1,
/// slice 1): the file's text — whose comment says whether it is the model's
/// own outline or the smallest blank it fits (slice 2) — the statement alone
/// (the `#` lines dropped), a one-line head, and whether the project's stock already IS this one —
/// read by parsing the proposal through the one parser and comparing the
/// two stocks, so an accept by this window or any editor reads the same.
#[derive(Clone, Debug, PartialEq)]
pub struct StockProposal {
    pub head: String,
    pub text: String,
    pub statement: String,
    pub in_project: bool,
}

pub fn stock_proposal(proposal: &str, part: &Part) -> Result<StockProposal, String> {
    let statement: String = proposal
        .lines()
        .map(|l| l.split('#').next().unwrap_or("").trim_end())
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("
");
    // A proposal for a second setup (slice 12) carries the `given from`
    // line beside the stock. The comparison reads the stock alone: the
    // earlier project is found from the project's directory once the line
    // is written there, not from wherever this process runs.
    let given_from = proposal.lines().map(str::trim).find(|l| l.starts_with("given from ")).map(|l| l.trim_start_matches("given from ").to_string());
    let stock_only: String = proposal.lines().filter(|l| !l.trim().starts_with("given from ")).collect::<Vec<_>>().join("\n");
    let proposed = crate::parse::parse(&crate::template::project_text_from_proposal(&stock_only)?)?;
    let kind = statement.split_whitespace().nth(1).unwrap_or("").split('(').next().unwrap_or("");
    let s = &proposed.stock;
    let mut head = match kind {
        // A bounding blank (slice 2) is one line; its size is the row.
        "Cylinder" => format!("Stock  Cylinder ⌀{} × {}", s.w, s.h),
        "Box" => format!("Stock  Box {} × {} × {}", s.w, s.l, s.h),
        _ => format!("Stock  {kind} of {} points", statement.lines().count().saturating_sub(2)),
    };
    if let Some(gf) = given_from {
        head.push_str(&format!(", given from {}", gf.split_whitespace().next().unwrap_or("")));
    }
    Ok(StockProposal { head, text: proposal.trim_end().to_string(), statement, in_project: part.stock == proposed.stock })
}

/// The stock statement replaced by a proposed one (Phase 54 slice 5b): the
/// standing statement's extent — one line for a Box or a Cylinder, the
/// block for a Profile — leaves, and the proposal's lines stand in its
/// place; every other byte stays. The proposal's comment lines ride along
/// above the statement, as the sidecar wrote them.
pub fn replace_stock_statement(text: &str, first: usize, proposal: &str) -> Result<String, String> {
    let (first, last) = statement_extent(text, first)?;
    let code = text.lines().nth(first).unwrap_or("").split('#').next().unwrap_or("").trim();
    if !code.starts_with("stock ") {
        return Err(format!("line {} is not the stock statement: {code}", first + 1));
    }
    if !proposal.lines().any(|l| l.split('#').next().unwrap_or("").trim().starts_with("stock ")) {
        return Err("the proposal carries no stock statement".to_string());
    }
    let mut out = String::with_capacity(text.len() + proposal.len());
    for (i, l) in text.split_inclusive(NEWLINE).enumerate() {
        if i == first {
            out.push_str(proposal.trim_end());
            out.push(NEWLINE);
        }
        if i < first || i > last {
            out.push_str(l);
        }
    }
    Ok(out)
}

/// The repository's head for the band's chip (DESIGN.md §5.3, Phase 52
/// slice 7): the branch, and how many paths `git status` lists.
#[derive(Clone, Debug, PartialEq)]
pub struct GitHead {
    pub branch: String,
    pub changed: usize,
}

/// Read the head of the repository holding `dir` by running `git` as a
/// process — `run_import`'s precedent, the second place the program runs
/// one, no crate. `Err` says why there is none: no `git` on the PATH, or
/// no repository above `dir`. Read-only: the verbs are Phase 53's.
pub fn git_head(dir: &std::path::Path) -> Result<GitHead, String> {
    let run = |args: &[&str]| -> Result<String, String> {
        let out = std::process::Command::new("git").arg("-C").arg(dir).args(args).output().map_err(|e| format!("no git to run ({e})"))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            Err(if err.is_empty() { "git refused".to_string() } else { err })
        }
    };
    let branch = run(&["rev-parse", "--abbrev-ref", "HEAD"])?;
    let status = run(&["status", "--porcelain"])?;
    Ok(GitHead { branch, changed: status.lines().filter(|l| !l.trim().is_empty()).count() })
}

/// The chip's text and its hover (DESIGN.md §5.3): the project's name,
/// then ` · <branch>` with a `*` while the tree is dirty; the name alone
/// while the head is still being read or there is none, the hover saying
/// why. Pure, so the composition is pinned.
pub fn git_chip(project_path: &str, head: Option<&Result<GitHead, String>>) -> (String, String) {
    let name = std::path::Path::new(project_path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| project_path.to_string());
    match head {
        None => (name.clone(), format!("{name} — reading the repository…")),
        Some(Ok(h)) if h.changed == 0 => (format!("{name} · {}", h.branch), format!("branch {} — nothing changed since the last commit; click for fetch, pull, push, commit and branch", h.branch)),
        Some(Ok(h)) => (
            format!("{name} · {}*", h.branch),
            format!("branch {} — {} {} changed since the last commit; click for fetch, pull, push, commit and branch", h.branch, h.changed, if h.changed == 1 { "path" } else { "paths" }),
        ),
        Some(Err(why)) => (name.clone(), format!("{name} — no branch to show: {why}")),
    }
}

/// The Review pane's hint when no report stands (Phase 50 slice 10):
/// platform-aware, the drop first, the command second.
pub fn import_hint(project_dir: &str) -> String {
    let python = sidecar_dir().map(|d| sidecar_python(&d)).unwrap_or_else(|| sidecar_python(std::path::Path::new("sidecar")));
    format!(
        "Import STEP… (or drop a .step file on the window) runs the sidecar into {project_dir}: it proposes the stock and the features it recognizes, and this view lists them for review — Accept writes each into main.part, the stock first when the folder is fresh. From a terminal: nacs --import <model.step> {project_dir} (which runs {} sidecar/import_step.py).",
        python.display()
    )
}

/// Proposals that say the same statement as an earlier one, the feature
/// name aside (the sidecar proposes one `Plane(z=65.0)` per coplanar
/// face): `Some(i)` names the earlier row. Shown as "same as F3", and
/// skipped by *Accept all* — the earlier one is the feature.
pub fn duplicate_proposals(rows: &[ReviewRow]) -> Vec<Option<usize>> {
    let body = |s: &str| -> String {
        // `feature <name> <Kind>(…)` → `<Kind>(…)`: the words after the name.
        let mut it = s.splitn(3, ' ');
        let (_, _, rest) = (it.next(), it.next(), it.next());
        rest.unwrap_or(s).to_string()
    };
    let mut seen: Vec<(String, usize)> = Vec::new();
    rows.iter()
        .enumerate()
        .map(|(i, r)| {
            let Some(s) = &r.statement else { return None };
            // A member face carries its feature's statement: it repeats
            // the feature's own row, wherever that row stands.
            if r.member_of.is_some() {
                if let Some(head) = rows.iter().position(|h| h.member_of.is_none() && h.statement.as_ref() == Some(s)) {
                    return Some(head);
                }
            }
            let b = body(s);
            match seen.iter().find(|(x, _)| *x == b) {
                Some((_, first)) => Some(*first),
                None => {
                    seen.push((b, i));
                    None
                }
            }
        })
        .collect()
}

// ---- the third door (Phase 50 slice 9) ----------------------------------------

/// The acts a right-click offers on a selection (DESIGN.md §5.2, §5.5,
/// §6, 2026-10-15): `(id, label)`, every id an act in `ACTIONS` (pinned),
/// the label the menu's short word. A third door into the same acts the
/// keys and the buttons fire — no new write, no dialog (a menu is not a
/// dialog). The viewport's menu reads the picked feature's row.
pub fn context_actions(sel: Selection) -> Vec<(&'static str, &'static str)> {
    match sel {
        // Edit… first on everything that has a form (Phase 51 slice 3): the
        // way to a thing's settings from the thing itself.
        Selection::Operation(_) => vec![
            ("form.open", "Edit…"),
            ("view.focus", "Focus"),
            ("op.suppress", "Suppress"),
            ("op.move_top", "Move to top"),
            ("op.move_bottom", "Move to bottom"),
            ("statement.delete", "Delete"),
        ],
        Selection::Feature(_) => vec![("form.open", "Edit…"), ("op.new", "New operation…"), ("feature.clear_around", "Clear around it…"), ("feature.flip_side", "Flip side"), ("view.focus", "Focus"), ("statement.delete", "Delete")],
        Selection::Tool(_) => vec![("form.open", "Edit…"), ("view.focus", "Focus"), ("statement.delete", "Delete")],
        Selection::Stock => vec![("form.open", "Edit…"), ("view.fit", "Fit"), ("feature.new", "New feature…"), ("tool.new", "New tool…")],
        Selection::Finding(_) => vec![("view.focus", "Focus")],
        Selection::Review(_) => vec![("review.accept", "Accept into project"), ("review.reject", "Reject")],
        Selection::ProposedStock => vec![("review.accept", "Accept into project")],
        Selection::None => vec![("view.fit", "Fit"), ("feature.new", "New feature…"), ("tool.new", "New tool…")],
    }
}

/// Whether the sidebar's `tab` could have made `sel` by a click on one of
/// its rows (DESIGN.md §5.4, Phase 51 slice 3): the Outline's rows are the
/// stock, the features, the operations and the tools (the Sequence's were
/// the operations, until it folded into the Outline — Phase 52 slice 4);
/// Problems' the findings; Review's its rows. Nothing selected
/// is not a stale card, so it reads as shown. A tab change to a tab that
/// could not have made the standing selection shuts the form, so the form
/// is always the card of the last thing clicked in the tab on screen.
pub fn tab_shows(tab: SidebarTab, sel: Selection) -> bool {
    match sel {
        Selection::None => true,
        Selection::Stock | Selection::Feature(_) | Selection::Tool(_) => tab == SidebarTab::Outline,
        Selection::Operation(_) => tab == SidebarTab::Outline,
        Selection::Finding(_) => tab == SidebarTab::Problems,
        Selection::Review(_) | Selection::ProposedStock => tab == SidebarTab::Review,
    }
}

/// An operation's `run` line moved to the top or the bottom of the run
/// order (Phase 50 slice 9) as REPEATED SWAPS of neighbouring run lines
/// (`swap_run_lines`, the fourth write's own exchange, so every other line
/// stays where the author put it and a comment is never hoisted) — the
/// swaps composed in memory and written once. `op_lines` is the program's
/// `(file, line)` per operation; every run line must share the file.
pub fn move_run_line(text: &str, op_lines: &[(usize, usize)], op: usize, to_top: bool) -> Result<String, String> {
    let n = op_lines.len();
    if op >= n {
        return Err(format!("operation [{}] is not in the program", op));
    }
    let file = op_lines[op].0;
    if op_lines.iter().any(|(f, _)| *f != file) {
        return Err("the operations' run lines live in different part files".to_string());
    }
    let lines: Vec<usize> = op_lines.iter().map(|(_, l)| *l).collect();
    let mut out = text.to_string();
    if to_top {
        if op == 0 {
            return Err(format!("operation [{}] is already at the top", op));
        }
        for j in (1..=op).rev() {
            out = swap_run_lines(&out, lines[j], lines[j - 1])?;
        }
    } else {
        if op + 1 == n {
            return Err(format!("operation [{}] is already at the bottom", op));
        }
        for j in op..n - 1 {
            out = swap_run_lines(&out, lines[j], lines[j + 1])?;
        }
    }
    Ok(out)
}

// ---- the two reports in the window, and the reference (Phase 50 slice 8) ----

/// Seconds as `4 m 12 s`, `18 s`, `1 h 02 m`.
pub fn mmss(s: f64) -> String {
    let s = s.round().max(0.0) as u64;
    let (h, m, sec) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h} h {m:02} m")
    } else if m > 0 {
        format!("{m} m {sec:02} s")
    } else {
        format!("{sec} s")
    }
}

/// The clock as one line (DESIGN.md §5.6, 2026-10-15): the whole program's
/// cutting and rapid time from `cut_time::time_report`, the CLI's own
/// function and the same numbers `--time-report` prints — a declined rapid
/// total said by name, a rejected program's clock its absence.
pub fn clock_line(r: &Result<crate::cut_time::TimeReport, String>) -> String {
    match r {
        Err(_) => "no clock — the program is rejected".to_string(),
        Ok(t) if t.declined() => format!("cut {} · rapids uncounted (no rapid rates in the machine file)", mmss(t.cutting_s())),
        Ok(t) => format!("cut {} · rapids {} · total {}", mmss(t.cutting_s()), mmss(t.rapid_s()), mmss(t.cutting_s() + t.rapid_s())),
    }
}

/// The rest report's verdict as the Uncut layer's own line (DESIGN.md §5.5,
/// 2026-10-15): what the scan found, in the report's words — so an empty
/// layer can be told from a scan that has not run.
pub fn rest_verdict(cells: &RestCellsView) -> String {
    let vol = cells.cell.powi(3);
    let say = |n: usize| if n == 0 { "none".to_string() } else { format!("{n} cells (~{:.1} mm³)", n as f64 * vol) };
    format!("uncut (declared removed, never swept): {} · gouged (swept where nothing was declared): {}", say(cells.forgotten.len()), say(cells.violated.len()))
}

/// The `.part` language and the window's acts, GENERATED from the tables
/// the parser, the templates and the window read (Phase 50 slice 8, tenet
/// 7: derived text, one source) — printed by `nacs --reference`, checked
/// in as `REFERENCE.md`, and pinned equal so the file cannot drift.
pub fn reference_text() -> String {
    let mut o = String::new();
    o.push_str("# The `.part` language, and the window's acts\n\n");
    o.push_str("Generated by `nacs --reference` from the parser's own key tables, the creation templates and the window's action table. Do not edit: regenerate (`nacs --reference > REFERENCE.md`); `tests/view_tests.rs` pins the file to the generator.\n\n");
    o.push_str("A project is a directory holding `main.part` (and, optionally, `geometry.part`). Lines are statements; `#` starts a comment; a block statement opens with `(` and closes with `)` on a later line, its points between.\n\n");
    o.push_str("## The header — one word, one value, each line\n\n");
    for k in crate::parse::HEADER_KEYS {
        let what = match k {
            "units" => "`mm` or `inch` — every number in the file is read in this unit",
            "feed" => "the cutting feed, per minute",
            "plunge" => "the descent feed, per minute",
            "spindle" => "rpm",
            "rapid_z" => "the height every entry rapids to before feeding — above the stock top",
            "clearance_z" => "the between-operations retract — above the stock top",
            _ => "",
        };
        o.push_str(&format!("- `{k} <value>` — {what}\n"));
    }
    o.push_str("- `work_offset <register>...` — optional: the register the program selects, `G54` to `G59` (`G54` unwritten); several run the program at each, one part a register\n");
    o.push_str("\n## The stock — the blank, declared first\n\n");
    o.push_str(&format!("- `stock Box({})` — `w`, `l`, `h` required; `x`, `y` the datum corner, `z` the TOP (0 when absent)\n", crate::parse::STOCK_KEYS.iter().map(|k| format!("{k}=")).collect::<Vec<_>>().join(", ")));
    o.push_str(&format!("- `stock Cylinder({})` — `dia`, `h` required; `x`, `y` the axis, `z` the top\n", crate::parse::STOCK_CYLINDER_KEYS.iter().map(|k| format!("{k}=")).collect::<Vec<_>>().join(", ")));
    o.push_str(&format!("- `stock Profile({})` + `x y` point lines + `)` — a blank whose plan outline is the loop (CCW); `h` required, `z` the top\n", crate::parse::STOCK_PROFILE_KEYS.iter().map(|k| format!("{k}=")).collect::<Vec<_>>().join(", ")));
    o.push_str("\n## Tools — `tool T<n> <Kind>(key=value, …)`\n\n");
    for spec in crate::template::TOOL_SPECS {
        let keys: Vec<String> = crate::template::tool_keys(spec).iter().map(|k| if k.required { format!("`{}`", k.name) } else { format!("`{}`?", k.name) }).collect();
        o.push_str(&format!("- **{}** — {}\n", spec.kind, keys.join(", ")));
        for k in spec.keys {
            if let Some(says) = crate::template::tool_key_says(spec.kind, k.name) {
                o.push_str(&format!("  - `{}`: {says}\n", k.name));
            }
        }
    }
    o.push_str("\n(`?` marks an optional key; the body keys at the end of every kind describe the shank and holder for the collision checks.)\n");
    o.push_str("\n## Features — `feature <name> <Kind>(key=value, …)`\n\n");
    for (group, variants) in crate::template::FEATURE_GROUPS {
        o.push_str(&format!("### {group}\n\n"));
        for name in variants.iter() {
            if let Some(spec) = crate::template::FEATURE_SPECS.iter().find(|s| s.variant == *name) {
                let keys: Vec<String> = spec.keys.iter().map(|k| if k.required { format!("`{}`", k.name) } else { format!("`{}`?", k.name) }).collect();
                let points = match spec.points {
                    crate::template::Points::None => String::new(),
                    crate::template::Points::Planar => " — then the points, `x y` per line, a planar loop (a pocket CCW, a boss CW); a point may add `chamfer_depth=` or `chamfer_width=` to restate the leg for the edges from it on, 0 for none".to_string(),
                    crate::template::Points::Wire => " — then the points, `x y z` per line, a closed wire".to_string(),
                    crate::template::Points::Loops => " — then `loop` and its points, the outer first (CCW), inner loops after (CW)".to_string(),
                };
                o.push_str(&format!("- **{}** — {}{}\n", spec.variant, if keys.is_empty() { "no keys".to_string() } else { keys.join(", ") }, points));
            }
        }
        o.push('\n');
    }
    o.push_str("## Operations — `run <Verb>(tool=T<n>, <reference>=<feature>, key=value, …)`\n\n");
    o.push_str("The verbs name the part of the tool that cuts: **End** (the end — a face or a floor), **Side** (the flank — a wall or an outside profile), **Clear** (a volume — the pocket's material), **Copy** (the surface — a 3D finish), **Form** (a form tool or a drill; with `points=[…]`, a visit to holes). Each reads one reference: `surface=` for End, Side, Copy and Form, `profile=` for Clear, `points=[…]` for a Form or an End at points.\n\nThe keys a `run` line takes:\n\n");
    for k in crate::parse::RUN_KEYS {
        match enum_options(k) {
            Some(words) => o.push_str(&format!("- `{k}=` — one of {}\n", words.iter().map(|w| format!("`{w}`")).collect::<Vec<_>>().join(", "))),
            None => {
                let unit = unit_of(k, true).map(|u| format!(" ({u})")).unwrap_or_default();
                o.push_str(&format!("- `{k}=`{unit}\n"));
            }
        }
    }
    o.push_str("\n## The window's acts — `nacs --view`\n\nEvery act below is a palette entry (`p`), many a button; a key where one is bound.\n\n");
    for a in ACTIONS {
        let key = key_words(a);
        let key = key.trim();
        if key.is_empty() {
            o.push_str(&format!("- `{}` — {}\n", a.id, a.sentence));
        } else {
            o.push_str(&format!("- `{}` — {}  ({key})\n", a.id, a.sentence));
        }
    }
    o.push_str("\n## The menu — `λ` at the left end of the sidebar's first row\n\nThe job's verbs first, then the submenus; every row is an act above, named here by its id. A row that is a state of the arrangement shows a check.\n\n");
    for (title, rows) in MENU {
        if !title.is_empty() {
            o.push_str(&format!("**{title}**\n"));
        }
        for (id, label) in rows.iter() {
            o.push_str(&format!("- {label} — `{id}`\n"));
        }
        o.push('\n');
    }
    o.push_str("**Window ▸ Theme** — the built-in palette, then the themes carried in the binary, then any `<name>.json` in the config folder (`%APPDATA%\\nacs\\themes` on Windows, `~/.config/nacs/themes` elsewhere; a file there wins by name). A pick is `--theme <name>`'s twin and is remembered in the workspace file; a change of `background.appearance` takes effect at the next launch.\n");
    o.push_str("**Window ▸ Switch window frame** — the window wears its own frame by default (Phase 57): the band is the title bar — drag it to move the window, double-click it to maximize or restore — with the caption buttons at its right end; on Windows 11 the corners are rounded and the border takes the theme's `border` colour. The switch puts the system's frame back, live, and is remembered as `frame native` in the workspace file; `--frame native` with `--view` is the switch's twin and is remembered the same way, as a `--theme` pick is.\n");
    o.push_str("**Output's line** — Enter runs the line in the project's directory, its words into the log above with the exit said last, one run at a time: `nacs <flags>` (this binary on this project; a line that starts with a flag is read as nacs's), `import <file.step> [up=…] [solid=N] [allowance=…] [origin=…] [given=…]` and `stock <file.step> [up=…] [solid=N] [allowance=…] [given=…]` (the sidecar; `given=` names the program before this one, whose blank turned over is the proposed stock), `git <verb>` (status, log, diff, show, branch, remote, fetch, pull, push, commit -m …, switch), and `sh <line>` — everything after `sh`, as typed, handed whole to the shell: the system's (`COMSPEC` on Windows, `/bin/sh` elsewhere) unless `shell <program>` in the workspace file names another, run as `<program> -c <line>`; the log says which under the echo. Anything else is refused by name and nothing runs. ↑ and ↓ on the line walk what was entered since the window opened, newest first — a refused line too, so a typo comes back to fix — and past the newest the line being typed comes back; the window's own memory, written to no file.\n");
    for (name, _) in crate::theme::SHIPPED_THEMES {
        o.push_str(&format!("- {name}\n"));
    }
    o.push('\n');
    o
}

// ---- the checker's words (Phase 50 slice 7) ----------------------------------

/// A finding family's word on SCREEN (DESIGN.md §5.2, 2026-10-15): the
/// machinist's word for what the family is about. The `--check=json`
/// wire keeps `check_result::family_word`'s spelling (`body_foul`,
/// `decline`), unchanged — this is a rename in the view sense only, DESIGN
/// shift #8's precedent — so the two never disagree about which family a
/// row is, only about how it is said.
pub fn family_display(family: parse::FindingFamily) -> &'static str {
    use parse::FindingFamily as F;
    match family {
        F::Engagement => "Tool load",
        F::Deviation => "Deviation",
        F::Entry => "Entry",
        F::Fixture => "Fixture",
        F::Reach => "Tool reach",
        F::Envelope => "Machine travel",
        F::Containment => "Outside stock",
        F::BodyFoul => "Holder collision",
        F::FieldBodyFoul => "Collision (simulated)",
        F::PartViolation => "Gouges part",
        F::Decline => "Not checked",
    }
}

/// A finding's sentence cut to what a row can carry: about two lines,
/// ended at a word. The whole sentence is on the finding's card.
pub fn finding_gist(text: &str) -> String {
    const ROW_CHARS: usize = 150;
    if text.chars().count() <= ROW_CHARS {
        return text.to_string();
    }
    let cut: String = text.chars().take(ROW_CHARS).collect();
    let end = cut.rfind(' ').unwrap_or(cut.len());
    format!("{}…", cut[..end].trim_end_matches(|c: char| !c.is_alphanumeric() && c != ')'))
}

/// The check's account for the Problems pane: its summary, then a line a
/// family in the words the rows use ("Holder collision", not `body_foul`).
/// `--check` prints the same account in the wire's words.
pub fn account_lines(acc: &[crate::check_result::FamilyAccount]) -> (String, Vec<String>) {
    let words = crate::check_result::account_words(acc);
    let summary = words.first().map(|s| s.strip_prefix("checked: ").unwrap_or(s).to_string()).unwrap_or_default();
    let lines = words
        .iter()
        .skip(1)
        .zip(acc)
        .map(|(line, a)| {
            let wire = crate::check_result::family_word(a.family);
            format!("{}{}", family_display(a.family), line.strip_prefix(wire).unwrap_or(line))
        })
        .collect();
    (summary, lines)
}

/// The pane's two counts (DESIGN.md §5.2, 2026-10-15): problems, and the
/// checks that could not run — a `decline` is "we could not look", not
/// "you have a mistake", and counting it as one trained the badge to be
/// ignored. The rejected row counts as a problem.
pub fn problem_counts(rows: &[FindingRow]) -> (usize, usize) {
    let unchecked = rows.iter().filter(|r| r.unchecked).count();
    (rows.len() - unchecked, unchecked)
}

/// The Problems view's own line with the account behind it (ROADMAP item
/// 2, slice 7): `no problems · 6 checks ran, 0 could not`. The empty pane
/// was never a certificate; this says what was checked, not only what was
/// found. "Could not" counts the operations some check declined to judge.
pub fn problems_line(rows: &[FindingRow], reload_failed: bool, acc: &[crate::check_result::FamilyAccount]) -> String {
    let (ran, _, declined) = crate::check_result::account_counts(acc);
    format!(
        "{} · {ran} {} ran, {declined} could not",
        view_line(SidebarTab::Problems, "", 0, 0, 0, rows, reload_failed, None),
        if ran == 1 { "check" } else { "checks" }
    )
}

/// The Problems heading: `Problems (2)`, `Problems (2 · 3 unchecked)`,
/// `Problems (3 unchecked)`, or `Problems`.
pub fn problems_heading(rows: &[FindingRow], reload_failed: bool) -> String {
    let (p, u) = problem_counts(rows);
    let p = p + usize::from(reload_failed);
    match (p, u) {
        (0, 0) => "Problems".to_string(),
        (p, 0) => format!("Problems ({p})"),
        (0, u) => format!("Problems ({u} unchecked)"),
        (p, u) => format!("Problems ({p} · {u} unchecked)"),
    }
}

/// Where a parse error points (Phase 50 slice 7): the file (`main.part`
/// unless the sentence names `geometry.part`) and the 1-based line, read
/// off the parser's own `line N:` prefix — so the pane can offer the file
/// to an editor. `None` for a sentence with no line.
pub fn parse_error_place(sentence: &str) -> Option<(&'static str, usize)> {
    let s = sentence.trim_start_matches("Not reloaded — ").trim_start_matches("parse error: ");
    let (file, rest) = match s.strip_prefix("geometry.part: ") {
        Some(r) => ("geometry.part", r),
        None => ("main.part", s),
    };
    let rest = rest.strip_prefix("line ")?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    let line: usize = digits.parse().ok()?;
    rest[digits.len()..].starts_with(':').then_some((file, line))
}

/// Hand a file to the desktop's own handler (Phase 50 slice 7): the
/// window is not an editor and never will be, but it can open the one the
/// user has. `start` on Windows, `open` on macOS, `xdg-open` elsewhere; a
/// failure is a sentence, never a panic.
pub fn open_in_editor(path: &std::path::Path) -> Result<(), String> {
    let p = path.to_string_lossy().to_string();
    let result = if cfg!(windows) {
        std::process::Command::new("cmd").args(["/C", "start", "", &p]).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(&p).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(&p).spawn()
    };
    result.map(|_| ()).map_err(|e| format!("could not open {p}: {e}"))
}

// ---- block features (Phase 50 slice 6) ---------------------------------------

/// A points answer (`x,y;x,y;…`) traversed the other way — what the
/// creation form does, and says, when the parser refuses a loop's
/// winding: the parser's law stands (the engine never normalises a
/// traversal), and the form that authored the points may author them
/// the other way round.
pub fn reverse_points(text: &str) -> String {
    let mut pts: Vec<&str> = text.split(';').map(str::trim).filter(|s| !s.is_empty()).collect();
    pts.reverse();
    pts.join(";")
}

// ---- the forms speak machining (Phase 50 slice 5) ----------------------------

/// The words an enumerated run key accepts (DESIGN.md §5.4, amended
/// 2026-10-15: an enum key is a menu over the parser's list). Mirrors the
/// parser's own matches and is pinned to them — every word here parses,
/// and the reserved words it refuses (`adaptive`, `force`, `spline`) are
/// not offered. A key that is a number or a reference is `None`.
pub fn enum_options(key: &str) -> Option<Vec<String>> {
    let words: &[&str] = match key {
        "pattern" => &["offset", "zigzag", "oneway", "spine", "held"],
        "comp" => &["engine", "control"],
        "compress" => &["chords", "arcs"],
        "direction" => &["climb", "conventional"],
        "entry" => &["ramp", "plunge", "lead"],
        "strategy" => &["rest", "regions"],
        "cycle" => &["Simple", "Peck"],
        "coolant" => &["flood", "mist", "air", "through", "off", "custom1", "custom2", "custom3"],
        _ => return None,
    };
    Some(words.iter().map(|w| w.to_string()).collect())
}

/// A key's unit, said beside its label (DESIGN.md §5.4, 2026-10-15):
/// lengths in the project's units, feeds per minute, the spindle in rpm,
/// angles in degrees — and nothing for a count, a word, a reference or a
/// file. The parser converts an inch file to mm at its boundary, so the
/// label says the FILE's unit, which is what the field edits.
pub fn unit_of(key: &str, units_mm: bool) -> Option<&'static str> {
    const DEGREES: &[&str] = &["angle", "chamfer_angle", "csink_angle", "rotation", "heading", "tilt", "angle_error", "engagement", "lead_angle", "lead_descent"];
    const NONE: &[&str] = &[
        "n", "flute_count", "cover", "tool", "surface", "profile", "points", "cycle", "comp", "compress", "strategy",
        "coolant", "pattern", "direction", "entry", "file", "units", "material", "body", "source", "name", "work_offset",
    ];
    if DEGREES.contains(&key) {
        return Some("°");
    }
    if NONE.contains(&key) {
        return None;
    }
    Some(match key {
        "feed" | "plunge" => if units_mm { "mm/min" } else { "in/min" },
        "spindle" => "rpm",
        _ => if units_mm { "mm" } else { "in" },
    })
}

/// The form's label for a key: `stepover (mm) =`, `angle (°) =`,
/// `pattern =`.
pub fn field_label(key: &str, units_mm: bool) -> String {
    match unit_of(key, units_mm) {
        Some(u) => format!("{key} ({u}) ="),
        None => format!("{key} ="),
    }
}

// ---- the doors (Phase 50 slice 4) -------------------------------------------
//
// Every act a mouse could not reach (DESIGN.md §5.1's "the doors, ruled",
// reopened by the Phase 50 DRAFT) gets a button that calls the same
// `act(id)` a key does: `(id, label, what)`, the hover built by
// `door_hover` so it names the key where one exists. Three strips, one
// list each, pinned to `ACTIONS` so a door cannot name an act that is not
// one. The shell draws them and decides nothing.

/// Over the viewport's top-right corner: the camera's five acts.
pub const VIEW_STRIP: &[(&str, &str, &str)] = &[
    ("view.fit", "Fit", "fit the scene"),
    ("view.focus", "Focus", "focus the selection — an oblique look down the tool axis"),
    ("view.plan", "Plan", "look straight down"),
    ("view.front", "Front", "look from the front"),
    ("view.right", "Right", "look from the right"),
];

/// The chip's menu (DESIGN.md §5.3, Phase 54 slice 3): git's five verbs,
/// `(id, label)`, every id in `ACTIONS` — the same rows the corner menu's
/// Git group carries.
pub const GIT_DOORS: &[(&str, &str)] = &[("git.fetch", "Fetch"), ("git.pull", "Pull"), ("git.push", "Push"), ("git.commit", "Commit…"), ("git.branch", "Branch…")];

/// On the transport bar, beside the cursor's arrows: nothing but the
/// cursor (Phase 52 slice 13, fork 13a-A). The way back — Undo, Redo —
/// stood here from Phase 50 slice 4 until the user asked for them out of
/// the scrubber's row; they are `STATUS_DOORS` now. The list stays so the
/// lints' three strips keep their shape, and so a door can come back
/// here by one line if the bar earns one.
pub const TRANSPORT_DOORS: &[(&str, &str, &str)] = &[];

/// The caption buttons at the band's right end (Phase 57 slice 1, fork
/// 0A): `(id, glyph, what)`, in the order a right-to-left row lays them —
/// the close outermost, as every title bar has it. Shown only while the
/// window wears its own frame; the system's frame has its own.
pub const CAPTION_DOORS: &[(&str, &str, &str)] = &[
    ("window.close", "🗙", "close the window"),
    ("window.maximize", "🗖", "maximize the window, or restore it"),
    ("window.minimize", "🗕", "minimize the window"),
];

/// The resize margin (Phase 57 slice 2, fork 2A): how far in from the
/// window's edge a drag resizes it, in points — the OS's own frame width
/// near enough, and a corner is this square.
pub const RESIZE_MARGIN: f32 = 6.0;

/// A window edge, the eight ways a frame can be dragged from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}

impl Edge {
    /// Every edge, the sides first and the corners after — the order the
    /// shell registers them, so a corner's square wins over the two sides
    /// it ends.
    pub const ALL: [Edge; 8] = [Edge::North, Edge::South, Edge::East, Edge::West, Edge::NorthEast, Edge::NorthWest, Edge::SouthEast, Edge::SouthWest];

    /// The edge's rect inside a window rect `[min x, min y, max x, max y]`,
    /// a `margin` wide: a side runs between the corners; a corner is the
    /// `margin` square.
    pub fn rect(self, r: [f32; 4], margin: f32) -> [f32; 4] {
        let [x0, y0, x1, y1] = r;
        let m = margin;
        match self {
            Edge::North => [x0 + m, y0, x1 - m, y0 + m],
            Edge::South => [x0 + m, y1 - m, x1 - m, y1],
            Edge::West => [x0, y0 + m, x0 + m, y1 - m],
            Edge::East => [x1 - m, y0 + m, x1, y1 - m],
            Edge::NorthWest => [x0, y0, x0 + m, y0 + m],
            Edge::NorthEast => [x1 - m, y0, x1, y0 + m],
            Edge::SouthWest => [x0, y1 - m, x0 + m, y1],
            Edge::SouthEast => [x1 - m, y1 - m, x1, y1],
        }
    }
}

/// The edge under a point, `margin` in from the window's rect — the corners
/// first, then the sides; `None` inside the margin or outside the window.
/// The one hit test, the oracle the shell's eight interacts agree with.
pub fn resize_edge(pos: [f32; 2], r: [f32; 4], margin: f32) -> Option<Edge> {
    let [x, y] = pos;
    let [x0, y0, x1, y1] = r;
    if x < x0 || x > x1 || y < y0 || y > y1 {
        return None;
    }
    let (w, e) = (x - x0 <= margin, x1 - x <= margin);
    let (n, s) = (y - y0 <= margin, y1 - y <= margin);
    Some(match (n, s, w, e) {
        (true, _, true, _) => Edge::NorthWest,
        (true, _, _, true) => Edge::NorthEast,
        (_, true, true, _) => Edge::SouthWest,
        (_, true, _, true) => Edge::SouthEast,
        (true, _, _, _) => Edge::North,
        (_, true, _, _) => Edge::South,
        (_, _, true, _) => Edge::West,
        (_, _, _, true) => Edge::East,
        _ => return None,
    })
}

/// The caption's glyph for the window as it stands: the maximize button
/// says restore (`🗗`) while the window is maximized; the others are their
/// one glyph. The four are the window-control glyphs the fonts carry
/// (U+1F5D5..U+1F5D9); the box-drawing set the DRAFT named they do not
/// (found by the glyph pin).
pub fn caption_glyph(id: &str, maximized: bool) -> &'static str {
    match (id, maximized) {
        ("window.maximize", true) => "🗗",
        _ => CAPTION_DOORS.iter().find(|(d, ..)| *d == id).map(|(_, g, _)| *g).unwrap_or(""),
    }
}

/// What the platform did about the window's own frame (Phase 57 slice 1):
/// on Windows 11 the corners rounded and the border coloured through DWM,
/// one call each beside the blur's; elsewhere the platform draws what it
/// draws. Said once on stderr, as the blur is.
#[derive(Clone, Debug, PartialEq)]
pub enum FrameDwm {
    NotAsked,
    NotOnThisPlatform,
    Applied { border: [u8; 3] },
    Refused(String),
}

/// The frame's one sentence: whose frame the window wears, and what the
/// platform did about the corners and the border.
pub fn frame_sentence(native: bool, dwm: &FrameDwm) -> String {
    if native {
        return String::from("Frame: the system's (`frame native` in the workspace file; Window ▸ Switch window frame)");
    }
    match dwm {
        FrameDwm::NotAsked => String::from("Frame: the window's own — the band is the title bar"),
        FrameDwm::NotOnThisPlatform => String::from("Frame: the window's own — the band is the title bar; this platform draws no corners or border for it"),
        FrameDwm::Applied { border } => format!("Frame: the window's own — the band is the title bar; corners rounded, border #{:02x}{:02x}{:02x} (Windows 11 DWM)", border[0], border[1], border[2]),
        FrameDwm::Refused(why) => format!("Frame: the window's own — the band is the title bar; corners and border refused by the platform — {why}"),
    }
}

/// On the status line's right: the help and the commands.
pub const STATUS_DOORS: &[(&str, &str, &str)] = &[
    // The way back, at the status line's right end (Phase 52 slice 13,
    // fork 13a-A, the user's "bottom right"): the status line's right end
    // is where acts on the project as a whole live, and the transport
    // bar holds the cursor and nothing else. Words, not arrows (Phase 52
    // slice 1): the fonts have no ↶ ↷; the glyph lint reads this list.
    ("edit.undo", "Undo", "undo the last write"),
    ("edit.redo", "Redo", "redo the write undone"),
    ("help", "?", "the help"),
    ("palette.open", "Commands", "the quick-action palette"),
];

/// The corner menu (DESIGN.md §5.1, reopened 2026-10-17; Phase 51 slice 4;
/// at the left end of the sidebar's first row since the rail retired,
/// Phase 52 slice 1): one button, Zed's shape — the job's
/// verbs first (the untitled group), then the ordinary submenus. Every row
/// is `(action id, label)` and every id is in `ACTIONS` (pinned); the
/// shell draws the tree and fires the one `act`, deciding nothing. The
/// Window rows that are states carry a check (`menu_check`). `--reference`
/// prints the same tree, so a menu row is never the only way to an act.
pub const MENU: &[(&str, &[(&str, &str)])] = &[
    ("", &[
        ("project.new", "New project…"),
        ("import.step", "New target…"),
        ("feature.new", "New feature…"),
        ("tool.new", "New tool…"),
        ("op.new", "New operation…"),
        ("program.write", "Write program"),
    ]),
    ("File", &[
        ("project.new", "New project…"),
        ("import.step", "Import STEP…"),
        ("stock.new", "New stock…"),
        ("origin.register", "Work offset…"),
        ("project.reload", "Reload project"),
        ("program.write", "Write program (.nc)"),
        ("program.copy", "Copy program"),
        ("sheet.copy", "Copy setup sheet"),
        ("review.accept_all", "Accept all proposals"),
        ("schedule", "Schedule operations"),
        ("options", "Options…"),
    ]),
    ("Git", &[
        ("git.fetch", "Fetch"),
        ("git.pull", "Pull"),
        ("git.push", "Push"),
        ("git.commit", "Commit…"),
        ("git.branch", "Branch…"),
    ]),
    ("Edit", &[
        ("edit.undo", "Undo"),
        ("edit.redo", "Redo"),
        ("op.suppress", "Suppress operation"),
        ("statement.delete", "Delete"),
        ("op.move_top", "Move to top"),
        ("op.move_bottom", "Move to bottom"),
        ("tools.renumber", "Renumber tools"),
        ("ops.number", "Number operations"),
        ("ops.strip_numbers", "Strip operation numbers"),
    ]),
    ("View", &[
        ("view.fit", "Fit"),
        ("view.focus", "Focus"),
        ("view.plan", "Plan"),
        ("view.front", "Front"),
        ("view.right", "Right"),
        (Layer::Outlines.action_id(), "Outlines"),
        (Layer::Toolpaths.action_id(), "Toolpaths"),
        (Layer::DeclaredStock.action_id(), "Removal boundaries"),
        (Layer::SimulatedStock.action_id(), "Stock simulation"),
        (Layer::RestMaterial.action_id(), "Uncut material"),
        (Layer::Fixtures.action_id(), "Fixtures"),
        (Layer::Tool.action_id(), "Tool"),
        (Layer::Annotations.action_id(), "Annotations"),
        (Layer::Engagement.action_id(), "Engagement"),
        ("program.numbers", "Program line numbers"),
        ("program.minimap", "Program minimap"),
    ]),
    ("Window", &[
        ("sidebar.toggle", "Sidebar"),
        ("form.toggle", "Form"),
        ("bottom.toggle", "Output"),
        ("output.clear", "Clear Output"),
        ("output.copy", "Copy Output"),
        ("output.save", "Save Output (.log)"),
        ("output.stop", "Stop run"),
        (SidebarTab::Outline.action_id(), "Outline"),
        (SidebarTab::Problems.action_id(), "Problems"),
        (SidebarTab::Review.action_id(), "Review"),
        (Document::Viewport.action_id(), "Viewport alone"),
        (Document::Program.action_id(), "Program alone"),
        ("center.split", "Viewport and Program"),
        ("layout.revert", "Reset layout"),
        ("theme.reload", "Reload theme"),
        ("window.frame", "Switch window frame"),
        ("window.minimize", "Minimize"),
        ("window.maximize", "Maximize or restore"),
        ("window.close", "Close window"),
    ]),
    ("Help", &[
        ("help", "Help"),
        ("palette.open", "Commands"),
    ]),
];

/// The Theme submenu's rows (DESIGN.md §5.1, Phase 51 slice 5): the
/// built-in palette first, then every theme the binary carries, then the
/// config folder's files not already named — each `(label, choice, on)`,
/// `on` the one the window wears. A pick is `--theme <name>`'s twin: the
/// workspace's `theme` line written, the window re-dressed where it can.
pub fn theme_menu(shipped: &[&str], folder: &[String], current: Option<&str>) -> Vec<(String, Option<String>, bool)> {
    let mut rows = vec![("Default (built-in)".to_string(), None, current.is_none())];
    for n in shipped {
        rows.push((n.to_string(), Some(n.to_string()), current == Some(n)));
    }
    for n in folder {
        if !shipped.contains(&n.as_str()) {
            rows.push((n.clone(), Some(n.clone()), current == Some(n.as_str())));
        }
    }
    rows
}

/// What a theme pick says about the glass (Phase 51 slice 5): the window
/// asked for a see-through surface — or not — when it was MADE, so a pick
/// that changes `background.appearance` across that line takes effect at
/// the next launch, and the status line says so; a pick within the same
/// appearance is live.
pub fn theme_pick_note(asked_glass_at_launch: bool, picked: crate::theme::BackgroundAppearance) -> Option<String> {
    let wants = picked != crate::theme::BackgroundAppearance::Opaque;
    if wants == asked_glass_at_launch {
        None
    } else if wants {
        Some("theme applied; its glass takes effect at the next launch (the surface was made opaque)".to_string())
    } else {
        Some("theme applied; the window stays see-through until the next launch".to_string())
    }
}

/// A menu row's check: `Some(on)` for an act that is a STATE of the
/// arrangement — a dock shown, a tab showing, a document in the center, a
/// layer on, a gutter on — `None` for a verb. One reading of `Layout`, so
/// the menu cannot form its own opinion of what is on screen.
pub fn menu_check(id: &str, l: &Layout) -> Option<bool> {
    if id == "sidebar.toggle" {
        return Some(l.sidebar.open);
    }
    if id == "form.toggle" {
        return Some(l.form.open);
    }
    if id == "program.numbers" {
        return Some(l.program_numbers);
    }
    if id == "program.minimap" {
        return Some(l.program_minimap);
    }
    if id == "center.split" {
        return Some(l.center == Center::Split);
    }
    if let Some(t) = SidebarTab::ALL.into_iter().find(|t| t.action_id() == id) {
        return Some(l.sidebar.open && l.sidebar.tab == t);
    }
    if let Some(d) = Document::ALL.into_iter().find(|d| d.action_id() == id) {
        return Some(l.center == Center::Alone(d));
    }
    if let Some(layer) = Layer::ALL.into_iter().find(|x| x.action_id() == id) {
        return Some(l.layers.get(layer));
    }
    None
}

/// Every door the window draws for an act, by id — the three strips, the
/// menu, the right-click menus over every selection kind. The "an act
/// nobody finds" lint (DESIGN.md §5.1, 2026-09-04; extended Phase 51
/// slice 4) reads it: a keyless act must appear here, or it is a
/// palette-only act.
pub fn doors_of(id: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    for (name, strip) in [("view strip", VIEW_STRIP), ("transport", TRANSPORT_DOORS), ("status", STATUS_DOORS), ("band", CAPTION_DOORS)] {
        if strip.iter().any(|(d, ..)| *d == id) {
            out.push(name);
        }
    }
    // The move grain's doors (Phase 52 slice 13): `{` `}` as buttons on
    // the transport bar's second row, beside the op cursor's pair.
    if matches!(id, "cursor.back" | "cursor.on" | "cursor.block_back" | "cursor.block_on") {
        out.push("transport");
    }
    if MENU.iter().any(|(_, rows)| rows.iter().any(|(d, _)| *d == id)) {
        out.push("menu");
    }
    let kinds = [Selection::None, Selection::Operation(0), Selection::Feature(0), Selection::Tool(1), Selection::Stock, Selection::Finding(0), Selection::Review(0), Selection::ProposedStock];
    if kinds.into_iter().any(|s| context_actions(s).iter().any(|(d, _)| *d == id)) {
        out.push("right-click menu");
    }
    out
}

/// A door's hover: what it does, and its key when it has one — "fit the
/// scene  (f Home)", "look straight down" for a keyless act.
pub fn door_hover(id: &str, what: &str) -> String {
    match action(id) {
        Some(a) if !a.key.is_empty() => format!("{what}  ({})", key_words(a).trim()),
        _ => what.to_string(),
    }
}

// ---- the way in (Phase 50 slice 3) ------------------------------------------

/// A header line's value: the text after the keyword, before any comment.
pub fn header_value(line: &str) -> String {
    let code = line.split('#').next().unwrap_or("").trim();
    code.split_once(char::is_whitespace).map(|(_, v)| v.trim().to_string()).unwrap_or_default()
}

/// A header line as the file spells it: `key value`.
pub fn header_line(key: &str, value: &str) -> String {
    format!("{key} {}", value.trim())
}

/// Where the header's `key` line stands in `main.part` (file 0), if the
/// file has one; the write rewrites it in place.
pub fn header_write_plan(p: &Part, key: &str) -> Option<usize> {
    p.header_lines.iter().find(|(k, f, ..)| k == key && *f == 0).map(|(_, _, l, _)| *l)
}

/// Where a header line the file LACKS is inserted: before the stock line,
/// which every parsed project has — else at the top.
pub fn header_insert_at(p: &Part) -> usize {
    p.stock_line.as_ref().map(|(_, l, _)| *l).unwrap_or(0)
}

/// `text` with `line` inserted before line index `at` (past the end:
/// appended); the file's own newline convention kept, a trailing newline
/// preserved.
pub fn insert_line(text: &str, at: usize, line: &str) -> String {
    let trailing = text.ends_with('\n');
    let mut lines: Vec<&str> = text.lines().collect();
    let at = at.min(lines.len());
    lines.insert(at, line);
    let mut out = lines.join("\n");
    if trailing || text.is_empty() {
        out.push('\n');
    }
    out
}

/// What a drop on the window opens: the first dropped path that is a
/// directory, or a `.part` file's directory. A drop is not a dialog.
pub fn dropped_project(paths: &[std::path::PathBuf]) -> Option<std::path::PathBuf> {
    paths.iter().find_map(|p| {
        if p.is_dir() {
            Some(p.clone())
        } else if p.extension().is_some_and(|e| e == "part") {
            p.parent().map(|d| d.to_path_buf())
        } else {
            None
        }
    })
}

/// The project a window opens on when the path holds no parsed project
/// (Phase 50 slice 3): nothing declared, and the files the watch follows
/// named — `main.part` (and `geometry.part` where one stands) — so the
/// file appearing, or being fixed, reloads it. A `main.part` path is its
/// directory.
pub fn empty_project_part(project: &std::path::Path) -> Part {
    // No path at all is a window with no project open: nothing to watch.
    if project.as_os_str().is_empty() {
        return Part::default();
    }
    let dir = if project.extension().is_some_and(|e| e == "part") {
        project.parent().map(|p| p.to_path_buf()).unwrap_or_default()
    } else {
        project.to_path_buf()
    };
    let mut p = Part::default();
    p.sources.push(dir.join("main.part"));
    if dir.join("geometry.part").exists() {
        p.sources.push(dir.join("geometry.part"));
    }
    p
}

/// The empty viewport's sentence for the two stances a window can open
/// on without a parsed project (DESIGN.md §1, Phase 50 slice 3): no
/// `main.part` — the New project form writes one; a file that does not
/// parse — the Problems row says where. `None` when a project stands.
pub fn stance_note(p: &Part, refusal: Option<&str>) -> Option<String> {
    if p.sources.is_empty() && p.features.is_empty() && p.tools.is_empty() && refusal.is_some_and(|r| r.starts_with(NO_PROJECT)) {
        return Some("No project open. Import STEP… starts one beside the model; a project folder dropped here opens it".to_string());
    }
    let main = p.sources.first()?;
    if !main.exists() {
        let dir = main.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        return Some(format!("{dir} has no main.part yet — Import STEP… proposes one from a model, or New project… writes one, every value shown first"));
    }
    if refusal.is_some_and(|r| r.starts_with("parse error")) {
        return Some("this project does not parse — the Problems row says where; fix the file and the window reloads".to_string());
    }
    None
}

/// What the check says in a window opened on no project; `stance_note`
/// and the job guide read it.
pub const NO_PROJECT: &str = "no project open";

/// The folder a STEP imported with no project open starts: beside the
/// model and named for it, `bracket.step` giving `bracket.cam`.
pub fn home_for_step(step: &std::path::Path) -> std::path::PathBuf {
    step.with_extension("cam")
}

/// What an import proposes, drawn before any of it is accepted: the
/// proposed blank's box and the wireframe of every feature still waiting.
pub struct Preview {
    /// Whether the blank itself is still a proposal (then its box is drawn
    /// in place of the project's).
    pub stock_waiting: bool,
    pub stock_bb: [f64; 6],
    pub edges: Vec<FeatureEdges>,
    pub fit_bb: [f64; 6],
}

/// The preview of a proposed blank and the feature statements still
/// waiting: the project they would make, parsed in memory. A statement the
/// parser refuses is left out of the picture; its row in the Review says why.
pub fn proposal_preview(stock_text: &str, statements: &[String], stock_waiting: bool) -> Option<Preview> {
    let stock_only: String = stock_text.lines().filter(|l| !l.trim().starts_with("given from ")).collect::<Vec<_>>().join("\n");
    let mut text = crate::template::project_text_from_proposal(&stock_only).ok()?;
    let mut part = crate::parse::parse(&text).ok()?;
    for s in statements {
        let candidate = append_statement(&text, s);
        if let Ok(p) = crate::parse::parse(&candidate) {
            text = candidate;
            part = p;
        }
    }
    let edges = feature_edges(&part);
    let stock_bb = stock_aabb(&part.stock);
    let fit_bb = edges_aabb(&edges, stock_bb);
    Some(Preview { stock_waiting, stock_bb, edges, fit_bb })
}

/// The program file beside the project (Phase 50 slice 2, the way out):
/// `<project>/<name>.nc`, the name the directory's own without its `.cam`
/// — `plate.cam/plate.nc`. A project named by its `main.part` is its
/// directory. One home, so the window's act, the strip's hover and the
/// note all say the same file; the CLI's `-o` takes any path and reads
/// this for nothing.
pub fn program_file_path(project: &std::path::Path) -> std::path::PathBuf {
    let dir = if project.extension().is_some_and(|e| e == "part") {
        project.parent().map(|p| p.to_path_buf()).unwrap_or_default()
    } else {
        project.to_path_buf()
    };
    let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let stem = name.strip_suffix(".cam").unwrap_or(&name);
    let stem = if stem.is_empty() { "program" } else { stem };
    dir.join(format!("{stem}.nc"))
}

/// The program as a file: `gcode::program`'s text with the one trailing
/// newline `println!` gives stdout, so a written file and `nacs <project>
/// > file` are the same bytes — the window's act, the CLI's `-o` and the
/// clipboard all go through here.
pub fn program_text(program: &str) -> String {
    format!("{program}\n")
}

/// Write the program beside the project — exactly the text `nacs
/// <project> -o` writes — through the one atomic rename every file write
/// takes. NOT a write under DESIGN.md §1's law: the `.nc` is derived and
/// regenerable, never read back, outside the undo stack, and outside the
/// watch (which polls the files the parse opened — pinned). Returns the
/// path written.
pub fn write_program(project: &std::path::Path, program: &str) -> Result<std::path::PathBuf, String> {
    let path = program_file_path(project);
    crate::theme::write_atomically(&path, &program_text(program))?;
    Ok(path)
}

/// The setup sheet's file, beside the program: `plate.cam/plate.setup.txt`.
pub fn sheet_file_path(project: &std::path::Path) -> std::path::PathBuf {
    program_file_path(project).with_extension("setup.txt")
}

/// Write the setup sheet beside the program. Derived like the `.nc`:
/// never read back, outside the undo stack. Returns the path written.
pub fn write_sheet(project: &std::path::Path, rows: &[(&'static str, String)]) -> Result<std::path::PathBuf, String> {
    let path = sheet_file_path(project);
    crate::theme::write_atomically(&path, &setup_sheet_text(rows))?;
    Ok(path)
}

/// The note the program's write leaves (DESIGN.md §5.7): the file and its
/// size, and no "z undoes" — there is nothing to undo, the file is the
/// program on screen.
pub fn program_written_note(file: &str, lines: usize) -> String {
    format!("Written — {file}: the program, {lines} lines")
}

/// The note a write leaves behind (DESIGN.md §5.7, a transient
/// confirmation): what changed, in which file, and the way back. Every
/// write used to end in silence — the file on disk rewritten and the only
/// signal a redraw half a second later.
pub fn written_note(file: &str, what: &str) -> String {
    format!("Written — {file}: {what} · z undoes")
}

/// The note an undo or a redo leaves behind — a write like any other,
/// and the way back named.
pub fn undone_note(file: &str, redone: bool) -> String {
    if redone {
        format!("Redone — {file} · z undoes")
    } else {
        format!("Undone — {file} · y redoes")
    }
}

/// How long a transient note stands on the status line before it clears
/// on its own: long enough to read, short enough that the line is empty
/// again when nothing is happening (§5.7).
pub const TRANSIENT_SECS: f64 = 4.0;

/// Whether a transient note of this age is still shown.
pub fn transient_shown(age_secs: f64) -> bool {
    age_secs < TRANSIENT_SECS
}

/// The keys column's word for an act with no key of its own — one
/// rendering for the help and the palette, which used to disagree (`·`
/// against `()`).
pub fn key_column(key: &str) -> &str {
    if key.is_empty() {
        "·"
    } else {
        key
    }
}

/// What Esc closes: the topmost transient and nothing beneath it. The
/// palette (it holds the keyboard) over the help over an unsent field
/// draft over the creation form — so closing the help never drops a form
/// full of typed values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dismiss {
    Nothing,
    Help,
    Palette,
    Drafts,
    Form,
}

pub fn dismiss_target(help: bool, palette: bool, drafts: bool, form: bool) -> Dismiss {
    if palette {
        Dismiss::Palette
    } else if help {
        Dismiss::Help
    } else if drafts {
        Dismiss::Drafts
    } else if form {
        Dismiss::Form
    } else {
        Dismiss::Nothing
    }
}

/// How long the pointer rests on the Layers button (or its list) before
/// the list drops (Phase 52 slice 11, fork 11A): the open was the twitch —
/// a pointer crossing the button on its way to Fit opened it — so the
/// open waits and the close does not.
pub const LAYERS_HOVER_DELAY: f64 = 0.25;

/// Whether the Layers group is unfolded this frame (DESIGN.md §5.5, Phase
/// 51 slice 2; the delay Phase 52 slice 11): `hovered_for` is how long the
/// pointer has rested over the button or the list, `None` when it is not
/// there — open once that reaches `LAYERS_HOVER_DELAY`, or while pinned.
/// Pure, so the rule is pinned here and the shell only measures.
pub fn layers_open(hovered_for: Option<f64>, pinned: bool) -> bool {
    pinned || hovered_for.is_some_and(|t| t >= LAYERS_HOVER_DELAY)
}

/// The scroll offset that centres `line` in a `show_rows` document whose
/// rows are `row_h` tall and `spacing_y` apart — the pitch egui lays rows
/// at is the sum, and an offset built from the height alone lands short
/// by one spacing per line (85 rows short at line 500).
pub fn program_scroll_offset(row_h: f32, spacing_y: f32, line: usize, avail_h: f32) -> f32 {
    ((row_h + spacing_y) * line as f32 - avail_h / 2.0).max(0.0)
}

// ---- the Program's gutters (Phase 23 slice 3, DESIGN.md §5.3) -----------------
//
// Two toggles the user asked for by name ("I miss the line numbers"): a
// line-number gutter on the left and a minimap on the right — the whole
// program as a strip, one thin row per line in its op's role colour, the
// visible span framed, the cursor's operation lit, a click or drag
// scrolling there. The strip's geometry is here, pure and pinned, so the
// shell paints what it is handed; the map says nothing the document does
// not (it is drawn from `GcodeLine.op` and the one colour home).

/// The gutter's width in characters for `n` lines: the digits of the last
/// line number, at least two.
pub fn line_number_width(n: usize) -> usize {
    n.max(1).to_string().len().max(2)
}

/// The rows on screen in a `show_rows` document scrolled to `offset`
/// (rows `row_h` tall, `spacing` apart, `avail` visible): the half-open
/// line range, clamped to `n`.
pub fn program_visible_span(offset: f32, row_h: f32, spacing: f32, avail: f32, n: usize) -> std::ops::Range<usize> {
    let pitch = (row_h + spacing).max(1e-3);
    let first = (offset.max(0.0) / pitch).floor() as usize;
    let count = (avail.max(0.0) / pitch).ceil() as usize + 1;
    let first = first.min(n);
    first..(first + count).min(n)
}

/// Where line `line` of `n` sits down a strip `strip_h` tall — the
/// strip's own scale, every line one slice of it, so a thousand-line
/// program is a thousand thin rows and a ten-line one ten fat ones. The
/// slice is `[top, bottom)`; `bottom - top` is the row's height.
pub fn minimap_row(line: usize, n: usize, strip_h: f32) -> (f32, f32) {
    if n == 0 {
        return (0.0, 0.0);
    }
    let pitch = strip_h / n as f32;
    (pitch * line as f32, pitch * (line + 1) as f32)
}

/// The scroll offset that puts the line under a click at `y` (down the
/// strip) in the MIDDLE of the document — the map's click and drag.
pub fn minimap_scroll_to(y: f32, strip_h: f32, n: usize, row_h: f32, spacing: f32, avail: f32) -> f32 {
    if n == 0 || strip_h <= 0.0 {
        return 0.0;
    }
    let line = ((y.clamp(0.0, strip_h) / strip_h) * n as f32).floor() as usize;
    program_scroll_offset(row_h, spacing, line.min(n.saturating_sub(1)), avail)
}

/// The rail slot's word with its count (DESIGN.md §5.2, §5.1 — fork 8 of
/// Phase 23): `Problems (3)` where the user looks, so a project that
/// turns red is not silent on every other view. Only Problems carries one
/// The tab's word with its count (DESIGN.md §5.2, §5.1 — fork 8 of
/// Phase 23, the rail's label; the heading's counts folded in when the
/// tabs took the rail's place, Phase 52 slice 1): `Problems (3)` and
/// `Review (2)` where the user looks, so a project that turns red is
/// not silent on every other view. `review` is `review_counts` when a
/// report stands: the tab counts the features still waiting.
pub fn tab_label(slot: &Tab, n_problems: usize, n_unchecked: usize, review: Option<(usize, usize)>) -> String {
    match (slot.tab, n_problems, n_unchecked) {
        (SidebarTab::Problems, 0, 0) => slot.label.to_string(),
        (SidebarTab::Problems, p, 0) => format!("{} ({p})", slot.label),
        (SidebarTab::Problems, 0, u) => format!("{} ({u} unchecked)", slot.label),
        (SidebarTab::Problems, p, u) => format!("{} ({p} · {u} unchecked)", slot.label),
        (SidebarTab::Review, _, _) => match review {
            Some((w, _)) if w > 0 => format!("{} ({w})", slot.label),
            _ => slot.label.to_string(),
        },
        _ => slot.label.to_string(),
    }
}

/// The status note a reload posts when the project goes from emitting
/// to refused (DESIGN.md §5.7, 2026-09-17): the diagnose stance never
/// moves the view on a reload, so the words are how the user hears it.
/// `None` for every other transition — a reload that stays green, or
/// stays red, says nothing new.
pub fn turned_red_note(refused_before: bool, refused_now: bool) -> Option<&'static str> {
    (!refused_before && refused_now).then_some("This project has errors now — no program was generated; the Problems view has the reason")
}

/// What a reference menu says when the project declares nothing it could
/// offer — and where the missing thing is made. A menu over an empty
/// inventory was a dead end: a new project could not schedule its first
/// operation from the window.
pub fn empty_inventory_note(key: &str) -> &'static str {
    match key {
        "tool" => "no tool declared — New tool… (Outline, under Tools) adds one",
        _ => "no feature declared — New feature… (Outline) adds one",
    }
}

pub fn help_lines() -> Vec<String> {
    let mut out: Vec<String> = [
        "what the window shows — each a view of something the terminal prints",
        "  Outline        the sidebar: five groups — Stock, Target, Features (the tree, each operation's `removes:` line), Operations (run order, the cursor drawn between ran and to-run, ⏶⏷ to reorder), Tools   (nacs --outline)",
        "  Problems       the sidebar: the check's result, a row per finding, an annotation per place   (nacs --check)",
        "  Review         the sidebar: the import's report face by face, a proposal accepted into the project from its row   (nacs --review)",
        "  Viewport       the center: the part, its toolpaths and the viewport's layers",
        "  Program        the center: the emitted program, the cursor's operation highlighted, the active move marked; line numbers on the left and a minimap on the right, each a toggle   (nacs <project>, on stdout)",
        "  both           the center: the Viewport beside the Program, the split draggable — the default; either alone is one act away",
        "  form           on the left, beneath the sidebar: what you selected — an operation and its tool, a feature and its run skeleton (nacs --template <feature>), a tool, or a problem — the keys its line carries, the rest of what its kind takes under More keys",
        "  plan map       an inset over the viewport: the whole program in the drawing-sheet plan   (the emitted motion, from above)",
        "  the plan map is not a pane: it is drawn over the viewport's corner, and a click or drag on it pans the view; the axis triad in its corner says which way X, Y and Z run on screen",
        "  the legend  beside the layers: each role's colour and its word (roughing, finishing, untagged)",
        "  the floor's edges  on the selected floor: solid where there is material beside it, dashed where there is air, dotted over a hole narrower than the tool   (nacs --outline, the region line)",
        "",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    // The viewport's layers — each a checkbox in the viewport's own widget,
    // never a pane. Generated from `Layer::ALL`, so the help cannot name a
    // layer the widget does not have or miss one it does.
    out.push("the viewport's layers — checkboxes over the viewport, saved in the workspace file".to_string());
    for l in Layer::ALL {
        let voice = match l {
            Layer::DeclaredStock => "   (nacs --outline)",
            Layer::RestMaterial => "   (nacs --rest-report)",
            Layer::Engagement => "   (nacs --engagement <op>)",
            _ => "",
        };
        out.push(format!("  {:<20} {}{voice}", l.label(), layer_help(l)));
    }
    out.push(String::new());
    out.push(
        "keys — every one an action; the palette (p) lists them"
            .to_string(),
    );
    for a in ACTIONS {
        // An act with no key of its own keeps the column — the menu, a
        // button and the palette reach it, and a `key` line can give it one.
        out.push(format!("  {:<14} {}", key_words(a), a.sentence));
    }
    out.extend(
        [
            "  drag an edge   resize a panel         a tab's word   show that view in the sidebar (every view's home is fixed)",
            "  Ctrl + / -     larger or smaller text (Ctrl 0 returns it; a chord never fires a key above — Ctrl+Z and Ctrl+Y are undo's and redo's own second names)",
            "  the tabs       the sidebar's first row: its views, one word each; the λ at their left is the menu",
            "  a `·` above    an act with no key of its own — click it, or give it one with a `key` line",
            "  rebind: a `key <action> <key>` line in the workspace file",
            "  the arrangement is saved on close and restored on open (~/.config/nacs/workspace)",
            "  the window uses --theme <name|file.json> (Zed-shape JSON, hot-reloaded;",
            "  judged by nacs --theme-check — a confusable role palette is rejected)",
            "",
            "mouse",
            "  drag           orbit        right-drag / middle-drag   pan        scroll   zoom about the cursor",
            "  click an edge   pick the feature (its template opens)",
            "  drag a grip    set that value — the small squares on the selection; written on release, z undoes",
            "  click a row    go to the finding's annotation, select its op, place its tool",
            "  click the map  pan the view there",
        ]
        .into_iter()
        .map(String::from),
    );
    out
}

// ---- the click's hit test ---------------------------------------------------

/// The clicked feature: the index of the edge set whose projected polylines
/// pass nearest the screen point, if any come within `tol` screen units —
/// the per-variant moment slice 2 reserved ("the later click-a-face slice
/// hit-tests this same struct"). Distance is point-to-segment in projected
/// screen space, because that is what "clicked ON the line" means to an
/// eye; depth never breaks a tie (a wireframe has no occlusion to honor),
/// so the nearest-on-screen edge wins and an exact tie goes to the earlier
/// (name-sorted) feature, deterministically.
pub fn pick_feature(
    edges: &[FeatureEdges],
    cam: &Camera,
    sx: f64,
    sy: f64,
    tol: f64,
) -> Option<usize> {
    // `vox::dist2d_seg` is the point-to-segment home (the voxel stamps'
    // own primitive, promoted for its second consumer rather than restated
    // — the ellipse_offset_pts precedent). One click pays a few thousand
    // square roots; not a hot path.
    let mut best: Option<(f64, usize)> = None;
    for (wi, edge) in edges.iter().enumerate() {
        for pl in &edge.polylines {
            for seg in pl.windows(2) {
                let (ax, ay, _) = cam.project(seg[0]);
                let (bx, by, _) = cam.project(seg[1]);
                let d = crate::vox::dist2d_seg(
                    sx,
                    sy,
                    f64::from(ax),
                    f64::from(ay),
                    f64::from(bx),
                    f64::from(by),
                );
                if d <= tol && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, wi));
                }
            }
        }
    }
    best.map(|(_, wi)| wi)
}

// ---- the floor's kinds, drawn (Phase 41 slice 1) ----------------------------
//
// `region::of` says what every stretch of a floor's boundary IS to the tool —
// WALL (stand off), OPEN (air, the tool may run off), and inside it what
// stands, what is a void the tool rides inside, and what it bridges. The
// terminal has said so since Phase 27 (`--outline`'s region line); nothing
// drew it. This is the drawing's model: one polyline per stretch, each with
// its kind, at the floor's height. The kinds are told apart by STROKE, not
// by colour — solid for what stands, dashed for air, dotted for a bridged
// mouth — so no theme slot is added and no eye is asked to see a hue
// (`DECISIONS.md` → "Phase 41 slice 1…").

/// How a stretch of a floor's boundary meets the tool, as the drawing says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeStroke {
    /// Material stands there: the host's wall, an island, a keep-out. Solid.
    Wall,
    /// Air: the blank's edge, or a void the tool overhangs. Dashed.
    Open,
    /// A void narrower than the tool, run over as if it were floor. Dotted.
    Bridged,
}

/// One stretch of a floor's boundary with its kind — an OPEN polyline (its
/// ends are where the kind changes), at the floor's z.
#[derive(Clone, Debug, PartialEq)]
pub struct KindStroke {
    pub kind: EdgeStroke,
    pub pts: Vec<[f64; 3]>,
}

/// The operation whose floor the picture kinds: the selected op (or the
/// one a problem names); for a selected FEATURE, the first op in `run`
/// order that works a region on it — the kinds are the feature's against
/// its neighbours and the blank, and only the tool's diameter (void or
/// bridge) is the op's, so the first op is as true an answer as any.
pub fn region_op(p: &Part, sel: Selection, rows: &[FindingRow], edges: &[FeatureEdges]) -> Option<usize> {
    if let Some(i) = selected_op(sel, rows) {
        return Some(i);
    }
    let name = &edges.get(selected_feature(sel, rows, edges)?)?.name;
    p.operations
        .iter()
        .enumerate()
        .find(|(_, o)| o.feature_name == *name && crate::region::of(p, o).is_some())
        .map(|(i, _)| i)
}

/// The outer boundary as runs of one kind. Edge `i` runs from vertex `i`
/// to `i+1`; consecutive edges of one kind join into one polyline, and a
/// run that crosses the loop's seam is joined too, so a dashed stretch is
/// dashed continuously rather than restarting at vertex 0.
fn outer_runs(outer: &[crate::parse::Pt], kinds: &[crate::region::EdgeKind], z: f64) -> Vec<KindStroke> {
    let n = outer.len();
    if n < 2 || kinds.len() != n {
        return Vec::new();
    }
    let kind_of = |k: crate::region::EdgeKind| match k {
        crate::region::EdgeKind::Wall => EdgeStroke::Wall,
        crate::region::EdgeKind::Open => EdgeStroke::Open,
    };
    // Start at a kind change, so the seam never splits a run. All one
    // kind: start anywhere, one closed run.
    let start = (0..n).find(|&i| kinds[i] != kinds[(i + n - 1) % n]).unwrap_or(0);
    let mut out: Vec<KindStroke> = Vec::new();
    for s in 0..n {
        let i = (start + s) % n;
        let a = outer[i];
        let b = outer[(i + 1) % n];
        let kind = kind_of(kinds[i]);
        match out.last_mut() {
            Some(run) if run.kind == kind => run.pts.push([b.x, b.y, z]),
            _ => out.push(KindStroke { kind, pts: vec![[a.x, a.y, z], [b.x, b.y, z]] }),
        }
    }
    out
}

/// A closed plan loop as one stroke of `kind` at `z`, closed on its start.
fn closed(kind: EdgeStroke, lp: &[crate::parse::Pt], z: f64) -> Option<KindStroke> {
    if lp.len() < 2 {
        return None;
    }
    let mut pts: Vec<[f64; 3]> = lp.iter().map(|q| [q.x, q.y, z]).collect();
    pts.push(pts[0]);
    Some(KindStroke { kind, pts })
}

/// The kinds of the floor op `op_idx` works, as strokes: the outer
/// boundary in runs; what stands inside (islands and keep-outs — the
/// path goes around both the same way) solid; open voids dashed at the
/// RIM the tool overhangs; bridged mouths dotted along the named
/// feature's own drawn edges (`edges`, by name — the one home for a
/// feature's geometry, never restated here). Empty for an op that works
/// no region (a flank finish, a drill), and for a rest op, whose floors
/// are several and its own line's business.
pub fn region_strokes(p: &Part, op_idx: usize, edges: &[FeatureEdges]) -> Vec<KindStroke> {
    let Some(op) = p.operations.get(op_idx) else { return Vec::new() };
    if crate::rest::is_rest(op) {
        return Vec::new();
    }
    let Some(region) = crate::region::of(p, op) else { return Vec::new() };
    let z = region.z;
    let mut out = outer_runs(&region.outer, &region.edges, z);
    out.extend(region.standing_polygons().iter().filter_map(|lp| closed(EdgeStroke::Wall, lp, z)));
    // The lens of each given void (Phase 42 slice 2): the floor the tool
    // may occupy with its body in the void's air, drawn at its edge — a
    // WALL stroke, because the body would touch the bore there. The
    // model's own loops, one closed stroke per piece.
    for lens in crate::stock::given_air(p, op) {
        out.extend(lens.floor.iter().filter_map(|lp| closed(EdgeStroke::Wall, lp, z)));
    }
    // The open void's shrunk outline is where the tool's CENTRE rides; the
    // rim it overhangs is that loop pushed back out by a radius. Drawing
    // the centre loop would say the air is smaller than it is, so the
    // dashed stroke goes on the rim — the named feature's own edges —
    // and the model carries nothing of its own.
    for name in region.open_voids().into_iter().chain(region.bridged()) {
        let kind = if region.bridged().contains(&name) { EdgeStroke::Bridged } else { EdgeStroke::Open };
        if let Some(e) = edges.iter().find(|e| e.name == name) {
            // A sunk feature's drawn edges are rings at its top and its
            // bottom and the ties between; its MOUTH in this floor is any
            // one of those rings brought to the floor's height (a bore's
            // rings are one circle at two heights), so the first level ring
            // is taken and the rest, identical in plan, are not drawn twice.
            let mut seen: Vec<Vec<[i64; 2]>> = Vec::new();
            for pl in &e.polylines {
                let level = pl.len() >= 2 && pl.iter().all(|q| (q[2] - pl[0][2]).abs() < 1e-6);
                if !level {
                    continue;
                }
                let key: Vec<[i64; 2]> = pl.iter().map(|q| [(q[0] * 1e6).round() as i64, (q[1] * 1e6).round() as i64]).collect();
                if seen.contains(&key) {
                    continue;
                }
                seen.push(key);
                out.push(KindStroke { kind, pts: pl.iter().map(|q| [q[0], q[1], z]).collect() });
            }
        }
    }
    out
}

// ---- the handles: a drag is a draft (Phase 41 slice 4) ----------------------
//
// A key with a spatial meaning gets a grip at the place it means; dragging
// the grip produces a DRAFT value that goes through the one write path
// exactly as typing it into the form would — `set_key` on the statement's
// own line, the whole project re-parsed, the file rewritten, `z` undoes.
// The third door into that path, beside the form and the Program. This is
// the model: which keys, where, along which axis, scaled how; the shell
// draws the grips and reports the pointer. A handle may write nothing the
// form cannot, and holds no state the selection does not derive
// (`DECISIONS.md` → "Phase 41 slice 4…").

/// One grip.
#[derive(Clone, Debug, PartialEq)]
pub struct Handle {
    /// The statement the drag writes — the form's own target.
    pub statement: Statement,
    pub key: String,
    /// Where the grip is drawn, world mm.
    pub at: [f64; 3],
    /// The world direction a drag of +1mm along moves the grip.
    pub axis: [f64; 3],
    /// How much the VALUE changes per mm of grip travel: 1 for a length
    /// or a coordinate, 2 for a diameter whose grip rides the rim.
    pub per_mm: f64,
    /// The file's value.
    pub value: f64,
    /// The least a drag may draft (a stepover stays positive, an
    /// allowance may reach zero).
    pub min: f64,
    /// The most a drag may draft (Phase 43 slice 4): a stepover stops
    /// under the tool's diameter, where `validate` would refuse it — the
    /// grip declines earlier than the form, never later. Infinite for a
    /// key with no ceiling.
    pub max: f64,
    /// What the status line says when a drag reaches `max`, or nothing.
    pub stop: Option<String>,
    /// What the status line says when a drag reaches `min`, or nothing
    /// (ROADMAP item 3 slice 2: a stepover's floor).
    pub floor_stop: Option<String>,
}

/// The sentence a drag that has reached its grip's ceiling earns, if any.
pub fn handle_stop(h: &Handle, value: f64) -> Option<&str> {
    if value >= h.max {
        h.stop.as_deref()
    } else if value <= h.min {
        h.floor_stop.as_deref()
    } else {
        None
    }
}

/// The most squares the Engagement layer paints (ROADMAP item 3 slice 3).
/// The field is read at an eighth of the tool's diameter — 153,600 cells
/// for the plate's face under a ⌀2 — and a polygon per cell per frame was
/// the crawl: measured headless in a debug build, 101 ms a frame against
/// 9 ms with the layer off. Over the budget the field is DRAWN coarser,
/// `step` cells a square, the worst band of each; what `--engagement`
/// reads is untouched.
pub const IMMERSION_DRAW_BUDGET: usize = 20_000;

/// The immersion field as the layer draws it: squares of `step` × `step`
/// cells, each the worst band of the cells under it.
#[derive(Clone, Debug, PartialEq)]
pub struct ImmersionBlocks {
    /// Cells a square; 1 is the field itself.
    pub step: usize,
    /// A square's side, mm.
    pub side: f64,
    /// The floor's height.
    pub z: f64,
    /// `(centre x, centre y, band)` per square with a value under it. A
    /// square at the field's edge may reach past the last cell by under
    /// a square; the picture is a steer, not a measurement.
    pub blocks: Vec<(f64, f64, crate::engagement::Band)>,
}

/// Cells a square so that `nx × ny` cells draw as at most `budget` squares.
pub fn immersion_draw_step(nx: usize, ny: usize, budget: usize) -> usize {
    let cells = nx * ny;
    if cells <= budget {
        return 1;
    }
    ((cells as f64 / budget.max(1) as f64).sqrt().ceil() as usize).max(1)
}

pub fn immersion_blocks(f: &crate::engagement::Immersion) -> ImmersionBlocks {
    let step = immersion_draw_step(f.nx, f.ny, IMMERSION_DRAW_BUDGET);
    let side = f.cell * step as f64;
    let (bx, by) = (f.nx.div_ceil(step), f.ny.div_ceil(step));
    let mut blocks = Vec::new();
    for jy in 0..by {
        for jx in 0..bx {
            let mut worst: Option<f64> = None;
            for iy in jy * step..((jy + 1) * step).min(f.ny) {
                for ix in jx * step..((jx + 1) * step).min(f.nx) {
                    if let Some(t) = f.theta[iy * f.nx + ix] {
                        worst = Some(worst.map_or(t, |w| w.max(t)));
                    }
                }
            }
            if let Some(t) = worst {
                blocks.push((f.x0 + (jx as f64 + 0.5) * side, f.y0 + (jy as f64 + 0.5) * side, f.band(t)));
            }
        }
    }
    ImmersionBlocks { step, side, z: f.z, blocks }
}

/// What the layer's line adds when the field is drawn coarser than read.
pub fn immersion_drawn_note(step: usize) -> Option<String> {
    (step > 1).then(|| format!("drawn {step}×{step} cells a square, the worst of each"))
}

/// The least a stepover grip drafts: a twentieth of the tool's diameter
/// (ROADMAP item 3 slice 2). The grip's floor was 0.001 mm, and a drag
/// through it sent the offset family away for a minute with the window
/// frozen — measured on plate_floated's Clear under a ⌀10 in a debug
/// build: 0.5 mm plans in 2 s, 0.25 in 9 s, 0.1 in 66 s. A finer value may
/// still be typed; the grip is where a hand overshoots.
pub fn stepover_floor(dia: f64) -> f64 {
    dia / 20.0
}

/// Where a grip is drawn and picked, in view coordinates (from the
/// viewport's centre, +y up): its point's image — or `None` when its
/// axis's image is under two pixels (the axis looks at the viewer, the
/// floor `handle_drag` drafts nothing under), so a grip that could not be
/// dragged in this stance is neither drawn nor picked rather than drawn
/// on top of another (on the Right stance a bore's `dia` and `x` would
/// otherwise share the centre's pixel; Phase 43 slice 2). The ONE home
/// for a grip's place on screen: the shell paints here and `pick_handle`
/// picks here.
pub fn handle_view_point(h: &Handle, cam: &Camera) -> Option<(f64, f64)> {
    let lens = cam.lens();
    let (x, y, _) = lens.project(h.at);
    let (x, y) = (f64::from(x), f64::from(y));
    let (ax, ay, _) = lens.project([h.at[0] + h.axis[0], h.at[1] + h.axis[1], h.at[2] + h.axis[2]]);
    let (dx, dy) = (f64::from(ax) - x, f64::from(ay) - y);
    if dx.phypot(dy) < 2.0 {
        return None;
    }
    Some((x, y))
}

/// The value a key carries on a line, when it is a number.
fn numeric(line: &str, key: &str) -> Option<f64> {
    fields_of(line).into_iter().find(|(k, _)| k == key).and_then(|(_, v)| v.trim().parse().ok())
}

/// A handle for a key the line CARRIES as a number — an absent key has
/// no value to drag from, and the form is where one is first typed.
fn handle(line: &str, statement: &Statement, key: &str, at: [f64; 3], axis: [f64; 3], per_mm: f64, min: f64) -> Option<Handle> {
    let value = numeric(line, key)?;
    Some(Handle { statement: statement.clone(), key: key.to_string(), at, axis, per_mm, value, min, max: f64::INFINITY, stop: None, floor_stop: None })
}

const X: [f64; 3] = [1.0, 0.0, 0.0];
const Y: [f64; 3] = [0.0, 1.0, 0.0];
const Z: [f64; 3] = [0.0, 0.0, 1.0];
const DOWN: [f64; 3] = [0.0, 0.0, -1.0];

/// The handles of the selection: a single-line feature's dimensions and
/// datums at the places they mean (a diameter's grip on the rim at +X, an
/// extent's at the bottom, a height's at the top; a centre's coordinates
/// where the feature's own drawn edges reach farthest along the NEGATIVE
/// of each axis — `x` on the rim at −X, `y` at −Y, `z` at the bottom —
/// so no two of a feature's grips share a world point, and the rim at +X
/// stays the diameters'); an operation's `stepover` and `stock_to_leave` on its
/// floor, along the inward normal of its first wall edge — the tool's
/// radius plus the stepover in from the wall, and the allowance in from
/// the wall. The rule (Phase 43 slice 2): **a grip is a key with a LINEAR
/// axis.** An angle (`rotation`, `tilt`, `heading`) is a turn about a
/// point, a second drag arithmetic, and has no grip until it has a
/// design; a length whose circle has no place yet (a neck, a tip, a
/// mouth) waits likewise, named in its arm. Block statements (a
/// profile's points) have no handles, as the form has no fields for
/// them. The match is exhaustive so the next noun arrives with its arm
/// written, not swallowed by a wildcard.
pub fn handles(p: &Part, sel: Selection, rows: &[FindingRow], edges: &[FeatureEdges]) -> Vec<Handle> {
    let mut out = Vec::new();
    if let Some(wi) = selected_feature(sel, rows, edges) {
        let name = &edges[wi].name;
        if let (Some(f), Some((_, _, _, line))) = (p.features.get(name), p.feature_lines.iter().find(|(n, ..)| n == name)) {
            let st = Statement::Feature(name.clone());
            let top = p.stock.z;
            let push = |out: &mut Vec<Handle>, key: &str, at: [f64; 3], axis: [f64; 3], per_mm: f64, min: f64| {
                if let Some(h) = handle(line, &st, key, at, axis, per_mm, min) {
                    out.push(h);
                }
            };
            // A centre's coordinates (Phase 43 slice 2): three keys mean one
            // point, and three grips on one point left two ungrabbable — so
            // each grip sits where the feature's drawn edges reach farthest
            // along the NEGATIVE of its axis (`x` on the rim at −X, `y` at
            // −Y, `z` at the bottom), the centre's own height kept for `x`
            // and `y`; a feature with no edges keeps them at the centre.
            // A world place, as every grip's is: a screen standoff was
            // tried first and met the rim grip at +X whenever the radius
            // imaged to the standoff's length.
            let reach = {
                let inf = f64::INFINITY;
                let bb = edges_aabb(std::slice::from_ref(&edges[wi]), [inf, inf, inf, -inf, -inf, -inf]);
                if bb[0].is_finite() { Some(bb) } else { None }
            };
            let centre = |out: &mut Vec<Handle>, c: [f64; 3], with_z: bool| {
                let (x0, y0, z0) = reach.map_or((c[0], c[1], c[2]), |bb| (bb[0].min(c[0]), bb[1].min(c[1]), bb[2].min(c[2])));
                push(out, "x", [x0, c[1], c[2]], X, 1.0, f64::NEG_INFINITY);
                push(out, "y", [c[0], y0, c[2]], Y, 1.0, f64::NEG_INFINITY);
                if with_z {
                    push(out, "z", [c[0], c[1], z0], Z, 1.0, f64::NEG_INFINITY);
                }
            };
            // A diameter's grip on the rim at +X, two of value per one of grip.
            let rim = |x: f64, y: f64, z: f64, dia: f64| [x + dia / 2.0, y, z];
            match f {
                Feature::Plane { z } => push(&mut out, "z", [p.stock.x + p.stock.w / 2.0, p.stock.y + p.stock.l / 2.0, *z], Z, 1.0, f64::NEG_INFINITY),
                // A block's opener keys (Phase 50 slice 6): the extent at the
                // first vertex's floor pointing down, the rim's `z` when the
                // line carries it — the bore's rule; the points stay the
                // file's. A boss's rim is its `z` too (`prismatic_boss_z_span`).
                Feature::ProfilePocket { points, z, extent, .. } | Feature::ProfileBoss { points, z, extent, .. } => {
                    if let Some(p0) = points.first() {
                        let rim = derive_contour_top(top, *z);
                        push(&mut out, "extent", [p0.x, p0.y, derive_z_cut(rim, *extent)], DOWN, 1.0, 0.001);
                        push(&mut out, "z", [p0.x, p0.y, rim], Z, 1.0, f64::NEG_INFINITY);
                    }
                }
                Feature::Face { z, loops } => {
                    if let Some(p0) = loops.first().and_then(|l| l.first()) {
                        push(&mut out, "z", [p0.x, p0.y, *z], Z, 1.0, f64::NEG_INFINITY);
                    }
                }
                // A wire's points carry their own heights: no grip yet, by name.
                Feature::ContourPocket { .. } | Feature::ContourBoss { .. } => {}
                // Sunk from its RIM (Phase 45 slice 2 — the stock top unless
                // the line says `z`): the diameter's grip on the rim, the
                // extent at the bottom pointing down, and the centre's three
                // — `z` among them, a grip only when the line carries the
                // key, as every grip is.
                Feature::CylinderBore { dia, extent, x, y, z, .. } => {
                    let top = derive_contour_top(top, *z);
                    push(&mut out, "dia", rim(*x, *y, top, *dia), X, 2.0, 0.001);
                    push(&mut out, "extent", [x + dia / 2.0, *y, derive_z_cut(top, *extent)], DOWN, 1.0, 0.001);
                    centre(&mut out, [*x, *y, top], true);
                }
                // `tilt` and `heading` are angles: no grip (the rule above).
                Feature::TiltedBore { dia, extent, x, y, .. } => {
                    push(&mut out, "dia", rim(*x, *y, top, *dia), X, 2.0, 0.001);
                    push(&mut out, "extent", [x + dia / 2.0, *y, derive_z_cut(top, *extent)], DOWN, 1.0, 0.001);
                    centre(&mut out, [*x, *y, top], false);
                }
                Feature::SplineRevolveFunnel { x, y, .. } => centre(&mut out, [*x, *y, top], false),
                // Standing on a datum `z`: the rim at the base, what grows
                // up at the top pointing up.
                Feature::CylinderBoss { dia, extent, x, y, z, .. } => {
                    push(&mut out, "dia", rim(*x, *y, *z, *dia), X, 2.0, 0.001);
                    push(&mut out, "extent", [x + dia / 2.0, *y, z + extent], Z, 1.0, 0.001);
                    centre(&mut out, [*x, *y, *z], true);
                }
                Feature::Sphere { dia, x, y, z } => {
                    push(&mut out, "dia", rim(*x, *y, *z, *dia), X, 2.0, 0.001);
                    centre(&mut out, [*x, *y, *z], true);
                }
                Feature::Cone { dia, height, x, y, z } => {
                    push(&mut out, "dia", rim(*x, *y, *z, *dia), X, 2.0, 0.001);
                    push(&mut out, "height", [*x, *y, z + height], Z, 1.0, 0.001);
                    centre(&mut out, [*x, *y, *z], true);
                }
                Feature::Torus { major_dia, minor_dia, x, y, z } => {
                    push(&mut out, "major_dia", rim(*x, *y, *z, *major_dia), X, 2.0, 0.001);
                    push(&mut out, "minor_dia", [x + (major_dia + minor_dia) / 2.0, *y, *z], X, 2.0, 0.001);
                    centre(&mut out, [*x, *y, *z], true);
                }
                // Placed by a file: the centre is the only number the line
                // carries.
                Feature::Mesh { x, y, z, .. } | Feature::SplineRevolve { x, y, z, .. } | Feature::NurbsPatch { x, y, z, .. } => centre(&mut out, [*x, *y, *z], true),
            }
        }
    }
    if let Some(i) = selected_op(sel, rows) {
        if let (Some(op), Some(line), Some(tool)) = (p.operations.get(i), p.op_texts.get(i), p.operations.get(i).and_then(|o| p.tools.get(&o.tool_num))) {
            if let Some(region) = crate::region::of(p, op) {
                let n = region.outer.len();
                if n >= 2 {
                    // The first wall edge, else the first edge: its midpoint
                    // and inward normal (the outer boundary is CCW, so the
                    // interior is to the left of every edge).
                    let i0 = (0..n).find(|&k| region.edges[k] == crate::region::EdgeKind::Wall).unwrap_or(0);
                    let (a, b) = (region.outer[i0], region.outer[(i0 + 1) % n]);
                    let (dx, dy) = (b.x - a.x, b.y - a.y);
                    let len = dx.phypot(dy).max(1e-9);
                    let inward = [-dy / len, dx / len, 0.0];
                    let mid = [(a.x + b.x) / 2.0, (a.y + b.y) / 2.0, region.z];
                    let st = Statement::Op(i);
                    let along = |d: f64| [mid[0] + inward[0] * d, mid[1] + inward[1] * d, region.z];
                    if let Some(h) = handle(line, &st, "stock_to_leave", [0.0; 3], inward, 1.0, 0.0) {
                        out.push(Handle { at: along(h.value), ..h });
                    }
                    if let Some(h) = handle(line, &st, "stepover", [0.0; 3], inward, 1.0, stepover_floor(tool.dia)) {
                        let skin = numeric(line, "stock_to_leave").unwrap_or(0.0);
                        // The ceiling is `validate`'s: a stepover at or over
                        // the diameter is refused, so the grip stops a hair
                        // under it and says why. The floor is the grip's
                        // own (`stepover_floor`), said the same way.
                        let stop = format!("stepover stops under T{}'s diameter ({}mm)", op.tool_num, handle_text(tool.dia));
                        let floor_stop = format!(
                            "stepover stops at a twentieth of T{}'s diameter ({}mm) — a finer one takes minutes to plan; type it if you mean it",
                            op.tool_num,
                            handle_text(h.min)
                        );
                        out.push(Handle { at: along(tool.dia / 2.0 + skin + h.value), max: tool.dia - 0.001, stop: Some(stop), floor_stop: Some(floor_stop), ..h });
                    }
                }
            }
        }
    }
    out
}

/// The grip under a screen point (from the viewport's centre, +y up),
/// nearest within `tol` — the pick rule `pick_feature` uses, for grips.
pub fn pick_handle(handles: &[Handle], cam: &Camera, sx: f64, sy: f64, tol: f64) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, h) in handles.iter().enumerate() {
        let Some((x, y)) = handle_view_point(h, cam) else { continue };
        let d = (x - sx).phypot(y - sy);
        if d <= tol && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

/// The value a drag drafts: the pointer's travel from `from` to `to`
/// (screen, +y up) projected onto the grip's axis as the camera draws it,
/// in mm along the axis, times `per_mm`, from the file's value, never
/// under `min`. `None` when the axis looks at the viewer (its screen
/// image is under two pixels long) — a drag along nothing drafts nothing.
pub fn handle_drag(h: &Handle, cam: &Camera, from: (f64, f64), to: (f64, f64)) -> Option<f64> {
    let lens = cam.lens();
    let (x0, y0, _) = lens.project(h.at);
    let (x1, y1, _) = lens.project([h.at[0] + h.axis[0], h.at[1] + h.axis[1], h.at[2] + h.axis[2]]);
    let (ax, ay) = (f64::from(x1 - x0), f64::from(y1 - y0));
    let l2 = ax * ax + ay * ay;
    if l2 < 4.0 {
        return None;
    }
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let mm = (dx * ax + dy * ay) / l2;
    Some((h.value + mm * h.per_mm).max(h.min).min(h.max))
}

/// A drafted value as the line will carry it: three decimals, trailing
/// zeros dropped — `12.5`, `4`, `0.25` — the way a hand types one.
pub fn handle_text(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" { "0".to_string() } else { s.to_string() }
}

/// The edges' bounding box folded over `bb` — the fit must include a feature
/// nothing machines yet (a big unrun dome would otherwise crop at first
/// frame).
pub fn edges_aabb(edges: &[FeatureEdges], mut bb: [f64; 6]) -> [f64; 6] {
    for w in edges {
        for pl in &w.polylines {
            for p in pl {
                bb[0] = bb[0].min(p[0]);
                bb[1] = bb[1].min(p[1]);
                bb[2] = bb[2].min(p[2]);
                bb[3] = bb[3].max(p[0]);
                bb[4] = bb[4].max(p[1]);
                bb[5] = bb[5].max(p[2]);
            }
        }
    }
    bb
}

// ---- the glass: the surface spike's pure rules (Phase 25 slice 1) --------
//
// A transparent window is a favour the swapchain grants or refuses, and
// the shipped stack's DX12 surface refuses it (wgpu-hal's own table: a
// plain HWND surface offers `[Opaque]`). Slice 1 asks for the window
// anyway, looks at what every adapter's surface OFFERS, prefers one that
// can be see-through, and says the whole list once on stderr — the
// measurement the DRAFT named, in the user's own launch. The rule that
// picks is here, on plain data, so it is pinned without a GPU.

/// One adapter as the spike sees it: which backend and device, and the
/// composite-alpha modes its surface offered (empty when the surface and
/// the adapter are strangers, which is wgpu's own answer for that).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceOffer {
    pub backend: String,
    pub name: String,
    /// What kind of device — the ranking the default selection (high
    /// performance) uses, kept so the spike changes nothing on a machine
    /// whose surfaces are all opaque. Measured the day it was written:
    /// under WSL the Vulkan adapter is llvmpipe (a CPU) and the hardware
    /// is the GL one, so "discrete first, else first" would have picked
    /// software rendering. The rank is wgpu's own order.
    pub kind: DeviceKind,
    pub alpha_modes: Vec<String>,
}

/// wgpu's device types, in the order a high-performance request prefers
/// them (`request_adapter`'s own ranking): discrete, integrated, other,
/// virtual, then a CPU rasterizer last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    Discrete,
    Integrated,
    Other,
    Virtual,
    Cpu,
}

impl DeviceKind {
    fn rank(self) -> u8 {
        match self {
            DeviceKind::Discrete => 0,
            DeviceKind::Integrated => 1,
            DeviceKind::Other => 2,
            DeviceKind::Virtual => 3,
            DeviceKind::Cpu => 4,
        }
    }
}

impl SurfaceOffer {
    /// Whether the surface can be see-through on this adapter: a
    /// pre- or post-multiplied composite mode is on offer.
    pub fn transparent(&self) -> bool {
        self.alpha_modes.iter().any(|m| m == "PreMultiplied" || m == "PostMultiplied")
    }
}

/// Which adapter to use. Wanting transparency, the best-ranked adapter
/// whose surface offers it; otherwise — or when none offers it — the
/// default's own preference, the best-ranked device, the first on a tie.
/// `None` only for an empty list.
pub fn pick_adapter(offers: &[SurfaceOffer], want_transparent: bool) -> Option<usize> {
    let candidates: Vec<usize> = if want_transparent && offers.iter().any(SurfaceOffer::transparent) {
        (0..offers.len()).filter(|&i| offers[i].transparent()).collect()
    } else {
        (0..offers.len()).collect()
    };
    // `min_by_key` keeps the FIRST of equal keys — the tie rule.
    candidates.iter().copied().min_by_key(|&i| offers[i].kind.rank())
}

/// The spike's one sentence: every adapter, what its surface offered, and
/// which was chosen — the fact the DRAFT said the shipped stack cannot
/// state, stated from the machine that matters.
pub fn surface_sentence(offers: &[SurfaceOffer], chosen: Option<usize>, want_transparent: bool) -> String {
    if offers.is_empty() {
        return String::from("Transparency: no adapter offered a surface");
    }
    let parts: Vec<String> = offers
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let modes = if o.alpha_modes.is_empty() { String::from("nothing") } else { format!("[{}]", o.alpha_modes.join(", ")) };
            let mark = if Some(i) == chosen { " — chosen" } else { "" };
            format!("{} ({}) offers {}{}", o.name, o.backend, modes, mark)
        })
        .collect();
    let verdict = match chosen {
        _ if !want_transparent => "transparency not asked for on this platform",
        Some(i) if offers[i].transparent() => "the window can be see-through",
        _ => "no surface offers transparency: the window is opaque, glass worn over black",
    };
    format!("Transparency: {}; {}", parts.join("; "), verdict)
}

/// What became of a theme's `blurred` (Phase 25 slice 3, fork 5): the
/// platform was not asked (the theme did not say blurred, or the surface
/// was refused, so there is no glass to blur), has no blur to give
/// (Linux: the glass shows the desk as it is), applied it (Windows 11's
/// acrylic backdrop), or refused it with the HRESULT in words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Blur {
    NotAsked,
    NotOnThisPlatform,
    Applied,
    Refused(String),
}

/// The blur's one stderr sentence at open.
pub fn blur_sentence(b: &Blur) -> String {
    match b {
        Blur::NotAsked => String::from("Blur: not asked for"),
        Blur::NotOnThisPlatform => String::from("Blur: not on this platform — the glass shows the desk unblurred"),
        Blur::Applied => String::from("Blur: acrylic applied (Windows 11 system backdrop)"),
        Blur::Refused(why) => format!("Blur: refused by the platform — {why}"),
    }
}

/// The status line's word on the glass (Phase 25, fork 6): only a theme
/// that asked for a see-through surface gets a sentence, and only once
/// the surface has answered. Granted glass says nothing — the picture
/// says it; refused glass names the fact and what it means; glass that
/// asked for a blur and got none says which of the two it got.
pub fn glass_note(asked: crate::theme::BackgroundAppearance, granted: Option<bool>, blur: &Blur) -> Option<String> {
    use crate::theme::BackgroundAppearance::*;
    match (asked, granted) {
        (Opaque, _) | (_, None) => None,
        (Transparent, Some(false)) | (Blurred, Some(false)) => {
            Some(String::from("glass over black: the surface offers no transparency, so the theme's fills are worn over black"))
        }
        (Transparent, Some(true)) => None,
        (Blurred, Some(true)) => match blur {
            Blur::Applied | Blur::NotAsked => None,
            Blur::NotOnThisPlatform => Some(String::from("glass without blur: not on this platform")),
            Blur::Refused(why) => Some(format!("glass without blur: the platform refused it — {why}")),
        },
    }
}
