use apro_kernel::ops::{revolve, tessellate};

fn analyze(name: &str, profile: &[[f64; 2]]) {
    let solid = revolve(profile, 360.0).expect("revolve");
    let mesh = tessellate(&solid, 0.1);
    let pos = &mesh.positions;
    let idx = &mesh.indices;
    let nv = (pos.len() / 3) as u32;

    // Edge usage map keyed by QUANTIZED POSITIONS (verts are duplicated per face)
    use std::collections::HashMap;
    let q = |v: f32| (v * 1000.0).round() as i64; // 0.001 mm quantization
    let mut edges: HashMap<((i64, i64, i64), (i64, i64, i64)), u32> = HashMap::new();
    let mut zero_area = 0u32;
    let mut nan_verts = 0u32;
    let mut tri_area_sum = 0.0f64;
    let mut tri_area_min = f64::MAX;
    for t in idx.chunks(3) {
        let (a, b, c) = (t[0], t[1], t[2]);
        if a >= nv || b >= nv || c >= nv { continue; }
        let (ax, ay, az) = (pos[a as usize * 3], pos[a as usize * 3 + 1], pos[a as usize * 3 + 2]);
        let (bx, by, bz) = (pos[b as usize * 3], pos[b as usize * 3 + 1], pos[b as usize * 3 + 2]);
        let (cx, cy, cz) = (pos[c as usize * 3], pos[c as usize * 3 + 1], pos[c as usize * 3 + 2]);
        if !(ax.is_finite() && ay.is_finite() && az.is_finite()
            && bx.is_finite() && by.is_finite() && bz.is_finite()
            && cx.is_finite() && cy.is_finite() && cz.is_finite()) {
            nan_verts += 1;
        }
        // area
        let ux = bx - ax; let uy = by - ay; let uz = bz - az;
        let vx = cx - ax; let vy = cy - ay; let vz = cz - az;
        let nx = uy * vz - uz * vy;
        let ny = uz * vx - ux * vz;
        let nz = ux * vy - uy * vx;
        let area = (nx * nx + ny * ny + nz * nz).sqrt() * 0.5;
        tri_area_sum += area as f64;
        if area < tri_area_min as f32 { tri_area_min = area as f64; }
        if area < 1e-9 { zero_area += 1; }
        let ka = (q(ax), q(ay), q(az));
        let kb = (q(bx), q(by), q(bz));
        let kc = (q(cx), q(cy), q(cz));
        let key = |p: (i64, i64, i64), r: (i64, i64, i64)| if p <= r { (p, r) } else { (r, p) };
        for e in [key(ka, kb), key(kb, kc), key(kc, ka)] {
            *edges.entry(e).or_insert(0) += 1;
        }
    }

    let boundary: Vec<((i64, i64, i64), (i64, i64, i64))> =
        edges.iter().filter(|(_, n)| **n == 1).map(|(e, _)| *e).collect();
    println!("=== {} ===", name);
    println!("  verts={} tris={} zero_area_tris={} nan_verts={} avg_area={:.4} min_area={:.6}",
        nv, idx.len() / 3, zero_area, nan_verts, tri_area_sum / (idx.len() as f64 / 3.0), tri_area_min);
    println!("  boundary_edges={} (closed holes require edge pairs)", boundary.len());

    // Where are the boundary edges? Print centroid of each boundary edge.
    let mut by_bucket: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for ((ax, ay, az), (bx, by, bz)) in &boundary {
        let mx = (ax + bx) as f64 / 2000.0; let my = (ay + by) as f64 / 2000.0; let mz = (az + bz) as f64 / 2000.0;
        let r = (mx * mx + my * my).sqrt();
        let bucket = if r < 0.5 {
            format!("axis(center) z={:.1}", mz)
        } else if mz < 0.5 {
            format!("z0 rim r={:.1}", r)
        } else {
            format!("middle r={:.1} z={:.1}", r, mz)
        };
        *by_bucket.entry(bucket).or_insert(0) += 1;
    }
    for (k, v) in by_bucket {
        println!("  hole segment: {} -> {} edges", k, v);
    }
}

fn main() {
    // Body tube (like bodytube_to_ops)
    let body: Vec<[f64; 2]> = vec![[0.0, 50.0], [300.0, 50.0]];
    analyze("BodyTube(300x50)", &body);

    // Nosecone Von Karman (like nosecone_to_ops with tip radius)
    let nose: Vec<[f64; 2]> = (0..=64).map(|i| {
        let t = i as f64 / 64.0;
        let x = t * 200.0;
        let r = if t < 0.05 { 0.2 } else { 50.0 * (1.0 - t * t) };
        [x, r]
    }).collect();
    analyze("NoseCone(200x50)", &nose);

    // Tank (hemispherical dome + cylinder + dome)
    let mut tank: Vec<[f64; 2]> = Vec::new();
    for i in 0..=16 {
        let t = i as f64 / 16.0;
        tank.push([t * 50.0, 50.0 * (1.0 - t * t).sqrt()]); // dome from tip to mid
    }
    for i in 0..=8 {
        let t = i as f64 / 8.0;
        tank.push([50.0 + t * 200.0, 50.0]);
    }
    analyze("Tank(hemi+cyl)", &tank);

    // Nozzle-ish profile
    let nozzle: Vec<[f64; 2]> = vec![
        [0.0, 0.0], [20.0, 1.0], [60.0, 30.0], [120.0, 40.0],
    ];
    analyze("Nozzle", &nozzle);
}