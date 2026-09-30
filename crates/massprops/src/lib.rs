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

/// Combine per-part mass properties into one rigid-body set.
///
/// Each part's inertia is about **its own** centre of gravity, so combining them is not a
/// sum: the parallel axis theorem has to move each one onto the assembly's centre of
/// gravity first.
///
/// ```text
/// I_total = Σ ( I_i + m_i · (|d_i|²·E − d_i ⊗ d_i) ),  d_i = cg_i − cg_total
/// ```
///
/// Overlapping parts are counted twice. That is a modelling error rather than an
/// arithmetic one, and it is what the interference check exists to report.
pub fn combine(parts: &[MassProperties]) -> MassProperties {
    let total_mass: f64 = parts.iter().map(|p| p.mass).sum();
    if total_mass <= 1e-15 {
        return MassProperties::zero();
    }

    let mut com = [0.0_f64; 3];
    for part in parts {
        for axis in 0..3 {
            com[axis] += part.mass * part.center_of_mass[axis];
        }
    }
    for axis in 0..3 {
        com[axis] /= total_mass;
    }

    let mut inertia = [[0.0_f64; 3]; 3];
    for part in parts {
        let d = [
            part.center_of_mass[0] - com[0],
            part.center_of_mass[1] - com[1],
            part.center_of_mass[2] - com[2],
        ];
        let d_squared = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];

        for row in 0..3 {
            for col in 0..3 {
                let shift = if row == col {
                    part.mass * (d_squared - d[row] * d[col])
                } else {
                    -part.mass * d[row] * d[col]
                };
                inertia[row][col] += part.inertia_tensor[row][col] + shift;
            }
        }
    }

    MassProperties {
        volume: parts.iter().map(|p| p.volume).sum(),
        center_of_mass: com,
        mass: total_mass,
        inertia_tensor: inertia,
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

    /// A unit cube occupying `[x, x+1] x [0, 1] x [0, 1]`.
    fn cube_mesh(x: f32) -> MeshData {
        let positions = vec![
            x, 0.0, 0.0,  x + 1.0, 0.0, 0.0,  x + 1.0, 1.0, 0.0,  x, 1.0, 0.0,
            x, 0.0, 1.0,  x + 1.0, 0.0, 1.0,  x + 1.0, 1.0, 1.0,  x, 1.0, 1.0,
        ];
        let indices = vec![
            0, 2, 1, 0, 3, 2, // z = 0
            4, 5, 6, 4, 6, 7, // z = 1
            0, 5, 4, 0, 1, 5, // y = 0
            3, 6, 2, 3, 7, 6, // y = 1
            0, 7, 3, 0, 4, 7, // x = 0
            1, 6, 5, 1, 2, 6, // x = 1
        ];
        MeshData { positions, normals: vec![0.0; 24], indices }
    }

    /// The same cube at density 1, so mass equals volume.
    fn cube_at(x: f32) -> MassProperties {
        compute_mass_properties(&cube_mesh(x), 1.0)
    }

    #[test]
    fn combine_of_one_part_is_that_part() {
        let part = cube_at(0.0);
        let combined = combine(std::slice::from_ref(&part));
        assert!((combined.mass - part.mass).abs() < 1e-12);
        assert!((combined.center_of_mass[0] - part.center_of_mass[0]).abs() < 1e-12);
        assert!(
            (combined.inertia_tensor[1][1] - part.inertia_tensor[1][1]).abs() < 1e-12
        );
    }

    #[test]
    fn combine_of_nothing_is_zero() {
        let combined = combine(&[]);
        assert_eq!(combined.mass, 0.0);
        assert_eq!(combined.inertia_tensor, [[0.0; 3]; 3]);
    }

    /// Two unit cubes side by side along X, density 1, each of mass 1.
    ///
    /// The combined centre of gravity sits midway between them, and the parallel axis
    /// theorem has a clean answer: the offset is one cube-width from each centre, so
    /// I_yy = I_zz = 2 · (1/6 + 1) = 7/3, while I_xx is unchanged by the shift along X.
    #[test]
    fn combine_applies_the_parallel_axis_theorem() {
        let parts = [cube_at(0.0), cube_at(2.0)];
        let combined = combine(&parts);

        assert!((combined.mass - 2.0).abs() < 0.02, "mass={}", combined.mass);
        assert!((combined.center_of_mass[0] - 1.5).abs() < 0.01);
        assert!((combined.center_of_mass[1] - 0.5).abs() < 0.01);

        // I_xx about the combined cg: the shift is along X, so it contributes nothing.
        let ixx_expected = 2.0 * (1.0 / 6.0);
        assert!(
            (combined.inertia_tensor[0][0] - ixx_expected).abs() < 0.02,
            "Ixx={} expected={}",
            combined.inertia_tensor[0][0],
            ixx_expected
        );

        // I_yy and I_zz pick up m·d² with d = 1.
        let iyy_expected = 2.0 * (1.0 / 6.0 + 1.0);
        assert!(
            (combined.inertia_tensor[1][1] - iyy_expected).abs() < 0.05,
            "Iyy={} expected={}",
            combined.inertia_tensor[1][1],
            iyy_expected
        );
        assert!(
            (combined.inertia_tensor[2][2] - iyy_expected).abs() < 0.05,
            "Izz={} expected={}",
            combined.inertia_tensor[2][2],
            iyy_expected
        );
    }

    /// The bug this replaced: the assembly was measured with the FIRST component's
    /// density applied to every part, so a carbon nose on an aluminium body weighed as
    /// aluminium. Combining per-part results must use each part's own density.
    #[test]
    fn combine_respects_per_part_density() {
        let aluminium = compute_mass_properties(&cube_mesh(0.0), 2700.0 / 1.0e9);
        let carbon = compute_mass_properties(&cube_mesh(2.0), 1600.0 / 1.0e9);
        let combined = combine(&[aluminium, carbon]);

        let expected = (2700.0 + 1600.0) / 1.0e9;
        assert!(
            (combined.mass - expected).abs() < 1e-9,
            "mass={} expected={}",
            combined.mass,
            expected
        );
        // Mass-weighted, so the denser aluminium cube pulls the centre of gravity.
        assert!(
            combined.center_of_mass[0] < 1.5,
            "cg x={} should sit below the midpoint",
            combined.center_of_mass[0]
        );

        // The old single-density path would have weighed the carbon cube as aluminium.
        let naive = combine(&[
            compute_mass_properties(&cube_mesh(0.0), 2700.0 / 1.0e9),
            compute_mass_properties(&cube_mesh(2.0), 2700.0 / 1.0e9),
        ]);
        assert!(naive.mass > combined.mass);
        assert!((naive.center_of_mass[0] - 1.5).abs() < 0.01);
    }
}
