#!/usr/bin/env python3
"""Generate tests/fixtures/mushroom.step — a head overhanging the neck it
stands on, with nothing under it but the part bottom.

ROADMAP item 1, slice 5: the real part's ribs are heads on narrower necks.
A head's walls end on its own underside, a downward plane above the part
bottom, which the outer-profile pass refused as an undercut. The wall is
traced from above all the same, with air under the tool at the base, so
the head proposes with the note that the underside is the other setup's --
unless a proposed floor lower than its base hosts it, which is the engine's
own refusal (rib_head.step shows that case). Here the neck stands on the
part bottom, so there is no such floor and the head proposes.

  neck    20 x 20, 6 tall, at 20..40 x 20..40 x 0..6
  head    40 x 40, 8 tall, at 10..50 x 10..50 x 6..14

Expected faces: neck 4 walls + bottom 1 + head 6 (top, underside, 4 walls)
= 11.

Run from the repo root:

  sidecar/.venv/Scripts/python.exe sidecar/fixtures/make_mushroom.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = 11
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "mushroom.step"


def build():
    neck = cq.Workplane("XY", origin=(20, 20, 0)).box(20, 20, 6, centered=False)
    head = cq.Workplane("XY", origin=(10, 10, 6)).box(40, 40, 8, centered=False)
    return neck.union(head)


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
