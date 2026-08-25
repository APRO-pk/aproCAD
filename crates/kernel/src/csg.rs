//! Mesh-level CSG boolean operations (Union / Difference / Intersection).
//!
//! The pipeline is mesh-based (every `SolidOp` tessellates into `MeshData`),
//! so booleans are computed on triangle meshes rather than B-Rep solids.
//! This works for every shape the app can build — including mesh-direct ops
//! like revolve/loft — which a B-Rep boolean could not.
//!
//! Algorithm (classic triangle-split CSG, watertight by construction):
//! 1. Compute the intersection segments of every base/cutter triangle pair.
//!    A segment is clipped to both triangles, so it lies in the plane of
//!    both — the same 3D segment is attached to the base triangle and the
//!    cutter triangle.
//! 2. For each triangle, split it along its segments (plus the boundary
//!    points where *other* segments cross its edges, so neighboring
//!    triangles receive identical vertices — no T-junctions).
//! 3. Classify each resulting piece by point-in-mesh against the other
//!    solid and keep the pieces selected by the operation, reversing
//!    winding for cap surfaces.
//! 4. Weld the output by position — the shared seam vertices are the same
//!    segment endpoints, so the result is a closed, watertight surface.

use crate::MeshData;

type V3 = [f64; 3];
type V2 = [f64; 2];

/// Geometric epsilon for f64 predicates.
const EPS: f64 = 1e-7;
/// Welding tolerance for mesh positions (f32-sourced data).
const WELD_EPS: f64 = 1e-4;
/// Direction set for point-in-mesh ray casting (fixed, non-degenerate).
const RAY_DIRS: [[f64; 3]; 7] = [
    [1.0, 0.372, 0.831],
    [0.217, 1.0, 0.542],
    [0.665, 0.434, 1.0],
    [-1.0, 0.612, 0.317],
    [0.513, -1.0, 0.289],
    [0.317, 0.889, -1.0],
    [-0.831, 0.217, 0.512],
];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn normalize(a: V3) -> V3 {
    let n = norm(a);
    if n < EPS {
        a
    } else {
        [a[0] / n, a[1] / n, a[2] / n]
    }
}
fn dist(a: V3, b: V3) -> f64 {
    norm(sub(a, b))
}

/// Weld mesh positions (tolerance-based) into a compact index space.
/// Returns (positions, triangles) with exact-shared vertices.
fn weld_mesh(mesh: &MeshData) -> (Vec<V3>, Vec<[u32; 3]>) {
    let mut out_pos: Vec<V3> = Vec::new();
    let mut tris: Vec<[u32; 3]> = Vec::new();
    // Spatial grouping: use a coarse grid key to limit pair comparisons.
    let mut buckets: std::collections::HashMap<(i64, i64, i64), Vec<u32>> =
        std::collections::HashMap::new();
    let cell = 16.0 * WELD_EPS;
    let mut index_of: Vec<u32> = Vec::with_capacity(mesh.positions.len() / 3);
    for i in 0..mesh.positions.len() / 3 {
        let p: V3 = [
            mesh.positions[i * 3] as f64,
            mesh.positions[i * 3 + 1] as f64,
            mesh.positions[i * 3 + 2] as f64,
        ];
        let key = (
            (p[0] / cell).floor() as i64,
            (p[1] / cell).floor() as i64,
            (p[2] / cell).floor() as i64,
        );
        let mut found = None;
        if let Some(cand) = buckets.get(&key) {
            for &j in cand {
                if dist(out_pos[j as usize], p) <= WELD_EPS {
                    found = Some(j);
                    break;
                }
            }
        }
        let idx = match found {
            Some(j) => j,
            None => {
                let id = out_pos.len() as u32;
                out_pos.push(p);
                buckets.entry(key).or_default().push(id);
                id
            }
        };
        index_of.push(idx);
    }
    for t in 0..mesh.indices.len() / 3 {
        let a = index_of[mesh.indices[t * 3] as usize];
        let b = index_of[mesh.indices[t * 3 + 1] as usize];
        let c = index_of[mesh.indices[t * 3 + 2] as usize];
        if a == b || b == c || a == c {
            continue;
        }
        tris.push([a, b, c]);
    }
    (out_pos, tris)
}

/// Clip segment pq to a triangle, all in the triangle's plane. Returns the
/// clipped sub-segment, or None if the segment misses the triangle.
fn clip_segment_to_triangle(p: V3, q: V3, tri: &[V3; 3]) -> Option<(V3, V3)> {
    let n = cross(sub(tri[1], tri[0]), sub(tri[2], tri[0]));
    let nn = norm(n);
    if nn < 1e-12 {
        return None;
    }
    let u = normalize(sub(tri[1], tri[0]));
    let nrm = [n[0] / nn, n[1] / nn, n[2] / nn];
    let w = cross(nrm, u);
    let to2 = |pt: V3| [dot(sub(pt, tri[0]), u), dot(sub(pt, tri[0]), w)];
    let p2 = to2(p);
    let q2 = to2(q);
    let mut t0 = 0.0f64;
    let mut t1 = 1.0f64;
    for k in 0..3 {
        let e0 = to2(tri[k]);
        let e1 = to2(tri[(k + 1) % 3]);
        let e = [e1[0] - e0[0], e1[1] - e0[1]];
        let ref_pt = to2(tri[(k + 2) % 3]);
        let s_ref = e[0] * (ref_pt[1] - e0[1]) - e[1] * (ref_pt[0] - e0[0]);
        if s_ref.abs() < 1e-12 {
            continue;
        }
        let s_p = e[0] * (p2[1] - e0[1]) - e[1] * (p2[0] - e0[0]);
        let s_q = e[0] * (q2[1] - e0[1]) - e[1] * (q2[0] - e0[0]);
        if s_p * s_ref < 0.0 && s_q * s_ref < 0.0 {
            return None; // fully outside this half-plane
        }
        if s_p * s_ref < 0.0 {
            let t = s_p / (s_p - s_q);
            t0 = t0.max(t);
        } else if s_q * s_ref < 0.0 {
            let t = s_p / (s_p - s_q);
            t1 = t1.min(t);
        }
    }
    if t1 - t0 < 1e-9 {
        return None;
    }
    let lerp = |t: f64| {
        [
            p[0] + (q[0] - p[0]) * t,
            p[1] + (q[1] - p[1]) * t,
            p[2] + (q[2] - p[2]) * t,
        ]
    };
    Some((lerp(t0), lerp(t1)))
}

/// The portion of triangle A lying in the plane of triangle B, clipped to
/// triangle B. Returns the segment, or None.
fn tri_plane_clip(tri_a: &[V3; 3], tri_b: &[V3; 3], n_b: V3) -> Option<(V3, V3)> {
    let d = |p: V3| dot(sub(p, tri_b[0]), n_b);
    let da = [d(tri_a[0]), d(tri_a[1]), d(tri_a[2])];
    let mut pts: Vec<V3> = Vec::new();
    let mut in_plane_edge: Option<(V3, V3)> = None;
    let in_plane = |v: f64| v.abs() < 1e-6 * norm(n_b);
    for k in 0..3 {
        let p = tri_a[k];
        let q = tri_a[(k + 1) % 3];
        let dp = da[k];
        let dq = da[(k + 1) % 3];
        if in_plane(dp) && in_plane(dq) {
            in_plane_edge = Some((p, q));
        } else if in_plane(dp) {
            if !pts.iter().any(|x| dist(*x, p) < 1e-7) {
                pts.push(p);
            }
        } else if in_plane(dq) {
            if !pts.iter().any(|x| dist(*x, q) < 1e-7) {
                pts.push(q);
            }
        } else if dp * dq < 0.0 {
            let t = dp / (dp - dq);
            let x = add(p, [(q[0] - p[0]) * t, (q[1] - p[1]) * t, (q[2] - p[2]) * t]);
            if !pts.iter().any(|y| dist(*y, x) < 1e-7) {
                pts.push(x);
            }
        }
    }
    // Candidates: the in-plane edge, plus the farthest pair among all points
    // (covers the cross-section case where the triangle pierces the plane).
    let mut candidates: Vec<(V3, V3)> = Vec::new();
    let mut all = pts.clone();
    if let Some((e0, e1)) = in_plane_edge {
        candidates.push((e0, e1));
        all.push(e0);
        all.push(e1);
    }
    if all.len() >= 2 {
        let mut best = (all[0], all[1]);
        let mut bl = dist(all[0], all[1]);
        for i in 0..all.len() {
            for j in i + 1..all.len() {
                let l = dist(all[i], all[j]);
                if l > bl {
                    bl = l;
                    best = (all[i], all[j]);
                }
            }
        }
        candidates.push(best);
    }
    let mut best: Option<(V3, V3)> = None;
    let mut best_len = 0.0f64;
    for (p, q) in candidates {
        if let Some(s) = clip_segment_to_triangle(p, q, tri_b) {
            let l = dist(s.0, s.1);
            if l > best_len {
                best = Some(s);
                best_len = l;
            }
        }
    }
    best
}

/// Compute the segment where two triangles intersect, if any.
/// Returns None when the triangles do not overlap or are coplanar.
fn tri_tri_segment(pa: &[V3; 3], pb: &[V3; 3]) -> Option<(V3, V3)> {
    let n_a = cross(sub(pa[1], pa[0]), sub(pa[2], pa[0]));
    let n_b = cross(sub(pb[1], pb[0]), sub(pb[2], pb[0]));
    let dir = cross(n_a, n_b);
    let mut best: Option<(V3, V3)> = None;
    let mut best_len = 0.0f64;
    let mut consider = |s: (V3, V3)| {
        let l = dist(s.0, s.1);
        if l > best_len + 1e-9 {
            best = Some(s);
            best_len = l;
        }
    };
    if norm(dir) < 1e-9 {
        // Parallel planes. If the planes are distinct, no segment can exist.
        // If they coincide (coplanar), the overlap is a 2D region, not a 1D
        // seam, and emitting the coplanar edges would cut along lines that
        // are not part of the true boundary (e.g. a cap fan's radial edges
        // inside a coplanar face). The true boundary seams for coplanar
        // overlaps are emitted by the non-parallel pairs instead.
        return None;
    } else {
        // General: the intersection lies in the plane of either triangle.
        if let Some(s) = tri_plane_clip(pa, pb, n_b) {
            consider(s);
        }
        if let Some(s) = tri_plane_clip(pb, pa, n_a) {
            consider(s);
        }
    }
    best
}

/// 3D segment-segment intersection point, if the segments cross (or touch).
fn segment_segment_point(s0: (V3, V3), s1: (V3, V3)) -> Option<V3> {
    let d0 = sub(s0.1, s0.0);
    let d1 = sub(s1.1, s1.0);
    let n = cross(d0, d1);
    if norm(n) < 1e-9 {
        return None; // parallel
    }
    let t0 = dot(cross(sub(s1.0, s0.0), d1), n) / dot(n, n);
    let p = add(s0.0, [d0[0] * t0, d0[1] * t0, d0[2] * t0]);
    // Verify p lies on both segments.
    let eps = 1e-3;
    let on_seg = |seg: (V3, V3), q: V3| {
        let d = sub(seg.1, seg.0);
        let l = dot(d, d);
        if l < 1e-12 {
            return false;
        }
        let t = dot(sub(q, seg.0), d) / l;
        t >= -eps && t <= 1.0 + eps
    };
    if on_seg(s0, p) && on_seg(s1, p) {
        Some(p)
    } else {
        None
    }
}

/// Split a convex polygon (2D) by the infinite line through `a`-`b`.
/// Returns (pieces on the "left" of (b-a), pieces on the "right").
/// Points within EPS of the line are included in both sides so the two
/// halves meet exactly along the cut.
fn split_polygon_by_line(poly: &[V2], a: V2, b: V2) -> (Vec<Vec<V2>>, Vec<Vec<V2>>) {
    let d = [b[0] - a[0], b[1] - a[1]];
    let len = norm([d[0], d[1], 0.0]);
    let d = [d[0] / len, d[1] / len];
    let sign = |p: V2| {
        // cross(d, p - a)
        let v = [p[0] - a[0], p[1] - a[1]];
        let c = d[0] * v[1] - d[1] * v[0];
        if c.abs() < 1e-9 {
            0.0
        } else {
            c
        }
    };
    let mut left: Vec<V2> = Vec::new();
    let mut right: Vec<V2> = Vec::new();
    let n = poly.len();
    if n < 3 {
        return (Vec::new(), Vec::new());
    }
    for i in 0..n {
        let p = poly[i];
        let q = poly[(i + 1) % n];
        let sp = sign(p);
        let sq = sign(q);
        if sp >= 0.0 {
            left.push(p);
        }
        if sp <= 0.0 {
            right.push(p);
        }
        if (sp > 0.0 && sq < 0.0) || (sp < 0.0 && sq > 0.0) {
            // Crossing point on the line.
            let w = line_line_intersection(p, q, a, b);
            if let Some(w) = w {
                left.push(w);
                right.push(w);
            }
        }
    }
    let clean = |mut v: Vec<V2>| -> Vec<V2> {
        v.dedup_by(|x, y| (x[0] - y[0]).abs() < 1e-9 && (x[1] - y[1]).abs() < 1e-9);
        if v.len() < 3 {
            return Vec::new();
        }
        v
    };
    (if left.len() >= 3 { vec![clean(left)] } else { Vec::new() },
     if right.len() >= 3 { vec![clean(right)] } else { Vec::new() })
}

fn line_line_intersection(p: V2, q: V2, a: V2, b: V2) -> Option<V2> {
    let d1 = [q[0] - p[0], q[1] - p[1]];
    let d2 = [b[0] - a[0], b[1] - a[1]];
    let denom = d1[0] * d2[1] - d1[1] * d2[0];
    if denom.abs() < 1e-12 {
        return None;
    }
    let t = ((a[0] - p[0]) * d2[1] - (a[1] - p[1]) * d2[0]) / denom;
    Some([p[0] + d1[0] * t, p[1] + d1[1] * t])
}

/// Polygon area (signed, 2D).
fn poly_area(poly: &[V2]) -> f64 {
    let mut s = 0.0;
    for i in 0..poly.len() {
        let p = poly[i];
        let q = poly[(i + 1) % poly.len()];
        s += p[0] * q[1] - p[1] * q[0];
    }
    s / 2.0
}

/// Ray-triangle intersection (Möller–Trumbore). Returns the hit distance.
fn ray_triangle(origin: V3, dir: V3, tri: &[V3; 3]) -> Option<f64> {
    let e1 = sub(tri[1], tri[0]);
    let e2 = sub(tri[2], tri[0]);
    let pvec = cross(dir, e2);
    let det = dot(e1, pvec);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let tvec = sub(origin, tri[0]);
    let u = dot(tvec, pvec) * inv;
    if u < -EPS || u > 1.0 + EPS {
        return None;
    }
    let qvec = cross(tvec, e1);
    let v = dot(dir, qvec) * inv;
    if v < -EPS || u + v > 1.0 + EPS {
        return None;
    }
    let t = dot(e2, qvec) * inv;
    if t <= 1e-6 {
        return None;
    }
    Some(t)
}

/// Classify a point against a closed triangle mesh: true = inside.
fn point_in_mesh(p: V3, tris: &[[u32; 3]], pos: &[V3]) -> Option<bool> {
    let mut votes = 0i32;
    let mut undecided = 0usize;
    for d in RAY_DIRS {
        let dir = normalize(d);
        let mut hits = 0usize;
        let mut t_min = f64::MAX;
        let mut t_max = 0.0f64;
        for t in tris {
            let tri = [pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]];
            if let Some(t) = ray_triangle(p, dir, &tri) {
                hits += 1;
                t_min = t_min.min(t);
                t_max = t_max.max(t);
            }
        }
        // If every hit is (nearly) at the same distance, the point is on the
        // surface — skip this direction.
        if hits > 1 && t_max - t_min < 1e-4 {
            undecided += 1;
            continue;
        }
        votes += if hits % 2 == 1 { 1 } else { -1 };
    }
    if undecided >= 4 {
        return None;
    }
    let v = votes.signum();
    match v {
        1 => Some(true),
        -1 => Some(false),
        _ => None,
    }
}

/// True if p lies on (or very near) the surface of the given mesh: within
/// eps of some triangle's plane and projecting inside that triangle.
/// Used to resolve coplanar-face overlaps in boolean assembly.
fn point_on_mesh_surface(p: V3, tris: &[[u32; 3]], pos: &[V3]) -> bool {
    for t in tris {
        let pa = [pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]];
        let e1 = sub(pa[1], pa[0]);
        let e2 = sub(pa[2], pa[0]);
        let n = cross(e1, e2);
        let nn = norm(n);
        if nn < 1e-12 {
            continue;
        }
        let n = [n[0] / nn, n[1] / nn, n[2] / nn];
        if dot(sub(p, pa[0]), n).abs() > 1e-3 {
            continue;
        }
        let u = normalize(e1);
        let w = cross(n, u);
        let q = [dot(sub(p, pa[0]), u), dot(sub(p, pa[0]), w)];
        // Barycentric: q = s*a + t*b with a = (|e1|, 0), b = (e2·u, e2·w).
        let b2 = [dot(e2, u), dot(e2, w)];
        let det = norm(e1) * b2[1];
        if det.abs() < 1e-9 {
            continue;
        }
        let tb = q[1] / b2[1];
        let s = (q[0] - tb * b2[0]) / norm(e1);
        // Geometric containment: barycentric slack of 0.02 is far too loose for
        // long-thin triangles (a cap fan of radius 10 has 0.2 units of slack).
        // Use exact containment with a float epsilon, then a true in-plane
        // distance test against the triangle edges at the same 1e-3 scale as
        // the plane distance check above.
        let eps = 1e-6;
        let inside = s >= -eps && tb >= -eps && s + tb <= 1.0 + eps;
        let near = if inside {
            true
        } else {
            let a2 = [0.0, 0.0];
            let b2v = [norm(e1), 0.0];
            let c2 = [b2[0], b2[1]];
            let q2 = [q[0], q[1]];
            let d = [
                dist_point_seg2(q2, a2, b2v),
                dist_point_seg2(q2, b2v, c2),
                dist_point_seg2(q2, c2, a2),
            ]
            .iter()
            .cloned()
            .fold(f64::INFINITY, f64::min);
            d < 1e-3
        };
        if near {
            return true;
        }
    }
    false
}

/// Distance from a 2D point to a 2D segment.
fn dist_point_seg2(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let ap = [p[0] - a[0], p[1] - a[1]];
    let l2 = ab[0] * ab[0] + ab[1] * ab[1];
    if l2 < 1e-24 {
        return (ap[0] * ap[0] + ap[1] * ap[1]).sqrt();
    }
    let t = ((ap[0] * ab[0] + ap[1] * ab[1]) / l2).clamp(0.0, 1.0);
    let q = [a[0] + t * ab[0], a[1] + t * ab[1]];
    ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt()
}

/// Split a triangle (given as 3D points) by the segments lying in its plane
/// plus boundary points on its edges. Returns sub-triangles.
fn split_triangle(
    tri: &[V3; 3],
    interior_segs: &[(V3, V3)],
    edge_points: &[Vec<V3>],
) -> Vec<[V3; 3]> {
    // Plane basis for 2D projection.
    let u = normalize(sub(tri[1], tri[0]));
    let n = normalize(cross(sub(tri[1], tri[0]), sub(tri[2], tri[0])));
    if u[0] as f64 != u[0] {
        return vec![*tri];
    }
    let w = cross(n, u);
    let to2 = |p: V3| -> V2 { [dot(sub(p, tri[0]), u), dot(sub(p, tri[0]), w)] };
    let to3 = |p: V2| -> V3 {
        add(tri[0], [
            p[0] * u[0] + p[1] * w[0],
            p[0] * u[1] + p[1] * w[1],
            p[0] * u[2] + p[1] * w[2],
        ])
    };

    // The triangle's polygon, with its boundary split at edge points.
    let mut poly: Vec<V2> = Vec::new();
    for k in 0..3 {
        poly.push(to2(tri[k]));
    }
    // Merge the three boundary segments in order (v0->v1, v1->v2, v2->v0).
    let mut merged: Vec<V2> = Vec::new();
    for k in 0..3 {
        let start = to2(tri[k]);
        let end = to2(tri[(k + 1) % 3]);
        let mut pts: Vec<V2> = edge_points[k].iter().map(|&p| to2(p)).collect();
        pts.retain(|p| {
            let d1 = norm(sub([p[0], p[1], 0.0], [start[0], start[1], 0.0]));
            let d2 = norm(sub([end[0], end[1], 0.0], [p[0], p[1], 0.0]));
            let total = norm(sub([end[0], end[1], 0.0], [start[0], start[1], 0.0]));
            d1 > 1e-9 && d2 > 1e-9 && d1 + d2 <= total + 1e-6
        });
        // Sort along the edge from start to end.
        let dir = [end[0] - start[0], end[1] - start[1]];
        let dl = norm([dir[0], dir[1], 0.0]);
        if dl < 1e-12 {
            continue;
        }
        let dir = [dir[0] / dl, dir[1] / dl];
        pts.sort_by(|a, b| {
            let ta = (a[0] - start[0]) * dir[0] + (a[1] - start[1]) * dir[1];
            let tb = (b[0] - start[0]) * dir[0] + (b[1] - start[1]) * dir[1];
            ta.partial_cmp(&tb).unwrap()
        });
        pts.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9);
        merged.push(start);
        merged.extend(pts);
        if k == 2 {
            // close the loop back to v0
        }
    }
    let poly: Vec<V2> = merged;

    // Subdivide interior segments at their mutual crossings (in 2D).
    let segs2: Vec<(V2, V2)> = interior_segs.iter().map(|(a, b)| (to2(*a), to2(*b))).collect();
    let mut sub_segs: Vec<(V2, V2)> = Vec::new();
    for s in &segs2 {
        sub_segs.push(*s);
    }
    let mut extra: Vec<(V2, u32, u32)> = Vec::new(); // (point, seg idx a, seg idx b)
    for i in 0..sub_segs.len() {
        for j in i + 1..sub_segs.len() {
            let s1 = sub_segs[i];
            let s2 = sub_segs[j];
            if let Some(cross) = line_line_intersection(s1.0, s1.1, s2.0, s2.1) {
                let on1 = |p: V2, s: (V2, V2)| {
                    let d = norm([s.1[0] - s.0[0], s.1[1] - s.0[1], 0.0]);
                    let t1 = norm([p[0] - s.0[0], p[1] - s.0[1], 0.0]);
                    let t2 = norm([p[0] - s.1[0], p[1] - s.1[1], 0.0]);
                    d > 1e-9 && t1 + t2 <= d + 1e-6
                };
                if on1(cross, s1) && on1(cross, s2) {
                    extra.push((cross, i as u32, j as u32));
                }
            }
        }
    }
    // Insert extra vertices into segments.
    for (p, i, _) in &extra {
        let i = *i as usize;
        let s = sub_segs[i];
        let t = |q: V2| {
            let d = norm([s.1[0] - s.0[0], s.1[1] - s.0[1], 0.0]);
            if d < 1e-12 {
                return 0.0;
            }
            norm([q[0] - s.0[0], q[1] - s.0[1], 0.0]) / d
        };
        let tp = t(*p);
        if tp > 1e-6 && tp < 1.0 - 1e-6 {
            sub_segs.push((s.0, *p));
            sub_segs.push((*p, s.1));
            sub_segs[i] = (s.0, s.1); // keep original (harmless duplicates)
        }
    }
    // Also split segments at points where another segment's endpoint lies on them.
    for i in 0..sub_segs.len() {
        for j in 0..sub_segs.len() {
            if i == j {
                continue;
            }
            let s1 = sub_segs[i];
            let s2 = sub_segs[j];
            for e in [s2.0, s2.1] {
                let d = norm([s1.1[0] - s1.0[0], s1.1[1] - s1.0[1], 0.0]);
                if d < 1e-12 {
                    continue;
                }
                let ta = norm([e[0] - s1.0[0], e[1] - s1.0[1], 0.0]);
                let tb = norm([e[0] - s1.1[0], e[1] - s1.1[1], 0.0]);
                if ta > 1e-6 && tb > 1e-6 && ta + tb <= d + 1e-6 {
                    // e lies on s1's line?
                    let d1 = [s1.1[0] - s1.0[0], s1.1[1] - s1.0[1]];
                    let d2 = [e[0] - s1.0[0], e[1] - s1.0[1]];
                    if (d1[0] * d2[1] - d1[1] * d2[0]).abs() < 1e-6 {
                        sub_segs.push((s1.0, e));
                        sub_segs.push((e, s1.1));
                    }
                }
            }
        }
    }

    // Cut the polygon by each segment's line.
    let mut pieces: Vec<Vec<V2>> = vec![poly];
    for s in &sub_segs {
        let mut next: Vec<Vec<V2>> = Vec::new();
        let a = s.0;
        let b = s.1;
        let dl = norm([b[0] - a[0], b[1] - a[1], 0.0]);
        if dl < 1e-12 {
            continue;
        }
        for piece in &pieces {
            let (left, right) = split_polygon_by_line(piece, a, b);
            for side in [left, right] {
                for mut poly in side {
                    // Insert the cut segment's endpoints into the piece
                    // boundary so seam-line subdivision points become real
                    // vertices (a cut line inside the polygon otherwise
                    // drops them).
                    for q in [a, b] {
                        let mut i = 0;
                        while i < poly.len() {
                            let p0 = poly[i];
                            let p1 = poly[(i + 1) % poly.len()];
                            let d = norm([p1[0] - p0[0], p1[1] - p0[1], 0.0]);
                            if d > 1e-9 {
                                let t1 = norm([q[0] - p0[0], q[1] - p0[1], 0.0]);
                                let t2 = norm([q[0] - p1[0], q[1] - p1[1], 0.0]);
                                if t1 > 1e-6 && t2 > 1e-6 && t1 + t2 <= d + 1e-6 {
                                    poly.insert(i + 1, q);
                                    i += 1;
                                }
                            }
                            i += 1;
                        }
                    }
                    if poly_area(&poly).abs() > 1e-10 {
                        next.push(poly);
                    }
                }
            }
        }
        pieces = next;
        if pieces.is_empty() {
            break;
        }
    }
    // Clip every piece to the original triangle (cut lines may extend
    // slightly beyond it — the original polygon IS the triangle).
    // (Pieces are always subsets of the polygon; no further clip needed.)

    // Fan-triangulate convex pieces. Pick a fan apex that produces no
    // degenerate (zero-area) triangles: a collinear boundary chain can make
    // the fan from one apex collapse onto it, silently dropping the chain's
    // subdivision vertices.
    let mut out: Vec<[V3; 3]> = Vec::new();
    for piece in &pieces {
        let n = piece.len();
        if n < 3 {
            continue;
        }
        let mut chosen: Option<Vec<[V3; 3]>> = None;
        for vi in 0..n {
            let v0 = piece[vi];
            let mut tris_out: Vec<[V3; 3]> = Vec::new();
            let mut ok = true;
            for k in 1..n - 1 {
                let a = piece[(vi + k) % n];
                let b = piece[(vi + k + 1) % n];
                let area = ((a[0] - v0[0]) * (b[1] - v0[1]) - (a[1] - v0[1]) * (b[0] - v0[0])).abs();
                if area < 1e-9 {
                    ok = false;
                    break;
                }
                tris_out.push([to3(v0), to3(a), to3(b)]);
            }
            if ok {
                chosen = Some(tris_out);
                break;
            }
        }
        if let Some(tris_out) = chosen {
            out.extend(tris_out);
        }
    }
    out
}

/// Extract edge points where segments cross a triangle's boundary.
/// `segments` are the intersection segments attached to THIS mesh's triangles
/// (each entry is (tri index, segment)). Every triangle is checked against
/// every segment: a segment of one triangle may end on a neighboring
/// triangle's edge (T-junction prevention), so cross-checks are required.
fn collect_edge_points(tris: &[[u32; 3]], pos: &[V3], segments: &[(u32, (V3, V3))]) -> Vec<[Vec<V3>; 3]> {
    let mut out: Vec<[Vec<V3>; 3]> = tris.iter().map(|_| [Vec::new(), Vec::new(), Vec::new()]).collect();
    for (ti, tri_ref) in tris.iter().enumerate() {
        let tri = [
            pos[tri_ref[0] as usize],
            pos[tri_ref[1] as usize],
            pos[tri_ref[2] as usize],
        ];
        for &(_attached, seg) in segments {
            for k in 0..3 {
                let e0 = tri[k];
                let e1 = tri[(k + 1) % 3];
                if let Some(p) = segment_segment_point(seg, (e0, e1)) {
                    // Only keep points strictly on the edge (not vertices —
                    // vertices are already shared).
                    let l = dist(e0, e1);
                    if l < 1e-9 {
                        continue;
                    }
                    let d0 = dist(p, e0);
                    let d1 = dist(p, e1);
                    if d0 > 1e-4 && d1 > 1e-4 && d0 + d1 <= l + 1e-4 {
                        out[ti][k].push(p);
                    }
                }
            }
        }
    }
    out
}

/// Split every segment at the given points that lie strictly inside it.
/// Used to make both meshes subdivide shared seam lines identically.
fn subdivide_segments(segs: &mut [Vec<(V3, V3)>], pts: &[V3]) {
    for list in segs.iter_mut() {
        let mut out: Vec<(V3, V3)> = Vec::new();
        for &(a, b) in list.iter() {
            let l = dist(a, b);
            if l < 1e-9 {
                out.push((a, b));
                continue;
            }
            let mut inside: Vec<V3> = Vec::new();
            for &p in pts {
                let d0 = dist(p, a);
                let d1 = dist(p, b);
                if d0 > 1e-4 && d1 > 1e-4 && d0 + d1 <= l + 1e-4 {
                    if !inside.iter().any(|q| dist(*q, p) < 1e-6) {
                        inside.push(p);
                    }
                }
            }
            if inside.is_empty() {
                out.push((a, b));
            } else {
                inside.sort_by(|p, q| dist(*p, a).partial_cmp(&dist(*q, a)).unwrap());
                let mut cur = a;
                for p in inside {
                    out.push((cur, p));
                    cur = p;
                }
                out.push((cur, b));
            }
        }
        *list = out;
    }
}

/// Split every triangle of a mesh with its attached segments and edge points.
/// Returns (piece triangles, source triangle index).
fn split_mesh(
    tris: &[[u32; 3]],
    pos: &[V3],
    segs: &[Vec<(V3, V3)>],
    edge_pts: &[[Vec<V3>; 3]],
) -> Vec<(Vec<V3>, u32)> {
    let mut pieces: Vec<(Vec<V3>, u32)> = Vec::new();
    for (i, ta) in tris.iter().enumerate() {
        let tri = [
            pos[ta[0] as usize],
            pos[ta[1] as usize],
            pos[ta[2] as usize],
        ];
        let pts = split_triangle(&tri, &segs[i], &edge_pts[i]);
        for p in pts {
            pieces.push((p.to_vec(), i as u32));
        }
    }
    pieces
}

/// T-junction pass: cut lines are infinite, so a split can create vertices
/// strictly inside edges shared with other triangles of the same mesh. Add
/// every piece vertex that lies on a triangle edge to that edge, so the
/// re-split shares those vertices exactly.
fn propagate_edge_points(
    tris: &[[u32; 3]],
    pos: &[V3],
    pieces: &[(Vec<V3>, u32)],
    mut edge_pts: Vec<[Vec<V3>; 3]>,
) -> Vec<[Vec<V3>; 3]> {
    for (piece, _src) in pieces {
        for v in piece {
            for (ti, tri_ref) in tris.iter().enumerate() {
                let tri = [
                    pos[tri_ref[0] as usize],
                    pos[tri_ref[1] as usize],
                    pos[tri_ref[2] as usize],
                ];
                for k in 0..3 {
                    let e0 = tri[k];
                    let e1 = tri[(k + 1) % 3];
                    let l = dist(e0, e1);
                    if l < 1e-9 {
                        continue;
                    }
                    let d0 = dist(*v, e0);
                    let d1 = dist(*v, e1);
                    if d0 > 1e-4 && d1 > 1e-4 && d0 + d1 <= l + 1e-4 {
                        edge_pts[ti][k].push(*v);
                    }
                }
            }
        }
    }
    edge_pts
}

/// Perform a boolean operation between two meshes.
pub fn mesh_boolean(
    base: &MeshData,
    cutter: &MeshData,
    kind: &crate::ops::BooleanKind,
) -> Result<MeshData, crate::ops::KernelError> {
    if base.positions.len() < 9 || cutter.positions.len() < 9 {
        return Err(crate::ops::KernelError("boolean requires non-empty meshes".into()));
    }

    let (bpos, btris) = weld_mesh(base);
    let (cpos, ctris) = weld_mesh(cutter);
    if btris.is_empty() || ctris.is_empty() {
        return Err(crate::ops::KernelError("boolean mesh is degenerate (no triangles)".into()));
    }

    // 1. Intersection segments per pair, attached to both triangles.
    let mut segments: Vec<(u32, u32, (V3, V3))> = Vec::new();
    let mut segs_of_base: Vec<Vec<(V3, V3)>> = vec![Vec::new(); btris.len()];
    let mut segs_of_cutter: Vec<Vec<(V3, V3)>> = vec![Vec::new(); ctris.len()];
    for (i, ta) in btris.iter().enumerate() {
        let pa = [
            bpos[ta[0] as usize],
            bpos[ta[1] as usize],
            bpos[ta[2] as usize],
        ];
        for (j, tb) in ctris.iter().enumerate() {
            let pb = [
                cpos[tb[0] as usize],
                cpos[tb[1] as usize],
                cpos[tb[2] as usize],
            ];
            if let Some(seg) = tri_tri_segment(&pa, &pb) {
                segments.push((i as u32, j as u32, seg));
                segs_of_base[i].push(seg);
                segs_of_cutter[j].push(seg);
            }
        }
    }

    // 2. Edge-crossing points for T-junction-free splits.
    let base_segs: Vec<(u32, (V3, V3))> = segments.iter().map(|&(ta, _, s)| (ta, s)).collect();
    let cutter_segs: Vec<(u32, (V3, V3))> = segments.iter().map(|&(_, tb, s)| (tb, s)).collect();
    let base_edge_pts = collect_edge_points(&btris, &bpos, &base_segs);
    let cutter_edge_pts = collect_edge_points(&ctris, &cpos, &cutter_segs);

    // 2b. Cross-mesh seam uniformity: both meshes must subdivide their shared
    // seam lines identically. Build the union of every segment endpoint and
    // every tri-edge crossing point, then split every segment at any such
    // point that lies strictly inside it.
    let mut global_seam_pts: Vec<V3> = Vec::new();
    for s in segments.iter() {
        global_seam_pts.push(s.2 .0);
        global_seam_pts.push(s.2 .1);
    }
    for ep in base_edge_pts.iter().chain(cutter_edge_pts.iter()) {
        for p in ep.iter() {
            global_seam_pts.extend(p.iter());
        }
    }
    let mut dedup_pts: Vec<V3> = Vec::new();
    for p in global_seam_pts {
        if !dedup_pts.iter().any(|q| dist(*q, p) < 1e-6) {
            dedup_pts.push(p);
        }
    }
    let global_seam_pts = dedup_pts;
    subdivide_segments(&mut segs_of_base, &global_seam_pts);
    subdivide_segments(&mut segs_of_cutter, &global_seam_pts);

    // 3. Split all triangles.
    let base_pieces = split_mesh(&btris, &bpos, &segs_of_base, &base_edge_pts);
    let cutter_pieces = split_mesh(&ctris, &cpos, &segs_of_cutter, &cutter_edge_pts);

    // 3b. Infinite cut lines create vertices on edges shared with other
    // triangles of the same mesh (T-junctions). Propagate those vertices and
    // re-split so shared edges are subdivided identically.
    let base_edge_pts = propagate_edge_points(&btris, &bpos, &base_pieces, base_edge_pts);
    let cutter_edge_pts = propagate_edge_points(&ctris, &cpos, &cutter_pieces, cutter_edge_pts);
    let base_pieces = split_mesh(&btris, &bpos, &segs_of_base, &base_edge_pts);
    let cutter_pieces = split_mesh(&ctris, &cpos, &segs_of_cutter, &cutter_edge_pts);

    // 4. Classify and assemble.
    let mut out_tris: Vec<[u32; 3]> = Vec::new();
    let mut out_pos: Vec<V3> = Vec::new();
    let mut push_tri = |a: V3, b: V3, c: V3, weld: &mut std::collections::HashMap<u32, u32>| {
        // (weld handled at the end via weld_mesh — skip here)
        let base_idx = out_pos.len() as u32;
        out_pos.push(a);
        out_pos.push(b);
        out_pos.push(c);
        out_tris.push([base_idx, base_idx + 1, base_idx + 2]);
        let _ = weld;
    };
    let mut weld_map: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();

    let centroid = |tri: &[V3]| -> V3 {
        [
            (tri[0][0] + tri[1][0] + tri[2][0]) / 3.0,
            (tri[0][1] + tri[1][1] + tri[2][1]) / 3.0,
            (tri[0][2] + tri[1][2] + tri[2][2]) / 3.0,
        ]
    };

    for (tri, _src) in &base_pieces {
        let c = centroid(tri);
        let inside = point_in_mesh(c, &ctris, &cpos).unwrap_or(false);
        let on_cut_surface = point_on_mesh_surface(c, &ctris, &cpos);
        let keep = match kind {
            // On a coplanar overlap the base's surface is the boundary for
            // union/intersection; for difference it is removed (the opening).
            crate::ops::BooleanKind::Union => !inside || on_cut_surface,
            crate::ops::BooleanKind::Difference => !inside && !on_cut_surface,
            crate::ops::BooleanKind::Intersection => inside || on_cut_surface,
        };
        if keep {
            push_tri(tri[0], tri[1], tri[2], &mut weld_map);
        }
    }
    for (tri, _src) in &cutter_pieces {
        let c = centroid(tri);
        let inside = point_in_mesh(c, &btris, &bpos).unwrap_or(false);
        // A cutter surface coplanar with the base's surface is never kept:
        // it is interior to the result (union/intersection) or part of the
        // opening (difference), and would duplicate/overlap the base surface.
        let on_base_surface = point_on_mesh_surface(c, &btris, &bpos);
        let keep = !on_base_surface
            && match kind {
                crate::ops::BooleanKind::Union => !inside,
                crate::ops::BooleanKind::Difference | crate::ops::BooleanKind::Intersection => inside,
            };
        if keep {
            // Cap surfaces face INTO the void: reverse winding.
            if matches!(kind, crate::ops::BooleanKind::Difference) {
                push_tri(tri[0], tri[2], tri[1], &mut weld_map);
            } else {
                push_tri(tri[0], tri[1], tri[2], &mut weld_map);
            }
        }
    }

    // 5. Weld the output and compute flat normals.
    let mut out = MeshData {
        positions: Vec::new(),
        normals: Vec::new(),
        indices: Vec::new(),
    };
    let (wpos, wtris) = weld_mesh(&MeshData {
        positions: out_pos.iter().flat_map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect(),
        normals: vec![],
        indices: out_tris.iter().flat_map(|t| t.iter().copied()).collect(),
    });
    let mut index_map: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    for t in &wtris {
        let tri = [wpos[t[0] as usize], wpos[t[1] as usize], wpos[t[2] as usize]];
        let n = normalize(cross(sub(tri[1], tri[0]), sub(tri[2], tri[0])));
        for v in 0..3 {
            let key = t[v];
            let idx = *index_map.entry(key).or_insert_with(|| {
                let p = wpos[key as usize];
                out.positions.extend_from_slice(&[p[0] as f32, p[1] as f32, p[2] as f32]);
                out.normals.extend_from_slice(&[n[0] as f32, n[1] as f32, n[2] as f32]);
                (out.positions.len() / 3 - 1) as u32
            });
            out.indices.push(idx);
        }
    }

    Ok(out)
}

