//! Console Effect from the keyboard and the pointer (egui_kittest): Ctrl+Space opens it, typing
//! searches, Enter applies to the selected layer, 1–9 apply the shortcuts, Tab jumps to the
//! presets, right-click teaches it favorites, Escape and a click outside close it; the panel and
//! the gallery's keys stay out of the app's shortcuts.

use effectcraft_engine::Session;
use effectcraft_engine::project::LayerId;
use effectcraft_ui_egui::panels::console_effect as fx;
use effectcraft_ui_egui::{Dialog, EffectcraftApp};
use egui::{Event, Key, Modifiers, PointerButton, Pos2, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

/// A comp with a solid (selected) and a text layer.
fn harness() -> (Harness<'static, EffectcraftApp>, u64, u64) {
    let mut s = Session::default();
    s.execute("comp.new", json!({"name": "Main", "width": 320, "height": 180, "duration": 2})).unwrap();
    let text = s.execute("layer.newText", json!({"text": "Title"})).unwrap()["layer"].as_u64().unwrap();
    let solid = s.execute("layer.newSolid", json!({"name": "Plate", "color": "#336699"})).unwrap()["layer"].as_u64().unwrap();
    s.execute("layer.select", json!({"layers": [solid]})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(move |_| EffectcraftApp::new(s));
    h.run_steps(3);
    (h, solid, text)
}

fn effects(h: &Harness<'_, EffectcraftApp>, l: u64) -> Vec<String> {
    let comp = h.state().session.active_comp().unwrap();
    comp.layer(LayerId(l)).unwrap().effects().map(|fx| fx.groups().map(|g| g.match_id.clone()).collect()).unwrap_or_default()
}

fn press(h: &mut Harness<'_, EffectcraftApp>, key: Key, modifiers: Modifiers) {
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers });
        h.step();
    }
    h.run_steps(2);
}

fn type_text(h: &mut Harness<'_, EffectcraftApp>, text: &str) {
    h.input_mut().events.push(Event::Text(text.into()));
    h.run_steps(3);
}

/// Ctrl+Space (Control on macOS, where Cmd+Space is Spotlight's).
fn ctrl_space(h: &mut Harness<'_, EffectcraftApp>) {
    let m = Modifiers { ctrl: true, command: !cfg!(target_os = "macos"), ..Default::default() };
    press(h, Key::Space, m);
}

fn center(h: &Harness<'_, EffectcraftApp>, id: &str) -> Pos2 {
    let e = h.state().auto.find(id).unwrap_or_else(|| panic!("no {id}"));
    pos2(e.rect[0] + e.rect[2] / 2.0, e.rect[1] + e.rect[3] / 2.0)
}

fn click(h: &mut Harness<'_, EffectcraftApp>, p: Pos2, button: PointerButton) {
    h.input_mut().events.push(Event::PointerMoved(p));
    h.step();
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos: p, button, pressed, modifiers: Modifiers::NONE });
        h.step();
    }
    h.run_steps(2);
}

fn console<'a>(h: &'a Harness<'_, EffectcraftApp>) -> &'a fx::ConsoleState {
    fx::state(h.state())
}

fn open(h: &Harness<'_, EffectcraftApp>) -> bool {
    h.state().dialog == Some(Dialog::ConsoleEffect)
}

#[test]
fn ctrl_space_type_and_enter_applies_to_the_selected_layer() {
    let (mut h, solid, _) = harness();
    ctrl_space(&mut h);
    assert!(open(&h), "Ctrl+Space opened the console");
    assert!(h.state().auto.find("consoleEffect.search").is_some());
    // Typing goes to the search field: the app's shortcuts wait (G is the Pen tool, Space a preview).
    type_text(&mut h, "gaussian");
    assert_eq!(console(&h).query, "gaussian");
    assert!(!h.state().playback.playing);
    assert_eq!(h.state().auto.find("consoleEffect.row.0").map(|e| e.label.as_str()), Some("Gaussian Blur"));
    press(&mut h, Key::Enter, Modifiers::NONE);
    assert!(!open(&h), "Enter applied and closed");
    assert_eq!(effects(&h, solid), ["ec.blur.gaussian"]);
    // Shift+Enter applies and stays open; Down picks the next row.
    ctrl_space(&mut h);
    assert!(console(&h).query.is_empty(), "it opens with an empty search");
    type_text(&mut h, "glow");
    press(&mut h, Key::Enter, Modifiers::SHIFT);
    assert!(open(&h), "Shift+Enter keeps it open");
    assert_eq!(effects(&h, solid).len(), 2);
    let first = h.state().auto.find("consoleEffect.row.0").unwrap().label.clone();
    press(&mut h, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(console(&h).sel, 1);
    press(&mut h, Key::ArrowUp, Modifiers::NONE);
    assert_eq!(console(&h).sel, 0);
    assert_eq!(first, "Glow");
    press(&mut h, Key::Escape, Modifiers::NONE);
    assert!(!open(&h), "Escape closed it");
    assert_eq!(effects(&h, solid).len(), 2, "nothing more applied");
}

#[test]
fn digits_apply_shortcuts_and_tab_jumps_to_the_presets() {
    let (mut h, solid, text) = harness();
    h.state_mut().session.execute("consoleEffect.shortcut", json!({"slot": 2, "effect": "CC Page Turn", "label": "Turn"})).unwrap();
    ctrl_space(&mut h);
    // The empty console lists the shortcuts.
    assert_eq!(h.state().auto.find("consoleEffect.row.0").map(|e| e.label.as_str()), Some("CC Page Turn"));
    // A digit with no shortcut types; with one, it applies at once.
    type_text(&mut h, "2");
    assert!(!open(&h), "the shortcut applied and closed the console");
    assert_eq!(effects(&h, solid), ["ec.distort.ccpageturn"]);
    ctrl_space(&mut h);
    type_text(&mut h, "5");
    assert!(open(&h));
    assert_eq!(console(&h).query, "5", "an empty slot's digit is typed");
    press(&mut h, Key::Escape, Modifiers::NONE);
    // Tab: from the effects to the presets (a text preset on the text layer).
    h.state_mut().session.execute("layer.select", json!({"layers": [text]})).unwrap();
    ctrl_space(&mut h);
    type_text(&mut h, "type");
    let rows = h.state().auto.query("consoleEffect.row.");
    assert!(rows.iter().any(|r| r.label == "Typewriter"), "{:?}", rows.iter().map(|r| &r.label).collect::<Vec<_>>());
    press(&mut h, Key::Tab, Modifiers::NONE);
    press(&mut h, Key::Enter, Modifiers::NONE);
    let last = h.state().session.journal.last().cloned().unwrap();
    assert_eq!(last.0, "consoleEffect.apply");
    assert_eq!(last.1["textPreset"], "typewriter", "{last:?}");
}

#[test]
fn right_click_teaches_favorites_and_a_click_outside_closes() {
    let (mut h, _, _) = harness();
    ctrl_space(&mut h);
    type_text(&mut h, "levels");
    let row = center(&h, "consoleEffect.row.0");
    click(&mut h, row, PointerButton::Secondary);
    h.get_by_label("Add to Favorites").click();
    h.run_steps(3);
    let fav = h.state().session.console_effect.favorites.first().map(|f| f.target.clone());
    assert_eq!(fav, Some(effectcraft_engine::console_effect::Target::Effect("ec.color.levels".into())));
    assert!(open(&h), "the menu's choice keeps the console open");
    // Blacklisted: gone from the results.
    fx::set_query(h.state_mut(), "leave color");
    h.run_steps(3);
    assert_eq!(h.state().auto.find("consoleEffect.row.0").map(|e| e.label.as_str()), Some("Leave Color"));
    let row = center(&h, "consoleEffect.row.0");
    click(&mut h, row, PointerButton::Secondary);
    h.get_by_label("Blacklist").click();
    h.run_steps(3);
    assert_eq!(h.state().session.console_effect.blacklist.len(), 1);
    assert_ne!(h.state().auto.find("consoleEffect.row.0").map(|e| e.label.as_str()), Some("Leave Color"));
    // A click outside closes it.
    let window = h.state().auto.find("consoleEffect.window").unwrap().rect;
    click(&mut h, pos2(window[0] + window[2] + 200.0, window[1] + window[3] + 100.0), PointerButton::Primary);
    assert!(!open(&h));
}

#[test]
fn the_panel_opens_the_console_and_applies_shortcuts() {
    let (mut h, solid, _) = harness();
    h.state_mut().session.execute("consoleEffect.shortcut", json!({"slot": 1, "effect": "Glow"})).unwrap();
    h.state_mut().show_panel(effectcraft_ui_egui::dock::PanelKind::ConsoleEffect);
    h.run_steps(3);
    let slot = center(&h, "consoleEffectPanel.slot.1");
    click(&mut h, slot, PointerButton::Primary);
    assert_eq!(effects(&h, solid), ["ec.stylize.glow"]);
    let opener = center(&h, "consoleEffectPanel.open");
    click(&mut h, opener, PointerButton::Primary);
    assert!(open(&h), "the panel's search field opens the console");
}

#[test]
fn gallery_keys_stay_in_the_gallery() {
    let (mut h, solid, _) = harness();
    let dir = std::env::temp_dir().join(format!("ec-ui-console-gallery-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let s = &mut h.state_mut().session;
    s.execute("consoleEffect.setSettings", json!({"galleryFolder": dir.to_string_lossy()})).unwrap();
    for t in [0.0, 1.0] {
        s.execute("time.set", json!({"time": t})).unwrap();
        s.execute("consoleEffect.snapshot", json!({"scale": 0.25})).unwrap();
    }
    s.execute("consoleEffect.openGallery", json!({})).unwrap();
    h.run_steps(6);
    assert!(h.state().console_gallery.open);
    let shots = h.state().auto.query("consoleGallery.shot.").len();
    assert_eq!(shots, 2);
    let win = center(&h, "consoleGallery.window");
    h.input_mut().events.push(Event::PointerMoved(win));
    h.run_steps(2);
    // Click one, Ctrl/Cmd-click another: both side by side.
    let second = center(&h, "consoleGallery.shot.1");
    click(&mut h, second, PointerButton::Primary);
    assert_eq!(h.state().console_gallery.selected.len(), 1);
    let first = center(&h, "consoleGallery.shot.0");
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::COMMAND));
    click(&mut h, first, PointerButton::Primary);
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
    assert_eq!(h.state().console_gallery.selected.len(), 2, "compared");
    assert!(h.state().auto.find("consoleGallery.preview.1").is_some());
    // An armed eyedropper samples the snapshot's colour (the blue solid).
    let fill = h.state_mut().session.execute("effect.apply", json!({"layer": solid, "effect": "Fill"})).unwrap()["effects"][0].as_u64().unwrap();
    let color = {
        let comp = h.state().session.active_comp().unwrap();
        let fx = comp.layer(LayerId(solid)).unwrap().effects().unwrap().groups().find(|g| g.uid == fill).unwrap().clone();
        fx.get("color").unwrap().uid
    };
    h.state_mut().ui.fx_pick = Some(effectcraft_ui_egui::state::FxPick { kind: "color".into(), layer: solid, prop: color, name: "Color".into() });
    h.state_mut().console_gallery.selected.truncate(1);
    h.run_steps(6);
    let shot = center(&h, "consoleGallery.preview.0");
    click(&mut h, shot, PointerButton::Primary);
    assert!(h.state().ui.fx_pick.is_none(), "the pick is done");
    let v = h.state_mut().session.execute("prop.get", json!({"layer": solid, "prop": color})).unwrap();
    let c: Vec<f64> = v["value"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    assert!((c[0] - 0.2).abs() < 0.02 && (c[1] - 0.4).abs() < 0.02 && (c[2] - 0.6).abs() < 0.02, "{c:?}");
    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
    let win = center(&h, "consoleGallery.window");
    h.input_mut().events.push(Event::PointerMoved(win));
    h.run_steps(2);
    // (The last snapshot is selected: Up goes to the newer one, Down comes back.)
    let before = h.state().console_gallery.selected.clone();
    press(&mut h, Key::ArrowUp, Modifiers::NONE);
    assert_ne!(h.state().console_gallery.selected, before, "Up picked the other snapshot");
    press(&mut h, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(h.state().console_gallery.selected, before, "Down came back");
    press(&mut h, Key::Z, Modifiers::NONE);
    assert!(h.state().console_gallery.actual_size, "Z shows 100 %");
    // Delete removes the snapshot, not the selected layer.
    let layers = h.state().session.active_comp().unwrap().layers.len();
    press(&mut h, Key::Delete, Modifiers::NONE);
    h.run_steps(4);
    assert_eq!(h.state().session.active_comp().unwrap().layers.len(), layers);
    assert!(h.state().session.active_comp().unwrap().layer(LayerId(solid)).is_some());
    let left = std::fs::read_dir(&dir).unwrap().count();
    assert_eq!(left, 1, "one snapshot deleted");
    let _ = std::fs::remove_dir_all(&dir);
}
