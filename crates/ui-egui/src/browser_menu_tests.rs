//! A right-click on a browser row opens its menu, also when the press and the release reach the
//! same frame (a trackpad tap, or a click during a slow frame).

use egui::{Event, Modifiers, PointerButton, Pos2, vec2};

use crate::context_menu::Target;
use crate::scenario::Harness;

fn harness() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.ui.show_browser = true;
    h.app.session.run_script(&solidcraft_engine::sample::script()).expect("sample");
    h.frames(3);
    h
}

/// What a menu is about, without the browser's scope of a folder.
fn kind(t: &Target) -> String {
    match t {
        Target::Folder { folder, .. } => format!("folder:{folder}"),
        Target::Body { name } => format!("body:{name}"),
        other => format!("{other:?}"),
    }
}

fn right(at: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos: at, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE }
}

fn right_click(h: &mut Harness, at: Pos2, one_frame: bool) {
    h.frame_with(vec![Event::PointerMoved(at)]);
    if one_frame {
        h.frame_with(vec![right(at, true), right(at, false)]);
    } else {
        h.frame_with(vec![right(at, true)]);
        h.frame_with(vec![right(at, false)]);
    }
    h.frames(4);
}

#[test]
fn a_right_click_on_a_row_opens_its_menu_however_its_events_arrive() {
    let mut h = harness();
    let rows = [("row:Origin", "Origin"), ("row:Bodies", "folder:bodies"), ("row:Plate", "body:Plate"), ("row:Sketches", "folder:sketches")];
    for (row, target) in rows {
        let Some(centre) = crate::scenario::handle_at(row) else { panic!("{row} is not drawn: {:?}", crate::scenario::handles("row:")) };
        // Pointers sit between whole points (trackpads, HiDPI screens).
        for offset in [vec2(0.0, 0.0), vec2(0.25, 0.75), vec2(0.5, 0.5), vec2(0.75, 0.25)] {
            for one_frame in [false, true] {
                crate::context_menu::close(&mut h.app);
                h.frames(2);
                right_click(&mut h, centre + offset, one_frame);
                let how = if one_frame { "in one frame" } else { "in two frames" };
                assert!(h.app.viewport.context_menu.is_some(), "{row} at {offset:?}, press and release {how}: the menu did not stay open");
                assert_eq!(kind(&h.app.menu.target), target, "{row}");
            }
        }
    }
}

#[test]
fn a_click_elsewhere_moves_or_closes_an_open_menu() {
    let mut h = harness();
    let half = vec2(0.5, 0.5);
    let plate = crate::scenario::handle_at("row:Plate").expect("Plate row");
    let origin = crate::scenario::handle_at("row:Origin").expect("Origin row");
    right_click(&mut h, plate + half, true);
    assert!(h.app.viewport.context_menu.is_some());
    assert_eq!(h.app.menu.target, Target::Body { name: "Plate".into() });
    right_click(&mut h, origin + half, true);
    assert!(h.app.viewport.context_menu.is_some(), "a right-click on another row opens that row's menu");
    assert_eq!(h.app.menu.target, Target::Origin);

    let empty = h.app.viewport.rect.expect("viewport").right_bottom() - vec2(80.5, 80.5);
    right_click(&mut h, empty, true);
    assert!(h.app.viewport.context_menu.is_some(), "the viewport's menu opens the same way");
    assert_eq!(h.app.menu.target, Target::Viewport);

    let primary = |pressed| Event::PointerButton { pos: origin, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    h.frame_with(vec![Event::PointerMoved(origin)]);
    h.frame_with(vec![primary(true), primary(false)]);
    h.frames(2);
    assert!(h.app.viewport.context_menu.is_none(), "a click outside the menu closes it");
}
