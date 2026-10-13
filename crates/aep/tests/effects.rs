//! Effect instances: match names, parameter lists from `parT`/`pard` and stored values, checked
//! against the values the corpus was made with (its MANIFEST).

mod common;

use common::{first_comp, fixture, layer_with_id, open};
use effectcraft_aep::{Effect, ParamKind, ParamValue, Project};

const L: u32 = 0x1b;

/// Runs `f` on the single effect of the solid `L` in a fixture.
fn with_effect<R>(name: &str, f: impl FnOnce(&Effect<'_, '_>) -> R) -> R {
    let bytes = fixture(name);
    let file = open(&bytes);
    let project = Project::new(&file).unwrap();
    let layer = layer_with_id(&first_comp(&project), L);
    let effects = layer.effects();
    assert_eq!(effects.len(), 1, "{name}");
    f(&effects[0])
}

fn kinds(e: &Effect<'_, '_>) -> Vec<(ParamKind, String)> {
    e.params().into_iter().map(|p| (p.kind, p.name)).collect()
}

fn approx(v: Option<ParamValue>, expected: f64) {
    match v {
        Some(ParamValue::Number(n)) => assert!((n - expected).abs() < 1e-9, "{n} != {expected}"),
        other => panic!("{other:?} is not the number {expected}"),
    }
}

#[test]
fn no_effects_without_a_parade_entry() {
    let bytes = fixture("000-base.aep");
    let file = open(&bytes);
    let project = Project::new(&file).unwrap();
    assert!(layer_with_id(&first_comp(&project), L).effects().is_empty());
}

#[test]
fn slider_control_with_a_stored_value() {
    // 071: Slider Control, Slider = 77.
    with_effect("071-fx-slider-value77.aep", |e| {
        assert_eq!((e.match_name(), e.name().as_deref()), ("ADBE Slider Control", Some("Slider Control")));
        assert_eq!(e.declared_param_count(), Some(3));
        assert_eq!(kinds(e), [(ParamKind::Layer, String::new()), (ParamKind::FloatSlider, "Slider".to_string()), (ParamKind::NoData, String::new())]);
        let params = e.params();
        assert_eq!(params[1].match_name, "ADBE Slider Control-0001");
        assert_eq!(params[2].match_name, "ADBE Effect Built In Params");
        assert_eq!(params[1].stored_value(), Some(ParamValue::Number(77.0)));
        assert_eq!(params[1].value(), Some(ParamValue::Number(77.0)));
        // The pard default of the slider is 0 (070 has the untouched control).
        assert_eq!(params[1].pard_default(), Some(ParamValue::Number(0.0)));
        // The input layer parameter has a value record of its own that is not a number.
        assert_eq!(params[0].stored_value(), None);
        assert_eq!(params[0].pard().len(), 148);
    });
}

#[test]
fn color_control_stores_argb_doubles() {
    // 073: Color -> [0.2, 0.6, 1], stored as [255, 51, 153, 255] (alpha first).
    with_effect("073-fx-color-0.2-0.6-1.aep", |e| {
        assert_eq!((e.match_name(), e.name().as_deref()), ("ADBE Color Control", Some("Color Control")));
        assert_eq!(kinds(e), [(ParamKind::Layer, String::new()), (ParamKind::Color, "Color".to_string()), (ParamKind::NoData, String::new())]);
        let color = e.params()[1].value();
        let Some(ParamValue::Argb([a, r, g, b])) = color else { panic!("{color:?}") };
        for (got, want) in [(a, 1.0), (r, 0.2), (g, 0.6), (b, 1.0)] {
            assert!((got - want).abs() < 1e-6, "{color:?}");
        }
    });
}

#[test]
fn dropdown_menu_items_and_default() {
    // 077: Dropdown Menu Control with the default items.
    with_effect("077-fx-dropdown-default.aep", |e| {
        assert_eq!((e.match_name(), e.name().as_deref()), ("Pseudo/@@S1iPXtJFTHK4QaDbgJvLhw", Some("Dropdown Menu Control")));
        assert_eq!(kinds(e), [(ParamKind::Layer, String::new()), (ParamKind::Popup, "Menu".to_string()), (ParamKind::NoData, String::new())]);
        let menu = &e.params()[1];
        assert_eq!(menu.items.as_deref(), Some(&["Item 1".to_string(), "Item 2".to_string(), "Item 3".to_string()][..]));
        assert_eq!(menu.pard_default(), Some(ParamValue::Index(1)));
        // The choice count is the u16 at byte 60 of pard.
        assert_eq!(u16::from_be_bytes([menu.pard()[60], menu.pard()[61]]), 3);
        // No other parameter has items.
        assert!(e.params().iter().filter(|p| p.index != 1).all(|p| p.items.is_none()));
    });
}

#[test]
fn drop_shadow_defaults_come_from_pard() {
    // 080: Drop Shadow at its defaults (opacity 50 % = 127.5 of 255, direction 135, distance 5, softness 0).
    with_effect("080-fx-dropshadow-default.aep", |e| {
        assert_eq!((e.match_name(), e.name().as_deref()), ("ADBE Drop Shadow", Some("Drop Shadow")));
        let k: Vec<_> = e.params().into_iter().map(|p| (p.kind, p.name)).collect();
        let expected = [
            (ParamKind::Layer, ""),
            (ParamKind::Color, "Shadow Color"),
            (ParamKind::FixedSlider, "Opacity"),
            (ParamKind::Angle, "Direction"),
            (ParamKind::FixedSlider, "Distance"),
            (ParamKind::FixedSlider, "Softness"),
            (ParamKind::Checkbox, "Shadow Only"),
            (ParamKind::NoData, ""),
        ];
        assert_eq!(k, expected.map(|(kind, n)| (kind, n.to_string())));
        let p = e.params();
        approx(p[2].value(), 127.5);
        approx(p[3].value(), 135.0);
        approx(p[4].value(), 5.0);
        approx(p[5].value(), 0.0);
        // Checkbox and colour defaults are not decoded.
        assert_eq!(p[6].value(), None);
        assert_eq!(p[1].value(), None);
    });
}
