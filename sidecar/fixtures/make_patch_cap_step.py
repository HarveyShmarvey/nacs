#!/usr/bin/env python3
"""Generate tests/fixtures/patch_cap.step — the general-NURBS-patch reader
fixture (DECISIONS.md "the general NURBS patch…", slice 1).

A solid whose TOP face is an untrimmed (full-natural-domain) bicubic b-spline
patch — deliberately the same hand-checkable Bézier net as the Rust fixture
nurbs_patch.cam/cap.patch (x, y poles on the 0/8/16/24 grid, perimeter z = 0,
inner 2x2 at z = 16, apex at (12, 12, 9) exactly), so the reader's output is
verifiable against a surface whose numbers are already pinned end to end. The
solid is the patch extruded straight down (a prism): the prism's BOTTOM face is
the same untrimmed patch translated and facing DOWN, which pins the reader's
downward-face skip (only the top face is read), and the four side walls are
extrusion surfaces the reader ignores.

import_step_nurbs.py reads the top face's control net exactly into a `.patch`
file + a NurbsPatch proposal; the Rust round-trip test then parses AND machines
it. make_* scripts pin the geometry so a kernel change that alters the face
split is a loud test failure.

The STEP file is committed once and regenerated only deliberately (STEP headers
carry a timestamp, so a regeneration is a real reviewable diff). Run from the
repo root:

  sidecar/.venv/bin/python sidecar/fixtures/make_patch_cap_step.py
"""

import sys
from pathlib import Path

import cadquery as cq
from cadquery.occ_impl.shapes import Shape
from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeFace
from OCP.BRepPrimAPI import BRepPrimAPI_MakePrism
from OCP.Geom import Geom_BSplineSurface
from OCP.gp import gp_Pnt, gp_Vec
from OCP.TColgp import TColgp_Array2OfPnt
from OCP.TColStd import TColStd_Array1OfInteger, TColStd_Array1OfReal

EXPECTED_FACES = 6
EXPECTED_BSPLINE = 2  # the patch on top, its translated twin on the bottom
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "patch_cap.step"


def bezier_cap_surface():
    """The nurbs_patch.cam net as a Geom_BSplineSurface: single-span clamped
    bicubic (a pure Bézier patch), natural domain [0,1]²."""
    grid = [0.0, 8.0, 16.0, 24.0]
    poles = TColgp_Array2OfPnt(1, 4, 1, 4)
    for iu in range(4):
        for iv in range(4):
            inner = iu in (1, 2) and iv in (1, 2)
            poles.SetValue(iu + 1, iv + 1, gp_Pnt(grid[iu], grid[iv], 16.0 if inner else 0.0))
    knots = TColStd_Array1OfReal(1, 2)
    knots.SetValue(1, 0.0)
    knots.SetValue(2, 1.0)
    mults = TColStd_Array1OfInteger(1, 2)
    mults.SetValue(1, 4)
    mults.SetValue(2, 4)
    return Geom_BSplineSurface(poles, knots, knots, mults, mults, 3, 3, False, False)


def build():
    surf = bezier_cap_surface()
    face = BRepBuilderAPI_MakeFace(surf, 1e-6).Face()
    return Shape.cast(BRepPrimAPI_MakePrism(face, gp_Vec(0.0, 0.0, -12.0)).Shape())


def main():
    solid = build()
    faces = solid.Faces()
    n = len(faces)
    bspline = [f for f in faces if f.geomType() == "BSPLINE"]
    if n != EXPECTED_FACES or len(bspline) != EXPECTED_BSPLINE:
        sys.exit(
            f"error: fixture has {n} faces ({len(bspline)} b-spline), expected "
            f"{EXPECTED_FACES} ({EXPECTED_BSPLINE} b-spline) — the geometry (or the "
            "kernel's face split) changed"
        )
    OUT.parent.mkdir(parents=True, exist_ok=True)
    cq.exporters.export(solid, str(OUT))
    print(f"wrote {OUT} ({n} faces, {len(bspline)} b-spline)")


if __name__ == "__main__":
    main()
