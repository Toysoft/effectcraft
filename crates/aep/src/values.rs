//! Static transform values with their units, and the "absent = default" rule.
//!
//! After Effects stores a property only when it differs from the default it computes for the layer
//! and composition (sparse storage), so a reader has to supply the defaults: Position defaults to
//! the composition centre `[width / 2, height / 2, 0]` (verified: after the composition is resized
//! without moving the layer, the old centre appears as a stored value), Anchor Point to the layer
//! centre (`0.5, 0.5` of the layer size), Scale to 100 %, Rotation to 0 and Opacity to 100 %. These
//! defaults are verified for AV layers (solids); cameras and lights use other conventions that
//! are not decoded.

use crate::typed::{Layer, PropGroup, PropNode};

/// Where a [`Tracked`] value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueSource {
    /// The property is absent from the file: the value is the computed default.
    Default,
    /// The property stores a static value.
    Stored,
    /// The property has keyframes: the value is that of the first keyframe.
    Keyframed,
    /// The property is present but its value could not be read: the value is the default.
    Unreadable,
}

/// A value and where it came from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tracked<T> {
    pub value: T,
    pub source: ValueSource,
}

/// The five basic transform properties of an AV layer, in the units After Effects shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// Pixels `[x, y, z]`.
    pub position: Tracked<[f64; 3]>,
    /// Stored as a fraction of the layer size for x and y (`10, 20` px on a 1920x1080 layer is
    /// `10/1920, 20/1080`) and in pixels for z; see [`Transform::anchor_px`]. Whether the divisor is
    /// the layer's source size or the composition size is not decided by the corpus (both were
    /// 1920x1080).
    pub anchor_fraction: Tracked<[f64; 3]>,
    /// Percent `[x, y, z]` (stored as fractions: 50 % is `0.5`).
    pub scale_percent: Tracked<[f64; 3]>,
    /// Degrees (Rotation, the Z rotation).
    pub rotation_degrees: Tracked<f64>,
    /// Percent (stored as a fraction: 25 % is `0.25`).
    pub opacity_percent: Tracked<f64>,
}

impl Transform {
    /// The anchor point in pixels for a layer of this size.
    pub fn anchor_px(&self, width: f64, height: f64) -> [f64; 3] {
        let [x, y, z] = self.anchor_fraction.value;
        [x * width, y * height, z]
    }
}

/// Reads `N` components from the property `name` of `group` and multiplies them by `scale` (stored
/// fractions to percent); `default` is in the output units and used when the property is absent.
fn read<const N: usize>(group: &PropGroup<'_, '_>, name: &str, default: [f64; N], scale: f64) -> Tracked<[f64; N]> {
    let unreadable = Tracked { value: default, source: ValueSource::Unreadable };
    let property = match group.get(name) {
        None => return Tracked { value: default, source: ValueSource::Default },
        Some(PropNode::Property(p)) => p,
        Some(PropNode::Group(_) | PropNode::Other(_)) => return unreadable,
    };
    let (values, source) =
        if property.is_animated() { (property.first_key_value(), ValueSource::Keyframed) } else { (property.static_value(), ValueSource::Stored) };
    let Some(values) = values else { return unreadable };
    if values.len() < N {
        return unreadable;
    }
    let mut out = default;
    for (slot, v) in out.iter_mut().zip(&values) {
        *slot = *v * scale;
    }
    Tracked { value: out, source }
}

impl<'f, 'a> Layer<'f, 'a> {
    /// The transform of an AV layer in a composition of this size (pixels), with absent
    /// properties replaced by their defaults. `None` when the layer has no Transform group.
    pub fn transform(&self, comp_width: u32, comp_height: u32) -> Option<Transform> {
        let Some(PropNode::Group(group)) = self.properties()?.get("ADBE Transform Group") else { return None };
        let centre = [f64::from(comp_width) / 2.0, f64::from(comp_height) / 2.0, 0.0];
        let rotation = read(&group, "ADBE Rotate Z", [0.0], 1.0);
        let opacity = read(&group, "ADBE Opacity", [100.0], 100.0);
        Some(Transform {
            position: read(&group, "ADBE Position", centre, 1.0),
            anchor_fraction: read(&group, "ADBE Anchor Point", [0.5, 0.5, 0.0], 1.0),
            scale_percent: read(&group, "ADBE Scale", [100.0, 100.0, 100.0], 100.0),
            rotation_degrees: Tracked { value: rotation.value[0], source: rotation.source },
            opacity_percent: Tracked { value: opacity.value[0], source: opacity.source },
        })
    }
}
