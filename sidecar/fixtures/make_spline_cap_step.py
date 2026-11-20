#!/usr/bin/env python3
"""Generate tests/fixtures/spline_cap.step — the analytic NURBS reader fixture.

One plate carrying a single freeform revolved cap whose meridian is a b-spline —
the same *kind* of face as freeform_cap.step, but shaped as a MACHINABLE cap:
the meridian is monotone (radius non-decreasing, height non-increasing from apex
to base), with no bulge that tucks back under. freeform_cap.step deliberately
bulges (its widest ring sits above its base rim, a slight undercut), which is
correct for the mesh drop-cutter — it just leaves the overhang standing — but
the analytic finisher (finish_spline_revolve) refuses an undercut, so the
reader's happy-path fixture needs a profile that machines end to end:

  plate  50 x 50 x 6 mm, centred on the origin, sitting on z=0
  cap    a monotone spline profile revolved 360 deg about Z, sat on the plate
         top (z=6); apex on the axis, base rim at r≈13

import_step_nurbs.py reads the cap's b-spline meridian exactly into a `.spline`
file + a SplineRevolve proposal; the Rust round-trip test then parses AND
machines it (tests/sidecar_tests.rs::test_spline_revolve_round_trip), proving
the meridian survives the trip as a machinable cap. make_* scripts pin the
geometry so a kernel change that alters the face split is a loud test failure.

The STEP file is committed once and regenerated only deliberately (STEP headers
carry a timestamp, so a regeneration is a real reviewable diff). Run from the
repo root:

  sidecar/.venv/bin/python sidecar/fixtures/make_spline_cap_step.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = 7
EXPECTED_REVOLUTION = 1  # the spline cap; the other six are analytic planes
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "spline_cap.step"


def build():
    # A monotone spline profile in the XZ plane, from the axis (r=0) at the top
    # out and DOWN to the base rim — radius strictly increasing, height strictly
    # decreasing, so the revolved surface is a machinable cap with no undercut.
    prof = (
        cq.Workplane("XZ")
        .moveTo(0, 16)
        .spline([(0, 16), (4, 15), (8, 12), (11, 7), (12.5, 3), (13, 0)])
        .lineTo(0, 0)
        .close()
    )
    cap = prof.revolve(360, (0, 0, 0), (0, 1, 0))
    plate = cq.Workplane("XY").box(50, 50, 6, centered=(True, True, False))
    return plate.union(cap.translate((0, 0, 6)))


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
