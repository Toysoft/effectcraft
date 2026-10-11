use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};

use super::gallery::{self, Format};
use super::*;
use crate::config::{ConfigStore, MemoryConfig};
use crate::project::LayerId;

fn tmp(name: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!("ec-console-effect-{}-{}-{name}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn id(name: &str) -> String {
    crate::effects::lookup(name).unwrap_or_else(|| panic!("no effect {name}")).id.to_string()
}

fn fx(name: &str) -> Target {
    Target::Effect(id(name))
}

/// A comp with one selected solid layer.
fn session() -> (Session, u64) {
    let mut s = Session { config: Some(Arc::new(MemoryConfig::default())), ..Default::default() };
    s.execute("comp.new", json!({"name": "Main", "width": 160, "height": 90, "duration": 2})).unwrap();
    let l = s.execute("layer.newSolid", json!({"name": "Plate", "color": "#336699"})).unwrap()["layer"].as_u64().unwrap();
    (s, l)
}

fn effects_on(s: &Session, l: u64) -> Vec<String> {
    let layer = s.active_comp().unwrap().layer(LayerId(l)).unwrap();
    layer.effects().map(|fx| fx.groups().map(|g| g.match_id.clone()).collect()).unwrap_or_default()
}

fn names(rows: &[Hit]) -> Vec<&str> {
    rows.iter().map(|h| h.name.as_str()).collect()
}

fn find(s: &Session, settings: &Settings, q: &str) -> Results {
    search(&Catalog::build(s, settings, None), settings, q, 40)
}

#[test]
fn search_ranks_matches_favorites_first_and_hides_the_blacklist() {
    let (s, _) = session();
    let mut st = Settings::default();
    // The best match of a plain search is the closest name.
    let r = find(&s, &st, "corner pin");
    assert_eq!(r.effects.first().map(|h| h.name.as_str()), Some("Corner Pin"), "{:?}", names(&r.effects));
    // An alias on a favorite: typing "corner pin" finds CC Power Pin first.
    st.favorite_mut(&fx("CC Power Pin")).unwrap().alias = "corner pin, cpp".into();
    let r = find(&s, &st, "corner pin");
    assert_eq!(names(&r.effects)[..2], ["CC Power Pin", "Corner Pin"]);
    assert_eq!(r.effects[0].alias.as_deref(), Some("corner pin"));
    assert_eq!(r.effects[0].favorite, Some(0));
    // Blacklisted: gone from the results.
    st.set_blacklisted(&fx("Corner Pin"), true);
    assert!(!names(&find(&s, &st, "corner pin").effects).contains(&"Corner Pin"));
    // Fractal is the exact match until Fractal Noise is a favorite.
    assert_eq!(find(&s, &st, "fractal").effects[0].name, "Fractal");
    st.set_favorite(&fx("Fractal Noise"), true, None);
    assert_eq!(find(&s, &st, "fractal").effects[0].name, "Fractal Noise");
    // Favorites rank in their order (their priority), and reordering changes it.
    st.set_favorite(&fx("Levels"), true, None);
    st.set_favorite(&fx("Leave Color"), true, None);
    let r = find(&s, &st, "le");
    assert_eq!(names(&r.effects)[..2], ["Levels", "Leave Color"]);
    st.set_favorite(&fx("Leave Color"), true, Some(0));
    assert_eq!(names(&find(&s, &st, "le").effects)[..2], ["Leave Color", "Levels"]);
    // A favorite matching only loosely doesn't jump over strong matches.
    let r = find(&s, &st, "gaussian");
    assert_eq!(r.effects[0].name, "Gaussian Blur");
    // Initials and letters run together.
    assert!(names(&find(&s, &st, "gb").effects).contains(&"Gaussian Blur"));
    assert_eq!(find(&s, &st, "4color").effects[0].name, "4-Color Gradient");
    // Presets have their own section, after the effects; the option leaves them out.
    let r = find(&s, &st, "typewriter");
    assert!(r.effects.is_empty());
    assert_eq!(r.presets[0].target, Target::TextPreset("typewriter".into()));
    assert_eq!(r.get(0), r.presets.first());
    st.presets = false;
    assert!(find(&s, &st, "typewriter").presets.is_empty());
    // An empty search lists nothing; browse shows the shortcuts and favorites.
    assert!(find(&s, &st, "   ").is_empty());
    st.set_shortcut(2, Some(Shortcut { target: fx("CC Page Turn"), label: "Turn".into() })).unwrap();
    let b = browse(&Catalog::build(&s, &st, None), &st);
    assert_eq!(b.shortcuts.len(), SLOTS);
    let two = b.shortcuts[1].as_ref().unwrap();
    assert_eq!((two.label.as_deref(), two.name.as_str(), two.shortcut), (Some("Turn"), "CC Page Turn", Some(2)));
    assert_eq!(names(&b.favorites), ["Leave Color", "CC Power Pin", "Fractal Noise", "Levels"]);
    // Limits and highlight ranges.
    assert_eq!(search(&Catalog::build(&s, &st, None), &st, "e", 3).effects.len(), 3);
    let blur = find(&s, &st, "blur");
    let h = blur.effects.iter().find(|h| h.name == "Gaussian Blur").unwrap();
    assert_eq!(h.range.clone().map(|r| &h.name[r]), Some("Blur"));
}

#[test]
fn preset_files_and_their_targets_are_searchable() {
    let (mut s, _) = session();
    let dir = tmp("presets");
    std::fs::write(dir.join("Glow Pop.ecpreset"), "{}").unwrap();
    std::fs::write(dir.join("notes.txt"), "").unwrap();
    let elsewhere = tmp("elsewhere").join("Bounce.ecpreset");
    std::fs::write(&elsewhere, "{}").unwrap();
    s.console_effect.set_favorite(&Target::Preset(elsewhere.to_string_lossy().to_string()), true, None);
    let cat = Catalog::build(&s, &s.console_effect, Some(&dir));
    let r = search(&cat, &s.console_effect, "glow pop", 10);
    assert_eq!(r.presets.first().map(|h| h.name.as_str()), Some("Glow Pop"));
    assert_eq!(r.presets[0].category, "Animation Presets");
    // A favorite preset outside the presets folder is found too.
    assert_eq!(search(&cat, &s.console_effect, "bounce", 10).presets.first().map(|h| h.name.as_str()), Some("Bounce"));
    assert!(search(&cat, &s.console_effect, "notes", 10).presets.is_empty());
    assert_eq!(preset_name("/a/b/Zoom In.ecpreset"), "Zoom In");
}

#[test]
fn apply_runs_effects_presets_overrides_and_shortcut_slots() {
    let (mut s, l) = session();
    s.execute_checked("consoleEffect.apply", json!({"effect": "Gaussian Blur"})).unwrap();
    assert_eq!(effects_on(&s, l), [id("Gaussian Blur")]);
    // By search: its best match.
    let r = s.execute_checked("consoleEffect.apply", json!({"query": "power pin"})).unwrap();
    assert_eq!(r["name"], "CC Power Pin");
    assert!(s.execute("consoleEffect.apply", json!({"query": "zzzzqqq"})).is_err());
    s.execute("effect.removeAll", json!({})).unwrap();

    // An override: Fractal Noise saved with other settings as a preset, applied in its place.
    s.execute("consoleEffect.apply", json!({"effect": "Fractal Noise"})).unwrap();
    s.execute("prop.set", json!({"layer": l, "path": "effects/#1/contrast", "value": 250})).unwrap();
    s.execute("prop.set", json!({"layer": l, "path": "effects/#1/invert", "value": true})).unwrap();
    let preset = tmp("override").join("Fractal Noise Default.ecpreset").to_string_lossy().to_string();
    let saved = s.execute("anim.savePreset", json!({"path": preset})).unwrap();
    assert_eq!(saved["effects"], 1, "{saved}");
    s.execute("effect.removeAll", json!({})).unwrap();
    s.execute_checked("consoleEffect.override", json!({"effect": "Fractal Noise", "overridePreset": preset})).unwrap();
    let r = s.execute("consoleEffect.apply", json!({"effect": "Fractal Noise"})).unwrap();
    assert_eq!((r["override"].as_bool(), r["applied"]["preset"].as_str()), (Some(true), Some(preset.as_str())), "{r}");
    let contrast = |s: &Session| {
        let layer = s.active_comp().unwrap().layer(LayerId(l)).unwrap().clone();
        let g = layer.effects().unwrap().groups().find(|g| g.match_id == id("Fractal Noise")).unwrap().clone();
        g.get("contrast").unwrap().value.as_f64()
    };
    assert_eq!(contrast(&s), 250.0, "the override's settings");
    // The search shows the override.
    let found = s.execute("consoleEffect.search", json!({"query": "fractal noise"})).unwrap();
    assert_eq!(found["effects"][0]["override"], preset.as_str());
    // A missing override file: the effect itself, with a warning.
    std::fs::remove_file(&preset).unwrap();
    s.execute("effect.removeAll", json!({})).unwrap();
    let r = s.execute("consoleEffect.apply", json!({"effect": "Fractal Noise"})).unwrap();
    assert_eq!(r["override"], false);
    assert!(r["warning"].as_str().unwrap().contains("missing"));
    assert_eq!(contrast(&s), 100.0, "the effect's defaults");
    // Only presets can be overrides.
    let not = tmp("not").join("x.ecpreset");
    std::fs::write(&not, "{\"name\": 1}").unwrap();
    assert!(s.execute("consoleEffect.override", json!({"effect": "Levels", "overridePreset": not.to_string_lossy()})).is_err());
    s.execute("consoleEffect.override", json!({"effect": "Fractal Noise", "overridePreset": null})).unwrap();
    assert_eq!(s.console_effect.favorite(&fx("Fractal Noise")).unwrap().preset_override, None);

    // Shortcut slots.
    s.execute("effect.removeAll", json!({})).unwrap();
    s.execute_checked("consoleEffect.shortcut", json!({"slot": 3, "effect": "CC Page Turn", "label": "Turn"})).unwrap();
    s.execute("consoleEffect.apply", json!({"slot": 3})).unwrap();
    assert_eq!(effects_on(&s, l), [id("CC Page Turn")]);
    assert!(s.execute("consoleEffect.apply", json!({"slot": 4})).is_err(), "empty slot");
    // Without a slot: the first empty one.
    let r = s.execute("consoleEffect.shortcut", json!({"effect": "Glow"})).unwrap();
    assert_eq!(r["slot"], 1);
    // Relabel keeps the target; move and clear.
    s.execute("consoleEffect.shortcut", json!({"slot": 1, "label": "Shine"})).unwrap();
    assert_eq!(s.console_effect.shortcut(1).unwrap().shown(), "Shine");
    s.execute("consoleEffect.shortcut", json!({"slot": 3, "moveTo": 9})).unwrap();
    assert_eq!(s.console_effect.shortcut(9).unwrap().target, fx("CC Page Turn"));
    assert!(s.console_effect.shortcut(3).is_none());
    s.execute("consoleEffect.shortcut", json!({"slot": 9, "clear": true})).unwrap();
    assert!(s.console_effect.shortcut(9).is_none());
    for bad in [json!({"slot": 0, "effect": "Glow"}), json!({"slot": 10, "effect": "Glow"}), json!({"slot": 5}), json!({"slot": 1, "moveTo": 12})] {
        assert!(s.execute("consoleEffect.shortcut", bad.clone()).is_err(), "{bad}");
    }

    // Text presets go on text layers.
    assert!(s.execute("consoleEffect.apply", json!({"textPreset": "typewriter"})).is_err());
    let t = s.execute("layer.newText", json!({"text": "Hello"})).unwrap()["layer"].as_u64().unwrap();
    s.execute("layer.select", json!({"layers": [t]})).unwrap();
    s.execute("consoleEffect.apply", json!({"textPreset": "Typewriter"})).unwrap();
    let text = s.active_comp().unwrap().layer(LayerId(t)).unwrap().clone();
    assert!(text.props.group("text/animators").is_some_and(|a| !a.children.is_empty()));
}

#[test]
fn what_the_console_learns_persists_and_hostile_settings_load_safely() {
    let store = Arc::new(MemoryConfig::default());
    let mut s = Session { config: Some(store.clone()), ..Default::default() };
    s.execute_checked("consoleEffect.favorite", json!({"effect": "Levels"})).unwrap();
    s.execute_checked("consoleEffect.alias", json!({"effect": "CC Power Pin", "alias": "corner pin"})).unwrap();
    s.execute_checked("consoleEffect.blacklist", json!({"effect": "Corner Pin"})).unwrap();
    s.execute_checked("consoleEffect.shortcut", json!({"slot": 2, "effect": "Glow", "label": "G"})).unwrap();
    s.execute_checked("consoleEffect.setSettings", json!({"color": "#FF8800", "snapshotFormat": "jpeg", "jpegQuality": 80, "maxResults": 25})).unwrap();
    let rev = s.console_effect_revision;
    // Toggling: a favorite again is no favorite; the blacklist and favorites exclude each other.
    s.execute("consoleEffect.favorite", json!({"effect": "Levels"})).unwrap();
    assert!(s.console_effect.favorite(&fx("Levels")).is_none());
    assert!(s.console_effect_revision > rev);
    s.execute("consoleEffect.favorite", json!({"effect": "Corner Pin", "value": true})).unwrap();
    assert!(!s.console_effect.is_blacklisted(&fx("Corner Pin")));
    s.execute("consoleEffect.blacklist", json!({"effect": "Corner Pin", "value": true})).unwrap();
    assert!(s.console_effect.favorite(&fx("Corner Pin")).is_none());
    let mut again = Session { config: Some(store.clone()), ..Default::default() };
    again.load_settings();
    assert_eq!(again.console_effect, s.console_effect);
    assert_eq!((again.console_effect.color.as_str(), again.console_effect.snapshot_format.as_str(), again.console_effect.jpeg_quality), ("#ff8800", "jpg", 80));
    assert_eq!(again.execute("consoleEffect.settings", json!({})).unwrap()["settings"]["shortcuts"][1]["label"], "G");

    // Bad options are refused.
    for bad in [json!({"color": "orange"}), json!({"snapshotFormat": "gif"})] {
        assert!(s.execute("consoleEffect.setSettings", bad.clone()).is_err(), "{bad}");
    }
    for bad in [json!({}), json!({"effect": "No Such Effect"}), json!({"textPreset": "nope"}), json!({"preset": ""}), json!({"effect": 5})] {
        assert!(s.execute("consoleEffect.favorite", bad.clone()).is_err(), "{bad}");
    }

    // A hand-edited file: bad rows go, the rest stays; values come back into range.
    let shortcuts: Vec<Value> = (0..20)
        .map(|i| if i == 1 { json!({"target": {"nope": 1}}) } else { json!({"target": {"effect": "ec.blur.gaussian"}, "label": "x".repeat(500)}) })
        .collect();
    let text = json!({
        "favorites": [{"target": {"effect": "ec.blur.gaussian"}, "alias": "y".repeat(5000)}, {"bogus": 1}, 5, {"target": {"effect": "ec.blur.gaussian"}}],
        "blacklist": [{"effect": "ec.blur.gaussian"}, "nope"],
        "shortcuts": shortcuts,
        "color": "red",
        "maxResults": 99_999_999_999u64,
        "jpegQuality": 0,
        "snapshotFormat": "tiff",
    })
    .to_string();
    let (st, repairs) = Settings::from_json(&text);
    assert_eq!(st.favorites.len(), 1, "duplicates and bad rows dropped");
    assert_eq!(st.favorites[0].alias.chars().count(), 256);
    assert!(st.blacklist.is_empty(), "a favorite is never blacklisted");
    assert_eq!(st.shortcuts.len(), SLOTS);
    assert!(st.shortcuts[1].is_none());
    assert_eq!(st.shortcuts[0].as_ref().unwrap().label.chars().count(), 64);
    assert_eq!((st.color.as_str(), st.max_results, st.jpeg_quality, st.snapshot_format.as_str()), ("", 500, 1, "png"));
    assert!(repairs.len() >= 4, "{repairs:?}");
    for junk in ["", "not json", "[]", "null", "{\"favorites\": {\"a\": 1}}", "{\"shortcuts\": 7}"] {
        let (st, _) = Settings::from_json(junk);
        assert_eq!(st.shortcuts.len(), SLOTS, "{junk}");
    }
    // Loading a corrupt file never crashes and keeps the defaults.
    store.write(SETTINGS_FILE, "{{{{").unwrap();
    let mut broken = Session { config: Some(store.clone()), ..Default::default() };
    broken.load_settings();
    assert_eq!(broken.console_effect, Settings::default());
}

#[test]
fn settings_files_export_import_and_merge() {
    let store = Arc::new(MemoryConfig::default());
    let mut s = Session { config: Some(store.clone()), ..Default::default() };
    s.execute("consoleEffect.favorite", json!({"effect": "Levels"})).unwrap();
    s.execute("consoleEffect.shortcut", json!({"slot": 1, "effect": "Glow"})).unwrap();
    let file = tmp("settings").join("fx.json").to_string_lossy().to_string();
    s.execute_checked("consoleEffect.exportSettings", json!({"path": file})).unwrap();
    let exported = s.console_effect.clone();
    s.execute("consoleEffect.setSettings", json!({"reset": true})).unwrap();
    s.execute("consoleEffect.favorite", json!({"effect": "Curves"})).unwrap();
    // Merge: theirs are added after mine.
    s.execute_checked("consoleEffect.importSettings", json!({"path": file, "merge": true})).unwrap();
    assert_eq!(names(&browse(&Catalog::build(&s, &s.console_effect, None), &s.console_effect).favorites), ["Curves", "Levels"]);
    assert_eq!(s.console_effect.shortcut(1).map(|sc| sc.target.clone()), Some(fx("Glow")));
    // Replace.
    s.execute("consoleEffect.importSettings", json!({"path": file})).unwrap();
    assert_eq!(s.console_effect, exported);
    // Something that isn't settings changes nothing.
    let junk = tmp("junk").join("junk.json");
    for text in ["[1, 2]", "not json", "\u{0}\u{1}"] {
        std::fs::write(&junk, text).unwrap();
        assert!(s.execute("consoleEffect.importSettings", json!({"path": junk.to_string_lossy()})).is_err(), "{text}");
        assert_eq!(s.console_effect, exported);
    }
    assert!(s.execute("consoleEffect.importSettings", json!({"path": "/no/such/file.json"})).is_err());
}

#[test]
fn snapshots_fill_the_gallery_and_come_back_out() {
    let (mut s, _) = session();
    s.execute("layer.newSolid", json!({"name": "Small", "color": "#ff0000", "width": 40, "height": 20})).unwrap();
    // Only the small solid shows: the corners are transparent.
    s.execute("layer.setSwitch", json!({"layers": ["Plate"], "switch": "video", "value": false})).unwrap();
    let dir = tmp("gallery");
    s.execute("consoleEffect.setSettings", json!({"galleryFolder": dir.to_string_lossy()})).unwrap();
    let a = s.execute_checked("consoleEffect.snapshot", json!({"scale": 0.5, "alpha": true})).unwrap();
    assert_eq!((a["width"].as_u64(), a["height"].as_u64(), a["alpha"].as_bool()), (Some(80), Some(45), Some(true)), "{a}");
    let png = std::fs::read(a["path"].as_str().unwrap()).unwrap();
    let (w, h, rgba) = gallery::decode(&png).unwrap();
    assert_eq!((w, h), (80, 45));
    assert_eq!(rgba[3], 0, "a transparent corner");
    assert_eq!(&rgba[((22 * w + 40) * 4) as usize..][..4], [255, 0, 0, 255], "the red solid in the middle");
    // Without alpha the comp's background fills in; JPEG from the settings.
    s.execute("consoleEffect.setSettings", json!({"snapshotFormat": "jpg"})).unwrap();
    let b = s.execute("consoleEffect.snapshot", json!({})).unwrap();
    assert_eq!((b["format"].as_str(), b["alpha"].as_bool()), (Some("jpg"), Some(false)));
    let (_, _, rgba) = gallery::decode(&std::fs::read(b["path"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(rgba[3], 255);
    assert_ne!(a["name"], b["name"], "every snapshot gets its own name");
    let listed = s.execute("consoleEffect.gallery", json!({})).unwrap();
    let shots: Vec<&str> = listed["snapshots"].as_array().unwrap().iter().map(|x| x["name"].as_str().unwrap()).collect();
    assert_eq!(shots.len(), 2);
    assert!(shots.contains(&a["name"].as_str().unwrap()) && shots.contains(&b["name"].as_str().unwrap()));
    // Save a frame elsewhere: the format from the extension, or added.
    let out = tmp("frames");
    let f = s.execute_checked("consoleEffect.saveFrame", json!({"path": out.join("frame").to_string_lossy(), "format": "jpg"})).unwrap();
    assert!(f["path"].as_str().unwrap().ends_with("frame.jpg") && out.join("frame.jpg").is_file());
    s.execute("consoleEffect.saveFrame", json!({"path": out.join("alpha.png").to_string_lossy(), "alpha": true})).unwrap();
    assert_eq!(gallery::decode(&std::fs::read(out.join("alpha.png")).unwrap()).unwrap().2[3], 0);
    assert_eq!(s.execute("consoleEffect.gallery", json!({})).unwrap()["snapshots"].as_array().unwrap().len(), 2, "saved frames stay out of the gallery");
    // Export (converted) and reveal.
    let export = tmp("export");
    let r = s.execute_checked("consoleEffect.exportSnapshots", json!({"names": [a["name"]], "folder": export.to_string_lossy(), "format": "jpg"})).unwrap();
    let written = r["written"][0].as_str().unwrap();
    assert!(written.ends_with(".jpg") && std::path::Path::new(written).is_file());
    let (_, _, flat) = gallery::decode(&std::fs::read(written).unwrap()).unwrap();
    assert!(flat[0] > 240 && flat[3] == 255, "transparency exported over white");
    s.drain_events();
    s.execute("consoleEffect.revealGallery", json!({"name": a["name"]})).unwrap();
    assert!(s.drain_events().iter().any(|e| matches!(e, crate::Event::OpenUrl(u) if u.starts_with("file://"))));
    // Names never reach outside the gallery.
    let outside = tmp("outside").join("keep.png");
    std::fs::write(&outside, b"x").unwrap();
    for bad in
        [json!(["../keep.png"]), json!([outside.to_string_lossy()]), json!(["..\\keep.png"]), json!(["keep.txt"]), json!([""]), json!([]), json!("C:keep.png")]
    {
        assert!(s.execute("consoleEffect.deleteSnapshots", json!({"names": bad})).is_err(), "{bad}");
    }
    assert!(outside.is_file());
    let many: Vec<String> = (0..2000).map(|i| format!("{i}.png")).collect();
    assert!(s.execute("consoleEffect.deleteSnapshots", json!({"names": many})).is_err());
    let r = s.execute_checked("consoleEffect.deleteSnapshots", json!({"names": [a["name"], b["name"]]})).unwrap();
    assert_eq!(r["deleted"], 2);
    assert!(s.execute("consoleEffect.gallery", json!({})).unwrap()["snapshots"].as_array().unwrap().is_empty());
}

#[test]
fn snapshot_names_and_dates() {
    assert_eq!(gallery::civil(0), (1970, 1, 1, 0, 0, 0));
    assert_eq!(gallery::civil(951_782_400), (2000, 2, 29, 0, 0, 0));
    assert_eq!(gallery::civil(1_791_755_703), (2026, 10, 11, 21, 55, 3));
    assert_eq!(gallery::civil(4_107_585_659), (2100, 3, 1, 12, 0, 59));
    let _ = gallery::civil(u64::MAX);
    assert_eq!(gallery::stem("Main", 12, 1_791_755_703), "Main 00012 2026-10-11 21.55.03");
    assert_eq!(gallery::stem("a/b:c*\u{7}", 1, 0), "a_b_c__ 00001 1970-01-01 00.00.00");
    assert!(gallery::stem("...", -3, 0).starts_with("Comp -0003"));
    assert_eq!(Format::of_path("x/Shot.JPEG"), Some(Format::Jpg));
    assert_eq!(Format::of_path("x/Shot.gif"), None);
    // Encoding refuses a buffer that doesn't fit the size.
    assert!(gallery::encode(&[0; 7], 1, 2, Format::Png, 90).is_err());
    assert!(gallery::encode(&[], 0, 0, Format::Jpg, 90).is_err());
    assert!(gallery::decode(b"not an image").is_err());
}

#[test]
fn every_console_command_reports_instead_of_panicking() {
    let (mut s, _) = session();
    let ids: Vec<&str> = crate::command_specs().iter().map(|c| c.id).filter(|i| i.starts_with("consoleEffect.")).collect();
    assert!(ids.len() >= 20, "{ids:?}");
    let junk = [json!({}), json!(null), json!({"effect": [1]}), json!({"slot": -1}), json!({"names": {"a": 1}}), json!({"path": 5, "folder": [], "query": 7})];
    for id in ids {
        for p in &junk {
            // (The last-resort guard would turn a panic into this error.)
            if let Err(e) = s.execute(id, p.clone()) {
                assert!(!e.to_string().contains("internal error"), "{id} {p}: {e}");
            }
        }
    }
}
