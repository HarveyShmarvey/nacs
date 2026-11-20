// ---- mesh: camp-3 STL input + ball-drop contact math ------------------------
// The full-3D escape hatch (ROADMAP Phase 4; DECISIONS.md "Camp-3 slice 1:
// raster drop-cutter (z-map) mesh finishing"). A mesh is the one geometry
// source with no equation, so this module owns the two things camp 3 needs:
// reading a triangle soup from an STL file (binary or ASCII, std-only — no
// crate), and dropping a ball-nose tool onto a triangle to find where the
// ball's centre comes to rest.
//
// STL carries no units; vertex data is defined to be millimetres here
// (DECISIONS.md) — the parse layer scales a Mesh feature's *placement* offsets
// with the project units but never the triangles themselves.
//
// The file's stored facet normals are ignored (they are unreliable in the
// wild); face normals are recomputed from vertex winding and flipped so
// n.z >= 0 — a drop from above only ever contacts up-facing geometry, so
// winding order doesn't matter either.

/// One triangle, three vertices, in mm. No stored normal (recomputed on use).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tri {
    pub v: [[f64; 3]; 3],
}

/// Read an STL file, binary or ASCII, sniffed by content.
pub fn read_stl(path: &std::path::Path) -> Result<Vec<Tri>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;

    // Binary sniff first: an exact length match against the declared triangle
    // count is decisive, and binary files often begin with the bytes "solid"
    // in their free-text header — so the ASCII check cannot go first.
    if bytes.len() >= 84 {
        let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        if 84 + 50 * n == bytes.len() {
            return read_stl_binary(&bytes);
        }
    }
    match std::str::from_utf8(&bytes) {
        Ok(text) if text.trim_start().starts_with("solid") => read_stl_ascii(text),
        _ => Err("not a recognizable STL (neither a size-consistent binary nor an ASCII 'solid')".to_string()),
    }
}

// Binary STL: 80-byte header, u32 LE triangle count, then per triangle twelve
// LE f32 (normal — ignored — and three vertices) plus 2 attribute bytes.
fn read_stl_binary(bytes: &[u8]) -> Result<Vec<Tri>, String> {
    let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
    let mut tris = Vec::with_capacity(n);
    for i in 0..n {
        let base = 84 + 50 * i + 12; // skip the stored normal
        let mut v = [[0.0f64; 3]; 3];
        for (vi, vert) in v.iter_mut().enumerate() {
            for (ci, coord) in vert.iter_mut().enumerate() {
                let off = base + 12 * vi + 4 * ci;
                let b = [bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]];
                *coord = f32::from_le_bytes(b) as f64;
            }
        }
        tris.push(Tri { v });
    }
    Ok(tris)
}

// ASCII STL: liberal whitespace-token scan collecting `vertex x y z` triples;
// every three vertices form one triangle. `endsolid` is not required.
fn read_stl_ascii(text: &str) -> Result<Vec<Tri>, String> {
    let mut verts: Vec<[f64; 3]> = Vec::new();
    let mut tokens = text.split_whitespace();
    while let Some(tok) = tokens.next() {
        if tok == "vertex" {
            let mut v = [0.0f64; 3];
            for coord in &mut v {
                let t = tokens
                    .next()
                    .ok_or_else(|| "ASCII STL: 'vertex' with fewer than 3 coordinates".to_string())?;
                *coord = t
                    .parse::<f64>()
                    .map_err(|_| format!("ASCII STL: bad vertex coordinate '{}'", t))?;
            }
            verts.push(v);
        }
    }
    if verts.len() % 3 != 0 {
        return Err(format!(
            "ASCII STL: vertex count {} is not a multiple of 3",
            verts.len()
        ));
    }
    Ok(verts
        .chunks_exact(3)
        .map(|c| Tri { v: [c[0], c[1], c[2]] })
        .collect())
}

// ---- ball-drop contact ------------------------------------------------------
// Drop a ball of radius r down the vertical line at (px, py) onto one
// triangle: the highest Z the ball's *centre* can reach while touching the
// triangle, or None if the ball misses it entirely. Contact is inflate-by-r
// against the closed triangle — three cases, face / edge / vertex — and the
// answer is the max over the accepted cases. The caller takes the max over all
// triangles: the upper envelope, which by construction keeps the centre at
// least r from every sampled triangle (concavity is handled by max(), which is
// why the raster pass needs no gouge guard — see ops::finish).

const EPS_AREA: f64 = 1e-12; // squared-length floor for a degenerate face normal
const EPS_VERT: f64 = 1e-9; // n.z floor below which a face counts as vertical

pub fn drop_ball_z(tri: &Tri, px: f64, py: f64, r: f64) -> Option<f64> {
    let mut best: Option<f64> = None;
    let mut keep = |cand: Option<f64>| {
        if let Some(z) = cand {
            best = Some(best.map_or(z, |b: f64| b.max(z)));
        }
    };

    keep(face_drop(tri, px, py, r));
    keep(edge_drop(tri.v[0], tri.v[1], px, py, r));
    keep(edge_drop(tri.v[1], tri.v[2], px, py, r));
    keep(edge_drop(tri.v[2], tri.v[0], px, py, r));
    keep(vertex_drop(tri.v[0], px, py, r));
    keep(vertex_drop(tri.v[1], px, py, r));
    keep(vertex_drop(tri.v[2], px, py, r));

    best
}

// Face contact: the ball touches the triangle's interior. The contact point is
// the centre pushed r along the (upward) unit normal, so the centre sits at
// q + r·n where q is on the plane; solving for the centre on the vertical
// sample line means the contact point q is at (px - r·n.x, py - r·n.y) in XY.
// Accept iff q projects inside the triangle in XY (valid because n.z > 0 makes
// the projection invertible). Vertical faces (n.z ≈ 0) are skipped — their
// silhouette is owned by the edge case. Degenerate (zero-area) triangles are
// skipped — their edges and vertices still participate below.
fn face_drop(tri: &Tri, px: f64, py: f64, r: f64) -> Option<f64> {
    let [a, b, c] = tri.v;
    let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let raw = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ];
    let len2 = raw[0] * raw[0] + raw[1] * raw[1] + raw[2] * raw[2];
    if len2 < EPS_AREA {
        return None;
    }
    let len = len2.sqrt();
    let mut n = [raw[0] / len, raw[1] / len, raw[2] / len];
    if n[2] < 0.0 {
        n = [-n[0], -n[1], -n[2]];
    }
    if n[2] < EPS_VERT {
        return None;
    }

    let qx = px - r * n[0];
    let qy = py - r * n[1];
    if !point_in_tri_xy(qx, qy, a, b, c) {
        return None;
    }
    // Plane through a with normal n: n·q = n·a  =>  solve for q's z.
    let n_dot_a = n[0] * a[0] + n[1] * a[1] + n[2] * a[2];
    let qz = (n_dot_a - n[0] * qx - n[1] * qy) / n[2];
    Some(qz + r * n[2])
}

// XY point-in-triangle via edge sign tests, orientation-agnostic: inside iff
// all three cross products share a sign (boundary counts as inside — the edge
// case would find the same height there anyway).
fn point_in_tri_xy(px: f64, py: f64, a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> bool {
    let s0 = (b[0] - a[0]) * (py - a[1]) - (b[1] - a[1]) * (px - a[0]);
    let s1 = (c[0] - b[0]) * (py - b[1]) - (c[1] - b[1]) * (px - b[0]);
    let s2 = (a[0] - c[0]) * (py - c[1]) - (a[1] - c[1]) * (px - c[0]);
    let has_neg = s0 < 0.0 || s1 < 0.0 || s2 < 0.0;
    let has_pos = s0 > 0.0 || s1 > 0.0 || s2 > 0.0;
    !(has_neg && has_pos)
}

// Edge contact: the centre rides the vertical line at (px, py) at distance r
// from the segment AB — intersect the line with the radius-r cylinder around
// the edge. With w = (px - A.x, py - A.y, -A.z) and u the unit edge direction,
// the squared distance from the centre (at height z) to the infinite edge line
// is |w + z·ez|² - ((w + z·ez)·u)², giving the quadratic
//   (1 - u.z²)·z² + 2(w.z - (w·u)·u.z)·z + (|w|² - (w·u)² - r²) = 0.
// The larger root is the upper contact. Vertical edges (1 - u.z² ≈ 0) are
// skipped — their endpoints (the vertex case) own them. Accept iff the closest
// point on the edge line falls within the segment (0 ≤ t ≤ 1).
fn edge_drop(a: [f64; 3], b: [f64; 3], px: f64, py: f64, r: f64) -> Option<f64> {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let len2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    if len2 < EPS_AREA {
        return None;
    }
    let len = len2.sqrt();
    let u = [d[0] / len, d[1] / len, d[2] / len];
    let qa = 1.0 - u[2] * u[2];
    if qa < EPS_AREA {
        return None; // vertical edge: the vertex case owns it
    }
    let w = [px - a[0], py - a[1], -a[2]];
    let w_dot_u = w[0] * u[0] + w[1] * u[1] + w[2] * u[2];
    let w2 = w[0] * w[0] + w[1] * w[1] + w[2] * w[2];
    let qb = 2.0 * (w[2] - w_dot_u * u[2]);
    let qc = w2 - w_dot_u * w_dot_u - r * r;
    let disc = qb * qb - 4.0 * qa * qc;
    if disc < 0.0 {
        return None;
    }
    let z = (-qb + disc.sqrt()) / (2.0 * qa);
    // Parameter of the closest point on the edge for the centre (px, py, z).
    let t = (w_dot_u + z * u[2]) / len;
    if !(0.0..=1.0).contains(&t) {
        return None;
    }
    Some(z)
}

// Vertex contact: the ball rests on a single point. Centre at
// v.z + sqrt(r² - d²) where d is the XY distance to the vertex; miss if d > r.
fn vertex_drop(v: [f64; 3], px: f64, py: f64, r: f64) -> Option<f64> {
    let d2 = (px - v[0]) * (px - v[0]) + (py - v[1]) * (py - v[1]);
    if d2 > r * r {
        return None;
    }
    Some(v[2] + (r * r - d2).sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: f64 = 3.0;

    fn tri(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> Tri {
        Tri { v: [a, b, c] }
    }

    #[test]
    fn test_drop_on_horizontal_face() {
        // Right triangle in the z = 5 plane; an interior point rests at z + r.
        let t = tri([0.0, 0.0, 5.0], [10.0, 0.0, 5.0], [0.0, 10.0, 5.0]);
        let z = drop_ball_z(&t, 2.0, 2.0, R).unwrap();
        assert!((z - 8.0).abs() < 1e-12, "z = {}", z);
    }

    #[test]
    fn test_drop_on_45deg_face() {
        // Plane z = x for x in [0, 10]: unit normal (-1/√2, 0, 1/√2) after the
        // upward flip. Sampling over (5, 2): contact q.x = 5 + r/√2, so
        // centre z = q.x (the plane) + r/√2 = 5 + 2·r/√2 = 5 + r·√2.
        let t = tri([0.0, 0.0, 0.0], [10.0, 0.0, 10.0], [0.0, 10.0, 0.0]);
        let t2 = tri([10.0, 0.0, 10.0], [10.0, 10.0, 10.0], [0.0, 10.0, 0.0]);
        let z = drop_ball_z(&t, 5.0, 2.0, R)
            .unwrap()
            .max(drop_ball_z(&t2, 5.0, 2.0, R).unwrap_or(f64::NEG_INFINITY));
        let expect = 5.0 + R * 2.0f64.sqrt();
        assert!((z - expect).abs() < 1e-9, "z = {}, expect {}", z, expect);
    }

    #[test]
    fn test_drop_on_ridge_edge() {
        // Two 45° faces meeting at the horizontal ridge y = 5, z = 5 (a tent).
        // Directly over the ridge the edge case wins: centre = ridge + r.
        // Offset by δ < r in Y the centre rides the ridge cylinder:
        // ridge_z + sqrt(r² - δ²).
        let up = tri([0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 5.0, 5.0]);
        let up2 = tri([0.0, 0.0, 0.0], [10.0, 5.0, 5.0], [0.0, 5.0, 5.0]);
        let down = tri([0.0, 5.0, 5.0], [10.0, 5.0, 5.0], [10.0, 10.0, 0.0]);
        let down2 = tri([0.0, 5.0, 5.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0]);
        let tris = [up, up2, down, down2];

        let drop = |px: f64, py: f64| {
            tris.iter()
                .filter_map(|t| drop_ball_z(t, px, py, R))
                .fold(f64::NEG_INFINITY, f64::max)
        };

        let over = drop(5.0, 5.0);
        assert!((over - (5.0 + R)).abs() < 1e-9, "over ridge: {}", over);

        let delta = 1.5;
        let off = drop(5.0, 5.0 + delta);
        let expect = 5.0 + (R * R - delta * delta).sqrt();
        assert!((off - expect).abs() < 1e-9, "off ridge: {} expect {}", off, expect);
    }

    #[test]
    fn test_drop_on_apex_vertex() {
        // Cone-ish single triangle apex at (5, 5, 8). Over the apex the vertex
        // case gives apex + r; at XY distance d < r it gives apex_z +
        // sqrt(r² - d²) when the faces fall away steeper than the ball.
        let t = tri([0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [5.0, 5.0, 8.0]);
        let over = drop_ball_z(&t, 5.0, 5.0, R).unwrap();
        assert!((over - 11.0).abs() < 1e-9, "over apex: {}", over);

        let d = 2.0;
        let near = drop_ball_z(&t, 5.0 + d, 5.0, R).unwrap();
        let vertex_only = 8.0 + (R * R - d * d).sqrt();
        assert!(near >= vertex_only - 1e-9, "near apex: {} >= {}", near, vertex_only);

        // Beyond r of every part of the triangle: no contact at all.
        assert!(drop_ball_z(&t, 5.0, 20.0, R).is_none());
    }

    #[test]
    fn test_degenerate_triangle_skips_face_but_keeps_edges() {
        // Zero-area triangle (three collinear points) at z = 4: the face case
        // must not fire (or divide by zero), but the collapsed edge still
        // supports the ball: centre = 4 + r over the segment.
        let t = tri([0.0, 0.0, 4.0], [5.0, 0.0, 4.0], [10.0, 0.0, 4.0]);
        let z = drop_ball_z(&t, 5.0, 0.0, R).unwrap();
        assert!((z - 7.0).abs() < 1e-12, "z = {}", z);
    }

    #[test]
    fn test_read_stl_ascii() {
        let src = "solid tent\n\
                   facet normal 0 0 1\n\
                    outer loop\n\
                     vertex 0 0 0\n\
                     vertex 10 0 0\n\
                     vertex 10 5 5\n\
                    endloop\n\
                   endfacet\n\
                   endsolid tent\n";
        let tris = read_stl_ascii(src).unwrap();
        assert_eq!(tris.len(), 1);
        assert_eq!(tris[0].v[2], [10.0, 5.0, 5.0]);
    }

    #[test]
    fn test_read_stl_ascii_bad_vertex_count() {
        let src = "solid bad\nvertex 0 0 0\nvertex 1 0 0\nendsolid bad\n";
        assert!(read_stl_ascii(src).is_err());
    }

    #[test]
    fn test_read_stl_binary() {
        // One triangle, bytes built in memory — no temp file.
        let mut bytes = vec![0u8; 80]; // header
        bytes.extend_from_slice(&1u32.to_le_bytes());
        for _ in 0..3 {
            bytes.extend_from_slice(&0f32.to_le_bytes()); // stored normal, ignored
        }
        let verts: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 5.0, 5.0]];
        for v in verts {
            for c in v {
                bytes.extend_from_slice(&c.to_le_bytes());
            }
        }
        bytes.extend_from_slice(&[0u8; 2]); // attribute byte count
        assert_eq!(bytes.len(), 84 + 50);

        let tris = read_stl_binary(&bytes).unwrap();
        assert_eq!(tris.len(), 1);
        assert_eq!(tris[0].v[1], [10.0, 0.0, 0.0]);
    }
}
