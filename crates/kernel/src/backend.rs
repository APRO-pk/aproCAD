// ---------------------------------------------------------------------------
// ShapeBackend — swappable geometry engine surface (Kernel v2)
//
// The whole upper stack compiles down to MeshData; this trait is the seam
// where backends plug in. TruckBackend preserves today's exact behavior;
// a future OcctBackend (feature `occ`, see KERNEL_V2.md) implements the same
// surface over OpenCascade and unlocks fillet/chamfer/shell/splines.
//
// NOTE: transform_* stays a free function — it is pure vertex math and
// identical across backends.
// ---------------------------------------------------------------------------

use crate::csg::mesh_boolean;
use crate::mesh::MeshData;
use crate::ops::{extrude, loft_mesh, revolve_mesh, sweep, tessellate, BooleanKind};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EdgeSel {
    /// Every edge of the body.
    All,
    /// Edges whose adjacent faces meet above this dihedral angle (degrees).
    ByAngle(f64),
}

pub trait ShapeBackend {
    fn name(&self) -> &'static str;

    fn revolve_mesh(&self, profile: &[[f64; 2]], angle_deg: f64) -> MeshData;
    fn extrude_mesh(&self, profile: &[[f64; 2]], height: f64) -> Result<MeshData, String>;
    fn loft_mesh(&self, profiles: &[Vec<[f64; 2]>]) -> Result<MeshData, String>;
    fn sweep_mesh(&self, profile: &[[f64; 2]], path_points: &[[f64; 3]]) -> Result<MeshData, String>;
    fn boolean(&self, base: &MeshData, tool: &MeshData, kind: BooleanKind) -> Result<MeshData, String>;

    // --- v2-only operations (require an OCC-class backend) -----------------
    fn fillet_mesh(
        &self,
        _mesh: &MeshData,
        _radius: f64,
        _edges: EdgeSel,
    ) -> Result<MeshData, String> {
        Err(format!(
            "fillet requires the '{}' backend (occt); truck does not support it",
            self.name()
        ))
    }
    fn chamfer_mesh(
        &self,
        _mesh: &MeshData,
        _distance: f64,
        _edges: EdgeSel,
    ) -> Result<MeshData, String> {
        Err(format!(
            "chamfer requires the '{}' backend (occt); truck does not support it",
            self.name()
        ))
    }
    fn shell_mesh(&self, _mesh: &MeshData, _thickness: f64) -> Result<MeshData, String> {
        Err(format!(
            "shell requires the '{}' backend (occt); truck does not support it",
            self.name()
        ))
    }
}

/// Current production behavior — delegates to the existing Truck-based ops so
/// output is bit-for-bit what shipped before Kernel v2.
#[derive(Debug, Clone, Copy, Default)]
pub struct TruckBackend;

impl TruckBackend {
    pub fn new() -> Self {
        TruckBackend
    }
}

impl ShapeBackend for TruckBackend {
    fn name(&self) -> &'static str {
        "truck"
    }

    fn revolve_mesh(&self, profile: &[[f64; 2]], angle_deg: f64) -> MeshData {
        revolve_mesh(profile, angle_deg)
    }

    fn extrude_mesh(&self, profile: &[[f64; 2]], height: f64) -> Result<MeshData, String> {
        let solid = extrude(profile, height).map_err(|e| e.0)?;
        Ok(tessellate(&solid, 0.01))
    }

    fn loft_mesh(&self, profiles: &[Vec<[f64; 2]>]) -> Result<MeshData, String> {
        Ok(loft_mesh(profiles).map_err(|e| e.0)?)
    }

    fn sweep_mesh(&self, profile: &[[f64; 2]], path_points: &[[f64; 3]]) -> Result<MeshData, String> {
        let solid = sweep(profile, path_points).map_err(|e| e.0)?;
        Ok(tessellate(&solid, 0.01))
    }

    fn boolean(&self, base: &MeshData, tool: &MeshData, kind: BooleanKind) -> Result<MeshData, String> {
        Ok(mesh_boolean(base, tool, &kind).map_err(|e| e.0)?)
    }
}
