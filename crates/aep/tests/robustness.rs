//! Hostile and damaged input: nothing may panic; whatever parses must write back identically.

mod common;

use common::fixture;
use effectcraft_aep::{AepError, parse, write};

/// Parses; on success, the invariant "parse then write is the identity" must hold.
fn check(bytes: &[u8]) -> Result<(), AepError> {
    let file = parse(bytes)?;
    let back = write(&file).map_err(|_| AepError::TooLarge)?;
    assert!(back == bytes, "round trip of an accepted input differs ({} bytes)", bytes.len());
    Ok(())
}

/// A small deterministic generator (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

#[test]
fn empty_and_tiny_inputs() {
    assert_eq!(check(&[]), Err(AepError::NotRifx));
    for n in 0..16 {
        let bytes = fixture("001-empty.aep");
        let _ = check(&bytes[..n]);
    }
}

#[test]
fn truncated_at_every_length() {
    let bytes = fixture("001-empty.aep");
    let declared = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize + 8;
    for len in 0..=bytes.len() {
        let r = check(&bytes[..len]);
        if len < declared {
            assert!(r.is_err(), "length {len} of {declared} must be rejected");
        } else {
            assert!(r.is_ok(), "length {len} (>= declared end) must parse: {r:?}");
        }
    }
}

#[test]
fn truncated_larger_fixture_at_sampled_lengths() {
    let bytes = fixture("090-mask-rect.aep");
    let declared = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize + 8;
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..400 {
        let len = rng.below(bytes.len() + 1);
        let r = check(&bytes[..len]);
        assert_eq!(r.is_ok(), len >= declared, "len {len}");
    }
}

#[test]
fn random_byte_flips() {
    for (name, iterations) in [("001-empty.aep", 4000), ("000-base.aep", 1500), ("110-text-A.aep", 600)] {
        let original = fixture(name);
        let mut rng = Rng(0x1234_5678_9abc_def1 ^ original.len() as u64);
        let (mut ok, mut err) = (0, 0);
        for _ in 0..iterations {
            let mut b = original.clone();
            for _ in 0..=rng.below(8) {
                let at = rng.below(b.len());
                b[at] = rng.next() as u8;
            }
            match check(&b) {
                Ok(()) => ok += 1,
                Err(_) => err += 1,
            }
        }
        assert_eq!(ok + err, iterations, "{name}");
    }
}

#[test]
fn flips_in_the_headers_and_sizes() {
    // Aim at the bytes that matter most: the first 200 bytes, every value of every byte.
    let original = fixture("001-empty.aep");
    for at in 0..200 {
        for v in [0u8, 1, 0x7f, 0x80, 0xff] {
            let mut b = original.clone();
            b[at] = v;
            let _ = check(&b);
        }
    }
}

#[test]
fn oversized_chunk_sizes() {
    let original = fixture("001-empty.aep");
    // Every chunk header's size field set to huge values, one at a time, at every 4-byte aligned
    // offset of the first 2 KB.
    for at in (12..2048).step_by(2) {
        for v in [u32::MAX, 0x8000_0000, 0x0100_0000, original.len() as u32, 1 << 20] {
            let mut b = original.clone();
            b[at..at + 4].copy_from_slice(&v.to_be_bytes());
            let _ = check(&b);
        }
    }
    // The container size itself.
    for v in [0u32, 3, 4, 5, u32::MAX, original.len() as u32] {
        let mut b = original.clone();
        b[4..8].copy_from_slice(&v.to_be_bytes());
        let _ = check(&b);
    }
}

#[test]
fn garbage_after_a_valid_header() {
    let mut rng = Rng(42);
    for _ in 0..2000 {
        let n = rng.below(200);
        let mut b = b"RIFX\0\0\0\0Egg!".to_vec();
        for _ in 0..n {
            b.push(rng.next() as u8);
        }
        let size = (b.len() - 8) as u32;
        b[4..8].copy_from_slice(&size.to_be_bytes());
        let _ = check(&b);
    }
}

// ----------------------------------------------------------------------------------- typed view

use effectcraft_aep::{AepFile, Body, Chunk, Comp, Layer, Project, PropGroup, PropNode, Property};

/// Touches the property accessors; none may panic on a damaged property.
fn exercise_property(p: &Property<'_, '_>) {
    let _ = (p.header(), p.dims(), p.is_animated(), p.static_value(), p.time_scale(), p.flags(), p.is_static_flag_set());
    let _ = (p.expression(), p.expression_flags().map(|f| f.all_set()), p.user_name(), p.first_key_value());
    if let Some(keys) = p.keys() {
        let scale = p.time_scale().unwrap_or(0);
        for k in keys.keyframes() {
            let _ = (k.time_units(), k.spatial_value(), k.raw().len(), k.kind(), k.seconds(scale), k.seconds(0));
            let _ = (k.interpolation_in(), k.interpolation_out(), k.flags(), k.label(), k.is_roving());
            let _ = (k.scalar_value(), k.ease(), k.spatial());
        }
    }
}

fn exercise_group(g: &PropGroup<'_, '_>, depth: usize) {
    if depth > 24 {
        return;
    }
    let _ = (g.flags(), g.names(), g.user_name());
    for e in g.entries() {
        match e.node {
            PropNode::Group(g) => exercise_group(&g, depth + 1),
            PropNode::Property(p) => exercise_property(&p),
            PropNode::Other(c) => {
                if let Some(g) = c.child_list(b"tdgp").and_then(PropGroup::from_chunk) {
                    exercise_group(&g, depth + 1);
                }
            }
        }
    }
}

fn exercise_layer(project: &Project<'_, '_>, comp_size: (u32, u32), layer: &Layer<'_, '_>) {
    let _ = (layer.name(), layer.is_three_d(), layer.id());
    if let Some(h) = layer.header() {
        let _ = (h.in_point(), h.out_point(), h.in_point_seconds(), h.out_point_seconds(), h.start_seconds(), h.stretch_percent());
        let _ = (h.is_shy(), h.is_locked(), h.is_solo(), h.is_three_d(), h.is_motion_blur(), h.is_adjustment(), h.is_guide());
        let _ = (h.is_null_object(), h.collapses_transformations(), h.name_is_user_set(), h.quality, h.blend_mode, h.layer_type);
    }
    let _ = (project.layer_source(layer).map(|i| i.id()), project.layer_display_name(layer), project.layer_source_size(layer));
    if let Some(t) = layer.transform(comp_size.0, comp_size.1) {
        let _ = (t.anchor_px(f64::from(comp_size.0), f64::from(comp_size.1)), t.position.value, t.opacity_percent.value);
    }
    if let Some(root) = layer.properties() {
        exercise_group(&root, 0);
    }
    for e in layer.effects() {
        let _ = (e.match_name().len(), e.name(), e.declared_param_count());
        for p in e.params() {
            let _ = (p.index, p.kind, p.name.len(), p.items.as_ref().map(Vec::len), p.pard().len(), p.value(), p.stored_value(), p.pard_default());
            if let Some(s) = p.stored() {
                exercise_property(&s);
            }
        }
    }
    for m in layer.masks() {
        let _ = (m.name, m.mode, m.inverted, m.feather, m.opacity_percent, m.expansion, m.path_animated, m.path_key_seconds.len());
        for s in &m.shapes {
            let _ = (s.vertex_count(), s.vertices((1920.0, 1080.0)).len(), s.vertices((f64::NAN, f64::INFINITY)).len());
        }
    }
    if let Some(t) = layer.text_document() {
        let _ = (t.fill_rgb(), t.fonts.len(), t.text.len(), t.raw_text.len(), t.font_size, t.font);
    }
    let _ = layer.markers();
}

fn exercise_comp(project: &Project<'_, '_>, comp: &Comp<'_, '_>) {
    let tb = comp.time_base();
    let _ = tb.map(|t| (t.frame_rate(), t.seconds(i32::MIN), t.seconds(i32::MAX)));
    let _ = (comp.name(), comp.renderer(), comp.markers());
    let mut size = (1920, 1080);
    if let Some(h) = comp.header() {
        size = (u32::from(h.width), u32::from(h.height));
        let _ = (h.frame_rate(), h.frame_rate_from_fixed(), h.duration_seconds(), h.work_area_start_seconds(), h.work_area_end_seconds());
        let _ = (h.background_rgb(), h.pixel_aspect_ratio(), h.pixel_aspect.reduced(), h.shutter_angle, h.motion_blur);
    }
    for layer in comp.layers() {
        exercise_layer(project, size, &layer);
    }
}

/// Touches every accessor of the typed view; none may panic on a damaged tree.
fn exercise(file: &AepFile<'_>) {
    let Some(project) = Project::new(file) else { return };
    for item in project.all_items() {
        let _ = (item.kind(), item.name(), item.id(), item.label(), item.used_item_ids(), item.footage(), item.display_name());
        let _ = item.id().and_then(|id| project.item_by_id(id));
        if let Some(comp) = item.comp() {
            exercise_comp(&project, &comp);
        }
    }
}

/// The fixtures whose typed fields are decoded, one per kind of chunk body.
const DECODED: &[&str] = &[
    "000-base.aep",
    "010-layer-name.aep",
    "022-layer-parent-null.aep",
    "030-comp-size-1280x720.aep",
    "035-comp-workarea-1-2.aep",
    "039-comp-renderer-advanced3d.aep",
    "044-static-anchor-10-20.aep",
    "052-key-position-2key-ease.aep",
    "054-key-position-2key-spatial-bezier.aep",
    "056-key-position-3key-roving-key2.aep",
    "058-key-opacity-2key.aep",
    "060-expr-opacity-wiggle.aep",
    "071-fx-slider-value77.aep",
    "073-fx-color-0.2-0.6-1.aep",
    "077-fx-dropdown-default.aep",
    "080-fx-dropshadow-default.aep",
    "091-mask-rect-props.aep",
    "093-mask-path-2keys.aep",
    "113-text-Hello-size60-fillorange.aep",
    "122-marker-comp-1s.aep",
    "123-marker-layer-1s.aep",
    "124-precomp-Pre.aep",
    "126-footage-png-layer.aep",
];

#[test]
fn typed_view_reads_every_undamaged_fixture() {
    for name in common::FIXTURES {
        let bytes = fixture(name);
        exercise(&parse(&bytes).unwrap());
    }
}

#[test]
fn typed_view_survives_damaged_files() {
    for (name, iterations) in [("000-base.aep", 1500), ("055-key-position-3key-inserted-t2.5.aep", 800), ("071-fx-slider-value77.aep", 800)] {
        let original = fixture(name);
        let mut rng = Rng(0xfeed_beef_1234_5678 ^ original.len() as u64);
        for _ in 0..iterations {
            let mut b = original.clone();
            for _ in 0..=rng.below(12) {
                let at = rng.below(b.len());
                b[at] = rng.next() as u8;
            }
            if let Ok(file) = parse(&b) {
                exercise(&file);
            }
        }
    }
}

#[test]
fn typed_view_survives_byte_flips_in_the_decoded_fixtures() {
    for name in DECODED {
        let original = fixture(name);
        let mut rng = Rng(0x0dd_ba11_cafe_f00d ^ original.len() as u64);
        let (mut parsed, mut total) = (0, 0);
        for _ in 0..250 {
            let mut b = original.clone();
            for _ in 0..=rng.below(16) {
                let at = rng.below(b.len());
                b[at] = rng.next() as u8;
            }
            total += 1;
            if let Ok(file) = parse(&b) {
                parsed += 1;
                exercise(&file);
            }
        }
        // Flips that land inside bodies keep the file parseable; sizes that break the structure do not.
        assert!(parsed > 0 && parsed <= total, "{name}: {parsed} of {total}");
    }
}

#[test]
fn truncated_typed_files_never_panic() {
    // A cut inside the container is an error; a cut in the trailer still parses. Either way no panic.
    for name in DECODED {
        let original = fixture(name);
        let declared = u32::from_be_bytes([original[4], original[5], original[6], original[7]]) as usize + 8;
        let mut rng = Rng(0x7e57_7e57 ^ original.len() as u64);
        for _ in 0..120 {
            let len = rng.below(original.len() + 1);
            if let Ok(file) = parse(&original[..len]) {
                assert!(len >= declared, "{name}: {len} < {declared}");
                exercise(&file);
            }
        }
        // Also cut right before and after the declared end.
        for len in [declared - 1, declared, declared + 1] {
            if let Ok(file) = parse(&original[..len.min(original.len())]) {
                exercise(&file);
            }
        }
    }
}

/// The chunks whose bodies the typed view decodes.
const DECODED_IDS: &[&[u8; 4]] = &[
    b"ldta", b"cdta", b"prin", b"idta", b"idpi", b"tdb4", b"cdat", b"tdsb", b"lhd3", b"ldat", b"mkif", b"shph", b"parn", b"pard", b"sspc", b"opti", b"alas",
    b"Utf8", b"tdmn", b"NmHd", b"btdk",
];

/// Applies a random mutation to a body.
fn damage(body: &mut Vec<u8>, rng: &mut Rng) {
    match rng.below(7) {
        0 => body.truncate(rng.below(body.len() + 1)),
        1 => body.clear(),
        2 => {
            for _ in 0..=rng.below(4) {
                if !body.is_empty() {
                    let at = rng.below(body.len());
                    body[at] = rng.next() as u8;
                }
            }
        }
        3 => {
            // A 4-byte window set to an extreme value (sizes, counts, offsets).
            if body.len() >= 4 {
                let at = rng.below(body.len() - 3);
                let v = [0xffff_ffffu32, 0x8000_0000, 0x7fff_ffff, 0, 1][rng.below(5)];
                body[at..at + 4].copy_from_slice(&v.to_be_bytes());
            }
        }
        4 => body.extend((0..=rng.below(64)).map(|_| rng.next() as u8)),
        5 => {
            // An 8-byte window set to NaN / infinity / a huge double.
            if body.len() >= 8 {
                let at = rng.below(body.len() - 7);
                let v = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e308, -1e308][rng.below(5)];
                body[at..at + 8].copy_from_slice(&v.to_be_bytes());
            }
        }
        _ => body.iter_mut().for_each(|b| *b = 0xff),
    }
}

fn damage_tree(chunks: &mut [Chunk<'static>], rng: &mut Rng, depth: usize) {
    if depth > 70 {
        return;
    }
    for chunk in chunks {
        let id = chunk.id;
        match &mut chunk.body {
            Body::Data(d) => {
                if DECODED_IDS.contains(&&id) && rng.below(12) == 0 {
                    damage(d.to_mut(), rng);
                }
            }
            Body::OpaqueList { data, .. } => {
                if rng.below(3) == 0 {
                    damage(data.to_mut(), rng);
                }
            }
            Body::Chunks(c) | Body::List { children: c, .. } => damage_tree(c, rng, depth + 1),
        }
    }
}

#[test]
fn typed_view_survives_damaged_chunk_bodies() {
    // Damage the bodies the typed view decodes (not the container framing), directly in the tree:
    // wrong sizes, empty and truncated bodies, extreme counts, NaN doubles, unterminated text.
    for name in DECODED {
        let bytes = fixture(name);
        let original = parse(&bytes).unwrap().into_owned();
        let mut rng = Rng(0xbad_c0de_0001 ^ bytes.len() as u64);
        for _ in 0..120 {
            let mut file = original.clone();
            damage_tree(&mut file.chunks, &mut rng, 0);
            exercise(&file);
        }
    }
}

#[test]
fn extreme_values_in_the_decoded_headers() {
    use effectcraft_aep::{CompHeader, LayerHeader, Rational, ShapePath};
    // Every field at its maximum: no overflow, no division by zero, no panic.
    let h = LayerHeader::parse(&[0xff; 164]).unwrap();
    let _ = (h.in_point(), h.out_point(), h.in_point_seconds(), h.out_point_seconds(), h.start_seconds(), h.stretch_percent());
    let mut zero_den = [0u8; 164];
    zero_den[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
    zero_den[12..16].copy_from_slice(&i32::MIN.to_be_bytes());
    let h = LayerHeader::parse(&zero_den).unwrap();
    assert_eq!((h.in_point(), h.out_point(), h.start_seconds()), (None, None, None));
    let mut wide = [0u8; 164];
    wide[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
    wide[12..16].copy_from_slice(&i32::MIN.to_be_bytes());
    wide[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
    wide[20..24].copy_from_slice(&i32::MAX.to_be_bytes());
    wide[24..28].copy_from_slice(&u32::MAX.to_be_bytes());
    let h = LayerHeader::parse(&wide).unwrap();
    let _ = (h.in_point(), h.in_point_seconds());
    let c = CompHeader::parse(&[0xff; 204]).unwrap();
    assert_eq!((c.duration_seconds(), c.work_area_start_seconds(), c.work_area_end_units), (None, None, None));
    let _ = (c.frame_rate(), c.frame_rate_from_fixed(), c.background_rgb(), c.pixel_aspect_ratio());
    let zero = CompHeader::parse(&[0; 204]).unwrap();
    assert_eq!((zero.frame_rate(), zero.pixel_aspect_ratio(), zero.duration_seconds()), (None, None, None));
    assert_eq!(Rational { num: i64::MAX, den: i64::MIN }.reduced(), None);
    assert_eq!(Rational { num: i64::MIN, den: i64::MIN }.reduced(), Rational::new(1, 1));
    // A path whose point count is not a multiple of three, and non-finite box and scale.
    let path = ShapePath { bbox: [f32::NAN, f32::INFINITY, f32::MIN, f32::MAX], closed_flag: 9, open: true, points: vec![[f32::NAN, 0.0]; 7] };
    assert_eq!(path.vertex_count(), 2);
    assert_eq!(path.vertices((f64::NAN, f64::MAX)).len(), 2);
}
