//! After Effects project (`.aep`) container reader and writer.
//!
//! An `.aep` is a `RIFX` container (big-endian RIFF) with form type `Egg!`. This crate reads it
//! into a lossless chunk tree ([`AepFile`]) and writes it back byte for byte. See
//! `docs/aep-format.md` for the observed format and how it was established (clean room: only
//! files we generated ourselves, one change per file).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod chunk;
mod error;
mod read;
mod write;

pub use chunk::{AepFile, Body, Chunk, LIST, NESTED_IDS, Walk};
pub use error::AepError;
pub use read::{Limits, parse, parse_with_limits};
pub use write::{write, write_to};
