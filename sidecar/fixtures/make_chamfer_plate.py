#!/usr/bin/env python3
"""Generate tests/fixtures/chamfer_plate.step — the chamfer fixture.

Phase 46 slice 3: a 45° strip (a planar face that is neither vertical nor
±Z) or a cone at a round mouth, bounded above by a +Z plane and below by a
vertical wall, is that wall's edge break — absorbed into the wall's group and
written as the engine's own keys, `chamfer_depth=` and `chamfer_angle=` (the
included angle, the tool's convention), the loop read at the WALL. A strip
that is a wall's whole height has no flank left under it and refuses by the
engine's sentence (`chamfer_consumes_wall`).

  plate   100 x 80 x 20, corner at the origin                    -> top at z=20
  boss    30 x 20, 8 tall, at 10..40 x 10..30, 1 mm chamfer on its top edges
  pocket  30 x 20, 8 deep, at 55..85 x 10..30, 1 mm chamfer at its mouth
  bore    dia 12, 10 deep, at (70, 60), 1 mm chamfer at its mouth
  pad     20 x 20, 1 tall, at 10..30 x 50..70, 1 mm chamfer -- the whole wall

Expected faces: plate 6 + boss 9 (top, 4 walls, 4 strips) + pocket 9 (floor,
4 walls, 4 strips) + bore 3 (wall, bottom, cone) + pad 5 (top, 4 strips) = 32.

Run from the repo root:

  sidecar/.venv/Scripts/python.exe sidecar/fixtures/make_chamfer_plate.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = 32
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "chamfer_plate.step"


def build():
    plate = cq.Workplane("XY").box(100, 80, 20, centered=False)
    boss = (cq.Workplane("XY", origin=(10, 10, 20)).box(30, 20, 8, centered=False)
            .edges(">Z").chamfer(1.0))
    # The pad is ALL bevel: a 1 mm step whose four sides are 45° strips and
    # nothing vertical under them -- a tapered extrusion (planar strips; a
    # loft makes B-splines, and a 1 mm chamfer on a 1 mm wall is an edge the
    # kernel will not consume).
    pad = cq.Workplane("XY", origin=(20, 60, 20)).rect(20, 20).extrude(1, taper=45)
    solid = plate.union(boss).union(pad)
    # The pocket and the bore are cut, then their mouth edges beveled.
    pocket = cq.Workplane("XY", origin=(55, 10, 12)).box(30, 20, 8, centered=False)
    solid = solid.cut(pocket)
    solid = solid.edges(cq.selectors.BoxSelector((54, 9, 19.9), (86, 31, 20.1))).chamfer(1.0)
    bore = cq.Workplane("XY", origin=(70, 60, 10)).circle(6).extrude(10)
    solid = solid.cut(bore)
    solid = solid.edges(cq.selectors.BoxSelector((63, 53, 19.9), (77, 67, 20.1))).chamfer(1.0)
    return solid


def main():
    solid = build()
    n = len(solid.val().Faces())
    if n != EXPECTED_FACES:
        sys.exit(f"error: fixture has {n} faces, expected {EXPECTED_FACES} — "
                 "the geometry (or the kernel's face merging) changed")
    OUT.parent.mkdir(parents=True, exist_ok=True)
    cq.exporters.export(solid, str(OUT))
    print(f"wrote {OUT} ({n} faces)")


if __name__ == "__main__":
    main()
