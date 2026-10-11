//! FX Console commands (`fxConsole.*`): search and apply effects and presets the way the console
//! ranks them, teach it favorites, aliases, overrides, a blacklist and nine shortcuts, take
//! snapshots and manage their gallery (see [`crate::fx_console`]). The console itself (Edit ▸ FX
//! Console…, `app.fxConsole`), its gallery window, its settings dialog and copying a frame to the
//! clipboard are the frontend's.

use effectcraft_project::LayerSource;
use serde_json::{Value, json};

use super::{CommandSpec, always, b_p, bad, comp_id, f_p, frontend, has_comp, has_layers, layers_p, str_p};
use crate::fx_console::gallery::{self, Format};
use crate::fx_console::{self, Catalog, Settings, Shortcut, Target, valid_color};
use crate::{EngineError, Result, Session, cmd, query};

/// Most rows a search returns per section.
const MAX_LIMIT: usize = 500;
/// Most snapshot names one command takes.
const MAX_NAMES: usize = 1024;

/// The target a command names (`effect`, `textPreset` or `preset`).
fn target(p: &Value, cmd: &str) -> Result<Target> {
    match Target::from_params(p) {
        Some(Ok(t)) => Ok(t),
        Some(Err(e)) => Err(bad(cmd, e)),
        None => Err(bad(cmd, "name what: `effect` (id or name), `textPreset` (id or name) or `preset` (a .ecpreset file)")),
    }
}

/// Everything the console can apply right now.
fn catalog(s: &Session) -> Catalog {
    Catalog::build(s, &s.fx_console, fx_console::presets_dir().as_deref())
}

/// Run another command's implementation (this command is the one journalled).
fn run(s: &mut Session, id: &str, p: Value) -> Result<Value> {
    let spec = super::find(id).ok_or_else(|| EngineError::UnknownCommand(id.to_string()))?;
    (spec.run)(s, &p)
}

fn usize_p(p: &Value, k: &str) -> Option<usize> {
    p.get(k).and_then(Value::as_u64).map(|n| usize::try_from(n).unwrap_or(usize::MAX))
}

/// `names`: one snapshot file name or a list of them.
fn names_p(p: &Value, cmd: &str) -> Result<Vec<String>> {
    let names: Vec<String> = match p.get("names").or_else(|| p.get("name")) {
        Some(Value::String(n)) => vec![n.clone()],
        Some(Value::Array(a)) => a.iter().map(|v| v.as_str().map(str::to_string).ok_or_else(|| bad(cmd, "`names` are file names"))).collect::<Result<_>>()?,
        _ => return Err(bad(cmd, "missing `names` (snapshot file names from fxConsole.gallery)")),
    };
    if names.is_empty() || names.len() > MAX_NAMES {
        return Err(bad(cmd, format!("give 1–{MAX_NAMES} snapshot names")));
    }
    Ok(names)
}

fn save(s: &mut Session) -> Result<()> {
    fx_console::store(s)
}

// ---------------------------------------------------------------- search and apply

fn search(s: &mut Session, p: &Value) -> Result<Value> {
    let q = str_p(p, "query").unwrap_or_default();
    let limit = usize_p(p, "limit").unwrap_or(s.fx_console.max_results as usize).clamp(1, MAX_LIMIT);
    let cat = catalog(s);
    if q.trim().is_empty() {
        let b = fx_console::browse(&cat, &s.fx_console);
        return Ok(json!({"query": q, "shortcuts": b.shortcuts, "favorites": b.favorites}));
    }
    let r = fx_console::search(&cat, &s.fx_console, q, limit);
    Ok(json!({"query": q, "effects": r.effects, "presets": r.presets}))
}

/// Apply `t` to the layers `p` names (or the selected ones): its favorite's override preset in
/// its place when it has one (the target itself, with a warning, when that file is gone).
pub(crate) fn apply_target(s: &mut Session, t: &Target, p: &Value) -> Result<Value> {
    const C: &str = "fxConsole.apply";
    let (resolved, overridden) = s.fx_console.resolve(t);
    let (resolved, overridden, warning) = match &resolved {
        Target::Preset(path) if overridden && !s.services.exists(path) => {
            (t.clone(), false, Some(format!("The override preset {path} is missing: applied {} instead", t.name())))
        }
        _ => (resolved, overridden, None),
    };
    let mut params = serde_json::Map::new();
    for k in ["layers", "layer", "comp"] {
        if let Some(v) = p.get(k).filter(|v| !v.is_null()) {
            params.insert(k.into(), v.clone());
        }
    }
    let result = match &resolved {
        Target::Effect(id) => {
            params.insert("effect".into(), json!(id));
            run(s, "effect.apply", Value::Object(params))?
        }
        Target::Preset(path) => {
            params.insert("path".into(), json!(path));
            run(s, "anim.applyPreset", Value::Object(params))?
        }
        Target::TextPreset(id) => {
            // Text presets go on text layers, one at a time.
            let (cid, layers) = layers_p(s, p)?;
            let comp = s.project.comp(cid).ok_or(EngineError::NoComp)?;
            let text: Vec<u64> = layers.iter().filter(|l| comp.layer(**l).is_some_and(|l| matches!(l.source, LayerSource::Text))).map(|l| l.0).collect();
            if text.is_empty() {
                return Err(bad(C, format!("{} goes on text layers: select one", t.name())));
            }
            let mut out = vec![];
            for l in text {
                out.push(run(s, "layer.applyTextPreset", json!({"comp": cid.0, "layer": l, "preset": id}))?);
            }
            json!(out)
        }
    };
    if let Some(w) = &warning {
        s.toast(w.clone());
    }
    Ok(json!({"target": t, "name": t.name(), "applied": resolved, "override": overridden, "warning": warning, "result": result}))
}

fn apply(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "fxConsole.apply";
    let t = if p.get("slot").is_some_and(|v| !v.is_null()) {
        let n = usize_p(p, "slot").ok_or_else(|| bad(C, "`slot` is a shortcut number, 1–9"))?;
        s.fx_console.shortcut(n).map(|sc| sc.target.clone()).ok_or_else(|| bad(C, format!("shortcut {n} is empty")))?
    } else if let Some(q) = str_p(p, "query") {
        let r = fx_console::search(&catalog(s), &s.fx_console, q, 1);
        // Effects first, unless a preset matches better.
        let best = match (r.effects.first(), r.presets.first()) {
            (Some(e), Some(pr)) if pr.score > e.score => Some(pr),
            (e, pr) => e.or(pr),
        };
        best.map(|h| h.target.clone()).ok_or_else(|| bad(C, format!("nothing matches `{q}`")))?
    } else {
        target(p, C)?
    };
    apply_target(s, &t, p)
}

// ---------------------------------------------------------------- what the console learns

fn favorite(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "fxConsole.favorite";
    let t = target(p, C)?;
    let on = b_p(p, "value").unwrap_or(s.fx_console.favorite_index(&t).is_none());
    s.fx_console.set_favorite(&t, on, usize_p(p, "index"));
    if on && s.fx_console.favorite_index(&t).is_none() {
        return Err(bad(C, "the favorites list is full"));
    }
    save(s)?;
    Ok(json!({"favorite": on, "index": s.fx_console.favorite_index(&t), "favorites": s.fx_console.favorites.len()}))
}

fn alias(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "fxConsole.alias";
    let t = target(p, C)?;
    let alias = str_p(p, "alias").ok_or_else(|| bad(C, "missing `alias` (other names, comma-separated)"))?.to_string();
    let f = s.fx_console.favorite_mut(&t).ok_or_else(|| bad(C, "the favorites list is full"))?;
    f.alias = alias;
    save(s)?;
    Ok(json!({"alias": s.fx_console.favorite(&t).map(|f| f.alias.clone()), "index": s.fx_console.favorite_index(&t)}))
}

fn set_override(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "fxConsole.override";
    let t = target(p, C)?;
    let path = match p.get("overridePreset") {
        Some(Value::String(x)) if !x.trim().is_empty() => Some(x.trim().to_string()),
        Some(Value::String(_) | Value::Null) | None => None,
        Some(_) => return Err(bad(C, "`overridePreset` is a .ecpreset path")),
    };
    if let Some(path) = &path {
        // Only an animation preset can stand in for an effect.
        let bytes = s.services.read_file(path).map_err(|e| bad(C, format!("cannot read {path}: {e}")))?;
        let doc: Value = serde_json::from_slice(&bytes).map_err(|e| bad(C, format!("{path} is not a preset: {e}")))?;
        if doc.get(super::animation::PRESET_TAG).is_none() {
            return Err(bad(C, format!("{path} is not an EffectCraft animation preset")));
        }
    }
    let f = s.fx_console.favorite_mut(&t).ok_or_else(|| bad(C, "the favorites list is full"))?;
    f.preset_override = path.clone();
    save(s)?;
    Ok(json!({"override": path, "index": s.fx_console.favorite_index(&t)}))
}

fn blacklist(s: &mut Session, p: &Value) -> Result<Value> {
    let t = target(p, "fxConsole.blacklist")?;
    let on = b_p(p, "value").unwrap_or(!s.fx_console.is_blacklisted(&t));
    s.fx_console.set_blacklisted(&t, on);
    save(s)?;
    Ok(json!({"blacklisted": on, "blacklist": s.fx_console.blacklist.len()}))
}

fn shortcut(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "fxConsole.shortcut";
    let given = match Target::from_params(p) {
        Some(Ok(t)) => Some(t),
        Some(Err(e)) => return Err(bad(C, e)),
        None => None,
    };
    let slot = match usize_p(p, "slot") {
        Some(n) => n,
        None if given.is_some() => s.fx_console.free_slot().ok_or_else(|| bad(C, "all nine shortcuts are taken: give `slot` to replace one"))?,
        None => return Err(bad(C, "missing `slot` (1–9)")),
    };
    if let Some(to) = usize_p(p, "moveTo") {
        s.fx_console.move_shortcut(slot, to).map_err(|e| bad(C, e))?;
        save(s)?;
        return Ok(json!({"slot": to, "shortcut": s.fx_console.shortcut(to)}));
    }
    if b_p(p, "clear") == Some(true) {
        s.fx_console.set_shortcut(slot, None).map_err(|e| bad(C, e))?;
        save(s)?;
        return Ok(json!({"slot": slot, "shortcut": Value::Null}));
    }
    let existing = s.fx_console.shortcut(slot).cloned();
    let target = match (given, &existing) {
        (Some(t), _) => t,
        (None, Some(sc)) => sc.target.clone(),
        (None, None) => return Err(bad(C, format!("shortcut {slot} is empty: name what it applies"))),
    };
    let label = match str_p(p, "label") {
        Some(l) => l.to_string(),
        // A new target starts without the old one's label.
        None => existing.filter(|sc| sc.target == target).map(|sc| sc.label).unwrap_or_default(),
    };
    s.fx_console.set_shortcut(slot, Some(Shortcut { target, label })).map_err(|e| bad(C, e))?;
    save(s)?;
    Ok(json!({"slot": slot, "shortcut": s.fx_console.shortcut(slot)}))
}

// ---------------------------------------------------------------- options and settings files

fn settings(s: &mut Session, _: &Value) -> Result<Value> {
    Ok(json!({
        "settings": s.fx_console,
        "file": fx_console::SETTINGS_FILE,
        "galleryFolder": gallery::folder(s),
        "defaultGalleryFolder": gallery::default_folder(s),
        "presetsFolder": fx_console::presets_dir(),
    }))
}

fn set_settings(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "fxConsole.setSettings";
    let mut next = if b_p(p, "reset") == Some(true) { Settings::default() } else { s.fx_console.clone() };
    if let Some(c) = str_p(p, "color") {
        let c = c.trim().to_ascii_lowercase();
        if !c.is_empty() && !valid_color(&c) {
            return Err(bad(C, format!("`color` is #rrggbb, not `{c}`")));
        }
        next.color = c;
    }
    if let Some(b) = b_p(p, "presets") {
        next.presets = b;
    }
    if let Some(n) = usize_p(p, "maxResults") {
        next.max_results = u32::try_from(n.clamp(5, MAX_LIMIT)).unwrap_or(40);
    }
    if let Some(f) = str_p(p, "snapshotFormat") {
        next.snapshot_format = Format::parse(f).ok_or_else(|| bad(C, format!("`snapshotFormat` is png or jpg, not `{f}`")))?.ext().into();
    }
    if let Some(q) = usize_p(p, "jpegQuality") {
        next.jpeg_quality = u8::try_from(q.clamp(1, 100)).unwrap_or(92);
    }
    if let Some(f) = str_p(p, "galleryFolder") {
        next.gallery_folder = f.trim().to_string();
    }
    s.fx_console = next;
    save(s)?;
    settings(s, p)
}

fn export_settings(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_p(p, "path").ok_or_else(|| bad("fxConsole.exportSettings", "missing `path` (.json)"))?.to_string();
    s.services.write_file(&path, s.fx_console.to_json().as_bytes()).map_err(|e| EngineError::Other(format!("cannot write {path}: {e}")))?;
    s.toast(format!("FX Console settings exported to {path}"));
    Ok(json!({"path": path}))
}

fn import_settings(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "fxConsole.importSettings";
    let path = str_p(p, "path").ok_or_else(|| bad(C, "missing `path` (.json)"))?.to_string();
    let bytes = s.services.read_file(&path).map_err(|e| EngineError::Other(format!("cannot read {path}: {e}")))?;
    if bytes.len() > fx_console::MAX_SETTINGS_BYTES {
        return Err(bad(C, format!("{path} is too large for FX Console settings")));
    }
    let text = String::from_utf8(bytes).map_err(|_| bad(C, format!("{path} is not UTF-8 text")))?;
    // A file that isn't settings at all must not replace the user's.
    if !serde_json::from_str::<Value>(&text).is_ok_and(|v| v.is_object()) {
        return Err(bad(C, format!("{path} is not an FX Console settings file")));
    }
    let (imported, repairs) = Settings::from_json(&text);
    if b_p(p, "merge") == Some(true) {
        s.fx_console.merge(imported);
    } else {
        s.fx_console = imported;
    }
    save(s)?;
    s.toast(format!("FX Console settings imported from {path}"));
    Ok(json!({"path": path, "favorites": s.fx_console.favorites.len(), "blacklist": s.fx_console.blacklist.len(), "repairs": repairs}))
}

// ---------------------------------------------------------------- snapshots

/// `format` (`png` or `jpg`), if given.
fn format_p(p: &Value, cmd: &str) -> Result<Option<Format>> {
    match str_p(p, "format") {
        Some(f) => Format::parse(f).map(Some).ok_or_else(|| bad(cmd, format!("`format` is png or jpg, not `{f}`"))),
        None => Ok(None),
    }
}

fn snapshot(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "fxConsole.snapshot";
    let cid = comp_id(s, p)?;
    let format = format_p(p, C)?.or_else(|| Format::parse(&s.fx_console.snapshot_format)).unwrap_or(Format::Png);
    let r = gallery::snapshot(s, cid, f_p(p, "scale").unwrap_or(1.0), b_p(p, "alpha").unwrap_or(false), format, None)?;
    s.toast(format!("Snapshot saved to the FX Console gallery ({})", r["name"].as_str().unwrap_or_default()));
    Ok(r)
}

fn save_frame(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "fxConsole.saveFrame";
    let cid = comp_id(s, p)?;
    let path = str_p(p, "path").map(str::trim).filter(|p| !p.is_empty()).ok_or_else(|| bad(C, "missing `path` (.png or .jpg)"))?;
    // The file's extension decides; without one, `format` (else PNG) and its extension.
    let (path, format) = match (Format::of_path(path), format_p(p, C)?) {
        (Some(f), _) => (path.to_string(), f),
        (None, f) => {
            let f = f.unwrap_or(Format::Png);
            (format!("{path}.{}", f.ext()), f)
        }
    };
    let r = gallery::snapshot(s, cid, f_p(p, "scale").unwrap_or(1.0), b_p(p, "alpha").unwrap_or(false), format, Some(&path))?;
    s.toast(format!("Saved frame to {path}"));
    Ok(r)
}

fn gallery_list(s: &mut Session, _: &Value) -> Result<Value> {
    Ok(json!({"folder": gallery::folder(s), "snapshots": gallery::list(s)}))
}

fn delete_snapshots(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "fxConsole.deleteSnapshots";
    let names = names_p(p, C)?;
    let n = gallery::delete(s, &names).map_err(|e| bad(C, e))?;
    Ok(json!({"deleted": n}))
}

fn export_snapshots(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "fxConsole.exportSnapshots";
    let names = names_p(p, C)?;
    let folder = str_p(p, "folder").map(str::trim).filter(|f| !f.is_empty()).ok_or_else(|| bad(C, "missing `folder`"))?.to_string();
    let format = format_p(p, C)?;
    let quality = s.fx_console.jpeg_quality;
    let written = gallery::export(s, &names, &folder, format, quality).map_err(|e| bad(C, e))?;
    s.toast(format!("Exported {} snapshot(s) to {folder}", written.len()));
    Ok(json!({"written": written}))
}

fn import_snapshots(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "fxConsole.importSnapshots";
    let names = names_p(p, C)?;
    let paths = names
        .iter()
        .map(|n| gallery::resolve(s, n).map(|p| p.to_string_lossy().to_string()))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| bad(C, e))?;
    run(s, "file.import", json!({"paths": paths}))
}

fn reveal_gallery(s: &mut Session, p: &Value) -> Result<Value> {
    const C: &str = "fxConsole.revealGallery";
    let inside = match str_p(p, "name") {
        Some(n) => gallery::resolve(s, n).map_err(|e| bad(C, e))?,
        // (A name inside the folder: the file manager opens the folder itself.)
        None => gallery::folder(s).ok_or_else(|| bad(C, "there is no gallery folder here"))?.join("snapshot"),
    };
    let url = super::layer_menu::folder_url(&inside.to_string_lossy());
    s.events.push(crate::Event::OpenUrl(url.clone()));
    Ok(json!({"url": url}))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        query!(
            "fxConsole.search",
            "FX Console Search",
            "{query?, limit?} — effects and presets as the console ranks them (favorites first, blacklisted ones hidden); an empty query lists the shortcuts and favorites",
            search
        ),
        cmd!(
            "fxConsole.apply",
            "Apply from FX Console",
            [],
            None,
            "{effect? (id or name) | textPreset? | preset? (.ecpreset path) | query? (its best match) | slot? (shortcut 1–9), layers?} — a favorite's override preset applies in its place",
            has_layers,
            apply
        ),
        cmd!(
            "fxConsole.favorite",
            "FX Console Favorite",
            [],
            None,
            "{effect | textPreset | preset, value?: bool (default: toggle), index?: priority (0 = first)}",
            always,
            favorite
        ),
        cmd!(
            "fxConsole.alias",
            "FX Console Alias",
            [],
            None,
            "{effect | textPreset | preset, alias: other names, comma-separated} — makes it a favorite",
            always,
            alias
        ),
        cmd!(
            "fxConsole.override",
            "FX Console Override",
            [],
            None,
            "{effect | textPreset | preset, overridePreset?: .ecpreset path applied in its place (empty or null: none)} — makes it a favorite",
            always,
            set_override
        ),
        cmd!(
            "fxConsole.blacklist",
            "FX Console Blacklist",
            [],
            None,
            "{effect | textPreset | preset, value?: bool (default: toggle)} — hidden from the console's results",
            always,
            blacklist
        ),
        cmd!(
            "fxConsole.shortcut",
            "FX Console Shortcut",
            [],
            None,
            "{slot?: 1–9 (default: the first empty one), effect? | textPreset? | preset?, label?, clear?: bool, moveTo?: 1–9}",
            always,
            shortcut
        ),
        query!("fxConsole.settings", "FX Console Settings", "{} → favorites, blacklist, shortcuts, options and folders", settings),
        cmd!(
            "fxConsole.setSettings",
            "Set FX Console Options",
            [],
            None,
            "{color?: #rrggbb (empty: the theme's), presets?: bool, maxResults?, snapshotFormat?: png|jpg, jpegQuality?: 1–100, galleryFolder?, reset?: bool}",
            always,
            set_settings
        ),
        cmd!("fxConsole.exportSettings", "Export FX Console Settings...", [], None, "{path (.json)}", always, export_settings),
        cmd!(
            "fxConsole.importSettings",
            "Import FX Console Settings...",
            [],
            None,
            "{path (.json), merge?: bool (add to the current settings instead of replacing them)}",
            always,
            import_settings
        ),
        cmd!(
            "fxConsole.snapshot",
            "FX Console Snapshot",
            [],
            None,
            "{comp?, scale?: 0.05–1 (the viewer's resolution), alpha?: bool (keep transparency, PNG), format?: png|jpg} → a new snapshot in the gallery",
            has_comp,
            snapshot
        ),
        cmd!(
            "fxConsole.saveFrame",
            "Save Frame from FX Console",
            [],
            None,
            "{path (.png|.jpg), comp?, scale?: 0.05–1, alpha?: bool, format?: png|jpg}",
            has_comp,
            save_frame
        ),
        query!("fxConsole.gallery", "FX Console Gallery Snapshots", "{} → the gallery folder and its snapshots, newest first", gallery_list),
        cmd!("fxConsole.deleteSnapshots", "Delete FX Console Snapshots", [], None, "{names: [snapshot file name]}", always, delete_snapshots),
        cmd!(
            "fxConsole.exportSnapshots",
            "Export FX Console Snapshots",
            [],
            None,
            "{names: [snapshot file name], folder, format?: png|jpg (convert)}",
            always,
            export_snapshots
        ),
        cmd!(
            "fxConsole.importSnapshots",
            "Import FX Console Snapshots",
            [],
            None,
            "{names: [snapshot file name]} — into the project as footage",
            always,
            import_snapshots
        ),
        cmd!("fxConsole.revealGallery", "Reveal FX Console Gallery", [], None, "{name?: the snapshot to show}", always, reveal_gallery),
        // The frontend's windows and the clipboard.
        cmd!("fxConsole.openGallery", "FX Console Gallery", ["Window"], None, "{}", always, |s, p| frontend(s, "fxConsole.openGallery", p)),
        cmd!("fxConsole.openSettings", "FX Console Settings...", [], None, "{page?: favorites|shortcuts|blacklist|general}", always, |s, p| frontend(
            s,
            "fxConsole.openSettings",
            p
        )),
        cmd!(
            "fxConsole.copyFrame",
            "Copy Frame to Clipboard",
            [],
            None,
            "{comp?, alpha?: bool} — the current frame at the viewer's resolution",
            has_comp,
            |s, p| frontend(s, "fxConsole.copyFrame", p)
        ),
    ]
}
