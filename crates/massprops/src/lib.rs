#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MassProperties {
    pub volume: f64,
    pub center_of_mass: [f64; 3],
    pub mass: f64,
    pub inertia_tensor: [[f64; 3]; 3],
}

impl MassProperties {
    pub fn zero() -> Self {
        MassProperties {
            volume: 0.0,
            center_of_mass: [0.0; 3],
            mass: 0.0,
            inertia_tensor: [[0.0; 3]; 3],
        }
    }
}

pub fn compute_mass_properties(mesh: &apro_kernel::MeshData, density: f64) -> MassProperties {
    if mesh.indices.len() < 3 {
        return MassProperties::zero();
    }

    let mut volume = 0.0_f64;
    let mut first_moment = [0.0_f64; 3];
    let mut integral_xx = 0.0_f64;
    let mut integral_yy = 0.0_f64;
    let mut integral_zz = 0.0_f64;
    let mut integral_xy = 0.0_f64;
    let mut integral_xz = 0.0_f64;
    let mut integral_yz = 0.0_f64;

    let pos = &mesh.positions;

    for tri in mesh.indices.chunks(3) {
        if tri.len() < 3 { continue; }
        let i0 = tri[0] as usize * 3;
        let i1 = tri[1] as usize * 3;
        let i2 = tri[2] as usize * 3;

        let v0 = [pos[i0] as f64, pos[i0 + 1] as f64, pos[i0 + 2] as f64];
        let v1 = [pos[i1] as f64, pos[i1 + 1] as f64, pos[i1 + 2] as f64];
        let v2 = [pos[i2] as f64, pos[i2 + 1] as f64, pos[i2 + 2] as f64];

        // Signed volume of tetrahedron (origin, v0, v1, v2)
        let det = v0[0] * (v1[1] * v2[2] - v1[2] * v2[1])
                - v0[1] * (v1[0] * v2[2] - v1[2] * v2[0])
                + v0[2] * (v1[0] * v2[1] - v1[1] * v2[0]);
        let v_tet = det / 6.0;
        volume += v_tet;

        // First moment integrals
        first_moment[0] += v_tet * (v0[0] + v1[0] + v2[0]) / 4.0;
        first_moment[1] += v_tet * (v0[1] + v1[1] + v2[1]) / 4.0;
        first_moment[2] += v_tet * (v0[2] + v1[2] + v2[2]) / 4.0;

        // Second moment integrals (for inertia tensor)
        integral_xx += v_tet / 10.0 * (
            v0[0]*v0[0] + v1[0]*v1[0] + v2[0]*v2[0]
            + v0[0]*v1[0] + v0[0]*v2[0] + v1[0]*v2[0]
        );
        integral_yy += v_tet / 10.0 * (
            v0[1]*v0[1] + v1[1]*v1[1] + v2[1]*v2[1]
            + v0[1]*v1[1] + v0[1]*v2[1] + v1[1]*v2[1]
        );
        integral_zz += v_tet / 10.0 * (
            v0[2]*v0[2] + v1[2]*v1[2] + v2[2]*v2[2]
            + v0[2]*v1[2] + v0[2]*v2[2] + v1[2]*v2[2]
        );
        integral_xy += v_tet / 20.0 * (
            2.0*(v0[0]*v0[1] + v1[0]*v1[1] + v2[0]*v2[1])
            + v0[0]*v1[1] + v1[0]*v0[1]
            + v0[0]*v2[1] + v2[0]*v0[1]
            + v1[0]*v2[1] + v2[0]*v1[1]
        );
        integral_xz += v_tet / 20.0 * (
            2.0*(v0[0]*v0[2] + v1[0]*v1[2] + v2[0]*v2[2])
            + v0[0]*v1[2] + v1[0]*v0[2]
            + v0[0]*v2[2] + v2[0]*v0[2]
            + v1[0]*v2[2] + v2[0]*v1[2]
        );
        integral_yz += v_tet / 20.0 * (
            2.0*(v0[1]*v0[2] + v1[1]*v1[2] + v2[1]*v2[2])
            + v0[1]*v1[2] + v1[1]*v0[2]
            + v0[1]*v2[2] + v2[1]*v0[2]
            + v1[1]*v2[2] + v2[1]*v1[2]
        );
    }

    let volume_abs = volume.abs();
    let mass = density * volume_abs;
    let com = if volume_abs > 1e-15 {
        [
            first_moment[0] / volume,
            first_moment[1] / volume,
            first_moment[2] / volume,
        ]
    } else {
        [0.0; 3]
    };

    // Inertia tensor about the origin, then shifted to COM via parallel axis theorem
    // I_xx = ∫ (y² + z²) dV, I_yy = ∫ (x² + z²) dV, I_zz = ∫ (x² + y²) dV
    // I_xy = -∫ xy dV, I_xz = -∫ xz dV, I_yz = -∫ yz dV
    let i_xx_origin = density * (integral_yy + integral_zz);
    let i_yy_origin = density * (integral_xx + integral_zz);
    let i_zz_origin = density * (integral_xx + integral_yy);
    let i_xy_origin = -density * integral_xy;
    let i_xz_origin = -density * integral_xz;
    let i_yz_origin = -density * integral_yz;

    // Parallel axis theorem: I = I_cm + m * (d²·I - d⊗d)
    let d = com;
    let i_xx = i_xx_origin - mass * (d[1]*d[1] + d[2]*d[2]);
    let i_yy = i_yy_origin - mass * (d[0]*d[0] + d[2]*d[2]);
    let i_zz = i_zz_origin - mass * (d[0]*d[0] + d[1]*d[1]);
    let i_xy = i_xy_origin + mass * d[0] * d[1];
    let i_xz = i_xz_origin + mass * d[0] * d[2];
    let i_yz = i_yz_origin + mass * d[1] * d[2];

    MassProperties {
        volume: volume_abs,
        center_of_mass: com,
        mass,
        inertia_tensor: [
            [i_xx, i_xy, i_xz],
            [i_xy, i_yy, i_yz],
            [i_xz, i_yz, i_zz],
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apro_kernel::MeshData;

    fn unit_cube_mesh() -> MeshData {
        // A unit cube [0,1]^3 as 12 triangles (2 per face)
        let positions = vec![
            0.0, 0.0, 0.0,  1.0, 0.0, 0.0,  1.0, 1.0, 0.0,  0.0, 1.0, 0.0,
            0.0, 0.0, 1.0,  1.0, 0.0, 1.0,  1.0, 1.0, 1.0,  0.0, 1.0, 1.0,
        ];
        let indices = vec![
            0, 1, 2,  0, 2, 3,  // bottom (z=0)
            4, 6, 5,  4, 7, 6,  // top (z=1)
            0, 4, 5,  0, 5, 1,  // front (y=0)
            3, 2, 6,  3, 6, 7,  // back (y=1)
            0, 3, 7,  0, 7, 4,  // left (x=0)
            1, 5, 6,  1, 6, 2,  // right (x=1)
        ];
        let normals = vec![0.0; positions.len()];
        MeshData { positions, normals, indices }
    }

    #[test]
    fn test_unit_cube_volume() {
        let mesh = unit_cube_mesh();
        let mp = compute_mass_properties(&mesh, 1.0);
        assert!((mp.volume - 1.0).abs() < 0.01, "volume={}", mp.volume);
    }

    #[test]
    fn test_unit_cube_com() {
        let mesh = unit_cube_mesh();
        let mp = compute_mass_properties(&mesh, 1.0);
        assert!((mp.center_of_mass[0] - 0.5).abs() < 0.01);
        assert!((mp.center_of_mass[1] - 0.5).abs() < 0.01);
        assert!((mp.center_of_mass[2] - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_unit_cube_mass() {
        let mesh = unit_cube_mesh();
        let mp = compute_mass_properties(&mesh, 2.0);
        assert!((mp.mass - 2.0).abs() < 0.02, "mass={}", mp.mass);
    }

    #[test]
    fn test_zero_mesh() {
        let mesh = MeshData { positions: vec![], normals: vec![], indices: vec![] };
        let mp = compute_mass_properties(&mesh, 1.0);
        assert!((mp.volume).abs() < 1e-10);
        assert!((mp.mass).abs() < 1e-10);
    }
}
