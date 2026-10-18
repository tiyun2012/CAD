//! A small hole in a face of a large body, divided by a boolean. The boolean's tolerance follows
//! the larger body (a few millimetres on a part a few metres long), so each half circle of the
//! hole's rim was a single chord; the rim's polygon in the face's parameters had no area, and its
//! sign, which tells a hole from a face piece, came out at random. The hole then became a face of
//! its own, inside out, and the face it was cut in was left whole: a box cut round such a hole
//! gave a piece without it, and the shared part of that piece and a pin in the hole came out
//! negative or not at all.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use solidcraft_geom::Vec3;

use crate::{Body, BoolOp, FaceSurface, box_solid, cylinder, face_surface, measure, step_export, step_import, transform};

/// A plate 3 m long and 2 mm thick with ten holes of radius `r` along a diagonal, turned by
/// `tilt` about the X axis and read back from STEP (each rim is then two half circles).
fn plate(r: f64, tilt: f64) -> (Body, Vec<Vec3>) {
    let mut p = box_solid(Vec3::new(-1500.0, -150.0, 0.0), Vec3::new(1500.0, 150.0, 2.0)).unwrap();
    let centres: Vec<Vec3> = (0..10).map(|i| Vec3::new(-1259.3 + 271.3 * i as f64, -120.0 + 27.7 * i as f64, 0.0)).collect();
    for &c in &centres {
        let tool = cylinder(c - Vec3::Z, Vec3::Z, r, 4.0).unwrap();
        p = crate::boolean(&p, &tool, BoolOp::Cut).unwrap().unwrap();
    }
    let shift = Vec3::new(0.0, 300.0, 300.0);
    let p = transform(&p, shift, Vec3::ZERO, Vec3::X, tilt).unwrap();
    let text = step_export(&[&p], "plate").unwrap();
    let p = step_import(&text).unwrap().bodies.remove(0).body;
    let (s, c) = tilt.sin_cos();
    let placed = centres.iter().map(|q| Vec3::new(q.x, q.y * c - q.z * s, q.y * s + q.z * c) + shift).collect();
    (p, placed)
}

#[test]
fn a_box_cut_round_a_small_hole_of_a_long_plate_keeps_the_hole() {
    for (r, tilt) in [(1.8, 0.27), (1.8, 0.7)] {
        let (p, centres) = plate(r, tilt);
        for q in centres {
            let g = Vec3::new(8.0, 8.0, 8.0);
            let cut = box_solid(q - g, q + g).unwrap();
            let piece = crate::boolean(&p, &cut, BoolOp::Intersect).unwrap().unwrap();
            let walls = (0..piece.face_count()).filter(|&i| matches!(face_surface(&piece, i), Some(FaceSurface::Cylinder { .. }))).count();
            // Both faces of the plate in the box keep the hole: an outer loop and the rim.
            let holed = piece.solid.face_iter().filter(|f| f.boundaries().len() == 2).count();
            assert!(walls >= 1 && holed == 2, "r {r}, tilt {tilt}, hole at {q:?}: {walls} walls, {holed} faces with a hole");
            assert!(measure(&piece).unwrap().volume > 0.0, "r {r}, tilt {tilt}, hole at {q:?}");
        }
    }
}
