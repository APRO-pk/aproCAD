use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub enum Units {
    Millimeters,
    Centimeters,
    Inches,
    Feet,
    Meters,
}

impl Default for Units {
    fn default() -> Self { Units::Millimeters }
}

// ---------------------------------------------------------------------------
// Vec3 helper for transforms
// ---------------------------------------------------------------------------
pub type Vec3 = (f64, f64, f64);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Transform {
    pub position: Vec3,
    pub rotation: Vec3,
    /// Optional per-axis scale. Absent / None means uniform (1,1,1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Vec3>,
}

impl Default for Transform {
    fn default() -> Self {
        Transform { position: (0.0, 0.0, 0.0), rotation: (0.0, 0.0, 0.0), scale: None }
    }
}

// ---------------------------------------------------------------------------
// Vehicle & Component
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Vehicle {
    pub name: String,
    /// Stable identity for this design, assigned the first time it is published to
    /// APRO Works.
    ///
    /// `name` is a label the author may edit at any time. `uid` is what the platform
    /// tracks the published artifact under, so renaming a design does not orphan its
    /// revision history or silently look like a different vehicle to every consumer.
    ///
    /// Absent on documents authored before this field existed, and on designs that have
    /// never been published. The publish path refuses to guess rather than falling back
    /// to `name`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "de_uid_opt"
    )]
    pub uid: Option<String>,
    #[serde(default)]
    pub units: Units,
    /// Named parameters and equations (`Parameters(...)` block).
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::params::de_params_opt")]
    pub parameters: Option<Vec<crate::params::Parameter>>,
    pub components: Vec<Component>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Component {
    pub name: String,
    #[serde(default)]
    pub material: String,
    /// Optional display color, e.g. "#ff8800" or a CSS color name like "red".
    /// When absent the renderer derives a color from the material name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default = "default_visible")]
    pub visible: bool,
    #[serde(default)]
    pub transform: Transform,
    pub kind: ComponentKind,
}

fn default_visible() -> bool { true }

/// Deserialize the `Vehicle.uid` field.
///
/// Accepts `uid: "veh-0001"` and `uid: None`.
///
/// RON only accepts the `Some(...)` wrapper around an `Option`, but that wrapper is noise
/// in a document a human — or a language model — is writing, and JSON has no wrapper at
/// all. Dispatching through `deserialize_any` instead of `deserialize_option` lets the
/// bare string through, so one spelling works in both formats, which matters because
/// `json_to_ron` converts between them. This mirrors `de_params_opt` in `params.rs`,
/// which solved the same problem for `parameters`.
pub fn de_uid_opt<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    struct UidVisitor;

    impl<'de> serde::de::Visitor<'de> for UidVisitor {
        type Value = Option<String>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a uid string or `None`")
        }

        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
            Ok(Some(value.to_string()))
        }

        fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
            Ok(Some(value))
        }

        fn visit_some<D2: Deserializer<'de>>(self, d: D2) -> Result<Self::Value, D2::Error> {
            // Re-dispatch so a wrapped value lands on `visit_str`.
            d.deserialize_any(UidVisitor)
        }
    }

    d.deserialize_any(UidVisitor)
}

impl Component {
    pub fn material_name(&self) -> String {
        if !self.material.is_empty() {
            return self.material.clone();
        }
        match &self.kind {
            ComponentKind::NoseCone(p) => p.material.clone(),
            ComponentKind::BodyTube(p) => p.material.clone(),
            ComponentKind::Transition(p) => p.material.clone(),
            ComponentKind::Tank(p) => p.material.clone(),
            ComponentKind::Nozzle(p) => p.material.clone(),
            ComponentKind::FinSet(p) => p.material.clone(),
            ComponentKind::Solid(_) => "Steel-4130".into(),
            ComponentKind::Sketch(_) => String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// ComponentKind – aerospace shorthands + general-purpose Solid ops
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub enum ComponentKind {
    #[serde(alias = "Nosecone", alias = "nose_cone")]
    NoseCone(NoseConeParams),
    #[serde(alias = "Bodytube", alias = "body_tube")]
    BodyTube(BodyTubeParams),
    Nozzle(NozzleParams),
    #[serde(alias = "Finset", alias = "fin_set")]
    FinSet(FinSetParams),
    Transition(TransitionParams),
    Tank(TankParams),
    /// General-purpose solid defined by an operation stack.
    Solid(Vec<SolidOp>),
    /// A 2D sketch on a workplane. Evaluates to no renderable mesh on its own;
    /// reference it from `Profile::Reference` inside an Extrude/Revolve/Loft to
    /// turn it into a solid.
    Sketch(SketchParams),
}

// ---------------------------------------------------------------------------
// Solid operations (general-purpose CAD)
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub enum SolidOp {
    Revolve {
        profile: Profile,
        angle: f64,
        axis: Option<Axis>,
    },
    /// Revolve a chain of coaxial profile segments as one piece.
    /// Each segment is an expanded point list `[x, r]`. Segments are
    /// concatenated head-to-tail — the last point of segment i must
    /// match the first point of segment i+1 (same x and radius).
    /// This eliminates seam/shoulder artifacts from separate revolves.
    RevolveChain {
        segments: Vec<Vec<[f64; 2]>>,
        angle: f64,
    },
    Extrude {
        profile: Profile,
        height: f64,
        direction: Option<Direction>,
        taper: Option<f64>,
    },
    Loft {
        profiles: Vec<Profile>,
        guide_curves: Option<Vec<Profile>>,
    },
    Sweep {
        profile: Profile,
        path: Path3D,
        twist: Option<f64>,
    },
    #[schemars(skip)]
    Shell {
        thickness: f64,
        faces: Option<FaceRef>,
    },
    Boolean {
        kind: BooleanKind,
        target: SolidRef,
    },
    #[schemars(skip)]
    Fillet {
        radius: f64,
        edges: EdgeRef,
    },
    #[schemars(skip)]
    Chamfer {
        distance: f64,
        edges: EdgeRef,
    },
    // Machined features: drilled holes & patterns (cut into the current stack).
    /// A single drilled hole along an axis through the local origin.
    Hole {
        diameter: f64,
        depth: f64,
        axis: Axis,
    },
    /// A ring of `count` holes on a bolt circle centred on the Z axis.
    BoltCircle {
        count: u32,
        pitch_diameter: f64,
        hole_diameter: f64,
        depth: f64,
    },
    /// A rectangular grid of holes in the XY plane, centred on the Z axis.
    RectPattern {
        x_count: u32,
        y_count: u32,
        spacing_x: f64,
        spacing_y: f64,
        hole_diameter: f64,
        depth: f64,
    },
    TransformOp {
        translate: Option<Vec3>,
        rotate: Option<Vec3>,
        scale: Option<Vec3>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub enum BooleanKind {
    Union,
    Difference,
    Intersection,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub enum Axis { X, Y, Z }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub enum Direction { PosX, NegX, PosY, NegY, PosZ, NegZ }

// ---------------------------------------------------------------------------
// Profile – 2D sketch shapes
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub enum Profile {
    Points(Vec<[f64; 2]>),
    Circle { radius: f64 },
    Rectangle { width: f64, height: f64, corner_radius: Option<f64> },
    Polygon { sides: u32, circumradius: f64 },
    /// A 2D loop produced by a `Sketch` component (possibly transformed into a
    /// local extrusion frame). Resolved against the sibling sketch registry
    /// when the stack is evaluated.
    Reference(String),
    /// A placed sketch plane: the resolved loop in the plane's own 2D frame,
    /// bundled with the workspace transform that puts an extrusion off that
    /// plane back into place. Produced by [`SketchPlane::placed_profile`];
    /// authored output should normally use `Reference` instead.
    PlacedReference { sketch: String, origin: Vec3, plane: SketchPlane },
    UserFunction { expr: String, variable: String, range: [f64; 2], samples: u32 },
}

// ---------------------------------------------------------------------------
// Sketch – 2D entities drawn on a workplane
// ---------------------------------------------------------------------------
/// The workplane a sketch is drawn on. Local 2D coordinates are `(u, v)`;
/// the 3D frame (used to place an extrusion back into space) is:
///   XY -> u = +X, v = +Y, normal = +Z   (default, matches the extrude axis)
///   XZ -> u = +X, v = +Z, normal = -Y
///   YZ -> u = +Y, v = +Z, normal = +X
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub enum SketchPlane {
    XY,
    XZ,
    YZ,
}

impl Default for SketchPlane {
    fn default() -> Self { SketchPlane::XY }
}

impl SketchPlane {
    /// Orthonormal frame `(u_axis, v_axis, normal)` in world space.
    pub fn basis(&self) -> ([f64; 3], [f64; 3], [f64; 3]) {
        match self {
            SketchPlane::XY => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
            SketchPlane::XZ => ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]),
            SketchPlane::YZ => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        }
    }

    /// Place a plane-local 2D point into world space.
    pub fn to_world(&self, uv: [f64; 2], origin: Vec3) -> [f64; 3] {
        let (u, v, _) = self.basis();
        [
            origin.0 + u[0] * uv[0] + v[0] * uv[1],
            origin.1 + u[1] * uv[0] + v[1] * uv[1],
            origin.2 + u[2] * uv[0] + v[2] * uv[1],
        ]
    }

    /// World-space origin of the plane at `offset` along its normal.
    pub fn origin_at(&self, offset: f64) -> Vec3 {
        let (_, _, n) = self.basis();
        (n[0] * offset, n[1] * offset, n[2] * offset)
    }
}

/// One 2D sketch entity, in the plane's local `(u, v)` coordinates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub enum SketchEntity {
    /// A single straight segment.
    Line { start: [f64; 2], end: [f64; 2] },
    /// An axis-aligned rectangle spanning two opposite corners.
    Rectangle { corner1: [f64; 2], corner2: [f64; 2] },
    /// A full circle.
    Circle { center: [f64; 2], radius: f64 },
    /// A circular arc from `start_angle` to `end_angle` (radians, CCW positive).
    Arc { center: [f64; 2], radius: f64, start_angle: f64, end_angle: f64 },
    /// A Catmull-Rom spline passing through every point. Set `closed` to join
    /// the last point back to the first.
    Spline { points: Vec<[f64; 2]>, closed: bool },
}

/// A named 2D sketch drawn on a workplane.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct SketchParams {
    /// Plane the sketch is drawn on. Defaults to XY.
    #[serde(default)]
    pub plane: SketchPlane,
    /// Offset of the plane's origin along its normal. Lets several sketches
    /// share a plane orientation at different heights.
    #[serde(default)]
    pub offset: f64,
    /// Entities in draw order. Loops are stitched geometrically, not by order.
    pub entities: Vec<SketchEntity>,
}

// ---------------------------------------------------------------------------
// Path3D – sweep path definitions
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub enum Path3D {
    Line { start: Vec3, end: Vec3 },
    Arc { center: Vec3, radius: f64, start_angle: f64, end_angle: f64 },
    Spline(Vec<Vec3>),
    Helix { radius: f64, pitch: f64, turns: f64 },
}

// ---------------------------------------------------------------------------
// Geometry references for Boolean / fillet / chamfer ops
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub enum SolidRef {
    Component(String),
    This,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub enum FaceRef {
    Index(usize),
    Normal(Vec3),
    Plane { point: Vec3, normal: Vec3 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub enum EdgeRef {
    All,
    Indices(Vec<usize>),
}

// ---------------------------------------------------------------------------
// Aerospace param structs (unchanged, kept for backward compat)
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct NoseConeParams {
    pub profile: NoseConeProfile,
    pub length: f64,
    pub base_radius: f64,
    pub wall: f64,
    pub material: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub enum NoseConeProfile {
    Conical,
    Ogive,
    VonKarman,
    Haack { c: f64 },
    Power { n: f64 },
    Parabolic { k: f64 },
}

impl Default for NoseConeProfile {
    fn default() -> Self { NoseConeProfile::VonKarman }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct BodyTubeParams {
    pub length: f64,
    pub radius: f64,
    pub wall: f64,
    pub material: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct NozzleParams {
    pub kind: NozzleKind,
    pub throat_radius: f64,
    pub expansion_ratio: f64,
    pub percent_bell: f64,
    pub chamber_radius: f64,
    pub wall: f64,
    pub material: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub enum NozzleKind {
    Conical,
    Bell,
    Moc,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct FinSetParams {
    pub count: u32,
    pub root_chord: f64,
    pub tip_chord: f64,
    pub span: f64,
    pub sweep: f64,
    pub airfoil: AirfoilParams,
    pub thickness: f64,
    pub material: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct AirfoilParams {
    pub family: AirfoilFamily,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub enum AirfoilFamily {
    NACA { digits: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct TransitionParams {
    pub length: f64,
    pub start_radius: f64,
    pub end_radius: f64,
    pub wall: f64,
    pub material: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct TankParams {
    pub radius: f64,
    pub cylindrical_length: f64,
    pub dome: DomeKind,
    pub wall: f64,
    pub material: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub enum DomeKind {
    Hemispherical,
    Ellipsoidal { ratio: f64 },
}

impl Default for DomeKind {
    fn default() -> Self { DomeKind::Ellipsoidal { ratio: 2.0 } }
}

// ---------------------------------------------------------------------------
// Validation types
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Issue {
    pub severity: IssueSeverity,
    pub message: String,
    pub component: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub enum IssueSeverity {
    Error,
    Warning,
    Info,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A document written before `uid` existed must still load. Adding a field to the
    /// grammar would otherwise invalidate every saved file in the library.
    #[test]
    fn a_document_without_a_uid_still_parses() {
        let ron = r#"Vehicle(
            name: "Legacy",
            units: Millimeters,
            components: [],
        )"#;
        let vehicle: Vehicle = ron::from_str(ron).unwrap();
        assert_eq!(vehicle.uid, None);
        assert_eq!(vehicle.name, "Legacy");
    }

    /// RON matches named struct fields by name, not position, which is what let `uid` be
    /// inserted into the middle of the struct without rewriting saved documents.
    #[test]
    fn uid_round_trips_through_ron() {
        let ron = r#"Vehicle(
            uid: "veh-0001",
            name: "Rocket",
            units: Millimeters,
            components: [],
        )"#;
        let vehicle: Vehicle = ron::from_str(ron).unwrap();
        assert_eq!(vehicle.uid.as_deref(), Some("veh-0001"));

        let text = ron::to_string(&vehicle).unwrap();
        let again: Vehicle = ron::from_str(&text).unwrap();
        assert_eq!(vehicle, again);
    }

    /// An unpublished design must not accumulate a null field in every saved file.
    #[test]
    fn an_unpublished_design_omits_the_uid() {
        let vehicle = Vehicle {
            uid: None,
            name: "Draft".into(),
            units: Units::Millimeters,
            parameters: None,
            components: vec![],
        };
        let text = ron::to_string(&vehicle).unwrap();
        assert!(!text.contains("uid"), "serialized as {text}");
    }

    /// The schema the AI grammar is generated from must expose `uid`, otherwise a model
    /// can never produce a document the publish path will accept.
    #[test]
    fn the_json_schema_mentions_uid() {
        let schema = schemars::schema_for!(Vehicle);
        let json = serde_json::to_string(&schema).unwrap();
        assert!(json.contains("uid"), "schema did not mention uid");
    }

    /// `json_to_ron` converts between the two formats, so a bare string has to mean the
    /// same thing on both sides. JSON has no `Some(...)`, which is the whole reason the
    /// RON side accepts a bare string too.
    #[test]
    fn uid_round_trips_through_json() {
        let json = r#"{
            "name": "Rocket",
            "uid": "veh-0001",
            "units": "Millimeters",
            "components": []
        }"#;
        let vehicle: Vehicle = serde_json::from_str(json).unwrap();
        assert_eq!(vehicle.uid.as_deref(), Some("veh-0001"));

        let text = serde_json::to_string(&vehicle).unwrap();
        let again: Vehicle = serde_json::from_str(&text).unwrap();
        assert_eq!(vehicle, again);
    }

    /// A JSON null and an absent key must both mean "no uid", not a parse failure.
    #[test]
    fn json_null_and_absent_both_mean_no_uid() {
        let with_null = r#"{"name":"A","uid":null,"units":"Millimeters","components":[]}"#;
        assert_eq!(serde_json::from_str::<Vehicle>(with_null).unwrap().uid, None);

        let without = r#"{"name":"A","units":"Millimeters","components":[]}"#;
        assert_eq!(serde_json::from_str::<Vehicle>(without).unwrap().uid, None);
    }
}
