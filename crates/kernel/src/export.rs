use crate::types::*;
use crate::MeshData;
use std::path::Path;
use std::io::Write;
use truck_stepio::out::*;
use truck_modeling::{Point3, Curve, Surface};

pub fn write_step(solid: &Solid, path: &Path) -> std::result::Result<(), String> {
    let header = StepHeaderDescriptor {
        file_name: path.file_name().and_then(|n| n.to_str()).unwrap_or("output.stp").into(),
        time_stamp: "2026-07-16T00:00:00".into(),
        authors: vec!["APRO CAD".into()],
        organization: vec![],
        organization_system: "APRO CAD".into(),
        authorization: "".into(),
    };
    let compressed: truck_topology::compress::CompressedSolid<Point3, Curve, Surface> = solid.compress();
    let model: StepModel<'_, Point3, Curve, Surface> = StepModel::from(&compressed);
    let display = CompleteStepDisplay::new(model, header);
    let step_str = format!("{}", display);
    std::fs::write(path, &step_str).map_err(|e| format!("STEP write error: {}", e))
}

pub fn write_stl(mesh: &MeshData, path: &Path) -> std::result::Result<(), String> {
    if mesh.positions.len() < 9 || mesh.indices.len() < 3 {
        return Err("Mesh has no triangles".into());
    }
    let mut f = std::fs::File::create(path).map_err(|e| format!("Cannot create file: {}", e))?;

    let pos: Vec<[f32; 3]> = mesh.positions.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
    let tri_count = mesh.indices.len() / 3;

    let mut header = [0u8; 80];
    let name = b"APRO CAD STL Export";
    let n = name.len().min(80);
    header[..n].copy_from_slice(&name[..n]);
    f.write_all(&header).map_err(|e| format!("Write error: {}", e))?;
    f.write_all(&(tri_count as u32).to_le_bytes()).map_err(|e| format!("Write error: {}", e))?;

    for i in 0..tri_count {
        let i0 = mesh.indices[i * 3] as usize;
        let i1 = mesh.indices[i * 3 + 1] as usize;
        let i2 = mesh.indices[i * 3 + 2] as usize;
        let mut tri_data = Vec::with_capacity(50);
        tri_data.extend_from_slice(&[0u8; 12]);
        tri_data.extend_from_slice(&pos[i0][0].to_le_bytes());
        tri_data.extend_from_slice(&pos[i0][1].to_le_bytes());
        tri_data.extend_from_slice(&pos[i0][2].to_le_bytes());
        tri_data.extend_from_slice(&pos[i1][0].to_le_bytes());
        tri_data.extend_from_slice(&pos[i1][1].to_le_bytes());
        tri_data.extend_from_slice(&pos[i1][2].to_le_bytes());
        tri_data.extend_from_slice(&pos[i2][0].to_le_bytes());
        tri_data.extend_from_slice(&pos[i2][1].to_le_bytes());
        tri_data.extend_from_slice(&pos[i2][2].to_le_bytes());
        tri_data.extend_from_slice(&[0u8; 2]);
        f.write_all(&tri_data).map_err(|e| format!("Write error: {}", e))?;
    }

    Ok(())
}
