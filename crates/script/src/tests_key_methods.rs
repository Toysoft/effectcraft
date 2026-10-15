//! Per-key methods (`setInterpolationTypeAtKey`, `setTemporalEaseAtKey`, `removeKey`…) on a
//! property of a comp that is not the active one act on that key, as in After Effects (#1531).
//! Every comp keeps its own key selection (#1037): the method must not run on the selection the
//! property's comp remembers, nor leave its target key selected anywhere.

use std::sync::Arc;

use effectcraft_engine::Session;
use serde_json::json;

use crate::{Outcome, run_code};

fn session() -> Session {
    let mut s = Session { expr: Some(Arc::new(effectcraft_expr::Expressions)), expr_check: Some(effectcraft_expr::check_syntax), ..Default::default() };
    crate::install(&mut s);
    s
}

fn ok(s: &mut Session, code: &str) -> Outcome {
    let o = run_code(s, code, "test.jsx");
    assert!(o.error.is_none(), "script failed: {:?}\noutput: {:?}", o.error, o.output);
    o
}

/// The script from the report: comp A (with a layer) is made first, the keys are in comp C.
#[test]
fn interpolation_and_ease_apply_in_a_comp_that_is_not_active() {
    let mut s = session();
    let o = ok(
        &mut s,
        r#"
        var A = app.project.items.addComp('A', 400, 400, 1, 10, 20); A.layers.addNull();
        var C = app.project.items.addComp('C', 400, 400, 1, 10, 20);
        var n = C.layers.addNull(); n.threeDLayer = true;
        var p = n.property('ADBE Transform Group').property('ADBE Rotate Y');
        p.setValueAtTime(1, 0); p.setValueAtTime(2, -178);
        for (var j = 1; j <= 2; j++) {
          p.setInterpolationTypeAtKey(j, KeyframeInterpolationType.BEZIER, KeyframeInterpolationType.BEZIER);
          p.setTemporalEaseAtKey(j, [new KeyframeEase(0, 60)], [new KeyframeEase(0, 60)]);
        }
        [p.keyOutInterpolationType(1), p.keyInInterpolationType(2), p.keyOutTemporalEase(1)[0].influence,
         p.keyInTemporalEase(2)[0].influence, p.valueAtTime(1.25, false)];
        "#,
    );
    let v = o.result.as_array().unwrap();
    assert_eq!((&v[0], &v[1]), (&json!(6613), &json!(6613)), "Bezier on both keys: {v:?}");
    assert!((v[2].as_f64().unwrap() - 60.0).abs() < 1e-6 && (v[3].as_f64().unwrap() - 60.0).abs() < 1e-6, "{v:?}");
    let at = v[4].as_f64().unwrap();
    assert!((at - -44.5).abs() > 1.0, "eased, not the linear -44.5: {at}");
    assert!(at < 0.0 && at > -44.5, "an ease-out start moves slower than linear: {at}");
}

/// A key the user selected in the property's comp stays selected, and only the named key changes.
/// The active comp's own key selection is untouched.
#[test]
fn a_per_key_method_in_another_comp_changes_only_its_key_and_keeps_both_selections() {
    let mut s = session();
    let o = ok(
        &mut s,
        r#"
        var C = app.project.items.addComp('C', 100, 100, 1, 10, 20);
        var p = C.layers.addNull().transform.rotation;
        p.setValuesAtTimes([0, 1, 2], [0, 90, 180]);
        var A = app.project.items.addComp('A', 100, 100, 1, 10, 20);
        A.openInViewer();
        var q = A.layers.addNull().transform.opacity;
        q.setValuesAtTimes([0, 1], [0, 100]);
        app.run('edit.deselectAll', {});
        q.setSelectedAtKey(2, true);
        // the user selected key 3 of C earlier: C remembers it while A is active
        p.setSelectedAtKey(3, true);
        p.setInterpolationTypeAtKey(1, KeyframeInterpolationType.HOLD);
        p.setTemporalEaseAtKey(2, [new KeyframeEase(0, 80)]);
        var hold = KeyframeInterpolationType.HOLD;
        var out = [p.keyOutInterpolationType(1) == hold, p.keyOutInterpolationType(3) == hold, p.keyInInterpolationType(3) == hold,
          p.keyInTemporalEase(2)[0].influence, p.keyInTemporalEase(3)[0].influence,
          q.keySelected(1), q.keySelected(2)];
        p.removeKey(1);
        out.push(p.numKeys, p.keyTime(1), p.keyValue(1), q.keySelected(2), app.project.activeItem.name);
        // C's remembered selection is live again once C is active: still the user's key (now 2)
        C.openInViewer();
        out.push(p.keySelected(1), p.keySelected(2));
        out;
        "#,
    );
    let v = o.result.as_array().unwrap();
    let ease = |i: usize| v[i].as_f64().unwrap();
    assert_eq!(&v[..3], &[json!(true), json!(false), json!(false)], "only key 1 holds: {v:?}");
    assert!((ease(3) - 80.0).abs() < 1e-6, "key 2 eased: {v:?}");
    assert!((ease(4) - 80.0).abs() > 1e-6, "key 3 untouched: {v:?}");
    assert_eq!(&v[5..7], &[json!(false), json!(true)], "A's key selection kept: {v:?}");
    assert_eq!(&v[7..12], &[json!(2), json!(1), json!(90), json!(true), json!("A")], "key 1 removed, A still active: {v:?}");
    assert_eq!(&v[12..], &[json!(false), json!(true)], "C's key selection kept, the target key not added: {v:?}");
}
