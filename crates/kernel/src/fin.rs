fn f32x3(a: f64, b: f64, c: f64) -> [f32; 3] { [a as f32, b as f32, c as f32] }

use crate::MeshData;

pub fn fin_mesh(root_profile: &[[f64; 2]], tip_profile: &[[f64; 2]], span: f64, sweep: f64) -> MeshData {
    let n = root_profile.len();
    assert_eq!(n, tip_profile.len(), "root and tip profiles must have same number of points");
    assert!(n >= 3, "profile must have at least 3 points");

    let n_side_quads = n - 1;
    let n_side_tris = n_side_quads * 2;
    let n_cap_tris = n - 2;
    let n_total_tris = n_side_tris + n_cap_tris * 2;

    let mut positions = Vec::with_capacity(n * 6);
    let mut normals = Vec::with_capacity(n * 6);
    let mut indices = Vec::with_capacity(n_total_tris * 3);

    let _span_f = span as f32;
    let _sweep_f = sweep as f32;

    for i in 0..n_side_quads {
        let i_next = (i + 1) % n;

        let r0 = root_profile[i];
        let r1 = root_profile[i_next];
        let t0x = tip_profile[i][0] + sweep;
        let t0y = tip_profile[i][1];
        let t1x = tip_profile[i_next][0] + sweep;
        let t1y = tip_profile[i_next][1];

        // True outward normal of the side quad: (profile edge) x (span edge).
        let ux = r1[0] - r0[0]; let uy = r1[1] - r0[1]; let uz = 0.0;
        let vx = t0x - r0[0]; let vy = t0y - r0[1]; let vz = span;
        let mut nx = uy * vz - uz * vy;
        let mut ny = uz * vx - ux * vz;
        let mut nz = ux * vy - uy * vx;
        let len = (nx * nx + ny * ny + nz * nz).sqrt();
        if len > 0.0 { nx /= len; ny /= len; nz /= len; }
        let _n = f32x3(nx, ny, nz);

        let base = positions.len() as u32 / 3;
        positions.extend_from_slice(&f32x3(r0[0], r0[1], 0.0));
        positions.extend_from_slice(&f32x3(r1[0], r1[1], 0.0));
        positions.extend_from_slice(&f32x3(t0x, t0y, span));
        positions.extend_from_slice(&f32x3(r1[0], r1[1], 0.0));
        positions.extend_from_slice(&f32x3(t1x, t1y, span));
        positions.extend_from_slice(&f32x3(t0x, t0y, span));

        let n = f32x3(0.0, 0.0, 1.0);
        for _ in 0..6 { normals.extend_from_slice(&n); }
        indices.push(base);
        indices.push(base + 1);
        indices.push(base + 2);
        indices.push(base + 3);
        indices.push(base + 4);
        indices.push(base + 5);
    }

    let root_offset = positions.len() as u32 / 3;
    for &p in root_profile {
        positions.extend_from_slice(&f32x3(p[0], p[1], 0.0));
        normals.extend_from_slice(&f32x3(0.0, 0.0, -1.0));
    }
    for i in 1..n - 1 {
        indices.push(root_offset);
        indices.push(root_offset + i as u32);
        indices.push(root_offset + (i + 1) as u32);
    }

    let tip_offset = positions.len() as u32 / 3;
    for &p in tip_profile {
        positions.extend_from_slice(&f32x3(p[0] + sweep, p[1], span));
        normals.extend_from_slice(&f32x3(0.0, 0.0, 1.0));
    }
    for i in 1..n - 1 {
        indices.push(tip_offset);
        indices.push(tip_offset + i as u32);
        indices.push(tip_offset + (i + 1) as u32);
    }

    MeshData { positions, normals, indices }
}

pub fn pattern_mesh(mesh: &MeshData, count: u32) -> MeshData {
    if count <= 1 { return mesh.clone(); }

    let angle_step = std::f64::consts::TAU / count as f64;
    let mut out = MeshData {
        positions: Vec::with_capacity(mesh.positions.len() * count as usize),
        normals: Vec::with_capacity(mesh.normals.len() * count as usize),
        indices: Vec::with_capacity(mesh.indices.len() * count as usize),
    };

    for i in 0..count {
        let angle = angle_step * i as f64;
        let (sin_f, cos_f) = (angle.sin() as f32, angle.cos() as f32);
        let base = out.positions.len() as u32 / 3;

        for j in 0..mesh.positions.len() / 3 {
            let x = mesh.positions[j * 3];
            let y = mesh.positions[j * 3 + 1];
            let z = mesh.positions[j * 3 + 2];
            out.positions.push(x * cos_f - y * sin_f);
            out.positions.push(x * sin_f + y * cos_f);
            out.positions.push(z);
        }

        for j in 0..mesh.normals.len() / 3 {
            let nx = mesh.normals[j * 3];
            let ny = mesh.normals[j * 3 + 1];
            let nz = mesh.normals[j * 3 + 2];
            out.normals.push(nx * cos_f - ny * sin_f);
            out.normals.push(nx * sin_f + ny * cos_f);
            out.normals.push(nz);
        }

        for &idx in &mesh.indices {
            out.indices.push(base + idx);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fin_mesh_has_vertices() {
        let root: Vec<[f64; 2]> = vec![[0.0, 0.0], [50.0, 10.0], [100.0, 0.0], [50.0, -10.0]];
        let tip: Vec<[f64; 2]> = vec![[0.0, 0.0], [40.0, 8.0], [80.0, 0.0], [40.0, -8.0]];
        let mesh = fin_mesh(&root, &tip, 100.0, 10.0);
        assert!(mesh.positions.len() >= 9);
        assert!(mesh.indices.len() >= 3);
    }

    #[test]
    fn test_pattern_mesh() {
        let root: Vec<[f64; 2]> = vec![[0.0, 0.0], [50.0, 10.0], [100.0, 0.0]];
        let tip: Vec<[f64; 2]> = vec![[0.0, 0.0], [40.0, 8.0], [80.0, 0.0]];
        let single = fin_mesh(&root, &tip, 100.0, 0.0);
        let patterned = pattern_mesh(&single, 4);
        assert_eq!(patterned.positions.len(), single.positions.len() * 4);
        assert_eq!(patterned.indices.len(), single.indices.len() * 4);
    }

    #[test]
    fn test_pattern_1_is_identity() {
        let root: Vec<[f64; 2]> = vec![[0.0, 0.0], [50.0, 10.0], [100.0, 0.0]];
        let tip: Vec<[f64; 2]> = vec![[0.0, 0.0], [40.0, 8.0], [80.0, 0.0]];
        let single = fin_mesh(&root, &tip, 100.0, 0.0);
        let patterned = pattern_mesh(&single, 1);
        assert_eq!(patterned.positions, single.positions);
    }
}
