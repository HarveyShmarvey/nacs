#!/usr/bin/env python3
"""Generate tests/fixtures/split_hole.step — a block with a groove in one
side and two holes through it, one of them crossing the groove.

A groove across a drilled hole leaves two wall faces on one axis with air
between. They are one hole, drilled once, from either side.

  block    40 x 30 x 20
  groove   z 8..12, cut 15 into the +x side, the whole way along y
  split    r 2 through the block at (32, 15), crossing the groove
  plain    r 2 through the block at (10, 15), clear of it

Run from the repo root:

  sidecar/.venv/Scripts/python.exe sidecar/fixtures/make_split_hole.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = 13
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "split_hole.step"


def build():
    block = cq.Workplane("XY").box(40, 30, 20, centered=False)
    groove = cq.Workplane("XY", origin=(25, 0, 8)).box(15, 30, 4, centered=False)
    split = cq.Workplane("XY", origin=(32, 15, -1)).circle(2).extrude(22)
    plain = cq.Workplane("XY", origin=(10, 15, -1)).circle(2).extrude(22)
    return block.cut(groove).cut(split).cut(plain)


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
