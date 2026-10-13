//! Shared helpers for the integration tests.
#![allow(dead_code)]

use std::path::PathBuf;

/// The generated fixtures (own test data, see ATTRIBUTION.md).
pub const FIXTURES: &[&str] = &[
    "001-empty.aep",
    "000-base.aep",
    "015-layer-3d.aep",
    "031-comp-fps30.aep",
    "040-static-position-100-200.aep",
    "055-key-position-3key-inserted-t2.5.aep",
    "071-fx-slider-value77.aep",
    "090-mask-rect.aep",
    "110-text-A.aep",
];

pub fn fixture(name: &str) -> Vec<u8> {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "fixtures", name].iter().collect();
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}
