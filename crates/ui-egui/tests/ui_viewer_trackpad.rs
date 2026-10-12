//! Trackpad gestures in the Composition viewer: a two-finger scroll pans, a pinch (and the mouse
//! wheel) zooms around the pointer (egui_kittest, UI logic only).

use effectcraft_engine::Session;
use effectcraft_ui_egui::EffectcraftApp;
use effectcraft_ui_egui::dock::PanelKind;
use egui::{Event, Rect, pos2, vec2};
use egui_kittest::Harness;
use serde_json::json;

fn harness() -> Harness<'static, EffectcraftApp> {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "View", "width": 640, "height": 360, "duration": 4})).unwrap();
    let mut h = Harness::builder().with_size(vec2(1600.0, 1000.0)).build_eframe(|_| EffectcraftApp::new(s));
    h.state_mut().show_panel(PanelKind::Composition);
    h.run_steps(4);
    h
}

fn rect(h: &Harness<'_, EffectcraftApp>, id: &str) -> Rect {
    let e = h.state().auto.find(id).unwrap_or_else(|| panic!("no {id}"));
    Rect::from_min_size(pos2(e.rect[0], e.rect[1]), vec2(e.rect[2], e.rect[3]))
}

fn wheel(h: &mut Harness<'_, EffectcraftApp>, unit: egui::MouseWheelUnit, delta: egui::Vec2, modifiers: egui::Modifiers) {
    h.input_mut().events.push(Event::MouseWheel { unit, delta, modifiers, phase: egui::TouchPhase::Move });
    h.run_steps(30);
}

#[test]
fn trackpad_scroll_pans_the_composition() {
    let mut h = harness();
    let comp = rect(&h, "viewer.comp");
    h.input_mut().events.push(Event::PointerMoved(comp.center()));
    wheel(&mut h, egui::MouseWheelUnit::Point, vec2(80.0, -40.0), Default::default());
    let after = rect(&h, "viewer.comp");
    let moved = after.min - comp.min;
    assert!((moved.x - 80.0).abs() < 1.5 && (moved.y + 40.0).abs() < 1.5, "the scroll pans: {moved:?}");
    assert!((after.width() - comp.width()).abs() < 0.5, "a scroll doesn't zoom: {} → {}", comp.width(), after.width());
}

#[test]
fn pinch_and_mouse_wheel_zoom_around_the_pointer() {
    let mut h = harness();
    let comp = rect(&h, "viewer.comp");
    let at = comp.min + comp.size() * 0.25;
    h.input_mut().events.push(Event::PointerMoved(at));
    h.input_mut().events.push(Event::Zoom(1.5));
    h.run_steps(3);
    let zoomed = rect(&h, "viewer.comp");
    assert!((zoomed.width() / comp.width() - 1.5).abs() < 0.01, "the pinch zooms: {} → {}", comp.width(), zoomed.width());
    let still = zoomed.min + zoomed.size() * 0.25;
    assert!(still.distance(at) < 2.0, "the point under the pointer stays put: {at:?} vs {still:?}");
    // A mouse wheel (it scrolls in lines) zooms too, and so does Alt + a trackpad scroll.
    wheel(&mut h, egui::MouseWheelUnit::Line, vec2(0.0, 3.0), Default::default());
    let wheeled = rect(&h, "viewer.comp");
    assert!(wheeled.width() > zoomed.width() * 1.1, "the wheel zooms in: {} → {}", zoomed.width(), wheeled.width());
    h.input_mut().events.push(Event::ModifiersChanged(egui::Modifiers::ALT));
    wheel(&mut h, egui::MouseWheelUnit::Point, vec2(0.0, -120.0), egui::Modifiers::ALT);
    let out = rect(&h, "viewer.comp");
    assert!(out.width() < wheeled.width() * 0.9, "Alt + scroll zooms out: {} → {}", wheeled.width(), out.width());
}
