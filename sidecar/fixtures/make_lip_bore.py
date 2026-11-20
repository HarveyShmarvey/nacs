#!/usr/bin/env python3
"""Generate tests/fixtures/lip_bore.step — the mouth-round recognition fixture
(DECISIONS.md -> "sidecar recognition of a bore's mouth round").

One plate, one blind flat-floor bore, and the bore's mouth edge filleted —
the surface a corner rounder leaves, arriving from CAD instead of being
hand-declared:

  plate 60 x 60 x 15 mm, corner at the origin
  blind bore  dia 24, centred (30, 30), from the plate top (z=15) down 10
  mouth round r=3 on the bore's top edge

Expected faces: plate bottom, 4 side walls, plate top, bore wall (now
stopping 3 under the top), bore floor, and the quarter-torus blend = 9. The
blend is the recognition target: a full-revolution toroidal face, material
outside the tube — the exact shape classify_torus refuses as an internal
groove when it stands alone — tangent to the recognized bore wall below and
the top plane above, so it is a declared edge_round on the bore, not residue.

The STEP file is committed once and regenerated only deliberately — STEP
headers carry a timestamp, so a regeneration is a real reviewable diff, not
noise. Run from the repo root:

  sidecar/.venv/bin/python sidecar/fixtures/make_lip_bore.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = 9
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "lip_bore.step"


def build():
    plate = cq.Workplane("XY").box(60, 60, 15, centered=False)
    bore = cq.Workplane("XY", origin=(30, 30, 15)).circle(12).extrude(-10)
    solid = plate.cut(bore)
    # The bore's mouth circle is the only circular edge on the top face.
    return solid.faces(">Z").edges("%CIRCLE").fillet(3.0)


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
