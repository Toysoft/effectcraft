//! Console Effect: a keyboard-first launcher for effects and animation presets, EffectCraft's
//! built-in analog of the FX Console workflow After Effects users add with a plug-in.
//!
//! Edit ▸ Console Effect… (Ctrl+Space) opens a search box at the pointer. Typing part of a name lists
//! the matching effects and animation presets, effects first ([`search`]); Enter applies the
//! highlighted one to the selected layers, Tab jumps to the presets, and with an empty search the
//! keys 1–9 apply the nine shortcuts. What the user teaches it lives in the settings store
//! ([`SETTINGS_FILE`]), not in the project:
//!
//! - **favorites** rank above every other match, in their order (their priority);
//! - a favorite's **alias** is another name it answers to ("corner pin" finds CC Power Pin);
//! - a favorite's **override** is an animation preset applied in its place: the effect with other
//!   defaults, or several effects and expressions;
//! - the **blacklist** hides effects and presets from the results;
//! - nine numbered **shortcuts** with labels.
//!
//! Snapshots of the composition go to a gallery folder ([`gallery`]). Everything is a command
//! (`consoleEffect.*`, see `commands::console_effect`), so agents get what the console does.

pub mod gallery;
mod search;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use search::{Browse, Catalog, Entry, Hit, Results, browse, search};

use crate::{EngineError, Result, Session};

/// The console's settings in the settings store.
pub const SETTINGS_FILE: &str = "console_effect.json";
/// Numbered shortcut slots (keys 1–9).
pub const SLOTS: usize = 9;
/// Longest settings file read (a hand-edited or hostile file is refused, not loaded).
pub(crate) const MAX_SETTINGS_BYTES: usize = 4 << 20;
const MAX_FAVORITES: usize = 512;
const MAX_BLACKLIST: usize = 2048;
const MAX_ALIAS_CHARS: usize = 256;
const MAX_LABEL_CHARS: usize = 64;
const MAX_PATH_CHARS: usize = 4096;
const DEFAULT_RESULTS: u32 = 40;
const DEFAULT_JPEG_QUALITY: u8 = 92;
/// Preset files: EffectCraft's animation presets.
pub const PRESET_EXT: &str = "ecpreset";

/// What a console row applies.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Target {
    /// An effect by registry id (`ec.blur.gaussian`).
    Effect(String),
    /// One of EffectCraft's text animation presets by id (`typewriter`).
    TextPreset(String),
    /// An animation preset file (`.ecpreset`).
    Preset(String),
}

/// The two sections of the results.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Effect,
    Preset,
}

impl Target {
    pub fn kind(&self) -> Kind {
        match self {
            Target::Effect(_) => Kind::Effect,
            Target::TextPreset(_) | Target::Preset(_) => Kind::Preset,
        }
    }

    /// The name a row shows: the effect's or text preset's name, or the preset file's name.
    pub fn name(&self) -> String {
        match self {
            Target::Effect(id) => crate::effects::find(id).map_or_else(|| id.clone(), |e| e.name.to_string()),
            Target::TextPreset(id) => crate::text_presets().into_iter().find(|(i, _)| i == id).map_or_else(|| id.clone(), |(_, n)| n),
            Target::Preset(path) => preset_name(path),
        }
    }

    /// The target named by command parameters: `effect` (id or name), `textPreset` (id or name)
    /// or `preset` (a preset file path). `None` when none is given.
    pub fn from_params(p: &Value) -> Option<std::result::Result<Target, String>> {
        if let Some(v) = p.get("effect").filter(|v| !v.is_null()) {
            let name = v.as_str().unwrap_or_default();
            return Some(crate::effects::lookup(name).map(|e| Target::Effect(e.id.to_string())).ok_or_else(|| format!("unknown effect `{name}`")));
        }
        if let Some(v) = p.get("textPreset").filter(|v| !v.is_null()) {
            let name = v.as_str().unwrap_or_default();
            return Some(
                crate::text_presets()
                    .into_iter()
                    .find(|(id, n)| id == name || n.eq_ignore_ascii_case(name))
                    .map(|(id, _)| Target::TextPreset(id))
                    .ok_or_else(|| format!("unknown text animation preset `{name}`")),
            );
        }
        if let Some(v) = p.get("preset").filter(|v| !v.is_null()) {
            let path = v.as_str().unwrap_or_default().trim();
            if path.is_empty() || path.chars().count() > MAX_PATH_CHARS {
                return Some(Err("`preset` must be a preset file path".into()));
            }
            return Some(Ok(Target::Preset(path.to_string())));
        }
        None
    }

    /// The parameters [`Target::from_params`] reads back.
    pub fn to_params(&self) -> Value {
        match self {
            Target::Effect(id) => serde_json::json!({"effect": id}),
            Target::TextPreset(id) => serde_json::json!({"textPreset": id}),
            Target::Preset(path) => serde_json::json!({"preset": path}),
        }
    }
}

/// A preset file's name: its file name without the extension.
pub fn preset_name(path: &str) -> String {
    let p = std::path::Path::new(path);
    p.file_stem().map(|s| s.to_string_lossy().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| path.to_string())
}

/// A favorite: ranked above other matches in list order, with its aliases and override.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Favorite {
    pub target: Target,
    /// Other names it answers to, comma-separated ("corner pin, cp").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub alias: String,
    /// An animation preset file applied in place of the target.
    #[serde(default, rename = "override", skip_serializing_if = "Option::is_none")]
    pub preset_override: Option<String>,
}

impl Favorite {
    pub fn new(target: Target) -> Favorite {
        Favorite { target, alias: String::new(), preset_override: None }
    }

    /// The aliases, trimmed and lower-cased (for matching).
    pub fn aliases(&self) -> impl Iterator<Item = String> + '_ {
        self.alias.split(',').map(|a| a.trim().to_lowercase()).filter(|a| !a.is_empty())
    }
}

/// A numbered shortcut: what it applies and the label it shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shortcut {
    pub target: Target,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
}

impl Shortcut {
    /// The label, or the target's name when it has none.
    pub fn shown(&self) -> String {
        if self.label.trim().is_empty() { self.target.name() } else { self.label.clone() }
    }
}

/// Everything the console remembers ([`SETTINGS_FILE`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub version: u32,
    pub favorites: Vec<Favorite>,
    pub blacklist: Vec<Target>,
    /// Slots 1–9 ([`SLOTS`] entries, `None` = empty).
    pub shortcuts: Vec<Option<Shortcut>>,
    /// The console's accent colour (`#rrggbb`); empty: the theme's.
    pub color: String,
    /// List animation presets (text presets and preset files) with the effects.
    pub presets: bool,
    /// Rows per section of the results.
    pub max_results: u32,
    /// Snapshot file format: `png` (keeps transparency) or `jpg`.
    pub snapshot_format: String,
    /// JPEG quality of `jpg` snapshots (1–100).
    pub jpeg_quality: u8,
    /// The snapshot gallery's folder; empty: [`gallery::default_folder`].
    pub gallery_folder: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: 1,
            favorites: vec![],
            blacklist: vec![],
            shortcuts: vec![None; SLOTS],
            color: String::new(),
            presets: true,
            max_results: DEFAULT_RESULTS,
            snapshot_format: "png".into(),
            jpeg_quality: DEFAULT_JPEG_QUALITY,
            gallery_folder: String::new(),
        }
    }
}

/// `s` cut to `max` characters (at a character boundary), trimmed.
fn cap(s: &str, max: usize) -> String {
    s.trim().chars().take(max).collect::<String>().trim().to_string()
}

/// Whether `c` is a `#rrggbb` colour.
pub fn valid_color(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c.bytes().skip(1).all(|b| b.is_ascii_hexdigit())
}

impl Settings {
    /// Read settings leniently: entries that don't parse are dropped (and reported) instead of
    /// losing the whole file to one bad row.
    pub fn from_json(text: &str) -> (Settings, Vec<String>) {
        let mut repairs = vec![];
        let v: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => return (Settings::default(), vec![format!("not valid JSON ({e}); starting with defaults")]),
        };
        let Some(obj) = v.as_object() else { return (Settings::default(), vec!["not a settings object; starting with defaults".into()]) };
        let mut s = Settings::default();
        fn list<T: serde::de::DeserializeOwned>(v: Option<&Value>, what: &str, repairs: &mut Vec<String>) -> Vec<T> {
            let Some(a) = v.and_then(Value::as_array) else { return vec![] };
            a.iter()
                .enumerate()
                .filter_map(|(i, x)| serde_json::from_value(x.clone()).map_err(|e| repairs.push(format!("{what} #{}: {e}", i + 1))).ok())
                .collect()
        }
        s.favorites = list(obj.get("favorites"), "favorite", &mut repairs);
        s.blacklist = list(obj.get("blacklist"), "blacklist entry", &mut repairs);
        if let Some(a) = obj.get("shortcuts").and_then(Value::as_array) {
            s.shortcuts = a
                .iter()
                .enumerate()
                .map(|(i, x)| match x {
                    Value::Null => None,
                    x => serde_json::from_value(x.clone()).map_err(|e| repairs.push(format!("shortcut {}: {e}", i + 1))).ok(),
                })
                .collect();
        }
        let str_of = |k: &str| obj.get(k).and_then(Value::as_str).map(str::to_string);
        if let Some(c) = str_of("color") {
            s.color = c;
        }
        if let Some(b) = obj.get("presets").and_then(Value::as_bool) {
            s.presets = b;
        }
        if let Some(n) = obj.get("maxResults").and_then(Value::as_u64) {
            s.max_results = u32::try_from(n).unwrap_or(u32::MAX);
        }
        if let Some(f) = str_of("snapshotFormat") {
            s.snapshot_format = f;
        }
        if let Some(q) = obj.get("jpegQuality").and_then(Value::as_u64) {
            s.jpeg_quality = u8::try_from(q.min(100)).unwrap_or(DEFAULT_JPEG_QUALITY);
        }
        if let Some(f) = str_of("galleryFolder") {
            s.gallery_folder = f;
        }
        repairs.extend(s.normalize());
        (s, repairs)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }

    /// Bring hand-edited or imported settings into range: no duplicates, bounded lists and text,
    /// exactly [`SLOTS`] shortcut slots, a valid colour and format. Returns what was repaired.
    pub fn normalize(&mut self) -> Vec<String> {
        let mut repairs = vec![];
        self.version = 1;
        let mut seen = HashSet::new();
        self.favorites.retain(|f| seen.insert(f.target.clone()));
        if self.favorites.len() > MAX_FAVORITES {
            repairs.push(format!("kept the first {MAX_FAVORITES} of {} favorites", self.favorites.len()));
            self.favorites.truncate(MAX_FAVORITES);
        }
        for f in &mut self.favorites {
            f.alias = cap(&f.alias, MAX_ALIAS_CHARS);
            f.preset_override = f.preset_override.as_deref().map(|p| cap(p, MAX_PATH_CHARS)).filter(|p| !p.is_empty());
        }
        let mut seen = HashSet::new();
        self.blacklist.retain(|t| seen.insert(t.clone()));
        if self.blacklist.len() > MAX_BLACKLIST {
            repairs.push(format!("kept the first {MAX_BLACKLIST} of {} blacklist entries", self.blacklist.len()));
            self.blacklist.truncate(MAX_BLACKLIST);
        }
        // Blacklisting removes a favorite; a favorite is never hidden.
        let fav: HashSet<Target> = self.favorites.iter().map(|f| f.target.clone()).collect();
        self.blacklist.retain(|t| !fav.contains(t));
        if self.shortcuts.len() > SLOTS {
            repairs.push(format!("kept the first {SLOTS} of {} shortcuts", self.shortcuts.len()));
        }
        self.shortcuts.resize(SLOTS, None);
        for sc in self.shortcuts.iter_mut().flatten() {
            sc.label = cap(&sc.label, MAX_LABEL_CHARS);
        }
        let color = self.color.trim().to_ascii_lowercase();
        self.color = if color.is_empty() || valid_color(&color) {
            color
        } else {
            repairs.push(format!("`{}` is not a #rrggbb colour", self.color));
            String::new()
        };
        self.max_results = if self.max_results == 0 { DEFAULT_RESULTS } else { self.max_results.clamp(5, 500) };
        self.snapshot_format = match self.snapshot_format.trim().to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => "jpg".into(),
            "png" | "" => "png".into(),
            other => {
                repairs.push(format!("unknown snapshot format `{other}`; using PNG"));
                "png".into()
            }
        };
        self.jpeg_quality = self.jpeg_quality.clamp(1, 100);
        self.gallery_folder = cap(&self.gallery_folder, MAX_PATH_CHARS);
        repairs
    }

    /// The favorite's priority (0 = first), if `t` is one.
    pub fn favorite_index(&self, t: &Target) -> Option<usize> {
        self.favorites.iter().position(|f| &f.target == t)
    }

    pub fn favorite(&self, t: &Target) -> Option<&Favorite> {
        self.favorites.iter().find(|f| &f.target == t)
    }

    pub fn is_blacklisted(&self, t: &Target) -> bool {
        self.blacklist.contains(t)
    }

    /// The shortcut slot (1–9) that applies `t`, if any.
    pub fn shortcut_slot(&self, t: &Target) -> Option<usize> {
        self.shortcuts.iter().position(|s| s.as_ref().is_some_and(|s| &s.target == t)).map(|i| i + 1)
    }

    /// The shortcut in slot `slot` (1–9).
    pub fn shortcut(&self, slot: usize) -> Option<&Shortcut> {
        self.shortcuts.get(slot.checked_sub(1)?)?.as_ref()
    }

    /// Make `t` a favorite (`on`), at priority `index` (a new one defaults to last, an existing
    /// one stays where it is), or no longer one. A favorite comes off the blacklist. Nothing is
    /// added to a full list.
    pub fn set_favorite(&mut self, t: &Target, on: bool, index: Option<usize>) {
        match (on, self.favorite_index(t)) {
            (false, Some(i)) => {
                self.favorites.remove(i);
            }
            (false, None) => {}
            (true, Some(i)) => {
                if let Some(to) = index {
                    let f = self.favorites.remove(i);
                    let to = to.min(self.favorites.len());
                    self.favorites.insert(to, f);
                }
            }
            (true, None) => {
                if self.favorites.len() >= MAX_FAVORITES {
                    return;
                }
                let to = index.unwrap_or(self.favorites.len()).min(self.favorites.len());
                self.favorites.insert(to, Favorite::new(t.clone()));
                self.blacklist.retain(|b| b != t);
            }
        }
    }

    /// The favorite entry of `t`, made a favorite first when it isn't one (`None` when the list
    /// is full).
    pub fn favorite_mut(&mut self, t: &Target) -> Option<&mut Favorite> {
        if self.favorite_index(t).is_none() {
            self.set_favorite(t, true, None);
        }
        self.favorites.iter_mut().find(|f| &f.target == t)
    }

    /// Hide `t` from the results (`on`), or show it again. A hidden target is no favorite.
    pub fn set_blacklisted(&mut self, t: &Target, on: bool) {
        self.blacklist.retain(|b| b != t);
        if on {
            self.favorites.retain(|f| &f.target != t);
            self.blacklist.push(t.clone());
            self.blacklist.truncate(MAX_BLACKLIST);
        }
    }

    /// Put `shortcut` in slot `slot` (1–9; `None` empties it).
    pub fn set_shortcut(&mut self, slot: usize, shortcut: Option<Shortcut>) -> std::result::Result<(), String> {
        let i = slot.checked_sub(1).filter(|i| *i < SLOTS).ok_or_else(|| format!("shortcut slots are 1–{SLOTS}, not {slot}"))?;
        self.shortcuts.resize(SLOTS, None);
        let shortcut = shortcut.map(|mut s| {
            s.label = cap(&s.label, MAX_LABEL_CHARS);
            s
        });
        if let Some(cell) = self.shortcuts.get_mut(i) {
            *cell = shortcut;
        }
        Ok(())
    }

    /// Move the shortcut in slot `from` to slot `to` (1–9), shifting the ones between.
    pub fn move_shortcut(&mut self, from: usize, to: usize) -> std::result::Result<(), String> {
        let ok = |n: usize| n.checked_sub(1).filter(|i| *i < SLOTS).ok_or_else(|| format!("shortcut slots are 1–{SLOTS}, not {n}"));
        let (a, b) = (ok(from)?, ok(to)?);
        self.shortcuts.resize(SLOTS, None);
        let s = self.shortcuts.remove(a);
        self.shortcuts.insert(b, s);
        Ok(())
    }

    /// The first empty slot (1–9).
    pub fn free_slot(&self) -> Option<usize> {
        self.shortcuts.iter().position(Option::is_none).map(|i| i + 1).filter(|n| *n <= SLOTS)
    }

    /// What applying `t` runs: its favorite's override preset when it has one, else itself.
    pub fn resolve(&self, t: &Target) -> (Target, bool) {
        match self.favorite(t).and_then(|f| f.preset_override.clone()) {
            Some(path) => (Target::Preset(path), true),
            None => (t.clone(), false),
        }
    }

    /// Imported settings added to these: favorites, blacklist entries and empty shortcut slots
    /// they don't have; the other options are taken over.
    pub fn merge(&mut self, other: Settings) {
        for f in other.favorites {
            if self.favorite_index(&f.target).is_none() && !self.is_blacklisted(&f.target) {
                self.favorites.push(f);
            }
        }
        for t in other.blacklist {
            if self.favorite_index(&t).is_none() && !self.is_blacklisted(&t) {
                self.blacklist.push(t);
            }
        }
        self.shortcuts.resize(SLOTS, None);
        for (mine, theirs) in self.shortcuts.iter_mut().zip(other.shortcuts) {
            if mine.is_none() {
                *mine = theirs;
            }
        }
        (self.color, self.presets, self.max_results) = (other.color, other.presets, other.max_results);
        (self.snapshot_format, self.jpeg_quality) = (other.snapshot_format, other.jpeg_quality);
        if !other.gallery_folder.is_empty() {
            self.gallery_folder = other.gallery_folder;
        }
        self.normalize();
    }
}

/// Where the user's animation presets live: `$EC_PRESETS_DIR`, else
/// `~/Documents/EffectCraft/Presets` (the Effects & Presets panel lists them; the console finds
/// them).
pub fn presets_dir() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("EC_PRESETS_DIR")
        && !d.trim().is_empty()
    {
        return Some(d.into());
    }
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok().filter(|h| !h.trim().is_empty())?;
    Some(std::path::Path::new(&home).join("Documents").join("EffectCraft").join("Presets"))
}

/// The preset files (`.ecpreset`) directly in `dir`, sorted by name.
pub fn preset_files(s: &Session, dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = s
        .services
        .list_dir(&dir.to_string_lossy())
        .into_iter()
        .filter(|p| std::path::Path::new(p).extension().is_some_and(|x| x.eq_ignore_ascii_case(PRESET_EXT)))
        .collect();
    v.sort_by_key(|p| preset_name(p).to_lowercase());
    v
}

/// Load the console's settings from the settings store (defaults when there are none; a corrupt
/// file loses only its bad entries).
pub(crate) fn load(s: &mut Session) {
    let Some(cfg) = s.config.clone() else { return };
    match cfg.read_bounded(SETTINGS_FILE, MAX_SETTINGS_BYTES) {
        Ok(Some(text)) => {
            let (settings, repairs) = Settings::from_json(&text);
            for r in repairs {
                log::warn!("{SETTINGS_FILE}: {r}");
            }
            s.console_effect = settings;
        }
        Ok(None) => s.console_effect = Settings::default(),
        Err(e) => log::warn!("{SETTINGS_FILE} was not read ({e}); Console Effect starts with its defaults"),
    }
    s.console_effect_revision = s.console_effect_revision.wrapping_add(1);
}

/// Save the console's settings (nothing to do without a settings store) and tell frontends.
pub(crate) fn store(s: &mut Session) -> Result<()> {
    s.console_effect.normalize();
    s.console_effect_revision = s.console_effect_revision.wrapping_add(1);
    let Some(cfg) = &s.config else { return Ok(()) };
    cfg.write(SETTINGS_FILE, &s.console_effect.to_json()).map_err(|e| EngineError::Other(format!("cannot save Console Effect settings: {e}")))
}
