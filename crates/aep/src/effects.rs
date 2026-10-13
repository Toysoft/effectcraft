//! Effect instances: match name, instance name, the parameter list from `parT`/`pard` and the
//! values stored with the instance.
//!
//! An effect on a layer is a `LIST sspc` under `ADBE Effect Parade`. It holds `fnam` (the
//! instance name), `LIST parT` (the parameter definitions: `parn`, then a `tdmn` and a 148-byte
//! `pard` per parameter, a `pdnm` after a popup), `LIST tdgp` (the values) and `pgui`. Only
//! parameters that differ from their `pard` default have a value record in the `tdgp`.

use crate::Chunk;
use crate::bytes::{be_f64, be_i32, be_u32, cstr, text, until_nul};
use crate::typed::{Layer, PropGroup, PropNode, Property};

/// The parameter type, the `u32` at byte 12 of `pard`. The numbering matches the plug-in
/// parameter types of the After Effects effect interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    /// `0`: a layer reference (also the implicit first parameter, the input layer).
    Layer,
    /// `2`: a fixed-point (16.16) slider.
    FixedSlider,
    /// `3`: an angle (16.16 degrees).
    Angle,
    /// `4`
    Checkbox,
    /// `5`
    Color,
    /// `6`
    Point,
    /// `7`: a popup menu (the items are in the `pdnm` chunk).
    Popup,
    /// `9`: no data (the group-end entry `ADBE Effect Built In Params`).
    NoData,
    /// `10`: a floating-point slider.
    FloatSlider,
    /// `12`: a path / mask reference.
    Path,
    /// Any other value.
    Other(u32),
}

impl ParamKind {
    fn from_u32(v: u32) -> Self {
        match v {
            0 => Self::Layer,
            2 => Self::FixedSlider,
            3 => Self::Angle,
            4 => Self::Checkbox,
            5 => Self::Color,
            6 => Self::Point,
            7 => Self::Popup,
            9 => Self::NoData,
            10 => Self::FloatSlider,
            12 => Self::Path,
            other => Self::Other(other),
        }
    }
}

/// A decoded parameter value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParamValue {
    /// A slider or angle value (float sliders: as stored; fixed sliders and angles: 16.16 decoded).
    Number(f64),
    /// A colour as ARGB fractions 0..1 (stored as doubles 0..255 in the order alpha, red, green,
    /// blue: a colour `[0.2, 0.6, 1]` is `[255, 51, 153, 255]`, with float rounding).
    Argb([f64; 4]),
    /// A popup's default item index (1-based in the samples).
    Index(u32),
}

/// An effect instance on a layer.
#[derive(Debug, Clone)]
pub struct Effect<'f, 'a> {
    match_name: String,
    chunk: &'f Chunk<'a>,
}

/// One parameter of an effect.
#[derive(Debug, Clone)]
pub struct EffectParam<'f, 'a> {
    /// The position in `parT` (0 is the implicit input layer).
    pub index: usize,
    /// The `tdmn` match name (`<effect match name>-NNNN`, the last one is
    /// `ADBE Effect Built In Params`).
    pub match_name: String,
    pub kind: ParamKind,
    /// The parameter name (`Blurriness`, `Slider`, ...; empty for the implicit parameters).
    pub name: String,
    /// The popup items (`pdnm`, split at `|`).
    pub items: Option<Vec<String>>,
    pard: &'f [u8],
    stored: Option<Property<'f, 'a>>,
}

impl<'f, 'a> Effect<'f, 'a> {
    /// The effect's match name (`ADBE Slider Control`, `ADBE Gaussian Blur 2`, ...).
    pub fn match_name(&self) -> &str {
        &self.match_name
    }

    /// The underlying `LIST sspc` chunk.
    pub fn chunk(&self) -> &'f Chunk<'a> {
        self.chunk
    }

    /// The instance name shown in the effect controls (`fnam`).
    pub fn name(&self) -> Option<String> {
        self.chunk.child(b"fnam")?.children().iter().find(|c| c.is(b"Utf8"))?.bytes().map(text)
    }

    /// The number of parameters declared by `parn` (the implicit input layer and the built-in
    /// entry included).
    pub fn declared_param_count(&self) -> Option<u32> {
        be_u32(self.chunk.child_list(b"parT")?.child(b"parn")?.bytes()?, 0)
    }

    /// The parameters in `parT` order.
    pub fn params(&self) -> Vec<EffectParam<'f, 'a>> {
        let mut out: Vec<EffectParam<'f, 'a>> = Vec::new();
        let Some(part) = self.chunk.child_list(b"parT") else { return out };
        let values = self.chunk.child_list(b"tdgp").and_then(PropGroup::from_chunk);
        let mut pending: Option<String> = None;
        for c in part.children() {
            if c.is(b"tdmn") {
                pending = c.bytes().map(|b| text(until_nul(b)));
            } else if c.is(b"pard") {
                let Some(pard) = c.bytes() else { continue };
                let match_name = pending.take().unwrap_or_default();
                let stored = values.as_ref().and_then(|g| match g.get(&match_name) {
                    Some(PropNode::Property(p)) => Some(p),
                    _ => None,
                });
                out.push(EffectParam {
                    index: out.len(),
                    match_name,
                    kind: ParamKind::from_u32(be_u32(pard, 12).unwrap_or(u32::MAX)),
                    name: cstr(pard, 16, 32).unwrap_or_default(),
                    items: None,
                    pard,
                    stored,
                });
            } else if c.is(b"pdnm")
                && let Some(last) = out.last_mut()
                && let Some(u) = c.children().iter().find(|u| u.is(b"Utf8"))
                && let Some(b) = u.bytes()
            {
                last.items = Some(text(b).split('|').map(str::to_string).collect());
            }
        }
        out
    }
}

impl<'f, 'a> EffectParam<'f, 'a> {
    /// The raw 148-byte `pard` record (it contains stale memory after the name).
    pub fn pard(&self) -> &'f [u8] {
        self.pard
    }

    /// The value record stored with the instance (`<match>-NNNN` in the instance's `tdgp`), which
    /// exists only for parameters that differ from the default.
    pub fn stored(&self) -> Option<Property<'f, 'a>> {
        self.stored
    }

    /// The value in the value record: float sliders and colours only (the units of the other
    /// types were not sampled). The record counts when its `tdsb` is 1.
    pub fn stored_value(&self) -> Option<ParamValue> {
        let p = self.stored?;
        if p.flags() != Some(1) || p.is_animated() {
            return None;
        }
        let v = p.static_value()?;
        match self.kind {
            ParamKind::FloatSlider => v.first().copied().map(ParamValue::Number),
            ParamKind::Color => {
                let [a, r, g, b] = *v.as_slice().first_chunk::<4>()?;
                Some(ParamValue::Argb([a / 255.0, r / 255.0, g / 255.0, b / 255.0]))
            }
            _ => None,
        }
    }

    /// The default (or built-in current value) stored in `pard` at byte 56: a float slider's `f64`,
    /// a fixed slider's or angle's 16.16 value, a popup's item index. Other types: `None`
    /// (checkbox, point and colour defaults are not verified).
    ///
    /// For Gaussian Blur the 25 stored here equals the value that was set (078); whether built-in
    /// effects keep their current value in `pard` is not decided by the corpus.
    pub fn pard_default(&self) -> Option<ParamValue> {
        match self.kind {
            ParamKind::FloatSlider => be_f64(self.pard, 56).map(ParamValue::Number),
            ParamKind::FixedSlider | ParamKind::Angle => be_i32(self.pard, 56).map(|v| ParamValue::Number(f64::from(v) / 65536.0)),
            ParamKind::Popup => be_u32(self.pard, 56).map(ParamValue::Index),
            _ => None,
        }
    }

    /// The effective value: the stored value when there is one, else the `pard` default.
    pub fn value(&self) -> Option<ParamValue> {
        self.stored_value().or_else(|| self.pard_default())
    }
}

impl<'f, 'a> Layer<'f, 'a> {
    /// The effects on the layer, in stack order (`ADBE Effect Parade`).
    pub fn effects(&self) -> Vec<Effect<'f, 'a>> {
        let Some(root) = self.properties() else { return Vec::new() };
        let Some(PropNode::Group(parade)) = root.get("ADBE Effect Parade") else { return Vec::new() };
        parade
            .entries()
            .into_iter()
            .filter_map(|e| match e.node {
                PropNode::Other(chunk) if chunk.is_list(b"sspc") => Some(Effect { match_name: e.match_name, chunk }),
                _ => None,
            })
            .collect()
    }
}
