//! The Extrude window's targets: with none picked a Join or Cut changes the visible bodies of the
//! active component, stored on the feature; picked ones are stored as picked.
use crate::dialogs::{Kind, TARGETS, apply_commands};
use crate::scenario::Harness;
use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::FeatureKind;

/// A 40 x 40 x 5 plate, a copy of it where it is, and a rim sketched over the plate's top: on
/// the plate's face, or on a plane at its height.
fn plate_and_copy(on_face: bool) -> Harness {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":40,"width":40,"height":5,"body_name":"Plate"})).unwrap();
    h.app.run("solid.move", json!({"bodies":["Plate"],"copy":true})).unwrap();
    let plane = if on_face {
        let st = h.app.session.world_state();
        let faces = st.body("Plate").unwrap().body.faces(0.01).unwrap();
        let top = faces.into_iter().find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99)).unwrap();
        json!({"face":[20,20,5],"body":"Plate","index":top.index})
    } else {
        json!({"origin":[0,0,5],"x_dir":[1,0,0],"y_dir":[0,1,0]})
    };
    h.app.run("sketch.create", json!({"plane":plane,"name":"Rim"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[2,2],"p1":[38,38]})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[4,4],"p1":[36,36]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h
}

fn hide(h: &mut Harness, body: &str) {
    h.app.run("browser.visibility", json!({"items":[body],"visible":false})).unwrap();
}

/// The extrude window on the rim's ring, joining.
fn start_join(h: &mut Harness) {
    let ring = rim_ring(h);
    h.app.session.selection = vec![ring];
    h.app.start("solid.extrude");
    let d = h.app.dialog.as_mut().unwrap();
    if let Kind::Extrude { distance, operation, auto_op, .. } = &mut d.kind {
        *distance = "3 mm".into();
        *operation = 1;
        *auto_op = false;
    }
    h.frames(2);
}

fn rim_ring(h: &Harness) -> Sel {
    let st = h.app.session.world_state();
    let ss = st.sketches.iter().find(|ss| ss.name == "Rim").unwrap();
    let index = ss.profiles.iter().position(|p| !p.region.holes.is_empty()).unwrap();
    Sel::Profile { sketch: ss.feature, index }
}

fn params(h: &Harness) -> Value {
    apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().remove(0).1
}

fn volume(h: &Harness, body: &str) -> f64 {
    solidcraft_engine::doc::kernel::measure(&h.app.session.model.state().body(body).unwrap().body).unwrap().volume
}

const RIM: f64 = (36.0 * 36.0 - 32.0 * 32.0) * 3.0;

/// The reported case: a copy hidden, the join goes into the plate alone and the copy stays.
#[test]
fn a_join_with_a_body_hidden_targets_the_visible_one() {
    let mut h = plate_and_copy(false);
    hide(&mut h, "Plate (2)");
    start_join(&mut h);
    assert_eq!(params(&h)["targets"], json!(["Plate"]));
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog);
    assert!((volume(&h, "Plate") - (8000.0 + RIM)).abs() < 1e-6);
    assert!((volume(&h, "Plate (2)") - 8000.0).abs() < 1e-6);
    // Showing the copy again changes nothing: the targets are the feature's.
    h.app.run("browser.visibility", json!({"items":["Plate (2)"],"visible":true})).unwrap();
    assert_eq!(h.app.session.model.state().bodies.len(), 2);
    let FeatureKind::Extrude { targets, .. } = &h.app.session.doc.features.last().unwrap().kind else { panic!("an extrude") };
    assert_eq!(targets.iter().map(|t| t.to_string()).collect::<Vec<_>>(), ["Plate"]);
}

/// Hiding the other one (the copy shown) targets the copy, even from a sketch on the plate's face.
#[test]
fn a_join_from_a_hidden_bodys_face_targets_the_visible_one() {
    let mut h = plate_and_copy(true);
    hide(&mut h, "Plate");
    start_join(&mut h);
    assert_eq!(params(&h)["targets"], json!(["Plate (2)"]));
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!(volume(&h, "Plate (2)") > 8000.0 + 100.0, "the copy is joined");
    assert!((volume(&h, "Plate") - 8000.0).abs() < 1e-6);
}

/// With every body shown, or with the face's own body shown, the window stores no targets: the
/// command's own default (the component's bodies, or that face's body) is the same.
#[test]
fn nothing_hidden_or_the_face_body_shown_leaves_the_command_default() {
    let mut h = plate_and_copy(false);
    start_join(&mut h);
    assert!(params(&h).get("targets").is_none(), "{}", params(&h));
    let mut h = plate_and_copy(true);
    hide(&mut h, "Plate (2)");
    start_join(&mut h);
    assert!(params(&h).get("targets").is_none(), "{}", params(&h));
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!((volume(&h, "Plate (2)") - 8000.0).abs() < 1e-6, "the sketch's face body is joined alone");
}

/// Targets picked in the viewport are what the join changes; a New Body takes none.
#[test]
fn picked_targets_are_sent_and_kept_when_editing() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":40,"width":40,"height":5,"body_name":"Plate"})).unwrap();
    h.app.run("solid.box", json!({"length":40,"width":40,"height":5,"corner":[0,0,10],"operation":"new","body_name":"Ref"})).unwrap();
    h.app.run("sketch.create", json!({"plane":{"origin":[0,0,5],"x_dir":[1,0,0],"y_dir":[0,1,0]},"name":"Rim"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[2,2],"p1":[38,38]})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[4,4],"p1":[36,36]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    start_join(&mut h);
    let d = h.app.dialog.as_ref().unwrap();
    assert_eq!(d.active, 0, "the profiles stay the active input");
    let index = d.inputs.iter().position(|i| i.label == TARGETS).unwrap();
    assert!(d.inputs[index].items.is_empty());
    // Pick the plate's side into Targets, as a hand does.
    h.call("ui.dialogInput", json!({"index":index}));
    h.call("ui.view", json!({"view":"front"}));
    h.frames(2);
    h.step(&json!({"click":{"world":[20,0,2.5]}})).unwrap();
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[index].items, vec![Sel::Body { name: "Plate".into() }]);
    assert_eq!(params(&h)["targets"], json!(["Plate"]));
    if let Kind::Extrude { operation, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
        *operation = 0;
    }
    assert!(params(&h).get("targets").is_none(), "a new body has no targets");
    if let Kind::Extrude { operation, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
        *operation = 1;
    }
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog);
    let feature = h.app.session.doc.features.last().unwrap().id;
    let before = serde_json::to_value(&h.app.session.doc.feature(feature).unwrap().kind).unwrap();
    // Edit Feature shows them in the input and sends them back unchanged.
    h.app.edit_feature(feature);
    h.frames(2);
    let d = h.app.dialog.as_ref().unwrap();
    let index = d.inputs.iter().position(|i| i.label == TARGETS).unwrap();
    assert_eq!(d.inputs[index].items, vec![Sel::Body { name: "Plate".into() }]);
    let edited = apply_commands(&h.app, d).unwrap();
    assert_eq!(edited[0].1["params"]["targets"], json!(["Plate"]));
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog);
    assert_eq!(serde_json::to_value(&h.app.session.doc.feature(feature).unwrap().kind).unwrap(), before);
}

/// Editing a feature made with no targets keeps it that way, whatever is hidden now.
#[test]
fn editing_keeps_a_feature_without_targets_as_it_is() {
    let mut h = plate_and_copy(false);
    h.app.run("solid.extrude", json!({"sketch":"Rim","profiles":[0],"distance":3,"operation":"join","name":"Lip"})).unwrap();
    let feature = h.app.session.doc.find_feature("Lip").unwrap().id;
    hide(&mut h, "Plate");
    h.app.edit_feature(feature);
    h.frames(2);
    let edited = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert!(edited[0].1["params"].get("targets").is_none(), "{}", edited[0].1);
}

/// A face picked before Extrude is still extruded (Press Pull style), not taken as a target.
#[test]
fn a_preselected_face_stays_the_profile_and_a_preselected_body_is_the_target() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":20,"width":20,"height":10,"body_name":"Base"})).unwrap();
    let st = h.app.session.world_state();
    let base = st.body("Base").unwrap();
    let top = base.body.faces(0.01).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.z > 0.99)).unwrap();
    h.app.session.selection = vec![Sel::Face { body: "Base".into(), index: top.index, point: top.centroid }];
    h.app.start("solid.extrude");
    let d = h.app.dialog.as_ref().unwrap();
    assert!(matches!(d.inputs[0].items.first(), Some(Sel::Face { .. })), "{:?}", d.inputs);
    assert!(d.inputs.iter().find(|i| i.label == TARGETS).unwrap().items.is_empty());
    h.app.dialog = None;
    h.app.session.selection = vec![Sel::Body { name: "Base".into() }];
    h.app.start("solid.extrude");
    let d = h.app.dialog.as_ref().unwrap();
    assert_eq!(d.inputs.iter().find(|i| i.label == TARGETS).unwrap().items, vec![Sel::Body { name: "Base".into() }]);
}
