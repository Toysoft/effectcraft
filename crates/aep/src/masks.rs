//! Masks and bezier paths.
//!
//! A layer's masks are in `tdgp[ADBE Mask Parade]`: one `mkif` chunk (mode and invert switch) next
//! to one `tdgp[ADBE Mask Atom]` per mask. The atom holds the optional properties feather, opacity
//! and expansion (stored only when not at their defaults) and `om-s[ADBE Mask Shape]`: the path
//! property (`tdbs`, with a key list when animated) and `omks` with one `LIST shap` per keyframe (a
//! single one when the path is static).

use crate::Chunk;
use crate::bytes::{be_f32, be_u32};
use crate::typed::{KeyList, Layer, PropGroup, PropNode, Property};
use crate::values::{Tracked, read};

/// The mask mode (`mkif` `u32` at byte 4). The mode None was not sampled and is kept as its raw
/// value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskMode {
    /// `1`
    Add,
    /// `2`
    Subtract,
    /// `3`
    Intersect,
    /// `4`
    Lighten,
    /// `5`
    Darken,
    /// `6`
    Difference,
    /// Any other value (not sampled).
    Other(u32),
}

impl MaskMode {
    /// The mode for a stored id.
    pub fn from_id(id: u32) -> Self {
        match id {
            1 => Self::Add,
            2 => Self::Subtract,
            3 => Self::Intersect,
            4 => Self::Lighten,
            5 => Self::Darken,
            6 => Self::Difference,
            other => Self::Other(other),
        }
    }
}

/// A vertex of a bezier path with its two control points as absolute positions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathVertex {
    pub position: [f64; 2],
    /// The control point of the incoming segment.
    pub in_handle: [f64; 2],
    /// The control point of the outgoing segment.
    pub out_handle: [f64; 2],
}

impl PathVertex {
    /// The in tangent: the in handle minus the vertex (what the scripting API calls `inTangents`).
    pub fn in_tangent(&self) -> [f64; 2] {
        [self.in_handle[0] - self.position[0], self.in_handle[1] - self.position[1]]
    }

    /// The out tangent: the out handle minus the vertex (`outTangents`).
    pub fn out_tangent(&self) -> [f64; 2] {
        [self.out_handle[0] - self.position[0], self.out_handle[1] - self.position[1]]
    }
}

/// One path shape (`LIST shap`): a bounding box and the control points normalised inside it.
///
/// The `shph` chunk holds a magic word whose last byte carries the flags (`0x01` closed, `0x09` open:
/// bit `0x08`), the box (left, top, right, bottom) as four `f32`, and the byte at offset 20 (`1` in
/// every sample). The box is the box of the vertices **and** their handles. The `list` holds three
/// points per vertex, each as two `f32` in [0, 1] of the box, in the order `[vertex i, out handle i,
/// in handle i + 1]`; the last record is the in handle of vertex 0. For masks the box is normalised
/// to the layer size (whether the layer's or the composition's size is not decided: both were
/// 1920x1080); for the paths of shape layers it is in pixels. Handles equal their vertex when a
/// vertex has no tangents; a tangent is the handle minus the vertex, without a sign change.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapePath {
    /// Left, top, right, bottom.
    pub bbox: [f32; 4],
    /// The byte at offset 20 of `shph` (`1` in every sample, also for the open path).
    pub closed_flag: u8,
    /// True for an open path: bit `0x08` of the last byte of the first `shph` word.
    pub open: bool,
    /// The normalised points as stored, three per vertex.
    pub points: Vec<[f32; 2]>,
}

impl ShapePath {
    /// Decodes a `LIST shap`; `None` when `shph` or the point list is malformed.
    pub fn from_chunk(shap: &Chunk<'_>) -> Option<Self> {
        let shph = shap.child(b"shph")?.bytes()?;
        let bbox = [be_f32(shph, 4)?, be_f32(shph, 8)?, be_f32(shph, 12)?, be_f32(shph, 16)?];
        let closed_flag = *shph.get(20)?;
        let open = shph.get(3)? & 0x08 != 0;
        let list = KeyList::from_chunk(shap.child_list(b"list")?)?;
        if list.header.record_size != 8 {
            return None;
        }
        let points = list.records().map(|r| Some([be_f32(r, 0)?, be_f32(r, 4)?])).collect::<Option<Vec<_>>>()?;
        Some(Self { bbox, closed_flag, open, points })
    }

    /// The number of vertices (`points / 3`).
    pub fn vertex_count(&self) -> usize {
        self.points.len() / 3
    }

    /// The vertices with absolute control points: `box.left + u * (box.right - box.left)` (and the
    /// same for y), multiplied by `scale` (the layer size in pixels for a mask, `(1, 1)` for a
    /// shape layer path).
    pub fn vertices(&self, scale: (f64, f64)) -> Vec<PathVertex> {
        let [left, top, right, bottom] = self.bbox.map(f64::from);
        let abs = |p: Option<&[f32; 2]>| -> [f64; 2] {
            let [u, v] = p.copied().unwrap_or([0.0, 0.0]).map(f64::from);
            [(left + u * (right - left)) * scale.0, (top + v * (bottom - top)) * scale.1]
        };
        let n = self.vertex_count();
        let total = n.saturating_mul(3);
        (0..n)
            .map(|i| {
                let base = i.saturating_mul(3);
                PathVertex {
                    position: abs(self.points.get(base)),
                    out_handle: abs(self.points.get(base.saturating_add(1))),
                    // The in handle of vertex i is the record before vertex i (the last one for vertex 0).
                    in_handle: abs(self.points.get(base.saturating_add(total).saturating_sub(1) % total.max(1))),
                }
            })
            .collect()
    }
}

/// One mask of a layer.
#[derive(Debug, Clone, PartialEq)]
pub struct Mask {
    /// The mask name (`Mask 1`).
    pub name: Option<String>,
    /// The mode, from the parade's `mkif`. Only the first mask of a layer gets it: with several
    /// masks it is not known whether `mkif` is stored once per mask.
    pub mode: Option<MaskMode>,
    /// The invert switch (`mkif` byte 0); same restriction as `mode`.
    pub inverted: Option<bool>,
    /// Feather in pixels `[x, y]`; absent = `[0, 0]`.
    pub feather: Tracked<[f64; 2]>,
    /// Opacity in percent (stored as a fraction); absent = 100.
    pub opacity_percent: Tracked<f64>,
    /// Expansion in pixels; absent = 0.
    pub expansion: Tracked<f64>,
    /// True when the path has keyframes.
    pub path_animated: bool,
    /// The path keyframe times in seconds (empty for a static path).
    pub path_key_seconds: Vec<f64>,
    /// The path shapes: one for a static path, one per keyframe otherwise.
    pub shapes: Vec<ShapePath>,
}

fn mask_from_atom(atom: &PropGroup<'_, '_>, mkif: Option<&[u8]>) -> Mask {
    let opacity = read(atom, "ADBE Mask Opacity", [100.0], 100.0);
    let expansion = read(atom, "ADBE Mask Offset", [0.0], 1.0);
    let mut mask = Mask {
        name: atom.user_name(),
        mode: mkif.and_then(|m| be_u32(m, 4)).map(MaskMode::from_id),
        inverted: mkif.and_then(|m| m.first()).map(|b| *b != 0),
        feather: read(atom, "ADBE Mask Feather", [0.0, 0.0], 1.0),
        opacity_percent: Tracked { value: opacity.value[0], source: opacity.source },
        expansion: Tracked { value: expansion.value[0], source: expansion.source },
        path_animated: false,
        path_key_seconds: Vec::new(),
        shapes: Vec::new(),
    };
    if let Some(PropNode::Other(oms)) = atom.get("ADBE Mask Shape") {
        if let Some(prop) = oms.child_list(b"tdbs").and_then(Property::from_chunk) {
            mask.path_animated = prop.is_animated();
            if let (Some(keys), Some(scale)) = (prop.keys(), prop.time_scale()) {
                mask.path_key_seconds = keys.keyframes().filter_map(|k| k.seconds(scale)).collect();
            }
        }
        if let Some(omks) = oms.child_list(b"omks") {
            mask.shapes = omks.children().iter().filter(|c| c.is_list(b"shap")).filter_map(ShapePath::from_chunk).collect();
        }
    }
    mask
}

impl<'f, 'a> Layer<'f, 'a> {
    /// The layer's masks, in stack order.
    pub fn masks(&self) -> Vec<Mask> {
        let Some(root) = self.properties() else { return Vec::new() };
        let Some(PropNode::Group(parade)) = root.get("ADBE Mask Parade") else { return Vec::new() };
        let mkif = parade.chunk().child(b"mkif").and_then(Chunk::bytes);
        parade
            .entries()
            .into_iter()
            .filter(|e| e.match_name == "ADBE Mask Atom")
            .filter_map(|e| match e.node {
                PropNode::Group(atom) => Some(atom),
                _ => None,
            })
            .enumerate()
            .map(|(i, atom)| mask_from_atom(&atom, if i == 0 { mkif } else { None }))
            .collect()
    }
}
