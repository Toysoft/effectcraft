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

/// Touches every accessor of the typed view; none may panic on a damaged tree.
fn exercise(file: &effectcraft_aep::AepFile<'_>) {
    use effectcraft_aep::{PropGroup, PropNode};
    fn group(g: &PropGroup<'_, '_>, depth: usize) {
        if depth > 24 {
            return;
        }
        let _ = (g.flags(), g.names());
        for e in g.entries() {
            match e.node {
                PropNode::Group(g) => group(&g, depth + 1),
                PropNode::Property(p) => {
                    let _ = (p.header(), p.dims(), p.is_animated(), p.static_value());
                    if let Some(keys) = p.keys() {
                        for k in keys.keyframes() {
                            let _ = (k.time_units(), k.spatial_value(), k.raw().len());
                        }
                    }
                }
                PropNode::Other(c) => {
                    if let Some(g) = c.child_list(b"tdgp").and_then(PropGroup::from_chunk) {
                        group(&g, depth + 1);
                    }
                }
            }
        }
    }
    let Some(project) = effectcraft_aep::Project::new(file) else { return };
    for item in project.all_items() {
        let _ = (item.kind(), item.name());
        if let Some(comp) = item.comp() {
            let tb = comp.time_base();
            let _ = tb.map(|t| (t.frame_rate(), t.seconds(i32::MIN), t.seconds(i32::MAX)));
            for layer in comp.layers() {
                let _ = (layer.name(), layer.is_three_d());
                if let Some(root) = layer.properties() {
                    group(&root, 0);
                }
            }
        }
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
