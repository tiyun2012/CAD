//! A pattern of an extrude and its chamfers: its copies applied at once build what the same
//! features built one by one do, and the copies' chamfer faces are named after the source's.

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// A 130 × 46 × 3 plate.
fn plate() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Plate"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [130, 46]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 3, "body_name": "Plate"}));
    s
}

/// A rounded boss under the plate at `x`, its root and tip chamfered.
fn boss(s: &mut Session, x: f64, k: usize) {
    run(s, "sketch.create", json!({"plane": "XY", "name": format!("Boss{k}")}));
    run(s, "sketch.rectangle.rounded", json!({"p0": [x + 4.0, 4], "p1": [x + 38.0, 38], "radius": 4}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": 4, "direction": "negative", "operation": "join", "name": format!("BossX{k}"), "profiles": "all"}));
    run(s, "solid.chamfer", json!({"edges": [[x + 21.0, 4, 0]], "distance": 1, "name": format!("Root{k}")}));
    run(s, "solid.chamfer", json!({"edges": [[x + 21.0, 4, -4]], "distance": 0.8, "name": format!("Tip{k}")}));
}

fn summary(s: &mut Session) -> (f64, f64, u64, u64) {
    let m = run(s, "inspect.measure", json!({}));
    let b = &m["bodies"][0];
    (b["volume_mm3"].as_f64().unwrap(), b["area_mm2"].as_f64().unwrap(), b["faces"].as_u64().unwrap(), b["edges"].as_u64().unwrap())
}

#[test]
fn a_pattern_of_an_extrude_and_its_chamfers_builds_what_its_copies_one_by_one_do() {
    let mut patterned = plate();
    boss(&mut patterned, 0.0, 0);
    run(
        &mut patterned,
        "solid.pattern.rectangular",
        json!({"features": ["BossX0", "Root0", "Tip0"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 42}),
    );
    let mut by_hand = plate();
    for k in 0..3 {
        boss(&mut by_hand, 42.0 * k as f64, k);
    }
    let (a, b) = (summary(&mut patterned), summary(&mut by_hand));
    assert!((a.0 - b.0).abs() <= 1e-9 * b.0, "volume {} vs {}", a.0, b.0);
    assert!((a.1 - b.1).abs() <= 1e-9 * b.1, "area {} vs {}", a.1, b.1);
    assert_eq!((a.2, a.3), (b.2, b.3), "faces and edges");
}

#[test]
fn copies_chamfer_faces_are_named_after_the_source_chamfers() {
    let mut s = plate();
    boss(&mut s, 0.0, 0);
    run(&mut s, "solid.pattern.rectangular", json!({"features": ["BossX0", "Root0", "Tip0"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 42}));
    let id = |s: &Session, name: &str| s.doc.find_feature(name).unwrap().id;
    let (boss, pattern) = (id(&s, "BossX0"), id(&s, "RectangularPattern1"));
    let faces = run(&mut s, "model.faces", json!({"body": "Plate"}));
    let names: Vec<&str> = faces["faces"].as_array().unwrap().iter().filter_map(|f| f["name"].as_str()).collect();
    assert!(!names.iter().any(|n| n.contains(":face:")), "no face named by position: {names:?}");
    let tip = format!("F{pattern}:blend:F{boss}:bottom@F{pattern}.1|F{boss}:side:a1@F{pattern}.1");
    assert!(names.contains(&tip.as_str()), "{tip} in {names:?}");
}
