//! After Effects project (`.aep`) container reader and writer.
//!
//! An `.aep` is a `RIFX` container (big-endian RIFF) with form type `Egg!`. This crate reads it
//! into a lossless chunk tree ([`AepFile`]) and writes it back byte for byte. See
//! `docs/aep-format.md` for the observed format and how it was established (clean room: only
//! files we generated ourselves, one change per file).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod bytes;
mod chunk;
mod effects;
mod error;
mod footage;
mod header;
mod keys;
mod masks;
mod read;
mod textdoc;
mod typed;
mod values;
mod write;

pub use chunk::{AepFile, Body, Chunk, LIST, NESTED_IDS, Walk};
pub use effects::{Effect, EffectParam, ParamKind, ParamValue};
pub use error::AepError;
pub use footage::Footage;
pub use header::{BlendMode, CompHeader, LayerHeader, LayerType, Quality, Rational, Renderer};
pub use keys::{ExpressionFlags, Interpolation, KeyframeKind, SpatialKey};
pub use masks::{Mask, MaskMode, PathVertex, ShapePath};
pub use read::{Limits, parse, parse_with_limits};
pub use textdoc::{Marker, TextDocument, TextValue, parse_text_dict};
pub use typed::{Comp, Item, ItemKind, KeyList, Keyframe, Layer, ListHeader, Project, PropEntry, PropGroup, PropNode, Property, TimeBase};
pub use values::{Tracked, Transform, ValueSource};
pub use write::{write, write_to};
