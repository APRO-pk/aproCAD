use apro_geometry::nozzle::{nozzle_contour, NozzleKind as GeomKind};
use apro_document::vehicle::{NozzleParams, NozzleKind as DocKind};
use truck_modeling::Solid;
use apro_kernel::{revolve, solid_to_meshdata, MeshData};
use crate::BuildSolid;

fn convert_kind(doc: &DocKind) -> GeomKind {
    match doc {
        DocKind::Conical => GeomKind::Conical,
        DocKind::Bell => GeomKind::Bell,
        DocKind::Moc => GeomKind::Moc,
    }
}

pub fn build_nozzle(params: &NozzleParams) -> Solid {
    let kind = convert_kind(&params.kind);
    let pts = nozzle_contour(
        &kind,
        params.throat_radius,
        params.expansion_ratio,
        params.percent_bell,
        params.chamber_radius,
        48,
    );
    revolve(&pts, 360.0).expect("nozzle revolve failed")
}

pub fn mesh_nozzle(params: &NozzleParams) -> MeshData {
    let solid = build_nozzle(params);
    solid_to_meshdata(&solid, 0.1)
}

impl BuildSolid for NozzleParams {
    fn build(&self) -> Solid {
        build_nozzle(self)
    }
}

#[cfg(test)]
mod tests {
    use apro_document::vehicle::{NozzleParams, NozzleKind};
    use super::build_nozzle;

    fn test_nozzle(kind: NozzleKind) {
        let params = NozzleParams {
            kind,
            throat_radius: 25.0,
            expansion_ratio: 9.0,
            percent_bell: 80.0,
            chamber_radius: 75.0,
            wall: 3.0,
            material: "Al-6061-T6".into(),
        };
        let solid = build_nozzle(&params);
        let mesh = apro_kernel::solid_to_meshdata(&solid, 0.1);
        assert!(mesh.positions.len() >= 9);
        assert!(mesh.indices.len() >= 3);
    }

    #[test]
    fn test_conical_pipeline() { test_nozzle(NozzleKind::Conical); }
    #[test]
    fn test_bell_pipeline() { test_nozzle(NozzleKind::Bell); }
    #[test]
    fn test_moc_pipeline() { test_nozzle(NozzleKind::Moc); }
}
