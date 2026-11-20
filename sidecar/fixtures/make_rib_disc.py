#!/usr/bin/env python3
"""Generate tests/fixtures/rib_disc.step — the real part's shape in small: a
disc whose heads stand over the floor on necks and pads.

ROADMAP item 1, slice 6. From above, nothing reaches what is under a head --
the neck, the underside, the pad the neck stands on, the pad's 1 mm foot
bevel -- so the head proposes down to the floor it stands over and the
floor proposes as the level, one `Plane`, the whole blank cleared to it
around what stands.

  disc      r 50, 0..20: the floor at z=20
  rim rib   a pad 24 x 26 at the foot tapering 45 deg to 22 x 24 at z=21
            (the head's own outline), a neck 18 x 12 from 21 to 27, a head
            22 x 24 from 27 to 35, all running out past the rim and clipped
            to it, so the rim is one wall from 0 to 35 (the stepped base)
  round rib r 13 at the foot tapering to r 12 at z=21, a neck r 6 from 21
            to 27, a head r 12 from 27 to 35: the CylinderBoss path
  bore      r 5 through the floor at (-15, 30): a void the level goes round

Expected faces: disc 2 (bottom, rim) + floor 1 + rim rib 12 (3 bevels, the
pad top, 3 neck walls, the underside, 3 head walls, top) + round rib 6
(cone, pad top, neck, underside, wall, top) + bore 1 = 22.

Run from the repo root:

  sidecar/.venv/Scripts/python.exe sidecar/fixtures/make_rib_disc.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = 22
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "rib_disc.step"


def build():
    disc = cq.Workplane("XY").circle(50).extrude(20)
    clip = cq.Workplane("XY").circle(50).extrude(40)
    pad = cq.Workplane("XY", origin=(41, 0, 20)).rect(24, 26).extrude(1, taper=45)
    neck = cq.Workplane("XY", origin=(43, 0, 21)).rect(18, 12).extrude(6)
    head = cq.Workplane("XY", origin=(41, 0, 27)).rect(22, 24).extrude(8)
    rib = pad.union(neck).union(head).intersect(clip)
    rpad = cq.Workplane("XY", origin=(-15, 0, 20)).circle(13).extrude(1, taper=45)
    rneck = cq.Workplane("XY", origin=(-15, 0, 21)).circle(6).extrude(6)
    rhead = cq.Workplane("XY", origin=(-15, 0, 27)).circle(12).extrude(8)
    bore = cq.Workplane("XY", origin=(-15, 30, -1)).circle(5).extrude(30)
    return disc.union(rib).union(rpad).union(rneck).union(rhead).cut(bore)


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
