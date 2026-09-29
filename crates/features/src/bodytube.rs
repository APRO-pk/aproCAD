use apro_geometry::bodytube::BodyTube;
use apro_document::vehicle::BodyTubeParams;
use truck_modeling::Solid;
use apro_kernel::revolve;
use crate::BuildSolid;

pub fn build_bodytube(params: &BodyTubeParams) -> Solid {
    let pts = BodyTube::sample(params.length, params.radius, 4);
    revolve(&pts, 360.0).expect("bodytube revolve failed")
}

impl BuildSolid for BodyTubeParams {
    fn build(&self) -> Solid {
        build_bodytube(self)
    }
}

#[cfg(test)]
mod tests {
    use apro_document::vehicle::BodyTubeParams;
    use super::build_bodytube;

    #[test]
    fn test_bodytube_pipeline() {
        let params = BodyTubeParams { length: 500.0, radius: 50.0, wall: 3.0, material: "Al-6061-T6".into() };
        let solid = build_bodytube(&params);
        let mesh = apro_kernel::solid_to_meshdata(&solid, 0.1);
        assert!(mesh.positions.len() >= 9);
        assert!(mesh.indices.len() >= 3);
    }
}