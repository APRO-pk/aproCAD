//! Sketch compilation: turn 2D sketch entities into ordered, closed point
//! loops that the kernel can extrude/revolve/loft.
//!
//! A sketch is authored as a bag of independent entities (lines, rectangles,
//! circles, arcs, splines) in draw order. The kernel, however, wants a single
//! ordered boundary loop. [`compile_sketch`] bridges the two:
//!
//! 1. every entity is tessellated into a polyline (loops stay loops);
//! 2. open polylines are stitched end-to-end into chains (order-independent,
//!    so a rectangle drawn as four separate lines still closes);
//! 3. each resulting closed region is checked for a real area and normalized
//!    to counter-clockwise winding.
//!
//! Multiple disjoint regions are returned as separate loops; callers that can
//! only consume one profile report that as an error rather than guessing.

use apro_document::vehicle::{SketchEntity, SketchParams};

/// Geometric tolerance (in sketch units) for "these two points are the same".
pub const WELD_TOL: f64 = 1e-6;

/// A sketch compiled into usable geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledSketch {
    /// One point loop per closed region, each counter-clockwise.
    pub loops: Vec<Vec<[f64; 2]>>,
    /// Chains that never closed, reported so the UI can explain a failed
    /// profile instead of silently producing nothing.
    pub open_chains: Vec<Vec<[f64; 2]>>,
    /// Entities skipped as degenerate (zero radius, < 2 spline points, ...).
    pub skipped: usize,
}

impl CompiledSketch {
    /// The single closed region, or a human-readable reason there isn't one.
    /// Errors are lowercase with no trailing punctuation (house style).
    pub fn single_profile(&self) -> Result<Vec<[f64; 2]>, String> {
        match self.loops.len() {
            0 => {
                if self.open_chains.is_empty() {
                    Err("sketch has no closed profile".into())
                } else {
                    Err(format!(
                        "sketch profile is not closed ({} open chain{})",
                        self.open_chains.len(),
                        if self.open_chains.len() == 1 { "" } else { "s" }
                    ))
                }
            }
            1 => Ok(self.loops[0].clone()),
            n => Err(format!("sketch has {n} disjoint regions; extrude one region at a time")),
        }
    }
}

fn dist2(a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (a[0] - b[0], a[1] - b[1]);
    dx * dx + dy * dy
}

fn same_point(a: [f64; 2], b: [f64; 2], tol: f64) -> bool {
    dist2(a, b) <= tol * tol
}

/// Sample count for a full circle; arcs and rounded shapes scale from this.
const CIRCLE_SAMPLES: usize = 64;

/// How many samples an arc of `sweep` radians gets.
fn arc_samples(sweep: f64) -> usize {
    let frac = (sweep.abs() / std::f64::consts::TAU).min(1.0);
    ((CIRCLE_SAMPLES as f64 * frac).ceil() as usize).max(2)
}

/// Uniform Catmull-Rom evaluation for a 2D span p1->p2.
/// Mirrors the 3D spline used for sweep paths so both feel the same.
fn catmull_rom_2d(p0: [f64; 2], p1: [f64; 2], p2: [f64; 2], p3: [f64; 2], t: f64) -> [f64; 2] {
    let (t2, t3) = (t * t, t * t * t);
    let mut out = [0.0f64; 2];
    for i in 0..2 {
        out[i] = 0.5
            * ((2.0 * p1[i])
                + (-p0[i] + p2[i]) * t
                + (2.0 * p0[i] - 5.0 * p1[i] + 4.0 * p2[i] - p3[i]) * t2
                + (-p0[i] + 3.0 * p1[i] - 3.0 * p2[i] + p3[i]) * t3);
    }
    out
}

/// Tessellate one entity into a polyline. `closed` is true when the entity is
/// intrinsically closed (circle, closed spline) or when a shape's point list
/// should be treated as a loop (rectangle).
struct Polyline {
    points: Vec<[f64; 2]>,
    closed: bool,
}

fn tessellate(entity: &SketchEntity) -> Option<Polyline> {
    match entity {
        SketchEntity::Line { start, end } => {
            if same_point(*start, *end, WELD_TOL) {
                return None; // zero-length segment contributes nothing
            }
            Some(Polyline { points: vec![*start, *end], closed: false })
        }

        SketchEntity::Rectangle { corner1, corner2 } => {
            let (u0, v0) = (corner1[0], corner1[1]);
            let (u1, v1) = (corner2[0], corner2[1]);
            if (u1 - u0).abs() <= WELD_TOL || (v1 - v0).abs() <= WELD_TOL {
                return None; // collapsed rectangle
            }
            // Closed, so the first corner is not repeated at the end.
            Some(Polyline {
                points: vec![[u0, v0], [u1, v0], [u1, v1], [u0, v1]],
                closed: true,
            })
        }

        SketchEntity::Circle { center, radius } => {
            if *radius <= WELD_TOL {
                return None;
            }
            let n = CIRCLE_SAMPLES;
            let points = (0..n)
                .map(|i| {
                    let a = std::f64::consts::TAU * i as f64 / n as f64;
                    [center[0] + radius * a.cos(), center[1] + radius * a.sin()]
                })
                .collect();
            Some(Polyline { points, closed: true })
        }

        SketchEntity::Arc { center, radius, start_angle, end_angle } => {
            if *radius <= WELD_TOL {
                return None;
            }
            // Normalize so the sweep is the CCW distance from start to end.
            let mut sweep = end_angle - start_angle;
            while sweep <= 0.0 {
                sweep += std::f64::consts::TAU;
            }
            let n = arc_samples(sweep);
            let points = (0..=n)
                .map(|i| {
                    let a = start_angle + sweep * i as f64 / n as f64;
                    [center[0] + radius * a.cos(), center[1] + radius * a.sin()]
                })
                .collect();
            Some(Polyline { points, closed: false })
        }

        SketchEntity::Spline { points, closed } => {
            if points.len() < 2 {
                return None;
            }
            if points.len() == 2 {
                return Some(Polyline { points: points.clone(), closed: false });
            }
            // Catmull-Rom through every control point; duplicate the end
            // control points so the first and last spans are well defined.
            let n = points.len();
            let at = |i: isize| -> [f64; 2] {
                let idx = if *closed {
                    ((i % n as isize) + n as isize) % n as isize
                } else {
                    i.clamp(0, n as isize - 1)
                };
                points[idx as usize]
            };
            let spans = if *closed { n } else { n - 1 };
            let per_span = (CIRCLE_SAMPLES / spans.max(1)).max(6);
            let mut out: Vec<[f64; 2]> = Vec::with_capacity(per_span * spans + 1);
            for s in 0..spans {
                let i = s as isize;
                let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
                for k in 0..per_span {
                    let t = k as f64 / per_span as f64;
                    out.push(catmull_rom_2d(p0, p1, p2, p3, t));
                }
            }
            if !*closed {
                out.push(points[n - 1]);
            }
            Some(Polyline { points: out, closed: *closed })
        }
    }
}

/// Signed area of a closed polygon (positive = counter-clockwise).
pub fn signed_area(loop_pts: &[[f64; 2]]) -> f64 {
    let n = loop_pts.len();
    if n < 3 {
        return 0.0;
    }
    let mut a = 0.0;
    for i in 0..n {
        let p = loop_pts[i];
        let q = loop_pts[(i + 1) % n];
        a += p[0] * q[1] - q[0] * p[1];
    }
    a / 2.0
}

/// Reverse a loop in place (used to normalize winding).
fn reverse(loop_pts: &mut Vec<[f64; 2]>) {
    loop_pts.reverse();
}

/// Compile sketch entities into closed loops.
pub fn compile_sketch(sketch: &SketchParams) -> CompiledSketch {
    compile_entities(&sketch.entities)
}

/// Compile a bare entity list (shared by sketches and one-off previews).
pub fn compile_entities(entities: &[SketchEntity]) -> CompiledSketch {
    let mut loops: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut open: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut skipped = 0usize;

    for entity in entities {
        match tessellate(entity) {
            None => skipped += 1,
            Some(poly) if poly.closed => {
                if poly.points.len() >= 3 && signed_area(&poly.points).abs() > WELD_TOL {
                    let mut pts = poly.points;
                    if signed_area(&pts) < 0.0 {
                        reverse(&mut pts);
                    }
                    loops.push(pts);
                } else {
                    skipped += 1;
                }
            }
            Some(poly) => open.push(poly.points),
        }
    }

    // Stitch open chains end-to-end until nothing more can join. Merging chain
    // against chain (not just segment against chain) is what closes a boundary
    // built from many separate pieces, and it makes the result independent of
    // the order the entities were drawn in.
    let mut chains: Vec<Vec<[f64; 2]>> = open;
    loop {
        let mut merged_any = false;
        'outer: for i in 0..chains.len() {
            for j in (i + 1)..chains.len() {
                match join_chains(&chains[i], &chains[j]) {
                    Some(joined) => {
                        chains[i] = joined;
                        chains.remove(j);
                        merged_any = true;
                        break 'outer;
                    }
                    None => continue,
                }
            }
        }
        if !merged_any {
            break;
        }
    }

    // A chain whose ends meet is a closed loop; everything else stays open.
    let mut open_chains: Vec<Vec<[f64; 2]>> = Vec::new();
    for chain in chains {
        if chain.len() >= 4 && same_point(chain[0], *chain.last().unwrap(), WELD_TOL) {
            let mut pts = chain;
            pts.pop(); // drop the duplicate closing point
            if signed_area(&pts).abs() > WELD_TOL {
                if signed_area(&pts) < 0.0 {
                    reverse(&mut pts);
                }
                loops.push(pts);
                continue;
            }
            open_chains.push(pts);
        } else {
            open_chains.push(chain);
        }
    }

    CompiledSketch { loops, open_chains, skipped }
}

/// Join two open chains if an endpoint of one meets an endpoint of the other.
/// Returns the combined chain, or `None` when they do not touch.
fn join_chains(a: &[[f64; 2]], b: &[[f64; 2]]) -> Option<Vec<[f64; 2]>> {
    let (a_head, a_tail) = (a[0], *a.last().unwrap());
    let (b_head, b_tail) = (b[0], *b.last().unwrap());

    if same_point(a_tail, b_head, WELD_TOL) {
        let mut out = a.to_vec();
        out.extend_from_slice(&b[1..]);
        return Some(out);
    }
    if same_point(a_tail, b_tail, WELD_TOL) {
        let mut out = a.to_vec();
        out.extend(b[..b.len() - 1].iter().rev().copied());
        return Some(out);
    }
    if same_point(a_head, b_tail, WELD_TOL) {
        let mut out = b.to_vec();
        out.extend_from_slice(&a[1..]);
        return Some(out);
    }
    if same_point(a_head, b_head, WELD_TOL) {
        let mut out: Vec<[f64; 2]> = a.iter().rev().copied().collect();
        out.extend_from_slice(&b[1..]);
        return Some(out);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> [f64; 2] { [x, y] }

    fn sketch(entities: Vec<SketchEntity>) -> SketchParams {
        SketchParams { plane: Default::default(), offset: 0.0, entities }
    }

    #[test]
    fn rectangle_entity_closes() {
        let s = sketch(vec![SketchEntity::Rectangle {
            corner1: p(0.0, 0.0),
            corner2: p(10.0, 5.0),
        }]);
        let c = compile_sketch(&s);
        assert_eq!(c.loops.len(), 1);
        assert_eq!(c.loops[0].len(), 4);
        assert!((signed_area(&c.loops[0]) - 50.0).abs() < 1e-9, "CCW area 10x5");
        assert!(c.single_profile().is_ok());
    }

    #[test]
    fn four_separate_lines_stitch_into_a_loop() {
        // Deliberately shuffled, and one segment drawn backwards, to prove
        // stitching is geometric rather than order-dependent.
        let s = sketch(vec![
            SketchEntity::Line { start: p(10.0, 0.0), end: p(10.0, 10.0) },
            SketchEntity::Line { start: p(0.0, 10.0), end: p(0.0, 0.0) },
            SketchEntity::Line { start: p(0.0, 0.0), end: p(10.0, 0.0) },
            SketchEntity::Line { start: p(10.0, 10.0), end: p(0.0, 10.0) },
        ]);
        let c = compile_sketch(&s);
        assert_eq!(c.loops.len(), 1, "expected one stitched loop, got {:?}", c);
        assert!(c.open_chains.is_empty());
        assert!(
            (signed_area(&c.loops[0]).abs() - 100.0).abs() < 1e-6,
            "area should be 10x10"
        );
    }

    #[test]
    fn circle_is_ccw_and_closed() {
        let s = sketch(vec![SketchEntity::Circle { center: p(0.0, 0.0), radius: 5.0 }]);
        let c = compile_sketch(&s);
        assert_eq!(c.loops.len(), 1);
        assert!(c.loops[0].len() >= 12);
        let area = signed_area(&c.loops[0]);
        assert!(area > 0.0, "winding normalized CCW");
        // Polygon area of a regular n-gon inscribed in r=5.
        let n = c.loops[0].len() as f64;
        let expected = 0.5 * n * 25.0 * (std::f64::consts::TAU / n).sin();
        assert!((area - expected).abs() < 1e-6);
    }

    #[test]
    fn arc_endpoints_and_sweep() {
        let s = sketch(vec![SketchEntity::Arc {
            center: p(0.0, 0.0),
            radius: 10.0,
            start_angle: 0.0,
            end_angle: std::f64::consts::FRAC_PI_2,
        }]);
        let c = compile_sketch(&s);
        // An open arc alone is not a closed profile.
        assert!(c.loops.is_empty());
        assert_eq!(c.open_chains.len(), 1);
        let chain = &c.open_chains[0];
        assert!((chain[0][0] - 10.0).abs() < 1e-9 && chain[0][1].abs() < 1e-9);
        let last = chain[chain.len() - 1];
        assert!(last[0].abs() < 1e-9 && (last[1] - 10.0).abs() < 1e-9);
        assert!(c.single_profile().is_err());
    }

    #[test]
    fn arc_plus_line_closes_a_d_shape() {
        // Half circle r=10 centred on the origin, closed by a straight chord.
        let s = sketch(vec![
            SketchEntity::Arc {
                center: p(0.0, 0.0),
                radius: 10.0,
                start_angle: 0.0,
                end_angle: std::f64::consts::PI,
            },
            SketchEntity::Line { start: p(-10.0, 0.0), end: p(10.0, 0.0) },
        ]);
        let c = compile_sketch(&s);
        assert_eq!(c.loops.len(), 1, "arc + chord should close: {:?}", c);
        // Half disc area = pi r^2 / 2.
        let expected = std::f64::consts::PI * 100.0 / 2.0;
        let area = signed_area(&c.loops[0]);
        assert!((area - expected).abs() / expected < 0.01, "area {area} vs {expected}");
    }

    #[test]
    fn open_spline_reports_open_chain() {
        let s = sketch(vec![SketchEntity::Spline {
            points: vec![p(0.0, 0.0), p(5.0, 8.0), p(10.0, 0.0)],
            closed: false,
        }]);
        let c = compile_sketch(&s);
        assert!(c.loops.is_empty());
        assert_eq!(c.open_chains.len(), 1);
        let err = c.single_profile().unwrap_err();
        assert!(err.starts_with("sketch profile is not closed"), "got: {err}");
    }

    #[test]
    fn closed_spline_forms_a_loop() {
        let s = sketch(vec![SketchEntity::Spline {
            points: vec![p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(0.0, 10.0)],
            closed: true,
        }]);
        let c = compile_sketch(&s);
        assert_eq!(c.loops.len(), 1, "closed spline should yield a loop: {:?}", c);
        assert!(signed_area(&c.loops[0]).abs() > 50.0);
    }

    #[test]
    fn degenerate_entities_are_skipped_not_fatal() {
        let s = sketch(vec![
            SketchEntity::Circle { center: p(0.0, 0.0), radius: 0.0 },
            SketchEntity::Line { start: p(1.0, 1.0), end: p(1.0, 1.0) },
            SketchEntity::Rectangle { corner1: p(0.0, 0.0), corner2: p(0.0, 5.0) },
            SketchEntity::Spline { points: vec![p(0.0, 0.0)], closed: false },
        ]);
        let c = compile_sketch(&s);
        assert_eq!(c.loops.len(), 0);
        assert_eq!(c.open_chains.len(), 0);
        assert_eq!(c.skipped, 4);
        assert_eq!(c.single_profile().unwrap_err(), "sketch has no closed profile");
    }

    #[test]
    fn two_disjoint_regions_are_rejected_for_extrude() {
        let s = sketch(vec![
            SketchEntity::Rectangle { corner1: p(0.0, 0.0), corner2: p(5.0, 5.0) },
            SketchEntity::Rectangle { corner1: p(20.0, 0.0), corner2: p(25.0, 5.0) },
        ]);
        let c = compile_sketch(&s);
        assert_eq!(c.loops.len(), 2);
        let err = c.single_profile().unwrap_err();
        assert!(err.contains("2 disjoint regions"), "got: {err}");
    }

    #[test]
    fn clockwise_input_is_normalized_to_ccw() {
        // Same square, wound clockwise.
        let s = sketch(vec![
            SketchEntity::Line { start: p(0.0, 0.0), end: p(0.0, 10.0) },
            SketchEntity::Line { start: p(0.0, 10.0), end: p(10.0, 10.0) },
            SketchEntity::Line { start: p(10.0, 10.0), end: p(10.0, 0.0) },
            SketchEntity::Line { start: p(10.0, 0.0), end: p(0.0, 0.0) },
        ]);
        let c = compile_sketch(&s);
        assert_eq!(c.loops.len(), 1);
        assert!(signed_area(&c.loops[0]) > 0.0, "winding normalized CCW");
    }
}
