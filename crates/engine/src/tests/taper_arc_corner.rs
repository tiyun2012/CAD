//! #2754: redefine a joined negative extrusion of a line/arc profile with a taper.
use super::*;

#[test]
fn sharp_arc_corner_taper_survives_join_redefine_and_reopen() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [-25, -10, 0], "length": 50, "width": 40, "height": 5, "body_name": "Base"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Half disc"}));
    run(&mut s, "sketch.line", json!({"points": [[-20, 0], [20, 0]], "closed": false}));
    run(&mut s, "sketch.arc.center_point", json!({"center": [0, 0], "start": [20, 0], "end": [-20, 0]}));
    run(&mut s, "sketch.finish", json!({}));
    let id =
        run(&mut s, "solid.extrude", json!({"sketch": "Half disc", "distance": "10 mm", "direction": "negative", "operation": "join"}))["feature"]
            .as_u64()
            .unwrap();
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature": id, "command": "solid.extrude", "params": {
            "sketch": "Half disc", "profiles": [0], "distance": "10 mm", "convention": "total_distance",
            "direction": "negative", "operation": "join", "taper": "3 deg"
        }}),
    );
    assert_eq!(s.model.state().bodies.len(), 1);
    // Independent integration of a circle of radius 20+d above y=-d.
    let n = 2048;
    let wanted = 10_000.0
        + (0..n)
            .map(|i| {
                let d = 10.0 * 3f64.to_radians().tan() * (i as f64 + 0.5) / n as f64;
                let r = 20.0 + d;
                r * r * (PI / 2.0 + (d / r).asin()) + d * (r * r - d * d).sqrt()
            })
            .sum::<f64>()
            * 10.0
            / n as f64;
    assert!(rel(volume(&mut s), wanted) < 5e-4, "{} vs {wanted}", volume(&mut s));
    let saved = s.doc.to_json();
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
    assert_eq!(reopened.model.state().bodies.len(), 1);
    assert!(rel(volume(&mut reopened), wanted) < 5e-4);
    let before = saved;
    assert!(
        s.execute(
            "timeline.redefine",
            &json!({"feature": id, "command": "solid.extrude", "params": {
                "sketch": "Half disc", "distance": 10, "direction": "negative", "operation": "join", "taper": "-70 deg"
            }})
        )
        .is_err()
    );
    assert_eq!(s.doc.to_json(), before, "a refused taper must preserve the design");
}
