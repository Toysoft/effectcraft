//! Item ids and labels, footage items (solids and files), precomp linkage, and the name and size
//! of a layer's source.

use crate::bytes::{be_f32, be_u32, cstr, le_u32, text};
use crate::typed::{Item, ItemKind, Layer, Project};

/// The footage data of an item (`LIST Pin ` with `sspc`, `opti` and, for files, `Als2/alas`).
#[derive(Debug, Clone, PartialEq)]
pub struct Footage {
    /// The 4-character file type tag at byte 22 of `sspc` (`Soli` for a solid, `png!` for a png).
    pub file_type: String,
    /// The width in pixels (`u32` at byte 30 of `sspc`).
    pub width: u32,
    /// The height in pixels (`u32` at byte 34 of `sspc`).
    pub height: u32,
    /// The display name stored in `opti` (the item's own `Utf8` name is empty for footage): at byte
    /// 26 for a solid and at byte 58 for a file.
    pub name: Option<String>,
    /// True for a solid (tag `Soli` at the start of `opti`).
    pub is_solid: bool,
    /// The colour of a solid as ARGB floats 0..1 (`opti` bytes 10, 14, 18, 22); a red solid is
    /// `[1, 1, 0, 0]`.
    pub solid_argb: Option<[f32; 4]>,
    /// The file path from the alias JSON (`Als2/alas`, key `fullpath`); none for solids.
    pub path: Option<String>,
}

impl Footage {
    /// The solid's colour as fractions R, G, B.
    pub fn solid_rgb(&self) -> Option<[f32; 3]> {
        let [_, r, g, b] = self.solid_argb?;
        Some([r, g, b])
    }
}

/// The string value of `"key"` in a flat JSON object, with the standard escapes decoded.
/// Returns `None` when the key is missing or the value is not a string.
pub(crate) fn json_string(json: &[u8], key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let needle = needle.as_bytes();
    let at = json.windows(needle.len()).position(|w| w == needle)?;
    let mut i = at.checked_add(needle.len())?;
    let skip_ws = |i: &mut usize| {
        while json.get(*i).is_some_and(|c| c.is_ascii_whitespace()) {
            *i = i.saturating_add(1);
        }
    };
    skip_ws(&mut i);
    if json.get(i) != Some(&b':') {
        return None;
    }
    i = i.saturating_add(1);
    skip_ws(&mut i);
    if json.get(i) != Some(&b'"') {
        return None;
    }
    i = i.saturating_add(1);
    let mut out: Vec<u8> = Vec::new();
    loop {
        let c = *json.get(i)?;
        i = i.saturating_add(1);
        match c {
            b'"' => return Some(text(&out)),
            b'\\' => {
                let e = *json.get(i)?;
                i = i.saturating_add(1);
                match e {
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'r' => out.push(b'\r'),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'u' => {
                        let hex = json.get(i..i.checked_add(4)?)?;
                        let code = u32::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?;
                        i = i.saturating_add(4);
                        let ch = char::from_u32(code).unwrap_or('\u{fffd}');
                        let mut buf = [0u8; 4];
                        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                    }
                    other => out.push(other),
                }
            }
            other => out.push(other),
        }
    }
}

impl<'f, 'a> Item<'f, 'a> {
    /// The item id: the big-endian `u32` at byte 16 of `idta` (what a layer's source id refers to).
    pub fn id(&self) -> Option<u32> {
        be_u32(self.chunk.child(b"idta")?.bytes()?, 16)
    }

    /// The label colour index, byte 58 of `idta` (15 for a composition made by precomposing).
    pub fn label(&self) -> Option<u8> {
        self.chunk.child(b"idta")?.bytes()?.get(58).copied()
    }

    /// The ids of the compositions this item uses as precomp layers: the little-endian `u32` of
    /// each `idpi` chunk.
    pub fn used_item_ids(&self) -> Vec<u32> {
        self.chunk.children().iter().filter(|c| c.is(b"idpi")).filter_map(|c| le_u32(c.bytes()?, 0)).collect()
    }

    /// The footage data of a footage item.
    pub fn footage(&self) -> Option<Footage> {
        if self.kind()? != ItemKind::Footage {
            return None;
        }
        let pin = self.chunk.child_list(b"Pin ")?;
        let sspc = pin.child(b"sspc")?.bytes()?;
        let opti = pin.child(b"opti")?.bytes()?;
        let is_solid = opti.get(..4) == Some(&b"Soli"[..]);
        let path = pin.child_list(b"Als2").and_then(|als| als.child(b"alas")).and_then(|alas| alas.bytes()).and_then(|json| json_string(json, "fullpath"));
        let solid_argb = if is_solid { Some([be_f32(opti, 10)?, be_f32(opti, 14)?, be_f32(opti, 18)?, be_f32(opti, 22)?]) } else { None };
        Some(Footage {
            file_type: cstr(sspc, 22, 4)?,
            width: be_u32(sspc, 30)?,
            height: be_u32(sspc, 34)?,
            name: cstr(opti, if is_solid { 26 } else { 58 }, 256).filter(|n| !n.is_empty()),
            is_solid,
            solid_argb,
            path,
        })
    }

    /// The name this item shows in the project: the item name, or for footage the name stored in
    /// `opti`.
    pub fn display_name(&self) -> Option<String> {
        self.name().or_else(|| self.footage().and_then(|f| f.name))
    }
}

impl<'f, 'a> Project<'f, 'a> {
    /// The item with this id (`idta` byte 16), searching every item including those in folders.
    pub fn item_by_id(&self, id: u32) -> Option<Item<'f, 'a>> {
        self.all_items().into_iter().find(|i| i.id() == Some(id))
    }

    /// The item a layer is made from (its source id), e.g. the solid, the footage or the precomp.
    pub fn layer_source(&self, layer: &Layer<'f, 'a>) -> Option<Item<'f, 'a>> {
        self.item_by_id(layer.header()?.source_id?)
    }

    /// The name a layer shows: its own name, or its source's name when it has none.
    pub fn layer_display_name(&self, layer: &Layer<'f, 'a>) -> Option<String> {
        layer.name().or_else(|| self.layer_source(layer)?.display_name())
    }

    /// The pixel size of a layer's source: a footage item's width and height, or a precomp's
    /// composition size.
    pub fn layer_source_size(&self, layer: &Layer<'f, 'a>) -> Option<(u32, u32)> {
        let source = self.layer_source(layer)?;
        match source.kind()? {
            ItemKind::Footage => source.footage().map(|f| (f.width, f.height)),
            ItemKind::Composition => source.comp()?.header().map(|h| (u32::from(h.width), u32::from(h.height))),
            ItemKind::Folder | ItemKind::Other(_) => None,
        }
    }
}
