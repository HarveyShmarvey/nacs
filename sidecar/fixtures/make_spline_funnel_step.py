#!/usr/bin/env python3
"""Generate tests/fixtures/spline_funnel.step — the analytic reader's cavity
fixture (DECISIONS.md "Spline funnel…").

One plate carrying a single freeform revolved CAVITY whose meridian is a
b-spline — the inside-orientation twin of spline_cap.step: a funnel cut into
the plate top, mouth widest at the top, narrowing monotonically (radius
non-increasing, height non-increasing mouth to tip) to a flat-floored tip, so
the revolved wall is a machinable funnel with no undercut. The wall's oriented
outward normal points INTO the void (out of the material), which is exactly
what import_step_nurbs.py's cavity detection reads — the fixture exists to pin
that branch: mouth-first `.spline` emission and a `SplineRevolveFunnel`
proposal with no z field (the mouth height goes in a comment).

  plate   50 x 50 x 16 mm, centred on the origin, sitting on z=0
  cavity  a monotone spline funnel cut from the plate top (z=16): mouth r=13
          in the top plane, tip r=5 at depth 12 (flat floor below — the
          reader takes only the revolved wall; the floor and top ring are
          analytic planes it skips)

The Rust round-trip test then parses AND machines the emitted meridian as a
SplineRevolveFunnel (tests/sidecar_tests.rs::test_spline_revolve_funnel_round_trip),
proving it survives the trip as a machinable cavity. make_* scripts pin the
geometry so a kernel change that alters the face split is a loud test failure.

The STEP file is committed once and regenerated only deliberately (STEP headers
carry a timestamp, so a regeneration is a real reviewable diff). Run from the
repo root:

  sidecar/.venv/bin/python sidecar/fixtures/make_spline_funnel_step.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = 8
EXPECTED_REVOLUTION = 1  # the funnel wall; the rest are analytic planes
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "spline_funnel.step"


def build():
    # The cutter: a solid of revolution whose outer profile is the funnel
    # meridian, mouth-first and monotone both ways (r down, z down), closed to
    # the axis so the revolve is a solid. Drawn in XZ (x = r, the workplane's
    # y = world z), tip padded 1mm above the mouth so the cut clears the top
    # face rather than leaving a sliver.
    prof = (
        cq.Workplane("XZ")
        .moveTo(13, 1)
        .lineTo(13, 0)
        .spline([(13, 0), (11, -4), (8, -8), (6, -10), (5, -12)])
        .lineTo(0, -12)
        .lineTo(0, 1)
        .close()
    )
    cutter = prof.revolve(360, (0, 0, 0), (0, 1, 0))
    plate = cq.Workplane("XY").box(50, 50, 16, centered=(True, True, False))
    return plate.cut(cutter.translate((0, 0, 16)))


def main():
    solid = build()
    faces = solid.val().Faces()
    n = len(faces)
    revolution = [f for f in faces if f.geomType() == "REVOLUTION"]
    if n != EXPECTED_FACES or len(revolution) != EXPECTED_REVOLUTION:
        sys.exit(
            f"error: fixture has {n} faces ({len(revolution)} revolution), "
            f"expected {EXPECTED_FACES} ({EXPECTED_REVOLUTION} revolution) — the "
            "geometry (or the kernel's face split) changed"
        )
    OUT.parent.mkdir(parents=True, exist_ok=True)
    cq.exporters.export(solid, str(OUT))
    print(f"wrote {OUT} ({n} faces, {len(revolution)} revolution)")


if __name__ == "__main__":
    main()
