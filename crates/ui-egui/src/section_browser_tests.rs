//! Section Analysis in the browser (#822): an Analysis folder lists it with an eye to hide and
//! show it, and its menu edits or deletes it.

use egui::pos2;
use serde_json::json;

use crate::context_menu::{self, Target};
use crate::scenario::Harness;

fn harness() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.ui.show_browser = true;
    h.frames(3);
    h
}

fn click(h: &mut Harness, handle: &str) {
    assert!(crate::scenario::handle_at(handle).is_some(), "{handle} is not drawn: {:?}", crate::scenario::handles(""));
    h.step(&json!({"click": {"handle": handle}})).unwrap();
    h.frames(3);
}

fn menu(h: &mut Harness, label: &str) {
    let items = context_menu::items(&h.app, &Target::Section);
    let item = items.iter().find(|i| i.label == label).unwrap_or_else(|| panic!("no {label} in {items:?}")).clone();
    context_menu::run_item(&mut h.app, &item, pos2(0.0, 0.0));
    h.frames(3);
}

#[test]
fn no_analysis_folder_without_a_section() {
    let h = harness();
    assert!(crate::scenario::handle_at("row:Analysis").is_none());
    assert!(crate::scenario::handle_at("row:Section Analysis").is_none());
    drop(h);
}

#[test]
fn the_eye_hides_and_shows_the_section_and_delete_removes_it() {
    let mut h = harness();
    h.app.run("inspect.section", json!({"plane": "XZ", "offset": 15})).unwrap();
    h.frames(3);
    let section = h.app.session.section.expect("a section");
    assert!(crate::scenario::handle_at("row:Section Analysis").is_some(), "the section is listed under Analysis");

    click(&mut h, "eye:Section Analysis");
    assert_eq!(h.app.session.section, None, "the eye turns the section off");
    assert!(crate::scenario::handle_at("row:Section Analysis").is_some(), "a hidden section stays listed");

    click(&mut h, "eye:Section Analysis");
    let back = h.app.session.section.expect("the eye turns it back on");
    assert!((back.0 - section.0).len() < 1e-9 && (back.1 - section.1).len() < 1e-9, "the same plane: {back:?} vs {section:?}");

    // The folder's eye is its own: hiding it turns the section off without changing the
    // section's eye, and hiding the section doesn't change the folder's.
    click(&mut h, "eye:Analysis");
    assert_eq!(h.app.session.section, None);
    assert!(h.app.tree.analysis_hidden && !h.app.tree.section_hidden);
    menu(&mut h, "Show/Hide");
    assert!(h.app.session.section.is_none() && h.app.tree.section_hidden, "the menu hides the section itself");
    assert!(h.app.tree.analysis_hidden, "the section's eye doesn't change the folder's");
    click(&mut h, "eye:Analysis");
    assert_eq!(h.app.session.section, None, "the section was hidden on its own meanwhile");
    assert!(!h.app.tree.analysis_hidden);
    menu(&mut h, "Show/Hide");
    assert!(h.app.session.section.is_some(), "Show/Hide in the menu brings it back");

    menu(&mut h, "Delete");
    assert_eq!(h.app.session.section, None);
    assert!(h.app.tree.hidden_section.is_none());
    assert!(crate::scenario::handle_at("row:Section Analysis").is_none(), "a deleted section leaves the browser");
}

#[test]
fn deleting_a_hidden_section_forgets_it() {
    let mut h = harness();
    h.app.run("inspect.section", json!({"plane": "XY", "offset": 2})).unwrap();
    h.frames(3);
    click(&mut h, "eye:Section Analysis");
    menu(&mut h, "Delete");
    assert!(h.app.session.section.is_none() && h.app.tree.hidden_section.is_none());
    assert!(crate::scenario::handle_at("row:Analysis").is_none());
}

#[test]
fn edit_opens_the_section_dialog() {
    let mut h = harness();
    h.app.run("inspect.section", json!({"plane": "YZ"})).unwrap();
    h.frames(3);
    menu(&mut h, "Edit Section Analysis");
    assert!(h.app.dialog.is_some(), "the Section Analysis dialog opens");
}

#[test]
fn contribution_quarter_section_hide_show_edit_and_delete_retain_both_planes() {
    let mut h = harness();
    h.app.run("inspect.section", json!({"plane":"YZ","at":4,"second":{"plane":"XZ","at":6,"flip":true}})).unwrap();
    let cut = (h.app.session.section, h.app.session.section_second);
    crate::browser::set_section_visible(&mut h.app, false);
    assert!(h.app.session.section.is_none() && h.app.session.section_second.is_none());
    crate::browser::set_section_visible(&mut h.app, true);
    assert_eq!((h.app.session.section, h.app.session.section_second), cut);
    h.app.start("inspect.section");
    let cmds = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    let mut scratch = h.app.session.scratch();
    scratch.execute(&cmds[0].0, &cmds[0].1).unwrap();
    assert_eq!((scratch.section, scratch.section_second), cut);
    crate::dialogs::cancel(&mut h.app);
    assert_eq!((h.app.session.section, h.app.session.section_second), cut);
    crate::browser::set_section_visible(&mut h.app, false);
    menu(&mut h, "Delete");
    assert!(h.app.session.section.is_none() && h.app.session.section_second.is_none());
    assert!(h.app.tree.hidden_section.is_none() && h.app.tree.hidden_section_second.is_none());
}

#[test]
fn contribution_cpu_quarter_section_caps_both_exposed_planes() {
    use solidcraft_engine::{Session, geom::Vec3};
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
    s.execute("inspect.section", &json!({"plane":"YZ","at":5,"second":{"plane":"XZ","at":5,"flip":true}})).unwrap();
    let cam = solidcraft_engine::render::Camera { target: Vec3::new(5.0, 5.0, 5.0), distance: 30.0, ..Default::default() };
    let scene = solidcraft_engine::view::scene(&s, &cam);
    let image = solidcraft_engine::render::render(&scene, &cam, 200, 200);
    let vp = cam.view_proj(1.0, scene.radius);
    for point in [Vec3::new(5.0, 2.5, 5.0), Vec3::new(7.5, 5.0, 5.0)] {
        let q = vp.apply(point);
        let x = ((q[0] / q[3] + 1.0) * 100.0) as usize;
        let y = ((1.0 - q[1] / q[3]) * 100.0) as usize;
        let i = (y * 200 + x) * 4;
        let rgb = &image.rgba[i..i + 3];
        assert!(i < image.rgba.len() && i16::from(rgb[0]) > i16::from(rgb[2]) + 10, "{point:?}: {rgb:?}");
    }
}

#[test]
fn contribution_quarter_section_respects_both_browser_eyes() {
    let mut h = harness();
    h.app.run("inspect.section", json!({"plane":"YZ","at":4,"second":{"plane":"XZ","at":6,"flip":true}})).unwrap();
    h.frames(3);
    let cut = (h.app.session.section, h.app.session.section_second);
    click(&mut h, "eye:Analysis");
    assert!(h.app.session.section.is_none() && h.app.session.section_second.is_none());
    assert_eq!((h.app.tree.hidden_section, h.app.tree.hidden_section_second), cut);
    click(&mut h, "eye:Section Analysis");
    click(&mut h, "eye:Analysis");
    assert!(h.app.session.section.is_none() && h.app.session.section_second.is_none());
    click(&mut h, "eye:Section Analysis");
    assert_eq!((h.app.session.section, h.app.session.section_second), cut);
    assert!(h.app.tree.hidden_section.is_none() && h.app.tree.hidden_section_second.is_none());
    menu(&mut h, "Delete");
    assert!(h.app.session.section.is_none() && h.app.session.section_second.is_none());
}
