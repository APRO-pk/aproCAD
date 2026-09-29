use apro_document::vehicle::{SolidOp, Profile, Path3D, Axis, Direction, SolidRef, BooleanKind, SketchPlane};
use apro_document::params::ParamEnv;
use apro_kernel::{transform_mesh, MeshData};
use apro_kernel::backend::{ShapeBackend, TruckBackend};
use std::sync::OnceLock;

// Kernel v2 seam: geometry ops flow through the ShapeBackend trait so the
// OpenCascade backend can replace Truck without touching this pipeline.
static TRUCK_BACKEND: OnceLock<TruckBackend> = OnceLock::new();
fn backend() -> &'static TruckBackend { TRUCK_BACKEND.get_or_init(TruckBackend::new) }

/// A sketch resolved into the pieces an op needs: its plane-local 2D loop plus
/// the workplane placement a resulting mesh must be moved by.
#[derive(Debug, Clone)]
pub struct ResolvedSketch {
    pub loop_pts: Vec<[f64; 2]>,
    pub plane: SketchPlane,
    pub origin: apro_document::vehicle::Vec3,
}

/// Looks a named sketch up by component name. Returns an error string when the
/// name is unknown or the sketch does not form a single closed region.
pub type SketchLookup<'a> = &'a mut dyn FnMut(&str) -> Result<ResolvedSketch, String>;

/// Reject-all lookup, for stacks evaluated outside a vehicle (no siblings).
fn no_sketches(name: &str) -> Result<ResolvedSketch, String> {
    Err(format!("sketch '{name}' cannot be resolved outside a vehicle"))
}

/// Expand any Profile variant into a concrete 2D point list. `env` supplies
/// named parameters; `Profile::UserFunction` evaluates its `expr` against the
/// environment plus the free `variable` sampled across `range`.
pub fn expand_profile(profile: &Profile, samples: u32, env: &ParamEnv) -> Result<Vec<[f64; 2]>, String> {
    expand_profile_with(profile, samples, env, &mut no_sketches)
}

/// Like [`expand_profile`], but resolves `Profile::Reference` against a sketch
/// registry.
pub fn expand_profile_with(
    profile: &Profile,
    samples: u32,
    env: &ParamEnv,
    lookup: SketchLookup,
) -> Result<Vec<[f64; 2]>, String> {
    match profile {
        Profile::Reference(name) => Ok(lookup(name)?.loop_pts),
        Profile::PlacedReference { sketch, origin, plane } => {
            // The plane and origin are inlined, so this resolves without a
            // registry — useful for self-contained documents and previews.
            let _ = (origin, plane);
            Ok(lookup(sketch)?.loop_pts)
        }
        _ => expand_profile_inner(profile, samples, env),
    }
}

/// Resolve a profile into its 2D loop plus the workplane placement that the
/// resulting mesh must be moved by. Placement is `None` for every profile that
/// is already authored in op-local coordinates.
fn resolve_profile(
    profile: &Profile,
    samples: u32,
    env: &ParamEnv,
    lookup: SketchLookup,
) -> Result<(Vec<[f64; 2]>, Option<(SketchPlane, apro_document::vehicle::Vec3)>), String> {
    match profile {
        Profile::Points(_)
        | Profile::Circle { .. }
        | Profile::Rectangle { .. }
        | Profile::Polygon { .. }
        | Profile::UserFunction { .. } => Ok((expand_profile(profile, samples, env)?, None)),
        Profile::PlacedReference { sketch, origin, plane } => {
            // The plane placement is inlined, so the loop stays local and the
            // placement rides along to be applied after the op runs.
            let resolved = lookup(sketch)?;
            Ok((resolved.loop_pts, Some((*plane, *origin))))
        }
        Profile::Reference(name) => {
            let resolved = lookup(name)?;
            Ok((resolved.loop_pts, Some((resolved.plane, resolved.origin))))
        }
    }
}

/// Move a mesh produced from a sketch-plane profile into world space.
///
/// The kernel always extrudes/revolves about the local +Z axis from Z=0, so a
/// sketch on another plane is handled by rotating that local frame onto the
/// plane (a quarter turn about the shared world axis) and translating to the
/// plane origin. Extrusion length is already baked into the mesh, so the only
/// translation applied here is the sketch's own plane offset.
pub fn place_mesh_on_plane(
    mesh: &MeshData,
    plane: SketchPlane,
    origin: apro_document::vehicle::Vec3,
) -> MeshData {
    let t = [origin.0, origin.1, origin.2];
    let rot = match plane {
        SketchPlane::XY => [0.0, 0.0, 0.0],
        SketchPlane::XZ => [90.0, 0.0, 0.0],
        SketchPlane::YZ => [0.0, 90.0, 0.0],
    };
    transform_mesh(mesh, &t, &rot)
}

fn expand_profile_inner(profile: &Profile, samples: u32, env: &ParamEnv) -> Result<Vec<[f64; 2]>, String> {
    match profile {
        // Resolved by `expand_profile_with` before this point; reaching here
        // means the caller bypassed the sketch registry.
        Profile::Reference(name) => Err(format!("sketch reference '{name}' was not resolved")),
        Profile::PlacedReference { .. } => Err("placed sketch reference was not resolved".into()),
        Profile::Points(pts) => Ok(pts.clone()),
        Profile::Rectangle { width, height, corner_radius } => {
            let w = *width;
            let h = *height;
            if let Some(r) = corner_radius {
                if *r <= 0.0 {
                    return Err("corner_radius must be positive".into());
                }
                let r = *r;
                // Rounded rectangle: 4 arc segments
                let mut pts = Vec::new();
                let seg = samples.max(4) / 4;
                // bottom-left arc
                for i in 0..seg {
                    let a = std::f64::consts::PI * (1.0 + i as f64 / seg as f64);
                    pts.push([-w / 2.0 + r + r * a.cos(), -h / 2.0 + r + r * a.sin()]);
                }
                // bottom-right arc
                for i in 0..seg {
                    let a = std::f64::consts::PI * (1.5 + i as f64 / seg as f64);
                    pts.push([w / 2.0 - r + r * a.cos(), -h / 2.0 + r + r * a.sin()]);
                }
                // top-right arc
                for i in 0..seg {
                    let a = std::f64::consts::PI * (0.0 + i as f64 / seg as f64);
                    pts.push([w / 2.0 - r + r * a.cos(), h / 2.0 - r + r * a.sin()]);
                }
                // top-left arc
                for i in 0..seg {
                    let a = std::f64::consts::PI * (0.5 + i as f64 / seg as f64);
                    pts.push([-w / 2.0 + r + r * a.cos(), h / 2.0 - r + r * a.sin()]);
                }
                Ok(pts)
            } else {
                Ok(vec![
                    [-w / 2.0, -h / 2.0],
                    [ w / 2.0, -h / 2.0],
                    [ w / 2.0,  h / 2.0],
                    [-w / 2.0,  h / 2.0],
                ])
            }
        }
        Profile::Circle { radius } => {
            let n = samples.max(12);
            let pts: Vec<[f64; 2]> = (0..n)
                .map(|i| {
                    let a = std::f64::consts::TAU * i as f64 / n as f64;
                    [radius * a.cos(), radius * a.sin()]
                })
                .collect();
            Ok(pts)
        }
        Profile::Polygon { sides, circumradius } => {
            if *sides < 3 {
                return Err("polygon must have at least 3 sides".into());
            }
            let n = *sides;
            let pts: Vec<[f64; 2]> = (0..n)
                .map(|i| {
                    let a = std::f64::consts::TAU * i as f64 / n as f64;
                    [circumradius * a.cos(), circumradius * a.sin()]
                })
                .collect();
            Ok(pts)
        }
        Profile::UserFunction { expr, variable, range, samples: s } => {
            let n = (*s).max(2) as usize;
            let (lo, hi) = (range[0], range[1]);
            let mut pts = Vec::with_capacity(n);
            for i in 0..n {
                let t = i as f64 / (n - 1) as f64;
                let x = lo + (hi - lo) * t;
                let mut local = env.clone();
                local.insert(variable.clone(), x);
                let y = apro_document::params::eval_expr(expr, &local)
                    .map_err(|e| format!("UserFunction '{expr}' at {variable}={x:.3}: {e}"))?;
                pts.push([x, y]);
            }
            Ok(pts)
        }
    }
}

/// Convert a Path3D into sweep path points.
pub fn convert_path(path: &Path3D, _samples: u32) -> Result<Vec<[f64; 3]>, String> {
    match path {
        Path3D::Line { start, end } => {
            Ok(vec![[start.0, start.1, start.2], [end.0, end.1, end.2]])
        }
        Path3D::Arc { center, radius, start_angle, end_angle } => {
            let n = _samples.max(8);
            let (sx, cx) = start_angle.sin_cos();
            let (ex, ey) = end_angle.sin_cos();
            let start = [center.0 + radius * cx, center.1 + radius * sx, center.2];
            let end_pt = [center.0 + radius * ex, center.1 + radius * ey, center.2];
            if n <= 2 {
                return Ok(vec![start, end_pt]);
            }
            let mut pts = vec![start];
            for i in 1..n - 1 {
                let a = start_angle + (end_angle - start_angle) * i as f64 / (n - 1) as f64;
                let (s, c) = a.sin_cos();
                pts.push([center.0 + radius * c, center.1 + radius * s, center.2]);
            }
            pts.push(end_pt);
            Ok(pts)
        }
        Path3D::Spline(points) => {
            if points.len() < 2 {
                return Err("spline path must have at least 2 points".into());
            }
            let arr: Vec<[f64; 3]> = points.iter().map(|p| [p.0, p.1, p.2]).collect();
            if points.len() == 2 {
                return Ok(arr); // straight segment, nothing to smooth
            }
            // Catmull-Rom: C1-continuous curve passing THROUGH every control point.
            let per_span = ((_samples.max(16) as usize) / (points.len() - 1)).max(8);
            let mut pts: Vec<[f64; 3]> = Vec::with_capacity(per_span * (points.len() - 1) + 1);
            for span in 0..points.len() - 1 {
                let p0 = if span == 0 { arr[0] } else { arr[span - 1] };
                let p1 = arr[span];
                let p2 = arr[span + 1];
                let p3 = if span + 2 < arr.len() { arr[span + 2] } else { arr[arr.len() - 1] };
                for s in 0..per_span {
                    let t = s as f64 / per_span as f64;
                    pts.push(catmull_rom_point(p0, p1, p2, p3, t));
                }
            }
            pts.push(arr[arr.len() - 1]);
            Ok(pts)
        }
        Path3D::Helix { radius, pitch, turns } => {
            let n = (_samples as f64 * turns).ceil() as u32;
            let n = n.max(8);
            let pts: Vec<[f64; 3]> = (0..n)
                .map(|i| {
                    let a = std::f64::consts::TAU * turns * i as f64 / n as f64;
                    let z = pitch * turns * i as f64 / n as f64;
                    [radius * a.cos(), radius * a.sin(), z]
                })
                .collect();
            Ok(pts)
        }
    }
}

/// Solid cylinder along +Z: revolve a rectangle profile `(axisPos, radius)`.
fn cylinder_mesh(radius: f64, length: f64) -> Result<MeshData, String> {
    if radius <= 0.0 || length <= 0.0 {
        return Err("cylinder radius/length must be positive".into());
    }
    Ok(backend().revolve_mesh(&[
        [0.0, 0.0],
        [0.0, radius],
        [length, radius],
        [length, 0.0],
    ], 360.0))
}

/// Subtract `cutter` from `base` via the active backend boolean.
fn cut_from(base: &MeshData, cutter: &MeshData) -> Result<MeshData, String> {
    backend().boolean(base, cutter, apro_kernel::BooleanKind::Difference)
}

/// Uniform Catmull-Rom spline point: C1-continuous, passes through p1->p2.
fn catmull_rom_point(p0:[f64;3],p1:[f64;3],p2:[f64;3],p3:[f64;3],t:f64)->[f64;3]{
    let t2=t*t;
    let t3=t2*t;
    let mut out=[0.0f64;3];
    for i in 0..3 {
        out[i]=0.5*((2.0*p1[i])
            +(-p0[i]+p2[i])*t
            +(2.0*p0[i]-5.0*p1[i]+4.0*p2[i]-p3[i])*t2
            +(-p0[i]+3.0*p1[i]-3.0*p2[i]+p3[i])*t3);
    }
    out
}

/// Evaluate a SolidOp stack and produce a mesh.
///
/// The pipeline works as follows:
/// 1. The first profile-based op (revolve/extrude/loft/sweep) creates the base geometry.
/// 2. Subsequent profile-based ops are independent (no chaining — blocked ops prevent it).
/// 3. TransformOp applies a mesh-level translation/rotation.
/// 4. Boolean combines the current mesh with another component's mesh (or itself).
/// 5. Blocked ops (shell, fillet, chamfer) return an error.
pub fn evaluate_solid_ops(ops: &[SolidOp]) -> Result<MeshData, String> {
    evaluate_solid_ops_full(ops, &ParamEnv::new(), &mut no_sketches, &mut |name: &str| -> Result<MeshData, String> {
        Err(format!("boolean target component '{name}' cannot be resolved outside a vehicle"))
    })
}

/// Like [`evaluate_solid_ops`], but evaluates profile expressions against the
/// parameter environment, resolves `SolidRef::Component` targets through
/// `resolve_component` (used when evaluating inside a vehicle where sibling
/// components are available), and resolves sketch references through
/// `resolve_sketch`.
pub fn evaluate_solid_ops_full(
    ops: &[SolidOp],
    params: &ParamEnv,
    resolve_sketch: SketchLookup,
    resolve_component: &mut dyn FnMut(&str) -> Result<MeshData, String>,
) -> Result<MeshData, String> {
    if ops.is_empty() {
        return Err("solid op stack is empty".into());
    }

    let mut current_mesh: Option<MeshData> = None;

    for op in ops {
        match op {
            SolidOp::Revolve { profile, angle, axis } => {
                let (pts, placement) = resolve_profile(profile, 24, params, resolve_sketch)?;
                let mut mesh = if let Some(ax) = axis {
                    match ax {
                        Axis::X => {
                            // revolve_mesh puts axial along Z; rotate Z -> X
                            let m = backend().revolve_mesh(&pts, *angle);
                            transform_mesh(&m, &[0.0, 0.0, 0.0], &[0.0, 90.0, 0.0])
                        }
                        Axis::Y => {
                            // revolve_mesh puts axial along Z; rotate Z -> Y
                            let m = backend().revolve_mesh(&pts, *angle);
                            transform_mesh(&m, &[0.0, 0.0, 0.0], &[-90.0, 0.0, 0.0])
                        }
                        Axis::Z => backend().revolve_mesh(&pts, *angle),
                    }
                } else {
                    backend().revolve_mesh(&pts, *angle)
                };
                if let Some((plane, origin)) = placement {
                    // A sketch-plane revolve needs no extra lift along the
                    // normal; the rotation itself carries it to the plane.
                    mesh = place_mesh_on_plane(&mesh, plane, origin);
                }
                current_mesh = Some(mesh);
            }

            SolidOp::Boolean { kind, target } => {
                let current = current_mesh
                    .as_ref()
                    .ok_or_else(|| "Boolean op must follow a solid-producing op (it cannot be first)".to_string())?;
                let target_mesh = match target {
                    SolidRef::This => current.clone(),
                    SolidRef::Component(name) => resolve_component(name)?,
                };
                let k = match kind {
                    BooleanKind::Union => apro_kernel::ops::BooleanKind::Union,
                    BooleanKind::Difference => apro_kernel::ops::BooleanKind::Difference,
                    BooleanKind::Intersection => apro_kernel::ops::BooleanKind::Intersection,
                };
                current_mesh = Some(
                    backend().boolean(current, &target_mesh, k)
                        .map_err(|e| format!("boolean ({kind:?}) failed: {e}"))?,
                );
            }

            SolidOp::RevolveChain { segments, angle } => {
                if segments.is_empty() {
                    return Err("RevolveChain requires at least one segment".into());
                }
                // Concatenate segments head-to-tail
                let mut combined = segments[0].clone();
                for seg in &segments[1..] {
                    if seg.is_empty() { continue; }
                    // Offset segment so its first point matches the combined last point
                    let last_x = combined.last().map_or(0.0, |p| p[0]);
                    let offset = last_x - seg[0][0];
                    for pt in seg {
                        combined.push([pt[0] + offset, pt[1]]);
                    }
                }
                let mesh = backend().revolve_mesh(&combined, *angle);
                current_mesh = Some(mesh);
            }

            SolidOp::Extrude { profile, height, direction, taper } => {
                let (pts, placement) = resolve_profile(profile, 24, params, resolve_sketch)?;

                // Direction resolves to (rotation that carries the local +Z
                // extrusion onto the requested axis, sign of the height).
                let (rot, sign) = match direction {
                    None | Some(Direction::PosZ) => ([0.0, 0.0, 0.0], 1.0),
                    Some(Direction::NegZ) => ([0.0, 0.0, 0.0], -1.0),
                    Some(Direction::PosX) => ([0.0, -90.0, 0.0], 1.0),
                    Some(Direction::NegX) => ([0.0, 90.0, 0.0], 1.0),
                    Some(Direction::PosY) => ([90.0, 0.0, 0.0], 1.0),
                    Some(Direction::NegY) => ([-90.0, 0.0, 0.0], 1.0),
                };

                let mut mesh = match taper {
                    None => backend()
                        .extrude_mesh(&pts, sign * height)
                        .map_err(|e| format!("extrude failed: {e}"))?,
                    Some(t) => {
                        // Tapered extrude as a ruled loft. `loft_mesh_spaced`
                        // places the second profile `height` away, so the solid
                        // reaches the requested length.
                        let top_pts: Vec<[f64; 2]> = pts
                            .iter()
                            .map(|&[x, y]| [x * (1.0 + t), y * (1.0 + t)])
                            .collect();
                        backend()
                            .loft_mesh_spaced(&[pts.clone(), top_pts], sign * height)
                            .map_err(|e| format!("tapered extrude loft failed: {e}"))?
                    }
                };

                if rot != [0.0, 0.0, 0.0] {
                    mesh = transform_mesh(&mesh, &[0.0, 0.0, 0.0], &rot);
                }
                if let Some((plane, origin)) = placement {
                    mesh = place_mesh_on_plane(&mesh, plane, origin);
                }
                current_mesh = Some(mesh);
            }

            SolidOp::Loft { profiles, guide_curves: _ } => {
                if profiles.len() < 2 {
                    return Err("loft requires at least 2 profiles".into());
                }
                let mut pts_list = Vec::new();
                for p in profiles {
                    let (pts, _) = resolve_profile(p, 24, params, resolve_sketch)?;
                    pts_list.push(pts);
                }                let mesh = backend().loft_mesh(&pts_list)
                    .map_err(|e| format!("loft failed: {e}"))?;
                current_mesh = Some(mesh);
            }

            SolidOp::Sweep { profile, path, twist: _ } => {
                let (pts, _) = resolve_profile(profile, 24, params, resolve_sketch)?;
                let path_pts = convert_path(path, 24)?;
                let mesh = backend().sweep_mesh(&pts, &path_pts)
                    .map_err(|e| format!("sweep failed: {e}"))?;
                current_mesh = Some(mesh);
            }

            SolidOp::TransformOp { translate, rotate, scale: _ } => {
                let mesh = current_mesh.as_ref()
                    .ok_or_else(|| "TransformOp applied to empty stack".to_string())?;
                let t = translate.unwrap_or((0.0, 0.0, 0.0));
                let r = rotate.unwrap_or((0.0, 0.0, 0.0));
                current_mesh = Some(transform_mesh(mesh, &[t.0, t.1, t.2], &[r.0, r.1, r.2]));
            }

            SolidOp::Hole { diameter, depth, axis } => {
                // Drill one cylinder along `axis` through the local origin.
                let base = current_mesh.as_ref()
                    .ok_or_else(|| "Hole applied to empty stack (add Extrude/Revolve first)".to_string())?;
                if *diameter <= 0.0 { return Err("Hole diameter must be positive".into()); }
                if *depth <= 0.0 { return Err("Hole depth must be positive".into()); }
                let mut cutter = cylinder_mesh(*diameter / 2.0, *depth)?;
                let (rx, ry, rz) = match axis {
                    Axis::X => (0.0, -90.0, 0.0),
                    Axis::Y => (90.0, 0.0, 0.0),
                    Axis::Z => (0.0, 0.0, 0.0),
                };
                cutter = transform_mesh(&cutter, &[0.0, 0.0, 0.0], &[rx, ry, rz]);
                current_mesh = Some(cut_from(base, &cutter)?);
            }

            SolidOp::BoltCircle { count, pitch_diameter, hole_diameter, depth } => {
                // Ring of `count` holes around the Z axis.
                let base = current_mesh.as_ref()
                    .ok_or_else(|| "BoltCircle applied to empty stack".to_string())?;
                if *count < 3 { return Err("BoltCircle count must be >= 3".into()); }
                if *pitch_diameter <= 0.0 || *hole_diameter <= 0.0 || *depth <= 0.0 {
                    return Err("BoltCircle diameters/depth must be positive".into());
                }
                let pitch_r = *pitch_diameter / 2.0;
                let mut result = base.clone();
                for i in 0..*count {
                    let a = std::f64::consts::TAU * i as f64 / *count as f64;
                    let cx = pitch_r * a.cos();
                    let cy = pitch_r * a.sin();
                    let mut cutter = cylinder_mesh(*hole_diameter / 2.0, *depth)?;
                    cutter = transform_mesh(&cutter, &[cx, cy, 0.0], &[0.0, 0.0, 0.0]);
                    result = cut_from(&result, &cutter)?;
                }
                current_mesh = Some(result);
            }

            SolidOp::RectPattern { x_count, y_count, spacing_x, spacing_y, hole_diameter, depth } => {
                let base = current_mesh.as_ref()
                    .ok_or_else(|| "RectPattern applied to empty stack".to_string())?;
                if *x_count < 1 || *y_count < 1 { return Err("RectPattern counts must be >= 1".into()); }
                if *hole_diameter <= 0.0 || *depth <= 0.0 {
                    return Err("RectPattern hole diameter/depth must be positive".into());
                }
                let mut result = base.clone();
                for ix in 0..*x_count {
                    for iy in 0..*y_count {
                        let px = (ix as f64 - (*x_count as f64 - 1.0) / 2.0) * *spacing_x;
                        let py = (iy as f64 - (*y_count as f64 - 1.0) / 2.0) * *spacing_y;
                        let mut cutter = cylinder_mesh(*hole_diameter / 2.0, *depth)?;
                        cutter = transform_mesh(&cutter, &[px, py, 0.0], &[0.0, 0.0, 0.0]);
                        result = cut_from(&result, &cutter)?;
                    }
                }
                current_mesh = Some(result);
            }

            SolidOp::Shell { .. } => {
                return Err("shell not yet supported (Truck 0.6 limitation)".into());
            }
            SolidOp::Fillet { .. } => {
                return Err("fillet not yet supported (Truck 0.6 limitation)".into());
            }
            SolidOp::Chamfer { .. } => {
                return Err("chamfer not yet supported (Truck 0.6 limitation)".into());
            }
        }
    }

    current_mesh.ok_or_else(|| "no mesh was produced by the op stack".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use apro_document::vehicle::*;

    #[test]
    fn test_expand_rectangle_profile() {
        let p = Profile::Rectangle { width: 100.0, height: 50.0, corner_radius: None };
        let pts = expand_profile(&p, 4, &ParamEnv::new()).unwrap();
        assert_eq!(pts.len(), 4);
        assert_eq!(pts[0], [-50.0, -25.0]);
        assert_eq!(pts[2], [50.0, 25.0]);
    }

    #[test]
    fn test_expand_circle_profile() {
        let p = Profile::Circle { radius: 10.0 };
        let pts = expand_profile(&p, 12, &ParamEnv::new()).unwrap();
        assert_eq!(pts.len(), 12);        for &[x, y] in &pts {
            let r = (x * x + y * y).sqrt();
            assert!((r - 10.0).abs() < 1e-6);
        }
    }

    #[test]
    fn test_expand_polygon_profile() {
        let p = Profile::Polygon { sides: 6, circumradius: 20.0 };
        let pts = expand_profile(&p, 0, &ParamEnv::new()).unwrap();
        assert_eq!(pts.len(), 6);
    }

    #[test]
    fn test_expand_points_profile_identity() {
        let pts = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]];
        let p = Profile::Points(pts.clone());
        let result = expand_profile(&p, 0, &ParamEnv::new()).unwrap();
        assert_eq!(result, pts);
    }

    #[test]
    fn test_convert_line_path() {
        let path = Path3D::Line { start: (0.0, 0.0, 0.0), end: (100.0, 0.0, 0.0) };
        let pts = convert_path(&path, 0).unwrap();
        assert_eq!(pts.len(), 2);
    }

    #[test]
    fn test_convert_arc_path() {
        let path = Path3D::Arc {
            center: (0.0, 0.0, 0.0), radius: 50.0,
            start_angle: 0.0, end_angle: std::f64::consts::PI,
        };
        let pts = convert_path(&path, 12).unwrap();
        assert!(pts.len() >= 8);
    }

    #[test]
    fn test_convert_spline_path() {
        let path = Path3D::Spline(vec![(0.0, 0.0, 0.0), (50.0, 0.0, 10.0), (100.0, 0.0, 0.0)]);
        let pts = convert_path(&path, 0).unwrap();
        // Catmull-Rom smooths: densely sampled, passes through every control point.
        assert!(pts.len() > 3, "spline is now smoothed: {}", pts.len());
        assert_eq!(pts.first().unwrap().to_vec(), [0.0, 0.0, 0.0]);
        assert_eq!(pts.last().unwrap().to_vec(), [100.0, 0.0, 0.0]);
        let near_mid = pts.iter().any(|p| (p[0] - 50.0).abs() < 1e-6 && (p[1] - 0.0).abs() < 1e-6);
        assert!(near_mid, "passes through interior control point");
    }

    #[test]
    fn test_convert_helix_path() {
        let path = Path3D::Helix { radius: 10.0, pitch: 5.0, turns: 2.0 };
        let pts = convert_path(&path, 12).unwrap();
        assert!(pts.len() >= 16);
    }

    #[test]
    fn test_hole_cuts_plate() {
        let ops = vec![
            SolidOp::Extrude {
                profile: Profile::Rectangle { width: 60.0, height: 60.0, corner_radius: None },
                height: 10.0, direction: None, taper: None,
            },
            SolidOp::Hole { diameter: 12.0, depth: 10.0, axis: Axis::Z },
        ];
        let mesh = evaluate_solid_ops(&ops).expect("plate with hole evaluates");
        assert!(mesh.positions.len() > 0);
    }

    #[test]
    fn test_hole_requires_base() {
        let ops = vec![SolidOp::Hole { diameter: 12.0, depth: 10.0, axis: Axis::Z }];
        let res = evaluate_solid_ops(&ops);
        assert!(res.is_err(), "Hole on empty stack must error");
    }

    #[test]
    fn test_bolt_circle_rings() {
        let ops = vec![
            SolidOp::Extrude {
                profile: Profile::Rectangle { width: 80.0, height: 80.0, corner_radius: None },
                height: 8.0, direction: None, taper: None,
            },
            SolidOp::BoltCircle { count: 6, pitch_diameter: 60.0, hole_diameter: 8.0, depth: 8.0 },
        ];
        let mesh = evaluate_solid_ops(&ops).expect("bolt circle evaluates");
        assert!(mesh.positions.len() > 0);
        let err = evaluate_solid_ops(&vec![
            SolidOp::Extrude {
                profile: Profile::Rectangle { width: 80.0, height: 80.0, corner_radius: None },
                height: 8.0, direction: None, taper: None,
            },
            SolidOp::BoltCircle { count: 2, pitch_diameter: 60.0, hole_diameter: 8.0, depth: 8.0 },
        ]);
        assert!(err.is_err(), "count < 3 must error");
    }

    #[test]
    fn test_rect_pattern_lightening() {
        let ops = vec![
            SolidOp::Extrude {
                profile: Profile::Rectangle { width: 100.0, height: 100.0, corner_radius: None },
                height: 6.0, direction: None, taper: None,
            },
            SolidOp::RectPattern { x_count: 4, y_count: 4, spacing_x: 20.0, spacing_y: 20.0, hole_diameter: 6.0, depth: 6.0 },
        ];
        let mesh = evaluate_solid_ops(&ops).expect("rectangle pattern evaluates");
        assert!(mesh.positions.len() > 0);
    }

    #[test]
    fn test_evaluate_revolve_full() {
        let ops = vec![
            SolidOp::Revolve {
                profile: Profile::Points(vec![[0.0, 0.0], [100.0, 50.0], [300.0, 54.0]]),
                angle: 360.0,
                axis: Some(Axis::Z),
            },
        ];
        let mesh = evaluate_solid_ops(&ops).expect("eval failed");
        assert!(mesh.positions.len() >= 9);
    }

    #[test]
    fn test_evaluate_extrude_rectangle() {
        let ops = vec![
            SolidOp::Extrude {
                profile: Profile::Rectangle { width: 100.0, height: 50.0, corner_radius: None },
                height: 200.0,
                direction: Some(Direction::PosZ),
                taper: None,
            },
        ];
        let mesh = evaluate_solid_ops(&ops).expect("eval failed");
        assert!(mesh.positions.len() >= 9);
    }

    #[test]
    fn test_evaluate_loft_two_profiles() {
        let ops = vec![
            SolidOp::Loft {
                profiles: vec![
                    Profile::Points(vec![[0.0, 0.0], [50.0, 0.0], [25.0, 40.0]]),
                    Profile::Points(vec![[0.0, 0.0], [80.0, 0.0], [40.0, 60.0]]),
                ],
                guide_curves: None,
            },
        ];
        let mesh = evaluate_solid_ops(&ops).expect("eval failed");
        assert!(mesh.positions.len() >= 9);
    }

    #[test]
    fn test_evaluate_transform_op() {
        let ops = vec![
            SolidOp::Extrude {
                profile: Profile::Rectangle { width: 10.0, height: 10.0, corner_radius: None },
                height: 10.0,
                direction: None,
                taper: None,
            },
            SolidOp::TransformOp {
                translate: Some((5.0, 10.0, 15.0)),
                rotate: None,
                scale: None,
            },
        ];
        let mesh = evaluate_solid_ops(&ops).expect("eval failed");
        // After translate(0,0,15), z values should be >= 15
        let min_z = mesh.positions.iter().skip(2).step_by(3).copied().fold(f32::MAX, f32::min);
        assert!(min_z >= 14.9, "min_z = {min_z}");
    }

    #[test]
    fn test_evaluate_empty_stack_errors() {
        let result = evaluate_solid_ops(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_evaluate_shell_errors() {
        let ops = vec![
            SolidOp::Revolve {
                profile: Profile::Points(vec![[0.0, 0.0], [100.0, 50.0], [300.0, 54.0]]),
                angle: 360.0,
                axis: None,
            },
            SolidOp::Shell {
                thickness: 2.0,
                faces: None,
            },
        ];
        let result = evaluate_solid_ops(&ops);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("shell"));
    }

    #[test]
    fn test_evaluate_boolean_first_op_errors() {
        let ops = vec![
            SolidOp::Boolean {
                kind: BooleanKind::Union,
                target: SolidRef::This,
            },
        ];
        let result = evaluate_solid_ops(&ops);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_lowercase();
        assert!(msg.contains("boolean"), "expected boolean error, got: {msg}");
    }

    #[test]
    fn test_evaluate_boolean_union_works() {
        // Base cube + cutter cube shifted: union must produce a watertight mesh
        // larger than the base alone.
        let base = SolidOp::Extrude {
            profile: Profile::Rectangle { width: 50.0, height: 50.0, corner_radius: None },
            height: 30.0,
            direction: None,
            taper: None,
        };
        let cutter = SolidOp::Extrude {
            profile: Profile::Rectangle { width: 50.0, height: 50.0, corner_radius: None },
            height: 30.0,
            direction: None,
            taper: None,
        };
        let ops = vec![base.clone(), cutter.clone(), SolidOp::Boolean {
            kind: BooleanKind::Union,
            target: SolidRef::This,
        }];
        let result = evaluate_solid_ops(&ops);
        assert!(result.is_ok(), "union eval failed: {:?}", result.err());
        let mesh = result.unwrap();
        assert!(mesh.positions.len() >= 9);
        let max_x = mesh.positions.iter().step_by(3).copied().fold(f32::MIN, f32::max);
        let min_x = mesh.positions.iter().step_by(3).copied().fold(f32::MAX, f32::min);
        assert!((max_x - 25.0).abs() < 1.0, "union x max {}, expected ~25", max_x);
        assert!((min_x + 25.0).abs() < 1.0, "union x min {}, expected ~-25", min_x);
    }

    #[test]
    fn test_evaluate_boolean_difference_this() {
        // Hollow it out: extrude a large block, then boolean-difference an
        // overlapping smaller block placed by a TransformOp.
        let ops = vec![
            SolidOp::Extrude {
                profile: Profile::Rectangle { width: 60.0, height: 60.0, corner_radius: None },
                height: 20.0,
                direction: None,
                taper: None,
            },
            SolidOp::Extrude {
                profile: Profile::Circle { radius: 10.0 },
                height: 20.0,
                direction: None,
                taper: None,
            },
            SolidOp::Boolean {
                kind: BooleanKind::Difference,
                target: SolidRef::This,
            },
        ];
        let result = evaluate_solid_ops(&ops);
        assert!(result.is_ok(), "difference eval failed: {:?}", result.err());
        let mesh = result.unwrap();
        // A hole of r=10 must be open at the top face z=20.
        let center_filled = mesh.positions.chunks(3).any(|p| {
            let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
            (p[2] - 20.0).abs() < 1.0 && r < 9.0
        });
        assert!(!center_filled, "hole must be open at the top face");
    }

    #[test]
    fn test_evaluate_boolean_component_target_unresolvable() {
        let ops = vec![
            SolidOp::Extrude {
                profile: Profile::Rectangle { width: 60.0, height: 60.0, corner_radius: None },
                height: 20.0,
                direction: None,
                taper: None,
            },
            SolidOp::Boolean {
                kind: BooleanKind::Difference,
                target: SolidRef::Component("Other".into()),
            },
        ];
        let result = evaluate_solid_ops(&ops);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Other"));
    }

    #[test]
    fn test_evaluate_fillet_chamfer_error() {
        let ops = vec![SolidOp::Fillet { radius: 5.0, edges: EdgeRef::All }];
        assert!(evaluate_solid_ops(&ops).unwrap_err().contains("fillet"));

        let ops = vec![SolidOp::Chamfer { distance: 5.0, edges: EdgeRef::All }];
        assert!(evaluate_solid_ops(&ops).unwrap_err().contains("chamfer"));
    }

    #[test]
    fn test_evaluate_circle_profile_revolve() {
        let ops = vec![
            SolidOp::Revolve {
                profile: Profile::Circle { radius: 25.0 },
                angle: 360.0,
                axis: Some(Axis::Z),
            },
        ];
        let mesh = evaluate_solid_ops(&ops).expect("eval failed");
        assert!(mesh.positions.len() >= 9);
    }

    #[test]
    fn test_evaluate_polygon_extrude() {
        let ops = vec![
            SolidOp::Extrude {
                profile: Profile::Polygon { sides: 6, circumradius: 30.0 },
                height: 50.0,
                direction: None,
                taper: None,
            },
        ];
        let mesh = evaluate_solid_ops(&ops).expect("eval failed");
        assert!(mesh.positions.len() >= 9);
    }

    #[test]
    fn mesh_quality_nosecone_revolve() {
        // Verify that B-Rep revolve produces a complete mesh with caps
        let pts: Vec<[f64; 2]> = (0..40).map(|i| {
            let t = i as f64 / 39.0;
            [t * 300.0, 54.0 * (1.0 - t * t)]
        }).collect();
        let ops = vec![SolidOp::Revolve {
            profile: Profile::Points(pts),
            angle: 360.0,
            axis: Some(Axis::Z),
        }];
        let mesh = evaluate_solid_ops(&ops).expect("eval failed");
        // Should have side faces + cap faces → at least some triangles
        assert!(mesh.indices.len() >= 6000, "too few indices: {}", mesh.indices.len());
        // Every index should be in range
        let vert_count = mesh.positions.len() as u32 / 3;
        for &idx in &mesh.indices {
            assert!(idx < vert_count, "index {} out of range (verts {})", idx, vert_count);
        }
        // Positions and normals should match count
        assert_eq!(mesh.positions.len(), mesh.normals.len(),
            "positions {} != normals {}", mesh.positions.len(), mesh.normals.len());
        println!("  Revolve mesh: {} verts, {} tris", vert_count, mesh.indices.len() / 3);
    }

    #[test]
    fn test_evaluate_revolve_chain() {
        // Nosecone segment: tip→base
        let nose: Vec<[f64; 2]> = (0..20).map(|i| {
            let t = i as f64 / 19.0;
            [t * 300.0, 50.0 * (1.0 - t * t).max(0.2 / 50.0)]
        }).collect();
        // Bodytube segment: top→bottom (same radius at junction)
        let body: Vec<[f64; 2]> = (0..10).map(|i| {
            let t = i as f64 / 9.0;
            [t * 500.0, 50.0]
        }).collect();
        let ops = vec![SolidOp::RevolveChain {
            segments: vec![nose, body],
            angle: 360.0,
        }];
        let mesh = evaluate_solid_ops(&ops).expect("revolve chain failed");
        assert!(mesh.positions.len() >= 9, "too few positions: {}", mesh.positions.len());
        println!("  RevolveChain: {} verts, {} tris", mesh.positions.len() / 3, mesh.indices.len() / 3);
    }

    #[test]
    fn benchmark_evaluate_time() {
        use std::time::Instant;
        // Small revolve (nosecone-like)
        let pts: Vec<[f64; 2]> = (0..40).map(|i| {
            let t = i as f64 / 39.0;
            [t * 300.0, 54.0 * (1.0 - t * t)]
        }).collect();
        let ops = vec![SolidOp::Revolve {
            profile: Profile::Points(pts.clone()),
            angle: 360.0,
            axis: Some(Axis::Z),
        }];
        let start = Instant::now();
        let m = evaluate_solid_ops(&ops).expect("revolve");
        let t1 = start.elapsed();
        println!("  Revolve 40-pt nosecone: {:?} ({} verts)", t1, m.positions.len());

        // Parabolic profile with 60 pts (curved, avoids collinear edge issue)
        let pts2: Vec<[f64; 2]> = (0..60).map(|i| {
            let t = i as f64 / 59.0;
            [t * 300.0, 54.0 * (1.0 - t * t)]
        }).collect();
        let ops2 = vec![SolidOp::Revolve {
            profile: Profile::Points(pts2),
            angle: 360.0,
            axis: Some(Axis::Z),
        }];
        let start = Instant::now();
        let m2 = evaluate_solid_ops(&ops2).expect("60-pt revolve");
        let t2 = start.elapsed();
        println!("  Revolve 60-pt parabolic: {:?} ({} verts)", t2, m2.positions.len());

        // 80-pt parabolic profile (tests point limit)
        let pts3: Vec<[f64; 2]> = (0..80).map(|i| {
            let t = i as f64 / 79.0;
            [t * 300.0, 54.0 * (1.0 - t * t)]
        }).collect();
        let ops3 = vec![SolidOp::Revolve {
            profile: Profile::Points(pts3),
            angle: 360.0,
            axis: Some(Axis::Z),
        }];
        let start = Instant::now();
        match evaluate_solid_ops(&ops3) {
            Ok(m) => println!("  Revolve 80-pt parabolic: {:?} ({} verts)", start.elapsed(), m.positions.len()),
            Err(e) => println!("  Revolve 80-pt parabolic: FAILED ({})", e),
        }

        // Extrude
        let square = vec![[0.0, 0.0], [100.0, 0.0], [100.0, 50.0], [0.0, 50.0]];
        let ops3 = vec![SolidOp::Extrude {
            profile: Profile::Points(square),
            height: 200.0,
            direction: None,
            taper: None,
        }];
        let start = Instant::now();
        let m3 = evaluate_solid_ops(&ops3).expect("extrude");
        let t3 = start.elapsed();
        println!("  Extrude 4-pt square x200: {:?} ({} verts)", t3, m3.positions.len());

        // Multi-op stack: extrude + transform
        let ops4 = vec![
            SolidOp::Extrude {
                profile: Profile::Circle { radius: 25.0 },
                height: 100.0,
                direction: None,
                taper: None,
            },
            SolidOp::TransformOp {
                translate: Some((50.0, 0.0, 0.0)),
                rotate: Some((0.0, 45.0, 0.0)),
                scale: None,
            },
        ];
        let start = Instant::now();
        let m4 = evaluate_solid_ops(&ops4).expect("stack");
        let t4 = start.elapsed();
        println!("  Extrude+Transform 2-op stack: {:?} ({} verts)", t4, m4.positions.len());

        // All operations below 100ms on dev hardware
        assert!(t1.as_millis() < 5000);
        assert!(t2.as_millis() < 5000);
        assert!(t3.as_millis() < 5000);
        assert!(t4.as_millis() < 5000);
    }
}
