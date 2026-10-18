//! Extrude targets: the bodies a Join, Cut or Intersect changes.
#![allow(clippy::unwrap_used, clippy::panic)]

use crate::{Session, auto_operation, join_face_body};
use serde_json::{Value, json};

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|error| panic!("{command}: {error}"))
}

fn volume(s: &Session, body: &str) -> f64 {
    solidcraft_kernel::measure(&s.model.state().body(body).unwrap().body).unwrap().volume
}

/// A 40 x 40 x 5 plate, a copy of it where it is (a Move with `copy`), and a rim sketched on
/// the plate's top.
fn plate_and_copy() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 5, "body_name": "Plate"}));
    run(&mut s, "solid.move", json!({"bodies": ["Plate"], "copy": true}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 5, "name": "Rim"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [2, 2], "p1": [38, 38]}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [4, 4], "p1": [36, 36]}));
    run(&mut s, "sketch.finish", json!({}));
    s
}

const RIM: f64 = (36.0 * 36.0 - 32.0 * 32.0) * 3.0;

/// Without targets a Join takes every body of the component it meets, hidden or not: the copy
/// is joined in. Visibility never changes what a command does; named targets keep the copy.
#[test]
fn a_join_without_targets_takes_every_body_it_meets_and_targets_keep_the_rest() {
    let mut s = plate_and_copy();
    run(&mut s, "browser.visibility", json!({"items": ["Plate (2)"], "visible": false}));
    let params = json!({"sketch": "Rim", "profiles": [0], "distance": 3, "operation": "join"});
    let r = run(&mut s, "solid.extrude", params.clone());
    assert_eq!(r["removed_bodies"], json!(["Plate (2)"]), "{r}");
    run(&mut s, "edit.undo", json!({}));
    let mut scoped = params;
    scoped["targets"] = json!(["Plate"]);
    let r = run(&mut s, "solid.extrude", scoped);
    assert_eq!((r["changed_bodies"].clone(), r["removed_bodies"].clone()), (json!(["Plate"]), json!([])), "{r}");
    assert!((volume(&s, "Plate") - (8000.0 + RIM)).abs() < 1e-6);
    assert!((volume(&s, "Plate (2)") - 8000.0).abs() < 1e-6);
    // Saved and reopened, the targets keep the copy apart.
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert_eq!(reopened.model.state().bodies.len(), 2);
}

/// An extrude up to a reference body's face joins only its targets: the reference stays.
#[test]
fn an_extrude_to_a_reference_body_joins_only_its_targets() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 5, "body_name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 5, "corner": [0, 0, 10], "operation": "new", "body_name": "Ref"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 5, "name": "Post"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [10, 10], "p1": [30, 30]}));
    run(&mut s, "sketch.finish", json!({}));
    let r = run(&mut s, "solid.extrude", json!({"sketch": "Post", "to": [20, 20, 10], "operation": "join", "targets": ["Plate"]}));
    assert_eq!(r["removed_bodies"], json!([]), "{r}");
    assert!((volume(&s, "Plate") - 10_000.0).abs() < 1e-6);
    assert!((volume(&s, "Ref") - 8000.0).abs() < 1e-6);
}

/// `operation: auto` probes the targets it is given (else the participants), as the evaluator
/// changes them: a profile starting inside a body that is not a target joins.
#[test]
fn auto_operation_probes_only_the_named_targets() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 5, "body_name": "Plate"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [15, 15, 5], "operation": "new", "body_name": "Block"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 5, "name": "Pad"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [16, 16], "p1": [24, 24]}));
    run(&mut s, "sketch.finish", json!({}));
    let p = |extra: Value| {
        let mut p = json!({"sketch": "Pad", "distance": 3});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        p
    };
    assert_eq!(auto_operation(&s, &p(json!({}))), Some("cut"));
    assert_eq!(auto_operation(&s, &p(json!({"targets": ["Plate"]}))), Some("join"));
    assert_eq!(auto_operation(&s, &p(json!({"targets": ["Block"]}))), Some("cut"));
    assert_eq!(auto_operation(&s, &p(json!({"targets": ["*"]}))), Some("cut"));
    // Targets win over participants, as they do when the extrude is evaluated.
    assert_eq!(auto_operation(&s, &p(json!({"targets": ["Plate"], "participants": ["Block"]}))), Some("join"));
    assert_eq!(auto_operation(&s, &p(json!({"participants": ["Plate"]}))), Some("join"));
}

/// The body a Join defaults to: the picked face's, else the face its sketch lies on.
#[test]
fn join_face_body_is_the_face_or_sketch_face_body() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": "Base"}));
    run(&mut s, "sketch.create", json!({"plane": {"face": [10, 10, 10]}, "name": "OnTop"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 2}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 10, "name": "Free"}));
    run(&mut s, "sketch.circle.center", json!({"center": [5, 5], "radius": 2}));
    run(&mut s, "sketch.finish", json!({}));
    assert_eq!(join_face_body(&s, &json!({"sketch": "OnTop"})).as_deref(), Some("Base"));
    assert_eq!(join_face_body(&s, &json!({"sketch": "Free"})), None);
    assert_eq!(join_face_body(&s, &json!({"face": [10, 10, 10], "face_body": "Base"})).as_deref(), Some("Base"));
    assert_eq!(join_face_body(&s, &json!({"face": [10, 10, 10]})), None);
}
