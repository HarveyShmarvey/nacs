#!/usr/bin/env python3
"""Generate tests/fixtures/through_chamfer.step — the far mouth fixture.

Phase 48 slice 3 (fork 2A of the Phase 48 DRAFT): a through hole whose mouth
on the FAR face is beveled. Imported this way up (`--up z+`) the bevel is a
full-revolution cone at the bore's bottom, widening downward into the far
face -- the other setup's chamfer, which the recognizer used to refuse as
`cone_cavity_undercut` and leave the bore stopping at the cone's top. Now the
bore proposes THROUGH to the far face and the cone is a member of its group,
named as the other setup's work. Imported turned over (`--up z-`) the same
cone is the mouth bevel at the top, absorbed as `chamfer_depth=` by the Phase
46 pass -- the two imports agree on the hole's column, which `given from`'s
certificate reads.

  plate   60 x 40 x 12, corner at the origin                       -> top at z=12
  hole    dia 12, through, at (20, 20), 1 mm chamfer on its BOTTOM mouth
  blind   dia 8, 7 deep, at (45, 20), flat floor -- a control that must not move

Expected faces: plate 6 + hole 2 (wall, cone) + blind 2 (wall, floor) = 10.

Run from the repo root:

  sidecar/.venv/Scripts/python.exe sidecar/fixtures/make_through_chamfer.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = 10
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "through_chamfer.step"


def build():
    plate = cq.Workplane("XY").box(60, 40, 12, centered=False)
    hole = cq.Workplane("XY", origin=(20, 20, 0)).circle(6).extrude(12)
    solid = plate.cut(hole)
    # The bevel on the hole's BOTTOM rim: the edge at z=0 inside the hole's box.
    solid = solid.edges(cq.selectors.BoxSelector((13, 13, -0.1), (27, 27, 0.1))).chamfer(1.0)
    blind = cq.Workplane("XY", origin=(45, 20, 5)).circle(4).extrude(7)
    return solid.cut(blind)


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
