#!/usr/bin/env python3
"""Generate tests/fixtures/rib_head.step — a rib running out to the part's
edge, and a head overhanging its neck.

ROADMAP item 1, slice 5: the real part's ribs failed two outer-profile
checks once their bevels were allowed. Their outline runs out to the disc's
rim, so one wall of the loop is the rim's and its base is the part bottom
(a STEPPED base); and every rib is a head on a narrower neck, so the face
under each wall's foot faces down (an OVERHANG). Both now propose, to the
highest base, with the notes saying so.

  plate   100 x 80 x 20, corner at the origin                    -> top at z=20
  rib     20 x 20, 10 tall, at 80..100 x 30..50: its x=100 wall is the
          plate's own side, one face from z=0 to z=30
  neck    20 x 20, 6 tall, at 20..40 x 30..50
  head    30 x 30, 8 tall, at 15..45 x 25..55, on the neck: its walls end
          at z=26 on its own underside

Expected faces: plate 6 (one side shared with the rib) + rib 4 (top, 3
walls) + neck 4 walls + head 6 (top, underside, 4 walls) = 20.

Run from the repo root:

  sidecar/.venv/Scripts/python.exe sidecar/fixtures/make_rib_head.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = 20
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "rib_head.step"


def build():
    plate = cq.Workplane("XY").box(100, 80, 20, centered=False)
    rib = cq.Workplane("XY", origin=(80, 30, 20)).box(20, 20, 10, centered=False)
    neck = cq.Workplane("XY", origin=(20, 30, 20)).box(20, 20, 6, centered=False)
    head = cq.Workplane("XY", origin=(15, 25, 26)).box(30, 30, 8, centered=False)
    return plate.union(rib).union(neck).union(head)


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
