//! Shape layers: the groups under `ADBE Root Vectors Group` and the items in them.
//!
//! A shape layer (layer type 4) holds `tdgp[ADBE Root Vectors Group]` with one `ADBE Vector Group`
//! per shape group; the group holds `ADBE Vectors Group` (the contents, in z-order) and
//! `ADBE Vector Transform Group` (the group transform, only the properties that differ from the
//! default). Only the items and properties whose layout was verified are decoded; anything else is
//! listed as [`ShapeItem::Other`] with its match name.
//!
//! Colours are stored as ARGB doubles 0..255 and returned as fractions. Sizes, positions and path
//! points are in pixels relative to the layer origin.

use crate::masks::ShapePath;
use crate::typed::{Layer, PropGroup, PropNode, Property};
use crate::values::{Tracked, read, read_opt};

/// The stroke line cap (`ADBE Vector Stroke Line Cap`, stored as a double; butt is the default and
/// is not stored).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokeCap {
    /// `1`
    Butt,
    /// `2`
    Round,
    /// Any other value (projecting square was not sampled).
    Other(i64),
}

/// The stroke line join (`ADBE Vector Stroke Line Join`; miter is the default and is not stored).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokeJoin {
    /// `1`
    Miter,
    /// `2`
    Round,
    /// Any other value (bevel was not sampled).
    Other(i64),
}

/// A stroke item.
#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    /// The color as `[A, R, G, B]` fractions; `None` while the property is absent (its default is
    /// not verified).
    pub color: Option<Tracked<[f64; 4]>>,
    /// The width in pixels; `None` while absent.
    pub width: Option<Tracked<f64>>,
    pub cap: Tracked<StrokeCap>,
    pub join: Tracked<StrokeJoin>,
    /// The miter limit; after the join was touched it is stored in its default state (4).
    pub miter_limit: Option<Tracked<f64>>,
}

/// An item of a shape group's contents.
#[derive(Debug, Clone, PartialEq)]
pub enum ShapeItem {
    /// `ADBE Vector Shape - Rect`: the rectangle path.
    Rect { name: Option<String>, size: Option<Tracked<[f64; 2]>> },
    /// `ADBE Vector Shape - Group`: a bezier path (one shape per keyframe), points in pixels.
    Path { name: Option<String>, paths: Vec<ShapePath> },
    /// `ADBE Vector Graphic - Fill`: the color as `[A, R, G, B]` fractions.
    Fill { name: Option<String>, color: Option<Tracked<[f64; 4]>> },
    /// `ADBE Vector Graphic - Stroke`
    Stroke { name: Option<String>, stroke: Stroke },
    /// `ADBE Vector Graphic - G-Fill`: the start and end point of the gradient in pixels (the
    /// gradient type and the colour stops are not stored while they are at After Effects' default
    /// and their encoding is not known).
    GradientFill { name: Option<String>, start: Option<Tracked<[f64; 2]>>, end: Option<Tracked<[f64; 2]>> },
    /// `ADBE Vector Filter - RC`: rounded corners, the radius in pixels.
    RoundCorners { name: Option<String>, radius: Option<Tracked<f64>> },
    /// Any other item (ellipse, polystar, trim, repeater, ...: not ported).
    Other { match_name: String, name: Option<String> },
}

/// A shape group (`ADBE Vector Group`).
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeGroup {
    /// The group's name (`Group 1`).
    pub name: Option<String>,
    /// The group transform's Position in pixels; absent = `[0, 0]`.
    pub position: Tracked<[f64; 2]>,
    /// The contents in z-order (top first, as in the file).
    pub items: Vec<ShapeItem>,
}

const COLOR_SCALE: f64 = 1.0 / 255.0;

fn number(group: &PropGroup<'_, '_>, name: &str) -> Option<Tracked<f64>> {
    read_opt::<1>(group, name, 1.0).map(|t| Tracked { value: t.value[0], source: t.source })
}

fn paths_of(group: &PropGroup<'_, '_>) -> Vec<ShapePath> {
    let Some(PropNode::Other(oms)) = group.get("ADBE Vector Shape") else { return Vec::new() };
    let Some(omks) = oms.child_list(b"omks") else { return Vec::new() };
    omks.children().iter().filter(|c| c.is_list(b"shap")).filter_map(ShapePath::from_chunk).collect()
}

fn item(match_name: String, group: &PropGroup<'_, '_>) -> ShapeItem {
    let name = group.user_name();
    match match_name.as_str() {
        "ADBE Vector Shape - Rect" => ShapeItem::Rect { name, size: read_opt::<2>(group, "ADBE Vector Rect Size", 1.0) },
        "ADBE Vector Shape - Group" => ShapeItem::Path { name, paths: paths_of(group) },
        "ADBE Vector Graphic - Fill" => ShapeItem::Fill { name, color: read_opt::<4>(group, "ADBE Vector Fill Color", COLOR_SCALE) },
        "ADBE Vector Graphic - Stroke" => {
            let cap = read(group, "ADBE Vector Stroke Line Cap", [1.0], 1.0);
            let join = read(group, "ADBE Vector Stroke Line Join", [1.0], 1.0);
            let stroke = Stroke {
                color: read_opt::<4>(group, "ADBE Vector Stroke Color", COLOR_SCALE),
                width: number(group, "ADBE Vector Stroke Width"),
                cap: Tracked {
                    value: match cap.value[0].round() as i64 {
                        1 => StrokeCap::Butt,
                        2 => StrokeCap::Round,
                        other => StrokeCap::Other(other),
                    },
                    source: cap.source,
                },
                join: Tracked {
                    value: match join.value[0].round() as i64 {
                        1 => StrokeJoin::Miter,
                        2 => StrokeJoin::Round,
                        other => StrokeJoin::Other(other),
                    },
                    source: join.source,
                },
                miter_limit: number(group, "ADBE Vector Stroke Miter Limit"),
            };
            ShapeItem::Stroke { name, stroke }
        }
        "ADBE Vector Graphic - G-Fill" => ShapeItem::GradientFill {
            name,
            start: read_opt::<2>(group, "ADBE Vector Grad Start Pt", 1.0),
            end: read_opt::<2>(group, "ADBE Vector Grad End Pt", 1.0),
        },
        "ADBE Vector Filter - RC" => ShapeItem::RoundCorners { name, radius: number(group, "ADBE Vector RoundCorner Radius") },
        _ => ShapeItem::Other { match_name, name },
    }
}

fn group_of(group: &PropGroup<'_, '_>) -> ShapeGroup {
    let items = match group.get("ADBE Vectors Group") {
        Some(PropNode::Group(contents)) => contents
            .entries()
            .into_iter()
            .filter_map(|e| match e.node {
                PropNode::Group(g) => Some(item(e.match_name, &g)),
                PropNode::Property(_) | PropNode::Other(_) => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    let position = match group.get("ADBE Vector Transform Group") {
        Some(PropNode::Group(t)) => read(&t, "ADBE Vector Position", [0.0, 0.0], 1.0),
        _ => Tracked { value: [0.0, 0.0], source: crate::values::ValueSource::Default },
    };
    ShapeGroup { name: group.user_name(), position, items }
}

impl<'f, 'a> Layer<'f, 'a> {
    /// The shape groups of a shape layer (`ADBE Root Vectors Group`), top group first.
    pub fn shape_groups(&self) -> Vec<ShapeGroup> {
        let Some(root) = self.properties() else { return Vec::new() };
        let Some(PropNode::Group(vectors)) = root.get("ADBE Root Vectors Group") else { return Vec::new() };
        vectors
            .entries()
            .into_iter()
            .filter(|e| e.match_name == "ADBE Vector Group")
            .filter_map(|e| match e.node {
                PropNode::Group(g) => Some(group_of(&g)),
                PropNode::Property(_) | PropNode::Other(_) => None,
            })
            .collect()
    }
}

impl<'f, 'a> Property<'f, 'a> {
    /// The ARGB fractions of a static colour property (4 components stored as doubles 0..255).
    pub fn color_argb(&self) -> Option<[f64; 4]> {
        let v = self.static_value()?;
        let [a, r, g, b] = *v.as_slice().first_chunk::<4>()?;
        Some([a / 255.0, r / 255.0, g / 255.0, b / 255.0])
    }
}
