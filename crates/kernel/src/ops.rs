use std::f64::consts::TAU;
use crate::types::*;
use crate::MeshData;

#[derive(Debug, Clone, PartialEq)]
pub enum BooleanKind {
    Union,
    Difference,
    Intersection,
}

#[derive(Debug)]
pub struct KernelError(pub String);

impl std::fmt::Display for KernelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for KernelError {}

/// Deduplicate profile points: remove trailing point if it equals the first.
fn dedup_profile(profile: &[[f64; 2]]) -> Vec<[f64; 2]> {
    if profile.len() < 2 { return profile.to_vec(); }
    let last = profile.last().unwrap();
    let first = profile.first().unwrap();
    if last[0] == first[0] && last[1] == first[1] {
        profile[..profile.len() - 1].to_vec()
    } else {
        profile.to_vec()
    }
}

/// Build a closed planar wire from 2D profile points (placed at given Z).
fn profile_to_wire(profile: &[[f64; 2]], z: f64) -> Wire {
    let pts = dedup_profile(profile);
    let verts: Vec<Vertex> = pts.iter()
        .map(|&[x, y]| builder::vertex(Point3::new(x, y, z)))
        .collect();
    let n = verts.len();
    let mut edges = Vec::new();
    for i in 0..n - 1 {
        edges.push(builder::line(&verts[i], &verts[i + 1]));
    }
    edges.push(builder::line(&verts[n - 1], &verts[0]));
    let refs: Vec<&Edge> = edges.iter().collect();
    Wire::from_iter(refs)
}

/// Build a single planar face from a 2D profile at Z=0.
fn profile_to_face(profile: &[[f64; 2]]) -> std::result::Result<Face, KernelError> {
    let wire = profile_to_wire(profile, 0.0);
    builder::try_attach_plane(&[wire])
        .map_err(|_| KernelError("failed to attach plane to profile".into()))
}

// ---------------------------------------------------------------------------
// Revolve — spin a 2D profile around Z axis
// ---------------------------------------------------------------------------
pub fn revolve(profile: &[[f64; 2]], angle_deg: f64) -> std::result::Result<Solid, KernelError> {
    let pts = dedup_profile(profile);
    if pts.len() < 2 {
        return Err(KernelError("profile must have at least 2 points".into()));
    }
    if angle_deg <= 0.0 || angle_deg > 360.0 {
        return Err(KernelError("angle must be in (0, 360]".into()));
    }

    let n = pts.len();
    let vertices: Vec<Vertex> = pts.iter()
        .map(|&[x, r]| builder::vertex(Point3::new(r, 0.0, x)))
        .collect();

    let tip = &vertices[0];
    let base_edge = vertices.last().unwrap();
    let base_axis = builder::vertex(Point3::new(0.0, 0.0, pts[n - 1][0]));

    let mut edges = Vec::new();
    for i in 0..vertices.len() - 1 {
        edges.push(builder::line(&vertices[i], &vertices[i + 1]));
    }
    edges.push(builder::line(base_edge, &base_axis));
    edges.push(builder::line(&base_axis, tip));

    let edge_refs: Vec<&Edge> = edges.iter().collect();
    let wire = Wire::from_iter(edge_refs);
    let face = builder::try_attach_plane(&[wire])
        .map_err(|_| KernelError("failed to attach plane to profile wire".into()))?;

    let angle_rad = Rad(angle_deg * TAU / 360.0);
    Ok(builder::rsweep(&face, Point3::origin(), Vector3::unit_z(), angle_rad))
}

// ---------------------------------------------------------------------------
// Extrude — pull a 2D profile along Z
// ---------------------------------------------------------------------------
pub fn extrude(profile: &[[f64; 2]], height: f64) -> std::result::Result<Solid, KernelError> {
    if dedup_profile(profile).len() < 3 {
        return Err(KernelError("extrude profile must have at least 3 unique points".into()));
    }
    if height == 0.0 {
        return Err(KernelError("extrude height must be non-zero".into()));
    }

    let face = profile_to_face(profile)?;
    let dir = Vector3::new(0.0, 0.0, height);
    Ok(builder::tsweep(&face, dir))
}

// ---------------------------------------------------------------------------
// Loft — ruled surface between profiles using homotopy shells
// ---------------------------------------------------------------------------
pub fn loft(profiles: &[Vec<[f64; 2]>]) -> std::result::Result<Solid, KernelError> {
    if profiles.len() < 2 {
        return Err(KernelError("loft requires at least 2 profiles".into()));
    }
    let n = dedup_profile(&profiles[0]).len();
    for (i, p) in profiles.iter().enumerate() {
        if dedup_profile(p).len() != n {
            return Err(KernelError(format!(
                "profile {} has {} unique points, expected {}", i, dedup_profile(p).len(), n
            )));
        }
    }

    // Build wires at increasing Z offsets
    let wires: Vec<Wire> = profiles.iter().enumerate()
        .map(|(idx, pts)| profile_to_wire(pts, idx as f64 * 1.0))
        .collect();

    // Collect all faces from homotopy shells + cap ends
    let mut all_faces: Vec<Face> = Vec::new();

    for i in 0..profiles.len() - 1 {
        let shell = builder::try_wire_homotopy(&wires[i], &wires[i + 1])
            .map_err(|_| KernelError(format!("homotopy failed between profiles {} and {}", i, i + 1)))?;
        all_faces.extend(shell);
    }

    // Cap ends — note: Solid::try_new may fail if cap face edges don't match
    // homotopy shell boundary edges exactly (topological stitching limitation).
    // This is a known limitation in Truck 0.6.
    let cap_bottom = builder::try_attach_plane(&[wires[0].clone()])
        .map_err(|_| KernelError("failed to cap bottom".into()))?;
    let cap_top = builder::try_attach_plane(&[wires.last().unwrap().clone()])
        .map_err(|_| KernelError("failed to cap top".into()))?;
    all_faces.push(cap_bottom);
    all_faces.push(cap_top);

    let shell: Shell = all_faces.into_iter().collect();
    Solid::try_new(vec![shell])
        .map_err(|_| KernelError("failed to stitch loft into closed solid — cap edges may not match shell boundaries".into()))
}

// ---------------------------------------------------------------------------
// Sweep — tsweep along each path segment (single segment for now)
// ---------------------------------------------------------------------------
pub fn sweep(profile: &[[f64; 2]], path_points: &[[f64; 3]]) -> std::result::Result<Solid, KernelError> {
    if dedup_profile(profile).len() < 3 {
        return Err(KernelError("sweep profile must have at least 3 unique points".into()));
    }
    if path_points.len() < 2 {
        return Err(KernelError("sweep path must have at least 2 points".into()));
    }

    let face = profile_to_face(profile)?;

    // Sweep along first segment only (multi-segment needs bool union)
    let start = Point3::new(path_points[0][0], path_points[0][1], path_points[0][2]);
    let end = Point3::new(path_points[1][0], path_points[1][1], path_points[1][2]);
    let dir = end - start;
    Ok(builder::tsweep(&face, dir))
}

// ---------------------------------------------------------------------------
// Shell — hollow out a solid (not yet supported in Truck 0.6)
// ---------------------------------------------------------------------------
pub fn shell(_solid: &Solid, _thickness: f64) -> std::result::Result<Solid, KernelError> {
    Err(KernelError("shell not yet implemented (Truck 0.6 limitation)".into()))
}

// ---------------------------------------------------------------------------
// Boolean — Union / Difference / Intersection via truck-shapeops
// ---------------------------------------------------------------------------
pub fn boolean(base: &Solid, cutter: &Solid, kind: &BooleanKind) -> std::result::Result<Solid, KernelError> {
    const TOL: f64 = 0.01;

    let solid = match kind {
        BooleanKind::Union => truck_shapeops::or(base, cutter, TOL),
        BooleanKind::Intersection => truck_shapeops::and(base, cutter, TOL),
        BooleanKind::Difference => {
            // difference = base AND (complement of cutter)
            let mut complement = cutter.clone();
            complement.not();
            truck_shapeops::and(base, &complement, TOL)
        }
    }
    .ok_or_else(|| {
        KernelError("boolean operation failed: the solids could not be combined (check that the cutter overlaps the base)".into())
    })?;

    // Convert intersection-curve leaders to B-spline curves so the result
    // tessellates cleanly (mirrors the truck-shapeops punched-cube example).
    for edge in solid.edge_iter() {
        let mut curve = edge.curve();
        if let Curve::IntersectionCurve(_) = &curve {
            curve.to_bspline_leader(0.01, 0.1, 20);
            edge.set_curve(curve);
        }
    }

    Ok(solid)
}

// ---------------------------------------------------------------------------
// Transform solid — apply translation + rotation (same convention as
// transform_mesh: rotation order Z, then Y, then X, then translate)
// ---------------------------------------------------------------------------
pub fn transform_solid(solid: &Solid, translation: &[f64; 3], rotation: &[f64; 3]) -> Solid {
    use truck_base::cgmath64::{Matrix4, Rad};
    let (rx, ry, rz) = (
        Rad(rotation[0].to_radians()),
        Rad(rotation[1].to_radians()),
        Rad(rotation[2].to_radians()),
    );
    let m: Matrix4 = Matrix4::from_translation(Point3::new(translation[0], translation[1], translation[2]).to_vec())
        * Matrix4::from_angle_x(rx)
        * Matrix4::from_angle_y(ry)
        * Matrix4::from_angle_z(rz);
    builder::transformed(solid, m)
}

// ---------------------------------------------------------------------------
// Fillet & Chamfer (not available in Truck 0.6)
// ---------------------------------------------------------------------------
pub fn fillet(_solid: &Solid, _radius: f64) -> std::result::Result<Solid, KernelError> {
    Err(KernelError("fillet not yet implemented (Truck 0.6 limitation)".into()))
}

pub fn chamfer(_solid: &Solid, _distance: f64) -> std::result::Result<Solid, KernelError> {
    Err(KernelError("chamfer not yet implemented (Truck 0.6 limitation)".into()))
}

// ---------------------------------------------------------------------------
// Transform mesh — apply translation + rotation (+ optional per-axis scale,
// applied BEFORE rotation) to the vertex buffer
// ---------------------------------------------------------------------------
pub fn transform_mesh(mesh: &MeshData, translation: &[f64; 3], rotation: &[f64; 3]) -> MeshData {
    transform_mesh_srt(mesh, translation, rotation, &[1.0, 1.0, 1.0])
}

pub fn transform_mesh_srt(
    mesh: &MeshData,
    translation: &[f64; 3],
    rotation: &[f64; 3],
    scale: &[f64; 3],
) -> MeshData {
    let (tx, ty, tz) = (translation[0] as f32, translation[1] as f32, translation[2] as f32);
    let (rx, ry, rz) = (rotation[0].to_radians(), rotation[1].to_radians(), rotation[2].to_radians());

    let (sx, cx) = rx.sin_cos();
    let (sy, cy) = ry.sin_cos();
    let (sz, cz) = rz.sin_cos();

    let mut out = mesh.clone();
    for j in 0..out.positions.len() / 3 {
        let x = out.positions[j * 3] as f64 * scale[0];
        let y = out.positions[j * 3 + 1] as f64 * scale[1];
        let z = out.positions[j * 3 + 2] as f64 * scale[2];

        let x1 = x * cz - y * sz;
        let y1 = x * sz + y * cz;
        let z1 = z;

        let x2 = x1 * cy + z1 * sy;
        let z2 = -x1 * sy + z1 * cy;
        let y2 = y1;

        let y3 = y2 * cx - z2 * sx;
        let z3 = y2 * sx + z2 * cx;
        let x3 = x2;

        out.positions[j * 3] = x3 as f32 + tx;
        out.positions[j * 3 + 1] = y3 as f32 + ty;
        out.positions[j * 3 + 2] = z3 as f32 + tz;
    }

    for j in 0..out.normals.len() / 3 {
        let nx = out.normals[j * 3] as f64;
        let ny = out.normals[j * 3 + 1] as f64;
        let nz = out.normals[j * 3 + 2] as f64;

        let nx1 = nx * cz - ny * sz;
        let ny1 = nx * sz + ny * cz;
        let nz1 = nz;

        let nx2 = nx1 * cy + nz1 * sy;
        let nz2 = -nx1 * sy + nz1 * cy;
        let ny2 = ny1;

        let ny3 = ny2 * cx - nz2 * sx;
        let nz3 = ny2 * sx + nz2 * cx;
        let nx3 = nx2;

        out.normals[j * 3] = nx3 as f32;
        out.normals[j * 3 + 1] = ny3 as f32;
        out.normals[j * 3 + 2] = nz3 as f32;
    }

    out
}

// ---------------------------------------------------------------------------
// Loft — direct mesh generation (bypasses B-Rep stitching limitation)
// ---------------------------------------------------------------------------
pub fn loft_mesh(profiles: &[Vec<[f64; 2]>]) -> std::result::Result<MeshData, KernelError> {
    if profiles.len() < 2 {
        return Err(KernelError("loft requires at least 2 profiles".into()));
    }
    let n = dedup_profile(&profiles[0]).len();
    for (i, p) in profiles.iter().enumerate() {
        if dedup_profile(p).len() != n {
            return Err(KernelError(format!(
                "profile {} has {} unique points, expected {}", i, dedup_profile(p).len(), n
            )));
        }
    }

    let mut positions: Vec<f32> = Vec::new();
    let mut normals: Vec<f32> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    // Build profile point lists at increasing Z offsets
    let profile_pts: Vec<Vec<[f64; 2]>> = profiles.iter()
        .map(|pts| dedup_profile(pts))
        .collect();

    let z_spacing = 1.0; // spacing between profiles in Z

    // Generate side triangles between each pair of adjacent profiles
    for pi in 0..profiles.len() - 1 {
        let z0 = pi as f64 * z_spacing;
        let z1 = (pi + 1) as f64 * z_spacing;
        for j in 0..n {
            let j_next = (j + 1) % n;
            let p0 = profile_pts[pi][j];
            let p1 = profile_pts[pi][j_next];
            let q0 = profile_pts[pi + 1][j];
            let q1 = profile_pts[pi + 1][j_next];

            // Two triangles per quad: (p0, p1, q0) and (p1, q1, q0)
            let base = positions.len() as u32 / 3;
            positions.extend_from_slice(&[p0[0] as f32, p0[1] as f32, z0 as f32]);
            positions.extend_from_slice(&[p1[0] as f32, p1[1] as f32, z0 as f32]);
            positions.extend_from_slice(&[q0[0] as f32, q0[1] as f32, z1 as f32]);
            positions.extend_from_slice(&[p1[0] as f32, p1[1] as f32, z0 as f32]);
            positions.extend_from_slice(&[q1[0] as f32, q1[1] as f32, z1 as f32]);
            positions.extend_from_slice(&[q0[0] as f32, q0[1] as f32, z1 as f32]);

            // Compute face normal from first triangle
            let ax = q0[0] - p0[0]; let ay = q0[1] - p0[1]; let az = z1 - z0;
            let bx = p1[0] - p0[0]; let by = p1[1] - p0[1]; let bz = 0.0;
            let nx = ay * bz - az * by;
            let ny = az * bx - ax * bz;
            let nz = ax * by - ay * bx;
            let len = (nx * nx + ny * ny + nz * nz).sqrt();
            let (nx, ny, nz) = if len > 0.0 {
                (nx / len, ny / len, nz / len)
            } else {
                (0.0, 0.0, 1.0)
            };
            for _ in 0..6 {
                normals.extend_from_slice(&[nx as f32, ny as f32, nz as f32]);
            }

            indices.push(base);
            indices.push(base + 1);
            indices.push(base + 2);
            indices.push(base + 3);
            indices.push(base + 4);
            indices.push(base + 5);
        }
    }

    // Cap first profile (fan triangulation, normal pointing -Z)
    let z0 = 0.0_f64;
    let cap_base = positions.len() as u32 / 3;
    for &pt in &profile_pts[0] {
        positions.extend_from_slice(&[pt[0] as f32, pt[1] as f32, z0 as f32]);
        normals.extend_from_slice(&[0.0, 0.0, -1.0]);
    }
    for j in 1..(n as u32 - 1) {
        indices.push(cap_base);
        indices.push(cap_base + j);
        indices.push(cap_base + j + 1);
    }

    // Cap last profile (fan triangulation, normal pointing +Z)
    let last = profiles.len() - 1;
    let z1 = last as f64 * z_spacing;
    let cap_base = positions.len() as u32 / 3;
    for &pt in &profile_pts[last] {
        positions.extend_from_slice(&[pt[0] as f32, pt[1] as f32, z1 as f32]);
        normals.extend_from_slice(&[0.0, 0.0, 1.0]);
    }
    for j in 1..(n as u32 - 1) {
        indices.push(cap_base);
        indices.push(cap_base + j);
        indices.push(cap_base + j + 1);
    }

    Ok(MeshData { positions, normals, indices })
}

// ---------------------------------------------------------------------------
// Revolve — direct mesh generation (bypasses B-Rep tessellation gaps)
// ---------------------------------------------------------------------------
pub fn revolve_mesh(profile: &[[f64; 2]], angle_deg: f64) -> MeshData {
    let n_profile = profile.len();
    if n_profile < 2 {
        return MeshData { positions: vec![], normals: vec![], indices: vec![] };
    }

    let angle_rad = angle_deg * std::f64::consts::PI / 180.0;
    let is_full = angle_deg >= 359.9;

    // Theta resolution: proportional to circumference for smooth curves
    let max_r = profile.iter().map(|&[_, r]| r.abs()).fold(0.0f64, f64::max);
    let n_theta = if is_full {
        (max_r * 2.0).ceil().clamp(16.0, 128.0) as u32
    } else {
        (max_r * angle_rad / 2.0).ceil().clamp(8.0, 128.0) as u32
    };

    // Profile tangent (dx, dr) at each point for outward normal computation.
    // Profile convention: [axial_position, radius].
    // Output axes: profile axial_position → mesh Z, radius → mesh XY.
    let mut tangents: Vec<[f64; 2]> = Vec::with_capacity(n_profile);
    for j in 0..n_profile {
        let (dx, dr) = if j == 0 {
            (profile[1][0] - profile[0][0], profile[1][1] - profile[0][1])
        } else if j == n_profile - 1 {
            (profile[j][0] - profile[j - 1][0], profile[j][1] - profile[j - 1][1])
        } else {
            (profile[j + 1][0] - profile[j - 1][0], profile[j + 1][1] - profile[j - 1][1])
        };
        tangents.push([dx, dr]);
    }

    let mut positions: Vec<f32> = Vec::new();
    let mut normals: Vec<f32> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    // Generate vertices: (n_theta+1) rings × n_profile points per ring
    // Vertices are SHARED between adjacent triangles — no T-junction gaps.
    for i in 0..=n_theta {
        let theta = angle_rad * i as f64 / n_theta as f64;
        let cos_t = theta.cos();
        let sin_t = theta.sin();

        for j in 0..n_profile {
            let [axial, r] = profile[j];
            // Profile axial → Z, profile radius → XY
            positions.push((r * cos_t) as f32);
            positions.push((r * sin_t) as f32);
            positions.push(axial as f32);

            // Outward normal: (dx·cos, dx·sin, dr) / |tangent|
            let [dx, dr] = tangents[j];
            let len = (dx * dx + dr * dr).sqrt();
            if len > 1e-12 && r.abs() > 1e-12 {
                normals.push((dx * cos_t / len) as f32);
                normals.push((dx * sin_t / len) as f32);
                normals.push((dr / len) as f32);
            } else {
                // Pole or degenerate — use radial direction
                normals.push(cos_t as f32);
                normals.push(sin_t as f32);
                normals.push(0.0);
            }
        }
    }

    // Side triangles — every edge is shared between exactly 2 triangles.
    let np = n_profile as u32;
    for i in 0..n_theta {
        let ring = i * np;
        let next_ring = (i + 1) * np;
        for j in 0..n_profile - 1 {
            let base = ring + j as u32;
            let next = next_ring + j as u32;
            indices.push(base);
            indices.push(next);
            indices.push(base + 1);
            indices.push(base + 1);
            indices.push(next);
            indices.push(next + 1);
        }
    }

    // End caps for partial revolution (< 360°)
    if !is_full {
        // Cap at theta=0 — fan from profile point 0
        for j in 1..n_profile as u32 - 1 {
            indices.push(0);
            indices.push(j);
            indices.push(j + 1);
        }
        // Cap at theta=angle — reversed winding for outward normal
        let ring_start = n_theta as u32 * np;
        for j in 1..n_profile as u32 - 1 {
            indices.push(ring_start);
            indices.push(ring_start + j + 1);
            indices.push(ring_start + j);
        }
    }

    MeshData { positions, normals, indices }
}

// ---------------------------------------------------------------------------
// Tessellate (alias for solid_to_meshdata)
// ---------------------------------------------------------------------------
pub fn tessellate(solid: &Solid, tolerance: f64) -> MeshData {
    crate::solid_to_meshdata(solid, tolerance)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh_boolean;

    #[test]
    fn test_revolve_full() {
        let profile = vec![[0.0, 0.0], [100.0, 50.0], [300.0, 54.0]];
        let solid = revolve(&profile, 360.0).expect("revolve failed");
        let mesh = tessellate(&solid, 1.0);
        assert!(mesh.positions.len() >= 9);
    }

    #[test]
    fn test_revolve_partial_angle() {
        let profile = vec![[0.0, 0.0], [100.0, 50.0]];
        let solid = revolve(&profile, 180.0).expect("revolve failed");
        let mesh = tessellate(&solid, 1.0);
        assert!(mesh.positions.len() >= 9);
    }

    #[test]
    fn test_extrude_triangle() {
        let profile = vec![[0.0, 0.0], [50.0, 0.0], [25.0, 40.0]];
        let solid = extrude(&profile, 100.0).expect("extrude failed");
        let mesh = tessellate(&solid, 1.0);
        assert!(mesh.positions.len() >= 9);
    }

    #[test]
    fn test_extrude_rectangle() {
        let profile = vec![
            [0.0, 0.0], [100.0, 0.0], [100.0, 50.0], [0.0, 50.0], [0.0, 0.0],
        ];
        let solid = extrude(&profile, 50.0).expect("extrude failed");
        let mesh = tessellate(&solid, 1.0);
        assert!(mesh.positions.len() >= 9);
    }

    #[test]
    #[ignore = "Truck 0.6 cannot stitch cap faces to homotopy shells; use loft_mesh() instead"]
    fn test_loft_two_profiles() {
        let p1 = vec![[0.0, 0.0], [50.0, 0.0], [25.0, 40.0]];
        let p2 = vec![[0.0, 0.0], [80.0, 0.0], [40.0, 60.0]];
        let solid = loft(&[p1, p2]).expect("loft failed");
        let mesh = tessellate(&solid, 1.0);
        assert!(mesh.positions.len() >= 9);
    }

    #[test]
    fn test_transform_mesh_translation() {
        let mesh = MeshData {
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
        };
        let out = transform_mesh(&mesh, &[10.0, 20.0, 30.0], &[0.0, 0.0, 0.0]);
        assert!((out.positions[2] - 30.0).abs() < 1e-6);
        assert!((out.positions[5] - 30.0).abs() < 1e-6);
    }

    #[test]
    fn test_transform_mesh_rotation() {
        let mesh = MeshData {
            positions: vec![10.0, 0.0, 0.0],
            normals: vec![1.0, 0.0, 0.0],
            indices: vec![0, 0, 0],
        };
        let out = transform_mesh(&mesh, &[0.0, 0.0, 0.0], &[0.0, 0.0, 90.0]);
        assert!((out.positions[0]).abs() < 1e-6, "x should be 0 after 90deg Z rotation");
        assert!((out.positions[1] - 10.0).abs() < 1e-6, "y should be 10 after 90deg Z rotation");
    }

    #[test]
    fn test_revolve_zero_angle_error() {
        let profile = vec![[0.0, 0.0], [100.0, 50.0]];
        let result = revolve(&profile, 0.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_extrude_zero_height_error() {
        let profile = vec![[0.0, 0.0], [50.0, 0.0], [25.0, 40.0]];
        let result = extrude(&profile, 0.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_shell_not_implemented() {
        let profile = vec![[0.0, 0.0], [100.0, 50.0], [300.0, 54.0]];
        let solid = revolve(&profile, 360.0).unwrap();
        let result = shell(&solid, 2.0);
        assert!(result.is_err());
    }

    #[test]
    fn test_loft_mesh_two_profiles() {
        let p1 = vec![[0.0, 0.0], [50.0, 0.0], [25.0, 40.0]];
        let p2 = vec![[0.0, 0.0], [80.0, 0.0], [40.0, 60.0]];
        let mesh = loft_mesh(&[p1, p2]).expect("loft_mesh failed");
        assert!(mesh.positions.len() >= 9);
        assert!(mesh.indices.len() >= 3);
        // 3 side quads (18 verts) + 2 caps (6 verts) = 24 verts, each 3 floats
        assert_eq!(mesh.positions.len(), 72);
        assert!(mesh.indices.len() > 0);
    }

    #[test]
    fn test_loft_mesh_rectangle_to_circle() {
        let p1 = vec![[-50.0, -50.0], [50.0, -50.0], [50.0, 50.0], [-50.0, 50.0]];
        let p2 = vec![[-30.0, -30.0], [30.0, -30.0], [30.0, 30.0], [-30.0, 30.0]];
        let mesh = loft_mesh(&[p1, p2]).expect("loft_mesh failed");
        assert!(mesh.positions.len() >= 9);
    }

    #[test]
    fn test_loft_mesh_three_profiles() {
        let p1 = vec![[0.0, 0.0], [50.0, 0.0], [25.0, 40.0]];
        let p2 = vec![[0.0, 0.0], [80.0, 0.0], [40.0, 60.0]];
        let p3 = vec![[0.0, 0.0], [60.0, 0.0], [30.0, 50.0]];
        let mesh = loft_mesh(&[p1, p2, p3]).expect("loft_mesh 3 profiles failed");
        assert!(mesh.positions.len() >= 9);
    }

    #[test]
    fn test_fillet_not_implemented() {
        let profile = vec![[0.0, 0.0], [100.0, 50.0], [300.0, 54.0]];
        let solid = revolve(&profile, 360.0).unwrap();
        let result = fillet(&solid, 5.0);
        assert!(result.is_err());
    }

    fn cube(size: f64) -> Solid {
        let face = profile_to_face(&vec![
            [-size / 2.0, -size / 2.0],
            [size / 2.0, -size / 2.0],
            [size / 2.0, size / 2.0],
            [-size / 2.0, size / 2.0],
        ]).unwrap();
        builder::tsweep(&face, Vector3::new(0.0, 0.0, size))
    }

    fn cylinder(radius: f64, height: f64) -> Solid {
        revolve(&vec![[0.0, 0.0], [0.0, radius], [height, radius], [height, 0.0]], 360.0).unwrap()
    }

    fn cube_mesh(size: f64) -> MeshData {
        tessellate(&cube(size), 0.01)
    }

    fn cylinder_mesh(radius: f64, height: f64) -> MeshData {
        tessellate(&cylinder(radius, height), 0.01)
    }

    #[test]
    fn test_boolean_difference_hole() {
        let base = cube_mesh(100.0);
        let cutter = cylinder_mesh(10.0, 200.0);
        let mesh = mesh_boolean(&base, &cutter, &BooleanKind::Difference).expect("difference failed");
        assert!(mesh.positions.len() >= 9);
        crate::watertight(&mesh).expect("difference result must be watertight");
        // The cylinder crosses the cube's top face (z=100): the hole boundary
        // ring must be present there.
        let ring = mesh.positions.chunks(3).any(|p| {
            let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
            (p[2] - 100.0).abs() < 1.0 && (r - 10.0).abs() < 0.5
        });
        assert!(ring, "expected hole-boundary ring (r=10) at the top face z=100");
        // The hole is open at the top face: no material near the axis.
        let center_filled = mesh.positions.chunks(3).any(|p| {
            let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
            (p[2] - 100.0).abs() < 1.0 && r < 9.0
        });
        assert!(!center_filled, "hole must be open at the top face");
        // The tunnel wall is present inside the cube: a band triangle whose
        // vertices sit on the r=10 ring at both the bottom (z=0) and top
        // (z=100) faces of the cube.
        let mut wall = false;
        for i in mesh.indices.chunks(3) {
            let p = [
                &mesh.positions[i[0] as usize * 3..i[0] as usize * 3 + 3],
                &mesh.positions[i[1] as usize * 3..i[1] as usize * 3 + 3],
                &mesh.positions[i[2] as usize * 3..i[2] as usize * 3 + 3],
            ];
            let all_ring = p.iter().all(|v| {
                let r = (v[0] * v[0] + v[1] * v[1]).sqrt();
                r > 9.9 && r < 10.1
            });
            let has_bottom = p.iter().any(|v| (v[2] - 0.0).abs() < 1e-3);
            let has_top = p.iter().any(|v| (v[2] - 100.0).abs() < 1e-3);
            if all_ring && has_bottom && has_top {
                wall = true;
                break;
            }
        }
        assert!(wall, "expected tunnel wall inside the cube");
    }

    #[test]
    fn test_boolean_union_overlapping() {
        let a = cube_mesh(100.0);
        let b = transform_mesh(&cube_mesh(100.0), &[40.0, 25.0, 20.0], &[0.0, 0.0, 0.0]);
        let mesh = mesh_boolean(&a, &b, &BooleanKind::Union).expect("union failed");
        crate::watertight(&mesh).expect("union result must be watertight");
        let xs: Vec<f32> = mesh.positions.chunks(3).map(|p| p[0]).collect();
        let max_x = xs.iter().cloned().fold(f32::MIN, f32::max);
        let min_x = xs.iter().cloned().fold(f32::MAX, f32::min);
        assert!((max_x - 90.0).abs() < 1.0, "union x max should be 90, got {max_x}");
        assert!((min_x + 50.0).abs() < 1.0, "union x min should be -50, got {min_x}");
        // B's far corner (90, 75, 120) is on the merged surface.
        let corner = mesh.positions.chunks(3).any(|p| {
            (p[0] - 90.0).abs() < 0.5 && (p[1] - 75.0).abs() < 0.5 && (p[2] - 120.0).abs() < 0.5
        });
        assert!(corner, "union should include B's far corner (90,75,120)");
    }

    #[test]
    fn test_boolean_intersection_overlap() {
        let a = cube_mesh(100.0);
        let b = transform_mesh(&cube_mesh(100.0), &[40.0, 25.0, 20.0], &[0.0, 0.0, 0.0]);
        let mesh = mesh_boolean(&a, &b, &BooleanKind::Intersection).expect("intersection failed");
        crate::watertight(&mesh).expect("intersection result must be watertight");
        let xs: Vec<f32> = mesh.positions.chunks(3).map(|p| p[0]).collect();
        let max_x = xs.iter().cloned().fold(f32::MIN, f32::max);
        let min_x = xs.iter().cloned().fold(f32::MAX, f32::min);
        assert!((max_x - 50.0).abs() < 1.0, "intersection x max should be 50, got {max_x}");
        assert!((min_x + 10.0).abs() < 1.0, "intersection x min should be -10, got {min_x}");
        // A's far corner (50, 50, 100) is a corner of the overlap box.
        let corner = mesh.positions.chunks(3).any(|p| {
            (p[0] - 50.0).abs() < 0.5 && (p[1] - 50.0).abs() < 0.5 && (p[2] - 100.0).abs() < 0.5
        });
        assert!(corner, "intersection should include corner (50,50,100)");
    }

    #[test]
    fn dbg_open_edges() {
        let a = cube_mesh(100.0);
        let b = transform_mesh(&cube_mesh(100.0), &[40.0, 25.0, 20.0], &[0.0, 0.0, 0.0]);
        let mesh = mesh_boolean(&a, &b, &BooleanKind::Union).expect("union failed");
        let pos: Vec<[f64; 3]> = mesh
            .positions
            .chunks(3)
            .map(|p| [p[0] as f64, p[1] as f64, p[2] as f64])
            .collect();
        let mut edges: std::collections::HashMap<(u32, u32), u32> = std::collections::HashMap::new();
        for t in mesh.indices.chunks(3) {
            for (x, y) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let (x, y) = if x < y { (x, y) } else { (y, x) };
                *edges.entry((x, y)).or_insert(0) += 1;
            }
        }
        let mut n_open = 0;
        for ((x, y), count) in edges {
            if count != 2 {
                n_open += 1;
                if n_open <= 12 {
                    println!("open {n_open}: ({x},{y}) x{count}  p={:?} q={:?}", pos[x as usize], pos[y as usize]);
                }
            }
        }
        println!("open edges total: {n_open}, tris: {}", mesh.indices.len() / 3);
        // Dump triangles containing the vertex (65,50,20) or (50,50,20).
        for (ti, t) in mesh.indices.chunks(3).enumerate() {
            let verts: Vec<[f64; 3]> = t.iter().map(|&i| pos[i as usize]).collect();
            let has65 = verts.iter().any(|p| (p[0] - 65.0).abs() < 0.01 && (p[1] - 50.0).abs() < 0.01 && (p[2] - 20.0).abs() < 0.01);
            let has50 = verts.iter().any(|p| (p[0] - 50.0).abs() < 0.01 && (p[1] - 50.0).abs() < 0.01 && (p[2] - 20.0).abs() < 0.01);
            if has65 || has50 {
                println!("tri {ti}: {verts:?}");
            }
        }
    }

    #[test]
    fn test_transform_solid_matches_mesh_convention() {
        let solid = cube(10.0);
        let moved = transform_solid(&solid, &[10.0, 20.0, 30.0], &[0.0, 0.0, 90.0]);
        let mesh = tessellate(&moved, 0.01);
        // After +90° about Z then translate: a vertex originally at (5,5,z)
        // lands at (-5+10, 5+20, z+30)
        let has_expected = mesh.positions.chunks(3).any(|p| {
            (p[0] - 5.0).abs() < 0.01 && (p[1] - 25.0).abs() < 0.01 && (p[2] - 30.0).abs() < 0.01
        });
        assert!(has_expected, "rotated+translated cube vertex missing");
    }

}
