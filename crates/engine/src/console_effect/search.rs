//! The console's search: what is searchable ([`Catalog`]) and how matches rank ([`search`]).
//!
//! A name matches in tiers, best first: the whole name, its start, the start of one of its words,
//! anywhere in it, every typed word (at word starts, then anywhere), its initials ("gb" →
//! Gaussian Blur), its letters and digits run together ("4color" → 4-Color Gradient), and finally
//! four or more of its letters in order, close together ("frctl" → Fractal). Within a tier
//! shorter names come first. A favorite that matches by one of the strong tiers (or by an alias)
//! outranks everything, in the favorites' order; blacklisted targets never show.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::Path;

use serde::Serialize;

use super::{Favorite, Kind, SLOTS, Settings, Target, preset_name};
use crate::Session;

const EXACT: i32 = 1000;
const PREFIX: i32 = 900;
const WORD: i32 = 800;
const CONTAINS: i32 = 700;
const WORDS_AT_STARTS: i32 = 650;
const WORDS_ANYWHERE: i32 = 560;
const INITIALS: i32 = 500;
const COMPACT: i32 = 450;
/// A favorite matching at least this well outranks every other row.
const STRONG: i32 = COMPACT;
const FUZZY_BASE: i32 = 100;
const FUZZY_MAX: i32 = 400;
/// Favorites matching weakly (letters in order only) still move up this much.
const FAVORITE_NUDGE: i32 = 300;
const FAVORITE: i32 = 100_000;

/// A name folded for matching once, when the catalog is built.
#[derive(Clone, Debug, Default)]
struct Names {
    /// Lower case.
    lower: String,
    /// The first letter or digit of each word ("gaussian blur" → "gb").
    initials: String,
    /// Letters and digits only ("4-color gradient" → "4colorgradient").
    compact: String,
}

impl Names {
    fn new(s: &str) -> Names {
        let lower = s.to_lowercase();
        let mut initials = String::new();
        let mut in_word = false;
        for c in lower.chars() {
            let alnum = c.is_alphanumeric();
            if alnum && !in_word {
                initials.push(c);
            }
            in_word = alnum;
        }
        let compact = lower.chars().filter(|c| c.is_alphanumeric()).collect();
        Names { lower, initials, compact }
    }
}

/// One searchable effect or preset.
#[derive(Clone, Debug)]
pub struct Entry {
    pub target: Target,
    pub name: String,
    /// The effect category, `Text Presets` or `Animation Presets`.
    pub category: String,
    folded: Names,
    /// Other names it answers to (the registry's aliases), folded.
    others: Vec<Names>,
}

impl Entry {
    pub fn new(target: Target, name: &str, category: &str, others: &[&str]) -> Entry {
        Entry {
            target,
            name: name.to_string(),
            category: category.to_string(),
            folded: Names::new(name),
            others: others.iter().map(|o| Names::new(o)).collect(),
        }
    }
}

/// Everything the console can apply.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub entries: Vec<Entry>,
    index: HashMap<Target, usize>,
}

impl Catalog {
    pub fn new(entries: Vec<Entry>) -> Catalog {
        let mut index = HashMap::with_capacity(entries.len());
        for (i, e) in entries.iter().enumerate() {
            index.entry(e.target.clone()).or_insert(i);
        }
        Catalog { entries, index }
    }

    /// Every effect (built-in and plug-in), the text animation presets, the preset files in
    /// `presets_dir` and the preset files favorites and shortcuts name elsewhere.
    pub fn build(s: &Session, settings: &Settings, presets_dir: Option<&Path>) -> Catalog {
        let mut entries: Vec<Entry> = crate::effects::all()
            .into_iter()
            .map(|e| Entry::new(Target::Effect(e.id.to_string()), e.name, e.category, crate::effects::aliases(e.id)))
            .collect();
        entries.extend(crate::text_presets().into_iter().map(|(id, name)| Entry::new(Target::TextPreset(id), &name, "Text Presets", &[])));
        let mut files: Vec<String> = presets_dir.map(|d| super::preset_files(s, d)).unwrap_or_default();
        let named = settings.favorites.iter().map(|f| &f.target).chain(settings.shortcuts.iter().flatten().map(|sc| &sc.target));
        for t in named {
            if let Target::Preset(p) = t
                && !files.contains(p)
                && s.services.exists(p)
            {
                files.push(p.clone());
            }
        }
        entries.extend(files.into_iter().map(|p| {
            let name = preset_name(&p);
            Entry::new(Target::Preset(p), &name, "Animation Presets", &[])
        }));
        Catalog::new(entries)
    }

    pub fn get(&self, t: &Target) -> Option<&Entry> {
        self.entries.get(*self.index.get(t)?)
    }
}

/// One result row.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub target: Target,
    pub name: String,
    pub category: String,
    pub score: i32,
    /// The favorite's priority (0 = first).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub favorite: Option<usize>,
    /// The alias that matched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    /// The preset applied in its place.
    #[serde(rename = "override", skip_serializing_if = "Option::is_none")]
    pub preset_override: Option<String>,
    /// The shortcut slot (1–9) that applies it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<usize>,
    /// A shortcut's label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Not in the catalog (an effect plug-in that isn't loaded, a preset file that is gone).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub missing: bool,
    /// Byte range of the typed text in `name`, for highlighting.
    #[serde(skip)]
    pub range: Option<Range<usize>>,
}

impl Hit {
    fn of(catalog: &Catalog, settings: &Settings, target: &Target) -> Hit {
        let entry = catalog.get(target);
        let fav = settings.favorite_index(target);
        Hit {
            target: target.clone(),
            name: entry.map_or_else(|| target.name(), |e| e.name.clone()),
            category: entry.map(|e| e.category.clone()).unwrap_or_default(),
            score: 0,
            favorite: fav,
            alias: None,
            preset_override: settings.favorite(target).and_then(|f| f.preset_override.clone()),
            shortcut: settings.shortcut_slot(target),
            label: None,
            missing: entry.is_none(),
            range: None,
        }
    }
}

/// The results of a search: effects, then presets.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Results {
    pub effects: Vec<Hit>,
    pub presets: Vec<Hit>,
}

impl Results {
    pub fn len(&self) -> usize {
        self.effects.len() + self.presets.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Row `i` counting the effects, then the presets.
    pub fn get(&self, i: usize) -> Option<&Hit> {
        self.effects.get(i).or_else(|| self.presets.get(i.checked_sub(self.effects.len())?))
    }
    /// The rows in order.
    pub fn rows(&self) -> impl Iterator<Item = &Hit> {
        self.effects.iter().chain(&self.presets)
    }
}

/// The console with an empty search: the shortcuts and the favorites.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Browse {
    /// Slots 1–9.
    pub shortcuts: Vec<Option<Hit>>,
    pub favorites: Vec<Hit>,
}

/// The shortcuts and favorites (hidden ones and presets when presets are off left out).
pub fn browse(catalog: &Catalog, settings: &Settings) -> Browse {
    let shortcuts = (1..=SLOTS)
        .map(|n| {
            settings.shortcut(n).map(|sc| {
                let mut h = Hit::of(catalog, settings, &sc.target);
                h.label = Some(sc.shown());
                h.shortcut = Some(n);
                h
            })
        })
        .collect();
    let favorites =
        settings.favorites.iter().filter(|f| settings.presets || f.target.kind() == Kind::Effect).map(|f| Hit::of(catalog, settings, &f.target)).collect();
    Browse { shortcuts, favorites }
}

/// What was typed, folded once.
struct Query {
    /// Lower case, single spaces.
    text: String,
    words: Vec<String>,
    compact: String,
}

impl Query {
    fn new(q: &str) -> Query {
        let lower = q.to_lowercase();
        let words: Vec<String> = lower.split_whitespace().map(str::to_string).collect();
        let compact = lower.chars().filter(|c| c.is_alphanumeric()).collect();
        Query { text: words.join(" "), words, compact }
    }
}

/// Whether byte `i` of `s` starts a word.
fn at_word_start(s: &str, i: usize) -> bool {
    i == 0 || s.get(..i).and_then(|p| p.chars().next_back()).is_some_and(|c| !c.is_alphanumeric())
}

/// The letters of `q` in order in `name`, close together: a score in `FUZZY_BASE..=FUZZY_MAX`
/// (higher when they are adjacent and when the first letter starts the name).
fn fuzzy(name: &str, q: &str) -> Option<i32> {
    let mut it = name.char_indices();
    let (mut first, mut end, mut n) = (None, 0usize, 0usize);
    for qc in q.chars() {
        let (i, c) = it.by_ref().find(|(_, c)| *c == qc)?;
        first.get_or_insert(i);
        end = i + c.len_utf8();
        n += 1;
    }
    let first = first?;
    let span = name.get(first..end)?.chars().count().max(1);
    // Short abbreviations and letters spread wider than twice their count are noise ("cur"
    // would find CC Page Turn), not a match.
    if n < 4 || span > 2 * n {
        return None;
    }
    let tight = i32::try_from(250 * n / span).unwrap_or(0);
    let lead = if first == 0 { 40 } else { 0 };
    Some((FUZZY_BASE + tight + lead).min(FUZZY_MAX))
}

/// How well `n` matches `q`, and where in `n.lower` the typed text is (when it is one run).
fn score(n: &Names, q: &Query) -> Option<(i32, Option<Range<usize>>)> {
    let (name, text) = (n.lower.as_str(), q.text.as_str());
    if text.is_empty() {
        return None;
    }
    if name == text {
        return Some((EXACT, Some(0..name.len())));
    }
    if name.starts_with(text) {
        return Some((PREFIX, Some(0..text.len())));
    }
    let mut anywhere = None;
    for (i, _) in name.match_indices(text) {
        if at_word_start(name, i) {
            return Some((WORD, Some(i..i + text.len())));
        }
        anywhere.get_or_insert(i);
    }
    if let Some(i) = anywhere {
        return Some((CONTAINS, Some(i..i + text.len())));
    }
    if q.words.len() > 1 {
        if q.words.iter().all(|w| name.match_indices(w.as_str()).any(|(i, _)| at_word_start(name, i))) {
            return Some((WORDS_AT_STARTS, None));
        }
        if q.words.iter().all(|w| name.contains(w.as_str())) {
            return Some((WORDS_ANYWHERE, None));
        }
    }
    let qc = q.compact.as_str();
    // (One letter matches too much beyond the tiers above.)
    qc.chars().nth(1)?;
    if n.initials.starts_with(qc) {
        return Some((INITIALS + if n.initials == qc { 20 } else { 0 }, None));
    }
    if n.compact.contains(qc) {
        return Some((COMPACT, None));
    }
    fuzzy(&n.compact, qc).map(|s| (s, None))
}

/// The best of `a` and `b` (the earlier one on ties).
fn better<T>(a: Option<(i32, T)>, b: Option<(i32, T)>) -> Option<(i32, T)> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if b.0 > a.0 { b } else { a }),
        (a, b) => a.or(b),
    }
}

/// Search `catalog` for `query`: up to `limit` effects and `limit` presets, best first.
pub fn search(catalog: &Catalog, settings: &Settings, query: &str, limit: usize) -> Results {
    let q = Query::new(query);
    let mut out = Results::default();
    if q.text.is_empty() || limit == 0 {
        return out;
    }
    let favorites: HashMap<&Target, (usize, &Favorite)> = settings.favorites.iter().enumerate().map(|(i, f)| (&f.target, (i, f))).collect();
    let hidden: HashSet<&Target> = settings.blacklist.iter().collect();
    let alias_names: HashMap<&Target, Vec<(String, Names)>> =
        settings.favorites.iter().filter(|f| !f.alias.is_empty()).map(|f| (&f.target, f.aliases().map(|a| (a.clone(), Names::new(&a))).collect())).collect();
    for e in &catalog.entries {
        let kind = e.target.kind();
        if (kind == Kind::Preset && !settings.presets) || hidden.contains(&e.target) {
            continue;
        }
        let mut best = score(&e.folded, &q).map(|(s, r)| (s, (r, None)));
        for o in &e.others {
            best = better(best, score(o, &q).map(|(s, _)| (s, (None, None))));
        }
        for (alias, names) in alias_names.get(&e.target).into_iter().flatten() {
            best = better(best, score(names, &q).map(|(s, _)| (s, (None, Some(alias.clone())))));
        }
        let Some((mut sc, (range, alias))) = best else { continue };
        let fav = favorites.get(&e.target);
        if let Some((i, _)) = fav {
            let strong = sc >= STRONG || alias.is_some();
            sc = if strong { FAVORITE - i32::try_from(*i).unwrap_or(i32::MAX / 2) } else { sc + FAVORITE_NUDGE };
        }
        // Highlight only where folding kept byte offsets (lower-casing can change lengths).
        let range = range.filter(|r| e.folded.lower.len() == e.name.len() && e.name.is_char_boundary(r.start) && e.name.is_char_boundary(r.end));
        let hit = Hit {
            target: e.target.clone(),
            name: e.name.clone(),
            category: e.category.clone(),
            score: sc,
            favorite: fav.map(|(i, _)| *i),
            alias,
            preset_override: fav.and_then(|(_, f)| f.preset_override.clone()),
            shortcut: settings.shortcut_slot(&e.target),
            label: None,
            missing: false,
            range,
        };
        match kind {
            Kind::Effect => out.effects.push(hit),
            Kind::Preset => out.presets.push(hit),
        }
    }
    for rows in [&mut out.effects, &mut out.presets] {
        rows.sort_by_cached_key(|h| (std::cmp::Reverse(h.score), h.name.chars().count(), h.name.to_lowercase()));
        rows.truncate(limit);
    }
    out
}

#[cfg(test)]
mod unit {
    use super::*;

    fn q(s: &str) -> Query {
        Query::new(s)
    }

    #[test]
    fn tiers_rank_in_order() {
        let n = Names::new("Gaussian Blur");
        assert_eq!(score(&n, &q("gaussian blur")).map(|s| s.0), Some(EXACT));
        assert_eq!(score(&n, &q("gaus")).map(|s| s.0), Some(PREFIX));
        assert_eq!(score(&n, &q("blur")).map(|s| s.0), Some(WORD));
        assert_eq!(score(&n, &q("ussian")).map(|s| s.0), Some(CONTAINS));
        assert_eq!(score(&n, &q("blur gau")).map(|s| s.0), Some(WORDS_AT_STARTS));
        assert_eq!(score(&n, &q("lur ssi")).map(|s| s.0), Some(WORDS_ANYWHERE));
        assert_eq!(score(&n, &q("gb")).map(|s| s.0), Some(INITIALS + 20));
        assert_eq!(score(&Names::new("4-Color Gradient"), &q("4color")).map(|s| s.0), Some(COMPACT));
        let f = score(&Names::new("Fractal Noise"), &q("frctl")).map(|s| s.0).unwrap();
        assert!((FUZZY_BASE..=FUZZY_MAX).contains(&f), "{f}");
        assert!(score(&n, &q("zz")).is_none());
        assert!(score(&n, &q("")).is_none());
        // Letters spread over the whole name, or too few of them, aren't a match.
        assert!(score(&Names::new("Gaussian Blur"), &q("gnbr")).is_none());
        assert!(score(&Names::new("CC Page Turn"), &q("cur")).is_none());
        assert!(score(&Names::new("Curves"), &q("crvs")).is_some());
    }

    #[test]
    fn ranges_point_at_the_typed_text() {
        let n = Names::new("CC Radial Blur");
        assert_eq!(score(&n, &q("radial")).and_then(|s| s.1), Some(3..9));
        assert_eq!(score(&n, &q("  CC   radial ")).and_then(|s| s.1), Some(0..9));
    }

    #[test]
    fn hostile_text_is_matched_without_panicking() {
        for name in ["", " ", "日本語のエフェクト", "İstanbul Blur", "a\u{0301}b", "--", "x".repeat(10_000).as_str()] {
            let n = Names::new(name);
            for query in ["", "i", "İs", "日本", "\u{0301}", "-", "xx", "a b c d e f g", &"x".repeat(300)] {
                let _ = score(&n, &q(query));
            }
        }
    }
}
