use apro_geometry::transition::Transition;
use apro_document::vehicle::TransitionParams;
use truck_modeling::Solid;
use apro_kernel::{revolve, solid_to_meshdata, MeshData};
use crate::BuildSolid;

pub fn build_transition(params: &TransitionParams) -> Solid {
    let pts = Transition::sample(params.length, params.start_radius, params.end_radius, 16);
    revolve(&pts, 360.0).expect("transition revolve failed")
}

pub fn mesh_transition(params: &TransitionParams) -> MeshData {
    let solid = build_transition(params);
    solid_to_meshdata(&solid, 0.1)
}

impl BuildSolid for TransitionParams {
    fn build(&self) -> Solid {
        build_transition(self)
    }
}

#[cfg(test)]
mod tests {
    use apro_document::vehicle::TransitionParams;
    use super::build_transition;

    #[test]
    fn test_transition_pipeline() {
        let params = TransitionParams { length: 200.0, start_radius: 50.0, end_radius: 70.0, wall: 3.0, material: "Al-6061-T6".into() };
        let solid = build_transition(&params);
        let mesh = apro_kernel::solid_to_meshdata(&solid, 0.1);
        assert!(mesh.positions.len() >= 9);
        assert!(mesh.indices.len() >= 3);
    }
}