//! Keyframe records, property flags and expressions.
//!
//! Keyframe times are **layer time**, in units of the property's own time scale
//! ([`Property::time_scale`], equal to the composition's timescale in every sample).

use crate::bytes::{be_f64, be_u16, be_u32, text};
use crate::typed::{Keyframe, Property};

/// A keyframe interpolation type (a byte of the record). The names are likely, not proven by a
/// second source: `1` is what keys added through the scripting interface get ("linear in and
/// out"), `2` is what the ease preset gives, `3` is what a hold keyframe has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    /// `1`
    Linear,
    /// `2`
    Bezier,
    /// `3`
    Hold,
    /// Any other value.
    Other(u8),
}

impl Interpolation {
    fn from_byte(b: u8) -> Self {
        match b {
            1 => Self::Linear,
            2 => Self::Bezier,
            3 => Self::Hold,
            other => Self::Other(other),
        }
    }
}

/// What a keyframe record is, judged by its size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyframeKind {
    /// 48 bytes: one `f64` value at byte 8 (verified for Opacity).
    Scalar,
    /// 128 bytes: three `f64` components with spatial tangents (verified for Position).
    Spatial,
    /// 64 bytes: a mask path key; the path itself is in the matching `shap` chunk.
    MaskPath,
    /// 16 bytes: a marker (time and two hold bytes).
    Marker,
    /// Any other size (not sampled).
    Other(usize),
}

/// The fields of a 128-byte spatial keyframe record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpatialKey {
    /// The `u32` at byte 8: `1` normally, `0` after the spatial interpolation was changed to
    /// non-auto-bezier / non-continuous (the two switches were changed together).
    pub auto_bezier_flag: u32,
    /// The `f64` at byte 16: the arc length of the segment to the next key, updated lazily by
    /// After Effects (0 right after creation). Safe to ignore.
    pub segment_length: f64,
    /// The four `f64` at bytes 24, 32, 40 and 48: two (speed, influence) pairs. The ease preset
    /// puts speed 0 at 24 and 40 and influence 0.33 at 32 and 48. Which pair is the in side and
    /// which the out side is not decided by the corpus, so the slots are exposed as stored.
    pub ease: [f64; 4],
    /// The value (x, y, z) in pixels, bytes 56 to 72.
    pub value: [f64; 3],
    /// The spatial in tangent as an offset from the value, bytes 80 to 96.
    pub in_tangent: [f64; 3],
    /// The spatial out tangent as an offset from the value, bytes 104 to 120.
    pub out_tangent: [f64; 3],
}

fn vec3(b: &[u8], at: usize) -> Option<[f64; 3]> {
    Some([be_f64(b, at)?, be_f64(b, at.checked_add(8)?)?, be_f64(b, at.checked_add(16)?)?])
}

impl<'f> Keyframe<'f> {
    /// The record kind by size.
    pub fn kind(&self) -> KeyframeKind {
        match self.record.len() {
            48 => KeyframeKind::Scalar,
            128 => KeyframeKind::Spatial,
            64 => KeyframeKind::MaskPath,
            16 => KeyframeKind::Marker,
            other => KeyframeKind::Other(other),
        }
    }

    /// The time in seconds given the property's time scale (units per second).
    pub fn seconds(&self, time_scale: u32) -> Option<f64> {
        if time_scale == 0 {
            return None;
        }
        Some(f64::from(self.time_units()?) / f64::from(time_scale))
    }

    /// The interpolation byte at offset 4. It equals the byte at offset 5 in every sample; which of
    /// the two is the incoming and which the outgoing side is unresolved.
    pub fn interpolation_a(&self) -> Option<Interpolation> {
        self.record.get(4).copied().map(Interpolation::from_byte)
    }

    /// The interpolation byte at offset 5, see [`Keyframe::interpolation_a`].
    pub fn interpolation_b(&self) -> Option<Interpolation> {
        self.record.get(5).copied().map(Interpolation::from_byte)
    }

    /// The flags word at byte 6.
    pub fn flags(&self) -> Option<u16> {
        be_u16(self.record, 6)
    }

    /// The keyframe label: bits 6 to 9 of the flags (label 9 gives `0x0247` on a spatial key).
    pub fn label(&self) -> Option<u8> {
        u8::try_from((self.flags()? >> 6) & 0x0f).ok()
    }

    /// True when the key is roving: bit `0x20` of the flags.
    pub fn is_roving(&self) -> Option<bool> {
        Some(self.flags()? & 0x0020 != 0)
    }

    /// The value of a 48-byte scalar record: the `f64` at byte 8 (Opacity as a fraction).
    pub fn scalar_value(&self) -> Option<f64> {
        if self.kind() != KeyframeKind::Scalar {
            return None;
        }
        be_f64(self.record, 8)
    }

    /// The four `f64` slots at bytes 16 to 40 of a scalar record, as stored. They are 0 for linear
    /// keys; how they map to (speed, influence) pairs is not decided by the corpus.
    pub fn scalar_ease_slots(&self) -> Option<[f64; 4]> {
        if self.kind() != KeyframeKind::Scalar {
            return None;
        }
        Some([be_f64(self.record, 16)?, be_f64(self.record, 24)?, be_f64(self.record, 32)?, be_f64(self.record, 40)?])
    }

    /// The decoded 128-byte spatial record.
    pub fn spatial(&self) -> Option<SpatialKey> {
        if self.kind() != KeyframeKind::Spatial {
            return None;
        }
        let r = self.record;
        Some(SpatialKey {
            auto_bezier_flag: be_u32(r, 8)?,
            segment_length: be_f64(r, 16)?,
            ease: [be_f64(r, 24)?, be_f64(r, 32)?, be_f64(r, 40)?, be_f64(r, 48)?],
            value: vec3(r, 56)?,
            in_tangent: vec3(r, 80)?,
            out_tangent: vec3(r, 104)?,
        })
    }
}

/// The bytes of a property header that mark an expression: bytes 76, 77 and 120 of `tdb4` are
/// all 1 when the property has an expression. Which of them is the enable switch is unknown (the
/// disabled state was not sampled).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpressionFlags {
    pub byte_76: u8,
    pub byte_77: u8,
    pub byte_120: u8,
}

impl ExpressionFlags {
    /// True when all three bytes are 1.
    pub fn all_set(&self) -> bool {
        self.byte_76 == 1 && self.byte_77 == 1 && self.byte_120 == 1
    }
}

impl<'f, 'a> Property<'f, 'a> {
    /// The time scale (units per second) of this property's keyframe times: the `u32` at byte 12
    /// of `tdb4` (25600 at 25 fps, 30720 at 30 fps).
    pub fn time_scale(&self) -> Option<u32> {
        be_u32(self.header()?, 12)
    }

    /// The property's `tdsb` flags: `1` for a property that was written (non-default), `3` for
    /// the default state.
    pub fn flags(&self) -> Option<u32> {
        be_u32(self.chunk.child(b"tdsb")?.bytes()?, 0)
    }

    /// True when `tdb4` byte 5 has bit 0 set: the property is static, it has no keyframes.
    pub fn is_static_flag_set(&self) -> Option<bool> {
        Some(self.header()?.get(5)? & 1 != 0)
    }

    /// The expression source: the raw UTF-8 text of the `Utf8` child that follows the value in the
    /// property (no length prefix, no engine tag; the engine is project-wide).
    pub fn expression(&self) -> Option<String> {
        self.chunk.child(b"Utf8").and_then(|c| c.bytes()).map(text)
    }

    /// The expression flag bytes of `tdb4`.
    pub fn expression_flags(&self) -> Option<ExpressionFlags> {
        let h = self.header()?;
        Some(ExpressionFlags { byte_76: *h.get(76)?, byte_77: *h.get(77)?, byte_120: *h.get(120)? })
    }

    /// The value of the first keyframe (a scalar or spatial record), for reading an animated
    /// property at its first key.
    pub fn first_key_value(&self) -> Option<Vec<f64>> {
        let key = self.keys()?.keyframes().next()?;
        match key.kind() {
            KeyframeKind::Spatial => key.spatial().map(|s| s.value.to_vec()),
            KeyframeKind::Scalar => key.scalar_value().map(|v| vec![v]),
            KeyframeKind::MaskPath | KeyframeKind::Marker | KeyframeKind::Other(_) => None,
        }
    }
}
