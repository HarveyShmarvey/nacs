#!/usr/bin/env python3
"""STEP -> mesh export: tessellate a part's FREEFORM faces to STL (contract v1).

The sidecar's third tool (ROADMAP Phase 4, the camp-3 follow-up "sidecar
STEP->mesh export for freeform faces"). Recognition (import_step.py) proposes
clean camp-1/2 primitives for every face it can read as an equation — planes,
bores, quadric bosses, pockets, profiles. This tool handles the tail it can't:
the freeform faces (b-spline, bezier, surface of revolution/extrusion, offset)
that no equation captures. It tessellates exactly those faces to a triangle
soup and writes the STL that the Rust core's `Feature::Mesh` already reads —
finished by the raster drop-cutter (DECISIONS.md "Camp-3 slice 1…").

Like the DXF tool, this is NOT a new seam: the output is the same STL form
`src/mesh.rs::read_stl` already consumes, so the Rust core and the `.part`
grammar are unchanged — mesh export *is* this tessellation. Recognition and
mesh export are complementary halves of one import: the analytic faces are
LEFT for import_step.py, the freeform faces are taken here. Neither machines
raw geometry it could describe as a primitive.

Invocation (argv + files, no stdin protocol, no network — same ground rules
as the other two tools):

  sidecar/.venv/bin/python sidecar/import_step_mesh.py <model.step> -o <outdir>/
      [--solid N] [--up {x+,x-,y+,y-,z+,z-}]

--solid / --up are the same setup declarations as import_step.py: a multi-solid
STEP refuses unless --solid picks the part, and --up names which MODEL axis is
up on the machine (default z+) and is rotated onto +Z BEFORE tessellation, so a
Y-up export's mesh sits the same way its Z-up twin would — the drop-cutter's
"up-facing" reachability is decided in machine orientation.

Writes <outdir>/mesh.stl (the tessellated freeform faces) and
<outdir>/proposed.part (a reviewable Feature::Mesh line pointing at it). A part
with NO freeform face refuses — there is nothing to tessellate, and an empty
STL is never written (use import_step.py for an all-analytic part). On any
error: one-line stderr, exit 1, no partial output files — both outputs are
built in full before either is written.
"""

import argparse
import sys
from pathlib import Path

from OCP.BRep import BRep_Tool
from OCP.BRepMesh import BRepMesh_IncrementalMesh
from OCP.TopAbs import TopAbs_Orientation
from OCP.TopLoc import TopLoc_Location

# Shared with the recognition tool — one number formatter, one set of setup
# declarations, one multi-solid guard. Imported (not duplicated) because both
# scripts live in this directory and importing import_step has no side effects
# until its main() runs under __main__.
from import_step import UP_ROTATIONS, fmt, select_solid

# The five analytic surface types recognition handles as primitives; every
# other face is freeform and taken here. geomType() strings, matching the rest
# of import_step.py.
ANALYTIC = {"PLANE", "CYLINDER", "CONE", "SPHERE", "TORUS"}

# Tessellation chord tolerance (mm): the max deviation of a facet from the true
# surface. A SEPARATE error class from CHORD_TOL (circle flattening) and from
# the mesh finisher's `stepover` sample budget — the mesh's own chord error,
# named as such in DECISIONS.md "Camp-3 slice 1…". Coarse on purpose: this is
# the walking-skeleton export, and a coarse deflection keeps the committed
# golden reviewable. The angular deflection bounds facet turn on curved spans.
MESH_DEFLECTION_MM = 2.5
MESH_ANGULAR_DEFLECTION = 1.0


def facet_normal(a, b, c):
    """Unit normal of triangle (a, b, c) from its winding, or (0, 0, 0) for a
    degenerate face. The Rust reader recomputes normals and ignores this, but a
    real STL carries one, so we emit the true geometric normal."""
    e1 = [b[i] - a[i] for i in range(3)]
    e2 = [c[i] - a[i] for i in range(3)]
    n = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ]
    mag = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]) ** 0.5
    if mag == 0.0:
        return (0.0, 0.0, 0.0)
    return (n[0] / mag, n[1] / mag, n[2] / mag)


def face_triangles(face):
    """Yield each tessellated triangle of a meshed face as three (x, y, z)
    tuples in world coordinates, wound outward (the face's oriented normal).
    Assumes BRepMesh has already run on the owning shape."""
    loc = TopLoc_Location()
    tri = BRep_Tool.Triangulation_s(face.wrapped, loc)
    if tri is None:
        return
    trsf = loc.Transformation()
    reversed_face = face.wrapped.Orientation() == TopAbs_Orientation.TopAbs_REVERSED
    for k in range(1, tri.NbTriangles() + 1):
        i1, i2, i3 = tri.Triangle(k).Get()
        if reversed_face:
            i2, i3 = i3, i2  # keep the winding outward
        verts = []
        for idx in (i1, i2, i3):
            p = tri.Node(idx).Transformed(trsf)
            verts.append((p.X(), p.Y(), p.Z()))
        yield verts


def stl_text(solid_name, faces):
    """One deterministic ASCII STL for the given freeform faces. Facets in
    kernel face-traversal order, triangles in triangulation order; every number
    through fmt() so the file is byte-identical across runs (a committed
    golden)."""
    lines = [f"solid {solid_name}"]
    n_tris = 0
    for face in faces:
        for a, b, c in face_triangles(face):
            nx, ny, nz = facet_normal(a, b, c)
            lines.append(f"  facet normal {fmt(nx)} {fmt(ny)} {fmt(nz)}")
            lines.append("    outer loop")
            for v in (a, b, c):
                lines.append(f"      vertex {fmt(v[0])} {fmt(v[1])} {fmt(v[2])}")
            lines.append("    endloop")
            lines.append("  endfacet")
            n_tris += 1
    lines.append(f"endsolid {solid_name}")
    return "\n".join(lines) + "\n", n_tris


def part_text(source_name, feat_name, n_free, n_analytic, n_tris):
    """The reviewable proposed.part fragment: provenance, the Feature::Mesh
    line, and the suggested consuming run. A fragment, not a runnable file — a
    human accepts it into a real project (the same convention as import_step's
    proposed.part)."""
    return (
        f"# {source_name} -- {n_free} freeform face(s) tessellated to STL by "
        "import_step_mesh.py\n"
        f"# {n_tris} triangles at deflection {fmt(MESH_DEFLECTION_MM)} mm; the "
        f"{n_analytic} analytic face(s) are LEFT for import_step.py recognition.\n"
        "# Camp-3 mesh finishing accepts tessellation + sampling error "
        "(DECISIONS.md \"Camp-3 slice 1…\"); the analytical camps do not.\n"
        "# STL vertices are millimetres by definition; x/y/z place the mesh in "
        "stock coords.\n"
        f'feature {feat_name} Mesh(file="mesh.stl", x=0, y=0, z=0)\n'
        "# Consume with a ball-nose Copy (the only Mesh consumer today):\n"
        f"#   run Copy(tool=T?, surface={feat_name}, stepover=?)\n"
    )


def sanitize(stem):
    """A source stem -> a valid `.part` feature identifier (alnum + '_'),
    suffixed _Mesh. Deterministic, so the proposal name is a golden too."""
    cleaned = "".join(ch if ch.isalnum() or ch == "_" else "_" for ch in stem)
    cleaned = cleaned.strip("_") or "Freeform"
    if cleaned[0].isdigit():
        cleaned = "M_" + cleaned
    return cleaned[0].upper() + cleaned[1:] + "_Mesh"


def export(step_path, solid_sel=None, up="z+"):
    solid, solid_sel, n_solids = select_solid(step_path, solid_sel)
    if UP_ROTATIONS[up] is not None:
        # Rotate the declared up axis onto machine +Z before tessellation, so
        # the STL sits as it will on the machine — the drop-cutter's up-facing
        # reachability is decided in machine orientation, exactly as
        # recognition's checks are (import_step.recognize).
        axis, degrees = UP_ROTATIONS[up]
        solid = solid.rotate((0, 0, 0), axis, degrees)

    faces = solid.Faces()
    freeform = [f for f in faces if f.geomType() not in ANALYTIC]
    if not freeform:
        raise ValueError(
            f"no freeform faces in {step_path.name}: all {len(faces)} face(s) "
            "are analytic (plane/cylinder/cone/sphere/torus) -- use "
            "import_step.py recognition, there is nothing to tessellate"
        )

    # Mesh the whole solid once; face_triangles reads each freeform face's
    # stored triangulation. (Meshing the solid, not each face in isolation,
    # keeps shared-edge vertices consistent.)
    BRepMesh_IncrementalMesh(
        solid.wrapped, MESH_DEFLECTION_MM, False, MESH_ANGULAR_DEFLECTION, True
    )

    stem = step_path.stem
    feat_name = sanitize(stem)
    stl, n_tris = stl_text(stem, freeform)
    if n_tris == 0:
        raise ValueError(
            f"{step_path.name}: {len(freeform)} freeform face(s) tessellated to "
            "zero triangles -- the deflection or geometry is degenerate"
        )
    part = part_text(step_path.name, feat_name, len(freeform),
                     len(faces) - len(freeform), n_tris)
    summary = (f"{len(faces)} faces: {len(freeform)} freeform tessellated "
               f"({n_tris} triangles), {len(faces) - len(freeform)} analytic "
               "skipped for recognition")
    return stl, part, summary


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("step", type=Path, help="input STEP file")
    ap.add_argument("-o", "--outdir", type=Path, required=True,
                    help="output directory (mesh.stl + proposed.part)")
    ap.add_argument("--solid", type=int, default=None, metavar="N",
                    help="which solid is the part (multi-solid STEP)")
    ap.add_argument("--up", choices=sorted(UP_ROTATIONS), default="z+",
                    help="which model axis is up on the machine (default z+)")
    args = ap.parse_args()

    try:
        stl, part, summary = export(args.step, args.solid, args.up)
    except Exception as e:  # noqa: BLE001 — one-line stderr, exit 1, no files
        sys.exit(f"error: {e}")

    # Both outputs are fully built above; write only now, so a failure never
    # leaves a partial STL or a dangling proposal (the "no partial output"
    # convention shared with the Rust CLI and the other sidecar tools).
    args.outdir.mkdir(parents=True, exist_ok=True)
    (args.outdir / "mesh.stl").write_text(stl, encoding="utf-8", newline="\n")
    (args.outdir / "proposed.part").write_text(part, encoding="utf-8", newline="\n")
    print(summary)


if __name__ == "__main__":
    main()
