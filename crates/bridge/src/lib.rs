//! # apro-cad-bridge
//!
//! Publishes aproCAD mass properties to the APRO Works orchestration store.
//!
//! This crate is the producer half of the platform's motivating example: a design drawn
//! in aproCAD becomes an artifact that HexaDOF can pull and import.
//!
//! It lives in the aproCAD repository because it depends on `apro-document` and
//! `apro-massprops`. The platform must never depend on an application, so the dependency
//! points one way: this crate depends on the published SDK, never the other way round.
//!
//! ## Two things this crate refuses to guess
//!
//! **Identity.** A vehicle publishes under its `uid`, not its `name`. `name` is a label
//! the author edits freely; publishing under it would mean renaming a design looks like a
//! brand-new vehicle to every consumer, and silently orphans the revision history. If a
//! document has no `uid` the publish path stops and asks for one
//! ([`ensure_uid`] stamps it in).
//!
//! **The datum.** The payload states where the CAD origin sits in the body datum frame
//! rather than assuming the two coincide. See [`DatumOffset`].
//!
//! ## What this crate does *not* do
//!
//! It does not open a connection of its own, hold global state, or talk to the UI. The
//! caller passes an [`AproStoreClient`], which keeps every one of these functions
//! testable and keeps the Tauri layer free of orchestration logic.

use apro_client::{
    AproStoreClient, AppInterface, ClientError, Encoding, RevisionHandle, TypeId, TypeError,
};
use apro_contracts::{
    DocumentMassProperties, MassPropertiesV1, ReferenceGeometrySi, SourceUnits,
    MASS_PROPERTIES_TYPE,
};
use apro_document::vehicle::{Units, Vehicle};
use apro_massprops::MassProperties;

/// Re-exported so an application embedding this bridge does not have to pin the same SDK
/// version twice — and cannot pin two different ones.
pub use apro_client;
pub use apro_contracts;

/// The slug the hub launches aproCAD under, and the owner segment of every type it
/// publishes. Both come from the hub's product registry; changing one without the other
/// makes writes fail with `write_not_declared`.
pub const APP_SLUG: &str = "apro-cad";

/// Prefix for generated vehicle identities, so a `uid` is recognisable at a glance.
const UID_PREFIX: &str = "veh-";

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum BridgeError {
    /// The document has no `uid`, so there is no stable identity to publish under.
    /// The caller should offer to assign one rather than falling back to `name`.
    MissingUid,
    /// Not launched by APRO Works. Not a failure: degrade to local-only.
    NotConnected,
    Contract(apro_contracts::ContractError),
    Client(ClientError),
    /// A type id that does not parse. A programming error, not a user error: the id is a
    /// constant in this crate.
    TypeId(TypeError),
    Json(serde_json::Error),
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BridgeError::MissingUid => write!(
                f,
                "this design has no uid, so it has no stable identity to publish under; \
                 assign one before publishing"
            ),
            BridgeError::NotConnected => {
                write!(f, "aproCAD was not launched by APRO Works, so there is no store to publish to")
            }
            BridgeError::Contract(err) => write!(f, "{err}"),
            BridgeError::Client(err) => write!(f, "{err}"),
            BridgeError::TypeId(err) => write!(f, "invalid artifact type: {err}"),
            BridgeError::Json(err) => write!(f, "could not serialise the payload: {err}"),
        }
    }
}

impl std::error::Error for BridgeError {}

impl From<ClientError> for BridgeError {
    fn from(err: ClientError) -> Self {
        BridgeError::Client(err)
    }
}

impl From<apro_contracts::ContractError> for BridgeError {
    fn from(err: apro_contracts::ContractError) -> Self {
        BridgeError::Contract(err)
    }
}

impl From<serde_json::Error> for BridgeError {
    fn from(err: serde_json::Error) -> Self {
        BridgeError::Json(err)
    }
}

pub type Result<T> = std::result::Result<T, BridgeError>;

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

/// The instance name a vehicle publishes and is consumed under.
///
/// Deliberately an error rather than a fallback to `name`: a silent fallback produces a
/// *different* artifact every time the design is renamed, which is invisible until a
/// consumer is quietly reading a stale copy.
pub fn instance_name(vehicle: &Vehicle) -> Result<String> {
    match vehicle.uid.as_deref().map(str::trim) {
        Some(uid) if !uid.is_empty() => Ok(uid.to_string()),
        _ => Err(BridgeError::MissingUid),
    }
}

/// Generate a fresh identity and stamp it into the document.
///
/// Returns the new uid. The caller is responsible for persisting the document — this
/// crate does not write files.
pub fn ensure_uid(vehicle: &mut Vehicle) -> String {
    if let Some(existing) = vehicle.uid.as_deref().map(str::trim) {
        if !existing.is_empty() {
            return existing.to_string();
        }
    }
    let uid = format!("{UID_PREFIX}{}", uuid::Uuid::new_v4().simple());
    vehicle.uid = Some(uid.clone());
    uid
}

// ---------------------------------------------------------------------------
// Units and the datum
// ---------------------------------------------------------------------------

/// Map the document's declared units onto the contract's unit system.
///
/// Kept here rather than in `apro-contracts` so the contract crate never has to know
/// about `apro-document`.
pub fn source_units(units: &Units) -> SourceUnits {
    match units {
        Units::Millimeters => SourceUnits::Millimeters,
        Units::Centimeters => SourceUnits::Centimeters,
        Units::Meters => SourceUnits::Meters,
        Units::Inches => SourceUnits::Inches,
        Units::Feet => SourceUnits::Feet,
    }
}

/// Where the CAD document origin sits in the consumer's body datum frame.
///
/// The common conventions are nose tip, base, and centre of gravity, and nothing in a CAD
/// document says which one applies. So this is passed in explicitly rather than inferred.
///
/// [`DatumOffset::coincident`] is the honest default for a design drawn from its own
/// origin: it *states* that the two frames line up instead of leaving it unstated.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DatumOffset {
    pub offset_m: [f64; 3],
}

impl DatumOffset {
    /// The CAD origin *is* the body datum.
    pub fn coincident() -> Self {
        Self {
            offset_m: [0.0; 3],
        }
    }

    pub fn from_metres(offset_m: [f64; 3]) -> Self {
        Self { offset_m }
    }

    /// Build from an offset expressed in the document's own units.
    pub fn from_document_units(offset: [f64; 3], units: &Units) -> Self {
        let scale = source_units(units).metres_per_unit();
        Self {
            offset_m: [offset[0] * scale, offset[1] * scale, offset[2] * scale],
        }
    }
}

impl Default for DatumOffset {
    fn default() -> Self {
        Self::coincident()
    }
}

// ---------------------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------------------

/// Convert the CAD app's native mass properties into the published SI payload.
///
/// The unit maths lives in `apro-contracts`, not here, so the consumer can see and test
/// the exact conversion it is trusting.
pub fn to_payload(
    vehicle: &Vehicle,
    mass: &MassProperties,
    datum: DatumOffset,
    reference: Option<ReferenceGeometrySi>,
) -> MassPropertiesV1 {
    let native = DocumentMassProperties {
        volume: mass.volume,
        mass_kg: mass.mass,
        center_of_mass: mass.center_of_mass,
        inertia: mass.inertia_tensor,
    };

    let mut payload = MassPropertiesV1::from_document(
        native,
        source_units(&vehicle.units),
        datum.offset_m,
    );
    payload.source_note = format!(
        "{} — {} component(s), published from aproCAD in {}; cg and inertia converted \
         to SI at the publish boundary, mass unchanged",
        payload.source_note,
        vehicle.components.len(),
        payload.source_units.as_str()
    );
    if let Some(reference) = reference {
        payload = payload.with_reference_geometry(reference);
    }
    payload
}

/// Derive aerodynamic reference geometry from the assembly's bounding box.
///
/// `extent` is the bounding-box size in **document units**; it is converted to metres
/// here. `longitudinal` is the index of the body's long axis.
///
/// aproCAD models rockets along Z, so callers normally pass `2`. This is a stated
/// convention rather than a law — see [`ReferenceGeometrySi::from_bounding_box`] — and
/// the convention string travels with the numbers so a consumer can disagree with it.
pub fn reference_geometry_from_extent(
    extent: [f64; 3],
    longitudinal: usize,
    units: &Units,
) -> ReferenceGeometrySi {
    let scale = source_units(units).metres_per_unit();
    ReferenceGeometrySi::from_bounding_box(
        [extent[0] * scale, extent[1] * scale, extent[2] * scale],
        longitudinal,
    )
}

// ---------------------------------------------------------------------------
// The store conversation
// ---------------------------------------------------------------------------

/// Declare what aproCAD publishes.
///
/// Writes to an undeclared type are refused by the store, so this must happen before the
/// first push. It is idempotent.
pub fn declare_interface(client: &dyn AproStoreClient) -> Result<()> {
    let type_id = TypeId::parse(MASS_PROPERTIES_TYPE).map_err(BridgeError::TypeId)?;
    client.declare_interface(&AppInterface {
        app: APP_SLUG.into(),
        publishes: vec![type_id],
        consumes: vec![],
    })?;
    Ok(())
}

/// What a successful publish did.
#[derive(Debug, Clone)]
pub struct PublishOutcome {
    /// The artifact instance, i.e. the vehicle's `uid`.
    pub instance: String,
    /// The `type_id` written.
    pub type_id: String,
    pub revision_number: u32,
    pub content_hash: String,
    pub byte_size: u64,
    /// True when the bytes matched the current revision, so nothing was appended.
    /// Publishing an unchanged design is a no-op, not an error.
    pub unchanged: bool,
}

impl PublishOutcome {
    pub fn summary(&self) -> String {
        if self.unchanged {
            format!(
                "{} is already at revision {} — nothing to publish",
                self.instance, self.revision_number
            )
        } else {
            format!(
                "published {} at revision {} ({} bytes, {})",
                self.instance,
                self.revision_number,
                self.byte_size,
                &self.content_hash[..self.content_hash.len().min(12)]
            )
        }
    }
}

/// Everything a publish needs. A struct rather than six positional arguments, so a caller
/// cannot silently swap the datum for the reference geometry — both are `[f64; 3]`-ish.
pub struct PublishRequest<'a> {
    pub vehicle: &'a Vehicle,
    pub mass: &'a MassProperties,
    /// Where the CAD origin sits in the consumer's body datum frame.
    pub datum: DatumOffset,
    /// Aerodynamic reference geometry, when it could be derived. Omitted means the
    /// consumer must ask the user.
    pub reference: Option<ReferenceGeometrySi>,
    /// A human label for the revision, shown in the hub's activity console.
    pub label: Option<&'a str>,
}

/// Publish a vehicle's mass properties.
///
/// Validates the payload before it goes on the wire, so a unit or datum mistake surfaces
/// in the producing app rather than in the consumer's importer.
pub fn publish(
    client: &dyn AproStoreClient,
    request: PublishRequest<'_>,
) -> Result<PublishOutcome> {
    let PublishRequest {
        vehicle,
        mass,
        datum,
        reference,
        label,
    } = request;

    let instance = instance_name(vehicle)?;
    let payload = to_payload(vehicle, mass, datum, reference);
    payload.validate()?;

    declare_interface(client)?;

    let type_id = TypeId::parse(MASS_PROPERTIES_TYPE).map_err(BridgeError::TypeId)?;
    let bytes = serde_json::to_vec_pretty(&payload)?;
    let handle: RevisionHandle = client.push_labeled(
        &type_id,
        &instance,
        Encoding::Json,
        label,
        &bytes,
    )?;

    Ok(PublishOutcome {
        instance,
        type_id: MASS_PROPERTIES_TYPE.to_string(),
        revision_number: handle.revision_number,
        content_hash: handle.content_hash,
        byte_size: handle.byte_size,
        unchanged: handle.unchanged,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use apro_document::vehicle::{Component, ComponentKind};

    fn vehicle_with_uid(uid: Option<&str>, units: Units) -> Vehicle {
        Vehicle {
            uid: uid.map(str::to_string),
            name: "Sounding Rocket".into(),
            units,
            parameters: None,
            components: vec![],
        }
    }

    /// A 100 x 100 x 600 mm block of aluminium, matching the closed-form checks in
    /// `apro-contracts`.
    fn block_mass_properties() -> MassProperties {
        let (edge, length) = (100.0_f64, 600.0_f64);
        let density = 2700.0 / 1.0e9;
        let volume = edge * edge * length;
        let mass = density * volume;
        let ixx = mass / 12.0 * (edge * edge + length * length);
        let izz = mass / 12.0 * (edge * edge + edge * edge);
        MassProperties {
            volume,
            mass,
            center_of_mass: [edge / 2.0, edge / 2.0, length / 2.0],
            inertia_tensor: [
                [ixx, 0.0, 0.0],
                [0.0, mass / 12.0 * (edge * edge + length * length), 0.0],
                [0.0, 0.0, izz],
            ],
        }
    }

    #[test]
    fn a_vehicle_without_a_uid_cannot_publish() {
        let vehicle = vehicle_with_uid(None, Units::Millimeters);
        assert!(matches!(instance_name(&vehicle), Err(BridgeError::MissingUid)));
    }

    #[test]
    fn a_blank_uid_counts_as_missing() {
        let vehicle = vehicle_with_uid(Some("   "), Units::Millimeters);
        assert!(matches!(instance_name(&vehicle), Err(BridgeError::MissingUid)));
    }

    #[test]
    fn the_instance_is_the_uid_not_the_name() {
        let vehicle = vehicle_with_uid(Some("veh-0001"), Units::Millimeters);
        assert_eq!(instance_name(&vehicle).unwrap(), "veh-0001");
        assert_ne!(instance_name(&vehicle).unwrap(), vehicle.name);
    }

    #[test]
    fn ensure_uid_stamps_a_fresh_identity_once() {
        let mut vehicle = vehicle_with_uid(None, Units::Millimeters);
        let first = ensure_uid(&mut vehicle);
        assert!(first.starts_with(UID_PREFIX));
        assert_eq!(vehicle.uid.as_deref(), Some(first.as_str()));

        // Idempotent: a second call must not re-identify the design.
        let second = ensure_uid(&mut vehicle);
        assert_eq!(first, second);
    }

    #[test]
    fn two_vehicles_get_different_identities() {
        let mut a = vehicle_with_uid(None, Units::Millimeters);
        let mut b = vehicle_with_uid(None, Units::Millimeters);
        assert_ne!(ensure_uid(&mut a), ensure_uid(&mut b));
    }

    #[test]
    fn publication_is_in_si_with_the_document_units_recorded() {
        let vehicle = vehicle_with_uid(Some("veh-0001"), Units::Millimeters);
        let payload = to_payload(&vehicle, &block_mass_properties(), DatumOffset::coincident(), None);

        assert_eq!(payload.units, "SI");
        assert_eq!(payload.schema, MASS_PROPERTIES_TYPE);
        assert_eq!(payload.source_units, SourceUnits::Millimeters);
        // The payload knows where its numbers came from...
        assert!(payload.source_note.contains("mm"));
        // ...and there were no components to count in this fixture.
        assert!(payload.source_note.contains("0 component"));
        payload.validate().unwrap();
    }

    #[test]
    fn a_permissive_document_unit_is_recorded_not_assumed() {
        let vehicle = vehicle_with_uid(Some("veh-0002"), Units::Inches);
        let payload = to_payload(&vehicle, &block_mass_properties(), DatumOffset::coincident(), None);
        assert_eq!(payload.source_units, SourceUnits::Inches);
        // 0.0254^2, not 1e-4 as a "divide by 100 twice" slip would produce.
        assert!((payload.source_units.inertia_scale() - 0.000_645_16).abs() < 1e-18);
    }

    #[test]
    fn the_datum_offset_reaches_the_payload_and_moves_the_centre_of_gravity() {
        let vehicle = vehicle_with_uid(Some("veh-0001"), Units::Millimeters);
        let mass = block_mass_properties();

        let coincident = to_payload(&vehicle, &mass, DatumOffset::coincident(), None);
        assert_eq!(coincident.datum_offset_m, [0.0; 3]);
        assert_eq!(
            coincident.center_of_gravity_in_datum(),
            coincident.center_of_gravity_m
        );

        // Declare the CAD origin 300 mm below the nose tip, so the datum sits at z = 0.
        let at_nose = to_payload(
            &vehicle,
            &mass,
            DatumOffset::from_document_units([0.0, 0.0, -300.0], &Units::Millimeters),
            None,
        );
        assert!((at_nose.datum_offset_m[2] + 0.3).abs() < 1e-15);
        assert!(at_nose.center_of_gravity_in_datum()[2].abs() < 1e-15);

        // The inertia must not have moved: it is about the centre of gravity.
        assert_eq!(coincident.inertia_kg_m2, at_nose.inertia_kg_m2);
    }

    #[test]
    fn a_datum_offset_given_in_inches_is_converted() {
        let datum = DatumOffset::from_document_units([1.0, 0.0, 0.0], &Units::Inches);
        assert!((datum.offset_m[0] - 0.0254).abs() < 1e-15);
    }

    #[test]
    fn every_document_unit_maps_across() {
        assert_eq!(source_units(&Units::Millimeters), SourceUnits::Millimeters);
        assert_eq!(source_units(&Units::Centimeters), SourceUnits::Centimeters);
        assert_eq!(source_units(&Units::Meters), SourceUnits::Meters);
        assert_eq!(source_units(&Units::Inches), SourceUnits::Inches);
        assert_eq!(source_units(&Units::Feet), SourceUnits::Feet);
    }

    #[test]
    fn the_published_range_is_named_after_the_owning_app() {
        // The store refuses a write whose type the app has not declared, and the type id
        // must be owned by the slug the hub launches us under.
        assert!(MASS_PROPERTIES_TYPE.starts_with(APP_SLUG));
        // Kebab-case only: a dot is rejected as an invalid type id.
        assert!(!MASS_PROPERTIES_TYPE.contains('.'));
        TypeId::parse(MASS_PROPERTIES_TYPE).unwrap();
    }

    #[test]
    fn the_source_note_counts_the_components() {
        let mut vehicle = vehicle_with_uid(Some("veh-0001"), Units::Millimeters);
        vehicle.components.push(Component {
            name: "Body".into(),
            material: "Al-6061-T6".into(),
            color: None,
            visible: true,
            transform: Default::default(),
            kind: ComponentKind::Solid(vec![]),
        });
        let payload = to_payload(&vehicle, &block_mass_properties(), DatumOffset::coincident(), None);
        assert!(
            payload.source_note.contains("1 component"),
            "note was {:?}",
            payload.source_note
        );
    }

    #[test]
    fn reference_geometry_comes_from_the_assembly_extent_in_metres() {
        // 200 x 150 x 600 mm, long axis Z.
        let reference =
            reference_geometry_from_extent([200.0, 150.0, 600.0], 2, &Units::Millimeters);

        assert!((reference.reference_length_m - 0.6).abs() < 1e-12);
        // The smaller transverse extent, in metres — not millimetres, and not the larger.
        assert!((reference.body_diameter_m - 0.15).abs() < 1e-12);
        let area = std::f64::consts::PI * 0.075 * 0.075;
        assert!((reference.reference_area_m2 - area).abs() < 1e-15);
        assert!(reference.convention.contains("axis 2"));
        reference.validate().unwrap();
    }

    #[test]
    fn reference_geometry_converts_across_document_units() {
        let inches = reference_geometry_from_extent([10.0, 10.0, 20.0], 2, &Units::Inches);
        assert!((inches.reference_length_m - 0.508).abs() < 1e-12);
        assert!((inches.body_diameter_m - 0.254).abs() < 1e-12);
    }

    #[test]
    fn reference_geometry_reaches_the_payload_and_survives_serialisation() {
        let vehicle = vehicle_with_uid(Some("veh-0001"), Units::Millimeters);
        let reference =
            reference_geometry_from_extent([200.0, 150.0, 600.0], 2, &Units::Millimeters);

        let payload = to_payload(
            &vehicle,
            &block_mass_properties(),
            DatumOffset::coincident(),
            Some(reference.clone()),
        );
        assert_eq!(payload.reference_geometry.as_ref(), Some(&reference));

        let text = payload.to_json().unwrap();
        let back = MassPropertiesV1::from_json_checked(&text).unwrap();
        assert_eq!(back.reference_geometry, Some(reference));
    }

    /// A caller that cannot derive the geometry must leave it unset rather than inventing
    /// one, and the consumer sees `None` and asks the user.
    #[test]
    fn reference_geometry_is_omitted_when_not_derivable() {
        let vehicle = vehicle_with_uid(Some("veh-0001"), Units::Millimeters);
        let payload = to_payload(
            &vehicle,
            &block_mass_properties(),
            DatumOffset::coincident(),
            None,
        );
        assert!(payload.reference_geometry.is_none());
    }
}
