use apro_geometry::tank::{Tank, DomeKind as GeomDome};
use apro_document::vehicle::{TankParams, DomeKind as DocDome};
use truck_modeling::Solid;
use apro_kernel::revolve;
use crate::BuildSolid;

pub fn build_tank(params: &TankParams) -> Solid {
    let dome = convert_dome(&params.dome);
    let pts = Tank::sample(params.radius, params.cylindrical_length, &dome, 32);
    revolve(&pts, 360.0).expect("tank revolve failed")
}

fn convert_dome(doc: &DocDome) -> GeomDome {
    match doc {
        DocDome::Hemispherical => GeomDome::Hemispherical,
        DocDome::Ellipsoidal { ratio } => GeomDome::Ellipsoidal { ratio: *ratio },
    }
}

impl BuildSolid for TankParams {
    fn build(&self) -> Solid {
        build_tank(self)
    }
}

#[cfg(test)]
mod tests {
    use apro_document::vehicle::{TankParams, DomeKind};
    use super::build_tank;

    #[test]
    fn test_tank_hemispherical_pipeline() {
        let params = TankParams { radius: 50.0, cylindrical_length: 300.0, dome: DomeKind::Hemispherical, wall: 3.0, material: "Al-6061-T6".into() };
        let solid = build_tank(&params);
        let mesh = apro_kernel::solid_to_meshdata(&solid, 0.1);
        assert!(mesh.positions.len() >= 9);
        assert!(mesh.indices.len() >= 3);
    }

    #[test]
    fn test_tank_ellipsoidal_pipeline() {
        let params = TankParams { radius: 50.0, cylindrical_length: 300.0, dome: DomeKind::Ellipsoidal { ratio: 2.0 }, wall: 3.0, material: "Al-6061-T6".into() };
        let solid = build_tank(&params);
        let mesh = apro_kernel::solid_to_meshdata(&solid, 0.1);
        assert!(mesh.positions.len() >= 9);
        assert!(mesh.indices.len() >= 3);
    }
}