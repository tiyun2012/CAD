//! Exact tapered walls at line/arc corners (#2754). Horizontal sections intersect moved
//! lines and circles; their corners trace conics where a plane meets a cone. Projecting a
//! rational circle from the cone apex onto the neighbouring plane gives that conic exactly.
//! The walls keep their planar/conical supports and share the same curved boundary edge.

use super::*;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

fn closes() -> KernelError {
    KernelError::Invalid("the taper closes the profile: a corner or edge disappears; try a smaller taper or distance".into())
}

fn unsupported() -> KernelError {
    KernelError::Failed("not supported yet: tapered corners between two arcs or between an arc and a free curve".into())
}

fn tangent(a: &Seg2, b: &Seg2) -> bool {
    let (u, v) = (a.end_tangent(), b.start_tangent());
    u.cross(v).abs() < 1e-6 && u.dot(v) > 0.0
}

/// Keep all-arc loops on their independent offset path; this builder is for line/arc corners.
pub(super) fn needed(region: &NumberedRegion) -> bool {
    region.loops().any(|l| {
        l.lp.segs
            .iter()
            .zip(l.lp.segs.iter().cycle().skip(1))
            .any(|(a, b)| matches!((a, b), (Seg2::Line { .. }, Seg2::Arc { .. }) | (Seg2::Arc { .. }, Seg2::Line { .. })) && !tangent(a, b))
    })
}

fn right(t: Vec2) -> Vec2 {
    Vec2::new(t.y, -t.x)
}

fn moved(s: Seg2, d: f64) -> Result<Seg2> {
    match s {
        Seg2::Line { a, b } => {
            let t = (b - a).normalized().ok_or_else(closes)?;
            Ok(Seg2::Line { a: a + right(t) * d, b: b + right(t) * d })
        }
        Seg2::Arc { center, radius, start, sweep } => {
            let radius = radius + sweep.signum() * d;
            if radius <= 1e-6 {
                return Err(closes());
            }
            Ok(Seg2::Arc { center, radius, start, sweep })
        }
        _ => Err(unsupported()),
    }
}

/// The intersection on the same branch as the original corner. Tangent neighbours keep
/// their common ruling; otherwise lines and circles are intersected analytically.
fn corner(a: Seg2, b: Seg2, d: f64) -> Result<Vec2> {
    let (p, q) = (moved(a, d)?, moved(b, d)?);
    if tangent(&a, &b) {
        return Ok(if matches!(p, Seg2::Arc { .. }) { p.end() } else { q.start() });
    }
    match (p, q) {
        (Seg2::Line { .. }, Seg2::Line { .. }) => {
            let (u, v) = (a.end_tangent(), b.start_tangent());
            if u.cross(v).abs() < 1e-9 {
                return Err(closes());
            }
            Ok(mitre(a.end(), u, b.start(), v, d))
        }
        (line @ Seg2::Line { .. }, arc @ Seg2::Arc { .. }) | (arc @ Seg2::Arc { .. }, line @ Seg2::Line { .. }) => {
            let Seg2::Arc { center, radius, .. } = arc else {
                return Err(unsupported());
            };
            let u = line.start_tangent();
            let foot = line.start() + u * (center - line.start()).dot(u);
            let height2 = radius * radius - foot.dist(center).powi(2);
            if height2 <= 1e-12 * radius * radius {
                return Err(closes());
            }
            let (x, y) = (foot + u * height2.sqrt(), foot - u * height2.sqrt());
            Ok(if x.dist(a.end()) <= y.dist(a.end()) { x } else { y })
        }
        _ => Err(unsupported()),
    }
}

/// An angle unwrapped near `reference` (so crossing ±π does not reverse an arc).
fn near_angle(p: Vec2, center: Vec2, reference: f64) -> f64 {
    let v = p - center;
    reference + (v.y.atan2(v.x) - reference + PI).rem_euclid(TAU) - PI
}

fn section(lp: &Loop2, d: f64) -> Result<Loop2> {
    let n = lp.segs.len();
    let starts: Vec<Vec2> = (0..n)
        .map(|i| {
            let a = lp.segs.get((i + n - 1) % n).copied().ok_or_else(closes)?;
            let b = lp.segs.get(i).copied().ok_or_else(closes)?;
            corner(a, b, d)
        })
        .collect::<Result<_>>()?;
    let segs = lp
        .segs
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let (a, b) = (*starts.get(i).ok_or_else(closes)?, *starts.get((i + 1) % n).ok_or_else(closes)?);
            match moved(*s, d)? {
                Seg2::Line { .. } => {
                    if (b - a).dot(s.end() - s.start()) <= 1e-12 {
                        return Err(closes());
                    }
                    Ok(Seg2::Line { a, b })
                }
                Seg2::Arc { center, radius, start, sweep } => {
                    let lo = near_angle(a, center, start);
                    let hi = near_angle(b, center, start + sweep);
                    if (hi - lo) * sweep <= 1e-10 || (hi - lo).abs() >= PI {
                        return Err(closes());
                    }
                    Ok(Seg2::Arc { center, radius, start: lo, sweep: hi - lo })
                }
                _ => Err(unsupported()),
            }
        })
        .collect::<Result<Vec<_>>>()?;
    let out = Loop2 { segs };
    if !out.is_closed(1e-6) || out.signed_area() * lp.signed_area() <= 0.0 {
        return Err(closes());
    }
    Ok(out)
}

/// Split arcs before any sections are made: both ends and every support keep the same
/// correspondence, even when an offset changes an arc's span across a semicircle.
fn split(lp: &NumberedLoop) -> Result<NumberedLoop> {
    let mut out = NumberedLoop { lp: Loop2 { segs: vec![] }, from: vec![] };
    for (i, s) in lp.lp.segs.iter().enumerate() {
        let pieces = match s {
            Seg2::Arc { sweep, .. } if sweep.abs() <= TAU + 1e-9 => (sweep.abs() / FRAC_PI_2).ceil().max(1.0) as usize,
            Seg2::Line { .. } => 1,
            _ => return Err(unsupported()),
        };
        if out.lp.segs.len() + pieces > MAX_SEGS {
            return Err(KernelError::TooLarge("too many tapered profile edges".into()));
        }
        for j in 0..pieces {
            let piece = match *s {
                Seg2::Arc { center, radius, start, sweep } => {
                    Seg2::Arc { center, radius, start: start + sweep * j as f64 / pieces as f64, sweep: sweep / pieces as f64 }
                }
                s => s,
            };
            out.lp.segs.push(piece);
            out.from.push(lp.numbers(i).to_vec());
        }
    }
    Ok(out)
}

/// A shared corner edge between two sections. A sharp line/arc corner traces the projective
/// image of a circular arc on the cone. Mapping homogeneous controls preserves the conic;
/// interpolation between section corners would cut through the cone instead.
fn seam(plane: &Plane, a: Seg2, b: Seg2, d0: f64, d1: f64, height: f64, v0: &mt::Vertex, v1: &mt::Vertex) -> Result<mt::Edge> {
    let slope = (d1 - d0) / height;
    if tangent(&a, &b) || slope.abs() < 1e-12 || matches!((a, b), (Seg2::Line { .. }, Seg2::Line { .. })) {
        return Ok(builder::line(v0, v1));
    }
    let (arc, line) = match (a, b) {
        (arc @ Seg2::Arc { .. }, line @ Seg2::Line { .. }) | (line @ Seg2::Line { .. }, arc @ Seg2::Arc { .. }) => {
            (moved(arc, d0)?, moved(line, d0)?)
        }
        _ => return Err(unsupported()),
    };
    let Seg2::Arc { center, radius, sweep, .. } = arc else {
        return Err(unsupported());
    };
    let (q0, q1) = (plane.to_local(from_p3(v0.point())), plane.to_local(from_p3(v1.point())));
    let theta = near_angle(q0, center, 0.0);
    let turn = near_angle(q1, center, theta) - theta;
    if turn.abs() < 1e-10 {
        return Ok(builder::line(v0, v1));
    }
    let reference = Seg2::Arc { center, radius, start: theta, sweep: turn };
    let source =
        seg_edges(plane, &reference, &builder::vertex(p3(plane.to_world(reference.start()))), &builder::vertex(p3(plane.to_world(reference.end()))))?;
    let [source] = &source[..] else {
        return Err(closes());
    };
    let curve = source.oriented_curve().lift_up();
    let apex = plane.to_world(center) - plane.normal() * (radius / (sweep.signum() * slope));
    let normal = plane.dir_to_world(right(line.start_tangent())) - plane.normal() * slope;
    let k = normal.dot(plane.to_world(line.start()) - apex);
    let controls: Vec<mt::Vector4> = curve
        .control_points()
        .iter()
        .map(|h| {
            let delta = Vec3::new(h.x, h.y, h.z) - apex * h.w;
            let den = normal.dot(delta);
            let p = apex * den + delta * k;
            mt::Vector4::new(p.x, p.y, p.z, den)
        })
        .collect();
    // The projective image must stay on one finite branch.
    let weight = controls.first().map_or(0.0, |h| h.w);
    if !weight.is_finite() || weight.abs() < 1e-12 || controls.iter().any(|h| !h.w.is_finite() || h.w * weight <= 0.0) {
        return Err(closes());
    }
    let controls: Vec<mt::Vector4> = controls.into_iter().map(|h| h / weight).collect();
    if controls.iter().any(|h| ![h.x, h.y, h.z, h.w].into_iter().all(f64::is_finite)) {
        return Err(closes());
    }
    let curve = mt::Curve::NurbsCurve(mt::NurbsCurve::new(mt::BSplineCurve::try_new(curve.knot_vec().clone(), controls).map_err(|_| closes())?));
    Ok(mt::Edge::new(v0, v1, curve))
}

struct Section {
    plane: Plane,
    wire: mt::Wire,
    edges: Vec<(mt::Edge, Seg2, usize)>,
}

impl Section {
    fn new(plane: Plane, lp: &Loop2) -> Result<Self> {
        let (wire, edges) = wire_sources(&plane, lp)?;
        Ok(Self { plane, wire, edges })
    }
}

fn walls(lp: &NumberedLoop, bottom: &Section, top: &Section, d0: f64, d1: f64) -> Result<Vec<Side>> {
    let (plane, top_plane) = (&bottom.plane, &top.plane);
    let (bottom_edges, top_edges) = (&bottom.edges, &top.edges);
    let height = (top_plane.origin - plane.origin).dot(plane.normal());
    let n = lp.lp.segs.len();
    if bottom_edges.len() != n || top_edges.len() != n {
        return Err(closes());
    }
    let seams = (0..n)
        .map(|i| {
            let a = *lp.lp.segs.get((i + n - 1) % n).ok_or_else(closes)?;
            let b = *lp.lp.segs.get(i).ok_or_else(closes)?;
            seam(plane, a, b, d0, d1, height, bottom_edges.get(i).ok_or_else(closes)?.0.front(), top_edges.get(i).ok_or_else(closes)?.0.front())
        })
        .collect::<Result<Vec<_>>>()?;
    let mut faces = vec![];
    for (i, ((e, lo, _), (t, hi, _))) in bottom_edges.iter().zip(top_edges).enumerate() {
        let wire: mt::Wire =
            vec![e.clone(), seams.get((i + 1) % n).ok_or_else(closes)?.clone(), t.inverse(), seams.get(i).ok_or_else(closes)?.inverse()].into();
        let face = match (*lo, *hi) {
            (Seg2::Line { .. }, Seg2::Line { .. }) => {
                builder::try_attach_plane(&[wire]).map_err(|e| KernelError::Failed(format!("taper wall: {e}")))?
            }
            (Seg2::Arc { center, radius: r0, start: a0, sweep: w0 }, Seg2::Arc { radius: r1, start: a1, sweep: w1, .. }) => {
                // A common angular range gives the exact cone. The boundary is trimmed by
                // the conic seams, rather than ruling arcs with different start angles.
                let (lo, hi) =
                    if w0 > 0.0 { (a0.min(a1) - 0.1, (a0 + w0).max(a1 + w1) + 0.1) } else { (a0.max(a1) + 0.1, (a0 + w0).min(a1 + w1) - 0.1) };
                let arc = |p: &Plane, radius| -> Result<mt::Curve> {
                    let s = Seg2::Arc { center, radius, start: lo, sweep: hi - lo };
                    let edges = seg_edges(p, &s, &builder::vertex(p3(p.to_world(s.start()))), &builder::vertex(p3(p.to_world(s.end()))))?;
                    edges.first().filter(|_| edges.len() == 1).map(mt::Edge::oriented_curve).ok_or_else(closes)
                };
                let surface = mt::BSplineSurface::homotopy(arc(plane, r0)?.lift_up(), arc(top_plane, r1)?.lift_up());
                mt::Face::try_new(vec![wire], mt::Surface::NurbsSurface(mt::NurbsSurface::new(surface)))
                    .map_err(|e| KernelError::Failed(format!("taper wall: {e}")))?
            }
            _ => return Err(unsupported()),
        };
        faces.push((face, Some(record::profile(lp.numbers(i)))));
    }
    Ok(faces)
}

pub(super) fn extrude(plane: &Plane, region: &NumberedRegion, from: f64, to: f64, foot: f64, sign: f64, t: f64) -> Result<Body> {
    let count = region.loops().try_fold(0usize, |n, l| n.checked_add(l.lp.segs.len()));
    if count.is_none_or(|n| n > MAX_SEGS) {
        return Err(KernelError::TooLarge("too many tapered profile edges".into()));
    }
    if region.loops().any(|l| l.lp.segs.is_empty() || !l.lp.is_closed(1e-6)) {
        return Err(KernelError::Invalid("profile loop is not closed".into()));
    }
    let loops = region.loops().map(split).collect::<Result<Vec<_>>>()?;
    let sections = |d| loops.iter().map(|l| section(&l.lp, d)).collect::<Result<Vec<_>>>();
    let (bottoms, tops) = (sections(from * t)?, sections(to * t)?);
    // Check intermediate sections too: a disappearing edge must not reappear on another
    // intersection branch and leave a plausible but self-intersecting solid.
    for j in 0..=16 {
        let at = from + (to - from) * j as f64 / 16.0;
        let sections: Vec<Vec<Option<Seg2>>> = sections(at * t)?.into_iter().map(|l| l.segs.into_iter().map(Some).collect()).collect();
        match offset_crosses(&sections, 1e-5) {
            Some(false) => {}
            Some(true) => return Err(closes()),
            None => return Err(KernelError::TooLarge("too many tapered profile intersections".into())),
        }
    }
    guard("tapered arc corners", || {
        let mut sides = vec![];
        let (mut bottom_wires, mut top_wires) = (vec![], vec![]);
        for ((lp, low), high) in loops.iter().zip(&bottoms).zip(&tops) {
            let bottom = Section::new(plane.offset(from * sign), low)?;
            let top = Section::new(plane.offset(to * sign), high)?;
            if foot > 0.0 {
                let knee = Section::new(plane.offset((from + foot) * sign), low)?;
                sides.extend(walls(lp, &bottom, &knee, from * t, from * t)?);
                sides.extend(walls(lp, &knee, &top, from * t, to * t)?);
            } else {
                sides.extend(walls(lp, &bottom, &top, from * t, to * t)?);
            }
            bottom_wires.push(bottom.wire);
            top_wires.push(top.wire);
        }
        cap_and_close(sides, &bottom_wires, &top_wires, false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mt::{BoundedCurve, ParametricCurve};

    fn half_disc(r: f64) -> Region2 {
        Region2 {
            outer: Loop2 {
                segs: vec![
                    Seg2::Line { a: Vec2::new(-r, 0.0), b: Vec2::new(r, 0.0) },
                    Seg2::Arc { center: Vec2::ZERO, radius: r, start: 0.0, sweep: PI },
                ],
            },
            holes: vec![],
        }
    }

    // Area of the circle R=r+d above y=-d; integrate independent analytic sections.
    fn expected(r: f64, h: f64, taper: f64) -> f64 {
        let n = 2048;
        (0..=n)
            .map(|i| {
                let d = h * taper.tan() * i as f64 / n as f64;
                let big = r + d;
                let area = big * big * (PI / 2.0 + (d / big).asin()) + d * (big * big - d * d).sqrt();
                let w = if i == 0 || i == n {
                    1.0
                } else if i % 2 == 0 {
                    2.0
                } else {
                    4.0
                };
                w * area
            })
            .sum::<f64>()
            * h
            / (3.0 * n as f64)
    }

    #[test]
    fn sharp_arc_corners_keep_exact_conical_walls() {
        for sign in [-1.0, 1.0] {
            for degrees in [-8.0_f64, 3.0, 12.0] {
                let taper = degrees.to_radians();
                let b = crate::extrude_tapered(&Plane::XY, &half_disc(20.0), 10.0, sign, taper).unwrap();
                let m = crate::measure(&b).unwrap();
                let wanted = expected(20.0, 10.0, taper);
                assert!((m.volume - wanted).abs() / wanted < 5e-4, "sign={sign}, taper={degrees}: {} vs {wanted}", m.volume);
                assert!(crate::boolean_record::edges_evaluate(&b.solid));
                // Every seam sample lies on the exact cone and the drafted diameter plane.
                let mut seams = 0;
                for edge in b.solid.edge_iter() {
                    let curve = edge.oriented_curve();
                    let (lo, hi) = curve.range_tuple();
                    let p0 = from_p3(curve.subs(lo));
                    let p1 = from_p3(curve.subs(hi));
                    if (p0.z - p1.z).abs() < 1.0 || p0.x.abs() < 10.0 {
                        continue;
                    }
                    seams += 1;
                    for k in 0..=16 {
                        let p = from_p3(curve.subs(lo + (hi - lo) * k as f64 / 16.0));
                        let d = p.z * sign * taper.tan();
                        assert!((p.y + d).abs() < 1e-6, "plane: {p:?}");
                        assert!(((p.x * p.x + p.y * p.y).sqrt() - (20.0 + d)).abs() < 1e-6, "cone: {p:?}");
                    }
                }
                assert!(seams >= 2);
            }
        }
    }

    #[test]
    fn sharp_arc_corners_keep_holes_and_profile_provenance() {
        let r = Region2 {
            outer: Loop2::polygon(&[Vec2::new(-30.0, -20.0), Vec2::new(30.0, -20.0), Vec2::new(30.0, 30.0), Vec2::new(-30.0, 30.0)]),
            holes: vec![half_disc(20.0).outer],
        };
        let (h, taper) = (10.0_f64, 3_f64.to_radians());
        let b = crate::extrude_tapered(&Plane::XY, &r, h, 1.0, taper).unwrap();
        let t = taper.tan();
        let wanted = 3000.0 * h + 110.0 * t * h * h + 4.0 * t * t * h.powi(3) / 3.0 - expected(20.0, h, -taper);
        let volume = crate::measure(&b).unwrap().volume;
        assert!((volume - wanted).abs() / wanted < 5e-4, "{volume} vs {wanted}");
        let history = b.history().unwrap();
        for segment in 0..6 {
            assert!(
                history.faces.iter().any(|f| f.origins.contains(&crate::history::FaceOrigin::Profile { segment })),
                "missing profile segment {segment}"
            );
        }
    }

    #[test]
    fn sharp_arc_corners_keep_a_straight_foot_and_an_offset_start() {
        let (from, to, foot, taper) = (-1.0_f64, 10.0_f64, 0.2_f64, 3_f64.to_radians());
        let area = |z: f64| {
            let d = z * taper.tan();
            let r = 20.0 + d;
            r * r * (PI / 2.0 + (d / r).asin()) + d * (r * r - d * d).sqrt()
        };
        let n = 2048;
        let integral = (0..n).map(|i| area(from + (to - from) * (i as f64 + 0.5) / n as f64)).sum::<f64>() * (to - from - foot) / n as f64;
        let wanted = area(from) * foot + integral;
        let plane = Plane::new(Vec3::new(71.0, -23.0, 12.0), Vec3::Y, Vec3::Z).unwrap();
        for sign in [-1.0, 1.0] {
            let b = crate::extrude_tapered_between(&plane, &half_disc(20.0), from, to, foot, sign, taper).unwrap();
            let volume = crate::measure(&b).unwrap().volume;
            assert!((volume - wanted).abs() / wanted < 5e-4, "{volume} vs {wanted}");
            assert!(crate::boolean_record::edges_evaluate(&b.solid));
        }
    }

    #[test]
    fn sharp_arc_corners_support_a_quarter_disc() {
        let r = Region2 {
            outer: Loop2 {
                segs: vec![
                    Seg2::Line { a: Vec2::ZERO, b: Vec2::new(20.0, 0.0) },
                    Seg2::Arc { center: Vec2::ZERO, radius: 20.0, start: 0.0, sweep: FRAC_PI_2 },
                    Seg2::Line { a: Vec2::new(0.0, 20.0), b: Vec2::ZERO },
                ],
            },
            holes: vec![],
        };
        for degrees in [-8_f64, 8.0] {
            let taper = degrees.to_radians();
            let n = 2048;
            let wanted = (0..n)
                .map(|i| {
                    let d = 10.0 * taper.tan() * (i as f64 + 0.5) / n as f64;
                    let r = 20.0 + d;
                    r * r * (PI / 4.0 + (d / r).asin()) + d * (r * r - d * d).sqrt() + d * d
                })
                .sum::<f64>()
                * 10.0
                / n as f64;
            let b = crate::extrude_tapered(&Plane::XY, &r, 10.0, 1.0, taper).unwrap();
            let volume = crate::measure(&b).unwrap().volume;
            assert!((volume - wanted).abs() / wanted < 5e-4, "{volume} vs {wanted}");
        }
    }

    #[test]
    fn sharp_arc_corners_support_oblique_chords_and_major_arcs() {
        for y in [-12_f64, 12.0] {
            let theta = (y / 20.0).asin();
            let r = Region2 {
                outer: Loop2 {
                    segs: vec![
                        Seg2::Line { a: Vec2::new(-16.0, y), b: Vec2::new(16.0, y) },
                        Seg2::Arc { center: Vec2::ZERO, radius: 20.0, start: theta, sweep: PI - 2.0 * theta },
                    ],
                },
                holes: vec![],
            };
            for degrees in [-8_f64, 8.0] {
                let taper = degrees.to_radians();
                let n = 2048;
                let wanted = (0..n)
                    .map(|i| {
                        let d = 10.0 * taper.tan() * (i as f64 + 0.5) / n as f64;
                        let r = 20.0 + d;
                        let cut = y - d;
                        r * r * (cut / r).acos() - cut * (r * r - cut * cut).sqrt()
                    })
                    .sum::<f64>()
                    * 10.0
                    / n as f64;
                let b = crate::extrude_tapered(&Plane::XY, &r, 10.0, -1.0, taper).unwrap();
                let volume = crate::measure(&b).unwrap().volume;
                assert!((volume - wanted).abs() / wanted < 5e-4, "y={y}, taper={degrees}: {volume} vs {wanted}");
            }
        }
    }

    #[test]
    fn sharp_arc_corner_refuses_a_disappearing_profile() {
        assert!(crate::extrude_tapered(&Plane::XY, &half_disc(5.0), 10.0, 1.0, -30_f64.to_radians()).is_err());
    }
}
