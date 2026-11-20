#!/usr/bin/env python3
"""Generate tests/fixtures/pocket_post.step — the bossed-pocket fixture.

The STEP twin of pocket_post.cam / pocket_islands.cam (Phase 26 slice 4): one
plate, one pocket, and standing material left inside it.

  plate 100 x 100 x 20 mm, corner at the origin, top at z=20
  pocket  60 x 60, x 20..80, y 20..80, floor z=15 (depth 5)
  A  round post   dia 12 at (38, 40), floor to mouth -- a full-revolution
                  convex cylinder: proposes as CylinderBoss
  B  square pad   14 x 10, x 44..58, y 60..70, floor to mouth -- four convex
                  planar walls: proposes as ProfileBoss

The pocket floor is ONE face with TWO inner wires. Until this slice that
refused the floor (`pocket_islands`: "island pocketing is not supported");
the engine clears around islands now, so the floor proposes its OUTER loop as
the pocket and each inner wire proposes the boss standing on it, beside it --
no new syntax, the island derived from containment at parse.

The two are placed so each one's finish can ENTER: the boss engine leads in
one tool diameter out along +X and the profile engine leads in well clear of
its corner, and first drawings that put a neighbour or the wall there were
refused by check_moves -- correctly. A bossed pocket needs that room.

Expected faces: plate 6 + pocket floor 1 + 4 pocket walls + post wall 1 +
post top 1 + pad 4 walls + pad top 1 = 18.

Run from the repo root:

  sidecar/.venv/bin/python sidecar/fixtures/make_pocket_post.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = 18
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "pocket_post.step"


def build():
    plate = cq.Workplane("XY").box(100, 100, 20, centered=False)
    pocket = (cq.Workplane("XY", origin=(20, 20, 20))
              .rect(60, 60, centered=False).extrude(-5))
    post = (cq.Workplane("XY", origin=(38, 40, 15)).circle(6).extrude(5))
    pad = (cq.Workplane("XY", origin=(44, 60, 15))
           .rect(14, 10, centered=False).extrude(5))
    return plate.cut(pocket).union(post).union(pad)


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
