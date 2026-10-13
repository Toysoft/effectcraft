//! Text documents, markers and footage items, checked against the values the corpus was made
//! with (its MANIFEST).

mod common;

use common::{first_comp, fixture, layer_with_id, open};
use effectcraft_aep::{ItemKind, KeyframeKind, LayerType, Marker, Project, Property, TextDocument, TextValue, parse_text_dict};

const L: u32 = 0x1b;

fn text_layer_document(name: &str) -> TextDocument {
    let bytes = fixture(name);
    let file = open(&bytes);
    let project = Project::new(&file).unwrap();
    let comp = first_comp(&project);
    let layer = comp.layers().find(|l| l.name().as_deref() == Some("T")).expect("the text layer T");
    let h = layer.header().unwrap();
    assert_eq!(h.layer_type, LayerType::Text);
    assert!(h.name_is_user_set());
    layer.text_document().expect("text document")
}

#[test]
fn text_a_with_the_defaults() {
    // 110: point text "A": Times New Roman (TimesNewRomanPSMT), size 100, white fill.
    let d = text_layer_document("110-text-A.aep");
    assert_eq!(d.text, "A");
    assert_eq!(d.raw_text, "A\r");
    assert_eq!(d.font_size, 100.0);
    assert_eq!(d.fill_argb, [1.0, 1.0, 1.0, 1.0]);
    assert_eq!(d.fill_rgb(), [1.0, 1.0, 1.0]);
    assert_eq!(d.font.as_deref(), Some("TimesNewRomanPSMT"));
    assert!(d.fonts.iter().any(|f| f == "TimesNewRomanPSMT"));
}

#[test]
fn text_hello_size_60_orange() {
    // 113 (chain 111-113): text "Hello", fontSize 60, fillColor [1, 0.5, 0].
    let d = text_layer_document("113-text-Hello-size60-fillorange.aep");
    assert_eq!(d.text, "Hello");
    assert_eq!(d.font_size, 60.0);
    assert_eq!(d.fill_rgb(), [1.0, 0.5, 0.0]);
    assert_eq!(d.font.as_deref(), Some("TimesNewRomanPSMT"));
}

#[test]
fn a_solid_layer_has_no_text_document() {
    let bytes = fixture("000-base.aep");
    let file = open(&bytes);
    let project = Project::new(&file).unwrap();
    assert!(layer_with_id(&first_comp(&project), L).text_document().is_none());
}

#[test]
fn text_dictionary_parser() {
    // Names, numbers, booleans, nested dictionaries and arrays, a UTF-16BE string with an escaped
    // parenthesis, a string without a byte order mark.
    let mut doc = Vec::new();
    doc.extend_from_slice(b"/0 << /a 1 /b -2.5 /c true /d [ 1 2.0 /x ] >>\n/1 (");
    doc.extend_from_slice(&[0xfe, 0xff, 0x00, b'H', 0x00, 0x69, 0x00, 0x5c, b')', 0x00, 0x21]);
    doc.extend_from_slice(b")\n/2 (plain) /3 word");
    let top = parse_text_dict(&doc);
    assert_eq!(top.get("0").unwrap().get("a"), Some(&TextValue::Int(1)));
    assert_eq!(top.get("0").unwrap().get("b"), Some(&TextValue::Float(-2.5)));
    assert_eq!(top.get("0").unwrap().get("c"), Some(&TextValue::Bool(true)));
    let d = top.get("0").unwrap().get("d").unwrap();
    assert_eq!((d.at(0), d.at(1), d.at(2)), (Some(&TextValue::Int(1)), Some(&TextValue::Float(2.0)), Some(&TextValue::Name("x".to_string()))));
    // The byte after a backslash is kept literally: the units are 0x0048 0x0069 0x0029 0x0021.
    assert_eq!(top.get("1").and_then(TextValue::as_str), Some("Hi)!"));
    assert_eq!(top.get("2"), Some(&TextValue::Bytes(b"plain".to_vec())));
    assert_eq!(top.get("3"), Some(&TextValue::Token("word".to_string())));
}

#[test]
fn text_dictionary_parser_survives_garbage() {
    // Unbalanced brackets, a string that never ends, nothing, binary junk, 100 levels of nesting.
    for body in [&b"<< /a"[..], b"[ 1 2", b"(abc", b"(abc\\", b"", b">>", b")", b"/a <<", &[0xff, 0xfe, 0x00, 0x80, 0x28]] {
        let _ = parse_text_dict(body);
    }
    let deep = [b"/k ".as_slice(), &b"[".repeat(100), &b"]".repeat(100)].concat();
    let top = parse_text_dict(&deep);
    // Nesting beyond the limit stops the parse; nothing was read for the key.
    assert_eq!(top, TextValue::Dict(Vec::new()));
    let ok = [b"/k ".as_slice(), &b"[".repeat(10), &b"]".repeat(10)].concat();
    assert!(matches!(parse_text_dict(&ok), TextValue::Dict(p) if p.len() == 1));
    // A body that is not a text document yields no document.
    assert_eq!(TextDocument::parse(b"/0 1 /1 2"), None);
}

// ------------------------------------------------------------------------------------- markers

#[test]
fn comp_marker() {
    // 122: a composition marker at 1 s with the comment "CompMarker".
    let bytes = fixture("122-marker-comp-1s.aep");
    let file = open(&bytes);
    let project = Project::new(&file).unwrap();
    let comp = first_comp(&project);
    assert_eq!(comp.markers(), [Marker { time_units: 25600, seconds: Some(1.0), comment: "CompMarker".to_string() }]);
    // The times are 16-byte records of the property inside `mrst`.
    let mrst = comp.item().chunk().child_list(b"SecL").unwrap().find_list(b"mrst").unwrap();
    let keys = Property::from_chunk(mrst.child_list(b"tdbs").unwrap()).unwrap().keys().unwrap();
    assert_eq!((keys.len(), keys.header.record_size), (1, 16));
    assert!(keys.keyframes().all(|k| k.kind() == KeyframeKind::Marker));
    // The layer has none.
    assert!(layer_with_id(&comp, L).markers().is_empty());
}

#[test]
fn layer_marker() {
    // 123: a marker on L at 1 s, comment "LayerMarker".
    let bytes = fixture("123-marker-layer-1s.aep");
    let file = open(&bytes);
    let project = Project::new(&file).unwrap();
    let comp = first_comp(&project);
    assert_eq!(layer_with_id(&comp, L).markers(), [Marker { time_units: 25600, seconds: Some(1.0), comment: "LayerMarker".to_string() }]);
    assert!(comp.markers().is_empty());
}

#[test]
fn no_markers_in_the_base() {
    let bytes = fixture("000-base.aep");
    let file = open(&bytes);
    let project = Project::new(&file).unwrap();
    let comp = first_comp(&project);
    assert!(comp.markers().is_empty());
    assert!(layer_with_id(&comp, L).markers().is_empty());
}

// ------------------------------------------------------------------------------------- footage

#[test]
fn solid_footage_item() {
    // The base solid L: red [1,0,0], 1920x1080 (ARGB floats 1, 1, 0, 0).
    let bytes = fixture("000-base.aep");
    let file = open(&bytes);
    let project = Project::new(&file).unwrap();
    let solid = project.item_by_id(0x1a).unwrap();
    assert_eq!(solid.kind(), Some(ItemKind::Footage));
    let f = solid.footage().unwrap();
    assert!(f.is_solid);
    assert_eq!(f.file_type, "Soli");
    assert_eq!((f.width, f.height), (1920, 1080));
    assert_eq!(f.name.as_deref(), Some("L"));
    assert_eq!(f.solid_argb, Some([1.0, 1.0, 0.0, 0.0]));
    assert_eq!(f.solid_rgb(), Some([1.0, 0.0, 0.0]));
    assert_eq!(f.path, None);
    assert_eq!(solid.name(), None);
    assert_eq!(solid.label(), Some(1));
    // A composition is not footage.
    assert!(first_comp(&project).item().footage().is_none());
}

#[test]
fn png_footage_layer() {
    // 126: the png test8x8.png (8x8) imported and used for a layer "png" (label 5).
    let bytes = fixture("126-footage-png-layer.aep");
    let file = open(&bytes);
    let project = Project::new(&file).unwrap();
    let comp = first_comp(&project);
    let layer = comp.layers().find(|l| l.name().as_deref() == Some("png")).unwrap();
    let h = layer.header().unwrap();
    assert_eq!((h.label, h.layer_type), (5, LayerType::Av));
    let item = project.layer_source(&layer).unwrap();
    assert_eq!(item.kind(), Some(ItemKind::Footage));
    let f = item.footage().unwrap();
    assert!(!f.is_solid);
    assert_eq!(f.file_type, "png!");
    assert_eq!((f.width, f.height), (8, 8));
    assert_eq!(f.name.as_deref(), Some("test8x8.png"));
    assert_eq!(f.solid_argb, None);
    // The path comes from the alias JSON (the fixture holds a neutral replacement of the original).
    assert!(f.path.as_deref().is_some_and(|p| p.ends_with("test8x8.png")), "{:?}", f.path);
    assert_eq!(project.layer_source_size(&layer), Some((8, 8)));
    assert_eq!(project.layer_display_name(&layer).as_deref(), Some("png"));
    // L is still there below it.
    assert_eq!(comp.layers().count(), 2);
}
