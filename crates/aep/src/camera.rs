//! Cameras and lights.
//!
//! Both are layers without a source (`ldta` byte 40 is 0 for a camera and `0xffff_ffff` for a light)
//! whose type is 2 (camera) or 1 (light). Their `ldta` carries the auto-orient bit (byte 38, `0x40`)
//! and, for a light, the light type (see [`LayerHeader::light_type`](crate::LayerHeader)). The
//! properties sit in the same Transform group as on other layers but follow other conventions:
//!
//! * **Position** is stored as given once the user has set it. While it is at its computed default
//!   the record is an "auto state" (`tdb4` bytes 76 and 77 are 1 although there is no expression)
//!   that holds `[0, 0, -zoom]`; a two-node camera reports the composition centre for x and y,
//!   a one-node camera reports the stored `0, 0`.
//! * **Anchor Point** of a camera is its point of interest, in pixels (not a fraction).
//! * A light's **Intensity** (`ADBE Light Options Group/ADBE Light Intensity`, percent) is stored
//!   only when it is not the default 100.

use crate::typed::{Layer, PropNode};
use crate::values::{Tracked, ValueSource, read};

/// The Position of a camera or light layer as stored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraPosition {
    /// `[x, y, z]` in pixels as stored: the absolute position once set by the user, `[0, 0, -zoom]`
    /// in the auto state (camera) or `[0, 0, -1333.33]` (default light).
    pub stored: [f64; 3],
    /// True while the position is derived (`tdb4` bytes 76 and 77 are 1 without an expression).
    pub auto_state: bool,
}

impl CameraPosition {
    /// The position After Effects reports: in the auto state of a two-node camera (`two_node`) x and
    /// y are the composition centre; otherwise the stored value.
    pub fn reported(&self, comp_width: u32, comp_height: u32, two_node: bool) -> [f64; 3] {
        if self.auto_state && two_node { [f64::from(comp_width) / 2.0, f64::from(comp_height) / 2.0, self.stored[2]] } else { self.stored }
    }
}

impl<'f, 'a> Layer<'f, 'a> {
    /// The Position record of a camera or light layer.
    pub fn camera_position(&self) -> Option<CameraPosition> {
        let Some(PropNode::Group(transform)) = self.properties()?.get("ADBE Transform Group") else { return None };
        let Some(PropNode::Property(position)) = transform.get("ADBE Position") else { return None };
        let v = position.static_value()?;
        let stored = *v.as_slice().first_chunk::<3>()?;
        let h = position.header()?;
        let derived = h.get(76) == Some(&1) && h.get(77) == Some(&1) && position.expression().is_none();
        Some(CameraPosition { stored, auto_state: derived })
    }

    /// The point of interest of a camera in pixels (its Anchor Point record); `None` when absent.
    pub fn camera_point_of_interest(&self) -> Option<[f64; 3]> {
        let Some(PropNode::Group(transform)) = self.properties()?.get("ADBE Transform Group") else { return None };
        let tracked = read(&transform, "ADBE Anchor Point", [0.0; 3], 1.0);
        matches!(tracked.source, ValueSource::Stored | ValueSource::Keyframed).then_some(tracked.value)
    }

    /// A light's intensity in percent; the default 100 when the property is absent.
    pub fn light_intensity(&self) -> Option<Tracked<f64>> {
        let root = self.properties()?;
        let Some(PropNode::Group(options)) = root.get("ADBE Light Options Group") else { return None };
        let t = read(&options, "ADBE Light Intensity", [100.0], 1.0);
        Some(Tracked { value: t.value[0], source: t.source })
    }
}
