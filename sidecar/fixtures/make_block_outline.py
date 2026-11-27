#!/usr/bin/env python3
"""Generate tests/fixtures/block_outline.step — a head on a narrower neck
whose top a pocket opens through, with rounded top edges and one bevelled.

ROADMAP item 9. The head's outline has three things a plain one lacks: the
top face's boundary runs round the pocket's mouth, three top edges carry a
round where the fourth is a bevel as deep as the head, and the bevel's ends
are blended into the walls beside it. A boss in the pocket stands taller
than the head, so the head's top is a level and not the part's top.

  base     60 x 40, z 0..10
  neck     z 10..14, set in 4 on +x, -y and +y (a groove under the head)
  head     60 x 40, z 14..26
  bevel    45 degrees, 12 down the head's -x top edge
  pocket   y 12..28, in from the bevel to x=40, floor at z=21, R1 mouth round
  rounds   R1 on the head's other three top edges and along the bevel's ends
  boss     dia 8 at (32, 20), from the pocket's floor up to z=30

Run from the repo root:

  sidecar/.venv/Scripts/python.exe sidecar/fixtures/make_block_outline.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = 29
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "block_outline.step"


def build():
    box = cq.selectors.BoxSelector
    base = cq.Workplane("XY").box(60, 40, 10, centered=False)
    neck = cq.Workplane("XY", origin=(0, 4, 10)).box(56, 32, 4, centered=False)
    head = cq.Workplane("XY", origin=(0, 0, 14)).box(60, 40, 12, centered=False)
    # The bevel takes the whole -x wall, so it is cut as a wedge: a chamfer
    # as deep as its wall is one the kernel refuses.
    wedge = (cq.Workplane("XZ", origin=(0, 41, 0)).polyline([(-1, 13), (13, 27), (-1, 27)]).close()
             .extrude(42))
    head = head.cut(wedge)
    cutter = cq.Workplane("XY", origin=(-1, 12, 21)).box(41, 16, 8, centered=False)
    head = head.cut(cutter)
    head = head.edges(box((6, 11, 25.9), (41, 29, 26.1))).fillet(1)
    # The three square top edges, and the two sloping edges at the bevel's ends.
    outer = (head.edges(box((11, -1, 25.9), (61, 0.1, 26.1))).vals()
             + head.edges(box((11, 39.9, 25.9), (61, 41, 26.1))).vals()
             + head.edges(box((59.9, -1, 25.9), (61, 41, 26.1))).vals()
             + head.edges(box((-1, -0.1, 13.9), (13, 0.1, 26.1))).vals()
             + head.edges(box((-1, 39.9, 13.9), (13, 40.1, 26.1))).vals())
    head = head.newObject(outer).fillet(1)
    boss = cq.Workplane("XY", origin=(32, 20, 21)).circle(4).extrude(9)
    return base.union(neck).union(head).union(boss)


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
