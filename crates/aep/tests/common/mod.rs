//! Shared helpers for the integration tests.
#![allow(dead_code)]

use std::path::PathBuf;

/// The generated fixtures (own test data, see ATTRIBUTION.md).
pub const FIXTURES: &[&str] = &[
    "000-base.aep",
    "001-empty.aep",
    "010-layer-name.aep",
    "012-layer-shy.aep",
    "013-layer-locked.aep",
    "014-layer-solo.aep",
    "015-layer-3d.aep",
    "016-layer-blend-multiply.aep",
    "017-layer-motionblur.aep",
    "018-layer-stretch200.aep",
    "019-layer-inpoint1.aep",
    "021-layer-starttime0.5.aep",
    "022-layer-parent-null.aep",
    "023-layer-quality-draft.aep",
    "024-layer-collapse.aep",
    "025-layer-adjustment.aep",
    "026-layer-guide.aep",
    "030-comp-size-1280x720.aep",
    "031-comp-fps30.aep",
    "032-comp-duration10.aep",
    "033-comp-bg-green.aep",
    "034-comp-shutter90.aep",
    "035-comp-workarea-1-2.aep",
    "037-comp-pixelaspect2.aep",
    "038-comp-motionblur.aep",
    "039-comp-renderer-advanced3d.aep",
    "040-static-position-100-200.aep",
    "041-static-scale-50.aep",
    "042-static-rotation-45.aep",
    "043-static-opacity-25.aep",
    "044-static-anchor-10-20.aep",
    "052-key-position-2key-ease.aep",
    "053-key-position-2key-hold-key1.aep",
    "054-key-position-2key-spatial-bezier.aep",
    "055-key-position-3key-inserted-t2.5.aep",
    "056-key-position-3key-roving-key2.aep",
    "057-key-position-3key-label9-key2.aep",
    "058-key-opacity-2key.aep",
    "060-expr-opacity-wiggle.aep",
    "071-fx-slider-value77.aep",
    "073-fx-color-0.2-0.6-1.aep",
    "077-fx-dropdown-default.aep",
    "080-fx-dropshadow-default.aep",
    "090-mask-rect.aep",
    "091-mask-rect-props.aep",
    "093-mask-path-2keys.aep",
    "110-text-A.aep",
    "113-text-Hello-size60-fillorange.aep",
    "122-marker-comp-1s.aep",
    "123-marker-layer-1s.aep",
    "124-precomp-Pre.aep",
    "126-footage-png-layer.aep",
];

pub fn fixture(name: &str) -> Vec<u8> {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "fixtures", name].iter().collect();
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}
