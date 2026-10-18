use serde::Serialize;
use solidcraft_geom::{Aabb3, Mesh, Vec3};

use crate::Result;
use crate::body::Body;
use crate::topo::{TopoCounts, merged_topology};

/// Measured properties of a body.
#[derive(Clone, Debug, Serialize)]
pub struct BodyMeasure {
    pub volume: f64,
    pub area: f64,
    pub centroid: Vec3,
    pub bbox: Aabb3,
    /// Raw kernel topology counts (closed curves and surfaces may be split in pieces).
    pub faces: usize,
    pub edges: usize,
    pub vertices: usize,
    pub shells: usize,
    /// Counts after merging split pieces of the same analytic surface/curve (comparable with
    /// systems that use periodic faces and edges).
    pub merged: TopoCounts,
}

/// Per-face sums: area, 6 × signed volume and 24 × first moments (divergence theorem), taken
/// about `o` (the moments relative to it), and the sum of the area vectors (zero over a closed
/// surface). Over a closed surface the volume does not depend on `o`; over one that leaves a
/// gap, the gap is closed by a cone to `o`, so `o` near the body keeps that cone small.
fn face_sums(m: &Mesh, nf: usize, o: Vec3) -> Vec<(f64, f64, Vec3, Vec3)> {
    let mut out = vec![(0.0, 0.0, Vec3::ZERO, Vec3::ZERO); nf];
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        let (Some([a, b, c]), Some(slot)) = (m.tri(t), out.get_mut(*f as usize)) else { continue };
        let n = (b - a).cross(c - a) * 0.5;
        let (a, b, c) = (a - o, b - o, c - o);
        let d = a.dot(b.cross(c));
        slot.0 += n.len();
        slot.1 += d;
        slot.2 += (a + b + c) * d;
        slot.3 += n;
    }
    out
}

/// Whether the mesh is closed: every edge, its ends matched by position to within `quantum`,
/// is crossed as often one way as the other.
fn is_closed(m: &Mesh, quantum: f64) -> bool {
    let key = |p: Vec3| [p.x, p.y, p.z].map(|x| (x / quantum).round() as i64);
    let mut net: std::collections::HashMap<([i64; 3], [i64; 3]), i32> = std::collections::HashMap::new();
    for t in &m.triangles {
        let Some([a, b, c]) = m.tri(t) else { return false };
        for (p, q) in [(a, b), (b, c), (c, a)] {
            let (p, q) = (key(p), key(q));
            if p < q {
                *net.entry((p, q)).or_insert(0) += 1;
            } else if q < p {
                *net.entry((q, p)).or_insert(0) -= 1;
            }
        }
    }
    !m.triangles.is_empty() && net.values().all(|n| *n == 0)
}

/// Area, 6 × volume and 24 × first moments of a closed mesh, taken about `o` (near the body,
/// so that a body far from the origin does not cancel its terms in rounding) and returned about
/// the origin.
fn closed_sums(m: &Mesh, o: Vec3) -> (f64, f64, Vec3) {
    let (mut area, mut v6, mut mom) = (0.0, 0.0, Vec3::ZERO);
    for t in &m.triangles {
        let Some([a, b, c]) = m.tri(t) else { continue };
        area += (b - a).cross(c - a).len() * 0.5;
        let (a, b, c) = (a - o, b - o, c - o);
        let d = a.dot(b.cross(c));
        v6 += d;
        mom += (a + b + c) * d;
    }
    (area, v6, o * (4.0 * v6) + mom)
}

/// The sums `(area, 6 × volume, 24 × first moments about the origin, area vector)` of a body
/// `size` across, taken about its box's centre, or those of its display plan (`plan`) when they
/// show a gap and the plan is closed.
///
/// A closed surface's area vectors sum to zero. Where they don't, the meshes leave a gap: the
/// reference mesher can leave a crack where a trimmed face meets a hole cut into it (a notch
/// through a cone). The volume beside the gap is then left out, and the sums close the gap by a
/// cone to the point they are taken about (a crack of 0.3 mm² moved a part ring's volume by 7 %
/// 1000 mm from the origin; about the box's centre, by 0.1 %). When the gap could move the
/// volume by more than 1e-5 of it within the body's size, and the display plan, which shares
/// every edge's nodes between its faces, is closed, it is measured instead; otherwise the sums
/// about the centre stand.
fn gap_closed((area, v6, mom, normal): (f64, f64, Vec3, Vec3), size: f64, plan: impl FnOnce() -> Option<Mesh>) -> (f64, f64, Vec3) {
    if normal.len() * size * 2.0 > 1e-5 * v6.abs()
        && let Some(plan) = plan().filter(|m| is_closed(m, (size * 1e-9).max(1e-12)))
    {
        return closed_sums(&plan, plan.bounds().center());
    }
    (area, v6, mom)
}

/// Smallest radius of curvature along the mesh's edge polylines (straight edges ignored).
fn min_edge_radius(m: &Mesh) -> Option<f64> {
    let mut best: Option<f64> = None;
    for e in &m.edges {
        for w in e.windows(3) {
            let (Some(&a), Some(&b), Some(&c)) = (w.first(), w.get(1), w.get(2)) else { continue };
            let (ab, bc, ca) = (b - a, c - b, a - c);
            let cross = ab.cross(bc).len();
            if !(cross > 1e-12 * ab.len() * bc.len()) {
                continue;
            }
            let r = ab.len() * bc.len() * ca.len() / (2.0 * cross);
            if r.is_finite() && r > 0.0 && best.is_none_or(|x| r < x) {
                best = Some(r);
            }
        }
    }
    best
}

/// Measure a body from a fine tessellation (chord tolerance 5e-5 of the body size, finer for
/// small radii down to 5e-6). Faces whose fine tessellation disagrees with a coarse one (a
/// meshing failure) use the coarse result.
pub fn measure(b: &Body) -> Result<BodyMeasure> {
    if let Some((pos, tris)) = b.triangle_mesh() {
        // Exact for a mesh body; topology counts are facets, sides and vertices.
        let m = b.tessellate(1.0)?;
        let mm = m.measure();
        let edges = b.mesh.as_ref().map(|m| m.edge_count()).unwrap_or(0);
        let merged =
            TopoCounts { faces: tris.len(), edges, vertices: pos.len(), face_types: [("mesh".to_string(), tris.len())].into_iter().collect() };
        return Ok(BodyMeasure {
            volume: mm.volume,
            area: mm.area,
            centroid: mm.centroid,
            bbox: m.bounds(),
            faces: tris.len(),
            edges,
            vertices: pos.len(),
            shells: 1,
            merged,
        });
    }
    let size = b.size();
    let coarse = b.tessellate_with((size * 1e-3).max(1e-3), true)?;
    // Chord error relative to a radius sets the volume error: keep it near 3e-4 of the
    // smallest radius (thin tubes, small fillets).
    let tight = min_edge_radius(&coarse).map(|r| r * 3e-4).unwrap_or(f64::INFINITY);
    let fine_tol = (size * 5e-5).min(tight).max(size * 5e-6).max(1e-4);
    let fine = b.tessellate_with(fine_tol, true)?;
    let medium = b.tessellate_with((fine_tol * 5.0).max(2e-4), true)?;
    let nf = b.face_count();
    // About the box's centre: see `face_sums`.
    let o = Some(fine.bounds().center()).filter(|c| c.is_finite()).unwrap_or(Vec3::ZERO);
    let (sf, sm, sc) = (face_sums(&fine, nf, o), face_sums(&medium, nf, o), face_sums(&coarse, nf, o));
    let close = |x: f64, y: f64| (x - y).abs() <= 0.01 * x.abs().max(y.abs()) + 1e-9;
    let (mut area, mut v6, mut mom, mut normal) = (0.0, 0.0, Vec3::ZERO, Vec3::ZERO);
    for ((f, m), c) in sf.iter().zip(&sm).zip(&sc) {
        // Trust the finest tessellation that agrees with the next coarser one.
        let pick = if close(f.0, m.0) {
            f
        } else if close(m.0, c.0) {
            m
        } else {
            f
        };
        area += pick.0;
        v6 += pick.1;
        mom += pick.2;
        normal += pick.3;
    }
    (area, v6, mom) = gap_closed((area, v6, o * (4.0 * v6) + mom, normal), size, || b.display_plan(fine_tol).ok().map(|p| p.evaluate()));
    let volume = v6 / 6.0;
    let centroid = if v6.abs() > 1e-12 { mom / (4.0 * v6) } else { fine.bounds().center() };
    Ok(BodyMeasure {
        volume,
        area,
        centroid,
        bbox: coarse.bounds().union(&fine.bounds()),
        faces: b.face_count(),
        edges: b.edge_count(),
        vertices: b.vertex_count(),
        shells: b.shell_count(),
        merged: merged_topology(b, &b.tessellate((size * 1e-3).max(1e-3))?)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 10 mm cube at `at`, each side in n × n squares of two triangles, one face per side;
    /// without its first `open` triangles.
    fn cube(at: Vec3, n: usize, open: usize) -> Mesh {
        let mut m = Mesh::default();
        let s = 10.0;
        for axis in 0..3 {
            for side in [0.0, 1.0] {
                for i in 0..n {
                    for j in 0..n {
                        let corner = |a: usize, b: usize| {
                            let mut c = [0.0; 3];
                            c[axis] = side * s;
                            c[(axis + 1) % 3] = a as f64 * s / n as f64;
                            c[(axis + 2) % 3] = b as f64 * s / n as f64;
                            at + Vec3::new(c[0], c[1], c[2])
                        };
                        let k = m.positions.len() as u32;
                        m.positions.extend([corner(i, j), corner(i + 1, j), corner(i + 1, j + 1), corner(i, j + 1)]);
                        m.triangles.extend(if side > 0.0 { [[k, k + 1, k + 2], [k, k + 2, k + 3]] } else { [[k, k + 2, k + 1], [k, k + 3, k + 2]] });
                        m.tri_face.extend([(2 * axis) as u32 + side as u32; 2]);
                    }
                }
            }
        }
        m.triangles.drain(..open);
        m.tri_face.drain(..open);
        m
    }

    /// The summed per-face sums of `m` about `o`, with the moments about the origin (as
    /// `measure` passes them on).
    fn sums(m: &Mesh, o: Vec3) -> (f64, f64, Vec3, Vec3) {
        let (area, v6, mom, normal) =
            face_sums(m, 6, o).iter().fold((0.0, 0.0, Vec3::ZERO, Vec3::ZERO), |s, f| (s.0 + f.0, s.1 + f.1, s.2 + f.2, s.3 + f.3));
        (area, v6, o * (4.0 * v6) + mom, normal)
    }

    /// A cube 1000 mm from the origin with one 0.5 mm² facet left out: the gap is seen, a closed
    /// display plan is measured instead, and without one the sums about the box's centre stand,
    /// which miss a cone of that facet to the centre (0.08 %), where about the origin they are
    /// 17 % over.
    #[test]
    fn a_mesh_left_open_is_measured_on_a_closed_plan_or_about_its_centre() {
        let at = Vec3::new(1000.0, 1000.0, 1000.0);
        let (closed, open) = (cube(at, 10, 0), cube(at, 10, 1));
        let quantum = 1e-9;
        assert!(is_closed(&closed, quantum) && !is_closed(&open, quantum));
        assert!(!is_closed(&Mesh::default(), quantum));
        let centre = at + Vec3::new(5.0, 5.0, 5.0);
        // About the origin and about the centre, the closed cube measures the same.
        for o in [Vec3::ZERO, centre] {
            let (area, v6, mom, normal) = sums(&closed, o);
            assert!((area - 600.0).abs() < 1e-9 && (v6 / 6.0 - 1000.0).abs() < 1e-6 && normal.len() < 1e-9);
            assert!((mom / (4.0 * v6) - centre).len() < 1e-6 * at.len(), "{o:?}");
        }
        // Left open: about the origin the volume is far off, about the centre close.
        let (_, v6_origin, _, normal) = sums(&open, Vec3::ZERO);
        assert!((normal.len() - 0.5).abs() < 1e-9);
        // The facet faces the origin: leaving it out adds its cone to the origin, 167 mm³.
        assert!((v6_origin / 6.0 - (1000.0 + 0.5 * 1000.0 / 3.0)).abs() < 1e-3, "{}", v6_origin / 6.0);
        let about_centre = sums(&open, centre);
        let off = 1000.0 - 0.5 * 5.0 / 3.0;
        assert!((about_centre.1 / 6.0 - off).abs() < 1e-6, "{}", about_centre.1 / 6.0);
        let size = 10.0 * 3f64.sqrt();
        // A closed plan is measured; an open plan, or none, leaves the sums about the centre.
        let (a, v, _) = gap_closed(about_centre, size, || Some(closed.clone()));
        assert!((a - 600.0).abs() < 1e-9 && (v / 6.0 - 1000.0).abs() < 1e-6);
        for plan in [Some(open.clone()), None] {
            let (_, v, m) = gap_closed(about_centre, size, || plan);
            assert!((v / 6.0 - off).abs() < 1e-6 && (v / 6.0 - 1000.0).abs() < 1e-3 * 1000.0);
            assert!((m / (4.0 * v) - centre).len() < 0.01, "{:?}", m / (4.0 * v));
        }
        // No gap: the plan is not asked for.
        let (_, v, _) = gap_closed(sums(&closed, centre), size, || panic!("no gap, no plan"));
        assert!((v / 6.0 - 1000.0).abs() < 1e-6);
    }
}
