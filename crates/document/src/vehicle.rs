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
