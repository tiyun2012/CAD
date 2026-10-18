//! What a picture of the model shows, independent of how it is drawn: meshes in colours, lines,
//! the background, a section cut and a surface analysis. The GPU draws it with the viewport's
//! pipelines (`solidcraft_gpu::capture`).

use std::sync::Arc;

use solidcraft_geom::{Mesh, Vec3};

use crate::SceneAnalysis;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// The colour as 0..1 per channel.
    pub fn f(self) -> [f64; 3] {
        [self.0 as f64 / 255.0, self.1 as f64 / 255.0, self.2 as f64 / 255.0]
    }
}

pub struct SceneMesh {
    pub mesh: Arc<Mesh>,
    pub color: Rgb,
    /// Faces in colours of their own: (B-rep face index, colour).
    pub face_colors: Vec<(u32, Rgb)>,
}

pub struct SceneLine {
    pub points: Vec<Vec3>,
    pub color: Rgb,
    /// Width in output pixels.
    pub width: f64,
    /// Draw over everything (sketch geometry being edited).
    pub on_top: bool,
}

#[derive(Default)]
pub struct Scene {
    pub meshes: Vec<SceneMesh>,
    pub lines: Vec<SceneLine>,
    /// The ground grid: drawn under everything (the model hides it; it never hides the model).
    pub grid: Vec<SceneLine>,
    /// Background gradient top → bottom.
    pub background: Option<(Rgb, Rgb)>,
    /// Scene radius for clipping planes.
    pub radius: f64,
    /// A section cut: (normal, d); what lies where normal·p > d is cut away, and the inside
    /// shows as a hatched cap.
    pub clip: Option<(Vec3, f64)>,
    /// Quarter section: hide only where both clip half-spaces are positive.
    pub clip_second: Option<(Vec3, f64)>,
    /// The colour the section cap is tinted toward (default: [`SECTION_CAP`]).
    pub cap: Option<Rgb>,
    /// A surface analysis shading the meshes (zebra, draft, curvature map, ...).
    pub analysis: Option<SceneAnalysis>,
}

/// The section cap's default tint, as in the viewport.
pub const SECTION_CAP: Rgb = Rgb(214, 128, 72);

/// Whether a section removes a world point. A second plane removes the shared wedge only.
pub fn section_clipped(p: Vec3, first: Option<(Vec3, f64)>, second: Option<(Vec3, f64)>) -> bool {
    first.is_some_and(|(n, d)| n.dot(p) > d && second.is_none_or(|(n, d)| n.dot(p) > d))
}

/// Visible pieces of a line; a quarter section can leave two disjoint pieces.
pub fn section_segments(p: Vec3, q: Vec3, first: Option<(Vec3, f64)>, second: Option<(Vec3, f64)>) -> Vec<(Vec3, Vec3)> {
    let mut cuts = vec![0.0, 1.0];
    for (n, d) in first.into_iter().chain(second) {
        let den = n.dot(q - p);
        if den.abs() > 1e-12 {
            let t = (d - n.dot(p)) / den;
            if t > 0.0 && t < 1.0 {
                cuts.push(t);
            }
        }
    }
    cuts.sort_by(f64::total_cmp);
    cuts.windows(2)
        .filter_map(|w| {
            let (&a, &b) = (w.first()?, w.get(1)?);
            (!section_clipped(p + (q - p) * ((a + b) * 0.5), first, second)).then_some((p + (q - p) * a, p + (q - p) * b))
        })
        .collect()
}

/// RGBA pixels (`w × h`) to PNG bytes.
pub fn png(w: usize, h: usize, rgba: Vec<u8>) -> Option<Vec<u8>> {
    let img = image::RgbaImage::from_raw(u32::try_from(w).ok()?, u32::try_from(h).ok()?, rgba)?;
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).ok()?;
    Some(buf.into_inner())
}

#[cfg(test)]
mod contribution_tests {
    use super::*;
    #[test]
    fn contribution_quarter_section_removes_one_quadrant_for_every_flip() {
        for sx in [-1.0, 1.0] {
            for sy in [-1.0, 1.0] {
                let a = Some((Vec3::X * sx, 0.0));
                let b = Some((Vec3::Y * sy, 0.0));
                let mut removed = 0;
                for x in [-1.0, 1.0] {
                    for y in [-1.0, 1.0] {
                        let p = Vec3::new(x, y, 0.0);
                        assert_eq!(section_clipped(p, a, b), x * sx > 0.0 && y * sy > 0.0);
                        removed += usize::from(section_clipped(p, a, b));
                    }
                }
                assert_eq!(removed, 1);
            }
        }
    }
    #[test]
    fn contribution_quarter_section_line_can_keep_two_separate_pieces() {
        let (p, q) = (Vec3::new(-2.0, 3.0, 0.0), Vec3::new(3.0, -2.0, 0.0));
        let a = Some((Vec3::X, 0.0));
        let b = Some((Vec3::Y, 0.0));
        let lines = section_segments(p, q, a, b);
        assert_eq!(lines.len(), 2);
        assert!((lines[0].0 - p).len() < 1e-9);
        assert!((lines[0].1 - Vec3::new(0.0, 1.0, 0.0)).len() < 1e-9);
        assert!((lines[1].0 - Vec3::new(1.0, 0.0, 0.0)).len() < 1e-9);
        assert!((lines[1].1 - q).len() < 1e-9);
        assert_eq!(section_segments(p, q, None, None), vec![(p, q)]);
        assert_eq!(section_segments(p, q, a, None).len(), 1);
    }
}
