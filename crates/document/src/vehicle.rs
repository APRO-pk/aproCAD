use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
    Reference(String),
    UserFunction { expr: String, variable: String, range: [f64; 2], samples: u32 },
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
