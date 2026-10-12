//! The Settings dialog (After Effects' Preferences): the page list on the left, the page on the
//! right, OK / Cancel / Previous / Next. Pages are built from the engine's schema
//! (`effectcraft_engine::prefs::pages`), so the dialog, `prefs.pages` and the docs agree. Edits
//! apply live; Cancel restores the settings from when the dialog opened, OK saves them.
//!
//! Automation ids: `settings.page.<id>`, `settings.<key>` (e.g. `settings.general.undoLevels`),
//! `settings.labels.<n>.name` / `.color`, `settings.button.<label>`, `settings.ok`,
//! `settings.cancel`, `settings.previous`, `settings.next`.

use crate::i18n::{tr, tr_args};
use effectcraft_engine::prefs::{APPEARANCE_MODES, DARK_THEMES, Item, Kind, LIGHT_THEMES, Page, pages};
use effectcraft_engine::segment::Task;
use egui::{Color32, RichText, vec2};
use serde_json::{Value, json};

use crate::theme::{ThemeKind, Tokens};
use crate::{Dialog, EffectcraftApp};

/// Open the dialog on a page, remembering the settings for Cancel.
pub fn open(app: &mut EffectcraftApp, page: &str) {
    if app.dialog != Some(Dialog::Settings) {
        app.dialog_state.prefs_snapshot = Some(app.session.prefs.clone());
        app.dialog_state.settings_search.clear();
    }
    app.dialog_state.settings_page = page.to_string();
    app.dialog = Some(Dialog::Settings);
}

enum Act {
    Set(String, Value),
    Run(String, Value),
    PickFolder(String),
    PickProject(String),
    /// Install from File (a downloaded weights file) for a task's models.
    PickModel(Task),
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// sRGB components (0–1) as bytes.
fn srgb_bytes(c: [f32; 3]) -> [u8; 3] {
    let col = crate::widgets::srgb32(c);
    [col.r(), col.g(), col.b()]
}

fn reg(app: &mut EffectcraftApp, id: &str, r: &egui::Response, label: &str) {
    app.auto.add(id, r.rect, label);
}

/// Match rank prioritizing exact prefix (^<query>), then word boundary (\b<query>), then substring (*<query>*);
/// and English, then all translations, then description/context.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ItemRank {
    /// 1. English label starts with query (^<query>)
    EnLabelPrefix = 0,
    /// 2. English label word boundary starts with query (\b<query>)
    EnLabelWord = 1,
    /// 3. English label contains query (*<query>*)
    EnLabelSubstring = 2,
    /// 4. English setting key or command matches query
    EnKeyMatch = 3,
    /// 5. Translation in any language starts with query (^<query>)
    I18nLabelPrefix = 4,
    /// 6. Translation in any language word boundary starts with query (\b<query>)
    I18nLabelWord = 5,
    /// 7. Translation in any language contains query (*<query>*)
    I18nLabelSubstring = 6,
    /// 8. Section title or Note starts with query (^<query>)
    DescPrefix = 7,
    /// 9. Section title or Note word boundary starts with query (\b<query>)
    DescWord = 8,
    /// 10. Section title or Note contains query (*<query>*)
    DescSubstring = 9,
}

#[derive(Clone, Debug)]
pub struct SearchHit {
    pub page_id: &'static str,
    pub page_title: &'static str,
    pub section_title: &'static str,
    pub item: Item,
    pub rank: ItemRank,
    pub page_index: usize,
    pub item_index: usize,
}

fn matches_word_boundary(text: &str, query: &str) -> bool {
    let q = query.to_lowercase();
    let lower = text.to_lowercase();
    if q.is_empty() {
        return false;
    }
    let mut prev_char = ' ';
    for (i, c) in text.char_indices() {
        let is_boundary = if i == 0 {
            true
        } else if !prev_char.is_alphanumeric() && c.is_alphanumeric() {
            true
        } else if prev_char.is_lowercase() && c.is_uppercase() {
            true
        } else {
            false
        };
        prev_char = c;
        if is_boundary && lower[i..].starts_with(&q) {
            return true;
        }
    }
    false
}

fn evaluate_item_rank(
    query: &str,
    en_label: &str,
    key_or_cmd: &str,
    i18n_labels: &[&str],
    desc_texts: &[&str],
) -> Option<ItemRank> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return None;
    }
    let en_lower = en_label.to_lowercase();

    // 1. Zuerst Englisch: ^<query> (Prefix)
    if en_lower.starts_with(&q) {
        return Some(ItemRank::EnLabelPrefix);
    }
    // 2. Englisch: \b<query> (Word boundary)
    if matches_word_boundary(en_label, &q) {
        return Some(ItemRank::EnLabelWord);
    }
    // 3. Englisch: *<query>* (Substring)
    if en_lower.contains(&q) {
        return Some(ItemRank::EnLabelSubstring);
    }
    // 4. Setting-Key / Befehl (z.B. "general.undoLevels", "undoLevels")
    if !key_or_cmd.is_empty() {
        let key_lower = key_or_cmd.to_lowercase();
        if key_lower.starts_with(&q) || matches_word_boundary(key_or_cmd, &q) || key_lower.contains(&q) {
            return Some(ItemRank::EnKeyMatch);
        }
    }
    // 5. Übersetzungen: ^<query> (Prefix)
    for t in i18n_labels {
        let t_lower = t.to_lowercase();
        if t_lower.starts_with(&q) {
            return Some(ItemRank::I18nLabelPrefix);
        }
    }
    // 6. Übersetzungen: \b<query> (Word boundary)
    for t in i18n_labels {
        if matches_word_boundary(t, &q) {
            return Some(ItemRank::I18nLabelWord);
        }
    }
    // 7. Übersetzungen: *<query>* (Substring)
    for t in i18n_labels {
        let t_lower = t.to_lowercase();
        if t_lower.contains(&q) {
            return Some(ItemRank::I18nLabelSubstring);
        }
    }
    // 8. Description / Notiz / Sektion: ^<query> (Prefix)
    for d in desc_texts {
        let d_lower = d.to_lowercase();
        if d_lower.starts_with(&q) {
            return Some(ItemRank::DescPrefix);
        }
    }
    // 9. Description / Notiz / Sektion: \b<query> (Word boundary)
    for d in desc_texts {
        if matches_word_boundary(d, &q) {
            return Some(ItemRank::DescWord);
        }
    }
    // 10. Description / Notiz / Sektion: *<query>* (Substring)
    for d in desc_texts {
        let d_lower = d.to_lowercase();
        if d_lower.contains(&q) {
            return Some(ItemRank::DescSubstring);
        }
    }

    None
}

pub fn search_settings(all_pages: &[Page], query: &str) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    let q = query.trim();
    if q.is_empty() {
        return hits;
    }

    for (page_idx, page) in all_pages.iter().enumerate() {
        let mut cur_section = page.title;
        let mut notes_for_item: std::collections::HashMap<usize, &'static str> = std::collections::HashMap::new();
        for (i, item) in page.items.iter().enumerate() {
            if let Item::Note(n) = item {
                if i > 0 {
                    notes_for_item.insert(i - 1, *n);
                }
            }
        }

        for (item_idx, item) in page.items.iter().enumerate() {
            match item {
                Item::Section(sec) => {
                    cur_section = *sec;
                }
                Item::Note(_) => {}
                _ => {
                    let (en_label, key_or_cmd) = match item {
                        Item::Setting { label, key, .. } => (*label, *key),
                        Item::Button { label, command, .. } => (*label, *command),
                        Item::AudioDevices { key } => ("Default Output", *key),
                        Item::Labels => ("Labels", "labels"),
                        Item::BrowserStorage => ("Browser Storage", "storage"),
                        Item::Models(_) => ("Trained Models", "models"),
                        _ => ("", ""),
                    };

                    let tr_label = tr(en_label);
                    let mut i18n = vec![tr_label];
                    for t in crate::i18n::all_translations(en_label) {
                        if !i18n.contains(&t) {
                            i18n.push(t);
                        }
                    }

                    let mut desc = vec![page.title, cur_section];
                    if let Some(note) = notes_for_item.get(&item_idx) {
                        desc.push(*note);
                    }

                    if let Some(rank) = evaluate_item_rank(q, en_label, key_or_cmd, &i18n, &desc) {
                        hits.push(SearchHit {
                            page_id: page.id,
                            page_title: page.title,
                            section_title: cur_section,
                            item: *item,
                            rank,
                            page_index: page_idx,
                            item_index: item_idx,
                        });
                    }
                }
            }
        }
    }

    hits.sort_by_key(|h| (h.rank, h.page_index, h.item_index));
    hits
}

/// One item row in a page or search result list.
fn single_item_ui(app: &mut EffectcraftApp, ui: &mut egui::Ui, item: &Item, cur: &Value, t: &Tokens, acts: &mut Vec<Act>) {
    let get = |key: &str| cur.pointer(&format!("/{}", key.replace('.', "/"))).cloned().unwrap_or(Value::Null);
    let label_w = 250.0;
    match *item {
        Item::Section(title) => {
            ui.add_space(6.0);
            ui.label(RichText::new(tr(title)).font(Tokens::semibold(12.5)).color(t.tab_text_active));
            ui.separator();
        }
            Item::Note(text) => {
                ui.label(RichText::new(tr(text)).color(t.text_dim));
            }
            Item::Button { label, command, params } => {
                let r = ui.button(tr(label));
                reg(app, &format!("settings.button.{}", label.trim_end_matches('.')), &r, label);
                if r.clicked() {
                    acts.push(Act::Run(command.into(), serde_json::from_str(params).unwrap_or(json!({}))));
                }
            }
            Item::Labels => labels_ui(app, ui, cur, acts),
            Item::BrowserStorage => browser_storage_ui(app, ui, t, acts),
            Item::Models(task) => models_ui(app, ui, t, acts, task),
            Item::AudioDevices { key } => {
                let devices: Vec<String> = app.hooks.audio_devices.as_ref().map(|f| f()).unwrap_or_default();
                let sel = get(key).as_str().unwrap_or("").to_string();
                ui.horizontal(|ui| {
                    ui.add_sized(vec2(label_w, 18.0), egui::Label::new(tr("Default Output:")));
                    let shown = if sel.is_empty() { "System Default".to_string() } else { sel.clone() };
                    let r = egui::ComboBox::from_id_salt(("settings", key)).selected_text(shown).width(260.0).show_ui(ui, |ui| {
                        if ui.selectable_label(sel.is_empty(), tr("System Default")).clicked() {
                            acts.push(Act::Set(key.into(), json!("")));
                        }
                        for d in &devices {
                            if ui.selectable_label(*d == sel, d).clicked() {
                                acts.push(Act::Set(key.into(), json!(d)));
                            }
                        }
                    });
                    reg(app, &format!("settings.{key}"), &r.response, "Default Output");
                });
                if devices.is_empty() {
                    ui.label(RichText::new(tr("No device list from this host; the system default output is used.")).color(t.text_dim));
                }
            }
            // Appearance Mode with the light and dark theme cards (the per-mode themes are
            // chosen on the cards).
            Item::Setting { key: "appearance.appearanceMode", .. } => appearance_ui(app, ui, cur, t, acts),
            Item::Setting { key: "appearance.lightTheme" | "appearance.darkTheme", .. } => {}
            Item::Setting { key, label, kind, live } => {
                let v = get(key);
                let id = format!("settings.{key}");
                let tip = (!live).then_some("Stored, but not used by EffectCraft yet");
                let mut resp: Option<egui::Response> = None;
                match kind {
                    Kind::Bool => {
                        let mut b = v.as_bool().unwrap_or(false);
                        let r = ui.checkbox(&mut b, tr(label));
                        if r.changed() {
                            acts.push(Act::Set(key.into(), json!(b)));
                        }
                        resp = Some(r);
                    }
                    _ => {
                        ui.horizontal(|ui| {
                            ui.add_sized(vec2(label_w, 18.0), egui::Label::new(format!("{}:", tr(label))).truncate());
                            let r = match kind {
                                Kind::Int(lo, hi, unit) => {
                                    let mut x = v.as_i64().unwrap_or(0);
                                    let r = ui.add(crate::widgets::drag_value(egui::DragValue::new(&mut x).range(lo..=hi).speed(0.25)));
                                    if !unit.is_empty() {
                                        ui.label(RichText::new(unit).color(t.text_dim));
                                    }
                                    if r.changed() {
                                        acts.push(Act::Set(key.into(), json!(x)));
                                    }
                                    r
                                }
                                Kind::Float(lo, hi, unit) => {
                                    let mut x = v.as_f64().unwrap_or(0.0);
                                    let r = ui.add(crate::widgets::drag_value(egui::DragValue::new(&mut x).range(lo..=hi).speed(0.1).max_decimals(3)));
                                    if !unit.is_empty() {
                                        ui.label(RichText::new(unit).color(t.text_dim));
                                    }
                                    if r.changed() {
                                        acts.push(Act::Set(key.into(), json!(x)));
                                    }
                                    r
                                }
                                Kind::Slider(lo, hi) => {
                                    let mut x = v.as_f64().unwrap_or(0.0);
                                    ui.label(RichText::new(tr("Darker")).color(t.text_dim));
                                    let r = ui.add(egui::Slider::new(&mut x, lo..=hi).show_value(false));
                                    ui.label(RichText::new(tr("Lighter")).color(t.text_dim));
                                    if ui.small_button("Default").clicked() {
                                        acts.push(Act::Set(key.into(), json!(0.0)));
                                    }
                                    if r.changed() {
                                        acts.push(Act::Set(key.into(), json!(x)));
                                    }
                                    r
                                }
                                Kind::Choice(opts) => {
                                    // (A number with choices, UI Scale, shows as its text.)
                                    let curv = v.as_str().map_or_else(|| v.to_string(), str::to_string);
                                    let shown = opts.iter().find(|o| o.1 == curv).map(|o| o.0).unwrap_or(curv.as_str()).to_string();
                                    egui::ComboBox::from_id_salt(("settings", key))
                                        .selected_text(tr(&shown))
                                        .width(220.0)
                                        .show_ui(ui, |ui| {
                                            for (l, val) in opts {
                                                if ui.selectable_label(*val == curv, tr(l)).clicked() {
                                                    acts.push(Act::Set(key.into(), json!(val)));
                                                }
                                            }
                                        })
                                        .response
                                }
                                Kind::Text | Kind::Path => {
                                    let mut s = v.as_str().unwrap_or("").to_string();
                                    let r = ui.add(egui::TextEdit::singleline(&mut s).desired_width(if kind == Kind::Path { 230.0 } else { 260.0 }));
                                    if r.changed() {
                                        acts.push(Act::Set(key.into(), json!(s)));
                                    }
                                    if kind == Kind::Path {
                                        let b = ui.button(tr("Choose..."));
                                        reg(app, &format!("{id}.choose"), &b, "Choose...");
                                        if b.clicked() {
                                            acts.push(if key.ends_with("Path") { Act::PickProject(key.into()) } else { Act::PickFolder(key.into()) });
                                        }
                                    }
                                    r
                                }
                                Kind::Color => {
                                    let c = crate::panels::viewer::hex_rgb(v.as_str().unwrap_or("")).unwrap_or([128, 128, 128]);
                                    let mut f = c.map(|v| v as f32 / 255.0);
                                    let r = crate::widgets::srgb_color_button(ui, &mut f);
                                    let rgb = if r.changed() { srgb_bytes(f) } else { c };
                                    ui.label(RichText::new(hex(rgb)).monospace().color(t.text_dim));
                                    if rgb != c {
                                        acts.push(Act::Set(key.into(), json!(hex(rgb))));
                                    }
                                    r
                                }
                                // Checkboxes are drawn above; nothing to edit here.
                                Kind::Bool => ui.label(""),
                            };
                            resp = Some(r);
                        });
                    }
                }
                if let Some(r) = resp {
                    let r = match tip {
                        Some(tip) => r.on_hover_text(tip),
                        None => r,
                    };
                    reg(app, &id, &r, label);
                }
            }
        }
    }

/// One page's rows.
fn page_ui(app: &mut EffectcraftApp, ui: &mut egui::Ui, page: &Page, cur: &Value, t: &Tokens, acts: &mut Vec<Act>) {
    for item in &page.items {
        single_item_ui(app, ui, item, cur, t, acts);
    }
}

/// A small EffectCraft window painted with theme `kind`'s tokens: the Tools bar, the Project
/// panel, the Composition panel, the Preview and Properties column and the Timeline with its
/// layer bars.
fn theme_preview(ui: &mut egui::Ui, kind: ThemeKind, width: f32) {
    use egui::{Rect, pos2};
    let p = Tokens::for_kind(kind);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 118.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, p.app_bg);
    let bar = Rect::from_min_size(rect.min, vec2(rect.width(), 13.0));
    painter.rect_filled(bar, 0.0, p.header_bg);
    // The brand mark, the tools (the Selection tool is active) and the Discord pill.
    painter.rect_filled(Rect::from_min_size(bar.min + vec2(4.0, 3.0), vec2(7.0, 7.0)), 2.0, p.accent);
    for i in 0..7 {
        let c = if i == 0 { p.accent } else { p.icon };
        painter.rect_filled(Rect::from_min_size(bar.min + vec2(16.0 + i as f32 * 8.0, 4.0), vec2(5.0, 5.0)), 1.0, c);
    }
    painter.rect_filled(
        Rect::from_min_max(pos2(bar.right() - 22.0, bar.top() + 3.0), pos2(bar.right() - 4.0, bar.bottom() - 3.0)),
        3.5,
        egui::Color32::from_rgb(0x58, 0x65, 0xf2),
    );
    let gap = 2.0;
    let body = Rect::from_min_max(pos2(rect.left() + gap, bar.bottom() + gap), pos2(rect.right() - gap, rect.bottom() - gap));
    let right_w = (body.width() * 0.22).round();
    let timeline_h = (body.height() * 0.42).round();
    let left_w = (body.width() * 0.2).round();
    let upper = Rect::from_min_max(body.min, pos2(body.right() - right_w - gap, body.bottom() - timeline_h - gap));
    let panel = |r: Rect, title_w: f32| {
        painter.rect_filled(r, 2.0, p.panel_bg);
        painter.rect_filled(Rect::from_min_size(r.min + vec2(4.0, 3.0), vec2(title_w, 2.0)), 1.0, p.tab_text_active);
    };
    // Project panel: rows with label swatches.
    let project = Rect::from_min_max(upper.min, pos2(upper.left() + left_w, upper.bottom()));
    panel(project, 14.0);
    for (i, label) in [5usize, 5, 2, 2].into_iter().enumerate() {
        let y = project.top() + 10.0 + i as f32 * 6.0;
        painter.rect_filled(Rect::from_min_size(pos2(project.left() + 4.0, y), vec2(3.0, 3.0)), 0.5, p.labels.get(label).copied().unwrap_or(p.icon));
        painter.rect_filled(Rect::from_min_size(pos2(project.left() + 9.0, y + 0.5), vec2(project.width() * 0.45, 2.0)), 1.0, p.text_dim);
    }
    // Composition panel: the pasteboard and the comp frame with a title.
    let comp = Rect::from_min_max(pos2(project.right() + gap, upper.top()), upper.max);
    panel(comp, 22.0);
    let paste = Rect::from_min_max(comp.min + vec2(2.0, 8.0), comp.max - vec2(2.0, 2.0));
    painter.rect_filled(paste, 0.0, p.pasteboard);
    let frame_h = (paste.height() - 6.0).max(4.0);
    let frame = Rect::from_center_size(paste.center(), vec2((frame_h * 16.0 / 9.0).min(paste.width() - 6.0), frame_h));
    painter.rect_filled(frame, 0.0, egui::Color32::from_rgb(0x14, 0x1c, 0x4a));
    painter.circle_stroke(frame.center(), frame.height() * 0.36, egui::Stroke::new(0.8, egui::Color32::from_rgb(0x4a, 0x7c, 0xe8)));
    painter.rect_filled(Rect::from_center_size(frame.center(), vec2(frame.width() * 0.5, 2.5)), 1.0, egui::Color32::WHITE);
    // Preview and Properties.
    let column = Rect::from_min_max(pos2(body.right() - right_w, body.top()), body.max);
    let preview = Rect::from_min_max(column.min, pos2(column.right(), column.top() + column.height() * 0.3));
    panel(preview, 12.0);
    painter.rect_filled(Rect::from_min_size(preview.left_bottom() + vec2(4.0, -5.0), vec2(preview.width() * 0.25, 1.5)), 0.5, p.cache_green);
    let props = Rect::from_min_max(pos2(column.left(), preview.bottom() + gap), column.max);
    panel(props, 18.0);
    for i in 0..5 {
        let y = props.top() + 10.0 + i as f32 * 6.0;
        painter.rect_filled(Rect::from_min_size(pos2(props.left() + 4.0, y), vec2(props.width() * 0.35, 2.0)), 1.0, p.text_dim);
        painter.rect_filled(Rect::from_min_size(pos2(props.left() + props.width() * 0.55, y), vec2(props.width() * 0.25, 2.0)), 1.0, p.hot_text);
    }
    // Timeline: the layer list and the time graph, with coloured layer bars and the time marker.
    let timeline = Rect::from_min_max(pos2(body.left(), upper.bottom() + gap), pos2(upper.right(), body.bottom()));
    panel(timeline, 18.0);
    painter.rect_filled(Rect::from_min_size(timeline.min + vec2(4.0, 8.0), vec2(14.0, 3.0)), 1.0, p.timecode);
    let graph = Rect::from_min_max(pos2(timeline.left() + timeline.width() * 0.38, timeline.top() + 8.0), timeline.max - vec2(2.0, 2.0));
    painter.rect_filled(graph, 0.0, p.tl_bg);
    painter.rect_filled(Rect::from_min_size(graph.min, vec2(graph.width(), 4.0)), 0.0, p.tl_ruler_bg);
    for (i, label) in [1usize, 1, 8, 8, 1].into_iter().enumerate() {
        let y = graph.top() + 7.0 + i as f32 * 5.5;
        if y + 3.5 > graph.bottom() {
            break;
        }
        let swatch = p.labels.get(label).copied().unwrap_or(p.icon);
        let row_l = timeline.left() + 4.0;
        if i == 1 {
            painter.rect_filled(Rect::from_min_max(pos2(timeline.left(), y - 1.0), pos2(graph.left(), y + 4.5)), 0.0, p.row_selected);
        }
        painter.rect_filled(Rect::from_min_size(pos2(row_l, y), vec2(3.0, 3.0)), 0.5, swatch);
        painter.rect_filled(Rect::from_min_size(pos2(row_l + 6.0, y + 0.5), vec2(graph.left() - row_l - 14.0, 2.0)), 1.0, p.text_dim);
        let bar = Rect::from_min_max(pos2(graph.left() + 1.0, y), pos2(graph.right() - 1.0, y + 3.5));
        painter.rect_filled(bar, 1.0, swatch.gamma_multiply(0.8));
    }
    let cti = graph.left() + graph.width() * 0.3;
    painter.line_segment([pos2(cti, graph.top()), pos2(cti, graph.bottom())], egui::Stroke::new(1.0, p.cti));
}

/// One theme family's card: its (translated) name, Active when the appearance shows it now, a preview of the
/// chosen theme and a radio button per theme.
fn theme_card(
    app: &mut EffectcraftApp,
    ui: &mut egui::Ui,
    t: &Tokens,
    key: &str,
    title: &str,
    active: bool,
    cur: &str,
    options: &[(&str, &str)],
    acts: &mut Vec<Act>,
) {
    egui::Frame::new()
        .fill(t.field_bg)
        .stroke(egui::Stroke::new(1.0, if active { t.accent } else { t.field_border }))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).font(Tokens::semibold(12.5)).color(t.text));
                if active {
                    ui.label(RichText::new(tr("Active")).color(t.accent).small());
                }
            });
            ui.add_space(4.0);
            let kind = ThemeKind::from_name(cur).unwrap_or(if key.ends_with("lightTheme") { ThemeKind::Light } else { ThemeKind::Dark });
            theme_preview(ui, kind, ui.available_width());
            ui.add_space(4.0);
            for (label, value) in options {
                let r = ui.radio(cur == *value, tr(label));
                reg(app, &format!("settings.{key}.{value}"), &r, label);
                if r.clicked() && cur != *value {
                    acts.push(Act::Set(key.into(), json!(value)));
                }
            }
        });
}

/// Settings ▸ Appearance: Appearance Mode (Sync with System, Dark, Light) above the light and dark
/// theme cards.
fn appearance_ui(app: &mut EffectcraftApp, ui: &mut egui::Ui, cur: &Value, t: &Tokens, acts: &mut Vec<Act>) {
    let get = |key: &str| cur.pointer(&format!("/appearance/{key}")).and_then(Value::as_str).unwrap_or_default().to_string();
    let (mode, light, dark) = (get("appearanceMode"), get("lightTheme"), get("darkTheme"));
    ui.horizontal(|ui| {
        ui.add_sized(vec2(250.0, 18.0), egui::Label::new(format!("{}:", tr("Appearance Mode"))).truncate());
        let shown = APPEARANCE_MODES.iter().find(|(_, v)| *v == mode).map_or(mode.as_str(), |(l, _)| *l);
        let r = egui::ComboBox::from_id_salt(("settings", "appearance.appearanceMode"))
            .selected_text(tr(shown))
            .width(220.0)
            .show_ui(ui, |ui| {
                for (l, v) in APPEARANCE_MODES {
                    if ui.selectable_label(*v == mode, tr(l)).clicked() {
                        acts.push(Act::Set("appearance.appearanceMode".into(), json!(v)));
                    }
                }
            })
            .response;
        reg(app, "settings.appearance.appearanceMode", &r, "Appearance Mode");
    });
    ui.add_space(6.0);
    let system_light = app.system_light(ui.ctx());
    let light_active = mode == "light" || (mode == "auto" && system_light == Some(true));
    ui.columns(2, |cols| {
        let [l, d] = cols else { return };
        theme_card(app, l, t, "appearance.lightTheme", tr("Light Theme"), light_active, &light, LIGHT_THEMES, acts);
        theme_card(app, d, t, "appearance.darkTheme", tr("Dark Theme"), !light_active, &dark, DARK_THEMES, acts);
    });
    ui.add_space(6.0);
}

/// Bytes as "12.3 MB".
pub fn human_bytes(b: u64) -> String {
    let units = ["bytes", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut u = 0;
    while v >= 1000.0 && u + 1 < units.len() {
        v /= 1000.0;
        u += 1;
    }
    if u == 0 { format!("{b} bytes") } else { format!("{v:.1} {}", units[u]) }
}

/// Settings ▸ Roto Brush and Settings ▸ Face Tracking: a task's models (`roto.models`,
/// `face.models`): choose one; download, install from a file or remove the trained ones; who made
/// them, their licence, size and source.
fn models_ui(app: &mut EffectcraftApp, ui: &mut egui::Ui, t: &Tokens, acts: &mut Vec<Act>, task: Task) {
    let (prefix, user) = match task {
        Task::Mask => ("roto", "Roto Brush"),
        Task::Face => ("face", "Face tracking"),
    };
    let Ok(info) = app.session.execute(&format!("{prefix}.models"), json!({})) else { return };
    let web = cfg!(target_arch = "wasm32");
    let busy = info["busy"].as_str().map(str::to_string);
    for m in info["models"].as_array().into_iter().flatten() {
        let id = m["id"].as_str().unwrap_or_default().to_string();
        let name = m["name"].as_str().unwrap_or_default();
        let classic = id == effectcraft_engine::segment::CLASSICAL;
        let (installed, selected, active) =
            (m["installed"].as_bool().unwrap_or(false), m["selected"].as_bool().unwrap_or(false), m["active"].as_bool().unwrap_or(false));
        ui.add_space(6.0);
        egui::Frame::new().fill(t.field_bg).corner_radius(6.0).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let r = ui.radio(selected, RichText::new(name).font(Tokens::semibold(12.5)));
                reg(app, &format!("settings.{prefix}.model.{id}"), &r, name);
                if r.clicked() && !selected {
                    acts.push(Act::Run(format!("{prefix}.model.select"), json!({"id": id})));
                }
                let state = match (active, installed, selected) {
                    (true, _, _) => "In use".to_string(),
                    _ if classic => "Built in".to_string(),
                    (false, true, true) if busy.is_some() => "Loading…".to_string(),
                    (false, true, _) => "Installed".to_string(),
                    (false, false, true) => format!("Not installed: {user} uses the classic engine"),
                    _ => "Not installed".to_string(),
                };
                ui.label(RichText::new(state).color(if active { t.accent } else { t.text_dim }));
            });
            ui.label(RichText::new(m["description"].as_str().unwrap_or_default()).color(t.text_dim));
            if let Some(by) = m["authors"].as_str() {
                ui.label(RichText::new(format!("By {by}")).color(t.text_dim));
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(tr_args("Licence: {}", &[&(m["licence"].as_str().unwrap_or_default())])).color(t.text_dim));
                if let Some(size) = m["size"].as_u64() {
                    ui.label(RichText::new(format!("·  {}", human_bytes(size))).color(t.text_dim));
                }
                for (label, key) in [("Project page", "homepage"), ("Licence", "licenceUrl")] {
                    if let Some(url) = m[key].as_str() {
                        let r = ui.link(label);
                        if r.clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                        }
                    }
                }
            });
            if classic {
                return;
            }
            ui.horizontal(|ui| {
                if !installed && !web {
                    let r = ui.add_enabled(busy.is_none(), egui::Button::new(tr("Download")));
                    reg(app, &format!("settings.{prefix}.download.{id}"), &r, "Download");
                    if r.clicked() {
                        acts.push(Act::Run(format!("{prefix}.model.download"), json!({"id": id})));
                    }
                }
                if !web {
                    let r = ui.add_enabled(busy.is_none(), egui::Button::new(tr("Install from File…")));
                    reg(app, &format!("settings.{prefix}.install.{id}"), &r, "Install from File");
                    if r.clicked() {
                        acts.push(Act::PickModel(task));
                    }
                }
                if installed {
                    let r = ui.add_enabled(busy.is_none(), egui::Button::new(tr("Remove")));
                    reg(app, &format!("settings.{prefix}.remove.{id}"), &r, "Remove");
                    if r.clicked() {
                        acts.push(Act::Run(format!("{prefix}.model.remove"), json!({"id": id})));
                    }
                }
            });
        });
    }
    if let Some(b) = &busy {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(b);
        });
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    }
    if let Some(e) = info["error"].as_str() {
        ui.label(RichText::new(e).color(t.danger));
    }
    if web {
        ui.label(RichText::new(tr("Trained models are available in the desktop app; the browser uses the classic engine.")).color(t.text_dim));
    } else if let Some(f) = info["folder"].as_str() {
        ui.label(RichText::new(tr_args("Models folder: {}", &[&f])).color(t.text_faint));
    }
}

/// Settings ▸ Disk ▸ Browser Storage (the web app's storage manager, `storage.*`): where the
/// data lives, the origin's usage and quota, persistent storage, and Clear buttons.
fn browser_storage_ui(app: &mut EffectcraftApp, ui: &mut egui::Ui, t: &Tokens, acts: &mut Vec<Act>) {
    let Some(host) = app.session.storage.clone() else { return };
    let info = host.info();
    let u = |v: &Value| v.as_u64().unwrap_or(0);
    ui.add_space(6.0);
    ui.label(RichText::new(tr("Browser Storage")).font(Tokens::semibold(12.5)).color(t.tab_text_active));
    ui.separator();
    let backend = match info["backend"].as_str().unwrap_or("") {
        "opfs" => "Origin Private File System",
        "indexeddb" => "IndexedDB",
        "memory" => "Memory only: nothing is kept after a reload",
        "" => "Unknown",
        other => other,
    };
    let r = ui.label(tr_args("Stored in: {}", &[&backend]));
    reg(app, "settings.storage.backend", &r, backend);
    let (usage, quota) = (u(&info["usage"]), u(&info["quota"]));
    let frac = if quota > 0 { usage as f32 / quota as f32 } else { 0.0 };
    let text = if quota > 0 { format!("{} of {} used ({:.1} %)", human_bytes(usage), human_bytes(quota), frac * 100.0) } else { "Usage unknown".into() };
    let r = ui
        .horizontal(|ui| {
            ui.add(egui::ProgressBar::new(frac.clamp(0.0, 1.0)).desired_width(220.0).desired_height(8.0));
            ui.label(&text);
        })
        .response;
    reg(app, "settings.storage.usage", &r, &text);
    let f = &info["files"];
    let row = |what: &str, k: &str| format!("{what}: {} ({})", u(&f[k]["count"]), human_bytes(u(&f[k]["bytes"])));
    ui.label(
        RichText::new(format!("{} · {} · {}", row(tr("Projects"), "projects"), row(tr("Imported media"), "media"), row(tr("Auto-saves"), "autoSaves")))
            .color(t.text_dim),
    );
    let d = &info["diskCache"];
    let dc = if d["enabled"].as_bool().unwrap_or(false) {
        format!(
            "Disk cache: {} frames, {} of {} (hits {}, misses {})",
            u(&d["entries"]),
            human_bytes(u(&d["bytes"])),
            human_bytes(u(&d["maxBytes"])),
            u(&d["hits"]),
            u(&d["misses"])
        )
    } else {
        "Disk cache: off".into()
    };
    let r = ui.label(RichText::new(&dc).color(t.text_dim));
    reg(app, "settings.storage.diskCache", &r, &dc);
    ui.horizontal(|ui| {
        let persisted = info["persisted"].as_bool().unwrap_or(false);
        let state = if persisted { "Persistent storage: granted" } else { "Persistent storage: not granted (the browser may clear it when space runs low)" };
        ui.label(state);
        if !persisted {
            let r = ui.button(tr("Request Persistent Storage"));
            reg(app, "settings.storage.persist", &r, "Request Persistent Storage");
            if r.clicked() {
                acts.push(Act::Run("storage.persist".into(), json!({})));
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        for (what, label) in
            [("diskCache", "Clear Disk Cache"), ("media", "Clear Imported Media"), ("autoSaves", "Clear Auto-Saves"), ("projects", "Clear Saved Projects")]
        {
            let r = ui.button(label);
            reg(app, &format!("settings.storage.clear.{what}"), &r, label);
            if r.clicked() {
                acts.push(Act::Run("storage.clear".into(), json!({"what": what})));
            }
        }
    });
    // The numbers refresh in the background.
    ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
}

fn labels_ui(app: &mut EffectcraftApp, ui: &mut egui::Ui, cur: &Value, acts: &mut Vec<Act>) {
    ui.label(tr("Label Colors and Names:"));
    ui.add_space(4.0);
    let labels = cur.get("labels").and_then(Value::as_array).cloned().unwrap_or_default();
    egui::Grid::new("settings-labels").num_columns(4).spacing([10.0, 4.0]).show(ui, |ui| {
        for (i, l) in labels.iter().enumerate() {
            let c = crate::panels::viewer::hex_rgb(l["color"].as_str().unwrap_or("")).unwrap_or([128, 128, 128]);
            let mut f = c.map(|v| v as f32 / 255.0);
            let r = crate::widgets::srgb_color_button(ui, &mut f);
            let rgb = if r.changed() { srgb_bytes(f) } else { c };
            app.auto.add(&format!("settings.labels.{i}.color"), r.rect, "label colour");
            if rgb != c {
                acts.push(Act::Set(format!("labels.{i}.color"), json!(hex(rgb))));
            }
            let mut name = l["name"].as_str().unwrap_or("").to_string();
            let r = ui.add_sized(vec2(160.0, 20.0), egui::TextEdit::singleline(&mut name));
            app.auto.add(&format!("settings.labels.{i}.name"), r.rect, "label name");
            if r.changed() {
                acts.push(Act::Set(format!("labels.{i}.name"), json!(name)));
            }
            if i % 2 == 1 {
                ui.end_row();
            }
        }
    });
    ui.add_space(8.0);
    let r = ui.button(tr("Reset Labels"));
    app.auto.add("settings.button.Reset Labels", r.rect, "Reset Labels");
    if r.clicked() {
        acts.push(Act::Run("prefs.reset".into(), json!({"page": "labels"})));
    }
}

pub fn show(app: &mut EffectcraftApp, ctx: &egui::Context, t: &Tokens) {
    let all = pages();
    let mut page = app.dialog_state.settings_page.clone();
    let mut search = app.dialog_state.settings_search.clone();
    let hits = search_settings(&all, &search);
    let searching = !search.trim().is_empty();

    if !all.iter().any(|p| p.id == page) {
        page = "general".into();
    }
    let idx = all.iter().position(|p| p.id == page).unwrap_or(0);
    let cur = serde_json::to_value(&app.session.prefs).unwrap_or_default();
    let mut acts: Vec<Act> = vec![];
    let (mut ok, mut cancel) = (false, false);
    super::dialogs::modal(ctx, "Settings", vec2(820.0, 600.0), t, |ui| {
        let body_h = (ctx.content_rect().height() - 170.0).clamp(120.0, 470.0);
        ui.allocate_ui_with_layout(vec2(ui.available_width(), body_h), egui::Layout::left_to_right(egui::Align::Min), |ui| {
            ui.set_height(body_h);
            // Page list (left, like After Effects) with sticky search input at the top.
            ui.vertical(|ui| {
                ui.set_width(180.0);

                // Sticky search input: stays visible even when page list scrolls
                ui.horizontal(|ui| {
                    let has_text = !search.is_empty();
                    let edit_w = if has_text { 146.0 } else { 172.0 };
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut search)
                            .desired_width(edit_w)
                            .hint_text(tr("Search..."))
                    );
                    reg(app, "settings.search", &r, "Search");
                    if has_text {
                        let clear_btn = ui.small_button("×");
                        reg(app, "settings.search_clear", &clear_btn, "Clear Search");
                        if clear_btn.clicked() {
                            search.clear();
                        }
                    }
                });
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(2.0);

                let list_h = (body_h - 36.0).max(60.0);
                egui::ScrollArea::vertical().id_salt("settings-pages").max_height(list_h).auto_shrink([false, false]).show(ui, |ui| {
                    if searching {
                        let (r, resp) = ui.allocate_exact_size(vec2(176.0, 24.0), egui::Sense::hover());
                        ui.painter().rect_filled(r, 3.0, t.accent);
                        ui.painter().text(
                            r.left_center() + vec2(10.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            format!("🔍 {} ({})", tr("Results"), hits.len()),
                            Tokens::ui(12.5),
                            Color32::WHITE,
                        );
                        reg(app, "settings.page.search_results", &resp, "Search Results");
                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(4.0);
                    }
                    for p in &all {
                        let (r, resp) = ui.allocate_exact_size(vec2(176.0, 24.0), egui::Sense::click());
                        let sel = !searching && page == p.id;
                        if sel {
                            ui.painter().rect_filled(r, 3.0, t.accent);
                        } else if resp.hovered() {
                            ui.painter().rect_filled(r, 3.0, t.hover);
                        }
                        ui.painter().text(
                            r.left_center() + vec2(10.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            tr(p.title),
                            Tokens::ui(12.5),
                            if sel { Color32::WHITE } else { t.text },
                        );
                        app.auto.add(&format!("settings.page.{}", p.id), r, p.title);
                        if resp.clicked() {
                            search.clear();
                            page = p.id.to_string();
                        }
                    }
                });
            });
            ui.separator();
            ui.vertical(|ui| {
                // The rest of the dialog's width: a fixed width ran past it, so the whole
                // dialog scrolled sideways a little (#284).
                ui.set_width(ui.available_width());
                if searching {
                    // Separate Result List!
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(tr("Search Results")).font(Tokens::semibold(14.0)).color(t.tab_text_active));
                        ui.add_space(8.0);
                        let count = hits.len();
                        let count_str = if count == 0 {
                            tr("No matching settings").to_string()
                        } else if count == 1 {
                            tr("1 setting found").to_string()
                        } else {
                            tr_args("{} settings found", &[&count])
                        };
                        ui.label(RichText::new(count_str).small().color(t.text_dim));
                    });
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical().id_salt("settings-search-scroll").max_height(body_h - 30.0).auto_shrink([false, false]).show(ui, |ui| {
                        if hits.is_empty() {
                            ui.add_space(40.0);
                            ui.vertical_centered(|ui| {
                                ui.label(RichText::new(tr("No settings match your search.")).color(t.text_dim));
                                ui.add_space(10.0);
                                if ui.button(tr("Clear Search")).clicked() {
                                    search.clear();
                                }
                            });
                        } else {
                            for (hit_i, hit) in hits.iter().enumerate() {
                                if hit_i > 0 {
                                    ui.add_space(6.0);
                                    ui.separator();
                                    ui.add_space(4.0);
                                }
                                ui.horizontal(|ui| {
                                    let breadcrumb = format!("{} ▸ {}", tr(hit.page_title), tr(hit.section_title));
                                    ui.label(RichText::new(breadcrumb).small().color(t.accent));
                                });
                                ui.add_space(2.0);
                                single_item_ui(app, ui, &hit.item, &cur, t, &mut acts);
                            }
                            ui.add_space(10.0);
                        }
                    });
                } else {
                    let p = &all[idx];
                    ui.label(RichText::new(tr(p.title)).font(Tokens::semibold(14.0)).color(t.tab_text_active));
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical().max_height(body_h - 30.0).auto_shrink([false, false]).show(ui, |ui| {
                        page_ui(app, ui, p, &cur, t, &mut acts);
                    });
                }
            });
        });
        ui.add_space(8.0);
        ui.separator();
        ui.horizontal(|ui| {
            let r = ui.add_enabled(!searching && idx > 0, egui::Button::new(tr("Previous")));
            app.auto.add("settings.previous", r.rect, "Previous");
            if r.clicked() {
                page = all[idx - 1].id.to_string();
            }
            let r = ui.add_enabled(!searching && idx + 1 < all.len(), egui::Button::new(tr("Next")));
            app.auto.add("settings.next", r.rect, "Next");
            if r.clicked() {
                page = all[idx + 1].id.to_string();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let r = ui.add(egui::Button::new(RichText::new(tr("   OK   ")).color(Color32::WHITE)).fill(t.accent));
                app.auto.add("settings.ok", r.rect, "OK");
                ok |= r.clicked();
                let r = ui.button(tr("Cancel"));
                app.auto.add("settings.cancel", r.rect, "Cancel");
                cancel |= r.clicked();
            });
        });
    });
    app.dialog_state.settings_page = page;
    app.dialog_state.settings_search = search;
    for a in acts {
        match a {
            Act::Set(k, v) => {
                if let Err(e) = app.session.prefs.set(&k, v) {
                    app.ui.status = e;
                }
                app.session.prefs_changed();
            }
            Act::Run(cmd, params) => {
                if let Err(e) = crate::menus::invoke(app, ctx, &cmd, params) {
                    app.ui.status = e;
                }
                // Buttons such as Reset Settings change the baseline Cancel returns to.
                if cmd.starts_with("prefs.") {
                    app.dialog_state.prefs_snapshot = Some(app.session.prefs.clone());
                }
                if app.dialog != Some(Dialog::Settings) && app.dialog != Some(Dialog::Info) {
                    app.dialog = Some(Dialog::Settings);
                }
            }
            Act::PickFolder(k) => {
                if let Some(f) = app.hooks.pick_folder.as_ref().and_then(|f| f()) {
                    let _ = app.session.prefs.set(&k, json!(f));
                    app.session.prefs_changed();
                }
            }
            Act::PickProject(k) => {
                if let Some(f) = app.hooks.pick_open_project.as_ref().and_then(|f| f()) {
                    let _ = app.session.prefs.set(&k, json!(f));
                    app.session.prefs_changed();
                }
            }
            Act::PickModel(task) => {
                // The registry's file types for the task (`.pt`, `.task`).
                let mut exts: Vec<&str> = effectcraft_engine::segment::models(task).filter_map(|m| m.file_name.rsplit_once('.').map(|x| x.1)).collect();
                exts.dedup();
                let picked = app.hooks.pick_files.as_ref().map(|f| f(&exts)).unwrap_or_default();
                let cmd = if task == Task::Face { "face.model.install" } else { "roto.model.install" };
                if let Some(path) = picked.into_iter().next()
                    && let Err(e) = crate::menus::invoke(app, ctx, cmd, json!({"path": path}))
                {
                    app.ui.status = e;
                }
            }
        }
    }
    if cancel {
        if let Some(p) = app.dialog_state.prefs_snapshot.take() {
            app.session.prefs = p;
            app.session.prefs_changed();
        }
        app.dialog = None;
    } else if ok {
        app.dialog_state.prefs_snapshot = None;
        app.session.save_prefs();
        app.dialog = None;
    }
}

/// Crash recovery: the previous session didn't exit cleanly; offer its latest auto-save.
pub fn recovery(app: &mut EffectcraftApp, ctx: &egui::Context, t: &Tokens) {
    let Some(r) = app.recovery.clone() else {
        app.dialog = None;
        return;
    };
    let mut choice: Option<&str> = None;
    super::dialogs::modal(ctx, "Recover Project", vec2(560.0, 230.0), t, |ui| {
        ui.label(tr("EffectCraft didn't quit normally last time."));
        ui.add_space(6.0);
        if let Some(p) = &r.project {
            ui.label(tr_args("Project: {}", &[&p]));
        }
        if let Some(a) = &r.autosave {
            let when = std::fs::metadata(a).and_then(|m| m.modified()).ok().and_then(|m| m.elapsed().ok()).map(|d| {
                let m = d.as_secs() / 60;
                if m < 1 {
                    "less than a minute ago".to_string()
                } else if m < 120 {
                    format!("{m} minutes ago")
                } else {
                    format!("{} hours ago", m / 60)
                }
            });
            ui.label(tr_args("Latest auto-save: {}", &[&a]));
            if let Some(w) = when {
                ui.label(RichText::new(tr_args("Saved {}", &[&w])).color(t.text_dim));
            }
        }
        ui.add_space(14.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let b = ui.add(egui::Button::new(RichText::new(tr(" Open Auto-Save ")).color(Color32::WHITE)).fill(t.accent));
            app.auto.add("recovery.openAutoSave", b.rect, "Open Auto-Save");
            if b.clicked() {
                choice = Some("autosave");
            }
            if r.project.is_some() {
                let b = ui.button(tr("Open Last Saved Project"));
                app.auto.add("recovery.openProject", b.rect, "Open Last Saved Project");
                if b.clicked() {
                    choice = Some("project");
                }
            }
            let b = ui.button(tr("Don't Recover"));
            app.auto.add("recovery.dismiss", b.rect, "Don't Recover");
            if b.clicked() {
                choice = Some("none");
            }
        });
    });
    let Some(c) = choice else { return };
    app.dialog = None;
    app.recovery = None;
    let path = match c {
        "autosave" => r.autosave,
        "project" => r.project,
        _ => None,
    };
    if let Some(p) = path
        && let Err(e) = crate::menus::invoke(app, ctx, "file.open", json!({"path": p}))
    {
        app.ui.status = e;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_boundary_matching() {
        assert!(matches_word_boundary("Levels of Undo", "undo"));
        assert!(matches_word_boundary("Levels of Undo", "levels"));
        assert!(matches_word_boundary("Levels of Undo", "of"));
        assert!(matches_word_boundary("undoLevels", "levels"));
        assert!(matches_word_boundary("general.undoLevels", "undo"));
        assert!(!matches_word_boundary("Blaundo", "undo"));
    }

    #[test]
    fn item_rank_hierarchy() {
        // 1. Prefix beats word boundary beats substring
        let rank_prefix = evaluate_item_rank("undo", "Undo Levels", "", &[], &[]).unwrap();
        let rank_word = evaluate_item_rank("undo", "Levels of Undo", "", &[], &[]).unwrap();
        let rank_sub = evaluate_item_rank("undo", "Soundtrack", "", &[], &[]).unwrap();
        assert_eq!(rank_prefix, ItemRank::EnLabelPrefix);
        assert_eq!(rank_word, ItemRank::EnLabelWord);
        assert_eq!(rank_sub, ItemRank::EnLabelSubstring);
        assert!(rank_prefix < rank_word);
        assert!(rank_word < rank_sub);

        // 2. English beats translations beats description
        let rank_en = evaluate_item_rank("auto", "Auto-Save", "", &[], &[]).unwrap();
        let rank_i18n = evaluate_item_rank("auto", "Speichern", "", &["Auto-Save translated"], &[]).unwrap();
        let rank_desc = evaluate_item_rank("auto", "Projects", "", &[], &["Auto-Save section"]).unwrap();
        assert_eq!(rank_en, ItemRank::EnLabelPrefix);
        assert_eq!(rank_i18n, ItemRank::I18nLabelPrefix);
        assert_eq!(rank_desc, ItemRank::DescPrefix);
        assert!(rank_en < rank_i18n);
        assert!(rank_i18n < rank_desc);
    }

    #[test]
    fn settings_search_returns_ranked_hits() {
        let pages = pages();
        let hits = search_settings(&pages, "undo");
        assert!(!hits.is_empty(), "finds undo setting");
        assert!(hits.iter().any(|h| match h.item {
            Item::Setting { key, .. } => key == "general.undoLevels",
            _ => false,
        }));
    }
}

