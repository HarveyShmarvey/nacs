#!/usr/bin/env python3
"""Generate tests/fixtures/ledge_face.step — the bounded-face fixture.

Phase 28 slice 4: a planar +Z face that is neither the part's topmost plane nor
a pocket's floor — a LEDGE — proposes as a `Face`: a height and a loop that can
contain loops, lifted straight from how STEP states it (one outer bound, any
number of inner bounds).

  plate  100 x 80 x 10, corner at the origin            -> ledge at z=10
  pad    an ELLIPSE 50 x 30 standing on it, 8 tall, centred (50, 40)
                                                        -> top at z=18

The ledge's outer wire is the plate's rectangle (its walls DESCEND from it, which
is why the pocket-floor pass has always refused it: "a plateau/step top, not a
pocket floor"). Its inner wire is where the pad stands — and it is an ELLIPSE,
which is neither a line nor a circular arc: for a wall profile that still
refuses (`edge_not_line_or_arc` — the chord would BE the surface), but a face's
boundary is overhung by a tool radius or stood off from, so any edge curve is
simply chorded at the one tolerance.

Expected faces: plate 5 (bottom + 4 sides) + ledge 1 + pad wall 1 + pad top 1
= 8.

Run from the repo root:

  sidecar/.venv/bin/python sidecar/fixtures/make_ledge_face.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = 8
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "ledge_face.step"


def build():
    plate = cq.Workplane("XY").box(100, 80, 10, centered=False)
    pad = cq.Workplane("XY", origin=(50, 40, 10)).ellipse(25, 15).extrude(8)
    return plate.union(pad)


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
