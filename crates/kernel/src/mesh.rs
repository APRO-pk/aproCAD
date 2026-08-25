use truck_meshalgo::prelude::*;
use truck_modeling::{Solid, Point3, Vector3};
use truck_polymesh::StandardVertex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshData {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub indices: Vec<u32>,
}

impl Default for MeshData {
    fn default() -> Self {
        MeshData { positions: Vec::new(), normals: Vec::new(), indices: Vec::new() }
    }
}

pub fn solid_to_meshdata(solid: &Solid, tolerance: f64) -> MeshData {
    let tessellated = solid.triangulation(tolerance);
    let polygon: PolygonMesh = tessellated.to_polygon();

    let positions: &Vec<Point3> = polygon.positions();
    let normals: &Vec<Vector3> = polygon.normals();

    let mut out_positions: Vec<f32> = Vec::new();
    let mut out_normals: Vec<f32> = Vec::new();
    let mut out_indices: Vec<u32> = Vec::new();

    for face in polygon.face_iter() {
        let first_idx: u32 = (out_positions.len() / 3) as u32;
        let verts: Vec<&StandardVertex> = face.iter().collect();
        let nv = verts.len() as u32;

        // Collect positions for this face (used for fallback normal computation)
        let mut face_positions: Vec<[f32; 3]> = Vec::new();
        for &sv in &verts {
            let p = &positions[sv.pos];
            face_positions.push([p.x as f32, p.y as f32, p.z as f32]);
        }

        // Compute face normal from first triangle for fallback
        let (fnx, fny, fnz) = if nv >= 3 {
            let ax = face_positions[1][0] - face_positions[0][0];
            let ay = face_positions[1][1] - face_positions[0][1];
            let az = face_positions[1][2] - face_positions[0][2];
            let bx = face_positions[2][0] - face_positions[0][0];
            let by = face_positions[2][1] - face_positions[0][1];
            let bz = face_positions[2][2] - face_positions[0][2];
            let nx = ay * bz - az * by;
            let ny = az * bx - ax * bz;
            let nz = ax * by - ay * bx;
            let len = (nx * nx + ny * ny + nz * nz).sqrt();
            if len > 0.0 { (nx / len, ny / len, nz / len) } else { (0.0, 0.0, 1.0) }
        } else {
            (0.0, 0.0, 1.0)
        };

        let face_nml = [fnx, fny, fnz];
        for &sv in &verts {
            let pos = &positions[sv.pos];
            let n = sv.nor.map(|j| {
                let v = &normals[j];
                [v.x as f32, v.y as f32, v.z as f32]
            }).unwrap_or(face_nml);
            out_positions.push(pos.x as f32);
            out_positions.push(pos.y as f32);
            out_positions.push(pos.z as f32);
            out_normals.extend_from_slice(&n);
        }

        // Fan-triangulate any polygon with 3+ vertices, skipping degenerate
        // (zero-area) triangles — e.g. seam slivers from a 360° revolve.
        if nv >= 3 {
            for j in 1..nv - 1 {
                let p0 = &face_positions[0usize];
                let p1 = &face_positions[j as usize];
                let p2 = &face_positions[j as usize + 1];
                let ux = p1[0] - p0[0]; let uy = p1[1] - p0[1]; let uz = p1[2] - p0[2];
                let vx = p2[0] - p0[0]; let vy = p2[1] - p0[1]; let vz = p2[2] - p0[2];
                let nx = uy * vz - uz * vy;
                let ny = uz * vx - ux * vz;
                let nz = ux * vy - uy * vx;
                if nx * nx + ny * ny + nz * nz == 0.0 { continue; }
                out_indices.push(first_idx);
                out_indices.push(first_idx + j);
                out_indices.push(first_idx + j + 1);
            }
        }
    }

    MeshData {
        positions: out_positions,
        normals: out_normals,
        indices: out_indices,
    }
}

/// Validate that a mesh is a closed, watertight surface:
/// - index ranges are valid, positions/normals lengths match
/// - after welding vertices by rounded position, every edge is shared by
///   exactly two triangles (manifold 2-manifold surface, no holes).
pub fn watertight(mesh: &MeshData) -> Result<(), String> {
    use std::collections::HashMap;

    if mesh.positions.len() % 3 != 0 {
        return Err("positions length is not a multiple of 3".into());
    }
    if mesh.normals.len() != mesh.positions.len() {
        return Err(format!(
            "normal count {} != position count {}",
            mesh.normals.len(),
            mesh.positions.len()
        ));
    }
    let vert_count = mesh.positions.len() / 3;
    if mesh.indices.len() % 3 != 0 {
        return Err("index count is not a multiple of 3".into());
    }

    let mut weld: HashMap<(i64, i64, i64), u32> = HashMap::new();
    let mut canonical: Vec<u32> = Vec::with_capacity(vert_count);
    let scale = 1.0e6_f32;
    let mut next_id = 0u32;
    for i in 0..vert_count {
        let (x, y, z) = (
            mesh.positions[i * 3],
            mesh.positions[i * 3 + 1],
            mesh.positions[i * 3 + 2],
        );
        let key = (
            (x * scale).round() as i64,
            (y * scale).round() as i64,
            (z * scale).round() as i64,
        );
        let id = match weld.get(&key) {
            Some(&existing) => existing,
            None => {
                weld.insert(key, next_id);
                let id = next_id;
                next_id += 1;
                id
            }
        };
        canonical.push(id);
    }

    let mut edge_count: HashMap<(u32, u32), u32> = HashMap::new();
    for t in 0..mesh.indices.len() / 3 {
        let a = canonical[mesh.indices[t * 3] as usize];
        let b = canonical[mesh.indices[t * 3 + 1] as usize];
        let c = canonical[mesh.indices[t * 3 + 2] as usize];
        if a == b || b == c || a == c {
            return Err(format!("degenerate triangle at index {}", t * 3));
        }
        let mut push = |x: u32, y: u32| {
            let key = if x < y { (x, y) } else { (y, x) };
            *edge_count.entry(key).or_insert(0) += 1;
        };
        push(a, b);
        push(b, c);
        push(c, a);
    }

    for (&(a, b), &count) in &edge_count {
        if count != 2 {
            return Err(format!(
                "edge ({a},{b}) is shared by {count} triangles — mesh is not watertight"
            ));
        }
    }
    Ok(())
}
