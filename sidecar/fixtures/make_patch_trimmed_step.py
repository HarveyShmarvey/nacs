#!/usr/bin/env python3
"""Generate tests/fixtures/patch_trimmed.step — the TRIMMED-patch refusal
fixture (DECISIONS.md "the general NURBS patch…", slice 1: a trimmed face
refuses by name, pointing at the mesh path).

The same bicubic Bézier surface as patch_cap.step, but the face is built over
the SUB-domain [0.2, 0.8]² of the surface's natural [0,1]² — a trimmed patch.
Reading it as its full rectangular domain would invent surface the part does
not have, so import_step_nurbs.py must refuse the whole part (there is nothing
else to read) with a message that counts the trimmed faces and points at
import_step_mesh.py. The prism gives the same 6-face solid shape as the cap
fixture: trimmed patch top and bottom, four extrusion walls.

Committed once, regenerated only deliberately. Run from the repo root:

  sidecar/.venv/bin/python sidecar/fixtures/make_patch_trimmed_step.py
"""

import sys
from pathlib import Path

import cadquery as cq
from cadquery.occ_impl.shapes import Shape
from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeFace
from OCP.BRepPrimAPI import BRepPrimAPI_MakePrism
from OCP.gp import gp_Vec

from make_patch_cap_step import bezier_cap_surface

EXPECTED_FACES = 6
EXPECTED_BSPLINE = 2
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "patch_trimmed.step"


def build():
    surf = bezier_cap_surface()
    face = BRepBuilderAPI_MakeFace(surf, 0.2, 0.8, 0.2, 0.8, 1e-6).Face()
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
