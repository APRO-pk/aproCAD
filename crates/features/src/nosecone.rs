use apro_geometry::NoseProfile;
use apro_document::vehicle::{NoseConeParams, NoseConeProfile as DocProfile};
use truck_modeling::Solid;
use apro_kernel::revolve;
use crate::BuildSolid;

pub fn build_nosecone(params: &NoseConeParams) -> Solid {
    let profile = convert_profile(&params.profile);
    let pts = profile.sample(params.length, params.base_radius, 64);
    revolve(&pts, 360.0).expect("nosecone revolve failed")
}

fn convert_profile(doc: &DocProfile) -> NoseProfile {
    match doc {
        DocProfile::Conical => NoseProfile::Conical,
        DocProfile::Ogive => NoseProfile::Ogive,
        DocProfile::VonKarman => NoseProfile::VonKarman,
        DocProfile::Haack { c } => NoseProfile::Haack { c: *c },
        DocProfile::Power { n } => NoseProfile::Power { n: *n },
        DocProfile::Parabolic { k } => NoseProfile::Parabolic { k: *k },
    }
}

impl BuildSolid for NoseConeParams {
    fn build(&self) -> Solid {
        build_nosecone(self)
    }
}

#[cfg(test)]
mod tests {
    use apro_document::vehicle::{NoseConeParams, NoseConeProfile};
    use apro_kernel::solid_to_meshdata;
    use super::build_nosecone;

    fn test_profile(profile: NoseConeProfile) {
        let params = NoseConeParams {
            profile,
            length: 200.0,
            base_radius: 50.0,
            wall: 3.0,
            material: "Test".into(),
        };
        let solid = build_nosecone(&params);
        let mesh = solid_to_meshdata(&solid, 0.1);
        assert!(mesh.positions.len() >= 9, "expected >= 3 vertices, got {}", mesh.positions.len());
        assert!(mesh.indices.len() >= 3, "expected >= 1 face, got {}", mesh.indices.len());
        assert_eq!(mesh.positions.len() % 3, 0, "positions not multiple of 3");
        assert_eq!(mesh.indices.len() % 3, 0, "indices not multiple of 3");
    }

    #[test]
    fn test_conical() { test_profile(NoseConeProfile::Conical); }
    #[test]
    fn test_ogive() { test_profile(NoseConeProfile::Ogive); }
    #[test]
    fn test_vonkarman() { test_profile(NoseConeProfile::VonKarman); }
    #[test]
    fn test_haack() { test_profile(NoseConeProfile::Haack { c: 0.333 }); }
    #[test]
    fn test_power() { test_profile(NoseConeProfile::Power { n: 0.75 }); }
    #[test]
    fn test_parabolic() { test_profile(NoseConeProfile::Parabolic { k: 0.5 }); }
}
