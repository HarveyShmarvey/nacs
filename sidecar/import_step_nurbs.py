#!/usr/bin/env python3
"""STEP -> analytic NURBS: read a freeform face's b-spline geometry exactly,
no tessellation (contract v1, sidecar/CONTRACT.md). Two kinds of face:

- a SURFACE_OF_REVOLUTION whose basis curve is a b-spline -> the meridian as
  an exact `.spline` (Feature::SplineRevolve / SplineRevolveFunnel);
- a general untrimmed B_SPLINE_SURFACE -> the control net as an exact
  `.patch` (Feature::NurbsPatch, the raster drop-cutter against the exact
  surface — DECISIONS.md "the general NURBS patch…", slice 1).

The sidecar's fourth tool, and the *analytic* counterpart to import_step_mesh.py
(ROADMAP Phase 4 follow-up "an analytic NURBS reader — machine the patch
without tessellating"; DECISIONS.md "Analytic spline-of-revolution reader…").
Where mesh export tessellates a freeform revolved face to an STL triangle soup
and the drop-cutter accepts the stair-step error, this reads the SAME kind of
face — a SURFACE_OF_REVOLUTION whose basis curve is a b-spline — and writes the
meridian out as an exact `.spline` curve. The Rust core's Feature::SplineRevolve
then machines the surface of revolution through the R2 pointwise-contact
machinery (finish_spline_revolve) with NO tessellation error: contact point,
normal, and curvature all come from de Boor evaluation of the spline.

Like the DXF and mesh tools, this is NOT a new seam: the output is the plain
`.spline` text the Rust parser already reads via `SplineRevolve(file=…)` /
`SplineRevolveFunnel(file=…)`, so the Rust core and the `.part` grammar are
unchanged — the analytic reader IS this conversion. It is deliberately narrow:
only a full-revolution face with a b-spline meridian and an axis parallel to
machine +Z. BOTH orientations read (DECISIONS.md "Spline funnel…"): the face's
oriented outward normal tells a cap (radial component positive — material
inside) from a cavity (negative — the normal points into the void), and each
is emitted in its envelope's canonical top-first order (apex-first cap,
mouth-first funnel) for its own Rust validator to accept or refuse. Anything
else is honestly refused — a partial revolution, a tilted axis, an ambiguous
(axial-normal) wall, or a part with no such face all exit nonzero with a
one-line reason and NO partial output.

Invocation (argv + files, no stdin protocol, no network — the same ground rules
as the other three tools):

  sidecar/.venv/bin/python sidecar/import_step_nurbs.py <model.step> -o <outdir>/
      [--solid N] [--up {x+,x-,y+,y-,z+,z-}]

--solid / --up are the same setup declarations as the other tools: a multi-solid
STEP refuses unless --solid picks the part, and --up names which MODEL axis is up
on the machine (default z+) and is rotated onto +Z BEFORE the meridian is read,
so a Y-up export's cap sits the same way its Z-up twin would — the finisher's
top-down reachability is decided in machine orientation.

Writes <outdir>/meridian.spline (the meridian b-spline; meridian_2.spline, … for
a part with more than one such face), <outdir>/surface.patch (an untrimmed
b-spline patch's control net; surface_2.patch, …), and <outdir>/proposed.part
(reviewable SplineRevolve / SplineRevolveFunnel / NurbsPatch line(s)). A part
with NO readable face refuses — there is nothing to read (use import_step.py
for analytic primitives, import_step_mesh.py for the freeform faces this can't
take). A TRIMMED b-spline patch is never read as its full rectangular domain —
that would invent surface the part does not have, and tessellating the trim
boundary is a guess, not a reading — so it stays with the mesh path, and the
refusal counts such faces by name. On any error: one-line stderr, exit 1, no
partial output files — every output is built in full before any is written.
"""

import argparse
import math
import sys
from pathlib import Path

from OCP.BRep import BRep_Tool
from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeFace
from OCP.BRepGProp import BRepGProp
from OCP.BRepTools import BRepTools
from OCP.GProp import GProp_GProps

# Shared with recognition — one number formatter, one set of setup declarations,
# one multi-solid guard, one oriented face normal (imported, not duplicated; the
# same DRY-within-Python as import_step_mesh.py).
from import_step import UP_ROTATIONS, face_normal, fmt, select_solid

# Axis-parallel tolerance: |axis.Z| within this of 1 counts as parallel to +Z
# (the cosine of ~2.5°), matching import_step's COS_TOL usage for revolved axes.
COS_TOL = 1e-3
# Full-revolution tolerance on the angular (v) span, mirroring import_step's
# full_revolution() check on cylindrical faces.
LIN_TOL = 1e-6


def revolution_meridian(face):
    """If `face` is a full-revolution SURFACE_OF_REVOLUTION with a b-spline
    basis curve and an axis parallel to +Z, return (degree, flat_knots, poles,
    weights, rational, cavity, x, y, z_base) with poles projected into the
    (r, z) meridian half-plane, top-first (z descending — apex-first for a
    cap, MOUTH-first for a cavity); otherwise return a string saying why not.

    `cavity` tells the two orientations apart, by the face's oriented outward
    normal (import_step.face_normal, the bore-wall material-side precedent):
    out of the material means a positive radial component on a cap (material
    inside/below the surface) and a negative one on a cavity wall (the normal
    points into the void the tool descends). A near-zero radial component —
    a (locally) vertical wall at mid-parameters that could be either — is
    refused by name rather than guessed.

    z_base is the axis location's height. For a cap the feature's `z` adds it
    back (world_z = z_base + meridian_z); a cavity proposal carries no z (the
    Rust variant puts the mouth in the stock-top plane), so z_base is only
    reported in a comment. Poles are millimetres by definition (the Rust
    variants scale only placement, never the meridian)."""
    if face.geomType() != "REVOLUTION":
        return "not a surface of revolution"
    surf = BRep_Tool.Surface_s(face.wrapped)
    bs = surf.BasisCurve()
    # BasisCurve() hands back the concrete curve type; only a b-spline meridian
    # is analytic here (a line basis is a cone, a circle a sphere/torus — those
    # are recognition's job, not this tool's).
    if bs.DynamicType().Name() != "Geom_BSplineCurve":
        return f"revolved face whose basis curve is {bs.DynamicType().Name()}, not a b-spline (recognition's job, not the analytic reader's)"
    if bs.IsPeriodic():
        return "revolved face whose meridian b-spline is periodic (a closed profile) -- not a machinable cap"

    # The revolution must be full 2*pi, or full waterline circles would overcut
    # the missing wedge.
    _, _, vmin, vmax = BRepTools.UVBounds_s(face.wrapped)
    if (vmax - vmin) < 2.0 * math.pi - LIN_TOL:
        return "partial-revolution face (not swept a full 360 deg) -- the finisher cuts full waterline circles"

    ax = surf.Axis()
    d = ax.Direction()
    loc = ax.Location()
    if abs(d.Z()) < 1.0 - COS_TOL:
        return "surface-of-revolution axis not parallel to +Z (tilted) -- machine it upright (declare --up) or leave it to the mesh path"
    # Normalize the axis to point +Z so the meridian z increases upward, even if
    # the kernel stored the axis pointing down.
    sign = 1.0 if d.Z() > 0.0 else -1.0
    ax_dir = (sign * d.X(), sign * d.Y(), sign * d.Z())
    ax_loc = (loc.X(), loc.Y(), loc.Z())

    # Cavity or cap, from the oriented outward normal's radial component at
    # mid-parameters (see the docstring). Computed in machine orientation —
    # --up has already rotated the solid.
    pnt, n = face_normal(face)
    v = (pnt.X() - ax_loc[0], pnt.Y() - ax_loc[1], pnt.Z() - ax_loc[2])
    zc = v[0] * ax_dir[0] + v[1] * ax_dir[1] + v[2] * ax_dir[2]
    rad = (v[0] - zc * ax_dir[0], v[1] - zc * ax_dir[1], v[2] - zc * ax_dir[2])
    rad_len = math.sqrt(rad[0] ** 2 + rad[1] ** 2 + rad[2] ** 2)
    if rad_len < LIN_TOL:
        return ("revolved b-spline face whose mid-parameter point sits on the "
                "axis -- cannot read the material side there")
    n_r = (n.X() * rad[0] + n.Y() * rad[1] + n.Z() * rad[2]) / rad_len
    if abs(n_r) <= COS_TOL:
        return ("revolved b-spline face whose outward normal is axial at "
                "mid-parameters -- a vertical wall that could be a cap's rim "
                "or a cavity's bore; cannot tell the material side, not guessing")
    cavity = n_r < 0.0

    def project(p):
        v = (p.X() - ax_loc[0], p.Y() - ax_loc[1], p.Z() - ax_loc[2])
        zc = v[0] * ax_dir[0] + v[1] * ax_dir[1] + v[2] * ax_dir[2]
        rad = (v[0] - zc * ax_dir[0], v[1] - zc * ax_dir[1], v[2] - zc * ax_dir[2])
        r = math.sqrt(rad[0] ** 2 + rad[1] ** 2 + rad[2] ** 2)
        return (r, zc)

    degree = bs.Degree()
    poles = [project(bs.Pole(k)) for k in range(1, bs.NbPoles() + 1)]
    weights = [bs.Weight(k) for k in range(1, bs.NbPoles() + 1)]
    rational = bs.IsRational()
    # Flatten the compact (knot, multiplicity) representation into the full knot
    # sequence the Rust de Boor reads.
    knots = []
    for k in range(1, bs.NbKnots() + 1):
        knots.extend([bs.Knot(k)] * bs.Multiplicity(k))

    # Emit top-first (z non-increasing from the first pole) — apex-first for a
    # cap, mouth-first for a cavity: the same normalization serves both, since
    # each envelope's canonical order starts at its top. If the kernel stored
    # the meridian bottom-first, reverse the curve: poles and weights reversed,
    # knots mirrored about their own span (a rigid reparametrization that
    # leaves the geometry identical).
    if poles[0][1] < poles[-1][1]:
        poles.reverse()
        weights.reverse()
        lo, hi = knots[0], knots[-1]
        knots = [lo + hi - k for k in reversed(knots)]

    return (degree, knots, poles, weights, rational, cavity, ax_loc[0], ax_loc[1], ax_loc[2])


def _face_area(shape):
    """Surface area of a face (or any shape), via GProp."""
    props = GProp_GProps()
    BRepGProp.SurfaceProperties_s(shape, props)
    return props.Mass()


def bspline_patch(face):
    """If `face` is an untrimmed (full-natural-domain) b-spline surface face
    machinable from above, return (degree_u, degree_v, knots_u, knots_v,
    poles, weights, rational) with poles as row-major (x, y, z) mm triples
    (u rows of v columns — the Rust `.patch` order); otherwise a string
    saying why not.

    A trimmed face's reason starts with 'trimmed' — read() counts those so
    the nothing-read refusal can point at the mesh path by name. The trim
    check is three independent proxies that together refuse any real trim:
    the face's UV box must equal the surface's natural bounds, the face must
    carry exactly one wire (an inner wire is a trimmed hole), and the face's
    area must equal the untrimmed surface's own (any proper trim strictly
    shrinks it — this one catches a non-rectangular outer wire whose UV box
    still spans the full domain)."""
    if face.geomType() != "BSPLINE":
        return "not a b-spline surface"
    surf = BRep_Tool.Surface_s(face.wrapped)
    if surf.DynamicType().Name() != "Geom_BSplineSurface":
        return f"b-spline-typed face whose surface is {surf.DynamicType().Name()}"
    if surf.IsUPeriodic() or surf.IsVPeriodic():
        return "periodic b-spline surface (a closed tube) -- not an open patch"

    fu1, fu2, fv1, fv2 = BRepTools.UVBounds_s(face.wrapped)
    su1, su2, sv1, sv2 = surf.Bounds()
    tol = 1e-9 * max(su2 - su1, sv2 - sv1)
    trimmed = (
        abs(fu1 - su1) > tol
        or abs(fu2 - su2) > tol
        or abs(fv1 - sv1) > tol
        or abs(fv2 - sv2) > tol
        or len(face.Wires()) != 1
    )
    if not trimmed:
        natural = BRepBuilderAPI_MakeFace(surf, 1e-6).Face()
        area_natural = _face_area(natural)
        trimmed = abs(_face_area(face.wrapped) - area_natural) > 1e-6 * max(area_natural, 1.0)
    if trimmed:
        return ("trimmed b-spline patch (its boundary does not span the surface's full "
                "natural domain)")

    # Machinable from above: the raster drop-cutter machines the reachable
    # upper envelope, so the oriented outward normal at mid-parameters needs
    # an upward component — a downward face is the part's underside, a
    # vertical one has no envelope under a vertical drop. Skipped, not
    # refused: the same per-face convention as the meridian reader.
    _, n = face_normal(face)
    if n.Z() <= COS_TOL:
        return ("b-spline patch facing down or vertical at mid-parameters -- the raster "
                "drop-cutter machines from above")

    degree_u, degree_v = surf.UDegree(), surf.VDegree()
    poles = []
    weights = []
    for i in range(1, surf.NbUPoles() + 1):
        for j in range(1, surf.NbVPoles() + 1):
            p = surf.Pole(i, j)
            poles.append((p.X(), p.Y(), p.Z()))
            weights.append(surf.Weight(i, j))
    rational = surf.IsURational() or surf.IsVRational()
    knots_u = []
    for k in range(1, surf.NbUKnots() + 1):
        knots_u.extend([surf.UKnot(k)] * surf.UMultiplicity(k))
    knots_v = []
    for k in range(1, surf.NbVKnots() + 1):
        knots_v.extend([surf.VKnot(k)] * surf.VMultiplicity(k))
    return (degree_u, degree_v, knots_u, knots_v, poles, weights, rational)


def patch_text(source_name, degree_u, degree_v, knots_u, knots_v, poles, weights, rational):
    """One deterministic `.patch` file for an untrimmed b-spline surface:
    degrees, the two flat knot vectors, one row-major `pole x y z` per control
    point, and (only when rational) a `weights` line. Every number through
    fmt() so the file is byte-identical across runs (a committed golden)."""
    lines = [
        f"# b-spline patch read from {source_name} by import_step_nurbs.py",
        "# (x, y, z) control net, row-major (u rows of v poles), in mm; full",
        "# natural domain (untrimmed). No tessellation.",
        f"degree_u {degree_u}",
        f"degree_v {degree_v}",
        "knots_u " + " ".join(fmt(k) for k in knots_u),
        "knots_v " + " ".join(fmt(k) for k in knots_v),
    ]
    for (x, y, z) in poles:
        lines.append(f"pole {fmt(x)} {fmt(y)} {fmt(z)}")
    if rational:
        lines.append("weights " + " ".join(fmt(w) for w in weights))
    return "\n".join(lines) + "\n"


def spline_text(source_name, feat_name, degree, knots, poles, weights, rational, cavity):
    """One deterministic `.spline` file for the meridian: degree, the flat knot
    vector, one `pole r z` per control point, and (only when rational) a
    `weights` line. Every number through fmt() so the file is byte-identical
    across runs (a committed golden). Only the header comment differs between
    the orientations — the grammar is one format."""
    if cavity:
        order = "# (r, z) control polygon in the meridian half-plane, mouth-first, in mm;"
        feat = "SplineRevolveFunnel"
    else:
        order = "# (r, z) control polygon in the meridian half-plane, apex-first, in mm;"
        feat = "SplineRevolve"
    lines = [
        f"# meridian b-spline read from {source_name} by import_step_nurbs.py",
        order,
        f"# revolved about the {feat} feature's Z axis. No tessellation.",
        f"degree {degree}",
        "knots " + " ".join(fmt(k) for k in knots),
    ]
    for (r, z) in poles:
        lines.append(f"pole {fmt(r)} {fmt(z)}")
    if rational:
        lines.append("weights " + " ".join(fmt(w) for w in weights))
    return "\n".join(lines) + "\n"


def sanitize(stem, suffix="_Cap"):
    """A source stem -> a valid `.part` feature identifier (alnum + '_'),
    suffixed by orientation (_Cap / _Funnel). Deterministic, so the proposal
    name is a golden too."""
    cleaned = "".join(ch if ch.isalnum() or ch == "_" else "_" for ch in stem)
    cleaned = cleaned.strip("_") or "Revolve"
    if cleaned[0].isdigit():
        cleaned = "R_" + cleaned
    return cleaned[0].upper() + cleaned[1:] + suffix


def part_text(source_name, entries, patch_entries, n_total):
    """The reviewable proposed.part fragment: provenance plus one SplineRevolve
    / SplineRevolveFunnel line per meridian and one NurbsPatch line per
    untrimmed patch, each with a suggested consuming Copy run. A fragment,
    not a runnable file (the same convention as the other tools'
    proposed.part). `entries` is a list of
    (feat_name, spline_file, cavity, x, y, z_base, z_mouth, npoles);
    `patch_entries` of (feat_name, patch_file, n_u, n_v, degree_u, degree_v).
    The revolve-only wording is kept byte-for-byte (committed goldens); the
    patch lines only appear when a patch was read."""
    kinds = []
    if entries:
        kinds.append(f"{len(entries)} analytic spline-of-revolution face(s)")
    if patch_entries:
        kinds.append(f"{len(patch_entries)} untrimmed b-spline patch(es)")
    out = [
        f"# {source_name} -- {' + '.join(kinds)} "
        f"read by import_step_nurbs.py ({n_total} faces total).",
    ]
    if entries:
        out.extend([
            "# Each meridian is an EXACT b-spline (no tessellation); Feature::"
            "SplineRevolve machines the surface of revolution through the pointwise",
            "# ball-offset R2 machinery. The .spline (r,z) control points are mm by "
            "definition; x/y/z place the axis in stock coords.",
        ])
    if patch_entries:
        out.extend([
            "# Each patch is the EXACT tensor-product b-spline surface (no "
            "tessellation); Feature::NurbsPatch machines it with the raster",
            "# drop-cutter against the exact surface — the mesh contract minus the "
            "chord error. The .patch control net is mm by definition,",
            "# in model coords (placement x/y/z default 0).",
        ])
    for (feat_name, spline_file, cavity, x, y, z_base, z_mouth, npoles) in entries:
        if cavity:
            # No z field — the Rust variant puts the mouth in the stock-top
            # plane by construction; the model's mouth height goes in a
            # comment so the reviewer can set the stock to match.
            out.append(
                f'feature {feat_name} SplineRevolveFunnel(file="{spline_file}", '
                f"x={fmt(x)}, y={fmt(y)})"
            )
            out.append(
                f"#   mouth circle at model z={fmt(z_mouth)}; SplineRevolveFunnel "
                "places the mouth in the stock-top plane -- set stock accordingly"
            )
            out.append(
                f"#   {npoles}-pole meridian (mouth-first); consume with a "
                "ball-nose Copy:"
            )
        else:
            out.append(
                f'feature {feat_name} SplineRevolve(file="{spline_file}", '
                f"x={fmt(x)}, y={fmt(y)}, z={fmt(z_base)})"
            )
            out.append(
                f"#   {npoles}-pole meridian; consume with a ball-nose Copy "
                "(the only SplineRevolve consumer):"
            )
        out.append(f"#   run Copy(tool=T?, surface={feat_name}, stepover=?)")
    for (feat_name, patch_file, n_u, n_v, degree_u, degree_v) in patch_entries:
        out.append(f'feature {feat_name} NurbsPatch(file="{patch_file}")')
        out.append(
            f"#   {n_u}x{n_v}-pole degree {degree_u}x{degree_v} patch, full natural "
            "domain; consume with a ball-nose Copy (raster drop-cutter):"
        )
        out.append(f"#   run Copy(tool=T?, surface={feat_name}, stepover=?)")
    return "\n".join(out) + "\n"


def read(step_path, solid_sel=None, up="z+"):
    solid, solid_sel, n_solids = select_solid(step_path, solid_sel)
    if UP_ROTATIONS[up] is not None:
        # Rotate the declared up axis onto machine +Z before reading, so the
        # meridian sits as it will on the machine — the finisher's top-down
        # reachability is decided in machine orientation, exactly as the mesh
        # tool tessellates post-rotation.
        axis, degrees = UP_ROTATIONS[up]
        solid = solid.rotate((0, 0, 0), axis, degrees)

    faces = solid.Faces()
    spline_files = {}
    part_entries = []
    patch_entries = []
    stem = step_path.stem
    idx = 0
    patch_idx = 0
    n_trimmed = 0
    for f in faces:
        got = revolution_meridian(f)
        if not isinstance(got, str):
            idx += 1
            degree, knots, poles, weights, rational, cavity, x, y, z_base = got
            # meridian.spline for the first, meridian_2.spline, … for the rest.
            fname = "meridian.spline" if idx == 1 else f"meridian_{idx}.spline"
            suffix = "_Funnel" if cavity else "_Cap"
            feat_name = sanitize(stem, suffix) if idx == 1 else f"{sanitize(stem, suffix)}_{idx}"
            spline_files[fname] = spline_text(
                step_path.name, feat_name, degree, knots, poles, weights, rational, cavity
            )
            # The mouth pole is first (top-first emission); its world height is
            # the cavity proposal's stock-top comment.
            z_mouth = z_base + poles[0][1]
            part_entries.append((feat_name, fname, cavity, x, y, z_base, z_mouth, len(poles)))
            continue
        pat = bspline_patch(f)
        if isinstance(pat, str):
            # Not this tool's face; honestly skipped (not total accounting).
            # Trimmed patches are counted so the nothing-read refusal can
            # name them — they machine via the mesh path, never as a guessed
            # full-domain read.
            if pat.startswith("trimmed"):
                n_trimmed += 1
            continue
        patch_idx += 1
        degree_u, degree_v, knots_u, knots_v, poles, weights, rational = pat
        fname = "surface.patch" if patch_idx == 1 else f"surface_{patch_idx}.patch"
        feat_name = (
            sanitize(stem, "_Patch") if patch_idx == 1 else f"{sanitize(stem, '_Patch')}_{patch_idx}"
        )
        spline_files[fname] = patch_text(
            step_path.name, degree_u, degree_v, knots_u, knots_v, poles, weights, rational
        )
        n_v = len(knots_v) - degree_v - 1
        n_u = len(poles) // n_v
        patch_entries.append((feat_name, fname, n_u, n_v, degree_u, degree_v))

    if not part_entries and not patch_entries:
        msg = (
            f"no analytic spline-of-revolution face or untrimmed b-spline patch in "
            f"{step_path.name}: none of the {len(faces)} face(s) qualifies -- use "
            "import_step.py for analytic primitives, import_step_mesh.py for freeform faces"
        )
        if n_trimmed:
            msg += (
                f" ({n_trimmed} b-spline patch face(s) present but TRIMMED: reading one "
                "as its full rectangular domain would invent surface the part does not "
                "have, so a trimmed patch stays with import_step_mesh.py)"
            )
        raise ValueError(msg)

    part = part_text(step_path.name, part_entries, patch_entries, len(faces))
    kinds = []
    if part_entries:
        kinds.append(f"{len(part_entries)} analytic spline-of-revolution meridian(s)")
    if patch_entries:
        kinds.append(f"{len(patch_entries)} untrimmed b-spline patch(es)")
    summary = f"{len(faces)} faces: {' + '.join(kinds)} read"
    return spline_files, part, summary


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("step", type=Path, help="input STEP file")
    ap.add_argument("-o", "--outdir", type=Path, required=True,
                    help="output directory (meridian.spline + proposed.part)")
    ap.add_argument("--solid", type=int, default=None, metavar="N",
                    help="which solid is the part (multi-solid STEP)")
    ap.add_argument("--up", choices=sorted(UP_ROTATIONS), default="z+",
                    help="which model axis is up on the machine (default z+)")
    args = ap.parse_args()

    try:
        spline_files, part, summary = read(args.step, args.solid, args.up)
    except Exception as e:  # noqa: BLE001 — one-line stderr, exit 1, no files
        sys.exit(f"error: {e}")

    # Everything is fully built above; write only now, so a failure never leaves
    # a partial .spline or a dangling proposal (the "no partial output"
    # convention shared with the Rust CLI and the other sidecar tools).
    args.outdir.mkdir(parents=True, exist_ok=True)
    for fname, text in spline_files.items():
        (args.outdir / fname).write_text(text, encoding="utf-8", newline="\n")
    (args.outdir / "proposed.part").write_text(part, encoding="utf-8", newline="\n")
    print(summary)


if __name__ == "__main__":
    main()
