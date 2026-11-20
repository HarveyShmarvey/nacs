# Master Taxonomy of Math-Definable Shapes by CAM Cuttability

> **Scope note (2026-07).** This is the doc set's first *reference* document —
> a map of shape machinability, not a plan and not a commitment. Multi-axis
> stays off the roadmap (`DECISIONS.md` → "Feature geometry is three camps…";
> `ROADMAP.md` → "Not on the roadmap"). For this engine the actionable columns
> are **Regime** and **3-Axis**; the 4-Axis / 5-Axis columns and §6
> (process-defined families) are context — good for explaining *why* a feature
> is unmachinable in an error message, never a to-do list. The regime axis
> (R1/R2/R3) is adopted into the camps model — camp 2 splits into 2a (R1) and
> 2b (R2) — see `DECISIONS.md` → "Implementation regimes (R1/R2/R3) refine the
> three camps". Row-by-row implementation status lives in `CHECKLIST.md` (same
> row order and IDs); never mark status in this file — it stays the normative
> reference.

A unified reference for machinable primitives, organized by the two variables that actually
predict how a shape cuts — now tagged with the **implementation regime** so it doubles as a
scoping spec for a code agent. Every "impossible" cell traces back to reachability, not to the
primitive's name; every "needs ray-casting" cell traces back to the math regime, not to the
shape family.

## How to read this

**Three physics/geometry variables plus one implementation variable govern every row.**

**1 · Curvature class** predicts the *right tool geometry and toolpath strategy*.
- **Planar (K = 0, flat)** → face/planar milling.
- **Developable (K = 0, singly-ruled)** → *flank/swarf is geometrically exact*. A straight
  (cylindrical/conical) tool contacts along a full ruling with zero theoretical error.
- **Synclastic (K > 0, dome/bowl)** → *ball-nose scallop*. No ruling to exploit; cusp height
  set by stepover.
- **Anticlastic (K < 0, saddle)** → ruled ones (saddle, 1-sheet hyperboloid, helicoid) *can* be
  swarf-cut, but being **non-developable** a cylindrical tool leaves inherent error; wants a
  conical tool and/or continuous tilt. Non-ruled regions fall back to scallop.
- **Variable K** → strategy switches across the surface (steep/shallow, rest-machining).

**2 · Re-entrancy to the tool axis** (the "Undercut?" column) is the *sole* determinant of
reachability. It decides whether a straight tool reaches the surface at all, or whether you need
a form cutter, a lollipop/undercut tool, or additional machine axes. **Curvature tells you the
strategy; re-entrancy tells you the tool and the axis count.**

**3 · Implementation regime** (the "Regime" column) predicts *how much math machinery cutter
contact/location actually costs to compute* — this is the axis your IDE agent cares about, and it
does **not** line up with the shape families.
- **R1 — closed-form, self-offsetting.** The five natural quadrics: plane, sphere, circular
  cylinder, circular cone, torus. Their offset surface is *the same species* (offset a sphere →
  sphere, a cone → coaxial cone same half-angle, a torus → torus with fatter tube), so contact
  points, CL points, and gouge checks are pure algebra with **zero surface sampling**.
- **R2 — analytic per point, numeric per path.** Parametric surface with a closed-form normal
  (general ellipsoid, paraboloid, both hyperboloids, saddle, superquadrics, analytic surfaces of
  revolution, ruled surfaces, extrusions of analytic curves). A *single* contact/CL point is
  exact — no ray-casting. But the offset is **not** elementary, so any *global* op (constant-
  scallop spacing, offset self-intersection, silhouette/waterline, swarf-error minimization for
  the non-developable ruled members) drops to Newton / marching / resultants. Iterative, still
  not ray-casting.
- **R3 — discretized / ray-cast floor.** Either **implicit with no parametrization** (TPMS is
  *definitionally* field-eval + root-find along a ray — there is no surface point to offset), or
  **global-query-dominated** (production NURBS, rest/pencil, variable-radius canal blends,
  accessibility/collision, enclosed channels). Industry uses z-maps, dexels, point clouds, and
  distance fields here because analytic is intractable, not merely inconvenient.

**⚠ (gouge flag)** on a regime tag = the analytic offset **self-intersects on concave regions**
when tool radius > local concave radius of curvature. The closed-form CL point then gouges
*silently* — no crash. Needs an explicit curvature test + trim. Hits concave/inside features even
on otherwise-clean R1 primitives.

**The one hard 3-axis limit:** no 3-axis strategy produces a *sharp* internal vertical corner —
you always inherit the tool radius. This quietly governs half of every "inside cavity" row.

**Axis regimes:** 3-axis = XYZ. **4-axis = XYZ + one rotary**, used either *indexed* (3+1
positioning to present a new face) or *continuous* (wrap / rotary-synced with Z). 5-axis = XYZ +
two rotaries, indexed (3+2) or simultaneous. 4-axis is the natural — not fallback — home for
surfaces of revolution and helical parts.

**Strategy legend:** Flank/Swarf · Scallop (constant-cusp) · Z-Level/Waterline · Planar/Face ·
Contour (2D profile) · Pencil/Rest (leftover) · Form Cutter (profile ground into tool) ·
Lollipop/Undercut (disc/ball-on-stem) · Thread-Mill (helical interp).

> **Tool-shape caveat on every "closed-form" claim:** R1/R2 exactness assumes a **ball tool**
> (contact is always the center-to-surface line). Flat, bull-nose (toroidal), and tapered tools
> make the contact point *migrate* around the tool tip with local slope — turning even a natural
> quadric into a small root-find for *where* on the tool edge contact occurs. Tell the agent its
> target tool class before it commits to an approach.

---

## 1 · Planar (K = 0, flat)

| Family | Shape Primitive | Feature / Orientation | Undercut? | Regime | 3-Axis | 4-Axis | 5-Axis |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| Planar | Flat face / pocket floor / step | Top-facing | No | R1 | Planar / Face | Index to reach other faces (3+1) | 3+2 to any orientation |
| Planar | Angled / draft face | Oblique | No | R1 | Planar (3-axis leaves scallop on slope) | Index normal-to-face (3+1) | 3+2 planar (flat contact) |

---

## 2 · Developable (K = 0, singly-ruled) — **flank/swarf is exact**

Straight walls, tapers, and single-curvature shells. A cylindrical tool flanks the circular
cases perfectly; a matched conical tool (or a tilt) flanks the tapered cases perfectly.
Circular cylinder/cone are R1; conic-section and general-contour extrusions are R2 (analytic
point, non-elementary offset).

| Family | Shape Primitive | Feature / Orientation | Undercut? | Regime | 3-Axis | 4-Axis | 5-Axis |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| Prismatic | Rectangular prism | Inside cavity | No | R1 ⚠ | Flank walls + Planar floor | 3+1 for opposing walls (little gain) | Flank (swarf) + Planar |
| Prismatic | Rectangular prism | Outside boss | No | R1 | Flank | Index faces (3+1) | Flank; scallop corner-blend |
| Prismatic | Cylinder (circular) | Inside bore | No | R1 ⚠ | Circular Flank / helical bore | Boring if part revolves | Flank |
| Prismatic | Cylinder (circular) | Outside boss | No | R1 | Flank | Turn-mill if revolved | Flank (swarf contour) |
| Prismatic | Conic-section cylinder (elliptic/parabolic/hyperbolic) | Inside/Outside | No | R2 ⚠ | Contour Flank + Planar floor | Wrap features onto face | Flank (swarf) |
| Prismatic | Regular polygonal prism (hex, pentagon…) | Outside boss | No | R1 | Flank | Index per facet (3+1) | Flank |
| Prismatic | General extruded profile (arbitrary closed contour) | Inside cavity | No | R2 ⚠ | 2D Contour + Pocket — *tool radius limits internal concave corners* | 3+1 access | 3+2 planar; Flank |
| Prismatic | General extruded profile | Outside island/boss | No | R2 | Contour Flank | Index faces | Flank (swarf) |
| Prismatic | Wedge / ramp | Inside/Outside | No | R1 | Scallop / Raster (or Planar per face) | Index to face the ramp (3+1) | Planar (3+2); Flank if heel clears |
| Prismatic | **Dovetail groove** | Inside undercut slot | **Yes** | R1 (form) | **Dovetail Form Cutter — still 3-axis** | 3+1 to angle the slot | Tilted endmill (3+2) or lollipop |
| Prismatic | **T-slot** | Inside undercut slot | **Yes** | R1 (form) | **T-Slot / Woodruff Cutter — 3-axis** | 3+1 to angle | 3+2 with side cutter |
| Ruled/Tapered | Cone / frustum (base-down) | Outside boss | No | R1 | Scallop / Z-Level (or Flank w/ tapered tool) | Rotary contour if revolved | Flank (swarf) |
| Ruled/Tapered | Cone / frustum (base-down) | Inside cavity | **Yes** (walls overhang) | R1 ⚠ | *Undercut → Lollipop* | 3+1 if draft clears the shank | Flank (swarf undercut), Scallop |
| Ruled/Tapered | Cone / frustum (point-down) | Inside cavity | No | R1 ⚠ | Scallop / Z-Level | Turn-mill if revolved | Flank (swarf), Scallop |
| Ruled/Tapered | Cone / frustum (point-down) | Outside boss | **Yes** (base overhangs) | R1 | *Undercut → Lollipop* | 3+1 to clear | Flank (swarf undercut), Scallop |
| Ruled/Tapered | Pyramid / frustum (base-down) | Inside cavity | **Yes** | R1 ⚠ | *Undercut → Lollipop* | 3+1 per face | Flank (swarf undercut), Scallop |
| Ruled/Tapered | Pyramid / frustum (base-down) | Outside boss | No | R1 | Scallop / Z-Level | Index per face | Flank (swarf), Scallop |
| Ruled/Tapered | Pyramid / frustum (point-down) | Inside cavity | No | R1 ⚠ | Scallop / Z-Level | Index per face | Flank (swarf), Scallop |
| Ruled/Tapered | Pyramid / frustum (point-down) | Outside boss | **Yes** | R1 | *Undercut → Lollipop* | 3+1 to clear | Flank (swarf undercut), Scallop |
| Ruled/Tapered | Oblique cone / cylinder (tilted axis) | Outside boss | Maybe | R2 | Scallop / Z-Level | Rotary if axis is rotary | Flank (single tilt) |
| Ruled/Tapered | Tangent developable (of a space curve) | Outside | No | R2 | Scallop | Rotary contour | **Flank — developable → zero-error swarf** |

---

## 3 · Synclastic (K > 0, dome / bowl) — **ball-nose scallop**

Doubly-curved, same-sign. No ruling to swarf; cusp height is a stepover trade-off. Tilting on
5-axis is about maintaining surface speed / avoiding the dead center of a ball tool, not about
exactness. Sphere is R1; every other member is R2 (analytic normal, non-elementary offset).
Convex/outside faces don't self-intersect on outward offset (no ⚠); concave/inside bowls do.

| Family | Shape Primitive | Feature / Orientation | Undercut? | Regime | 3-Axis | 4-Axis | 5-Axis |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| Quadric | Hemisphere (lower / bowl) | Inside top-open | No | R1 ⚠ | Scallop | Rotary-wrap scallop; turn if revolved | Scallop (tilted for optimal SFM) |
| Quadric | Hemisphere (upper / dome) | Outside boss | No | R1 | Scallop | Rotary-wrap scallop | Scallop (tilted for SFM) |
| Quadric | **Full sphere** | Standalone (dome + sub-equator undercut) | **Yes below equator** | R1 (⚠ inside) | Dome only; *underside → flip / Lollipop* | **Flip or continuous rotary — natural home** | Simultaneous or 3+2 both hemispheres |
| Quadric | Hemisphere (upper / widening) | Inside top-open undercut | **Yes** | R1 | *Impossible → Lollipop* | 3+1 into undercut if shallow | Scallop (simultaneous or 3+2) |
| Quadric | Hemisphere (lower / narrows to base) | Outside protruding undercut | **Yes** | R1 | *Impossible → Lollipop* | 3+1 to clear | Scallop (simultaneous) |
| Quadric | Tri-axial ellipsoid (dome) | Outside boss | No | R2 | Scallop (no symmetry to exploit) | Rotary-wrap if near-revolved | Scallop (normal-tracking) |
| Quadric | **Full tri-axial ellipsoid** | Standalone (dome + sub-equator undercut) | **Yes below equator** | R2 | Dome only; *underside → flip / Lollipop* | Flip / rotary to reach underside | Simultaneous both halves |
| Quadric | Tri-axial ellipsoid (below equator) | Outside undercut | **Yes** | R2 | *Impossible → Lollipop* | 3+1 to clear | Scallop (simultaneous) |
| Quadric | Elliptic paraboloid (bowl) | Inside top-open | No | R2 ⚠ | Scallop | Rotary-wrap if revolved | Scallop (surface normal) |
| Quadric | Elliptic paraboloid (dome) | Outside boss | No | R2 | Scallop | Rotary-wrap if revolved | Scallop (surface normal) |
| Quadric | Hyperboloid of two sheets (single sheet: bowl / dome) | Inside/Outside | No | R2 (⚠ inside) | Scallop (synclastic near apex) | Rotary-wrap if revolved | Scallop |
| Quadric | Torus — upper convex half | Outside boss | No | R1 | Scallop | Rotary-wrap | Scallop |
| Quadric | **Full torus** | Standalone surface of revolution | **Yes (inner + underside)** | R1 (⚠ inner) | Outer only; *inner/under → Woodruff / flip* | **Continuous rotary wrap — natural home** | Simultaneous all quadrants |
| Blend | Convex round (external edge) | Outside | No | R2 | Ball Scallop | Rotary if on a revolved edge | Tilted scallop |

---

## 4 · Anticlastic (K < 0, saddle) — **ruled ⇒ swarf-with-error; else scallop**

Opposite-sign curvature. The ruled members are doubly-ruled but **non-developable**: a straight
cylindrical tool flank-cutting them leaves a theoretical gouge/undercut error, so they want a
conical tool and/or continuous 5-axis tilt to track a ruling. This is the key distinction from
the developable cones/cylinders above. All R2 (analytic normal) but with heavier global numerics
for the swarf-error minimization.

| Family | Shape Primitive | Feature / Orientation | Undercut? | Regime | 3-Axis | 4-Axis | 5-Axis |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| Quadric | **Hyperbolic paraboloid (saddle)** | Inside/Outside anticlastic | No | R2 ⚠ | Scallop / Raster | Limited | **Flank (swarf either ruling family)** — non-developable ⇒ residual error, conical tool/tilt |
| Quadric | Hyperboloid of one sheet (upper half, flaring up) | Outside boss | No | R2 | Scallop / Z-Level | Rotary-wrap if revolved | Flank (swarf match), Scallop |
| Quadric | Hyperboloid of one sheet (lower half, flaring down) | Outside undercut | **Yes** | R2 | *Impossible → Lollipop* | 3+1 to clear | Flank (swarf match), Scallop |
| Quadric | **Full one-sheet hyperboloid** (single connected surface; cooling-tower) | Standalone (waisted boss) | **Yes below waist** | R2 | Above-waist only; *below-waist → Lollipop* | Continuous rotary wrap | Flank (swarf) both flares |
| Quadric | Torus — inner concave (C-groove) | Inside radial cavity | **Yes** | R1 ⚠ | *Impossible → Woodruff / Lollipop* | 4-axis wrap w/ side or lollipop cutter | Scallop (simultaneous) |
| Blend | Concave fillet region (steep) | Inside transition | Partial | R2 ⚠ | Pencil / Rest / constant-cusp — *undercuts on steep walls* | 3+1 for reach | Lead/tilt for reach |

---

## 5 · Variable / Mixed K — **strategy switches across the surface**

Curvature sign changes over the body, so these blend scallop, waterline, and rest-machining and
lean on steep/shallow separation. Analytic-profile members are R2; general freeform and
distance-field-driven blends are R3.

| Family | Shape Primitive | Feature / Orientation | Undercut? | Regime | 3-Axis | 4-Axis | 5-Axis |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| Revolution | Ogive / tangent-ogive (nose cone) | Outside boss | No | R2 | Scallop / Z-Level | **Continuous wrap — natural home** | Scallop (surface normal) |
| Revolution | Vase / goblet / necked profile | Outside | **Yes at re-entrant neck** | R2 | Scallop above; *neck → Lollipop* | Index / flip (3+1) to reach neck | Simultaneous |
| Revolution | Catenoid / pseudosphere / spline-revolved | Inside/Outside | Maybe | R2 (R3 if spline) | Scallop | **Continuous wrap turn-mill** | Scallop / Flank |
| Blend | Variable-radius / canal / pipe (ball rolling on a spine) | Inside/Outside | Maybe | R3 | Scallop, projection | Wrap if spine is planar-rotary | Simultaneous morph |
| Blend | Setback / corner (3-face) blend | Inside | Partial | R3 | Rest / Pencil | 3+1 reach | 5-axis reach |
| Freeform | General NURBS / Bézier / B-spline patch (synclastic) | Outside | No | R3 | Scallop, parallel, radial, spiral, waterline, flowline, projection, steep/shallow | 3+1 to improve access | Simultaneous lead/tilt, morph |
| Freeform | General NURBS patch (undercut / re-entrant) | Inside | **Yes** | R3 | *Impossible → Lollipop / barrel* | 3+1 partial | 5-axis simultaneous |
| Superquadric | Superellipsoid / squircle-prism / rounded cuboid (Lamé) | Outside | No | R2 | Flank sides + Scallop caps | Index faces | Blended simultaneous |

---

## 6 · Process-Defined Families (cross-cut curvature)

Here the *manufacturing method / tool* defines the family more usefully than a single curvature
label. Regime reflects **toolpath-generation** math; note that *form design* (cutting the profile
into the tool) is a separate offline inverse problem, analytic for threads/involutes, numeric for
screw-rotor/bevel meshing.

| Family | Shape Primitive | Feature / Orientation | Undercut? | Regime | 3-Axis | 4-Axis | 5-Axis |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| Helical | Thread (ID / OD) | Inside/Outside | Self-clearing | R1 (helical path) | **Thread-Mill (helical interp)** or single-point turning | Rotary-synced single-point (turning) | 3+2 for angled thread |
| Helical | Helicoid / auger / screw conveyor | Outside | No | R2 | Form cutter + rotary; ruled scallop | **Continuous rotary + Z sync — natural home** | Flank (swarf along flute) |
| Helical | Worm / screw-rotor flank (compressor) | Outside | No | R2/R3 (envelope) | *Impractical (form-limited)* | Rotary + Z (hobbing analog) | **Flank (swarf) — canonical 5-axis case** |
| Helical | Twist-drill flute | Outside | No | R2 | Form / relief grind | Rotary + Z flute grind | 5-axis flute grind |
| Gear/Involute | Spur gear (involute-of-circle, extruded) | Outside | No | R2 | **Involute Form Cutter / gash per tooth (2.5D)** | Rotary index per tooth (gashing / hob analog) | InvoMilling / point-mill |
| Gear/Involute | Helical gear (involute + helix) | Outside | No | R2 | Form cutter + rotary | **Continuous rotary + Z (hobbing / wrap)** | 5-axis flank / point |
| Gear/Involute | Bevel / spiral-bevel (spherical involute) | Outside | No | R3 | *Impractical* | Rotary + form (limited) | 5-axis simultaneous |
| Gear/Involute | Cycloidal / trochoidal tooth | Outside | No | R2 | Form cutter | Rotary index per tooth | 5-axis |
| Freeform | Impeller / blade channel (blisk) | Inside channel | **Yes (access-limited)** | R3 | *Impractical* | 4-axis for open / low-wrap impellers | 5-axis flank + point (blade cycles) |
| Minimal/TPMS | Gyroid / Schwarz-P / Diamond (implicit) | Enclosed lattice | **Yes (self-shadowing)** | R3 (implicit) | *Impossible — additive / hybrid only* | n/a | Surface-exposed portions only |

---

## Cross-cutting takeaways

- **The regime column does NOT track the shape families.** A humble elliptic paraboloid bowl (R2)
  is harder to compute than a torus (R1), because "self-offsetting" is a property of the *algebra*,
  not of how curvy the thing looks. The natural-quadric boundary — plane, sphere, circular
  cylinder, circular cone, torus — is the one line where pure algebra ends.

- **Completing a surface changes access, never regime.** A full sphere is R1 at *every* point
  including under the equator; what flips at the equator is reachability, not analyticity. So
  "whole vs. half" is a real machining distinction (adds a sub-equatorial undercut → flip / rotary
  / lollipop) but a *non-event* for the contact math. Same for the full ellipsoid, full torus, and
  the single-connected full one-sheet hyperboloid. The full quadric *cone* and *two-sheet
  hyperboloid* are two nappes/sheets each — you were only ever shown one — but the pair adds no
  new strategy or regime.

- **Developable vs. non-developable is the sharpest geometry line.** Cones, cylinders,
  tangent-developables, and extruded walls flank-cut with zero theoretical error. The saddle, the
  one-sheet hyperboloid, and the helicoid *look* similar (all ruled) but are non-developable, so
  cylindrical-tool swarf leaves error — reach for a conical tool or continuous tilt.

- **Every "*Impossible*" cell is re-entrancy, not curvature.** A bowl and an upper-hemisphere
  undercut have identical curvature; only the draft-angle sign relative to the tool axis differs.
  Undercuts are solved by the *tool* (form/lollipop/Woodruff) or by *adding an axis*.

- **⚠ is the silent-failure risk, not the crash risk.** When tool radius exceeds local concave
  radius, the analytic offset self-intersects and the closed-form CL point gouges without
  complaint. Every concave/inside feature needs an explicit curvature test + trim — including on
  clean R1 primitives.

- **4-axis is the missing middle for anything revolved or helical.** Surfaces of revolution and
  screws/threads/gears want a synchronized rotary far more than a second tilt; a single rotary
  turns many "3-axis-impossible" necks, grooves, and undersides into routine wrap or flip work.

- **Undercut ≠ 5-axis-only.** Dovetails, T-slots, and torus inner grooves are re-entrant yet fully
  3-axis via form/Woodruff cutters — the tool reaches where the kinematics can't.

---

## Scoping notes for the code agent

1. **Target one regime at a time.** Implement *pointwise contact + offset for parametric
   primitives with a ball tool* first (R1 + R2). Fall through to a dexel / z-map / distance-field
   path only for R3 rows.
2. **Hand it the five offset-closure results as givens** (sphere→sphere, cylinder→coaxial
   cylinder, cone→coaxial cone same half-angle apex-shifted, torus→fatter-tube torus, plane→
   parallel plane). Do **not** let it try to derive the analytic offset of a general ellipsoid —
   that's the conniption; there isn't an elementary one.
3. **Make the ⚠ curvature/trim test a first-class step**, not an afterthought — it's where "working"
   code silently gouges.
4. **Fix the tool class up front.** Ball → contact is the center-to-surface line. Bull-nose / flat
   / tapered → add a root-find for the contact point's migration around the tool tip, even on R1.
5. **Don't ask it to derive envelopes** (screw-rotor, bevel-gear meshing) analytically — those are
   R2/R3 numeric envelope problems; point it at simulation/marching instead.
