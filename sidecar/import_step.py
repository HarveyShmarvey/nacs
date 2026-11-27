#!/usr/bin/env python3
"""STEP import + walking-skeleton recognition (contract v1, sidecar/CONTRACT.md).

Reads a STEP file, accounts for EVERY face of the body (total accounting —
DECISIONS.md "Recognition residue policy…"), and proposes `.part` feature text
for the graduated surface types:

  plane    -> Plane(z=…)                      only at the topmost height, normal +Z
  cylinder -> CylinderBore(dia=…, extent=…, x=…, y=…)  only a full-revolution,
              axis-parallel-Z, upward-opening bore (material outside).
              The BOTTOM face decides hole-vs-bore semantics: a through
              hole or a drill-point cone proposes `_Hole` (consume with
              Form at points); a flat floor absorbs into a `_Bore` group (consume
              with Side); any other bottom is a wall-only `_Bore`
  sphere   -> Sphere(dia=…, x=…, y=…, z=…)    only a convex dome at/above
              its equator (material inside)
  cone     -> Cone(dia=…, height=…, x=…, y=…, z=…)  only an axis-parallel-Z,
              base-down apex-up full-cone boss (material inside)
  torus    -> Torus(major_dia=…, minor_dia=…, x=…, y=…, z=…)  only an
              axis-parallel-Z convex ring at/above its equator plane
  pocket   -> ProfilePocket(…) multi-line block      a below-top planar floor plus
              its closed loop of vertical walls (planes / partial cylinders),
              grouped into ONE proposal shared by every member face
  boss     -> CylinderBoss(dia=…, extent=…, x=…, y=…, z=…)  only a full-revolution,
              axis-parallel-Z standing boss (material inside) under a flat
              +Z top cap, based on a clean upward ledge or the part bottom
  outer    -> ProfileBoss(…) multi-line block, wound CW (material inside, tool
              outside -- the winding convention, DECISIONS.md "Row-2.4 side
              selector"): a +Z cap's outer wire whose vertical walls descend
              to one base height -- the part's silhouette or a plateau edge
  trim     -> ContourBoss(extent=…)/ContourPocket(extent=…) multi-line blocks
              of 3-number `x y z` points (the 3D-wire pair, DECISIONS.md "The
              3D-wire Contour…"): a closed loop of vertical walls descending
              to the PART BOTTOM whose straight top edges form a non-planar
              3D wire -- a through trim. The bottom face's outer wire
              proposes the boss (CW), each inner wire a pocket (CCW)

Everything else lands in the residue with a reason, its CHECKLIST.md row,
the refusing check's `code`, and the `refs` the reason names (`residue()`).
The proposal criteria ARE the orientation/undercut checks -- a surface type
graduates from residue to proposable only together with its check (ROADMAP
Phase 4 co-requisite). Recognition proposes; it never machines.

Invocation (argv + files, no stdin protocol, no network):

  sidecar/.venv/bin/python sidecar/import_step.py <model.step> -o <outdir>/
      [--solid N] [--up {x+,x-,y+,y-,z+,z-}]

Setup is declared, never guessed (DECISIONS.md -> "Import breadth…"): a
multi-solid STEP (an assembly) refuses with a per-solid inventory unless
--solid picks the part, and --up names which MODEL axis points up on the
machine (default z+) -- the sidecar rotates that axis onto +Z before
recognition, so a Y-up CAD export reads like its Z-up twin.

Writes <outdir>/report.json (always) and <outdir>/proposed.part (when at
least one face is proposed). On any error: one-line stderr, exit 1, no
partial output files -- both outputs are fully built and the totality
invariant asserted before either file is written.
"""

import argparse
import json
import math
import re
import sys
import textwrap
from pathlib import Path

import cadquery as cq
from OCP.BRepAdaptor import BRepAdaptor_Curve, BRepAdaptor_Surface
from OCP.BRepClass import BRepClass_FaceClassifier
from OCP.BRepClass3d import BRepClass3d_SolidClassifier
from OCP.BRepGProp import BRepGProp_Face
from OCP.BRepTools import BRepTools, BRepTools_WireExplorer
from OCP.GeomAbs import GeomAbs_CurveType
from OCP.gp import gp_Dir, gp_Lin, gp_Pnt, gp_Pnt2d, gp_Vec
from OCP.IntCurvesFace import IntCurvesFace_ShapeIntersector
from OCP.TopAbs import TopAbs_Orientation, TopAbs_ShapeEnum, TopAbs_State
from OCP.TopExp import TopExp
from OCP.TopTools import TopTools_IndexedDataMapOfShapeListOfShape

SCHEMA = "nacs-recognition-report"
VERSION = 1

# Geometric tolerances for the orientation checks. Angular: how parallel an
# axis/normal must be to +Z to count (cos of ~2.6 deg). Linear: mm slop for
# "same height" / full-revolution / point-classification comparisons.
COS_TOL = 0.999
LIN_TOL = 1e-3

# Max chord error (mm) when tessellating a circular boundary arc to line
# segments for a Contour proposal. A DELIBERATE duplicate of the Rust core's
# CHORD_TOL_MM (src/lib.rs) across the language boundary -- the sidecar can't
# import a Rust constant, and the committed goldens pin any drift between the
# two (DECISIONS.md -> "Pocket recognition…").
CHORD_TOL = 0.01

# How far past the blank's silhouette a floor's open edge is drawn (Phase 46
# slice 2): two chord tolerances, radially. The engine clips every outline to
# the declared stock, so the program is the same bytes however far past the
# blank the edge is drawn (`step.cam`'s own law) -- but an edge drawn ON the
# circle clips into slivers against the engine's chords of that same circle
# (its vertices sit a sagitta outside them, its chord midpoints a sagitta
# inside), and the engine's on-the-blank test is three decimals. Past by two
# sagittas, every vertex and midpoint is outside, the clip keeps the engine's
# own chords, and the edge reads OPEN on a `stock Cylinder` of the part's
# diameter -- and WALL on a larger Box, which is also right.
SILHOUETTE_PUSH = 2.0 * CHORD_TOL

# --up values: which MODEL axis is up on the machine, mapped to the one
# rotation (axis, degrees, right-hand rule) that carries it onto +Z. The
# values avoid a leading '-' so argparse never mistakes one for a flag.
# Twinned by the engine as `parse::UpAxis` (Phase 48 slice 1): a program's
# `given from <project> up=<axis>` means the same rotation about the same
# origin, so a change here is a change there -- like CHORD_TOL above.
UP_ROTATIONS = {
    "z+": None,
    "z-": ((1, 0, 0), 180),
    "y+": ((1, 0, 0), 90),
    "y-": ((1, 0, 0), -90),
    "x+": ((0, 1, 0), -90),
    "x-": ((0, 1, 0), 90),
}
# The other setup's up axis: the model turned over.
UP_OPPOSITE = {"z+": "z-", "z-": "z+", "y+": "y-", "y-": "y+",
               "x+": "x-", "x-": "x+"}

# How far a reach probe steps off a face into the void before looking
# straight up (ROADMAP item 1 slice 7): enough to clear the face's own
# tolerance, far less than any gap a tool could enter.
REACH_NUDGE = 0.05


def fmt(v):
    """One number formatter for everything emitted (determinism rule):
    3 decimals max, trailing zeros stripped, always at least one decimal,
    and no negative zero."""
    v = round(v, 3) + 0.0  # + 0.0 turns -0.0 into 0.0
    s = f"{v:.3f}".rstrip("0")
    return s + "0" if s.endswith(".") else s


def residue(code, row, reason, refs=()):
    """The one home for an `unsupported` entry's shape (CONTRACT.md "Residue
    codes"): `code` is the refusing check's name written down -- a word
    string, never a number, derived from the branch and not from the
    sentence -- and `refs` the face ids / feature names the sentence names,
    built from the same values the sentence is formatted from and present
    only when there is at least one (the additive rule: an absent field,
    never an empty list)."""
    c = dict(status="unsupported", checklist_row=row, code=code, reason=reason)
    refs = [r for k, r in enumerate(refs) if r not in refs[:k]]
    if refs:
        c["refs"] = refs
    return c


def face_normal(face):
    """Oriented outward normal at the face's mid-parameters, as a unit gp_Vec.
    BRepGProp_Face accounts for face orientation, so for a solid this points
    out of the material."""
    ad = BRepAdaptor_Surface(face.wrapped)
    u = (ad.FirstUParameter() + ad.LastUParameter()) / 2.0
    v = (ad.FirstVParameter() + ad.LastVParameter()) / 2.0
    pnt, normal = gp_Pnt(), gp_Vec()
    BRepGProp_Face(face.wrapped).Normal(u, v, pnt, normal)
    normal.Normalize()
    return pnt, normal


def classify_plane(face, z_top):
    """Graduation check for planes: only the topmost planar face with normal
    +Z is proposable. Everything else is residue with its row."""
    pnt, n = face_normal(face)
    if n.Z() > COS_TOL:
        if abs(pnt.Z() - z_top) < LIN_TOL:
            return dict(status="proposed", checklist_row="1.1",
                        why="planar, normal +Z, topmost planar face")
        # A +Z plane below the topmost height is a bore floor or a
        # pocket-floor candidate, and the grouping passes absorb, claim, or
        # refuse every one BEFORE per-face classification -- so this branch
        # cannot fire.
        raise AssertionError("below-top +Z plane escaped the grouping passes")
    if n.Z() < -COS_TOL:
        return residue(code="plane_downward", row="no row",
                       reason="downward-facing planar face -- unreachable from "
                              "above (undercut relative to the tool axis)")
    if abs(n.Z()) < 1.0 - COS_TOL:
        return residue(code="plane_vertical_unanchored", row="2.2",
                       reason="vertical planar wall not bounding a recognized "
                              "pocket floor or outer-profile cap loop -- a "
                              "standalone wall with no proposable anchor")
    return residue(code="plane_oblique", row="1.2",
                   reason="oblique planar face -- no tilted-plane primitive "
                          "(CHECKLIST row 1.2 is todo)")


def classify_cylinder(face, solid):
    """Graduation check for cylinders: full revolution, axis parallel +Z,
    material outside the surface (a bore), opening upward. Each failed check
    names itself in the residue reason. Hole-vs-bore semantics (what the
    bottom face makes of a graduated wall) live in hole_or_bore_group()."""
    ad = BRepAdaptor_Surface(face.wrapped)
    cyl = ad.Cylinder()
    axis = cyl.Axis().Direction()

    if abs(axis.Z()) < COS_TOL:
        return residue(code="cylinder_axis_tilted", row="2.20",
                       reason="cylindrical face with axis not parallel to +Z "
                              "(oblique cylinder, CHECKLIST row 2.20)")
    if (ad.LastUParameter() - ad.FirstUParameter()) < 2.0 * math.pi - LIN_TOL:
        return residue(code="cylinder_partial_unanchored", row="2.8",
                       reason="partial-revolution cylindrical face not bounding "
                              "a recognized pocket floor or outer-profile cap "
                              "loop -- part of a general profile with no "
                              "proposable anchor")

    # Material side: the outward normal of a bore wall points toward the
    # axis (out of the material is into the hole); a boss's points away.
    pnt, n = face_normal(face)
    loc = cyl.Axis().Location()
    radial = gp_Vec(pnt.X() - loc.X(), pnt.Y() - loc.Y(), 0.0)
    if n.Dot(radial) > 0.0:
        return residue(code="cylinder_boss_uncapped", row="2.4",
                       reason="cylindrical face with material inside (outside "
                              "boss) not anchored by a flat +Z top cap -- a "
                              "capped, domed, or stepped boss cannot graduate "
                              "to Feature::CylinderBoss (CHECKLIST row 2.4)")

    # A bore must open upward to be reachable: a point on the axis just
    # above the wall's top rim must be outside the solid, not in material.
    bb = face.BoundingBox()
    probe = gp_Pnt(loc.X(), loc.Y(), bb.zmax + 10.0 * LIN_TOL)
    state = BRepClass3d_SolidClassifier(solid.wrapped, probe, LIN_TOL).State()
    if state != TopAbs_State.TopAbs_OUT:
        return residue(code="cylinder_bore_capped", row="no row",
                       reason="bore does not open upward (capped above) -- "
                              "unreachable (undercut relative to the tool axis)")

    return dict(status="proposed", checklist_row="2.3",
                why="cylindrical, axis +Z, full revolution, material outside "
                    "(a bore), opens upward",
                dia=2.0 * cyl.Radius(), depth=bb.zmax - bb.zmin,
                x=loc.X(), y=loc.Y())


FIT_TOL = 1e-3


def plan_bounding_cylinder(faces, bb):
    """The vertical cylindrical face that bounds the plan on all four sides
    -- the round bar or disc the model came out of -- as (dia, x, y, face
    index), or None. One home for the stock proposer (a `stock Cylinder`)
    and the rim pass (the boss that brings the blank to that diameter)."""
    for i, f in enumerate(faces, start=1):
        if f.geomType() != "CYLINDER":
            continue
        ad = BRepAdaptor_Surface(f.wrapped)
        cyl = ad.Cylinder()
        if abs(cyl.Axis().Direction().Z()) < COS_TOL:
            continue
        loc, r = cyl.Axis().Location(), cyl.Radius()
        if (abs(loc.X() - r - bb.xmin) < FIT_TOL and abs(loc.X() + r - bb.xmax) < FIT_TOL
                and abs(loc.Y() - r - bb.ymin) < FIT_TOL and abs(loc.Y() + r - bb.ymax) < FIT_TOL):
            return 2.0 * r, loc.X(), loc.Y(), i
    return None


BLANK_KEYS = {"Cylinder": ("dia", "h", "x", "y", "z"), "Box": ("w", "l", "h", "x", "y", "z")}


def parse_blank(words):
    """The blank the engine hands over with `--blank` (ROADMAP item 1,
    slice 12): the stock statement's words, `Cylinder(dia=120, h=30, x=0,
    y=0, z=0)` or `Box(w=, l=, h=, x=, y=, z=)` -- the earlier program's
    blank turned over, which the engine computes and this side only reads.
    Returns (kind, {key: float}); anything else is an error."""
    m = re.fullmatch(r"\s*(Cylinder|Box)\s*\((.*)\)\s*", words)
    if m is None:
        raise ValueError(f"--blank {words!r}: not Cylinder(dia=, h=, x=, y=, z=) or "
                         f"Box(w=, l=, h=, x=, y=, z=)")
    kind = m.group(1)
    vals = {}
    for pair in m.group(2).split(","):
        k, _, v = pair.partition("=")
        try:
            vals[k.strip()] = float(v)
        except ValueError:
            raise ValueError(f"--blank {words!r}: {pair.strip()!r} is not key=number") from None
    missing = [k for k in BLANK_KEYS[kind] if k not in vals]
    if missing:
        raise ValueError(f"--blank {words!r}: {kind} wants {', '.join(missing)}")
    return kind, vals


def blank_words(kind, vals):
    return f"{kind}(" + ", ".join(f"{k}={fmt(vals[k])}" for k in BLANK_KEYS[kind]) + ")"


def blank_margins(blank, faces, bb):
    """How far the given blank stands outside the model: (on the sides, above
    the top). A Cylinder blank around a model whose plan a cylindrical face
    bounds is measured ring to ring; otherwise the blank's plan against the
    model's bounding box. A model that stands outside the blank is an
    error naming the side: the earlier blank turned over does not hold it,
    so the turn or the origin is wrong, and nothing proposes from it."""
    kind, v = blank
    if kind == "Cylinder":
        r = v["dia"] / 2.0
        round_ = plan_bounding_cylinder(faces, bb)
        if round_ is not None and abs(round_[1] - v["x"]) < FIT_TOL and abs(round_[2] - v["y"]) < FIT_TOL:
            side = r - round_[0] / 2.0
        else:
            side = min(bb.xmin - (v["x"] - r), v["x"] + r - bb.xmax,
                       bb.ymin - (v["y"] - r), v["y"] + r - bb.ymax)
    else:
        side = min(bb.xmin - v["x"], v["x"] + v["w"] - bb.xmax,
                   bb.ymin - v["y"], v["y"] + v["l"] - bb.ymax)
    top = v["z"] - bb.zmax
    bottom = bb.zmin - (v["z"] - v["h"])
    for what, d in (("the sides", side), ("the top", top), ("the bottom", bottom)):
        if d < -LIN_TOL:
            raise ValueError(f"the model stands {fmt(-d)} outside the given blank "
                             f"{blank_words(kind, v)} on {what}: the earlier program's "
                             f"blank turned over does not hold this model, so the turn "
                             f"(--up) or the origin differs between the two setups")
    return max(side, 0.0), max(top, 0.0)


def withdraw_rim_for_given(solid, faces, member_of, given):
    """With the blank given (slice 12), the face that bounds the plan on
    every side -- the blank's rim -- is not this setup's to bring to size:
    the earlier program's cuts are air here (`given from`), and what it
    left of the blank's skin stands as it left it. A whole-height boss the
    outer-profile pass made of that face is withdrawn, and the face lands in
    the residue by name. Returns {face: residue}."""
    notes = {}
    hit = plan_bounding_cylinder(faces, solid.BoundingBox())
    if hit is None:
        return notes
    i = hit[3]
    face = faces[i - 1]
    pnt, n = face_normal(face)
    if n.Dot(gp_Vec(pnt.X() - hit[1], pnt.Y() - hit[2], 0.0)) <= 0.0:
        return notes  # material outside: a bore, the hole pass's
    g = member_of.get(i)
    if g is not None and g["primary"] == i:
        for m in g["members"]:
            member_of.pop(m, None)
    notes[i] = residue(
        code="rim_given", row="2.4",
        reason="cylindrical face bounding the plan on every side -- the blank's "
               f"rim -- on a blank given from {given}: the earlier program's "
               "blank turned over, whose cuts are air here, so the rim was that "
               "setup's to bring to size and stands as it left it; nothing to "
               "propose from this side")
    return notes


def rim_band(face, z_bottom, emap):
    """The stretch of a cylindrical face, from the part bottom up, that runs
    the whole way round: it ends at the first height where an edge with
    another face across it crosses or lies (a wall standing on the rim, a
    floor meeting it). The seam, with this face on both sides, is no
    boundary. (z_lo, z_hi), or None when the face does not start at the
    part bottom or the stretch has no height."""
    fb = face.BoundingBox()
    if abs(fb.zmin - z_bottom) > LIN_TOL:
        return None
    hi = fb.zmax
    for e in face.Edges():
        if not any(not a.IsSame(face.wrapped) for a in emap.FindFromKey(e.wrapped)):
            continue
        eb = e.BoundingBox()
        if eb.zmax - eb.zmin > LIN_TOL:
            # A vertical or sloping edge interrupts the ring over its height.
            if eb.zmin < fb.zmin + LIN_TOL:
                return None
            hi = min(hi, eb.zmin)
        elif eb.zmin > fb.zmin + LIN_TOL:
            # A horizontal edge above the bottom is where the face ends.
            hi = min(hi, eb.zmin)
    return (fb.zmin, hi) if hi - fb.zmin > LIN_TOL else None


def recognize_rim(solid, faces, emap, member_of, allowance):
    """The rim pass (ROADMAP item 1, slice 11), run only when the blank
    carries an allowance around the model: the face that bounds the plan on
    every side is the blank's rim, and the stretch of it that runs the whole
    way round from the part bottom is a `CylinderBoss` whose wall pass
    brings the blank to the part's diameter. On a blank cut to size there is
    nothing to cut there, and the face keeps whatever the other passes made
    of it. Returns (member_of additions, notes for a bounding face that
    does not propose)."""
    found, notes = {}, {}
    if allowance <= 0.0:
        return found, notes
    bb = solid.BoundingBox()
    hit = plan_bounding_cylinder(faces, bb)
    if hit is None:
        return found, notes
    dia, cx, cy, i = hit
    if i in member_of:
        return found, notes
    face = faces[i - 1]
    pnt, n = face_normal(face)
    if n.Dot(gp_Vec(pnt.X() - cx, pnt.Y() - cy, 0.0)) <= 0.0:
        return found, notes  # material outside: a bore, the hole pass's
    band = rim_band(face, bb.zmin, emap)
    fid = f"F{i}"
    if band is None:
        notes[i] = residue(
            code="rim_not_on_bottom", row="2.4",
            reason="cylindrical face bounding the plan on every side -- the blank's "
                   "rim -- whose stretch that runs the whole way round does not "
                   "stand on the part bottom: a boss floating above air is an "
                   "undercut from this side, so the rim is the other setup's to "
                   "bring to size")
        return found, notes
    lo, hi = band
    name = f"{fid}_Rim"
    why = (f"cylindrical, axis +Z, full revolution, material inside: the face that "
           f"bounds the plan on every side, the blank's rim; from the part bottom "
           f"z={fmt(lo)} to z={fmt(hi)} it runs the whole way round"
           + (", and above that other faces stand on it" if hi < face.BoundingBox().zmax - LIN_TOL else "")
           + f"; with the blank {fmt(allowance)} outside it (--allowance), that "
           f"stretch is a boss to bring to size")
    extras = [
        f"# consume with: run Side(tool=..., surface={name}, stepover=...)  "
        f"[the {fmt(allowance)} the blank stands outside the rim is the pass's "
        "depth of cut; on a blank cut to size there is nothing to run]",
    ]
    part = (f"feature {name} CylinderBoss(dia={fmt(dia)}, extent={fmt(hi - lo)}, "
            f"x={fmt(cx)}, y={fmt(cy)}, z={fmt(lo)})")
    found[i] = dict(primary=i, members=[i], name=name, part=part, row="2.4",
                    whys={i: why}, extras=extras)
    return found, notes


def full_revolution(ad):
    """True when the face sweeps the surface's full 2*pi in u."""
    return (ad.LastUParameter() - ad.FirstUParameter()) >= 2.0 * math.pi - LIN_TOL


def far_mouth_cone(cf, axis_xy, bore_r, z_bot, faces, emap, face_index):
    """The far mouth's bevel (Phase 48 slice 3, fork 2A of the DRAFT): face
    `cf` is a full-revolution cone, axis on the bore's, material outside
    (void inside), whose NARROW rim is the bore's bottom rim (its radius the
    bore's, at the wall's bottom) and whose WIDE rim lies in a downward
    plane -- the far face. That is the chamfer the other setup cuts at this
    hole's mouth, seen from the wrong side: the hole goes through. Returns
    `(height, included_angle_deg)` or None. Mirrors `recognize_mouth_
    chamfers` (the cone at the near mouth) with the heights swapped."""
    if cf.geomType() != "CONE":
        return None
    ad = BRepAdaptor_Surface(cf.wrapped)
    if not full_revolution(ad):
        return None
    cone = ad.Cone()
    if abs(cone.Axis().Direction().Z()) < COS_TOL:
        return None
    loc = cone.Axis().Location()
    if abs(loc.X() - axis_xy[0]) > LIN_TOL or abs(loc.Y() - axis_xy[1]) > LIN_TOL:
        return None
    pnt, n = face_normal(cf)
    radial = gp_Vec(pnt.X() - loc.X(), pnt.Y() - loc.Y(), 0.0)
    if n.Dot(radial) >= 0.0:
        return None  # material inside: a boss, not a mouth
    bb = cf.BoundingBox()
    apex = cone.Apex()
    semi = abs(cone.SemiAngle())
    r_top = math.tan(semi) * abs(bb.zmax - apex.Z())
    r_bot = math.tan(semi) * abs(bb.zmin - apex.Z())
    if r_bot <= r_top + LIN_TOL:
        return None  # narrowing downward: a drill point's cousin, not a far mouth
    if abs(r_top - bore_r) > 10.0 * LIN_TOL or abs(bb.zmax - z_bot) > 10.0 * LIN_TOL:
        return None
    below = across_edges(cf, faces, emap, face_index, at_bottom=True)
    if not below or not all(faces[k - 1].geomType() == "PLANE" and face_normal(faces[k - 1])[1].Z() < -COS_TOL for k in below):
        return None
    return bb.zmax - bb.zmin, math.degrees(2.0 * semi)


def drill_point_cone(cf, axis_xy, bore_r, z_bot):
    """True when face `cf` is the conical bottom a twist drill leaves: full
    revolution, apex down and reached by the face, apex on the bore axis,
    material outside (void inside), spanning the bore's full mouth at the
    wall's bottom rim. Returns the included point angle (deg) or None."""
    if cf.geomType() != "CONE":
        return None
    ad = BRepAdaptor_Surface(cf.wrapped)
    cone = ad.Cone()
    if abs(cone.Axis().Direction().Z()) < COS_TOL or not full_revolution(ad):
        return None
    apex = cone.Apex()
    bb = cf.BoundingBox()
    # Apex down, at the bottom of the face (a frustum stops short); bbox
    # carries kernel inflation, so the slop is coarser than LIN_TOL.
    if apex.Z() > bb.zmin + 10.0 * LIN_TOL:
        return None
    if math.hypot(apex.X() - axis_xy[0], apex.Y() - axis_xy[1]) > 10.0 * LIN_TOL:
        return None
    # Material outside the surface (the void is inside the cone) -- the same
    # predicate as classify_cone's internal-wall branch.
    pnt, n = face_normal(cf)
    radial = gp_Vec(pnt.X() - apex.X(), pnt.Y() - apex.Y(), 0.0)
    if n.Dot(radial) >= 0.0:
        return None
    # The cone must span the bore's full mouth: its radius at the wall's
    # bottom rim equals the bore radius (no ledge between wall and point).
    r_at_rim = math.tan(cone.SemiAngle()) * (z_bot - apex.Z())
    if abs(r_at_rim - bore_r) > 10.0 * LIN_TOL:
        return None
    return math.degrees(2.0 * cone.SemiAngle())


def hole_or_bore_group(i, face, c, faces, solid, emap, face_index):
    """Hole-vs-bore semantics for a graduated bore wall (face `i`, proposed
    dict `c` from classify_cylinder): the face across the wall's bottom rim
    edge decides what the void IS, and therefore which op reproduces it.

      air past the far rim  -> through hole  -> `_Hole`, consume with Form at points
        and no floor across it   (the rim may break out across several faces)
      drill-point cone      -> drilled hole  -> `_Hole` grouped with the cone,
                               depth to the TIP (a Form at points sends the tip to depth)
      flat island-free +Z   -> milled bore   -> `_Bore` grouped with the floor,
        plane                  consume with Side (a twist drill can't leave
                               a flat bottom, so no drill run is suggested)
      anything else         -> wall-only `_Bore` -- the bottom face keeps its
                               own classification (a true residue statement)

    Every outcome proposes; the group machinery mirrors pocket_group's."""
    bb = face.BoundingBox()
    axis_xy = (c["x"], c["y"])

    # The far rim: the chain of edges touching the wall's bottom, grown
    # along shared vertices, never through an edge that reaches the top
    # (the seam -- which, split at a step, also stops short of the top and
    # is left out as the one edge with no face across it -- and a mouth
    # that crosses a step and so has edges at two heights). A hole that breaks out across a step, or through a tilted
    # face, has far-rim edges at more than one height: the real part's
    # bolt holes, seen from the back, come out across a pad's top, its
    # foot bevel and the floor around it.
    tol = 10.0 * LIN_TOL
    edges = [e for e in face.Edges()
             if e.BoundingBox().zmax < bb.zmax - tol
             and any(not a.IsSame(face.wrapped) for a in emap.FindFromKey(e.wrapped))]
    far = [k for k, e in enumerate(edges) if e.BoundingBox().zmin < bb.zmin + tol]
    corners = [[v.toTuple() for v in e.Vertices()] for e in edges]

    def touches(k):
        return any(math.dist(p, q) < tol
                   for p in corners[k] for c in far for q in corners[c])

    grown = True
    while grown:
        grown = False
        for k in range(len(edges)):
            if k not in far and touches(k):
                far.append(k)
                grown = True
    neighbors = []
    for edge in (edges[k] for k in far):
        for anc in emap.FindFromKey(edge.wrapped):
            if not anc.IsSame(face.wrapped):
                j = face_index(anc)
                if j not in neighbors:
                    neighbors.append(j)

    def not_a_floor(j):
        # A plane the hole comes out through: anything but one facing up,
        # which is a floor or ledge the bore ends on. A vertical one is a
        # wall the hole notches on its way out (a hole across a step).
        nb = faces[j - 1]
        return nb.geomType() == "PLANE" and face_normal(nb)[1].Z() < 0.5

    # Past the far rim, on the axis: air means the void goes on (the other
    # side of the part, or a cavity), material means a bottom of some kind.
    probe = gp_Pnt(axis_xy[0], axis_xy[1], bb.zmin - 10.0 * LIN_TOL)
    air_past = (BRepClass3d_SolidClassifier(solid.wrapped, probe, LIN_TOL).State()
                == TopAbs_State.TopAbs_OUT)

    kind, angle = "other", None
    if neighbors and air_past and all(not_a_floor(j) for j in neighbors):
        kind = "through"
    elif len(neighbors) == 1:
        nb = faces[neighbors[0] - 1]
        if nb.geomType() == "PLANE":
            nz = face_normal(nb)[1].Z()
            if nz > COS_TOL and not nb.innerWires():
                kind = "flat"
        else:
            angle = drill_point_cone(nb, axis_xy, c["dia"] / 2.0, bb.zmin)
            if angle is not None:
                kind = "drillpoint"
            else:
                # The far mouth's bevel (Phase 48 slice 3): a cone widening
                # DOWNWARD from the wall's bottom rim into the far face is
                # the other setup's chamfer, and the bore goes through.
                far = far_mouth_cone(nb, axis_xy, c["dia"] / 2.0, bb.zmin, faces, emap, face_index)
                if far is not None:
                    kind = "farmouth"

    fid = f"F{i}"
    members, whys, extras = [i], {}, []
    why_wall = c["why"]
    depth = c["depth"]

    if kind == "through":
        name = f"{fid}_Hole"
        why_wall += "; bottom opens through -- a drillable through hole"
        if len(neighbors) > 1:
            why_wall += (f", breaking out across {len(neighbors)} faces ("
                         + ", ".join(f"F{j}" for j in neighbors) + ")")
        extras = [
            f"# consume with: run Form(points=[{name}], tool=..., "
            "cycle=Simple)  [the bore carries its own extent, measured down "
            "from the stock top]",
            f"# through hole -- drill past extent={fmt(depth)} by the tip's "
            "point length for full-diameter breakthrough",
            f"# or finish the wall with: run Side(tool=..., surface={name}, "
            "stepover=...)",
        ]
    elif kind == "drillpoint":
        cone_idx = neighbors[0]
        cone_bb = faces[cone_idx - 1].BoundingBox()
        depth = bb.zmax - cone_bb.zmin  # rim to the tip: what a drill at points consumes
        name = f"{fid}_Hole"
        why_wall += (f"; drill-point bottom F{cone_idx} (included angle "
                     f"{fmt(angle)} deg) -- a drilled hole")
        members.append(cone_idx)
        whys[cone_idx] = (f"drill-point bottom of recognized hole {name} "
                          f"(wall {fid})")
        extras = [
            f"# consume with: run Form(points=[{name}], tool=..., "
            "cycle=Peck, peck_increment=...)  [the bore carries its own "
            "extent, measured down from its rim]",
            f"# extent={fmt(depth)} reaches the drill TIP; the full-diameter "
            f"depth is {fmt(c['depth'])}",
            f"# match the drill's point angle (~{fmt(angle)} deg) to "
            "reproduce the bottom exactly",
        ]
    elif kind == "farmouth":
        cone_idx = neighbors[0]
        height, angle = far
        depth = bb.zmax - (bb.zmin - height)  # rim to the far face: through
        name = f"{fid}_Hole"
        why_wall += (f"; far mouth bevel F{cone_idx} ({fmt(height)} deep at "
                     f"{fmt(angle)}deg included) opens through -- the other "
                     f"setup's chamfer; a drillable through hole")
        members.append(cone_idx)
        whys[cone_idx] = (f"far mouth bevel of recognized through hole {name} "
                          f"(wall {fid}): a full-revolution cone between the "
                          f"wall's bottom and the far face -- the OTHER setup's "
                          f"chamfer, absorbed so the hole proposes through; "
                          f"break it from that side")
        extras = [
            f"# consume with: run Form(points=[{name}], tool=..., "
            "cycle=Simple)  [the bore carries its own extent, measured down "
            "from the stock top]",
            f"# through hole -- drill past extent={fmt(depth)} by the tip's "
            "point length for full-diameter breakthrough",
            f"# or finish the wall with: run Side(tool=..., surface={name}, "
            "stepover=...)",
            f"# the far mouth's {fmt(height)}mm bevel at {fmt(angle)}deg included "
            "is the OTHER setup's chamfer: import that setup (--up z-), where it",
            f"# is this hole's chamfer_depth, and give this hole there "
            f"(given from <this project> up=z-)",
        ]
    elif kind == "flat":
        floor_idx = neighbors[0]
        name = f"{fid}_Bore"
        why_wall += f"; flat floor F{floor_idx} -- a milled bore"
        members.append(floor_idx)
        whys[floor_idx] = (f"flat floor of recognized bore {name} "
                           f"(wall {fid})")
        extras = [
            f"# finish the wall with: run Side(tool=..., surface={name}, "
            "stepover=...)  [stepover is the ring spacing up the wall]",
            "# a flat floor is milled, not drilled -- a twist drill leaves a "
            "conical point, so no drill run is suggested",
        ]
    else:
        name = f"{fid}_Bore"
        why_wall += ("; bottom is not a flat floor or drill point -- "
                     "proposed wall-only as a milled bore")
        extras = [
            f"# finish the wall with: run Side(tool=..., surface={name}, "
            "stepover=...)  [stepover is the ring spacing up the wall]",
            "# wall-only proposal: the bottom face is not reproducible by "
            "drill or a flat-floor bore (see its own report entry)",
        ]

    whys[i] = why_wall
    # The bore's vertical field is `extent` (DECISIONS.md "The `extent`
    # vocabulary is scoped to the extrusion features…"); `depth` here is the
    # sidecar's own local for the measured Z span, and for a drilled hole it is
    # also the travel the drill run spends.
    # `geom` is the numbers the feature line is built from -- here, by the
    # lip-round pass (which may absorb a mouth blend) and by the placing pass
    # (which writes `z` for a rim below the stock top): every rebuild goes
    # through `bore_part_line`, never string surgery on `part`.
    geom = dict(dia=c["dia"], x=c["x"], y=c["y"], depth=depth,
                wall_top=bb.zmax, rim=bb.zmax)
    return dict(primary=i, members=members, name=name,
                part=bore_part_line(name, geom),
                row="2.3", whys=whys, extras=extras, geom=geom)


def bore_part_line(name, geom):
    """The one home for a bore's `.part` line. The bore's vertical field is
    `extent` (DECISIONS.md "The `extent` vocabulary is scoped to the
    extrusion features…"); `depth` is the sidecar's own local for the
    measured Z span, and for a drilled hole it is also the travel the drill
    run spends. `z` is written only when the placing pass set it -- the rim
    below the stock top (Phase 45) -- and `edge_round` only when the
    lip-round pass absorbed a mouth blend; an omitted key is the grammar's
    default, said once by the parser rather than restated here."""
    keys = [f"dia={fmt(geom['dia'])}", f"extent={fmt(geom['depth'])}",
            f"x={fmt(geom['x'])}", f"y={fmt(geom['y'])}"]
    if "z" in geom:
        keys.append(f"z={fmt(geom['z'])}")
    if "edge_round" in geom:
        keys.append(f"edge_round={fmt(geom['edge_round'])}")
    if "chamfer_depth" in geom:
        keys.append(f"chamfer_depth={fmt(geom['chamfer_depth'])}")
        keys.append(f"chamfer_angle={fmt(geom['chamfer_angle'])}")
    return f"feature {name} CylinderBore({', '.join(keys)})"


def recognize_mouth_chamfers(faces, member_of, face_index, emap):
    """The mouth-bevel pass (Phase 46 slice 3), the lip round's sibling: a
    full-revolution internal cone, axis +Z, narrowing downward, whose bottom
    circle is a graduated bore's mouth (its radius the bore's, its height the
    wall's top) and whose top edge meets a +Z plane IS that bore's chamfer --
    `chamfer_depth` the cone's height, `chamfer_angle` twice its semi-angle
    (the engine's included-angle convention). The rim moves to the cone's
    top (the virtual sharp corner), as the round's does."""
    claimed = {}
    for t, f in enumerate(faces, start=1):
        if t in member_of or f.geomType() != "CONE":
            continue
        ad = BRepAdaptor_Surface(f.wrapped)
        if not full_revolution(ad):
            continue
        cone = ad.Cone()
        if abs(cone.Axis().Direction().Z()) < COS_TOL:
            continue
        bb = f.BoundingBox()
        apex = cone.Apex()
        semi = abs(cone.SemiAngle())
        r_bot = math.tan(semi) * abs(bb.zmin - apex.Z())
        r_top = math.tan(semi) * abs(bb.zmax - apex.Z())
        if r_top <= r_bot + LIN_TOL:
            continue  # widening downward: not a mouth
        above = across_edges(f, faces, emap, face_index, at_bottom=False)
        if not above or not all(faces[k - 1].geomType() == "PLANE" and face_normal(faces[k - 1])[1].Z() > COS_TOL for k in above):
            continue
        loc = cone.Axis().Location()
        for g in {id(g): g for g in member_of.values()}.values():
            geom = g.get("geom")
            if geom is None or "edge_round" in geom:
                continue
            if abs(geom["x"] - loc.X()) > LIN_TOL or abs(geom["y"] - loc.Y()) > LIN_TOL:
                continue
            if abs(r_bot - geom["dia"] / 2.0) > 10.0 * LIN_TOL or abs(geom["wall_top"] - bb.zmin) > 10.0 * LIN_TOL:
                continue
            depth = bb.zmax - bb.zmin
            angle = math.degrees(2.0 * semi)
            g["members"].append(t)
            g["whys"][t] = (f"mouth bevel of recognized bore {g['name']} (wall F{g['primary']}): "
                            f"a full-revolution cone between the wall's top and the surrounding "
                            f"plane -- proposed as chamfer_depth, not a standalone cavity")
            g["whys"][g["primary"]] += (f"; mouth bevel F{t} absorbed, {fmt(depth)} deep at "
                                        f"{fmt(angle)}deg included -- extent runs to the virtual "
                                        f"sharp corner ({fmt(geom['depth'] + depth)})")
            geom["rim"] = bb.zmax
            geom["depth"] = geom["depth"] + depth
            geom["chamfer_depth"] = depth
            geom["chamfer_angle"] = angle
            g["part"] = bore_part_line(g["name"], geom)
            g["extras"] = [e for e in g["extras"]] + [
                f"# the mouth's {fmt(depth)}mm bevel is the bore's chamfer_depth, at the included angle the",
                "# model measures; a print that says otherwise is typed here (the three chamfer words are",
                f"# pick-two) -- break it with: run Form(tool=<ChamferMill>, surface={g['name']})",
            ]
            claimed[t] = g
            break
    return claimed


def recognize_holes(solid, faces, emap, face_index):
    """The cylinder pass, run BEFORE the pocket pass so an absorbed bore
    floor is never a pocket-floor candidate. Returns (member_of,
    cyl_residue): every full/partial cylinder face is in exactly one --
    grouped into a hole/bore proposal, or carrying its residue dict."""
    member_of, cyl_residue = {}, {}
    for i, f in enumerate(faces, start=1):
        if f.geomType() != "CYLINDER":
            continue
        c = classify_cylinder(f, solid)
        if c["status"] == "proposed":
            g = hole_or_bore_group(i, f, c, faces, solid, emap, face_index)
            for m in g["members"]:
                member_of[m] = g
        else:
            cyl_residue[i] = c
    return member_of, cyl_residue


def column_open_above(solid, geom, z_top):
    """Whether a drill coming down from the blank top meets nothing of the
    part before this bore's rim: rays straight up from points just inside
    the mouth (the axis, a ring at half the radius, a ring just inside the
    wall, twelve round each), started a nudge above the rim. One hit and
    the column is not open. A sample, not a proof; a drill's own diameter
    is what it has to clear, and a face narrower than the ring spacing
    over a hole is not a shape a part has."""
    inter = IntCurvesFace_ShapeIntersector()
    inter.Load(solid.wrapped, LIN_TOL)
    r = geom["dia"] / 2.0
    z = geom["rim"] + REACH_NUDGE
    starts = [(geom["x"], geom["y"])]
    for k in range(12):
        a = 2.0 * math.pi * k / 12.0
        for rr in (r / 2.0, r - REACH_NUDGE):
            starts.append((geom["x"] + rr * math.cos(a), geom["y"] + rr * math.sin(a)))
    for x, y in starts:
        inter.Perform(gp_Lin(gp_Pnt(x, y, z), gp_Dir(0, 0, 1)), 0.0, z_top - z)
        if inter.NbPnt() > 0:
            return False
    return True


def place_sunken_bores(z_top, member_of, solid, blank_top=None):
    """A bore whose rim sits BELOW the stock top is PLACED: its feature line
    gains `z=<rim>`, the key `Feature::CylinderBore` has carried since Phase
    45 (DECISIONS.md -> "DRAFT: Phase 45 opens…", fork 4A). Before that key
    existed this pass REFUSED such a bore (`cylinder_bore_sunken`, 2026-10-09,
    one day in the contract), because a `CylinderBore` with no `z` is
    authored from the stock top and a counterbore on a step at z=35 of a part
    whose top is z=65 would have been machined 30 mm too high, silently.
    Runs after the mouth-round pass, which lifts a filleted mouth's rim to the
    virtual sharp corner, so the rim it writes is the one the grammar means.
    The group keeps its members; the wall's reason names the rim and the
    stock top so a reviewer sees the step the bore sits on. What stands above
    the rim is the step's material: the engine's entry restriction refuses a
    plunge through it until an earlier op has removed it, which is the
    program's business and not recognition's.

    One exception (ROADMAP item 1 slice 8): a drilled hole with a plain
    mouth whose column above the rim is open air to the top proposes FROM
    the top, extent the whole way down. A drill through the blank starts
    where the blank is, and the real part's bolt holes, proposed at the
    pad tops they come out of, could not be drilled before the pads were cut
    without restating every rim by hand. A hole with a mouth chamfer or
    round keeps its rim, which is where that mouth is; a milled bore keeps
    its rim, which is where its wall finish starts."""
    if z_top is None:
        return
    # The blank's top is the stock top the grammar means. With an allowance
    # (slice 11) it stands above the model's top, and a bore whose rim is the
    # model's top is then below it like any sunken bore: drilled from the
    # blank top when plain, placed by z= when its mouth is where its rim is.
    stock_top = z_top if blank_top is None else blank_top
    for g in {id(g): g for g in member_of.values()}.values():
        geom = g.get("geom")
        if geom is None or geom["rim"] >= stock_top - LIN_TOL:
            continue
        at_model_top = geom["rim"] >= z_top - LIN_TOL
        drilled = g["name"].endswith("_Hole")
        plain = "chamfer_depth" not in geom and "edge_round" not in geom
        if drilled and plain and (at_model_top or column_open_above(solid, geom, stock_top)):
            old = geom["depth"]
            geom["depth"] = old + (stock_top - geom["rim"])
            g["part"] = bore_part_line(g["name"], geom)
            g["extras"] = [e.replace(f"extent={fmt(old)}", f"extent={fmt(geom['depth'])}")
                           for e in g["extras"]]
            if at_model_top:
                g["whys"][g["primary"]] += (
                    f"; rim at the model's top z={fmt(geom['rim'])}, {fmt(stock_top - geom['rim'])} "
                    f"below the blank's top z={fmt(stock_top)} (the allowance) -- proposed "
                    f"from the blank top, where a drill through the blank starts, extent "
                    f"{fmt(geom['depth'])} the whole way down")
            else:
                g["whys"][g["primary"]] += (
                    f"; rim at z={fmt(geom['rim'])}, {fmt(stock_top - geom['rim'])} below "
                    f"the stock top z={fmt(stock_top)}, with open air straight above it "
                    f"to the top -- proposed from the top, where a drill through the "
                    f"blank starts, extent {fmt(geom['depth'])} the whole way down; "
                    f"what the model has above the rim is another feature's to cut")
            continue
        geom["z"] = geom["rim"]
        g["part"] = bore_part_line(g["name"], geom)
        if at_model_top:
            g["whys"][g["primary"]] += (
                f"; rim at the model's top z={fmt(geom['rim'])}, {fmt(stock_top - geom['rim'])} "
                f"below the blank's top z={fmt(stock_top)} (the allowance) -- authored from "
                f"its rim by z=, so the allowance above it is the top face's to clear first")
        else:
            g["whys"][g["primary"]] += (
                f"; rim at z={fmt(geom['rim'])}, {fmt(stock_top - geom['rim'])} below "
                f"the stock top z={fmt(stock_top)} -- authored from its rim by z=, so "
                f"the step above it is another feature's material")


def join_split_holes(member_of, faces):
    """A hole the model interrupts is one hole. A groove or a slot across a
    drilled hole leaves two wall faces on one axis with air between, and
    each proposed as its own hole from the top, so the schedule drilled the
    place twice. Two plain drilled holes from the stock top on one axis at
    one diameter are one drill run: the deeper keeps its line and the
    shallower's faces become its members. A hole placed by `z=` (a mouth
    chamfer, a round) and a milled bore are left alone: their rim is where
    something else starts."""
    def plain(g):
        geom = g.get("geom")
        return (geom is not None and g["name"].endswith("_Hole")
                and not {"z", "chamfer_depth", "edge_round"} & set(geom))

    holes = sorted((g for g in {id(g): g for g in member_of.values()}.values() if plain(g)),
                   key=lambda g: -g["geom"]["depth"])
    kept = []
    for g in holes:
        a = g["geom"]
        whole = next((k for k in kept
                      if abs(k["geom"]["x"] - a["x"]) < LIN_TOL
                      and abs(k["geom"]["y"] - a["y"]) < LIN_TOL
                      and abs(k["geom"]["dia"] - a["dia"]) < LIN_TOL), None)
        if whole is None:
            kept.append(g)
            continue
        p, q = whole["primary"], g["primary"]
        lo, hi = sorted((faces[p - 1].BoundingBox(), faces[q - 1].BoundingBox()),
                        key=lambda bb: bb.zmin)
        gap = (f"{fmt(hi.zmin - lo.zmax)} of air between them, z={fmt(lo.zmax)} to "
               f"z={fmt(hi.zmin)}" if hi.zmin - lo.zmax > LIN_TOL else "one above the other")
        for m in g["members"]:
            whole["members"].append(m)
            whole["whys"][m] = g["whys"][m]
            member_of[m] = whole
        whole["whys"][q] = (f"a stretch of recognized hole {whole['name']} (wall F{p}): the "
                            f"same axis and diameter, {gap} -- one drill run cuts both, "
                            f"so it is proposed once")
        whole["whys"][p] += (f"; wall F{q} is the same hole, {gap} -- one hole, drilled "
                             f"once from the top")
        whole["extras"] = whole["extras"] + [
            f"# the model interrupts this hole (walls F{p} and F{q}, {gap}): drilled",
            "# through the uncut blank it is one plain hole; drilled after the gap is cut, the",
            "# drill breaks out and starts again on the far side",
        ]


def recognize_lip_rounds(faces, z_top, member_of, face_index):
    """The mouth-round pass (DECISIONS.md -> "sidecar recognition of a bore's
    mouth round"): a full-revolution toroidal blend tangent to a graduated
    bore's wall below and the top plane above IS that bore's edge_round — the
    surface a corner rounder leaves, arriving from CAD instead of being
    hand-declared. Runs right after the hole pass, so the blend is absorbed
    into the bore's group before any other pass (or the standalone torus
    classifier, which would read the blend's local surface as a ring boss's
    inner quarter and mis-propose a standing ring) can see it.

    Tangency is three numeric checks, not a guess: the torus's inner equator
    radius (major - minor) must be the bore's radius, its equator height must
    be the wall's top, and its tube top (centre z + minor) must be the part's
    top plane. The absorbed group's extent grows to the VIRTUAL SHARP CORNER
    (the round consumes its own radius of wall, and Feature::CylinderBore
    carries the sharp geometry with the round declared on it) — which also
    fixes the honest under-statement a filleted mouth otherwise causes: the
    wall face alone measures short by the round, and an extent measured off
    it would put the floor at the wrong Z."""
    if z_top is None:
        return {}
    claimed = {}
    for t, f in enumerate(faces, start=1):
        if t in member_of or f.geomType() != "TORUS":
            continue
        ad = BRepAdaptor_Surface(f.wrapped)
        if not full_revolution(ad):
            continue
        tor = ad.Torus()
        if abs(tor.Axis().Direction().Z()) < COS_TOL:
            continue
        ctr = tor.Location()
        minor, major = tor.MinorRadius(), tor.MajorRadius()
        if abs(ctr.Z() + minor - z_top) > LIN_TOL:
            continue  # not tangent to the top plane
        for g in {id(g): g for g in member_of.values()}.values():
            geom = g.get("geom")
            if geom is None:
                continue
            if (abs(geom["x"] - ctr.X()) > LIN_TOL
                    or abs(geom["y"] - ctr.Y()) > LIN_TOL):
                continue
            if abs((major - minor) - geom["dia"] / 2.0) > LIN_TOL:
                continue
            if abs(geom["wall_top"] - ctr.Z()) > LIN_TOL:
                continue  # not tangent to the wall's own top

            fid = f"F{t}"
            old_extent = geom["depth"]
            new_extent = old_extent + minor
            g["members"].append(t)
            g["whys"][t] = (f"mouth round of recognized bore {g['name']} "
                            f"(wall F{g['primary']}): a full-revolution "
                            f"toroidal blend, tangent to the wall and the "
                            f"top plane -- proposed as edge_round, not a "
                            f"standalone ring")
            g["whys"][g["primary"]] += (
                f"; mouth blend {fid} absorbed -- extent runs to the virtual "
                f"sharp corner ({fmt(new_extent)}), not the wall face's own "
                f"top ({fmt(old_extent)})")
            geom["rim"] = ctr.Z() + minor  # the virtual sharp corner: the top plane
            geom["depth"] = new_extent
            geom["edge_round"] = minor
            g["part"] = bore_part_line(g["name"], geom)
            # Any extras that baked the measured extent restate the new one.
            g["extras"] = [e.replace(f"extent={fmt(old_extent)}",
                                     f"extent={fmt(new_extent)}")
                           for e in g["extras"]]
            g["extras"].append(
                f"# break the lip with: run Form(tool=..., "
                f"surface={g['name']})  [the tool must be a CornerRounder "
                f"whose ground arc is R{fmt(minor)} -- the pairing refusal "
                f"reads the declared radius]")
            geom["depth"] = new_extent
            claimed[t] = g
            break
    return claimed


def classify_sphere(face):
    """Graduation check for spheres: convex (material inside -- a dome/boss),
    full revolution, entirely at or above its equator plane. The sub-equator
    band is undercut relative to the tool axis; a concave face is a bowl
    (CHECKLIST row 3.1, todo). Feature::Sphere is a dome rising from its
    equator plane by construction, so the checks ARE that definition."""
    ad = BRepAdaptor_Surface(face.wrapped)
    sph = ad.Sphere()
    ctr = sph.Location()

    if not full_revolution(ad):
        return residue(code="sphere_partial", row="no row",
                       reason="partial-revolution spherical face (an edge blend "
                              "/ corner fillet) -- part of a larger region, not "
                              "a standalone dome")
    pnt, n = face_normal(face)
    if n.Dot(gp_Vec(pnt.X() - ctr.X(), pnt.Y() - ctr.Y(), pnt.Z() - ctr.Z())) < 0.0:
        return residue(code="sphere_concave", row="3.1",
                       reason="concave spherical face (a bowl cavity) -- bowl "
                              "finishing is CHECKLIST row 3.1, todo; "
                              "finish_sphere is dome-only by construction")
    if face.BoundingBox().zmin < ctr.Z() - LIN_TOL:
        return residue(code="sphere_below_equator", row="3.3",
                       reason="convex spherical face extends below its equator "
                              "-- the sub-equator band is undercut relative to "
                              "the tool axis (CHECKLIST row 3.3: dome half only)")

    return dict(status="proposed", checklist_row="3.2",
                why="spherical, full revolution, material inside (a dome boss), "
                    "entirely at/above its equator",
                dia=2.0 * sph.Radius(), x=ctr.X(), y=ctr.Y(), z=ctr.Z())


def classify_cone(face):
    """Graduation check for cones: axis parallel +Z, full revolution, material
    inside (a boss), base down with the face reaching the apex -- exactly
    Feature::Cone's base-down point-up boss. A widening-upward face (inside or
    outside) is undercut; a point-down cavity is rows 2.14/2.18 (todo); a
    frustum stops short of what Feature::Cone can declare."""
    ad = BRepAdaptor_Surface(face.wrapped)
    cone = ad.Cone()
    axis = cone.Axis().Direction()

    if abs(axis.Z()) < COS_TOL:
        return residue(code="cone_axis_tilted", row="2.20",
                       reason="conical face with axis not parallel to +Z "
                              "(oblique cone, CHECKLIST row 2.20)")
    if not full_revolution(ad):
        return residue(code="cone_partial", row="2.8",
                       reason="partial-revolution conical face -- part of a "
                              "general profile; conical blends have no "
                              "proposable primitive")

    apex = cone.Apex()
    bb = face.BoundingBox()
    pnt, n = face_normal(face)
    # The axis is vertical here, so the apex carries the axis' XY.
    radial = gp_Vec(pnt.X() - apex.X(), pnt.Y() - apex.Y(), 0.0)
    if n.Dot(radial) < 0.0:
        # Material outside the surface: an internal conical wall.
        if apex.Z() <= bb.zmin + LIN_TOL:
            return residue(code="cone_cavity_point_down", row="2.14",
                           reason="internal conical wall narrowing downward (a "
                                  "point-down cavity) -- CHECKLIST rows "
                                  "2.14/2.18 are todo")
        return residue(code="cone_cavity_undercut", row="no row",
                       reason="internal conical wall widening downward -- "
                              "undercut relative to the tool axis")
    if apex.Z() <= bb.zmin + LIN_TOL:
        return residue(code="cone_boss_undercut", row="no row",
                       reason="conical boss widening upward (apex below its "
                              "base) -- undercut relative to the tool axis")
    # A boss face that stops short of the apex is a frustum, which
    # Feature::Cone (a full base-to-apex boss) cannot declare. The bounding
    # box carries kernel inflation, so the slop here is coarser than LIN_TOL.
    if apex.Z() > bb.zmax + 10.0 * LIN_TOL:
        return residue(code="cone_frustum", row="2.12",
                       reason="conical frustum (face stops short of the apex) "
                              "-- Feature::Cone declares a full base-to-apex "
                              "boss; frustum recognition has not graduated")

    height = apex.Z() - bb.zmin
    return dict(status="proposed", checklist_row="2.12",
                why="conical, axis +Z, full revolution, material inside "
                    "(a boss), base down, apex up",
                dia=2.0 * math.tan(cone.SemiAngle()) * height, height=height,
                x=apex.X(), y=apex.Y(), z=bb.zmin)


def classify_torus(face, emap=None):
    """Graduation check for tori: axis parallel +Z, full revolution, material
    inside the tube (a ring boss), entirely at or above the equator plane --
    Feature::Torus's finishing stops at the equator by construction, and the
    underside is undercut relative to the tool axis.

    A LIP BLEND is refused by adjacency, and the check exists because the
    material-side test alone cannot make the distinction: a mouth round on a
    bore or boss is locally a ring boss's inner-upper quarter (same surface,
    same normals), and without this guard an unmatched blend graduates as a
    standing ring that is not there. A standing ring sits on planes; a blend
    rides a wall — so a toroidal face sharing an edge with a CYLINDER is a
    blend on adjacent geometry, never a standalone ring."""
    ad = BRepAdaptor_Surface(face.wrapped)
    tor = ad.Torus()
    axis = tor.Axis().Direction()
    ctr = tor.Location()

    if abs(axis.Z()) < COS_TOL:
        return residue(code="torus_axis_tilted", row="no row",
                       reason="toroidal face with axis not parallel to +Z -- "
                              "no tilted-torus primitive")
    if not full_revolution(ad):
        return residue(code="torus_partial", row="no row",
                       reason="partial-revolution toroidal face (an edge blend "
                              "/ fillet) -- part of a larger region, not a "
                              "standalone ring")
    if emap is not None:
        for edge in face.Edges():
            for anc in emap.FindFromKey(edge.wrapped):
                if not anc.IsSame(face.wrapped) and \
                        type(face)(anc).geomType() == "CYLINDER":
                    return residue(
                        code="torus_lip_blend", row="3.14",
                        reason="toroidal blend riding a cylindrical wall (a "
                               "lip round) -- recognized as a declared "
                               "edge_round only on a graduated bore's mouth; "
                               "a boss lip waits for the cap-anchored "
                               "grouping to step across a tangent ring")

    # Material side: the outward normal of a ring boss points away from the
    # tube's core circle; an internal groove/fillet's points toward it.
    pnt, n = face_normal(face)
    dx, dy = pnt.X() - ctr.X(), pnt.Y() - ctr.Y()
    d = math.hypot(dx, dy)
    tube = gp_Pnt(ctr.X() + tor.MajorRadius() * dx / d,
                  ctr.Y() + tor.MajorRadius() * dy / d, ctr.Z())
    if n.Dot(gp_Vec(pnt.X() - tube.X(), pnt.Y() - tube.Y(), pnt.Z() - tube.Z())) < 0.0:
        return residue(code="torus_internal", row="no row",
                       reason="toroidal face with material outside the tube "
                              "(an internal groove / fillet) -- no "
                              "internal-torus primitive")
    if face.BoundingBox().zmin < ctr.Z() - LIN_TOL:
        return residue(code="torus_below_equator", row="3.13",
                       reason="toroidal face extends below the equator plane "
                              "-- the underside is undercut relative to the "
                              "tool axis (CHECKLIST row 3.13: upper half only)")

    return dict(status="proposed", checklist_row="3.12",
                why="toroidal, axis +Z, full revolution, material inside the "
                    "tube (a ring boss), entirely at/above its equator plane",
                major_dia=2.0 * tor.MajorRadius(),
                minor_dia=2.0 * tor.MinorRadius(),
                x=ctr.X(), y=ctr.Y(), z=ctr.Z())


class Refusal(Exception):
    """A grouping candidate (pocket floor, outer-profile cap, trim wire)
    failing one of its graduation checks. It carries exactly what a residue
    entry carries -- the check's `code`, the `reason`, the row, and the
    `refs` the reason names -- so the code travels with the sentence to
    wherever the refusal lands: the candidate's own entry (`entry()`), or
    every face of the refused candidate with the face's role prefixed
    (`rewrap()`). The other faces keep their own per-face classifications
    (which stay true statements)."""

    def __init__(self, code, reason, row="no row", refs=()):
        super().__init__(reason)
        self.code = code
        self.reason = reason
        self.row = row
        self.refs = list(refs)

    def entry(self):
        """The refusal as the candidate's own residue entry."""
        return residue(self.code, self.row, self.reason, self.refs)

    def rewrap(self, role, prefix, refs=()):
        """The refusal re-voiced on another face of the refused candidate:
        `code` becomes `role/check` (CONTRACT.md "Residue codes" -- the role
        this face played, then the inner check, one slash, never nested),
        the sentence gains its prefix, and the face ids the prefix names
        lead the inner refs."""
        return residue(f"{role}/{self.code}", self.row,
                       f"{prefix}: {self.reason}", list(refs) + self.refs)


def edge_polyline(edge):
    """Points along one boundary edge in wire-traversal order, EXCLUDING the
    start point (it is the previous edge's end, so a closed wire yields each
    vertex exactly once). Lines contribute their end vertex; circular arcs
    tessellate at CHORD_TOL. Returns (points, is_arc); anything that is not
    a line or circular arc refuses the pocket (CHECKLIST row 2.5)."""
    c = BRepAdaptor_Curve(edge)
    t0, t1 = c.FirstParameter(), c.LastParameter()
    if edge.Orientation() == TopAbs_Orientation.TopAbs_REVERSED:
        t0, t1 = t1, t0
    kind = c.GetType()
    if kind == GeomAbs_CurveType.GeomAbs_Line:
        p = c.Value(t1)
        return [(p.X(), p.Y())], False
    if kind == GeomAbs_CurveType.GeomAbs_Circle:
        # Segment count from the exact chord-error formula: a chord across
        # arc angle theta on radius r has sagitta r*(1 - cos(theta/2)).
        r = c.Circle().Radius()
        theta_max = 2.0 * math.acos(max(-1.0, 1.0 - CHORD_TOL / r))
        n = max(1, math.ceil(abs(t1 - t0) / theta_max))
        pts = []
        for k in range(1, n + 1):
            p = c.Value(t0 + (t1 - t0) * k / n)
            pts.append((p.X(), p.Y()))
        return pts, True
    raise Refusal(code="edge_not_line_or_arc", row="2.5",
                  reason="boundary edge is not a line or circular arc -- "
                         "analytic-profile contours (CHECKLIST row 2.5) have "
                         "not graduated")


def face_edge_points(edge):
    """`edge_polyline` for a FACE's boundary: lines and circular arcs exactly as
    there, and ANY other edge curve (ellipse, B-spline, ...) chorded at
    CHORD_TOL by bisection on the curve's own parameter. A pocket or boss WALL
    still refuses such an edge (`edge_not_line_or_arc`) because there the chord
    would BE the surface; a face's boundary is overhung by a tool radius or
    stood off from, so the chord is nothing (DECISIONS.md -> "Phase 28 opens:
    the patterns DRAFT", fork 1A)."""
    try:
        return edge_polyline(edge)[0]
    except Refusal:
        pass
    c = BRepAdaptor_Curve(edge)
    t0, t1 = c.FirstParameter(), c.LastParameter()
    if edge.Orientation() == TopAbs_Orientation.TopAbs_REVERSED:
        t0, t1 = t1, t0

    def xy(t):
        q = c.Value(t)
        return (q.X(), q.Y())

    def split(ta, a, tb, b, depth):
        tm = 0.5 * (ta + tb)
        m = xy(tm)
        ex, ey = b[0] - a[0], b[1] - a[1]
        length = math.hypot(ex, ey)
        dev = (abs((m[0] - a[0]) * ey - (m[1] - a[1]) * ex) / length
               if length > 1e-12 else math.hypot(m[0] - a[0], m[1] - a[1]))
        # Always halve a few times first: a symmetric span's midpoint can sit
        # ON the chord while the curve bulges either side of it.
        if depth >= 3 and (dev <= CHORD_TOL or depth >= 16):
            return [b]
        return split(ta, a, tm, m, depth + 1) + split(tm, m, tb, b, depth + 1)

    return split(t0, xy(t0), t1, xy(t1), 0)


def face_loops(face):
    """A planar face's boundary as `Face` loops: the outer wire first, wound
    CCW; every inner wire after it, wound CW. Deterministic: each loop starts
    at its lexicographically smallest rounded (x, y), and the inner loops are
    ordered by that start."""
    def loop_of(wire):
        pts = []
        wexp = BRepTools_WireExplorer(wire, face.wrapped)
        while wexp.More():
            pts += face_edge_points(wexp.Current())
            wexp.Next()
        return pts

    def normalized(pts, ccw):
        if (signed_area(pts) > 0.0) != ccw:
            pts.reverse()
        k = min(range(len(pts)), key=lambda i: (round(pts[i][0], 3), round(pts[i][1], 3)))
        return pts[k:] + pts[:k]

    outer = normalized(loop_of(BRepTools.OuterWire_s(face.wrapped)), True)
    inner = [normalized(loop_of(w.wrapped), False) for w in face.innerWires()]
    inner.sort(key=lambda l: (round(l[0][0], 3), round(l[0][1], 3)))
    return [outer] + inner


def face_part(name, z, loops):
    lines = [f"feature {name} Face(z={fmt(z)}"]
    for l in loops:
        lines.append("    loop")
        lines += [f"        {fmt(x)} {fmt(y)}" for x, y in l]
    return "\n".join(lines + [")"])


def drop_collinear(pts):
    """The closed loop without the points that lie on the segment between
    their neighbours (within LIN_TOL of it)."""
    out = list(pts)
    changed = True
    while changed and len(out) > 3:
        changed = False
        for i in range(len(out)):
            (ax, ay), (bx, by), (cx, cy) = out[i - 1], out[i], out[(i + 1) % len(out)]
            ux, uy = cx - ax, cy - ay
            length = math.hypot(ux, uy)
            if length < LIN_TOL:
                continue
            dist = abs(ux * (by - ay) - uy * (bx - ax)) / length
            along = ((bx - ax) * ux + (by - ay) * uy) / (length * length)
            if dist < LIN_TOL and -LIN_TOL < along < 1.0 + LIN_TOL:
                del out[i]
                changed = True
                break
    return out


def signed_area(pts):
    """Twice-signed-area shoelace over a closed 2D loop; positive means CCW."""
    a = 0.0
    for (x0, y0), (x1, y1) in zip(pts, pts[1:] + pts[:1]):
        a += x0 * y1 - x1 * y0
    return a / 2.0


def strip_of(face):
    """A chamfer STRIP, if this face is one (Phase 46 slice 3): a planar face
    that is neither vertical nor +-Z, or a cone with its axis parallel to +Z
    (the bevel round a rounded corner or a round mouth). Returns the included
    angle in degrees -- the engine's `chamfer_angle` convention, the angle the
    ChamferMill grinds: twice the flank's angle from the vertical -- or None.
    The engine holds one leg and that angle; the slice writes what the
    model measures and the reviewer retypes what the print says."""
    kind = face.geomType()
    if kind == "PLANE":
        nz = abs(face_normal(face)[1].Z())
        # Neither vertical (nz under 1 - COS_TOL) nor +-Z (nz over COS_TOL).
        if 1.0 - COS_TOL <= nz <= COS_TOL:
            return math.degrees(2.0 * math.asin(nz))
        return None
    if kind == "CONE":
        ad = BRepAdaptor_Surface(face.wrapped)
        if abs(ad.Cone().Axis().Direction().Z()) >= COS_TOL:
            return math.degrees(2.0 * abs(ad.Cone().SemiAngle()))
    return None


def across_edges(face, faces, emap, face_index, at_bottom):
    """The faces across this face's BOTTOM edges (`at_bottom`) or TOP edges --
    the edges lying entirely within a tolerance of its zmin or zmax."""
    bb = face.BoundingBox()
    out = []
    for edge in face.Edges():
        eb = edge.BoundingBox()
        on = (eb.zmax <= bb.zmin + 10.0 * LIN_TOL) if at_bottom else (eb.zmin >= bb.zmax - 10.0 * LIN_TOL)
        if not on:
            continue
        for anc in emap.FindFromKey(edge.wrapped):
            if anc.IsSame(face.wrapped):
                continue
            j = face_index(anc)
            if j not in out:
                out.append(j)
    return out


def edge_points(edge):
    """`edge_polyline` with the START vertex included -- a full polyline, for
    chaining edges that did not come off one wire in order."""
    c = BRepAdaptor_Curve(edge)
    t0, t1 = c.FirstParameter(), c.LastParameter()
    if edge.Orientation() == TopAbs_Orientation.TopAbs_REVERSED:
        t0, t1 = t1, t0
    p = c.Value(t0)
    pts, arc = edge_polyline(edge)
    return [(p.X(), p.Y())] + pts, arc


def wall_top_points(edge, wall):
    """`edge_points` for an edge on a WALL's top boundary. A line or a circular
    arc reads as everywhere. Any other curve -- the ellipse where a bevel's
    plane meets a cylindrical wall's rounded end (the real part's ribs) --
    lies ON the wall's surface, so in plan it lies on the wall's own line or
    circle: a planar wall's is the straight segment between its ends, exact;
    a cylindrical wall's is chorded at CHORD_TOL along the curve, every point
    on the circle. Neither is a chord standing in for a surface, which is
    what `edge_not_line_or_arc` refuses elsewhere."""
    try:
        return edge_points(edge)
    except Refusal:
        pass
    c = BRepAdaptor_Curve(edge)
    t0, t1 = c.FirstParameter(), c.LastParameter()
    if edge.Orientation() == TopAbs_Orientation.TopAbs_REVERSED:
        t0, t1 = t1, t0
    p0 = c.Value(t0)
    start = [(p0.X(), p0.Y())]
    if wall.geomType() == "PLANE":
        p1 = c.Value(t1)
        return start + [(p1.X(), p1.Y())], False
    return start + face_edge_points(edge), True


def wall_under_edge(a, b, pieces_by_wall):
    """The wall whose top edge a loop edge lies on: the one whose chained
    top points pass nearest the edge's midpoint. The loop's edges ARE the
    walls' top edges (chain_loop joined them, drop_collinear merged points
    along one wall's line), so the nearest is on it to within chord
    tolerance."""
    mx, my = (a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0

    def seg_dist(p, q):
        dx, dy = q[0] - p[0], q[1] - p[1]
        if dx == 0.0 and dy == 0.0:
            return math.hypot(mx - p[0], my - p[1])
        t = ((mx - p[0]) * dx + (my - p[1]) * dy) / (dx * dx + dy * dy)
        t = min(1.0, max(0.0, t))
        return math.hypot(mx - (p[0] + t * dx), my - (p[1] + t * dy))

    best = None
    for w, pieces in pieces_by_wall.items():
        for piece in pieces:
            for p, q in zip(piece, piece[1:]):
                d = seg_dist(p, q)
                if best is None or d < best[0]:
                    best = (d, w)
    return best[1] if best else None


def chain_loop(polylines, reach=0.0):
    """Chain full polylines into one closed loop by matching endpoints (a
    polyline joins reversed when its END meets the chain). Returns the loop
    without its closing repeat, or None when the pieces do not close.

    With a `reach`, two pieces whose ends do not meet join at the CORNER
    their end segments make when carried on, if that corner lies within the
    reach of both ends: two walls with a blend between them the loop does
    not follow (the round along a bevel's end), read as the sharp corner."""
    def same(a, b):
        return abs(a[0] - b[0]) <= 10.0 * LIN_TOL and abs(a[1] - b[1]) <= 10.0 * LIN_TOL

    def corner(a0, a1, b0, b1):
        # Where the line a0->a1, carried past a1, meets the line b1->b0,
        # carried back past b0.
        dax, day = a1[0] - a0[0], a1[1] - a0[1]
        dbx, dby = b1[0] - b0[0], b1[1] - b0[1]
        den = dax * dby - day * dbx
        if abs(den) <= 1e-9 * math.hypot(dax, day) * math.hypot(dbx, dby):
            return None
        t = ((b0[0] - a1[0]) * dby - (b0[1] - a1[1]) * dbx) / den
        c = (a1[0] + t * dax, a1[1] + t * day)
        if t < 0.0 or math.dist(c, a1) > reach or math.dist(c, b0) > reach:
            return None
        return c
    pool = [list(p) for p in polylines if len(p) >= 2]
    if not pool:
        return None
    chain = pool.pop(0)
    while pool:
        hit = None
        for k, p in enumerate(pool):
            if same(p[0], chain[-1]):
                hit, piece = k, p[1:]
            elif same(p[-1], chain[-1]):
                hit, piece = k, p[-2::-1]
            if hit is not None:
                break
        if hit is None and reach > 0.0:
            for k, p in enumerate(pool):
                for q in (p, p[::-1]):
                    c = corner(chain[-2], chain[-1], q[0], q[1])
                    if c is not None:
                        hit, piece = k, [c] + q
                        break
                if hit is not None:
                    break
        if hit is None:
            return None
        pool.pop(hit)
        chain += piece
    if not same(chain[0], chain[-1]):
        c = corner(chain[-2], chain[-1], chain[0], chain[1]) if reach > 0.0 else None
        return None if c is None else chain + [c]
    return chain[:-1]


def see_through(w, faces, emap, face_index, below):
    """A strip's partner across its lower (`below`) or upper edges -- the ONE
    vertical wall (plane or cylinder) it breaks the edge of. Raises Refusal
    `chamfer_consumes_wall` where the strip meets a +Z plane instead (the
    bevel is the whole wall: the engine's own refusal), and
    `chamfer_at_foot` where a strip sits at a wall's foot (the engine's
    chamfer is a top-edge break and has no key for it)."""
    wf = faces[w - 1]
    if below:
        # Every face across an edge that TOUCHES the strip's foot, not only
        # the edges lying along it: a vertical corner fillet whose top rises
        # above the strips' feet (the real part's ribs, ROADMAP item 1 slice
        # 5) meets the strip along a short rising curve, and is a wall of
        # the loop all the same.
        bb = wf.BoundingBox()
        partners = []
        for edge in wf.Edges():
            if edge.BoundingBox().zmin > bb.zmin + 10.0 * LIN_TOL:
                continue
            for anc in emap.FindFromKey(edge.wrapped):
                if anc.IsSame(wf.wrapped):
                    continue
                j = face_index(anc)
                if j not in partners:
                    partners.append(j)
    else:
        partners = across_edges(wf, faces, emap, face_index, below)
    walls = []
    for j in partners:
        pf = faces[j - 1]
        if strip_of(pf) is not None:
            continue  # the next strip round the corner, not a wall
        kind = pf.geomType()
        if kind == "PLANE":
            nz = face_normal(pf)[1].Z()
            if abs(nz) < 1.0 - COS_TOL:
                walls.append(j)
            elif nz > COS_TOL:
                # The same face reads two ways and the reader cannot tell
                # them apart: a draft wall (CHECKLIST row 1.2) and a bevel as
                # deep as the wall it would break. Both are said; the engine
                # has a primitive for neither.
                depth = wf.BoundingBox().zmax - wf.BoundingBox().zmin
                raise Refusal(code="chamfer_consumes_wall", row="1.2",
                              reason=f"F{w} is a {fmt(strip_of(wf))}deg-included oblique face "
                                     f"spanning {fmt(depth)}mm from the floor to the surrounding "
                                     f"plane with nothing vertical under it: a draft wall (CHECKLIST "
                                     f"row 1.2, no tilted-plane primitive) or a bevel as deep as its "
                                     f"wall (the engine's chamfer needs a flank below it)",
                              refs=[f"F{w}"])
        elif kind == "CYLINDER" and upright(pf):
            walls.append(j)
    return walls


def upright(face):
    """A wall a tool's flank cuts: a vertical plane or a cylinder standing
    on the tool's axis."""
    kind = face.geomType()
    if kind == "PLANE":
        return abs(face_normal(face)[1].Z()) < 1.0 - COS_TOL
    if kind == "CYLINDER":
        return abs(BRepAdaptor_Surface(face.wrapped).Cylinder().Axis().Direction().Z()) >= COS_TOL
    return False


def top_round(face, z_cap):
    """The radius of a quarter-round that ends at a cap's height: the round
    on a boss's top edge, between the cap and the wall under it. None for
    anything else."""
    bb = face.BoundingBox()
    if abs(bb.zmax - z_cap) > 10.0 * LIN_TOL:
        return None
    return blend_radius(face, bb.zmin, axis_on_top=False)


def blend_radius(face, z_low, axis_on_top):
    """The radius of a quarter-round blend that rises one radius from
    `z_low`: a cylinder with a level axis along a straight edge, a torus
    with an upright axis round a corner. None for anything else. A floor's
    fillet has its axis at its top, one radius above the floor; a mouth's
    round has its axis at its foot, the wall's top."""
    kind = face.geomType()
    ad = BRepAdaptor_Surface(face.wrapped)
    if kind == "CYLINDER":
        cyl = ad.Cylinder()
        if abs(cyl.Axis().Direction().Z()) > 1.0 - COS_TOL:
            return None
        r, zc = cyl.Radius(), cyl.Location().Z()
    elif kind == "TORUS":
        tor = ad.Torus()
        if abs(tor.Axis().Direction().Z()) < COS_TOL:
            return None
        r, zc = tor.MinorRadius(), tor.Location().Z()
    else:
        return None
    bb = face.BoundingBox()
    tol = 10.0 * LIN_TOL
    if abs(bb.zmin - z_low) > tol or abs(bb.zmax - z_low - r) > tol:
        return None
    if abs(zc - (z_low + r if axis_on_top else z_low)) > tol:
        return None
    return r


def pocket_group(floor, floor_idx, faces, solid, emap, face_index, blank_side=0.0):
    """Graduation checks + boundary extraction for one pocket-floor
    candidate. Returns the group dict on success; raises Refusal naming the
    failed check otherwise. The checks ARE the pocket's orientation/undercut
    gate (the Phase 4 co-requisite), mirroring the per-face classifiers.
    `blank_side` is how far the blank stands outside the model on its sides,
    which an open side's loop has to pass."""
    z_floor = face_normal(floor)[0].Z()

    def is_air(x, y, z):
        return (BRepClass3d_SolidClassifier(solid.wrapped, gp_Pnt(x, y, z), LIN_TOL).State()
                == TopAbs_State.TopAbs_OUT)

    def falls_away(edge):
        """The plan direction out of the floor across this straight edge, if
        the surface past it drops away: just outside the edge, a point a
        little under the floor's height is in air. None if anything stands
        or lies level there."""
        c = BRepAdaptor_Curve(edge)
        if c.GetType() != GeomAbs_CurveType.GeomAbs_Line:
            return None
        d = c.Line().Direction()
        length = math.hypot(d.X(), d.Y())
        if length < LIN_TOL:
            return None
        nx, ny = d.Y() / length, -d.X() / length
        t0, t1 = c.FirstParameter(), c.LastParameter()
        out = None
        for frac in (0.25, 0.5, 0.75):
            p = c.Value(t0 + (t1 - t0) * frac)
            sides = [s for s in (1.0, -1.0)
                     if is_air(p.X() + s * nx * REACH_NUDGE, p.Y() + s * ny * REACH_NUDGE,
                               z_floor - 0.1 * REACH_NUDGE)]
            if len(sides) != 1 or (out is not None and sides[0] != out):
                return None
            out = sides[0]
        return (out * nx, out * ny)

    # Check 1 -- RETIRED (Phase 26 slice 4). A floor with inner wires used to
    # refuse here ("island pocketing is not supported"): ops::clear cleared
    # the island-free interior only. It clears AROUND islands now, and an
    # island is never declared -- the engine derives it from containment
    # (DECISIONS.md -> "Phase 10 lane D -- overlap semantics"). So the floor
    # proposes its OUTER loop as the pocket, exactly as before, and whatever
    # stands on an inner wire proposes beside it through the classifiers that
    # already recognize it (a full-revolution convex cylinder -> CylinderBoss,
    # a capped vertical wall loop -> ProfileBoss). Nothing is simplified away:
    # a standing shape this reader cannot name stays `unsupported` on its own
    # faces, and a proposal that leaves it out is a pocket the engine would
    # clear THROUGH it -- which is why the count is said out loud below.
    n_islands = len(floor.innerWires())

    # What a face on the floor's wire IS (Phase 46 slice 2) -- the three
    # kinds the engine's region already has, read here from the solid:
    #   "wall"        something standing beside the floor (every check below
    #                 is about these);
    #   "silhouette"  the part's OUTSIDE -- a full-revolution cylinder with
    #                 material inside it spanning the floor's height: the
    #                 blank's edge, open beyond; the loop keeps its arc,
    #                 drawn SILHOUETTE_PUSH past the circle;
    #   "void"        a bore that pierces the floor's edge from BELOW (a
    #                 full-revolution cylinder, material outside, bottom
    #                 under the floor): the loop runs along its rim, and the
    #                 bore proposes beside the pocket on its own.
    # A full-revolution bore whose bottom IS this floor stays a "wall" here
    # and refuses in check 3 as the bore's floor, as it always has.
    def wall_kind(w):
        wf = faces[w - 1]
        if wf.geomType() != "CYLINDER":
            return "wall"
        ad = BRepAdaptor_Surface(wf.wrapped)
        if abs(ad.Cylinder().Axis().Direction().Z()) < COS_TOL or not full_revolution(ad):
            return "wall"
        bb = wf.BoundingBox()
        if bb.zmin >= z_floor - LIN_TOL:
            return "wall"
        loc = ad.Cylinder().Axis().Location()
        pnt, n = face_normal(wf)
        radial = gp_Vec(pnt.X() - loc.X(), pnt.Y() - loc.Y(), 0.0)
        if n.Dot(radial) > 0.0:
            return "silhouette" if bb.zmax > z_floor + LIN_TOL else "wall"
        return "void"

    def pushed_out(w, ept):
        cyl = BRepAdaptor_Surface(faces[w - 1].wrapped).Cylinder()
        loc, r = cyl.Axis().Location(), cyl.Radius()
        k = (r + SILHOUETTE_PUSH) / r
        return [(loc.X() + (x - loc.X()) * k, loc.Y() + (y - loc.Y()) * k) for x, y in ept]

    # Walk the outer wire in connected order, collecting the 2D loop (check
    # 2 -- lines and circular arcs only -- lives in edge_polyline) and the
    # wall face owning each boundary edge (the other ancestor across it).
    #
    # Two more things a face on the wire can be (ROADMAP item 9):
    #   a floor fillet  a quarter-round blend between the floor and a wall.
    #                   The wall is the face across the blend's top edge and
    #                   the loop runs along that edge, the sharp corner: the
    #                   grammar has no floor radius, so the note says what
    #                   leaves the fillet;
    #   a falling side  a face that drops away from the floor's edge (a
    #                   bevel the pocket runs out onto). Nothing stands
    #                   there, so the side is open and the loop is drawn
    #                   straight out past the blank, where the engine reads
    #                   an edge as open.
    segs, wall_idxs, had_arc = [], [], False  # segs: (points start to end, outward or None)
    kinds, fillets, falling = {}, {}, {}
    wexp = BRepTools_WireExplorer(BRepTools.OuterWire_s(floor.wrapped),
                                  floor.wrapped)
    while wexp.More():
        edge = wexp.Current()
        ept, arc = edge_points(edge)
        had_arc = had_arc or arc
        wall = None
        for anc in emap.FindFromKey(edge):
            if not anc.IsSame(floor.wrapped):
                wall = face_index(anc)
        if wall is None:
            raise Refusal(code="pocket_edge_unwalled",
                          reason="pocket boundary edge has no adjacent wall "
                                 "face (non-manifold or seam edge)")
        outward = None
        wf = faces[wall - 1]
        r = blend_radius(wf, z_floor, axis_on_top=True)
        upright = (wf.geomType() == "PLANE" and abs(face_normal(wf)[1].Z()) < 1.0 - COS_TOL) or (
            wf.geomType() == "CYLINDER"
            and abs(BRepAdaptor_Surface(wf.wrapped).Cylinder().Axis().Direction().Z()) >= COS_TOL)
        if r is not None and not upright:
            tops = [e for e in wf.Edges() if e.BoundingBox().zmin >= z_floor + r - 10.0 * LIN_TOL]
            behind = across_edges(wf, faces, emap, face_index, at_bottom=False)
            if len(tops) != 1 or len(behind) != 1:
                raise Refusal(code="pocket_floor_fillet_split", row="2.1",
                              reason=f"floor fillet F{wall} does not meet one wall along one "
                                     "edge -- the wall behind it cannot be read",
                              refs=[f"F{wall}"])
            top, arc = edge_points(tops[0].wrapped)
            had_arc = had_arc or arc
            # The blend's own edge, turned to run the way the floor's does.
            if math.dist(top[0], ept[-1]) < math.dist(top[-1], ept[-1]):
                top.reverse()
            ept = top
            fillets[wall] = r
            wall = behind[0]
        elif not upright:
            outward = falls_away(edge)
            if outward is not None:
                falling[wall] = True
        if outward is None:
            if wall not in wall_idxs:
                wall_idxs.append(wall)
                kinds[wall] = wall_kind(wall)
            if kinds[wall] == "silhouette":
                ept = pushed_out(wall, ept)
        segs.append((ept, outward))
        wexp.Next()

    # The loop: each edge without its start point (the edge before ends
    # there), and a falling side replaced by the two wall ends beside it
    # carried straight out past the blank.
    bb_all = solid.BoundingBox()
    corners = [(x, y) for x in (bb_all.xmin, bb_all.xmax) for y in (bb_all.ymin, bb_all.ymax)]

    def carried_out(p, n):
        reach = max((cx - p[0]) * n[0] + (cy - p[1]) * n[1] for cx, cy in corners)
        d = reach + blank_side + SILHOUETTE_PUSH
        return (p[0] + n[0] * d, p[1] + n[1] * d)

    pts = []
    n_segs = len(segs)
    if n_segs and all(s[1] is not None for s in segs):
        raise Refusal(code="pocket_floor_unwalled", row="1.1",
                      reason="every edge of this floor falls away -- a face with nothing "
                             "standing beside it, not a pocket")
    for k, (ept, outward) in enumerate(segs):
        if outward is None:
            pts += ept[1:]
            continue
        before = segs[k - 1]
        if before[1] is not None:
            continue  # the run's first edge drew it
        j = k
        while segs[(j + 1) % n_segs][1] is not None:
            j += 1
            if math.dist(segs[j % n_segs][1], outward) > LIN_TOL:
                raise Refusal(code="pocket_open_side_bent", row="1.2",
                              reason="the side this floor runs out on is not one straight "
                                     "edge -- only a straight open side is drawn out to the blank")
        p, q = before[0][-1], segs[(j + 1) % n_segs][0][0]
        p_out, q_out = carried_out(p, outward), carried_out(q, outward)
        # What the loop now holds past the floor must be air above the
        # floor: the part has nothing there for the clearing to cut.
        lift = z_floor + max(fillets.values(), default=0.0) + 10.0 * LIN_TOL
        inter = IntCurvesFace_ShapeIntersector()
        inter.Load(solid.wrapped, LIN_TOL)
        for a in range(8):
            for b in range(8):
                u, v = (a + 0.5) / 8.0, (b + 0.5) / 8.0
                x0, y0 = p[0] + (q[0] - p[0]) * u, p[1] + (q[1] - p[1]) * u
                x1, y1 = p_out[0] + (q_out[0] - p_out[0]) * u, p_out[1] + (q_out[1] - p_out[1]) * u
                x, y = x0 + (x1 - x0) * v, y0 + (y1 - y0) * v
                if not (bb_all.xmin < x < bb_all.xmax and bb_all.ymin < y < bb_all.ymax):
                    continue
                inter.Perform(gp_Lin(gp_Pnt(x, y, lift), gp_Dir(0, 0, 1)), 0.0, bb_all.zmax - lift + 1.0)
                if not is_air(x, y, lift) or inter.NbPnt() > 0:
                    raise Refusal(code="pocket_open_side_blocked", row="1.2",
                                  reason="the floor runs out onto a falling face, but the part "
                                         "stands above the floor's height past that side -- the "
                                         "side cannot be drawn open to the blank")
        pts += [p_out, q_out, q]
    if falling:
        pts = drop_collinear(pts)
    standing = [w for w in wall_idxs if kinds[w] == "wall"]
    open_along = [w for w in wall_idxs if kinds[w] == "silhouette"]
    rims = [w for w in wall_idxs if kinds[w] == "void"]

    # A floor nothing stands beside -- every edge the blank's edge or a
    # void's rim -- is a bounded `Face`, open all round, with what stands
    # on its inner wires proposing beside it (the ledge's rule).
    if not standing:
        if signed_area(pts) < 0.0:
            pts.reverse()
        k = min(range(len(pts)), key=lambda i: (round(pts[i][0], 3), round(pts[i][1], 3)))
        pts = pts[k:] + pts[:k]
        name = f"F{floor_idx}_Face"
        loops = [pts] + face_loops(floor)[1:]
        edges = (f"open along {' '.join(f'F{w}' for w in open_along)} (the blank's silhouette)"
                 + (f"; along the rim of {' '.join(f'F{w}' for w in rims)} (a void proposed beside it)" if rims else ""))
        whys = {floor_idx: f"floor at z={fmt(z_floor)} with nothing standing beside it -- "
                           f"{edges}: a bounded face, open all round"}
        extras = [
            f"# consume with: run End(tool=..., surface={name}, stepover=...)",
            "# the silhouette edge is drawn 0.02 past the blank: the engine clips it to the",
            "# declared stock, so on a `stock Cylinder` of the part's diameter it reads OPEN",
        ]
        return dict(primary=floor_idx, members=[floor_idx], name=name,
                    part=face_part(name, z_floor, loops), row="1.1", whys=whys,
                    extras=extras)

    # Check 3 -- every wall is vertical: a plane with a horizontal normal,
    # or a partial-revolution cylinder with axis parallel +Z. A tilted wall
    # is unreachable-as-declared (draft/undercut relative to the tool axis).
    for w in standing:
        wf = faces[w - 1]
        kind = wf.geomType()
        if kind == "PLANE":
            if abs(face_normal(wf)[1].Z()) >= 1.0 - COS_TOL:
                # A bevel rising from the floor (Phase 46 slice 3): the face
                # across its top says what it is -- a +Z plane means the
                # bevel IS the wall (the engine refuses that by name), a
                # vertical wall means a bevel at the wall's foot, which the
                # engine's top-edge chamfer has no key for.
                if strip_of(wf) is not None:
                    # (`see_through` refuses `chamfer_consumes_wall` itself
                    # where a +Z plane is across the strip's top.)
                    if see_through(w, faces, emap, face_index, below=False):
                        raise Refusal(code="chamfer_at_foot", row="1.2",
                                      reason=f"F{w} is a bevel at the FOOT of a wall -- the "
                                             "engine's chamfer is a top-edge break (chamfer_depth "
                                             "on the feature whose edge it breaks) and has no key "
                                             "for a foot",
                                      refs=[f"F{w}"])
                raise Refusal(code="pocket_wall_not_vertical", row="1.2",
                              reason=f"pocket wall F{w} is not vertical -- a "
                                     "tilted/draft wall (CHECKLIST row 1.2) or "
                                     "an undercut relative to the tool axis",
                              refs=[f"F{w}"])
        elif kind == "CYLINDER":
            ad = BRepAdaptor_Surface(wf.wrapped)
            if abs(ad.Cylinder().Axis().Direction().Z()) < COS_TOL:
                raise Refusal(code="pocket_wall_cylinder_tilted", row="1.2",
                              reason=f"pocket wall F{w} is a cylinder with axis "
                                     "not parallel to +Z -- not a vertical wall",
                              refs=[f"F{w}"])
            if full_revolution(ad):
                # Reached only for a bore whose BOTTOM is this floor (the
                # silhouette and a bore from below were sorted out above).
                raise Refusal(code="pocket_floor_of_bore", row="2.3",
                              reason=f"floor of a full-revolution cylindrical "
                                     f"bore (wall F{w}) -- the bore wall has "
                                     "its own report entry (CHECKLIST row 2.3); "
                                     "a circular floor is not a pocket boundary",
                              refs=[f"F{w}"])
        else:
            raise Refusal(code="pocket_wall_not_plane_or_cylinder",
                          reason=f"pocket wall F{w} is a {kind.lower()} surface "
                                 "-- not a vertical planar or cylindrical wall",
                          refs=[f"F{w}"])

    # Check 4 -- one mouth height: every wall's top rim at the same Z (bbox
    # slop is coarser than LIN_TOL, per the cone_frustum precedent), and
    # that mouth must be ABOVE the floor -- walls that hang below mark a
    # plateau/step top, not a pocket.
    tops = [faces[w - 1].BoundingBox().zmax for w in standing]
    z_mouth = max(tops)
    if z_mouth - min(tops) > 10.0 * LIN_TOL:
        raise Refusal(code="pocket_walls_stepped",
                      reason="pocket walls do not rise to a single mouth "
                             "height (stepped or multi-level walls) -- not a "
                             "simple pocket")
    if z_mouth <= z_floor + LIN_TOL:
        raise Refusal(code="pocket_walls_descend",
                      reason="wall loop descends from this face -- a "
                             "plateau/step top, not a pocket floor")

    # Check 5 -- the mouth rim opens upward (the load-bearing undercut
    # check): the face across each wall's top edge must be planar with
    # normal +Z (the surrounding top/ledge). An overhanging lip puts a
    # downward face there. A single-point probe alone would miss this --
    # a lipped pocket can still have an open mouth over the floor centroid.
    # A bevel at the mouth (Phase 46 slice 3) is the pocket's `chamfer_depth`:
    # the face across a wall's top edge may be a STRIP whose own top meets the
    # surrounding +Z plane -- then the mouth is the strip's top, the walls'
    # tops are a leg below it, and every wall must carry the same bevel.
    chamfer = None  # (depth, included angle, strip faces)
    edge_round = None  # (radius, blend faces)
    bevels = {}
    rounds = set()
    for w in standing:
        wf = faces[w - 1]
        for edge in wf.Edges():
            if edge.BoundingBox().zmin < z_mouth - 10.0 * LIN_TOL:
                continue  # not a rim edge at the mouth height
            for anc in emap.FindFromKey(edge.wrapped):
                if anc.IsSame(wf.wrapped):
                    continue
                j = face_index(anc)
                nb = faces[j - 1]
                if nb.geomType() == "PLANE" and face_normal(nb)[1].Z() > COS_TOL:
                    bevels.setdefault(w, None)
                    continue
                angle = strip_of(nb)
                if angle is not None:
                    above = across_edges(nb, faces, emap, face_index, at_bottom=False)
                    if above and all(faces[k - 1].geomType() == "PLANE" and face_normal(faces[k - 1])[1].Z() > COS_TOL for k in above):
                        nbb = nb.BoundingBox()
                        bevels[w] = (round(nbb.zmax - nbb.zmin, 6), round(angle, 6), j)
                        continue
                # A round at the mouth is the pocket's `edge_round`, read
                # the way the bevel is: a quarter-round from the wall's top
                # to the surrounding plane.
                radius = blend_radius(nb, z_mouth, axis_on_top=False)
                if radius is not None:
                    above = across_edges(nb, faces, emap, face_index, at_bottom=False)
                    if above and all(faces[k - 1].geomType() == "PLANE" and face_normal(faces[k - 1])[1].Z() > COS_TOL for k in above):
                        bevels[w] = (round(radius, 6), "round", j)
                        rounds.add(j)
                        continue
                raise Refusal(code="pocket_mouth_lipped",
                              reason="pocket mouth rim is capped by an "
                                     "overhanging lip -- undercut relative "
                                     "to the tool axis")
    legs = {(b[0], b[1]) if b else None for b in bevels.values()}
    if len(legs) > 1:
        raise Refusal(code="chamfer_uneven",
                      reason="the walls of this mouth do not carry one bevel or one round "
                             "(a leg, an angle or a radius differs, or some walls have none) "
                             "-- one feature carries one chamfer or one edge_round",
                      refs=[f"F{b[2]}" for b in bevels.values() if b])
    if legs and None not in legs:
        d, a = next(iter(legs))
        if a == "round":
            edge_round = (d, sorted(rounds))
        else:
            chamfer = (d, a, sorted({b[2] for b in bevels.values()}))

    # Check 6 -- belt-and-suspenders solid probe, mirroring the bore's:
    # a point just above the mouth over the floor's centroid must be
    # outside the material.
    ctr = floor.Center()
    probe = gp_Pnt(ctr.x, ctr.y, z_mouth + 10.0 * LIN_TOL)
    state = BRepClass3d_SolidClassifier(solid.wrapped, probe, LIN_TOL).State()
    if state != TopAbs_State.TopAbs_OUT:
        raise Refusal(code="pocket_capped",
                      reason="pocket does not open upward (capped above) -- "
                             "unreachable (undercut relative to the tool axis)")

    # Normalize the loop for determinism: CCW winding by signed area, then
    # rotate to start at the lexicographically smallest (x, y) after fmt()
    # rounding -- the kernel's wire-start edge is arbitrary.
    if signed_area(pts) < 0.0:
        pts.reverse()
    k = min(range(len(pts)), key=lambda i: (round(pts[i][0], 3), round(pts[i][1], 3)))
    pts = pts[k:] + pts[:k]

    name = f"F{floor_idx}_Pocket"
    if chamfer is not None:
        z_mouth += chamfer[0]  # the mouth is the bevel's top
    if edge_round is not None:
        z_mouth += edge_round[0]  # and the round's
    depth = z_mouth - z_floor
    # The recognized geometry KNOWS its own vertical occupancy, so it authors
    # `z` + `extent` outright rather than leaving a number for a human to
    # transcribe (DECISIONS.md "Vertical occupancy is `extent`…"). Emitting `z`
    # explicitly also drops the old "measured from the stock top" caveat: a
    # mouth below the stock top is now stated, not assumed.
    keys = f"z={fmt(z_mouth)}, extent={fmt(depth)}"
    if edge_round is not None:
        keys += f", edge_round={fmt(edge_round[0])}"
    if chamfer is not None:
        keys += f", chamfer_depth={fmt(chamfer[0])}, chamfer_angle={fmt(chamfer[1])}"
    part = "\n".join([f"feature {name} ProfilePocket({keys}"]
                     + [f"    {fmt(x)} {fmt(y)}" for x, y in pts]
                     + [")"])
    walls = " ".join(f"F{w}" for w in standing)
    around = ("" if n_islands == 0 else
              f"; {n_islands} inner wire{'' if n_islands == 1 else 's'} -- "
              "standing material inside the pocket proposes beside it")
    edges = ""
    if open_along:
        edges += (f"; open along {' '.join(f'F{w}' for w in open_along)} "
                  "(the blank's silhouette, drawn 0.02 past it)")
    if rims:
        edges += (f"; the edge along the rim of {' '.join(f'F{w}' for w in rims)} "
                  "is a wall the Clear rides to (a void proposed beside it)")
    whys = {floor_idx: f"pocket floor at z={fmt(z_floor)}; vertical wall loop "
                       f"{walls}, mouth at z={fmt(z_mouth)}, depth "
                       f"{fmt(depth)}, opens upward{around}{edges}"}
    for w in standing:
        whys[w] = f"vertical wall of recognized pocket {name} (floor F{floor_idx})"
    if falling:
        sides = " ".join(f"F{w}" for w in falling)
        whys[floor_idx] += (f"; the floor runs out onto {sides}, which falls away from it -- "
                            "that side is open, the loop drawn straight out past the blank")
    if fillets:
        blends = " ".join(f"F{w}" for w in fillets)
        radii = "/".join(fmt(r) for r in sorted(set(fillets.values())))
        whys[floor_idx] += (f"; floor fillet R{radii} ({blends}) -- the loop is drawn at the "
                            "walls, the sharp corner")
        for w, r in fillets.items():
            whys[w] = (f"floor fillet R{fmt(r)} of recognized pocket {name} (floor F{floor_idx}): "
                       f"the pocket is drawn to the wall behind it; an endmill with a {fmt(r)} "
                       "corner radius leaves this fillet, a flat one cuts the corner square")
    if edge_round is not None:
        blends = " ".join(f"F{j}" for j in edge_round[1])
        whys[floor_idx] += (f"; mouth rounded R{fmt(edge_round[0])} ({blends}) -- proposed as "
                            "edge_round, the mouth at the round's top")
        for j in edge_round[1]:
            whys[j] = (f"mouth round of recognized pocket {name} (floor F{floor_idx}): a "
                       f"quarter-round between the walls' tops and the surrounding plane -- "
                       f"proposed as the pocket's edge_round, not a standalone face")
    if chamfer is not None:
        strips = " ".join(f"F{j}" for j in chamfer[2])
        whys[floor_idx] += (f"; mouth beveled {fmt(chamfer[0])} deep at {fmt(chamfer[1])}deg included "
                            f"({strips}) -- proposed as chamfer_depth, the mouth at the bevel's top")
        for j in chamfer[2]:
            whys[j] = (f"mouth bevel of recognized pocket {name} (floor F{floor_idx}): a strip "
                       f"between the walls' tops and the surrounding plane -- proposed as the "
                       f"pocket's chamfer_depth, not a standalone face")
    extras = [
        f"# consume with: run Clear(tool=..., profile={name}, stepover=...)"
        "  [the feature carries its own z + extent]",
        f"# or finish with: run Side(tool=..., surface={name}, "
        "comp=control)",
        "# comp=control is required on recognized profile Side runs: "
        "recognized boundaries carry rounded",
        "# corners tessellated to chords, and engine comp hard-errors on "
        "rounded-corner",
        '# offsets (DECISIONS.md -> "recognized contours propose comp=control")',
    ]
    if open_along:
        extras += [
            f"# open along {' '.join(f'F{w}' for w in open_along)}: the part's silhouette, drawn 0.02 past the blank --",
            "# the engine clips the outline to the declared stock, so on a `stock Cylinder` of the",
            "# part's diameter those edges read OPEN, and on a larger Box they read WALL",
        ]
    if n_islands:
        extras += [
            f"# this floor has {n_islands} inner wire"
            f"{'' if n_islands == 1 else 's'}: a boss proposed INSIDE this loop is "
            "its island, derived from",
            "# containment -- the Clear above goes around it (stepover at most "
            "the tool's radius), and the",
            "# island takes no Clear of its own. CHECK that every inner wire "
            "has a proposed boss below:",
            "# standing material this reader could not name is NOT in the "
            "proposal, and would be cleared through.",
        ]
    if falling:
        extras += [
            f"# open toward {' '.join(f'F{w}' for w in falling)}: the floor runs out onto a face that falls away, so",
            "# that side is drawn straight out past the blank, where the engine reads it open;",
            "# everything the loop holds past the floor is air in the model above this floor",
        ]
    if fillets:
        radii = "/".join(fmt(r) for r in sorted(set(fillets.values())))
        extras += [
            f"# the model has a floor fillet R{radii}: the loop is the walls' own line, so a flat",
            "# endmill cuts the corner square, past the model; an endmill with that corner",
            "# radius leaves the fillet",
        ]
    if edge_round is not None:
        extras += [
            f"# break the mouth with: run Form(tool=<CornerRounder>, surface={name})"
            f"  [its ground arc must be R{fmt(edge_round[0])}]",
        ]
    if chamfer is not None:
        extras += [
            f"# the mouth's {fmt(chamfer[0])}mm bevel is the pocket's chamfer_depth, at the included angle the",
            "# model measures; a print that says otherwise is typed here (the three chamfer words are",
            "# pick-two) -- break it with: run Form(tool=<ChamferMill>, surface=" + name + ")",
        ]
    members = ([floor_idx] + standing + sorted(fillets)
               + (chamfer[2] if chamfer is not None else [])
               + (edge_round[1] if edge_round is not None else []))
    return dict(primary=floor_idx, members=members, name=name,
                part=part, row="2.7" if had_arc else "2.1", whys=whys,
                extras=extras)


def recognize_pockets(solid, faces, z_top, emap, face_index, claimed, blank_side=0.0):
    """The grouping pass, run BEFORE per-face classification: every planar
    +Z face below the topmost height is a pocket-floor candidate -- unless
    the hole pass already `claimed` it (an absorbed bore floor is the bore's
    bottom, not a pocket floor). Returns (member_of, floor_fail): member_of
    maps a face index to its group for absorbed floors/walls; floor_fail
    maps a refused floor's index to its residue entry. Total accounting
    is preserved -- a candidate is always in exactly one."""
    member_of, floor_fail = {}, {}
    if z_top is None:
        return member_of, floor_fail

    for i, f in enumerate(faces, start=1):
        if i in claimed or f.geomType() != "PLANE":
            continue
        pnt, n = face_normal(f)
        if not (n.Z() > COS_TOL and pnt.Z() < z_top - LIN_TOL):
            continue
        try:
            g = pocket_group(f, i, faces, solid, emap, face_index, blank_side)
        except Refusal as r:
            floor_fail[i] = r.entry()
        else:
            for m in g["members"]:
                member_of[m] = g
    return member_of, floor_fail


def outer_profile_group(cap, cap_idx, wall_idxs, faces, solid, emap,
                        face_index, z_top, z_solid_min, chamfer=None,
                        edge_round=None, ends=()):
    """Graduation checks + boundary extraction for one outer-profile
    candidate: a +Z planar cap whose outer-wire walls all descend (the
    caller's filter). The exact mirror of pocket_group, cap-down instead of
    floor-up: material is INSIDE the loop, so the proposed Contour is wound
    CW -- under the winding convention (DECISIONS.md "Row-2.4 side
    selector") that puts the tool outside. A single full-revolution
    cylindrical wall is the circular boss and proposes Feature::CylinderBoss
    instead of a chord list (never degrade an analytic primitive to a
    polyline). Raises Refusal naming the failed check; the checks ARE the
    orientation/undercut gate (the Phase 4 co-requisite)."""
    z_cap = face_normal(cap)[0].Z()

    # Check 1 -- every wall is vertical, and material sits inside the loop:
    # a plane with a horizontal normal, or an axis-parallel-+Z cylinder. The
    # outward normal must point away from the cap's interior -- a wall whose
    # material lies outside the loop makes the cap a floor, not a top.
    ctr = cap.Center()
    boss_wall = None
    for w in wall_idxs:
        wf = faces[w - 1]
        kind = wf.geomType()
        if kind == "PLANE":
            if abs(face_normal(wf)[1].Z()) >= 1.0 - COS_TOL:
                raise Refusal(code="outer_profile_wall_not_vertical", row="1.2",
                              reason=f"outer-profile wall F{w} is not vertical "
                                     "-- a tilted/draft wall (CHECKLIST row "
                                     "1.2) or an undercut relative to the tool "
                                     "axis",
                              refs=[f"F{w}"])
        elif kind == "CYLINDER":
            ad = BRepAdaptor_Surface(wf.wrapped)
            if abs(ad.Cylinder().Axis().Direction().Z()) < COS_TOL:
                raise Refusal(code="outer_profile_wall_cylinder_tilted",
                              row="1.2",
                              reason=f"outer-profile wall F{w} is a cylinder "
                                     "with axis not parallel to +Z -- not a "
                                     "vertical wall",
                              refs=[f"F{w}"])
            if full_revolution(ad) and len(wall_idxs) == 1:
                # The loop's sole wall: a full-revolution rim circle IS the
                # whole outer wire. The same face on a wire with other walls
                # (the disc's rim on a rib's wire, the real part) is one arc
                # of that loop, read with the rest.
                boss_wall = w
        else:
            raise Refusal(code="outer_profile_wall_not_plane_or_cylinder",
                          reason=f"outer-profile wall F{w} is a {kind.lower()} "
                                 "surface -- not a vertical planar or "
                                 "cylindrical wall",
                          refs=[f"F{w}"])
        pnt, n = face_normal(wf)
        if n.X() * (pnt.X() - ctr.x) + n.Y() * (pnt.Y() - ctr.y) < 0.0:
            raise Refusal(code="outer_profile_wall_inverted",
                          reason=f"outer-profile wall F{w} faces the loop "
                                 "interior -- material is outside the loop, so "
                                 "this cap tops nothing (undercut or inverted "
                                 "geometry)",
                          refs=[f"F{w}"])

    # Check 2 -- the base. One height is the plain case (bbox slop coarser
    # than LIN_TOL, per the cone_frustum precedent), strictly below the cap.
    # Walls that run on deeper -- a rib running out to the part's rim, whose
    # wall IS the rim's, down to the part bottom (the real part, ROADMAP
    # item 1 slice 5) -- make a STEPPED base: the profile is proposed to the
    # highest base, the span every wall has, and the deeper walls are named
    # so the reviewer knows the profile stops above their feet.
    bases = {w: faces[w - 1].BoundingBox().zmin for w in wall_idxs}
    z_base = max(bases.values())
    deeper = sorted(w for w, zb in bases.items() if zb < z_base - 10.0 * LIN_TOL)
    if z_base >= z_cap - LIN_TOL:
        raise Refusal(code="outer_profile_degenerate_rim",
                      reason="outer-profile walls have no height below the "
                             "cap -- a degenerate rim")

    # Check 3 -- the base rim is clean (the load-bearing undercut check,
    # the mirror of the pocket's mouth rim): the face across each wall's
    # bottom edge must be an upward (+Z) planar ledge the tool can clear,
    # the part's own bottom (a downward plane AT the solid's zmin -- a
    # through profile with nothing below to collide with), or the wall's
    # own underside: a downward plane above the bottom means the wall tops
    # a head overhanging a narrower neck (the real part's ribs). The wall
    # is traced from above all the same, with air under the tool at the
    # base; the underside is the other setup's, and the note says so.
    # Anything else -- a fillet, a flare -- curls under the wall and refuses.
    overhang = False
    for w in wall_idxs:
        if w in deeper:
            continue  # its foot is below the base the profile stops at
        wf = faces[w - 1]
        for edge in wf.Edges():
            if edge.BoundingBox().zmax > z_base + 10.0 * LIN_TOL:
                continue  # not a base-rim edge
            for anc in emap.FindFromKey(edge.wrapped):
                if anc.IsSame(wf.wrapped):
                    continue
                nb = faces[face_index(anc) - 1]
                ok = False
                if nb.geomType() == "PLANE":
                    nz = face_normal(nb)[1].Z()
                    if nz > COS_TOL:
                        ok = True  # an upward ledge
                    elif nz < -COS_TOL and abs(z_base - z_solid_min) <= 10.0 * LIN_TOL:
                        ok = True  # the part bottom: a through profile
                    elif nz < -COS_TOL:
                        ok = overhang = True  # the head's own underside
                if not ok:
                    raise Refusal(code="outer_profile_base_rim_undercut",
                                  reason="outer-profile base rim is not an "
                                         "upward ledge, the part bottom or "
                                         "the wall's own underside -- "
                                         "material curls under the wall "
                                         "(undercut relative to the tool "
                                         "axis)")

    depth = z_top - z_base  # from the stock top (the topmost planar height)

    if boss_wall is not None:
        # The circular boss (CHECKLIST row 2.4): propose the analytic
        # primitive. Material-inside was already checked above; convexity is
        # Feature::CylinderBoss's by construction.
        wf = faces[boss_wall - 1]
        cyl = BRepAdaptor_Surface(wf.wrapped).Cylinder()
        loc = cyl.Axis().Location()
        name = f"F{boss_wall}_Boss"
        # A cone between the cap and the wall is the boss's top-edge break,
        # as a strip is a profile's: one cone, so one leg and one angle.
        broken = chamfer is not None and chamfer[0] is not None
        break_keys = (f", chamfer_depth={fmt(chamfer[0])}, chamfer_angle={fmt(chamfer[1])}"
                      if broken else "")
        part = (f"feature {name} CylinderBoss(dia={fmt(2.0 * cyl.Radius())}, "
                f"extent={fmt(z_cap - z_base)}, x={fmt(loc.X())}, "
                f"y={fmt(loc.Y())}, z={fmt(z_base)}{break_keys})")
        overhang_why = (f"base at z={fmt(z_base)} on the head's own underside (an overhang: "
                        f"the wall is traced from above, the underside is the other setup's)")
        base_words = overhang_why if overhang else f"clean base at z={fmt(z_base)}"
        whys = {boss_wall: "cylindrical, axis +Z, full revolution, material "
                           f"inside (a standing boss), flat top cap F{cap_idx}, "
                           f"{base_words}"}
        extras = [
            f"# finish the wall with: run Side(tool=..., surface={name}, "
            "stepover=...)  [stepover is the ring spacing up the wall]",
            "# the tool rides outside at R + r_tool (convex flank; "
            "finish_cylinder's mirror)",
        ]
        overhang_extras = [
            f"# the base at z={fmt(z_base)} is an overhang: the face under the wall faces down, so the",
            "# tool has air under it at the base; the underside itself is the other setup's",
        ]
        if overhang:
            extras += overhang_extras
        members = [boss_wall]
        if broken:
            strips = " ".join(f"F{j}" for j in chamfer[2])
            whys[boss_wall] += (f"; top edge beveled {fmt(chamfer[0])} deep at {fmt(chamfer[1])}deg "
                                f"included ({strips}) -- proposed as chamfer_depth")
            for j in chamfer[2]:
                whys[j] = (f"top-edge bevel of recognized boss {name} (top cap F{cap_idx}): a cone "
                           f"between the cap and the wall -- proposed as the boss's chamfer_depth, "
                           f"not a standalone face")
            extras += [
                f"# the top edge's {fmt(chamfer[0])}mm bevel is the boss's chamfer_depth, at the included angle the",
                "# model measures; a print that says otherwise is typed here (the three chamfer words are",
                f"# pick-two) -- break it with: run Form(tool=<ChamferMill>, surface={name})",
            ]
            members += chamfer[2]
        r = cyl.Radius()
        loop = [(loc.X() + r * math.cos(2.0 * math.pi * k / 128),
                 loc.Y() + r * math.sin(2.0 * math.pi * k / 128)) for k in range(128)]
        return dict(primary=boss_wall, members=members, name=name,
                    part=part, row="2.4", whys=whys, extras=extras,
                    cap=cap_idx, z_cap=z_cap, z_base=z_base, overhang=overhang,
                    inside=(ctr.x, ctr.y), loop=loop, deeper=deeper,
                    overhang_why=overhang_why, overhang_extras=overhang_extras)

    # Contour path: walk the cap's outer wire again for the ordered 2D loop
    # (lines and circular arcs only -- edge_polyline refuses the rest). A
    # cap whose edge is beveled (Phase 46 slice 3) reads its loop at the
    # WALLS -- their top edges, chained -- never at the cap's wire, because
    # the loop IS the wall the Side traces and the Form rides.
    # `chamfer` is (leg, angle, strips, strip_over, strip_leg): one leg under
    # every wall is the feature's chamfer_depth; (None, angle, ...) is one
    # angle with legs that differ edge to edge, or a wall left unbeveled,
    # and the loop's points restate the leg (slice 9); (None, None, ...) is
    # strips at two angles: the loop is still read at the walls, but no
    # chamfer is written and the strips are not members.
    # `edge_round` is (radius, round faces): a quarter-round between the cap
    # and every wall, read the same way -- the loop at the walls' tops, the
    # feature's top at the cap. `ends` are the blends along a bevel's ends,
    # between the bevel and the wall beside it: no part of the loop, which
    # runs on past them to the sharp corner.
    even = chamfer is not None and chamfer[0] is not None
    marked = chamfer is not None and chamfer[0] is None and chamfer[1] is not None
    strips_seen = chamfer[2] if chamfer is not None else []
    round_faces = edge_round[1] if edge_round is not None else []
    pts, had_arc = [], False
    pieces_by_wall = {}
    if chamfer is None and edge_round is None:
        wexp = BRepTools_WireExplorer(BRepTools.OuterWire_s(cap.wrapped),
                                      cap.wrapped)
        while wexp.More():
            ept, arc = edge_polyline(wexp.Current())
            pts += ept
            had_arc = had_arc or arc
            wexp.Next()
    else:
        # A wall's top boundary is every edge it shares with a strip or with
        # the cap itself (a wall that rises to the cap unbeveled, like the
        # disc's rim a rib runs out to): one level edge under an even bevel;
        # under uneven bevels also the short slanted edges where a deeper
        # bevel meets a shallower one at a corner (the real part's ribs). In
        # plan they all lie on the wall's line, so the chained loop is the
        # wall's.
        above = set(strips_seen) | set(round_faces) | set(ends) | {cap_idx}
        pieces = []
        for w in wall_idxs:
            wf = faces[w - 1]
            for edge in wf.Edges():
                shared_with_strip = any(
                    not anc.IsSame(wf.wrapped) and face_index(anc) in above
                    for anc in emap.FindFromKey(edge.wrapped))
                if not shared_with_strip:
                    continue
                ept, arc = wall_top_points(edge.wrapped, wf)
                pieces.append(ept)
                pieces_by_wall.setdefault(w, []).append(ept)
                had_arc = had_arc or arc
        # Where the wall under a bevel is cut away below it (a groove under
        # the head runs out through that wall), the bevel's foot goes on
        # along the underside, and the loop follows the foot.
        for j in strips_seen:
            sf = faces[j - 1]
            foot = sf.BoundingBox().zmin
            for edge in sf.Edges():
                if edge.BoundingBox().zmax > foot + 10.0 * LIN_TOL:
                    continue
                for anc in emap.FindFromKey(edge.wrapped):
                    nb = faces[face_index(anc) - 1]
                    if (not anc.IsSame(sf.wrapped) and nb.geomType() == "PLANE"
                            and face_normal(nb)[1].Z() < -COS_TOL):
                        pieces.append(edge_points(edge.wrapped)[0])
        reach = 4.0 * max((BRepAdaptor_Surface(faces[j - 1].wrapped).Cylinder().Radius()
                           for j in ends), default=0.0)
        pts = chain_loop(pieces, reach)
        if pts is not None:
            # The slanted corner edges project onto the wall's own line:
            # the points they add are collinear and say nothing.
            pts = drop_collinear(pts)
        if pts is None:
            raise Refusal(code="chamfer_wall_loop_open",
                          reason="the walls under this cap's bevel do not chain into one "
                                 "closed loop at their tops -- the bevel breaks an edge "
                                 "this reader cannot follow round",
                          refs=[f"F{w}" for w in list(strips_seen) + list(round_faces)])

    # Normalize for determinism, mirrored from pocket_group with the sign
    # flipped: CW winding by signed area (material inside the loop -> tool
    # outside under the convention), then rotate to start at the
    # lexicographically smallest (x, y) after fmt() rounding.
    if signed_area(pts) > 0.0:
        pts.reverse()
    k = min(range(len(pts)), key=lambda i: (round(pts[i][0], 3), round(pts[i][1], 3)))
    pts = pts[k:] + pts[:k]

    primary = min(w for w in wall_idxs if w not in deeper)
    name = f"F{primary}_Profile"
    # Same as the pocket group: the recognized wall knows its own top and
    # extent, so it authors them (DECISIONS.md "Vertical occupancy is
    # `extent`…").
    keys = f"z={fmt(z_top)}, extent={fmt(depth)}"
    if even or marked or edge_round is not None:
        # A broken top edge is at the cap, so the cap is the feature's top
        # and the extent runs from it.
        keys = f"z={fmt(z_cap)}, extent={fmt(z_cap - z_base)}"
    if edge_round is not None:
        keys += f", edge_round={fmt(edge_round[0])}"
    marks, default_leg, edge_legs = {}, None, []
    if even:
        keys += f", chamfer_depth={fmt(chamfer[0])}, chamfer_angle={fmt(chamfer[1])}"
    elif marked:
        # Each loop edge lies on one wall's top (the loop was chained from
        # the walls' top edges); the strip over that wall gives its leg,
        # none where the wall rises to the cap unbeveled. The feature
        # carries the leg that runs the longest way round the loop (an arc
        # is many short chords, so edges are not counted), and a point
        # restates the leg wherever it differs from the edge before it --
        # at the first point, from the feature's own.
        strip_over = chamfer[3]
        length = {}
        for i in range(len(pts)):
            a, b = pts[i], pts[(i + 1) % len(pts)]
            over = strip_over.get(wall_under_edge(a, b, pieces_by_wall))
            leg = over[0] if over else None
            edge_legs.append(leg)
            if leg is not None:
                length[leg] = length.get(leg, 0.0) + math.dist(a, b)
        default_leg = max(sorted(length), key=length.get)
        for i, leg in enumerate(edge_legs):
            before = default_leg if i == 0 else edge_legs[i - 1]
            if leg != before:
                marks[i] = leg
        keys += f", chamfer_depth={fmt(default_leg)}, chamfer_angle={fmt(chamfer[1])}"

    def point_line(i, x, y):
        line = f"    {fmt(x)} {fmt(y)}"
        if i in marks:
            line += f" chamfer_depth={fmt(marks[i] or 0.0)}"
        return line

    part = "\n".join([f"feature {name} ProfileBoss({keys}"]
                     + [point_line(i, x, y) for i, (x, y) in enumerate(pts)]
                     + [")"])
    walls = " ".join(f"F{w}" for w in wall_idxs)
    whys = {w: f"vertical wall of recognized outer profile {name} "
               f"(top cap F{cap_idx})" for w in wall_idxs}
    whys[primary] = (f"outer profile under top cap F{cap_idx}: vertical wall "
                     f"loop {walls}, top at z={fmt(z_cap)}, base at "
                     f"z={fmt(z_base)}, material inside the loop")
    if deeper:
        feet = ", ".join(f"F{w} to z={fmt(bases[w])}" for w in deeper)
        whys[primary] += (f"; stepped base: the profile stops at the highest base z={fmt(z_base)}, "
                          f"and {feet} run on below it")
    overhang_why = (f"; the base at z={fmt(z_base)} is the head's own underside (the face under "
                    f"the walls faces down): the wall is traced from above, the underside is the "
                    f"other setup's")
    if overhang:
        whys[primary] += overhang_why
    if even:
        strips = " ".join(f"F{j}" for j in chamfer[2])
        whys[primary] += (f"; top edge beveled {fmt(chamfer[0])} deep at {fmt(chamfer[1])}deg "
                          f"included ({strips}) -- proposed as chamfer_depth, the loop read at the walls")
        for j in chamfer[2]:
            whys[j] = (f"top-edge bevel of recognized outer profile {name} (top cap F{cap_idx}): "
                       f"a strip between the cap and the walls -- proposed as the boss's "
                       f"chamfer_depth, not a standalone face")
    elif marked:
        strips = " ".join(f"F{j}" for j in chamfer[2])
        legs_seen = ", ".join(fmt(d) for d in sorted({d for d in edge_legs if d is not None}))
        unbroken = " and 0 where a wall rises to the cap unbeveled" if None in edge_legs else ""
        whys[primary] += (f"; top edge beveled at {fmt(chamfer[1])}deg included with legs that differ "
                          f"edge to edge ({strips}: {legs_seen}) -- the loop carries {fmt(default_leg)} as "
                          f"its chamfer_depth and its points restate the leg where it changes{unbroken}")
        for j in chamfer[2]:
            whys[j] = (f"top-edge bevel of recognized outer profile {name} (top cap F{cap_idx}): "
                       f"a strip between the cap and the walls, {fmt(chamfer[4][j])} deep -- proposed "
                       f"as the boss's chamfer_depth, restated on the loop's points where the leg changes")
    elif chamfer is not None and edge_round is None:
        strips = " ".join(f"F{j}" for j in chamfer[2])
        whys[primary] += (f"; top edge beveled unevenly ({strips}) -- proposed without a chamfer, "
                          f"the loop read at the walls; the strips stay residue")
    if edge_round is not None:
        blends = " ".join(f"F{j}" for j in round_faces)
        whys[primary] += (f"; top edge rounded R{fmt(edge_round[0])} ({blends}) -- proposed as "
                          f"edge_round, the loop read at the walls")
        for j in round_faces:
            whys[j] = (f"top-edge round of recognized outer profile {name} (top cap F{cap_idx}): "
                       f"a quarter-round between the cap and the wall under it -- proposed as the "
                       f"boss's edge_round, not a standalone face")
        if strips_seen:
            strips = " ".join(f"F{j}" for j in strips_seen)
            whys[primary] += (f"; one edge is beveled instead ({strips}) -- a feature carries a round "
                              f"or a chamfer, not both, so the loop stands a square wall there and "
                              f"the bevel stays residue")
    extras = [
        f"# consume with: run Side(tool=..., surface={name}, comp=control)"
        "  [the feature carries its own z + extent]",
        "# the loop is wound CW: material inside, tool outside (the winding "
        "convention,",
        '# DECISIONS.md -> "Row-2.4 side selector")',
        "# comp=control is required on Contour runs: recognized boundaries "
        "carry rounded",
        "# corners tessellated to chords, and engine comp hard-errors on "
        "rounded-corner",
        '# offsets (DECISIONS.md -> "recognized contours propose comp=control")',
    ]
    if even:
        extras += [
            f"# the top edge's {fmt(chamfer[0])}mm bevel is the boss's chamfer_depth, at the included angle the",
            "# model measures; a print that says otherwise is typed here (the three chamfer words are",
            "# pick-two) -- break it with: run Form(tool=<ChamferMill>, surface=" + name + ", comp=control, tip_clearance=...)",
        ]
    if deeper:
        extras += [
            f"# the base is stepped: this profile stops at z={fmt(z_base)}, the highest base; the walls "
            + " ".join(f"F{w}" for w in deeper) + " run on below it",
            "# and belong to whatever stands lower -- declare that by hand or accept its own proposal",
        ]
    overhang_extras = [
        f"# the base at z={fmt(z_base)} is an overhang: the face under the walls faces down, so the",
        "# tool has air under it at the base; the underside itself is the other setup's",
    ]
    if overhang:
        extras += overhang_extras
    if marked:
        extras += [
            f"# the top edge's bevel is {fmt(default_leg)}mm deep on most edges, at the included angle the model",
            "# measures, and the loop's points restate the leg where it changes (0 leaves an edge unbroken) --",
            f"# break it with: run Form(tool=<ChamferMill>, surface={name})  [engine comp: control comp cannot",
            "# lift off an unbroken edge, and the deeper leg reaches about a reach's width round each corner]",
        ]
    if edge_round is not None:
        extras += [
            f"# break the top edge with: run Form(tool=<CornerRounder>, surface={name})"
            f"  [its ground arc must be R{fmt(edge_round[0])}]",
        ]
        if strips_seen:
            extras += [
                "# one edge of the model is a bevel, not a round (see its own entry): this loop stands a",
                "# square wall there, which leaves the bevel's material on; nothing proposed here cuts it",
            ]
    if chamfer is not None and not even and not marked and edge_round is None:
        extras += [
            "# the top edge is beveled unevenly (see the strips' own entries): no chamfer_depth is proposed;",
            "# type the one the print says, or break each edge with its own Form run",
        ]
    # A wall that runs on below the base is the loop's here, but belongs to
    # whatever stands lower (the disc's rim, every rib's at once): it keeps
    # its own entry and stays free for the next cap's loop.
    members = ([w for w in wall_idxs if w not in deeper] + (chamfer[2] if even or marked else [])
               + list(round_faces))
    # The primary stays the wall the feature is named for, whatever strip or
    # round has a lower number: the proposal is written at that face.
    return dict(primary=primary, members=members, name=name,
                part=part, row="2.8" if had_arc else "2.2", whys=whys,
                extras=extras, cap=cap_idx, z_cap=z_cap, z_base=z_base,
                overhang=overhang, inside=(ctr.x, ctr.y), loop=pts, deeper=deeper,
                overhang_why=overhang_why, overhang_extras=overhang_extras)


def recognize_outer_profiles(solid, faces, z_top, emap, face_index, claimed):
    """The third grouping pass, after holes and pockets: any +Z planar face
    (topmost or not) whose OUTER wire's walls all DESCEND is an
    outer-profile candidate -- the part's silhouette, or a boss/plateau
    standing on a ledge. The cap is the kernel-given anchor (its outer wire
    IS the loop) but never a member: a topmost cap keeps its Floor
    proposal, and a below-top cap keeps its residue entry with the reason
    rewritten to point at the group (`cap_note`). A candidate is SKIPPED
    silently -- no refusal recorded, every face keeps its own
    classification -- when the loop is not a standalone prismatic wall loop
    at all: a wall already claimed by an earlier pass, a non-plane/cylinder
    neighbor (the rim of a quadric), or a wall rising above the cap (pocket
    territory). A candidate that qualifies but fails a graduation check
    lands its named refusal on every wall (`wall_fail`)."""
    member_of, wall_fail, cap_note = {}, {}, {}
    if z_top is None:
        return member_of, wall_fail, cap_note
    z_solid_min = solid.BoundingBox().zmin

    for i, f in enumerate(faces, start=1):
        if i in claimed or i in member_of or f.geomType() != "PLANE":
            continue
        pnt, n = face_normal(f)
        if n.Z() <= COS_TOL:
            continue
        z_cap = pnt.Z()

        # First walk: the outer wire's neighbor faces, shapes only.
        wall_idxs = []
        wexp = BRepTools_WireExplorer(BRepTools.OuterWire_s(f.wrapped),
                                      f.wrapped)
        broken = False
        mouths = False
        while wexp.More():
            edge = wexp.Current()
            wall = None
            for anc in emap.FindFromKey(edge):
                if not anc.IsSame(f.wrapped):
                    wall = face_index(anc)
            if wall is None:
                broken = True
                break
            if claimed.get(wall, {}).get("name", "").endswith("_Pocket"):
                # The wire runs round a proposed pocket's mouth here: the
                # pocket opens out through this outline, and its mouth is no
                # wall of the loop.
                mouths = True
            elif wall not in wall_idxs:
                wall_idxs.append(wall)
            wexp.Next()

        skip = broken or not wall_idxs
        # A wall another group owns ends the candidacy -- a wall is one
        # feature's -- unless it runs on below this loop's other walls: the
        # disc's rim, the whole part's boss, is on every rib's wire too, and
        # a rib's loop reads it without claiming it (`deeper`, below).
        zmins = {w: faces[w - 1].BoundingBox().zmin for w in wall_idxs}
        free = [zmins[w] for w in wall_idxs if w not in claimed and w not in member_of]
        z_base_guess = max(free) if free else None
        for w in wall_idxs:
            if skip:
                break
            wf = faces[w - 1]
            taken = w in claimed or w in member_of
            runs_deeper = z_base_guess is not None and zmins[w] < z_base_guess - 10.0 * LIN_TOL
            if ((taken and not runs_deeper)
                    or (wf.geomType() not in ("PLANE", "CYLINDER") and strip_of(wf) is None
                        and top_round(wf, z_cap) is None)
                    or wf.BoundingBox().zmax > z_cap + 10.0 * LIN_TOL):
                skip = True
        if skip:
            continue

        # The cap's edge break (Phase 46 slice 3): a strip on the cap's wire
        # is seen THROUGH to the vertical wall under it, which joins the
        # loop in its place; the strips must agree on one leg and one angle.
        strips, walls, chamfer, refusal, uneven = [], [], None, None, None
        strip_over, strip_leg = {}, {}  # wall -> (leg, angle) of the strip over it; strip -> leg
        rounds, round_over, ends, edge_round = {}, set(), [], None  # round -> radius; walls under one
        candidate = list(wall_idxs)  # every face on the cap's wire, for a refusal to land on
        try:
            legs = set()
            for w in wall_idxs:
                wf = faces[w - 1]
                angle = strip_of(wf)
                if angle is None:
                    # A quarter-round between the cap and a wall is the top
                    # edge's round, seen through to the wall as a strip is.
                    radius = top_round(wf, z_cap)
                    below = ([j for j in across_edges(wf, faces, emap, face_index, at_bottom=True)
                              if upright(faces[j - 1])] if radius is not None else [])
                    if below:
                        rounds[w] = round(radius, 6)
                        round_over.update(below)
                        for j in below:
                            if j not in walls:
                                walls.append(j)
                        continue
                    if w not in walls:  # a strip's partner may have brought it in already
                        walls.append(w)
                    continue
                under = see_through(w, faces, emap, face_index, below=True)
                if not under:
                    walls.append(w)  # not a break on a wall: the group refuses it as before
                    continue
                strips.append(w)
                for edge in wf.Edges():
                    for anc in emap.FindFromKey(edge.wrapped):
                        j = face_index(anc)
                        nf = faces[j - 1]
                        if (j not in ends and nf.geomType() == "CYLINDER" and not upright(nf)
                                and blend_radius(nf, nf.BoundingBox().zmin, axis_on_top=False) is None):
                            ends.append(j)  # the blend along the bevel's end, to the wall beside it
                bb = wf.BoundingBox()
                leg = (round(bb.zmax - bb.zmin, 6), round(angle, 6))
                legs.add(leg)
                strip_leg[w] = leg[0]
                for j in under:
                    if j not in walls:
                        walls.append(j)
                # The wall this strip breaks the edge of is the one across
                # its FOOT edge, the edge lying at the strip's lowest height;
                # `under` also holds the walls round the corner, met along
                # the strip's short rising edges, and those carry their own
                # strips' legs.
                foot = bb.zmin
                for edge in wf.Edges():
                    if edge.BoundingBox().zmax > foot + 10.0 * LIN_TOL:
                        continue
                    for anc in emap.FindFromKey(edge.wrapped):
                        if not anc.IsSame(wf.wrapped) and face_index(anc) in under:
                            strip_over[face_index(anc)] = leg
            angles = {a for _, a in legs}
            if len(angles) > 1:
                # Strips at two angles are two tools' work: the profile
                # proposes with no chamfer, the strips stay residue, and the
                # edge is the hand's to break.
                uneven = sorted(legs)
                chamfer = (None, None, strips, strip_over, strip_leg)
            elif strips:
                # One angle. One leg under every wall is the feature's
                # chamfer_depth as before; legs that differ edge to edge, or
                # a wall that rises to the cap unbeveled, are the loop's
                # points to say (slice 9): the feature carries the commonest
                # leg and a point restates it where it changes, 0 where the
                # edge is unbroken.
                one_leg = len(legs) == 1 and all(w in strip_over for w in walls)
                chamfer = (next(iter(legs))[0] if one_leg else None,
                           next(iter(angles)), strips, strip_over, strip_leg)
            if rounds:
                bare = [w for w in walls if w not in round_over and w not in strip_over]
                if len(set(rounds.values())) > 1 or bare:
                    raise Refusal(code="chamfer_uneven",
                                  reason="the top edges under this cap do not carry one round (a radius "
                                         "differs, or some walls have none) -- one feature carries one "
                                         "edge_round",
                                  refs=[f"F{j}" for j in sorted(rounds)])
                edge_round = (next(iter(rounds.values())), sorted(rounds))
                if strips:
                    # A round and a bevel on one loop: the feature carries
                    # the round, the loop is still read at the walls, and
                    # the bevel is left out and says so.
                    chamfer = (None, None, strips, strip_over, strip_leg)
        except Refusal as r:
            refusal = r
        wall_idxs = walls

        try:
            if refusal is not None:
                raise refusal
            g = outer_profile_group(f, i, wall_idxs, faces, solid, emap,
                                    face_index, z_top, z_solid_min, chamfer,
                                    edge_round, ends)
        except Refusal as r:
            for w in sorted(set(candidate) | set(wall_idxs)):
                wall_fail[w] = r.rewrap(
                    role="outer_profile_candidate_wall",
                    prefix=f"wall of refused outer-profile candidate under "
                           f"cap F{i}", refs=[f"F{i}"])
            if z_cap < z_top - LIN_TOL:
                cap_note[i] = r.rewrap(
                    role="outer_profile_candidate_cap",
                    prefix="outer-profile candidate cap refused")
        else:
            for m in g["members"]:
                member_of[m] = g
            if edge_round is not None:
                for j in strips:
                    leg = fmt(strip_leg[j])
                    wall_fail[j] = residue(
                        code="chamfer_beside_round", row="no row",
                        reason=f"top-edge bevel round cap F{i}, {leg}mm deep, on a loop whose other "
                               f"edges are rounded R{fmt(edge_round[0])}: one feature carries a round or "
                               f"a chamfer, not both -- {g['name']} proposes with the round and a "
                               f"square wall along this edge, and the bevel is not cut",
                        refs=[f"F{g['primary']}"])
            elif uneven is not None:
                seen = ", ".join(f"{fmt(d)} at {fmt(a)}deg" for d, a in uneven)
                for j in strips:
                    wall_fail[j] = residue(
                        code="chamfer_uneven", row="no row",
                        reason=f"top-edge bevel round cap F{i}: the strips there do not "
                               f"carry one angle ({seen}), and one tool cuts one angle -- "
                               f"{g['name']} proposes without a chamfer; break this edge "
                               f"by hand",
                        refs=[f"F{k}" for k in strips if k != j] + [f"F{g['primary']}"])
            if z_cap < z_top - LIN_TOL:
                # A below-top cap of a RECOGNIZED outer profile: a ledge. It
                # used to be residue ("a below-top plane has no Floor proposal
                # of its own", code outer_profile_anchor_cap, retired Phase
                # 28); it proposes as a bounded `Face` now, and this note only
                # carries which profile it caps.
                # A cap a pocket opens through is not bounded by its own
                # wire, which runs round the pocket's mouth: it may be a
                # level instead (`recognize_levels`).
                cap_note[i] = dict(anchor=g["name"], open=mouths)
    return member_of, wall_fail, cap_note


def point_in_loop(pt, loop):
    """Ray casting: whether the plan point lies inside the closed polygon."""
    x, y = pt
    inside = False
    n = len(loop)
    for i in range(n):
        (x0, y0), (x1, y1) = loop[i], loop[(i + 1) % n]
        if (y0 > y) != (y1 > y):
            xi = x0 + (y - y0) * (x1 - x0) / (y1 - y0)
            if xi > x:
                inside = not inside
    return inside


def dist_to_loop(pt, loop):
    """Plan distance from a point to the closed polygon's boundary."""
    x, y = pt
    best = float("inf")
    n = len(loop)
    for k in range(n):
        (ax, ay), (bx, by) = loop[k], loop[(k + 1) % n]
        ux, uy = bx - ax, by - ay
        d2 = ux * ux + uy * uy
        t = 0.0 if d2 == 0.0 else max(0.0, min(1.0, ((x - ax) * ux + (y - ay) * uy) / d2))
        best = min(best, math.hypot(x - (ax + t * ux), y - (ay + t * uy)))
    return best


def under_loop(face, loop, tol=2.0 * CHORD_TOL):
    """Whether every vertex of the face lies inside the plan loop, or on it
    within the chording (a pad whose top IS the head's outline lands its
    vertices up to a chord tolerance outside the chorded loop)."""
    return all(point_in_loop((v.X, v.Y), loop) or dist_to_loop((v.X, v.Y), loop) <= tol
               for v in face.Vertices())


def settle_heads(member_of, wall_fail, cap_note, floor_fail, faces, emap,
                 face_index, z_solid_min):
    """A head on a narrower neck, from above: nothing reaches what is under
    it -- the neck, the underside, a pad the neck stands on, the pad's foot
    bevel -- so the material under the head stays, and what the program
    makes is the head's outline carried straight down to the floor the
    head stands over (the real part's ribs, ROADMAP item 1 slice 6). This
    walks down from each overhanging head through the faces lying under
    its outline until it meets an upward plane that reaches out from under
    it: that is the floor, and the boss is proposed to it. Every face
    passed on the way becomes residue `under_overhang`, naming the head; a
    foot bevel that stands outside the outline is cut away by the floor's
    clearing and its entry says so. Where the walk meets the part bottom
    (a neck on the bottom, nothing to stand over) or a face it cannot
    place, the head keeps its own base and the engine's rule decides
    (`demote_floating_heads`). Rewrites member_of, wall_fail, cap_note and
    the group dicts in place."""
    groups = {g["name"]: g for g in member_of.values() if g.get("overhang")}
    for name, g in groups.items():
        loop, z_base = g["loop"], g["z_base"]
        skip = set(g["members"]) | set(g.get("deeper", []))
        under, skirts, floors, seen, fail = [], [], {}, set(skip), False
        frontier = []
        for w in g["members"]:
            wf = faces[w - 1]
            for edge in wf.Edges():
                if edge.BoundingBox().zmax > z_base + 10.0 * LIN_TOL:
                    continue
                for anc in emap.FindFromKey(edge.wrapped):
                    j = face_index(anc)
                    if j not in seen:
                        frontier.append(j)
        while frontier and not fail:
            j = frontier.pop()
            if j in seen or j in member_of:
                seen.add(j)
                continue
            seen.add(j)
            f = faces[j - 1]
            kind = f.geomType()
            bb = f.BoundingBox()
            if kind == "PLANE":
                nz = face_normal(f)[1].Z()
                if nz > COS_TOL:
                    if under_loop(f, loop):
                        under.append((j, "the pad under the head"))
                    else:
                        floors.setdefault(round(bb.zmin, 3), []).append(j)
                        continue  # the floor: the walk stops here
                elif nz < -COS_TOL:
                    if abs(bb.zmin - z_solid_min) <= 10.0 * LIN_TOL:
                        continue  # the part bottom: nothing stands under the neck
                    if under_loop(f, loop):
                        under.append((j, "the head's underside"))
                    else:
                        fail = True
                elif strip_of(f) is not None:
                    tops = [v for v in f.Vertices() if v.Z >= bb.zmax - 10.0 * LIN_TOL]
                    if under_loop(f, loop):
                        under.append((j, "a bevel under the head"))
                    elif tops and all(point_in_loop((v.X, v.Y), loop)
                                      or dist_to_loop((v.X, v.Y), loop) <= 2.0 * CHORD_TOL for v in tops):
                        skirts.append(j)
                    else:
                        fail = True
                elif under_loop(f, loop):
                    under.append((j, "a wall of the neck"))
                else:
                    fail = True
            elif kind in ("CYLINDER", "CONE") and strip_of(f) is not None:
                tops = [v for v in f.Vertices() if v.Z >= bb.zmax - 10.0 * LIN_TOL]
                if under_loop(f, loop):
                    under.append((j, "a bevel under the head"))
                elif tops and all(point_in_loop((v.X, v.Y), loop)
                                  or dist_to_loop((v.X, v.Y), loop) <= 2.0 * CHORD_TOL for v in tops):
                    skirts.append(j)
                else:
                    fail = True
            elif kind == "CYLINDER" and under_loop(f, loop):
                under.append((j, "a wall of the neck"))
            else:
                fail = True
            if fail:
                break
            for edge in f.Edges():
                for anc in emap.FindFromKey(edge.wrapped):
                    k = face_index(anc)
                    if k not in seen:
                        frontier.append(k)
        if fail or len(floors) != 1:
            continue  # the fallback below: the head keeps its base
        z_floor = next(iter(floors))
        if z_floor >= z_base - 10.0 * LIN_TOL:
            continue
        floor_ids = sorted(floors[z_floor])
        depth_old, depth_new = g["z_cap"] - z_base, g["z_cap"] - z_floor
        part = g["part"].replace(f"extent={fmt(depth_old)}", f"extent={fmt(depth_new)}", 1)
        if "CylinderBoss(" in part:
            # The base key, whatever follows it (a chamfered boss has more keys).
            part = part.replace(f", z={fmt(z_base)}", f", z={fmt(z_floor)}", 1)
        g["part"] = part
        under.sort()
        skirts.sort()
        words = ", ".join(f"F{j} ({what})" for j, what in under)
        floor_words = " ".join(f"F{i}" for i in floor_ids)
        settled_why = (f"the base at z={fmt(z_base)} is the head's own underside, and nothing from "
                       f"above reaches what is under the head -- {words}"
                       + (f", and the foot bevels {' '.join(f'F{j}' for j in skirts)}" if skirts else "")
                       + f" -- so the boss is proposed down to z={fmt(z_floor)}, the floor {floor_words} "
                       f"it stands over, and the program leaves the material under the head standing")
        p = g["primary"]
        if g["overhang_why"] in g["whys"][p]:
            lead = "; " if g["overhang_why"].startswith("; ") else ""
            g["whys"][p] = g["whys"][p].replace(g["overhang_why"], lead + settled_why)
        g["extras"] = [line for line in g["extras"] if line not in g["overhang_extras"]] + [
            f"# the walls end at z={fmt(z_base)} on the head's own underside; the neck, the pad and the pad's",
            f"# foot bevels under it are out of reach from above, so the boss runs down to z={fmt(z_floor)}, the",
            "# floor it stands over, and the program leaves what is under the head standing",
        ] + ([
            "# the foot bevels stand outside the outline below the head: clearing the floor to the outline",
            "# cuts them away, and the wall runs straight down to the floor where the model has a bevel",
        ] if skirts else [])
        g["z_base"] = z_floor
        g["floor"] = (z_floor, floor_ids)
        g["under"] = [j for j, _ in under] + skirts
        for j, what in under:
            wall_fail[j] = residue(
                code="under_overhang", row="no row",
                reason=f"{what}, under the overhang of {name} (cap F{g['cap']}): nothing from above "
                       f"reaches it, and the boss is proposed down to z={fmt(z_floor)}, the floor it "
                       f"stands over -- the program leaves this face standing as the material under the head",
                refs=[f"F{p}"] + [f"F{i}" for i in floor_ids])
        for j in skirts:
            wall_fail[j] = residue(
                code="under_overhang", row="no row",
                reason=f"the foot bevel of the pad under {name} (cap F{g['cap']}): it stands outside "
                       f"the head's outline below the overhang, where only the floor's clearing reaches "
                       f"-- cleared away to the outline, so the wall runs straight down to z={fmt(z_floor)} "
                       f"where the model has this bevel",
                refs=[f"F{p}"] + [f"F{i}" for i in floor_ids])
        for i, note in list(cap_note.items()):
            if i in wall_fail and i in {j for j, _ in under}:
                del cap_note[i]


def recognize_levels(member_of, wall_fail, cap_note, floor_fail, faces, emap,
                     face_index):
    """The floor the heads stand over (ROADMAP item 1 slice 6): a refused
    floor whose every neighbour is the blank's silhouette, a proposed
    feature, or a face under a settled head proposes as the LEVEL, one
    `Plane(z=…)` for every such floor at that height -- the whole blank
    cleared down to it around what stands, the engine deriving islands and
    voids from containment. The Plane says nothing about where the floor
    ends, so it is proposed only when nothing stands above that height
    outside a proposed feature; anything that does is named and the floors
    stay refused as `level_obstructed`.

    A refused floor that nothing beside it rises above is a level too, heads
    or none: a block's top with a boss standing higher somewhere else on the
    part. Every face on its edges runs down from it or is a proposed
    feature's, so the floor ends in air or in a declared cut all the way
    round, and the same test for what stands above decides it."""
    settled = [g for g in {id(g): g for g in member_of.values()}.values() if g.get("floor")]

    def open_floor(i):
        # The cap of a refused boss keeps that refusal: it says why the boss
        # under it is not proposed, and a level would bury it.
        if i in member_of or i in wall_fail or (i in cap_note and not cap_note[i].get("open")):
            return None
        f = faces[i - 1]
        pnt, n = face_normal(f)
        if n.Z() <= COS_TOL:
            return None
        for edge in f.Edges():
            for anc in emap.FindFromKey(edge.wrapped):
                j = face_index(anc)
                if j == i or faces[j - 1].BoundingBox().zmax <= pnt.Z() + 10.0 * LIN_TOL:
                    continue
                # What rises from a level is a boss standing on it. A bore's
                # or a pocket's wall rising round a floor makes that floor
                # the cavity's own, however the cavity was proposed.
                if j not in member_of or member_of[j]["name"].endswith(("_Bore", "_Hole", "_Pocket")):
                    return None
        return round(pnt.Z(), 3)

    tops = {i: z for i in sorted(floor_fail) for z in [open_floor(i)] if z is not None}
    heights = sorted({g["floor"][0] for g in settled} | set(tops.values()))
    if not heights:
        return
    standing_loops = [g["loop"] for g in {id(g): g for g in member_of.values()}.values()
                      if g.get("loop")]

    def silhouette(j, z):
        f = faces[j - 1]
        if f.geomType() != "CYLINDER":
            return False
        ad = BRepAdaptor_Surface(f.wrapped)
        if abs(ad.Cylinder().Axis().Direction().Z()) < COS_TOL or not full_revolution(ad):
            return False
        loc = ad.Cylinder().Axis().Location()
        pnt, n = face_normal(f)
        radial = gp_Vec(pnt.X() - loc.X(), pnt.Y() - loc.Y(), 0.0)
        return n.Dot(radial) > 0.0 and f.BoundingBox().zmax > z + LIN_TOL

    def covered(j):
        return (j in member_of
                or wall_fail.get(j, {}).get("code") == "under_overhang"
                or any(under_loop(faces[j - 1], loop) for loop in standing_loops))

    for z in heights:
        heads = sorted({g["name"] for g in settled if g["floor"][0] == z})
        floors = []
        for i in sorted(floor_fail):
            if i in member_of or i in wall_fail or ("anchor" in cap_note.get(i, {}) and i not in tops):
                continue
            f = faces[i - 1]
            pnt, n = face_normal(f)
            if n.Z() <= COS_TOL or abs(pnt.Z() - z) > 10.0 * LIN_TOL:
                continue
            clean = True
            if i in tops:
                floors.append(i)
                continue
            for edge in f.Edges():
                for anc in emap.FindFromKey(edge.wrapped):
                    j = face_index(anc)
                    if j == i or j in member_of or silhouette(j, z):
                        continue
                    if wall_fail.get(j, {}).get("code") != "under_overhang":
                        clean = False
            if clean:
                floors.append(i)
        if not floors:
            continue
        blockers = sorted(j for j, f in enumerate(faces, start=1)
                          if j not in floors and f.BoundingBox().zmax > z + 10.0 * LIN_TOL
                          and not silhouette(j, z) and not covered(j))
        ids = " ".join(f"F{i}" for i in floors)
        if blockers and not heads:
            continue  # a top with something undeclared above it keeps its own refusal
        if blockers:
            named = " ".join(f"F{j}" for j in blockers)
            for i in floors:
                floor_fail[i] = residue(
                    code="level_obstructed", row="1.1",
                    reason=f"the floor at z={fmt(z)} is the level the heads {' '.join(heads)} stand over "
                           f"({ids}), but {named} stand above it outside every proposed feature, and a "
                           f"Plane there would clear through them -- declare them first",
                    refs=blockers)
            continue
        primary = floors[0]
        name = f"F{primary}_Floor"
        rims = sorted({face_index(anc) for i in floors for edge in faces[i - 1].Edges()
                       for anc in emap.FindFromKey(edge.wrapped) if silhouette(face_index(anc), z)})
        skirts = sorted({j for g in settled if g["floor"][0] == z for j in g["under"]
                         if "foot bevel" in wall_fail.get(j, {}).get("reason", "")})
        whys = {i: f"floor at z={fmt(z)}, one face of the level {name}"
                   + (" the heads stand over" if heads else "") for i in floors}
        lead = (f"the level at z={fmt(z)} the heads {' '.join(heads)} stand over" if heads else
                f"the level at z={fmt(z)}, below the part's topmost height and with nothing beside it rising above it")
        whys[primary] = (f"{lead}: {len(floors)} "
                         f"face{'' if len(floors) == 1 else 's'} ({ids}) at one height"
                         + (f", open along {' '.join(f'F{j}' for j in rims)} (the blank's silhouette)" if rims else "")
                         + "; nothing stands above it outside a proposed feature, so the whole blank clears "
                           "to it around what stands, the islands and voids derived from containment")
        extras = [
            f"# consume with: run Clear(tool=..., profile={name}, stepover=...)  [the whole blank down to "
            f"z={fmt(z)}, around what stands]",
            f"# then finish with: run End(tool=..., surface={name}, stepover=...)",
            f"# the islands are derived from containment: every proposed boss standing to z={fmt(z)} (the heads",
            "# named above), and every bore whose mouth opens in this floor is a void the paths bridge or go around",
        ] if heads else [
            f"# consume with: run Clear(tool=..., profile={name}, stepover=...)  [the whole blank down to "
            f"z={fmt(z)}, around what stands higher]",
            f"# then finish with: run End(tool=..., surface={name}, stepover=...)",
            "# what stands higher is an island by containment, and a pocket or bore that opens in this floor is",
            "# cut from it by its own run",
        ]
        if skirts:
            extras += [
                f"# the pads' foot bevels under the heads ({len(skirts)} face{'' if len(skirts) == 1 else 's'}, each "
                f"saying so in the report) stand outside the",
                "# heads' outlines and are cleared away: the walls run straight down to this floor",
            ]
        g = dict(primary=primary, members=floors, name=name,
                 part=f"feature {name} Plane(z={fmt(z)})", row="1.1", whys=whys, extras=extras)
        for i in floors:
            member_of[i] = g
            floor_fail.pop(i, None)


def demote_floating_heads(member_of, wall_fail, cap_note, floor_fail, faces):
    """The engine's own rule, applied here so a report's proposals parse
    together: a boss standing in a floor LOWER than its base is an undercut
    ("a boss floating above a cavity's floor ... nothing in this vocabulary
    cuts beneath it", src/stock.rs). A head `settle_heads` could not carry
    down to its floor proposes only while no proposed floor below it hosts
    it; one that is hosted refuses, naming the floor. Rewrites member_of,
    wall_fail and cap_note in place."""
    hosts = []
    for i, note in cap_note.items():
        if "anchor" in note and i in floor_fail:
            f = faces[i - 1]
            hosts.append((i, face_normal(f)[0].Z(), face_loops(f)[0]))
    groups = {g["name"]: g for g in member_of.values() if g.get("overhang") and not g.get("floor")}
    for name, g in groups.items():
        host = next(((i, z) for i, z, loop in hosts
                     if z < g["z_base"] - 10.0 * LIN_TOL and point_in_loop(g["inside"], loop)), None)
        if host is None:
            continue
        hi, hz = host
        r = Refusal(code="outer_profile_base_rim_undercut",
                    reason=f"the head's base at z={fmt(g['z_base'])} floats above floor F{hi} at "
                           f"z={fmt(hz)}, the face it stands in -- a boss floating above its floor "
                           f"is an undercut the engine refuses (nothing cuts beneath it from this "
                           f"side); the other setup's job",
                    refs=[f"F{hi}"])
        cap = g["cap"]
        for m in g["members"]:
            member_of.pop(m, None)
            wall_fail[m] = r.rewrap(role="outer_profile_candidate_wall",
                                    prefix=f"wall of refused outer-profile candidate under cap F{cap}",
                                    refs=[f"F{cap}"])
        for j, entry in list(wall_fail.items()):
            if j not in g["members"] and name in entry.get("reason", ""):
                wall_fail[j] = r.rewrap(role="outer_profile_candidate_wall",
                                        prefix=f"strip of refused outer-profile candidate under cap F{cap}",
                                        refs=[f"F{cap}"])
        for i, note in list(cap_note.items()):
            if note.get("anchor") == name:
                cap_note[i] = r.rewrap(role="outer_profile_candidate_cap",
                                       prefix="outer-profile candidate cap refused")


def trim_wire_group(wire, bottom_idx, faces, emap, face_index, z_bot,
                    is_outer, claimed):
    """Qualification + graduation + wire extraction for one trim-wire
    candidate: a wire of the part's BOTTOM face whose adjacent walls are all
    vertical, unclaimed, and topped by straight edges forming a non-planar
    closed 3D wire -- a through trim, the only home where the pair's
    constant-extent flank pass is gouge-safe (over an interior ledge the
    pass would dive into the ledge wherever the wire dips; anchoring on the
    part bottom makes "nothing below to cut" structural, the through-profile
    arm of the outer-profile pass turned into the anchor itself).

    Returns the group dict, None to skip silently (not a standalone
    prismatic wall loop, or a planar-topped one -- the bottom face anchors
    EVERY solid, so this pass must be quiet by default and leave flat wire
    loops to the cap/floor-anchored passes), or raises Refusal for a
    candidate that qualifies and fails a graduation check."""
    # Walk the wire in connected order: the adjacent wall faces.
    wall_idxs = []
    wexp = BRepTools_WireExplorer(wire, faces[bottom_idx - 1].wrapped)
    while wexp.More():
        edge = wexp.Current()
        wall = None
        for anc in emap.FindFromKey(edge):
            if not anc.IsSame(faces[bottom_idx - 1].wrapped):
                wall = face_index(anc)
        if wall is None:
            return None
        if wall not in wall_idxs:
            wall_idxs.append(wall)
        wexp.Next()
    if not wall_idxs:
        return None

    # Qualification (silent skip): every wall a vertical plane or an
    # axis-parallel-Z cylinder, none claimed by an earlier pass.
    for w in wall_idxs:
        wf = faces[w - 1]
        if w in claimed:
            return None
        kind = wf.geomType()
        if kind == "PLANE":
            if abs(face_normal(wf)[1].Z()) >= 1.0 - COS_TOL:
                return None
        elif kind == "CYLINDER":
            ad = BRepAdaptor_Surface(wf.wrapped)
            if abs(ad.Cylinder().Axis().Direction().Z()) < COS_TOL:
                return None
        else:
            return None

    # Collect every top edge of the loop: not at the bottom, not a vertical
    # side line. Edge TYPES are tolerated here so that a planar-topped loop
    # (whatever its boundary curves) skips on flatness below rather than
    # refusing -- flat loops belong to the cap/floor-anchored passes.
    top_edges = []
    for w in wall_idxs:
        wf = faces[w - 1]
        for edge in wf.Edges():
            bb = edge.BoundingBox()
            if bb.zmax <= z_bot + 10.0 * LIN_TOL:
                continue  # the bottom rim
            c = BRepAdaptor_Curve(edge.wrapped)
            if c.GetType() == GeomAbs_CurveType.GeomAbs_Line:
                d = c.Line().Direction()
                if abs(d.Z()) > COS_TOL:
                    continue  # a vertical side edge (a wall-wall junction)
            if not any(edge.wrapped.IsSame(e.wrapped) for _, e in top_edges):
                top_edges.append((w, edge))

    if not top_edges:
        return None

    # The planar-loop gate: a flat top rim is not a 3D wire -- skip, keeping
    # every face's own classification (and the earlier passes' refusals).
    z_lo = min(e.BoundingBox().zmin for _, e in top_edges)
    z_hi = max(e.BoundingBox().zmax for _, e in top_edges)
    if z_hi - z_lo <= 10.0 * LIN_TOL:
        return None

    # Graduation check 1 -- every top edge is a straight line: the wire
    # grammar carries straight `x y z` segments, and tessellating a curved
    # 3D trim edge would be a guess, not a reading.
    segs = []
    for w, edge in top_edges:
        c = BRepAdaptor_Curve(edge.wrapped)
        if c.GetType() != GeomAbs_CurveType.GeomAbs_Line:
            raise Refusal(code="trim_wire_edge_not_line",
                          reason=f"trim-wire top edge of wall F{w} is not a "
                                 "straight line -- a curved trim edge cannot "
                                 "ride the wire grammar's straight x y z "
                                 "segments, and tessellating a 3D curve would "
                                 "be a guess; curved trim edges have not "
                                 "graduated",
                          refs=[f"F{w}"])
        p0, p1 = c.Value(c.FirstParameter()), c.Value(c.LastParameter())
        segs.append(((p0.X(), p0.Y(), p0.Z()), (p1.X(), p1.Y(), p1.Z())))

    # Chain the segments into one closed 3D loop by shared endpoints (the
    # kernel gives no traversal order for a rim that crosses wall gables).
    def same(a, b):
        return (abs(a[0] - b[0]) <= 10.0 * LIN_TOL
                and abs(a[1] - b[1]) <= 10.0 * LIN_TOL
                and abs(a[2] - b[2]) <= 10.0 * LIN_TOL)

    chain = [segs[0][0], segs[0][1]]
    rest = segs[1:]
    while rest:
        for k, (a, b) in enumerate(rest):
            if same(chain[-1], a):
                chain.append(b)
                break
            if same(chain[-1], b):
                chain.append(a)
                break
        else:
            raise Refusal(code="trim_wire_open_loop",
                          reason="trim-wire top edges do not chain into one "
                                 "closed loop (a broken or non-manifold rim)")
        rest.pop(k)
    if not same(chain[0], chain[-1]):
        raise Refusal(code="trim_wire_open_loop",
                      reason="trim-wire top edges do not chain into one "
                             "closed loop (a broken or non-manifold rim)")
    pts = chain[:-1]

    # Graduation check 2 -- material on the loop's declared side: a boss
    # (the outer wire) keeps material inside, so every wall's outward
    # normal points away from the loop interior; a pocket (an inner wire)
    # keeps material outside, so the normals point into the void.
    cx = sum(p[0] for p in pts) / len(pts)
    cy = sum(p[1] for p in pts) / len(pts)
    for w in wall_idxs:
        pnt, n = face_normal(faces[w - 1])
        out = n.X() * (pnt.X() - cx) + n.Y() * (pnt.Y() - cy)
        if (out < 0.0) if is_outer else (out > 0.0):
            raise Refusal(code="trim_wire_wall_inverted",
                          reason=f"trim wall F{w} has material on the wrong "
                                 "side of its loop (inverted or non-prismatic "
                                 "geometry)",
                          refs=[f"F{w}"])

    # Normalize for determinism, the planar passes' rule with the winding
    # picked by the variant (the row-2.4 law judged on the XY projection):
    # CW for the boss (material inside, tool outside), CCW for a pocket.
    area = signed_area([(x, y) for x, y, _ in pts])
    if (area > 0.0) if is_outer else (area < 0.0):
        pts.reverse()
    k = min(range(len(pts)),
            key=lambda i: (round(pts[i][0], 3), round(pts[i][1], 3),
                           round(pts[i][2], 3)))
    pts = pts[k:] + pts[:k]

    # The pass rides the wire displaced down by `extent`, so reaching the
    # part bottom under the wire's HIGHEST point takes the full drop; where
    # the wire dips the pass rides below the bottom -- a through trim,
    # nothing below to cut (the drill-past-extent precedent).
    extent = z_hi - z_bot

    primary = min(wall_idxs)
    name = f"F{primary}_Trim"
    variant = "ContourBoss" if is_outer else "ContourPocket"
    part = "\n".join([f"feature {name} {variant}(extent={fmt(extent)}"]
                     + [f"    {fmt(x)} {fmt(y)} {fmt(z)}" for x, y, z in pts]
                     + [")"])
    walls = " ".join(f"F{w}" for w in wall_idxs)
    side = "inside" if is_outer else "outside"
    whys = {w: f"vertical wall of recognized trim wire {name} "
               f"(part-bottom anchor F{bottom_idx})" for w in wall_idxs}
    whys[primary] = (f"through-trim wall loop {walls} on part-bottom anchor "
                     f"F{bottom_idx}: straight top edges form a closed 3D "
                     f"wire (z {fmt(z_lo)}..{fmt(z_hi)}), walls descend to "
                     f"the part bottom at z={fmt(z_bot)}, material {side} "
                     "the loop")
    winding = ("wound CW: material inside, tool outside" if is_outer
               else "wound CCW: material outside, tool inside")
    extras = [
        f"# consume with: run Side(tool=..., surface={name})  [an Endmill "
        "-- the wall is a flank cut; the wire carries its own Z range]",
        f"# extent={fmt(extent)} reaches the part bottom below the wire's "
        "highest point; where the wire dips,",
        "# the pass rides below the bottom -- a through trim, nothing below "
        "to cut",
        f"# the wire's XY projection is {winding} (the winding convention, "
        'DECISIONS.md -> "Row-2.4 side selector")',
        "# every segment is a straight line (a curved trim edge refuses "
        "instead of tessellating), so the",
        "# comp=control rule for recognized contours does not bind -- "
        "either comp mode is legal",
    ]
    return dict(primary=primary, members=list(wall_idxs), name=name,
                part=part, row="2.8" if is_outer else "2.7", whys=whys,
                extras=extras)


def recognize_trim_wires(solid, faces, emap, face_index, claimed):
    """The fourth grouping pass, after holes, pockets and outer profiles:
    the 3D-wire trim slice (DECISIONS.md "The 3D-wire Contour…", slice 3).
    Anchors on the part's BOTTOM face -- the downward plane at the solid's
    zmin -- whose outer wire bounds the silhouette walls (a ContourBoss
    candidate) and whose inner wires bound through cutouts (ContourPocket
    candidates). The anchor is never a member: the bottom face keeps its
    residue entry, reason rewritten via `bottom_note` when a proposal
    anchored on it. Needs no z_top -- a fully-crowned part with no flat +Z
    plane anywhere is exactly this pass's home turf."""
    member_of, trim_fail, bottom_note = {}, {}, {}
    z_bot = solid.BoundingBox().zmin

    for i, f in enumerate(faces, start=1):
        if f.geomType() != "PLANE":
            continue
        pnt, n = f.Center(), face_normal(f)[1]
        if n.Z() >= -COS_TOL or abs(pnt.z - z_bot) > LIN_TOL:
            continue

        anchored = []
        wires = ([(BRepTools.OuterWire_s(f.wrapped), True)]
                 + [(w.wrapped, False) for w in f.innerWires()])
        for wire, is_outer in wires:
            try:
                g = trim_wire_group(wire, i, faces, emap, face_index, z_bot,
                                    is_outer, claimed)
            except Refusal as r:
                # The refusal lands on every wall of the failed candidate
                # (the outer-profile pass's wall_fail convention).
                wexp = BRepTools_WireExplorer(wire, f.wrapped)
                while wexp.More():
                    for anc in emap.FindFromKey(wexp.Current()):
                        if not anc.IsSame(f.wrapped):
                            w = face_index(anc)
                            if w not in claimed:
                                trim_fail[w] = r.rewrap(
                                    role="trim_wire_candidate_wall",
                                    prefix="wall of refused trim-wire "
                                           f"candidate on part bottom F{i}",
                                    refs=[f"F{i}"])
                    wexp.Next()
            else:
                if g is not None:
                    for m in g["members"]:
                        member_of[m] = g
                    anchored.append(g["name"])
        if anchored:
            bottom_note[i] = residue(
                code="trim_wire_anchor_bottom", row="no row",
                reason=f"part-bottom anchor of recognized trim wire(s) "
                       f"{' '.join(anchored)} -- the through-trim walls "
                       "descend to it; a downward face has no proposal of its "
                       "own (unreachable from above)", refs=anchored)
    return member_of, trim_fail, bottom_note


def point_on_face(face):
    """A point ON the face and the outward normal there. face_normal's
    mid-parameter point falls off a ring or a horseshoe (into its hole), so
    this walks a grid of parameters, middle first, and keeps the ones the
    face classifier says are inside -- up to five, spread over the face."""
    ad = BRepAdaptor_Surface(face.wrapped)
    u0, u1 = ad.FirstUParameter(), ad.LastUParameter()
    v0, v1 = ad.FirstVParameter(), ad.LastVParameter()
    cls = BRepClass_FaceClassifier()
    n = 7
    order = sorted(range(n), key=lambda k: abs(k - (n - 1) / 2.0))
    found = []
    for a in order:
        for b in order:
            u = u0 + (u1 - u0) * (a + 0.5) / n
            v = v0 + (v1 - v0) * (b + 0.5) / n
            cls.Perform(face.wrapped, gp_Pnt2d(u, v), LIN_TOL)
            if cls.State() == TopAbs_State.TopAbs_IN:
                pnt, normal = gp_Pnt(), gp_Vec()
                BRepGProp_Face(face.wrapped).Normal(u, v, pnt, normal)
                normal.Normalize()
                found.append((pnt, normal))
    if not found:
        return [face_normal(face)]
    step = max(1, len(found) // 5)
    return found[::step][:5]


def covered_from_above(solid, faces, face_index):
    """Which faces the part's own material keeps a tool from reaching
    (ROADMAP item 1 slice 7): from each of up to five points on a face,
    nudged off it into the void, a ray straight up. When every ray meets
    the part, the face is covered, and the entry is the nearest face the
    first ray met with how far up it lies. A face only some rays reach is
    not covered: a floor running under a head's overhang is still the
    floor, and the head is its island."""
    inter = IntCurvesFace_ShapeIntersector()
    inter.Load(solid.wrapped, LIN_TOL)
    covered = {}
    for i, f in enumerate(faces, start=1):
        hits = []
        for pnt, n in point_on_face(f):
            start = gp_Pnt(pnt.X() + n.X() * REACH_NUDGE,
                           pnt.Y() + n.Y() * REACH_NUDGE,
                           pnt.Z() + n.Z() * REACH_NUDGE)
            inter.Perform(gp_Lin(start, gp_Dir(0, 0, 1)), 0.0, 1e9)
            best = None
            for k in range(1, inter.NbPnt() + 1):
                if inter.Face(k).IsSame(f.wrapped):
                    continue
                w = inter.WParameter(k)
                if best is None or w < best[1]:
                    best = (face_index(inter.Face(k)), w)
            if best is None:
                hits = []
                break
            hits.append(best)
        if hits:
            covered[i] = hits[0]
    return covered


SURFACE_WORD = dict(plane="planar", cylinder="cylindrical", cone="conical",
                    sphere="spherical", torus="toroidal")


def other_setup_residue(kind, cover, faces, up, grouped_with=None):
    """The one sentence for a face this setup cannot reach: what lies over
    it, how far up, and which --up shows it from the side that can."""
    j, h = cover
    lead = f"{SURFACE_WORD.get(kind, kind)} face"
    if grouped_with is not None:
        lead += f" grouped with F{grouped_with}, which is the other setup's too"
    over = SURFACE_WORD.get(faces[j - 1].geomType().lower(), "")
    return residue(
        code="other_setup", row="no row",
        reason=(f"{lead}: the part's own material lies straight above it -- "
                f"{over} F{j} is {fmt(round(h, 1))} up -- "
                f"so nothing on this setup's tool axis reaches it; see what "
                f"the other side makes of it with --up {UP_OPPOSITE[up]}"),
        refs=[f"F{j}"])


def bbox_str(shape):
    bb = shape.BoundingBox()
    return (f"({fmt(bb.xmin)},{fmt(bb.ymin)},{fmt(bb.zmin)})-"
            f"({fmt(bb.xmax)},{fmt(bb.ymax)},{fmt(bb.zmax)})")


def select_solid(step_path, solid_sel):
    """Pick the one solid recognition runs on. Solids are numbered S1..SM in
    the kernel's deterministic import order. A multi-solid file (an assembly)
    with no --solid refuses with a per-solid inventory -- which solid is the
    part on the machine is the human's setup decision, never a guess."""
    solids = cq.importers.importStep(str(step_path)).solids().vals()
    n = len(solids)
    if solid_sel is None:
        if n != 1:
            inventory = "; ".join(
                f"S{i} {len(s.Faces())} faces bbox {bbox_str(s)}"
                for i, s in enumerate(solids, start=1))
            raise ValueError(f"{n} solids in one STEP (an assembly) -- pick "
                             f"the part with --solid N: {inventory}")
        solid_sel = 1
    if not 1 <= solid_sel <= n:
        raise ValueError(f"--solid {solid_sel} out of range: the file has "
                         f"{n} solid(s), S1..S{n}")
    return solids[solid_sel - 1], solid_sel, n


VERTICAL_TOL = 1e-3


def prism_outline(faces, bb):
    """The plan outline of a prism, or (None, why it is not one)."""
    top = None
    for f in faces:
        pnt, n = face_normal(f)
        if f.geomType() == "PLANE" and n.Z() > COS_TOL:
            if top is None or pnt.Z() > top[0]:
                top = (pnt.Z(), f)
        elif f.geomType() == "PLANE" and n.Z() < -COS_TOL:
            continue
        elif abs(n.Z()) <= VERTICAL_TOL:
            continue
        else:
            return None, (f"a face is neither flat on top, flat on the bottom nor "
                          f"vertical (normal z {n.Z():.3f})")
    if top is None:
        return None, "it has no flat +Z face to take an outline from"
    z_top, face = top
    if abs(z_top - bb.zmax) > 1e-3:
        return None, (f"its topmost flat face (z {fmt(z_top)}) is below the solid's "
                      f"top (z {fmt(bb.zmax)})")
    outer = face_loops(face)[0]  # CCW, chorded through the one tolerance
    if signed_area(outer) <= 0.0:
        outer = list(reversed(outer))
    # The top's outline must be the WHOLE plan silhouette: a boss standing on
    # the blank's top has a smaller topmost face, and its outline is not the
    # blank's (block_bore.step, found on the first run).
    xs = [x for x, _ in outer]
    ys = [y for _, y in outer]
    if (abs(min(xs) - bb.xmin) > 1e-3 or abs(max(xs) - bb.xmax) > 1e-3
            or abs(min(ys) - bb.ymin) > 1e-3 or abs(max(ys) - bb.ymax) > 1e-3):
        return None, (f"its topmost flat face (z {fmt(z_top)}) spans x {fmt(min(xs))}.."
                      f"{fmt(max(xs))}, y {fmt(min(ys))}..{fmt(max(ys))} of a solid "
                      f"spanning x {fmt(bb.xmin)}..{fmt(bb.xmax)}, y {fmt(bb.ymin)}.."
                      f"{fmt(bb.ymax)}, so something stands on its top")
    return outer, None


MARGIN_KEYS = ("top", "bottom", "x-", "x+", "y-", "y+")


def parse_allowance(text):
    """--allowance's words as a margin per side of the blank. A bare number
    is the sides and the top (never the bottom: a blank cut to thickness
    sits on its parallels); `top=`, `bottom=`, `x-=`, `x+=`, `y-=`, `y+=`
    after it give a side its own, e.g. `1,bottom=3,x+=10`."""
    base, own = 0.0, {}
    for i, word in enumerate(w.strip() for w in str(text).split(",")):
        key, _, val = word.rpartition("=")
        key = key.strip()
        try:
            v = float(val)
        except ValueError:
            v = -1.0
        bare_first = key == "" and i == 0
        if not (bare_first or key in MARGIN_KEYS) or key in own or not 0.0 <= v < float("inf"):
            raise ValueError(f"--allowance `{text}`: `{word}` is not a size in mm, "
                             "or one of top=, bottom=, x-=, x+=, y-=, y+= with a size, "
                             "e.g. 1 or 1,bottom=3,x+=10")
        if key == "":
            base = v
        else:
            own[key] = v
    return {k: own.get(k, 0.0 if k == "bottom" else base) for k in MARGIN_KEYS}


def allowance_even(m):
    """The one number the margins come to when they are the sides and the
    top alike with nothing under the model, else None."""
    a = m["top"]
    even = m["bottom"] == 0.0 and all(m[k] == a for k in MARGIN_KEYS[2:])
    return a if even else None


def allowance_says(m):
    return ", ".join(f"{k} {fmt(m[k])}" for k in MARGIN_KEYS)


def round_side(m):
    """How far a round blank stands outside the model's rim: one number, so
    the four sides must agree."""
    sides = {m[k] for k in MARGIN_KEYS[2:]}
    if len(sides) != 1:
        raise ValueError("a round blank has one margin on its sides, and --allowance "
                         f"gives {allowance_says(m)}: say x-, x+, y- and y+ alike")
    return sides.pop()


ORIGIN_WORDS = ("min", "mid", "max")


def parse_origin(text):
    """--origin's three components, x,y,z. Each is `model` (the model's own
    zero on that axis), a number (a model coordinate, after --up), min, mid
    or max of the blank, or part-min, part-mid or part-max of the model."""
    parts = [p.strip() for p in text.split(",")]
    if len(parts) != 3:
        raise ValueError(f"--origin `{text}` is not three components x,y,z, "
                         "e.g. mid,mid,max")
    for p in parts:
        word = p[5:] if p.startswith("part-") else p
        if p == "model" or word in ORIGIN_WORDS:
            continue
        try:
            float(p)
        except ValueError:
            raise ValueError(f"--origin component `{p}` is not model, a number, min, "
                             "mid, max, part-min, part-mid or part-max") from None
    return parts


def blank_box(blank):
    """A given blank's low and high corners."""
    kind, v = blank
    if kind == "Cylinder":
        r = v["dia"] / 2.0
        return (v["x"] - r, v["y"] - r, v["z"] - v["h"]), (v["x"] + r, v["y"] + r, v["z"])
    return (v["x"], v["y"], v["z"] - v["h"]), (v["x"] + v["w"], v["y"] + v["l"], v["z"])


def origin_point(parts, bb, grow, given=None):
    """The model point (after --up) that becomes 0,0,0. `grow` is how far
    the blank stands outside the model on each side (parse_allowance), or
    None for a blank that is the model's own outline. `given` is a blank
    handed over (the second setup's): its own box is then the blank."""
    lo, hi = (bb.xmin, bb.ymin, bb.zmin), (bb.xmax, bb.ymax, bb.zmax)
    g = grow or dict.fromkeys(MARGIN_KEYS, 0.0)
    blo = (bb.xmin - g["x-"], bb.ymin - g["y-"], bb.zmin - g["bottom"])
    bhi = (bb.xmax + g["x+"], bb.ymax + g["y+"], bb.zmax + g["top"])
    if given is not None:
        blo, bhi = blank_box(given)
    out = []
    for k, p in enumerate(parts):
        of_part = p.startswith("part-")
        word = p[5:] if of_part else p
        a, b = (lo[k], hi[k]) if of_part else (blo[k], bhi[k])
        if p == "model":
            v = 0.0
        elif word in ORIGIN_WORDS:
            v = {"min": a, "mid": (a + b) / 2.0, "max": b}[word]
        else:
            v = float(p)
        # The kernel's box carries float noise; the origin is a clean number.
        out.append(round(v, 6) + 0.0)
    return tuple(out)


def place_origin(solid, origin, allowance):
    """The solid moved so the origin's point is 0,0,0, and that point. A
    prism is its own blank (import_stock.py), so nothing is grown for it."""
    bb = solid.BoundingBox()
    outer, _ = prism_outline(solid.Faces(), bb)
    at = origin_point(origin, bb, None if outer is not None else allowance)
    if any(c != 0.0 for c in at):
        solid = solid.translate(cq.Vector(-at[0], -at[1], -at[2]))
    return solid, at


def parse_point(text, flag):
    try:
        x, y, z = (float(p) for p in text.split(","))
    except ValueError:
        raise ValueError(f"{flag} `{text}` is not three numbers x,y,z") from None
    return (x, y, z)


def place_in_given(solid, up, blank, given_origin, origin):
    """The second setup's model and blank in its own frame. The blank
    arrives in the earlier program's frame, turned; the model (already
    turned by --up) is first moved into that frame: `given_origin` is the
    model point the earlier program zeroed at, and it turns with the part.
    Then the origin asked for here becomes 0,0,0 for both. Returns the
    solid, the blank, where the earlier program's zero now sits (the
    `given from` line's at=) and the point that became zero."""
    gx, gy, gz = given_origin
    if up == "z-":
        gy, gz = -gy, -gz
    elif up != "z+":
        raise ValueError(f"--given with --up {up}: a program reads only z+ and z- "
                         "for the turn between two setups")
    if (gx, gy, gz) != (0.0, 0.0, 0.0):
        solid = solid.translate(cq.Vector(-gx, -gy, -gz))
    kind, vals = blank
    at = (0.0, 0.0, 0.0)
    here = (0.0, 0.0, 0.0)
    if origin is not None:
        here = origin_point(origin, solid.BoundingBox(), None, blank)
        if any(c != 0.0 for c in here):
            solid = solid.translate(cq.Vector(-here[0], -here[1], -here[2]))
            vals = dict(vals, x=vals["x"] - here[0], y=vals["y"] - here[1], z=vals["z"] - here[2])
            at = tuple(round(-c, 6) + 0.0 for c in here)
    return solid, (kind, vals), at, here


def at_word(at):
    """The `given from` line's third word, or nothing for a shared origin."""
    if not any(at):
        return ""
    return " at=" + ",".join(fmt(c) for c in at)


def origin_note(origin, at):
    return (f"# imported with --origin {','.join(origin)}: the model's point "
            f"({fmt(at[0])}, {fmt(at[1])}, {fmt(at[2])}) is 0,0,0 here")


def recognize(step_path, solid_sel=None, up="z+", allowance=None, blank=None, given=None, origin=None,
              given_origin=(0.0, 0.0, 0.0)):
    """`allowance` is the blank's margin per side, as parse_allowance gives it."""
    allowance = allowance or dict.fromkeys(MARGIN_KEYS, 0.0)
    solid, solid_sel, n_solids = select_solid(step_path, solid_sel)
    meta = {}
    if allowance["top"] > 0.0:
        meta["allowance"] = allowance["top"]
    if allowance_even(allowance) is None:
        meta["margins"] = allowance_says(allowance)
    if blank is not None:
        meta["given"] = given
        meta["blank"] = blank_words(*blank)  # said again below, once it is placed
    if n_solids > 1:
        meta["solid"] = f"S{solid_sel}"
        meta["solids_total"] = n_solids
    if UP_ROTATIONS[up] is not None:
        # Rotate the declared up axis onto machine +Z BEFORE anything looks
        # at the geometry, so every downstream check and proposed coordinate
        # is in machine orientation. fmt() swallows the rotation's float
        # noise (cos 90 deg ~ 6e-17) the same way it swallows the kernel's.
        axis, degrees = UP_ROTATIONS[up]
        solid = solid.rotate((0, 0, 0), axis, degrees)
        meta["up"] = up
    if blank is not None:
        # The second setup: into the earlier program's frame, then to this
        # setup's own zero, the blank with it.
        solid, blank, given_at, here = place_in_given(solid, up, blank, given_origin, origin)
        meta["given"] = given
        meta["blank"] = blank_words(*blank)
        if origin is not None:
            meta["origin"] = ",".join(origin)
            meta["given_at"] = given_at
    elif origin is not None:
        # After the turn and before anything reads the geometry, as --up is:
        # every reported coordinate is measured from the chosen zero.
        solid, at = place_origin(solid, origin, allowance)
        meta["origin"] = ",".join(origin)
        meta["origin_at"] = at
    faces = solid.Faces()

    # The reference height for the topmost-plane check, from its own pass so
    # classification order can't matter.
    up_plane_zs = []
    for f in faces:
        if f.geomType() == "PLANE":
            pnt, n = face_normal(f)
            if n.Z() > COS_TOL:
                up_plane_zs.append(pnt.Z())
    z_top = max(up_plane_zs, default=None)
    # The blank's top: the model's plus the allowance the stock proposal
    # adds (slice 11), or the given blank's own margin above the model
    # (slice 12). The passes that read the model (the topmost plane, the
    # pockets, the profiles) keep z_top; the bores, authored from the stock
    # top, read this.
    top_margin = allowance["top"]
    side_margin = max(allowance[k] for k in MARGIN_KEYS[2:])
    if blank is not None:
        side_margin, top_margin = blank_margins(blank, faces, solid.BoundingBox())
        if top_margin > 0.0:
            meta["allowance"] = top_margin
    blank_top = None if z_top is None else z_top + top_margin

    emap = TopTools_IndexedDataMapOfShapeListOfShape()
    TopExp.MapShapesAndAncestors_s(solid.wrapped, TopAbs_ShapeEnum.TopAbs_EDGE,
                                   TopAbs_ShapeEnum.TopAbs_FACE, emap)

    def face_index(shape):
        for j, g in enumerate(faces, start=1):
            if g.wrapped.IsSame(shape):
                return j
        return None

    # The grouping passes (multi-face proposals) run before the per-face
    # classifiers: first holes/bores (every graduated bore wall becomes a
    # group, possibly absorbing its bottom face), then pockets, so every
    # below-top +Z plane is already absorbed, claimed, or refused when the
    # loop reaches it -- then outer profiles, whose caps anchor but are
    # never members (a topmost cap keeps its Floor proposal; a below-top
    # cap keeps its residue entry, reason rewritten via cap_note).
    member_of, cyl_residue = recognize_holes(solid, faces, emap, face_index)
    # The mouth-round pass rides directly on the hole pass's groups: a blend
    # tangent to a graduated bore's wall and the top plane is that bore's
    # edge_round, absorbed before anything else can claim or misread it.
    member_of.update(recognize_lip_rounds(faces, z_top, member_of, face_index))
    # The mouth bevel, the round's sibling (Phase 46 slice 3): absorbed into
    # the bore's group before the rim is placed.
    member_of.update(recognize_mouth_chamfers(faces, member_of, face_index, emap))
    # Then the rim, against the rim the round pass may have lifted: a bore
    # below the stock top is placed by `z=` (Phase 45).
    place_sunken_bores(z_top, member_of, solid, blank_top)
    # Once every hole says where it starts: two stretches of one drilled
    # hole across a gap are one hole.
    join_split_holes(member_of, faces)
    pocket_member_of, floor_fail = recognize_pockets(
        solid, faces, z_top, emap, face_index, member_of, side_margin)
    member_of.update(pocket_member_of)
    outer_member_of, wall_fail, cap_note = recognize_outer_profiles(
        solid, faces, z_top, emap, face_index, member_of)
    member_of.update(outer_member_of)
    settle_heads(member_of, wall_fail, cap_note, floor_fail, faces, emap,
                 face_index, solid.BoundingBox().zmin)
    demote_floating_heads(member_of, wall_fail, cap_note, floor_fail, faces)
    # -- the rim (slice 11): when the blank stands outside the model, the
    # face bounding the plan is a boss to bring to size, claimed before
    # the trim-wire pass would refuse it as a wall with a curved top edge.
    if blank is None:
        # The rim is only found on a model a round blank fits, and that
        # blank has one margin on its sides.
        rim_side = max(allowance[k] for k in MARGIN_KEYS[2:])
        if rim_side > 0.0 and plan_bounding_cylinder(faces, solid.BoundingBox()) is not None:
            rim_side = round_side(allowance)
        rim_member_of, rim_note = recognize_rim(solid, faces, emap, member_of, rim_side)
        member_of.update(rim_member_of)
    else:
        rim_note = withdraw_rim_for_given(solid, faces, member_of, given)
    # -- then trim wires, last: a wall loop is a trim-wire candidate only
    # when no cap/floor-anchored pass claimed it, and a planar-topped loop
    # skips silently, so the earlier passes always win their own ground.
    trim_member_of, trim_fail, bottom_note = recognize_trim_wires(
        solid, faces, emap, face_index, member_of)
    member_of.update(trim_member_of)
    # -- and the level the settled heads stand over, once every proposal is
    # known: a Plane says nothing about where the floor ends, so it waits
    # for the whole account.
    recognize_levels(member_of, wall_fail, cap_note, floor_fail, faces, emap,
                     face_index)
    # -- and last of all, what this setup's tool cannot reach: a face with
    # the part's own material straight above it is the other setup's,
    # whatever a classifier made of it. A proposal whose primary face is
    # covered (a counterbore seen from the back: a void under a ceiling) is
    # withdrawn whole, every member saying so.
    covered = covered_from_above(solid, faces, face_index)
    other_setup = {}
    for g in {id(g): g for g in member_of.values()}.values():
        p = g["primary"]
        if p in covered:
            kind_p = faces[p - 1].geomType().lower()
            for m in g["members"]:
                cover = covered.get(m, covered[p])
                other_setup[m] = other_setup_residue(
                    faces[m - 1].geomType().lower(), cover, faces, up,
                    grouped_with=None if m == p else p)
                member_of.pop(m, None)
    # A downward face says itself that nothing from above reaches it, a face
    # under a head has an entry naming the head and what the program does
    # about it, and the part bottom a trim wire anchors on is its anchor:
    # those keep their words.
    keep_words = ("plane_downward", "under_overhang", "trim_wire_anchor_bottom")

    entries = []
    proposals = []  # (face_id, why, checklist_row, feature_text, extra_comments)
    for i, f in enumerate(faces, start=1):
        fid = f"F{i}"
        kind = f.geomType().lower()
        if i in other_setup:
            c = other_setup[i]
        elif i in member_of:
            # Absorbed into a grouped proposal (a pocket's floor + walls, or
            # a hole/bore wall + its bottom): every member face carries the
            # shared feature block verbatim (the totality invariant's
            # "every proposal has a part" holds per face), plus the group
            # name in the optional `feature` field when the group has more
            # than one member. proposed.part gets the block once, at the
            # primary face's traversal position (the pocket's floor, the
            # hole/bore's wall).
            g = member_of[i]
            c = dict(status="proposed", checklist_row=g["row"],
                     why=g["whys"][i])
            if len(g["members"]) > 1:
                c["feature"] = g["name"]
            part = g["part"]
            if i == g["primary"]:
                proposals.append((fid, g["whys"][i], g["row"], part,
                                  g["extras"]))
        elif i in rim_note:
            # The blank's rim that does not stand on the part bottom from
            # this side (slice 11), or on a given blank (slice 12): the
            # other setup's.
            c = rim_note[i]
        elif i in wall_fail:
            # A wall of a refused outer-profile candidate, or a strip left
            # out of a recognized one (uneven bevels): the loop-level
            # check it failed beats the generic per-face texts (and, for a
            # cylindrical wall, the hole pass's residue dict). Every branch
            # below carries a residue() entry whole -- code, reason, row,
            # refs travel together, never re-assembled here.
            c = wall_fail[i]
        elif i in trim_fail:
            # A wall of a refused trim-wire candidate: same convention.
            c = trim_fail[i]
        elif i in bottom_note:
            # The part-bottom anchor of a recognized trim wire: still
            # residue (a downward face proposes nothing), reason rewritten
            # to point at what anchors on it (the cap_note convention).
            c = bottom_note[i]
        elif i in floor_fail and "anchor" in cap_note.get(i, {}):
            # A below-top +Z plane that caps a recognized outer profile -- a
            # LEDGE, a step's top. It was residue for one stated reason: "a
            # below-top plane has no Floor proposal of its own." It has one
            # now (Phase 28): the bounded `Face`, lifted straight from how STEP
            # states it -- the outer wire, then every inner wire. What stands
            # on an inner wire proposes beside it through its own classifier,
            # and the engine derives it as the face's island from containment.
            pnt, _ = face_normal(f)
            loops = face_loops(f)
            c = dict(status="proposed", checklist_row="1.1",
                     why=f"planar, normal +Z, below the topmost height: a "
                         f"bounded face at z={fmt(pnt.Z())}, {len(loops)} loop"
                         f"{'' if len(loops) == 1 else 's'} (outer"
                         f"{'' if len(loops) == 1 else f' + {len(loops) - 1} inner'}); "
                         f"caps recognized outer profile {cap_note[i]['anchor']}")
            part = face_part(f"{fid}_Face", pnt.Z(), loops)
        elif i in floor_fail:
            # cap_note rewrites a below-top outer-profile anchor cap's
            # reason (the pocket pass's refusal stands otherwise).
            c = cap_note.get(i, floor_fail[i])
        elif i in cyl_residue:
            c = cyl_residue[i]
        elif kind == "plane":
            c = classify_plane(f, z_top)
            if c["status"] == "proposed":
                pnt, _ = face_normal(f)
                part = f"feature {fid}_Top Plane(z={fmt(pnt.Z())})"
        elif kind == "sphere":
            c = classify_sphere(f)
            if c["status"] == "proposed":
                part = (f"feature {fid}_Dome Sphere(dia={fmt(c.pop('dia'))}, "
                        f"x={fmt(c.pop('x'))}, y={fmt(c.pop('y'))}, "
                        f"z={fmt(c.pop('z'))})")
        elif kind == "cone":
            c = classify_cone(f)
            if c["status"] == "proposed":
                part = (f"feature {fid}_Boss Cone(dia={fmt(c.pop('dia'))}, "
                        f"height={fmt(c.pop('height'))}, x={fmt(c.pop('x'))}, "
                        f"y={fmt(c.pop('y'))}, z={fmt(c.pop('z'))})")
        elif kind == "torus":
            c = classify_torus(f, emap)
            if c["status"] == "proposed":
                part = (f"feature {fid}_Ring Torus(major_dia={fmt(c.pop('major_dia'))}, "
                        f"minor_dia={fmt(c.pop('minor_dia'))}, x={fmt(c.pop('x'))}, "
                        f"y={fmt(c.pop('y'))}, z={fmt(c.pop('z'))})")
        else:
            c = residue(code="surface_not_graduated", row="no row",
                        reason=f"surface type '{kind}' not yet graduated to a "
                               "proposable primitive; recognition breadth is "
                               "a later slice")

        if (i in covered and c.get("code") not in keep_words
                and (c["status"] == "unsupported" or i not in member_of)):
            c = other_setup_residue(kind, covered[i], faces, up)

        entry = dict(id=fid, surface=kind, **c)
        if c["status"] == "proposed":
            entry["part"] = part
            if i not in member_of:  # group proposals were appended once above
                proposals.append((fid, c["why"], c["checklist_row"], part, []))
        entries.append(entry)

    return solid, entries, proposals, meta


def build_outputs(step_path, entries, proposals, n_faces, meta):
    # The totality invariant, asserted before anything is written
    # (CONTRACT.md "The totality invariant").
    assert len(entries) == n_faces, "report must account for every face"
    for e in entries:
        assert e["status"] in ("proposed", "unsupported"), e
        assert e["checklist_row"], e
        assert "reason" in e or "part" in e, e
        if e["status"] == "unsupported":
            assert e.get("code"), e
            assert "refs" not in e or e["refs"], e

    report = {
        "schema": SCHEMA,
        "version": VERSION,
        "source": step_path.name,
        "units": "mm",
        "faces_total": n_faces,
        "faces": entries,
        # Setup declarations (--solid / --up), additive under v1: present
        # only when the import needed one, so single-solid Z-up reports --
        # and their committed goldens -- are unchanged.
        **meta,
    }
    report_text = json.dumps(report, indent=2, sort_keys=True) + "\n"

    part_text = None
    if proposals:
        lines = [
            f"# Proposed features recognized from {step_path.name} by "
            "sidecar/import_step.py",
            "# (contract v1 -- see sidecar/CONTRACT.md). Review before use:",
            "# recognition proposes, it does not machine.",
        ]
        # A reviewer must see the setup the coordinates assume.
        if "solid" in meta:
            lines.append(f"# solid {meta['solid']} of {meta['solids_total']} "
                         f"in the file (--solid {meta['solid'][1:]})")
        if "up" in meta:
            lines.append(f"# imported with --up {meta['up']} (model {meta['up']} "
                         "is machine +Z) -- coordinates are post-rotation")
        if "origin_at" in meta:
            lines.append(origin_note(meta["origin"].split(","), meta["origin_at"]))
        elif "origin" in meta:
            lines.append(f"# imported with --origin {meta['origin']}, read on the given blank: "
                         f"the earlier program's zero is at "
                         f"({', '.join(fmt(c) for c in meta['given_at'])}) here")
        if "given" in meta:
            lines += textwrap.wrap(
                f"imported with --given {meta['given']}: the blank is the earlier "
                f"program's turned over, {meta['blank']} (--blank); what that program "
                "cut is air here (given from), and the plan-bounding rim stands as it "
                "left it", width=78, initial_indent="# ", subsequent_indent="# ")
        if "margins" in meta:
            lines.append(f"# imported with the blank standing outside the model by "
                         f"{meta['margins']} (--allowance)")
        elif "allowance" in meta:
            lines.append(f"# imported with --allowance {fmt(meta['allowance'])}: the blank "
                         f"stands {fmt(meta['allowance'])} above the model's top and outside "
                         "its plan; bores at the model's top are drilled from the blank top")
            lines.append("# or placed by z=, and the plan-bounding rim is proposed as a boss "
                         "to bring to size")
        for fid, why, row, part, extras in proposals:
            lines += ["", f"# {fid}: {why} (CHECKLIST row {row})"] + extras + [part]
        part_text = "\n".join(lines) + "\n"
    return report_text, part_text


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("step", type=Path, help="input STEP file")
    ap.add_argument("-o", "--outdir", type=Path, required=True,
                    help="directory for report.json / proposed.part")
    ap.add_argument("--solid", type=int, default=None, metavar="N",
                    help="which solid is the part (S1..SM, required for a "
                         "multi-solid STEP)")
    ap.add_argument("--up", choices=sorted(UP_ROTATIONS), default="z+",
                    help="which MODEL axis points up on the machine "
                         "(default z+)")
    ap.add_argument("--allowance", default="0", metavar="MM[,SIDE=MM...]",
                    help="how far the blank stands above the model's top and "
                         "outside its plan (default 0: a blank cut to size); "
                         "then any of top=, bottom=, x-=, x+=, y-=, y+= for a "
                         "side of its own, e.g. 1,bottom=3,x+=10")
    ap.add_argument("--blank", metavar="WORDS", default=None,
                    help="the blank as given, the earlier program's turned over: "
                         "Cylinder(dia=, h=, x=, y=, z=) or Box(w=, l=, h=, x=, y=, z=); "
                         "with --given")
    ap.add_argument("--given", metavar="PROJECT", default=None,
                    help="the earlier program's project as `given from` will name it; "
                         "with --blank")
    ap.add_argument("--given-origin", metavar="X,Y,Z", default="0,0,0",
                    help="with --blank: the model point the earlier program zeroed "
                         "at (its report's origin_at), so the model is read in the "
                         "frame the given blank is written in")
    ap.add_argument("--origin", metavar="X,Y,Z", default=None,
                    help="the point that becomes 0,0,0: per axis `model`, a number "
                         "(a model coordinate), min, mid or max of the blank, or "
                         "part-min, part-mid or part-max of the model; e.g. "
                         "mid,mid,max is the blank's centre and its top "
                         "(default: the model's own origin)")
    args = ap.parse_args()
    try:
        allowance = parse_allowance(args.allowance)
    except ValueError as e:
        sys.exit(f"error: {e}")
    origin = None
    if args.origin is not None:
        try:
            origin = parse_origin(args.origin)
        except ValueError as e:
            sys.exit(f"error: {e}")
    try:
        given_origin = parse_point(args.given_origin, "--given-origin")
    except ValueError as e:
        sys.exit(f"error: {e}")
    blank = None
    if (args.blank is None) != (args.given is None):
        sys.exit("error: --blank and --given go together: the earlier program's blank "
                 "turned over, and the project it came from")
    if args.blank is not None:
        if any(allowance.values()):
            sys.exit("error: --allowance with --blank: the given blank says how far it "
                     "stands outside the model")
        try:
            blank = parse_blank(args.blank)
        except ValueError as e:
            sys.exit(f"error: {e}")

    try:
        solid, entries, proposals, meta = recognize(args.step, args.solid,
                                                    args.up, allowance,
                                                    blank, args.given, origin,
                                                    given_origin)
        n_faces = len(solid.Faces())
        report_text, part_text = build_outputs(args.step, entries, proposals,
                                               n_faces, meta)
    except Exception as e:
        sys.exit(f"error: {args.step}: {e}")

    args.outdir.mkdir(parents=True, exist_ok=True)
    (args.outdir / "report.json").write_text(report_text, encoding="utf-8", newline="\n")
    if part_text is not None:
        (args.outdir / "proposed.part").write_text(part_text, encoding="utf-8", newline="\n")

    # Count face statuses, not proposals -- a grouped pocket is one proposal
    # owned by several proposed faces, and the summary must agree with the
    # report's totality arithmetic.
    n_prop = sum(1 for e in entries if e["status"] == "proposed")
    print(f"{n_faces} faces: {n_prop} proposed, {n_faces - n_prop} unsupported")


if __name__ == "__main__":
    main()
