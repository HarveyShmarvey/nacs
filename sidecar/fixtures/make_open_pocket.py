#!/usr/bin/env python3
"""Generate tests/fixtures/open_pocket.step — a block with a bevel along one
top edge and a pocket that runs out onto it.

ROADMAP item 9. The pocket has three things a plain one lacks: a fillet
where the floor meets the walls, a round on the mouth, and one side with no
wall at all, where the floor meets the bevel and the bevel falls away.

  block    60 x 40 x 20
  bevel    45 degrees, 8 down the -x top edge
  pocket   y 8..32, in from the bevel to x=45, floor at z=14,
           R5 corners at the +x end, R1 floor fillet, R1 mouth round

Run from the repo root:

  sidecar/.venv/Scripts/python.exe sidecar/fixtures/make_open_pocket.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = 23
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "open_pocket.step"


def build():
    block = cq.Workplane("XY").box(60, 40, 20, centered=False)
    block = block.edges(cq.selectors.BoxSelector((-1, -1, 19), (1, 41, 21))).chamfer(8)
    cutter = (cq.Workplane("XY", origin=(-1, 8, 14)).box(46, 24, 8, centered=False)
              .edges(cq.selectors.BoxSelector((44, 7, 13), (46, 33, 23))).edges("|Z").fillet(5)
              .faces("<Z").edges().fillet(1))
    part = block.cut(cutter)
    rim = cq.selectors.BoxSelector((7, 7, 19.9), (46, 33, 20.1))
    return part.edges(rim).fillet(1)


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
