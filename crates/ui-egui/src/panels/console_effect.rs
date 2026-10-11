//! Console Effect (Edit ▸ Console Effect…, Ctrl+Space): a search box that opens at the pointer and
//! applies effects and animation presets from the keyboard, its settings dialog, the snapshot
//! gallery window and the dockable Console Effect panel. The search, what the console learns and the
//! snapshots are the engine's (`effectcraft_engine::console_effect`, the `consoleEffect.*` commands); this
//! module draws them.
//!
//! Keys in the console: type to search; ↑ ↓ (Page Up / Page Down) pick a row; Enter applies it
//! and closes, Shift+Enter applies it and stays open; Tab jumps to the presets and back; with an
//! empty search 1–9 apply the shortcuts; Escape or a click outside closes. Right-click a row to
//! add it to Favorites, Blacklist it or make it a Shortcut. In the gallery: ↑ ↓ pick a snapshot
//! (Shift adds to the comparison, up to four), Z shows 100 %, Delete deletes, Ctrl/Cmd+C copies;
//! while an Effect Controls eyedropper is armed, a click on a snapshot samples its colour.

use std::collections::HashMap;
use std::sync::Arc;

use effectcraft_engine::console_effect::{self, Browse, Catalog, Hit, Kind, Results, SLOTS, Target, gallery};
use egui::{Align2, Color32, Key, Modifiers, Pos2, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use serde_json::{Value, json};

use crate::dock::PanelKind;
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::panels::DragPayload;
use crate::theme::Tokens;
use crate::{Dialog, EffectcraftApp, widgets};

const WIDTH: f32 = 420.0;
const BAR_H: f32 = 44.0;
const LINE_H: f32 = 24.0;
const VISIBLE_LINES: usize = 14;
const FOOT_H: f32 = 22.0;
const TOOL_W: f32 = 30.0;
/// Section dots: effects blue, presets red, shortcuts gold.
const EFFECT_DOT: Color32 = Color32::from_rgb(0x3d, 0x8b, 0xe8);
const PRESET_DOT: Color32 = Color32::from_rgb(0xe0, 0x4a, 0x4a);
const GOLD: Color32 = Color32::from_rgb(0xe8, 0xc0, 0x40);
/// Most snapshots compared side by side.
const COMPARE_MAX: usize = 4;
/// Decoded snapshots kept as textures.
const IMAGE_CACHE: usize = 48;

// ---------------------------------------------------------------- the console

/// One line of the results: a section heading or a row.
#[derive(Clone, Debug)]
enum Line {
    Head(&'static str, Color32),
    Row(Hit),
}

/// The lines shown for one query.
#[derive(Clone, Debug, Default)]
struct View {
    lines: Vec<Line>,
    /// Line index of every row (what the selection walks).
    rows: Vec<usize>,
    /// First row of the second section (Tab jumps there).
    second: Option<usize>,
}

impl View {
    fn row(&mut self, h: &Hit) {
        self.rows.push(self.lines.len());
        self.lines.push(Line::Row(h.clone()));
    }

    fn hit(&self, row: usize) -> Option<&Hit> {
        match self.lines.get(*self.rows.get(row)?)? {
            Line::Row(h) => Some(h),
            Line::Head(..) => None,
        }
    }

    fn results(r: &Results) -> View {
        let mut v = View::default();
        if !r.effects.is_empty() {
            v.lines.push(Line::Head("Effects", EFFECT_DOT));
            r.effects.iter().for_each(|h| v.row(h));
        }
        if !r.presets.is_empty() {
            v.second = (!v.rows.is_empty()).then_some(v.rows.len());
            v.lines.push(Line::Head("Presets", PRESET_DOT));
            r.presets.iter().for_each(|h| v.row(h));
        }
        v
    }

    fn browse(b: &Browse) -> View {
        let mut v = View::default();
        if b.shortcuts.iter().any(Option::is_some) {
            v.lines.push(Line::Head("Shortcuts", GOLD));
            b.shortcuts.iter().flatten().for_each(|h| v.row(h));
        }
        if !b.favorites.is_empty() {
            v.second = (!v.rows.is_empty()).then_some(v.rows.len());
            v.lines.push(Line::Head("Favorites", GOLD));
            b.favorites.iter().for_each(|h| v.row(h));
        }
        v
    }
}

/// The console while it is open.
#[derive(Clone, Debug, Default)]
pub struct ConsoleState {
    pub query: String,
    /// The selected row.
    pub sel: usize,
    /// The first line shown (the list scrolls to keep the selection in view).
    top: usize,
    /// Where it opened (screen points).
    pub anchor: Option<Pos2>,
    /// Give the search field the keyboard (on open).
    focus: bool,
    /// The pass it opened in (that press is not a click outside).
    opened_pass: u64,
    /// The catalog, for the settings revision it was built at.
    catalog: Option<(u64, Arc<Catalog>)>,
    /// The lines for (query, settings revision).
    view: Option<(String, u64, Arc<View>)>,
}

/// The console's state (what it searches for, the selected row).
pub fn state(app: &EffectcraftApp) -> &ConsoleState {
    &app.dialog_state.console_effect
}

/// Search the open console for `query` (as typing it would).
pub fn set_query(app: &mut EffectcraftApp, query: &str) {
    let st = &mut app.dialog_state.console_effect;
    (st.query, st.sel, st.top) = (query.to_string(), 0, 0);
}

/// Open the console at the pointer (Edit ▸ Console Effect…, Ctrl+Space), searching for `query`.
pub fn open(app: &mut EffectcraftApp, ctx: &egui::Context, query: &str) {
    open_at(app, ctx, query, ctx.input(|i| i.pointer.latest_pos()));
}

fn open_at(app: &mut EffectcraftApp, ctx: &egui::Context, query: &str, at: Option<Pos2>) {
    let screen = ctx.content_rect();
    let st = &mut app.dialog_state.console_effect;
    *st = ConsoleState {
        query: query.to_string(),
        anchor: Some(at.filter(|p| screen.contains(*p)).unwrap_or(pos2(screen.center().x, screen.top() + screen.height() * 0.3))),
        focus: true,
        opened_pass: ctx.cumulative_pass_nr(),
        ..Default::default()
    };
    app.dialog = Some(Dialog::ConsoleEffect);
}

/// The console's accent colour (Console Effect Settings ▸ General), else the theme's.
fn accent(app: &EffectcraftApp) -> Color32 {
    parse_hex(&app.session.console_effect.color).unwrap_or(app.tokens.accent)
}

fn parse_hex(c: &str) -> Option<Color32> {
    if !console_effect::valid_color(c) {
        return None;
    }
    let v = u32::from_str_radix(c.get(1..)?, 16).ok()?;
    let [_, r, g, b] = v.to_be_bytes();
    Some(Color32::from_rgb(r, g, b))
}

/// Everything the console can apply (scanned again when the settings change).
fn catalog(app: &mut EffectcraftApp) -> Arc<Catalog> {
    let rev = app.session.console_effect_revision;
    if let Some((r, c)) = &app.dialog_state.console_effect.catalog
        && *r == rev
    {
        return c.clone();
    }
    let c = Arc::new(Catalog::build(&app.session, &app.session.console_effect, console_effect::presets_dir().as_deref()));
    app.dialog_state.console_effect.catalog = Some((rev, c.clone()));
    c
}

fn view(app: &mut EffectcraftApp) -> Arc<View> {
    let rev = app.session.console_effect_revision;
    let q = app.dialog_state.console_effect.query.clone();
    if let Some((vq, vr, v)) = &app.dialog_state.console_effect.view
        && *vq == q
        && *vr == rev
    {
        return v.clone();
    }
    let cat = catalog(app);
    let st = &app.session.console_effect;
    let v = Arc::new(if q.trim().is_empty() {
        View::browse(&console_effect::browse(&cat, st))
    } else {
        View::results(&console_effect::search(&cat, st, &q, st.max_results as usize))
    });
    app.dialog_state.console_effect.view = Some((q, rev, v.clone()));
    v
}

/// What a frame of the console asked for (run once it is drawn).
enum Action {
    Apply(Target, bool),
    Slot(usize),
    Favorite(Target, bool),
    Blacklist(Target),
    Shortcut(Target, Option<usize>),
    ClearShortcut(usize),
    Edit(Target),
    Snapshot,
    Gallery,
    Copy,
    Save(&'static str),
    Settings,
}

/// Keys the console takes before the search field sees them.
#[derive(Default)]
struct Keys {
    down: bool,
    up: bool,
    page_down: bool,
    page_up: bool,
    tab: bool,
    back_tab: bool,
    enter: bool,
    enter_keep: bool,
}

fn take_keys(ctx: &egui::Context) -> Keys {
    ctx.input_mut(|i| {
        // (Shifted first: an unshifted pattern also matches the shifted key.)
        let enter_keep = i.consume_key(Modifiers::SHIFT, Key::Enter);
        let back_tab = i.consume_key(Modifiers::SHIFT, Key::Tab);
        Keys {
            down: i.consume_key(Modifiers::NONE, Key::ArrowDown),
            up: i.consume_key(Modifiers::NONE, Key::ArrowUp),
            page_down: i.consume_key(Modifiers::NONE, Key::PageDown),
            page_up: i.consume_key(Modifiers::NONE, Key::PageUp),
            tab: i.consume_key(Modifiers::NONE, Key::Tab),
            back_tab,
            enter: i.consume_key(Modifiers::NONE, Key::Enter),
            enter_keep,
        }
    })
}

/// A digit 1–9 typed into an empty search whose shortcut slot is set: taken (it applies the
/// shortcut instead of typing).
fn take_slot(app: &EffectcraftApp, ctx: &egui::Context) -> Option<usize> {
    if !app.dialog_state.console_effect.query.is_empty() {
        return None;
    }
    let st = &app.session.console_effect;
    let mut slot = None;
    ctx.input_mut(|i| {
        i.events.retain(|e| match e {
            egui::Event::Text(s) if slot.is_none() => match s.parse::<usize>() {
                Ok(n) if (1..=SLOTS).contains(&n) && st.shortcut(n).is_some() => {
                    slot = Some(n);
                    false
                }
                _ => true,
            },
            _ => true,
        });
    });
    slot
}

/// Draw the open console (`Dialog::ConsoleEffect`).
pub fn show(app: &mut EffectcraftApp, ctx: &egui::Context) {
    let t = app.tokens;
    let accent = accent(app);
    let keys = take_keys(ctx);
    let slot = take_slot(app, ctx);
    let v = view(app);
    let rows = v.rows.len();
    // Keyboard selection.
    {
        let st = &mut app.dialog_state.console_effect;
        let last = rows.saturating_sub(1);
        let page = VISIBLE_LINES.saturating_sub(2).max(1);
        if keys.down {
            st.sel = if st.sel >= last { 0 } else { st.sel + 1 };
        }
        if keys.up {
            st.sel = if st.sel == 0 { last } else { st.sel - 1 };
        }
        if keys.page_down {
            st.sel = (st.sel + page).min(last);
        }
        if keys.page_up {
            st.sel = st.sel.saturating_sub(page);
        }
        if keys.tab || keys.back_tab {
            st.sel = match v.second {
                Some(s) if keys.tab && st.sel < s => s,
                _ => 0,
            };
        }
        st.sel = st.sel.min(last);
    }
    let screen = ctx.content_rect();
    let shown = v.lines.len().clamp(1, VISIBLE_LINES);
    let height = BAR_H + shown as f32 * LINE_H + FOOT_H + 18.0;
    let anchor = app.dialog_state.console_effect.anchor.unwrap_or(screen.center());
    let pos = pos2(
        (anchor.x - 60.0).clamp(screen.left() + 8.0, (screen.right() - WIDTH - 20.0).max(screen.left() + 8.0)),
        (anchor.y - BAR_H / 2.0).clamp(screen.top() + 8.0, (screen.bottom() - height - 8.0).max(screen.top() + 8.0)),
    );
    let mut actions: Vec<Action> = vec![];
    if let Some(n) = slot {
        actions.push(Action::Slot(n));
    }
    let save_menu = egui::Id::new("console-effect-save");
    let area = egui::Area::new(egui::Id::new("console-effect")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).fill(t.panel_bg).stroke(Stroke::new(1.5, accent)).corner_radius(8.0).inner_margin(egui::Margin::same(6)).show(
            ui,
            |ui| {
                ui.set_width(WIDTH);
                search_bar(app, ui, accent, save_menu, &mut actions);
                results(app, ui, &v, accent, &mut actions);
                footer(app, ui, &v);
            },
        );
    });
    if keys.enter || keys.enter_keep {
        // (What was typed this frame counts.)
        let v = view(app);
        let sel = app.dialog_state.console_effect.sel.min(v.rows.len().saturating_sub(1));
        if let Some(h) = v.hit(sel) {
            actions.push(Action::Apply(h.target.clone(), keys.enter_keep));
        }
    }
    app.auto.add("consoleEffect.window", area.response.rect, "Console Effect");
    // A press outside closes it (not the one that opened it, nor one in its menus).
    let st = &app.dialog_state.console_effect;
    if ctx.cumulative_pass_nr() > st.opened_pass
        && widgets::pressed_outside(ctx, &area.response)
        && !egui::Popup::is_any_open(ctx)
        && !widgets::any_menu_open(ctx)
    {
        app.dialog = None;
    }
    for a in actions {
        run_action(app, ctx, a);
    }
}

fn search_bar(app: &mut EffectcraftApp, ui: &mut egui::Ui, accent: Color32, save_menu: egui::Id, actions: &mut Vec<Action>) {
    let t = app.tokens;
    let (bar, _) = ui.allocate_exact_size(vec2(WIDTH, BAR_H), Sense::hover());
    let p = ui.painter().clone();
    icons::paint(&p, Rect::from_center_size(pos2(bar.min.x + 16.0, bar.center().y), vec2(18.0, 18.0)), Icon::Search, t.text);
    let tools = [
        (Icon::Snapshot, "snapshot", "Snapshot to the gallery"),
        (Icon::Image, "gallery", "Gallery"),
        (Icon::Download, "save", "Copy or save the frame"),
        (Icon::Gear, "settings", "Console Effect Settings"),
    ];
    let field = Rect::from_min_max(pos2(bar.min.x + 32.0, bar.min.y + 6.0), pos2(bar.max.x - tools.len() as f32 * TOOL_W - 10.0, bar.max.y - 6.0));
    let st = &mut app.dialog_state.console_effect;
    let mut q = st.query.clone();
    let resp = ui.put(
        field,
        egui::TextEdit::singleline(&mut q)
            .id(egui::Id::new("console-effect-field"))
            .hint_text(tr("Search effects and presets"))
            .font(Tokens::ui(16.0))
            .frame(egui::Frame::NONE)
            .desired_width(field.width())
            .vertical_align(egui::Align::Center),
    );
    // The field keeps the keyboard while the console is open (not while one of its menus is).
    if st.focus || (!resp.has_focus() && !egui::Popup::is_any_open(ui.ctx()) && !widgets::popup_is_open(ui, save_menu)) {
        resp.request_focus();
        st.focus = false;
    }
    if q != st.query {
        st.query = q;
        st.sel = 0;
        st.top = 0;
    }
    app.auto.add("consoleEffect.search", field, "Search effects and presets");
    let mut x = bar.max.x - tools.len() as f32 * TOOL_W;
    for (icon, id, tip) in tools {
        let r = Rect::from_min_size(pos2(x, bar.center().y - 13.0), vec2(TOOL_W - 4.0, 26.0));
        x += TOOL_W;
        let b = widgets::icon_button(ui, r, icon, false, &t, egui::Id::new(("console-effect-tool", id))).on_hover_text(tr(tip));
        app.auto.add(&format!("consoleEffect.{id}"), r, tip);
        if b.clicked() {
            match id {
                "snapshot" => actions.push(Action::Snapshot),
                "gallery" => actions.push(Action::Gallery),
                "save" => widgets::open_popup(ui, save_menu),
                _ => actions.push(Action::Settings),
            }
        }
        if id == "save" {
            let opts = [tr("Copy to Clipboard").to_string(), tr("Save as PNG...").to_string(), tr("Save as JPG...").to_string()];
            match widgets::popup_menu(ui, save_menu, r.left_bottom() - vec2(140.0, 0.0), &opts, None) {
                Some(0) => actions.push(Action::Copy),
                Some(1) => actions.push(Action::Save("png")),
                Some(2) => actions.push(Action::Save("jpg")),
                _ => {}
            }
        }
    }
    p.line_segment([pos2(bar.min.x, bar.max.y - 0.5), pos2(bar.max.x, bar.max.y - 0.5)], Stroke::new(1.0, accent.gamma_multiply(0.6)));
}

/// The name with the typed text highlighted.
fn name_text(p: &egui::Painter, pos: Pos2, h: &Hit, max_w: f32, accent: Color32, t: &Tokens) -> f32 {
    let font = Tokens::ui(13.0);
    let shown = h.label.as_deref().unwrap_or(&h.name);
    if h.label.is_none()
        && let Some(r) = h.range.clone().filter(|r| h.name.get(r.clone()).is_some())
    {
        let pre = p.layout_no_wrap(h.name.get(..r.start).unwrap_or_default().to_string(), font.clone(), t.text).size().x;
        let hit = p.layout_no_wrap(h.name.get(r).unwrap_or_default().to_string(), font.clone(), t.text).size().x;
        if pre + hit <= max_w {
            p.rect_filled(Rect::from_min_size(pos2(pos.x + pre - 1.0, pos.y - 8.5), vec2(hit + 2.0, 17.0)), 2.0, accent.gamma_multiply(0.4));
        }
    }
    widgets::text_fit(p, pos, Align2::LEFT_CENTER, shown, font, max_w, if h.missing { t.text_faint } else { t.text }).width()
}

fn results(app: &mut EffectcraftApp, ui: &mut egui::Ui, v: &View, accent: Color32, actions: &mut Vec<Action>) {
    let t = app.tokens;
    let lines = v.lines.len();
    let shown = lines.clamp(1, VISIBLE_LINES);
    let (list, resp) = ui.allocate_exact_size(vec2(WIDTH, shown as f32 * LINE_H), Sense::hover());
    let p = ui.painter().with_clip_rect(list);
    let ctx = ui.ctx().clone();
    // Keep the selection (and its section's heading) in view; the wheel scrolls.
    {
        let st = &mut app.dialog_state.console_effect;
        if resp.hovered() {
            let d = ui.input(|i| i.smooth_scroll_delta.y);
            // (The wheel scrolls the list; the selection stays where it is.)
            if d.abs() > 0.5 {
                let step = (d.abs() / LINE_H).ceil().max(1.0) as usize;
                st.top = if d > 0.0 { st.top.saturating_sub(step) } else { st.top + step };
            }
        }
        let wheel = resp.hovered() && ui.input(|i| i.smooth_scroll_delta.y.abs() > 0.5);
        if let Some(&line) = v.rows.get(st.sel)
            && !wheel
        {
            let head = if line > 0 && matches!(v.lines.get(line - 1), Some(Line::Head(..))) { line - 1 } else { line };
            if head < st.top {
                st.top = head;
            } else if line >= st.top + shown {
                st.top = line + 1 - shown;
            }
        }
        st.top = st.top.min(lines.saturating_sub(shown));
    }
    if lines == 0 {
        let msg = if app.dialog_state.console_effect.query.trim().is_empty() {
            tr("Type to search effects and presets. Right-click a result for Favorites and Shortcuts.")
        } else {
            tr("No matching effects or presets")
        };
        widgets::text_fit(&p, list.center(), Align2::CENTER_CENTER, msg, Tokens::ui(12.0), list.width() - 16.0, t.text_faint);
        return;
    }
    let moved = ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let top = app.dialog_state.console_effect.top;
    let sel_line = v.rows.get(app.dialog_state.console_effect.sel).copied();
    for (i, line) in v.lines.iter().enumerate().skip(top).take(shown) {
        let r = Rect::from_min_size(pos2(list.min.x, list.min.y + (i - top) as f32 * LINE_H), vec2(WIDTH, LINE_H));
        match line {
            Line::Head(label, dot) => {
                p.circle_filled(pos2(r.min.x + 12.0, r.center().y), 3.5, *dot);
                p.text(pos2(r.min.x + 22.0, r.center().y), Align2::LEFT_CENTER, tr(label), Tokens::semibold(11.0), t.text_dim);
                p.line_segment([pos2(r.min.x + 4.0, r.max.y - 0.5), pos2(r.max.x - 4.0, r.max.y - 0.5)], Stroke::new(1.0, t.separator));
            }
            Line::Row(h) => {
                let row = v.rows.iter().position(|l| *l == i).unwrap_or(0);
                let rr = ui.interact(r, egui::Id::new(("console-effect-row", i)), Sense::click());
                if rr.hovered() && moved {
                    app.dialog_state.console_effect.sel = row;
                }
                let selected = sel_line == Some(i);
                if selected {
                    p.rect_filled(r.shrink2(vec2(2.0, 1.0)), 4.0, accent);
                } else if rr.hovered() {
                    p.rect_filled(r.shrink2(vec2(2.0, 1.0)), 4.0, t.hover);
                }
                let fg = if selected { Color32::WHITE } else { t.text_dim };
                let icon = if h.target.kind() == Kind::Effect { Icon::Fx } else { Icon::Keyframe };
                icons::paint(&p, Rect::from_center_size(pos2(r.min.x + 16.0, r.center().y), vec2(12.0, 12.0)), icon, fg);
                // Right side: shortcut number, favorite star, override mark, category.
                let mut right = r.max.x - 8.0;
                if let Some(n) = h.shortcut {
                    let b = Rect::from_center_size(pos2(right - 8.0, r.center().y), vec2(16.0, 16.0));
                    p.rect_filled(b, 3.0, if selected { Color32::from_white_alpha(60) } else { t.field_bg });
                    p.text(b.center(), Align2::CENTER_CENTER, n.to_string(), Tokens::semibold(11.0), if selected { Color32::WHITE } else { GOLD });
                    right -= 22.0;
                }
                if h.favorite.is_some() {
                    p.text(pos2(right, r.center().y), Align2::RIGHT_CENTER, "★", Tokens::ui(12.0), if selected { Color32::WHITE } else { GOLD });
                    right -= 16.0;
                }
                if h.preset_override.is_some() {
                    p.text(pos2(right, r.center().y), Align2::RIGHT_CENTER, tr("override"), Tokens::ui(10.5), fg);
                    right -= 50.0;
                }
                // A shortcut's label shows what it applies; an alias match shows the alias.
                let side = match (&h.label, &h.alias) {
                    (Some(l), _) if *l != h.name => h.name.clone(),
                    (None, Some(a)) => format!("“{a}”"),
                    _ => h.category.clone(),
                };
                let name_x = r.min.x + 30.0;
                let room = (right - name_x).max(40.0);
                let name_w = {
                    let shown_t = if selected { Tokens { text: Color32::WHITE, text_faint: Color32::from_white_alpha(160), ..t } } else { t };
                    name_text(&p, pos2(name_x, r.center().y), h, room * 0.62, if selected { Color32::from_white_alpha(70) } else { accent }, &shown_t)
                };
                let side_room = right - (name_x + name_w + 12.0);
                if side_room > 24.0 {
                    widgets::text_fit(
                        &p,
                        pos2(right, r.center().y),
                        Align2::RIGHT_CENTER,
                        &side,
                        Tokens::ui(11.0),
                        side_room,
                        if selected { Color32::from_white_alpha(200) } else { t.text_faint },
                    );
                }
                app.auto.add(&format!("consoleEffect.row.{row}"), r, &h.name);
                if rr.clicked() {
                    actions.push(Action::Apply(h.target.clone(), ctx.input(|i| i.modifiers.shift)));
                }
                rr.context_menu(|ui| row_menu(app, ui, h, actions));
            }
        }
    }
    // A thin scroll bar while the list overflows.
    if lines > shown {
        let track = Rect::from_min_max(pos2(list.max.x - 4.0, list.min.y + 2.0), pos2(list.max.x - 1.0, list.max.y - 2.0));
        let frac = shown as f32 / lines as f32;
        let at = app.dialog_state.console_effect.top as f32 / lines as f32;
        let thumb = Rect::from_min_size(pos2(track.min.x, track.min.y + at * track.height()), vec2(track.width(), (frac * track.height()).max(12.0)));
        p.rect_filled(thumb, 2.0, t.text_faint);
    }
}

/// The right-click menu of a row.
fn row_menu(app: &mut EffectcraftApp, ui: &mut egui::Ui, h: &Hit, actions: &mut Vec<Action>) {
    ui.set_min_width(200.0);
    let st = &app.session.console_effect;
    let fav = st.favorite_index(&h.target).is_some();
    let mut item = |ui: &mut egui::Ui, label: &str, a: Action| {
        if ui.button(tr(label)).clicked() {
            actions.push(a);
            ui.close();
        }
    };
    item(ui, "Apply", Action::Apply(h.target.clone(), false));
    item(ui, "Apply and Keep Open", Action::Apply(h.target.clone(), true));
    ui.separator();
    if fav {
        item(ui, "Remove from Favorites", Action::Favorite(h.target.clone(), false));
    } else {
        item(ui, "Add to Favorites", Action::Favorite(h.target.clone(), true));
    }
    item(ui, "Alias and Override...", Action::Edit(h.target.clone()));
    item(ui, "Blacklist", Action::Blacklist(h.target.clone()));
    ui.separator();
    match st.shortcut_slot(&h.target) {
        Some(n) => item(ui, "Remove Shortcut", Action::ClearShortcut(n)),
        None => {
            if st.free_slot().is_some() {
                item(ui, "Create Shortcut", Action::Shortcut(h.target.clone(), None));
            }
        }
    }
    ui.menu_button(tr("Replace Shortcut"), |ui| {
        for n in 1..=SLOTS {
            let now = st.shortcut(n).map_or_else(|| tr("Empty").to_string(), |sc| sc.shown());
            if ui.button(format!("{n}   {now}")).clicked() {
                actions.push(Action::Shortcut(h.target.clone(), Some(n)));
                ui.close();
            }
        }
    });
}

fn footer(app: &mut EffectcraftApp, ui: &mut egui::Ui, v: &View) {
    let t = app.tokens;
    let (r, _) = ui.allocate_exact_size(vec2(WIDTH, FOOT_H), Sense::hover());
    let hint = if app.dialog_state.console_effect.query.is_empty() && v.lines.iter().any(|l| matches!(l, Line::Head("Shortcuts", _))) {
        tr("1–9 shortcuts · ↑↓ select · Enter apply · Esc close")
    } else {
        tr("↑↓ select · Enter apply · Shift+Enter keep open · Tab presets · Esc close")
    };
    widgets::text_fit(ui.painter(), pos2(r.min.x + 8.0, r.center().y + 2.0), Align2::LEFT_CENTER, hint, Tokens::ui(10.5), r.width() - 16.0, t.text_faint);
}

fn exec(app: &mut EffectcraftApp, id: &str, p: Value) -> Option<Value> {
    match app.session.execute(id, p) {
        Ok(v) => Some(v),
        Err(e) => {
            app.ui.status = e.to_string();
            None
        }
    }
}

/// `t`'s command parameters merged into `extra`.
fn with_target(t: &Target, extra: Value) -> Value {
    let mut m = extra.as_object().cloned().unwrap_or_default();
    if let Some(o) = t.to_params().as_object() {
        m.extend(o.clone());
    }
    Value::Object(m)
}

/// Apply `params` (a target or a shortcut slot) to the selected layers; the console closes
/// unless `keep`.
fn apply(app: &mut EffectcraftApp, params: Value, keep: bool) {
    if app.session.state.selected_layers.is_empty() {
        app.ui.status = tr("Select a layer to apply to").into();
        return;
    }
    let Some(r) = exec(app, "consoleEffect.apply", params) else { return };
    if r["applied"].get("effect").is_some() {
        // As Effects & Presets does: Effect Controls shows the new effect (focus stays).
        let focused = app.ui.focused;
        app.show_panel(PanelKind::EffectControls);
        app.ui.focused = focused;
        crate::panels::effect_controls::choose_files_after_apply(app, &r["result"]);
    }
    if keep {
        app.dialog_state.console_effect.focus = true;
    } else if app.dialog == Some(Dialog::ConsoleEffect) {
        app.dialog = None;
    }
}

fn run_action(app: &mut EffectcraftApp, ctx: &egui::Context, a: Action) {
    match a {
        Action::Apply(target, keep) => apply(app, target.to_params(), keep),
        Action::Slot(n) => apply(app, json!({"slot": n}), false),
        Action::Favorite(target, on) => {
            exec(app, "consoleEffect.favorite", with_target(&target, json!({"value": on})));
        }
        Action::Blacklist(target) => {
            exec(app, "consoleEffect.blacklist", with_target(&target, json!({"value": true})));
        }
        Action::Shortcut(target, slot) => {
            let p = match slot {
                Some(n) => with_target(&target, json!({"slot": n})),
                None => target.to_params(),
            };
            if let Some(r) = exec(app, "consoleEffect.shortcut", p) {
                app.toast = Some((format!("{} {}", tr("Shortcut"), r["slot"]), ctx.input(|i| i.time)));
            }
        }
        Action::ClearShortcut(n) => {
            exec(app, "consoleEffect.shortcut", json!({"slot": n, "clear": true}));
        }
        Action::Edit(target) => open_settings(app, Page::Favorites, Some(target)),
        Action::Snapshot => snapshot(app),
        Action::Gallery => {
            app.dialog = None;
            app.console_gallery.open = true;
        }
        Action::Copy => {
            if let Err(e) = copy_frame(app, ctx, None) {
                app.ui.status = e;
            }
        }
        Action::Save(format) => {
            app.dialog = None;
            save_frame(app, ctx, format);
        }
        Action::Settings => open_settings(app, Page::Favorites, None),
    }
}

// ---------------------------------------------------------------- frames out of the viewer

/// The render scale the viewer shows the active comp at (its resolution).
fn viewer_scale(app: &EffectcraftApp, comp: u64) -> f64 {
    app.viewer_shown
        .as_ref()
        .filter(|(_, k)| k.comp == comp && k.scale > 0)
        .map(|(_, k)| f64::from(k.scale) / 1000.0)
        .unwrap_or_else(|| app.ui.viewer.res.scale(app.ui.viewer.zoom.unwrap_or(1.0), 1.0))
}

/// Snapshot the active comp at the viewer's resolution (with transparency while the viewer
/// shows the transparency grid) into the gallery.
pub fn snapshot(app: &mut EffectcraftApp) {
    let Some(cid) = app.session.active_comp_id() else {
        app.ui.status = tr("Open a composition first").into();
        return;
    };
    let p = json!({"comp": cid.0, "scale": viewer_scale(app, cid.0), "alpha": app.ui.viewer.transparency_grid});
    if let Some(r) = exec(app, "consoleEffect.snapshot", p) {
        app.console_gallery.dirty = true;
        if let Some(name) = r["name"].as_str() {
            app.console_gallery.selected = vec![name.to_string()];
        }
    }
}

/// Copy the active comp's frame to the clipboard (`alpha`: keep transparency; default: as the
/// viewer's transparency grid shows it).
pub fn copy_frame(app: &mut EffectcraftApp, ctx: &egui::Context, alpha: Option<bool>) -> Result<Value, String> {
    let cid = app.session.active_comp_id().ok_or("Open a composition first")?;
    let alpha = alpha.unwrap_or(app.ui.viewer.transparency_grid);
    let (w, h, rgba) = gallery::frame_rgba(&app.session, cid, viewer_scale(app, cid.0), alpha).map_err(|e| e.to_string())?;
    ctx.copy_image(egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba));
    app.toast = Some((tr("Frame copied to the clipboard").into(), ctx.input(|i| i.time)));
    Ok(json!({"width": w, "height": h, "alpha": alpha}))
}

/// Save the active comp's frame as `format` (`png` / `jpg`) where the save dialog says.
pub fn save_frame(app: &mut EffectcraftApp, ctx: &egui::Context, format: &str) {
    let Some(cid) = app.session.active_comp_id() else {
        app.ui.status = tr("Open a composition first").into();
        return;
    };
    let p = json!({"format": format, "comp": cid.0, "scale": viewer_scale(app, cid.0), "alpha": app.ui.viewer.transparency_grid && format == "png"});
    if let Err(e) = crate::menus::invoke(app, ctx, "consoleEffect.saveFrame", p) {
        app.ui.status = e;
    }
}

// ---------------------------------------------------------------- the dockable panel

/// Window ▸ Console Effect: the console's other functions at hand (open it, snapshot, copy, save,
/// the gallery, settings) and the nine shortcuts.
pub fn panel(app: &mut EffectcraftApp, ui: &mut egui::Ui, rect: Rect) {
    let t = app.tokens;
    let accent = accent(app);
    let ctx = ui.ctx().clone();
    let p = ui.painter().clone();
    let mut y = rect.min.y + 8.0;
    // The search field look-alike that opens the console here.
    let open_r = Rect::from_min_size(pos2(rect.min.x + 8.0, y), vec2((rect.width() - 16.0).max(40.0), 28.0));
    let resp = ui.interact(open_r, egui::Id::new("console-panel-open"), Sense::click());
    p.rect_filled(open_r, 14.0, if resp.hovered() { t.hover } else { t.field_bg });
    p.rect_stroke(open_r, 14.0, Stroke::new(1.0, accent), StrokeKind::Inside);
    icons::paint(&p, Rect::from_center_size(pos2(open_r.min.x + 16.0, open_r.center().y), vec2(13.0, 13.0)), Icon::Search, t.text_dim);
    let key = console_shortcut(app);
    let label = if key.is_empty() { tr("Open Console Effect").to_string() } else { format!("{}  ({key})", tr("Open Console Effect")) };
    widgets::text_fit(&p, pos2(open_r.min.x + 28.0, open_r.center().y), Align2::LEFT_CENTER, &label, Tokens::ui(12.0), open_r.width() - 36.0, t.text_dim);
    app.auto.add("consoleEffectPanel.open", open_r, "Open Console Effect");
    if resp.clicked() {
        open_at(app, &ctx, "", Some(open_r.left_bottom() + vec2(60.0, 10.0)));
    }
    y += 36.0;
    // The tools.
    let tools: [(Icon, &str, &str); 6] = [
        (Icon::Snapshot, "snapshot", "Snapshot to the gallery"),
        (Icon::Image, "gallery", "Gallery"),
        (Icon::Copy, "copy", "Copy to Clipboard"),
        (Icon::Download, "savePng", "Save as PNG..."),
        (Icon::Download, "saveJpg", "Save as JPG..."),
        (Icon::Gear, "settings", "Console Effect Settings"),
    ];
    let w = ((rect.width() - 16.0) / tools.len() as f32).clamp(22.0, 40.0);
    for (i, (icon, id, tip)) in tools.into_iter().enumerate() {
        let r = Rect::from_min_size(pos2(rect.min.x + 8.0 + i as f32 * w, y), vec2(w - 4.0, 26.0));
        let b = widgets::icon_button(ui, r, icon, false, &t, egui::Id::new(("console-panel-tool", id))).on_hover_text(tr(tip));
        if id == "saveJpg" {
            p.text(r.right_bottom() - vec2(2.0, 1.0), Align2::RIGHT_BOTTOM, "J", Tokens::semibold(8.5), t.text_dim);
        }
        app.auto.add(&format!("consoleEffectPanel.{id}"), r, tip);
        if b.clicked() {
            match id {
                "snapshot" => snapshot(app),
                "gallery" => app.console_gallery.open = true,
                "copy" => {
                    if let Err(e) = copy_frame(app, &ctx, None) {
                        app.ui.status = e;
                    }
                }
                "savePng" => save_frame(app, &ctx, "png"),
                "saveJpg" => save_frame(app, &ctx, "jpg"),
                _ => open_settings(app, Page::Favorites, None),
            }
        }
    }
    y += 34.0;
    // The shortcuts: click one to apply it.
    p.text(pos2(rect.min.x + 10.0, y + 8.0), Align2::LEFT_CENTER, tr("Shortcuts"), Tokens::semibold(11.0), t.text_dim);
    y += 18.0;
    let list = Rect::from_min_max(pos2(rect.min.x, y), rect.max);
    let lp = p.with_clip_rect(list);
    let scroll = widgets::PanelScroll::begin(ui, egui::Id::new("console-panel-scroll"), list);
    let mut ry = list.min.y - scroll.offset;
    let mut apply_slot = None;
    for n in 1..=SLOTS {
        let r = Rect::from_min_size(pos2(list.min.x + 4.0, ry), vec2(list.width() - 8.0, 22.0));
        ry += 22.0;
        if r.max.y < list.min.y || r.min.y > list.max.y {
            continue;
        }
        let sc = app.session.console_effect.shortcut(n).cloned();
        let rr = ui.interact(r.intersect(list), egui::Id::new(("console-panel-slot", n)), Sense::click());
        if rr.hovered() && sc.is_some() {
            lp.rect_filled(r, 3.0, t.hover);
        }
        lp.text(pos2(r.min.x + 10.0, r.center().y), Align2::CENTER_CENTER, n.to_string(), Tokens::semibold(11.0), GOLD);
        let (text, col) = match &sc {
            Some(sc) => (sc.shown(), t.text),
            None => (tr("Empty").to_string(), t.text_faint),
        };
        widgets::text_fit(&lp, pos2(r.min.x + 24.0, r.center().y), Align2::LEFT_CENTER, &text, Tokens::ui(12.0), r.width() - 28.0, col);
        app.auto.add(&format!("consoleEffectPanel.slot.{n}"), r, &text);
        if rr.clicked() && sc.is_some() {
            apply_slot = Some(n);
        }
    }
    let content = ry + scroll.offset - list.min.y;
    scroll.end(ui, &mut app.auto, "consoleEffectPanel.scroll", content, &t);
    if let Some(n) = apply_slot {
        if app.session.state.selected_layers.is_empty() {
            app.ui.status = tr("Select a layer to apply to").into();
        } else {
            exec(app, "consoleEffect.apply", json!({"slot": n}));
        }
    }
}

/// The console's keyboard shortcut as the menus show it (empty when it has none).
fn console_shortcut(app: &EffectcraftApp) -> String {
    app.session
        .shortcuts()
        .bindings()
        .into_iter()
        .find(|(_, b)| b.command == "app.consoleEffect")
        .map(|(k, _)| crate::menus::shortcut_text(k))
        .unwrap_or_default()
}

// ---------------------------------------------------------------- settings

/// The settings dialog's pages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Favorites,
    Shortcuts,
    Blacklist,
    General,
}

impl Page {
    const ALL: [(Page, &'static str); 4] =
        [(Page::Favorites, "Favorites"), (Page::Shortcuts, "Shortcuts"), (Page::Blacklist, "Blacklist"), (Page::General, "General")];

    fn parse(s: &str) -> Page {
        Page::ALL.iter().find(|(_, n)| n.eq_ignore_ascii_case(s)).map_or(Page::Favorites, |(p, _)| *p)
    }
}

/// Console Effect Settings while it is open.
#[derive(Clone, Debug, Default)]
pub struct SettingsState {
    pub page: Page,
    /// Alias fields being typed in, by favorite.
    aliases: HashMap<Target, String>,
    /// The search that adds a favorite or fills a shortcut slot.
    find: String,
    /// The shortcut slot the search fills (`None`: it adds favorites).
    slot: Option<usize>,
    /// Label fields being typed in, by slot.
    labels: HashMap<usize, String>,
    /// The gallery folder being typed.
    folder: Option<String>,
    /// The favorite the dialog opened for (highlighted).
    focus: Option<Target>,
    /// Give that favorite's alias field the keyboard (once).
    focus_alias: bool,
}

/// Open Console Effect Settings on `page` (for favorite `focus`: the one to edit).
pub fn open_settings(app: &mut EffectcraftApp, page: Page, focus: Option<Target>) {
    if let Some(t) = &focus
        && app.session.console_effect.favorite_index(t).is_none()
    {
        exec(app, "consoleEffect.favorite", with_target(t, json!({"value": true})));
    }
    app.dialog_state.console_settings = SettingsState { page, focus_alias: focus.is_some(), focus, ..Default::default() };
    app.dialog = Some(Dialog::ConsoleEffectSettings);
}

/// Open the settings on the page `params` name (`consoleEffect.openSettings {page?}`).
pub fn open_settings_page(app: &mut EffectcraftApp, params: &Value) {
    open_settings(app, Page::parse(params.get("page").and_then(Value::as_str).unwrap_or("")), None);
}

/// Name and kind of a target for the settings rows.
fn describe(cat: &Catalog, t: &Target) -> (String, String, bool) {
    match cat.get(t) {
        Some(e) => (e.name.clone(), e.category.clone(), false),
        None => (t.name(), String::new(), true),
    }
}

/// The Console Effect Settings dialog (`Dialog::ConsoleEffectSettings`).
pub fn settings_dialog(app: &mut EffectcraftApp, ctx: &egui::Context, t: &Tokens) {
    let mut st = std::mem::take(&mut app.dialog_state.console_settings);
    let cat = catalog(app);
    let mut run: Vec<(&'static str, Value)> = vec![];
    let mut invoke: Vec<(&'static str, Value)> = vec![];
    let mut close = false;
    let mut keys_editor = false;
    super::dialogs::modal(ctx, "Console Effect Settings", vec2(860.0, 540.0), t, |ui| {
        ui.horizontal(|ui| {
            for (p, name) in Page::ALL {
                let b = ui.selectable_label(st.page == p, tr(name));
                app.auto.add(&format!("consoleEffectSettings.page.{}", name.to_lowercase()), b.rect, name);
                if b.clicked() {
                    st.page = p;
                    st.slot = None;
                }
            }
        });
        ui.separator();
        let settings = app.session.console_effect.clone();
        match st.page {
            Page::Favorites => favorites_page(app, ui, &cat, &settings, &mut st, &mut run, &mut invoke, t),
            Page::Shortcuts => shortcuts_page(app, ui, &cat, &settings, &mut st, &mut run, t),
            Page::Blacklist => blacklist_page(app, ui, &cat, &settings, &mut run, t),
            Page::General => general_page(app, ui, &settings, &mut st, &mut run, &mut invoke, &mut keys_editor, t),
        }
        ui.add_space(10.0);
        ui.separator();
        ui.horizontal(|ui| {
            let b = ui.button(tr("Close"));
            app.auto.add("consoleEffectSettings.close", b.rect, "Close");
            if b.clicked() {
                close = true;
            }
            let b = ui.button(tr("Open Console Effect"));
            app.auto.add("consoleEffectSettings.openConsole", b.rect, "Open Console Effect");
            if b.clicked() {
                close = true;
                run.push(("app.consoleEffect", json!({})));
            }
        });
    });
    app.dialog_state.console_settings = st;
    for (id, p) in run {
        if id == "app.consoleEffect" {
            open(app, ctx, "");
            continue;
        }
        exec(app, id, p);
    }
    for (id, p) in invoke {
        if let Err(e) = crate::menus::invoke(app, ctx, id, p) {
            app.ui.status = e;
        }
    }
    if keys_editor {
        crate::panels::shortcut_editor::open(app);
        app.dialog_state.shortcuts.query = "Console Effect".into();
        app.dialog_state.shortcuts.selected = Some("app.consoleEffect".into());
    } else if close && app.dialog == Some(Dialog::ConsoleEffectSettings) {
        app.dialog = None;
    }
}

/// A search over the catalog that picks a target (adds a favorite or fills a shortcut slot).
fn finder(
    app: &mut EffectcraftApp,
    ui: &mut egui::Ui,
    cat: &Catalog,
    settings: &console_effect::Settings,
    st: &mut SettingsState,
    hint: &str,
) -> Option<Target> {
    let r = ui.add_sized(vec2(320.0, 20.0), egui::TextEdit::singleline(&mut st.find).hint_text(tr(hint)));
    app.auto.add("consoleEffectSettings.find", r.rect, hint);
    if st.find.trim().is_empty() {
        return None;
    }
    let found = console_effect::search(cat, settings, &st.find, 8);
    let mut picked = None;
    for (i, h) in found.rows().take(8).enumerate() {
        let b = ui.selectable_label(false, format!("{}   ·   {}", h.name, h.category));
        app.auto.add(&format!("consoleEffectSettings.found.{i}"), b.rect, &h.name);
        if b.clicked() {
            picked = Some(h.target.clone());
        }
    }
    if picked.is_some() {
        st.find.clear();
    }
    picked
}

#[allow(clippy::too_many_arguments)]
fn favorites_page(
    app: &mut EffectcraftApp,
    ui: &mut egui::Ui,
    cat: &Catalog,
    settings: &console_effect::Settings,
    st: &mut SettingsState,
    run: &mut Vec<(&'static str, Value)>,
    invoke: &mut Vec<(&'static str, Value)>,
    t: &Tokens,
) {
    ui.label(
        egui::RichText::new(tr("Favorites come first in the results, in this order. An alias is another name a favorite answers to; an override is an animation preset applied in its place."))
            .color(t.text_dim),
    );
    ui.add_space(6.0);
    let presets: Vec<(String, String)> =
        cat.entries.iter().filter_map(|e| if let Target::Preset(p) = &e.target { Some((p.clone(), e.name.clone())) } else { None }).collect();
    let can_save = !app.session.state.selected_props.is_empty();
    let n = settings.favorites.len();
    egui::ScrollArea::vertical().max_height(330.0).auto_shrink([false, true]).show(ui, |ui| {
        egui::Grid::new("fx-favorites").num_columns(5).spacing(vec2(10.0, 6.0)).striped(true).show(ui, |ui| {
            for (i, f) in settings.favorites.iter().enumerate() {
                let (name, category, missing) = describe(cat, &f.target);
                ui.horizontal(|ui| {
                    let up = ui.add_enabled(i > 0, egui::Button::new("▲").small());
                    app.auto.add(&format!("consoleEffectSettings.favorite.{i}.up"), up.rect, "Move Up");
                    if up.clicked() {
                        run.push(("consoleEffect.favorite", with_target(&f.target, json!({"value": true, "index": i - 1}))));
                    }
                    let down = ui.add_enabled(i + 1 < n, egui::Button::new("▼").small());
                    app.auto.add(&format!("consoleEffectSettings.favorite.{i}.down"), down.rect, "Move Down");
                    if down.clicked() {
                        run.push(("consoleEffect.favorite", with_target(&f.target, json!({"value": true, "index": i + 1}))));
                    }
                });
                let focused = st.focus.as_ref() == Some(&f.target);
                let text = egui::RichText::new(format!("★ {name}")).color(if missing {
                    t.text_faint
                } else if focused {
                    t.accent
                } else {
                    t.text
                });
                ui.label(text).on_hover_text(category);
                // Alias: typed, then saved when the field is left.
                let draft = st.aliases.entry(f.target.clone()).or_insert_with(|| f.alias.clone());
                let r = ui.add_sized(vec2(180.0, 20.0), egui::TextEdit::singleline(draft).hint_text(tr("Alias")));
                app.auto.add(&format!("consoleEffectSettings.favorite.{i}.alias"), r.rect, "Alias");
                if focused && st.focus_alias {
                    r.request_focus();
                    r.scroll_to_me(Some(egui::Align::Center));
                    st.focus_alias = false;
                }
                if r.lost_focus() && *draft != f.alias {
                    run.push(("consoleEffect.alias", with_target(&f.target, json!({"alias": draft.clone()}))));
                }
                // Override: none, a preset file, or one picked or saved now (presets have none).
                if matches!(f.target, Target::Preset(_)) {
                    ui.label(egui::RichText::new("—").color(t.text_faint));
                    let x = ui.button(tr("Remove"));
                    app.auto.add(&format!("consoleEffectSettings.favorite.{i}.remove"), x.rect, "Remove from Favorites");
                    if x.clicked() {
                        run.push(("consoleEffect.favorite", with_target(&f.target, json!({"value": false}))));
                    }
                    ui.end_row();
                    continue;
                }
                let current = f.preset_override.as_ref().map_or_else(|| tr("No override").to_string(), |p| console_effect::preset_name(p));
                let cb = egui::ComboBox::from_id_salt(("fx-override", i)).selected_text(current).width(190.0).show_ui(ui, |ui| {
                    if ui.selectable_label(f.preset_override.is_none(), tr("No override")).clicked() {
                        run.push(("consoleEffect.override", with_target(&f.target, json!({"overridePreset": null}))));
                    }
                    for (path, pname) in &presets {
                        if ui.selectable_label(f.preset_override.as_deref() == Some(path.as_str()), pname).clicked() {
                            run.push(("consoleEffect.override", with_target(&f.target, json!({"overridePreset": path}))));
                        }
                    }
                    ui.separator();
                    if ui.button(tr("Choose Preset File...")).clicked() {
                        invoke.push(("consoleEffect.override", f.target.to_params()));
                    }
                    let save = ui.add_enabled(can_save, egui::Button::new(tr("Save Selected Effects as Override")));
                    if save.clicked()
                        && let Some(dir) = console_effect::presets_dir()
                    {
                        let path = dir.join(format!("{name} Default.{}", console_effect::PRESET_EXT)).to_string_lossy().to_string();
                        run.push(("anim.savePreset", json!({"path": path, "name": format!("{name} Default")})));
                        run.push(("consoleEffect.override", with_target(&f.target, json!({"overridePreset": path}))));
                    }
                });
                app.auto.add(&format!("consoleEffectSettings.favorite.{i}.override"), cb.response.rect, "Override");
                let x = ui.button(tr("Remove"));
                app.auto.add(&format!("consoleEffectSettings.favorite.{i}.remove"), x.rect, "Remove from Favorites");
                if x.clicked() {
                    run.push(("consoleEffect.favorite", with_target(&f.target, json!({"value": false}))));
                }
                ui.end_row();
            }
        });
        if n == 0 {
            ui.label(egui::RichText::new(tr("No favorites yet: right-click a result in the console, or search below.")).color(t.text_faint));
        }
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(tr("Add a favorite:"));
    });
    st.slot = None;
    if let Some(target) = finder(app, ui, cat, settings, st, "Search effects and presets") {
        run.push(("consoleEffect.favorite", with_target(&target, json!({"value": true}))));
    }
}

fn shortcuts_page(
    app: &mut EffectcraftApp,
    ui: &mut egui::Ui,
    cat: &Catalog,
    settings: &console_effect::Settings,
    st: &mut SettingsState,
    run: &mut Vec<(&'static str, Value)>,
    t: &Tokens,
) {
    ui.label(egui::RichText::new(tr("With an empty search, press 1–9 in the console to apply a shortcut at once.")).color(t.text_dim));
    ui.add_space(6.0);
    egui::Grid::new("fx-shortcuts").num_columns(5).spacing(vec2(10.0, 6.0)).striped(true).show(ui, |ui| {
        for n in 1..=SLOTS {
            let sc = settings.shortcut(n);
            ui.label(egui::RichText::new(n.to_string()).color(GOLD).strong());
            let what = sc.map_or_else(|| tr("Empty").to_string(), |s| describe(cat, &s.target).0);
            let b = ui.selectable_label(st.slot == Some(n), what.clone()).on_hover_text(tr("Choose what this shortcut applies"));
            app.auto.add(&format!("consoleEffectSettings.shortcut.{n}.target"), b.rect, &what);
            if b.clicked() {
                st.slot = if st.slot == Some(n) { None } else { Some(n) };
                st.find.clear();
            }
            let draft = st.labels.entry(n).or_insert_with(|| sc.map(|s| s.label.clone()).unwrap_or_default());
            let r = ui.add_enabled_ui(sc.is_some(), |ui| ui.add_sized(vec2(170.0, 20.0), egui::TextEdit::singleline(draft).hint_text(tr("Label")))).inner;
            app.auto.add(&format!("consoleEffectSettings.shortcut.{n}.label"), r.rect, "Label");
            if r.lost_focus() && sc.is_some_and(|s| s.label != *draft) {
                run.push(("consoleEffect.shortcut", json!({"slot": n, "label": draft.clone()})));
            }
            ui.horizontal(|ui| {
                let up = ui.add_enabled(n > 1 && sc.is_some(), egui::Button::new("▲").small());
                app.auto.add(&format!("consoleEffectSettings.shortcut.{n}.up"), up.rect, "Move Up");
                if up.clicked() {
                    run.push(("consoleEffect.shortcut", json!({"slot": n, "moveTo": n - 1})));
                    st.labels.clear();
                }
                let down = ui.add_enabled(n < SLOTS && sc.is_some(), egui::Button::new("▼").small());
                app.auto.add(&format!("consoleEffectSettings.shortcut.{n}.down"), down.rect, "Move Down");
                if down.clicked() {
                    run.push(("consoleEffect.shortcut", json!({"slot": n, "moveTo": n + 1})));
                    st.labels.clear();
                }
            });
            let x = ui.add_enabled(sc.is_some(), egui::Button::new(tr("Clear")));
            app.auto.add(&format!("consoleEffectSettings.shortcut.{n}.clear"), x.rect, "Clear");
            if x.clicked() {
                run.push(("consoleEffect.shortcut", json!({"slot": n, "clear": true})));
                st.labels.remove(&n);
            }
            ui.end_row();
        }
    });
    if let Some(n) = st.slot {
        ui.add_space(8.0);
        ui.label(format!("{} {n}:", tr("Shortcut")));
        if let Some(target) = finder(app, ui, cat, settings, st, "Search effects and presets") {
            run.push(("consoleEffect.shortcut", with_target(&target, json!({"slot": n}))));
            st.slot = None;
            st.labels.remove(&n);
        }
    }
}

fn blacklist_page(
    app: &mut EffectcraftApp,
    ui: &mut egui::Ui,
    cat: &Catalog,
    settings: &console_effect::Settings,
    run: &mut Vec<(&'static str, Value)>,
    t: &Tokens,
) {
    ui.label(egui::RichText::new(tr("Blacklisted effects and presets never show in the console's results.")).color(t.text_dim));
    ui.add_space(6.0);
    if settings.blacklist.is_empty() {
        ui.label(egui::RichText::new(tr("Nothing is blacklisted: right-click a result in the console to hide it.")).color(t.text_faint));
        return;
    }
    egui::ScrollArea::vertical().max_height(400.0).auto_shrink([false, true]).show(ui, |ui| {
        egui::Grid::new("fx-blacklist").num_columns(3).spacing(vec2(12.0, 6.0)).striped(true).show(ui, |ui| {
            for (i, target) in settings.blacklist.iter().enumerate() {
                let (name, category, _) = describe(cat, target);
                ui.label(name);
                ui.label(egui::RichText::new(category).color(t.text_faint));
                let b = ui.button(tr("Show Again"));
                app.auto.add(&format!("consoleEffectSettings.blacklist.{i}.remove"), b.rect, "Show Again");
                if b.clicked() {
                    run.push(("consoleEffect.blacklist", with_target(target, json!({"value": false}))));
                }
                ui.end_row();
            }
        });
    });
}

#[allow(clippy::too_many_arguments)]
fn general_page(
    app: &mut EffectcraftApp,
    ui: &mut egui::Ui,
    settings: &console_effect::Settings,
    st: &mut SettingsState,
    run: &mut Vec<(&'static str, Value)>,
    invoke: &mut Vec<(&'static str, Value)>,
    keys_editor: &mut bool,
    t: &Tokens,
) {
    egui::Grid::new("fx-general").num_columns(2).spacing(vec2(14.0, 10.0)).show(ui, |ui| {
        ui.label(tr("Console color:"));
        ui.horizontal(|ui| {
            let mut c = parse_hex(&settings.color).unwrap_or(t.accent);
            let r = egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque);
            app.auto.add("consoleEffectSettings.color", r.rect, "Console color");
            if r.changed() {
                run.push(("consoleEffect.setSettings", json!({"color": format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())})));
            }
            let b = ui.add_enabled(!settings.color.is_empty(), egui::Button::new(tr("Theme Color")));
            app.auto.add("consoleEffectSettings.themeColor", b.rect, "Theme Color");
            if b.clicked() {
                run.push(("consoleEffect.setSettings", json!({"color": ""})));
            }
        });
        ui.end_row();

        ui.label(tr("Shortcut key:"));
        ui.horizontal(|ui| {
            let key = console_shortcut(app);
            ui.label(egui::RichText::new(if key.is_empty() { tr("None").to_string() } else { key }).strong());
            let b = ui.button(tr("Change..."));
            app.auto.add("consoleEffectSettings.changeShortcut", b.rect, "Change shortcut");
            if b.clicked() {
                *keys_editor = true;
            }
        });
        ui.end_row();

        ui.label(tr("Results:"));
        ui.horizontal(|ui| {
            let mut presets = settings.presets;
            let r = ui.checkbox(&mut presets, tr("List animation presets"));
            app.auto.add("consoleEffectSettings.presets", r.rect, "List animation presets");
            if r.changed() {
                run.push(("consoleEffect.setSettings", json!({"presets": presets})));
            }
            let mut n = settings.max_results;
            let r = ui.add(egui::DragValue::new(&mut n).range(5..=500).suffix(format!(" {}", tr("per section"))));
            app.auto.add("consoleEffectSettings.maxResults", r.rect, "Results per section");
            if r.changed() {
                run.push(("consoleEffect.setSettings", json!({"maxResults": n})));
            }
        });
        ui.end_row();

        ui.label(tr("Snapshots:"));
        ui.horizontal(|ui| {
            for (f, label) in [("png", "PNG (keeps transparency)"), ("jpg", "JPG")] {
                let r = ui.radio(settings.snapshot_format == f, tr(label));
                app.auto.add(&format!("consoleEffectSettings.format.{f}"), r.rect, label);
                if r.clicked() {
                    run.push(("consoleEffect.setSettings", json!({"snapshotFormat": f})));
                }
            }
            let mut q = settings.jpeg_quality;
            let r = ui.add(egui::Slider::new(&mut q, 1..=100).text(tr("JPG quality")));
            app.auto.add("consoleEffectSettings.jpegQuality", r.rect, "JPG quality");
            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                run.push(("consoleEffect.setSettings", json!({"jpegQuality": q})));
            }
        });
        ui.end_row();

        ui.label(tr("Gallery folder:"));
        ui.horizontal(|ui| {
            let shown = gallery::folder(&app.session).map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
            let draft = st.folder.get_or_insert_with(|| settings.gallery_folder.clone());
            let r = ui.add_sized(vec2(320.0, 20.0), egui::TextEdit::singleline(draft).hint_text(shown));
            app.auto.add("consoleEffectSettings.galleryFolder", r.rect, "Gallery folder");
            if r.lost_focus() && *draft != settings.gallery_folder {
                run.push(("consoleEffect.setSettings", json!({"galleryFolder": draft.clone()})));
            }
            let b = ui.button(tr("Browse..."));
            app.auto.add("consoleEffectSettings.browseFolder", b.rect, "Browse");
            if b.clicked()
                && let Some(pick) = app.hooks.pick_folder.as_ref()
                && let Some(folder) = pick()
            {
                st.folder = Some(folder.clone());
                run.push(("consoleEffect.setSettings", json!({"galleryFolder": folder})));
            }
            let b = ui.add_enabled(!settings.gallery_folder.is_empty(), egui::Button::new(tr("Default")));
            app.auto.add("consoleEffectSettings.defaultFolder", b.rect, "Default folder");
            if b.clicked() {
                st.folder = Some(String::new());
                run.push(("consoleEffect.setSettings", json!({"galleryFolder": ""})));
            }
            let b = ui.button(tr("Reveal"));
            app.auto.add("consoleEffectSettings.revealFolder", b.rect, "Reveal");
            if b.clicked() {
                run.push(("consoleEffect.revealGallery", json!({})));
            }
        });
        ui.end_row();

        ui.label(tr("Settings file:"));
        ui.horizontal(|ui| {
            for (id, label, cmd) in
                [("import", "Import Settings...", "consoleEffect.importSettings"), ("export", "Export Settings...", "consoleEffect.exportSettings")]
            {
                let b = ui.button(tr(label));
                app.auto.add(&format!("consoleEffectSettings.{id}"), b.rect, label);
                if b.clicked() {
                    invoke.push((cmd, json!({})));
                    st.aliases.clear();
                    st.labels.clear();
                }
            }
            let b = ui.button(tr("Reset All"));
            app.auto.add("consoleEffectSettings.reset", b.rect, "Reset All");
            if b.clicked() {
                run.push(("consoleEffect.setSettings", json!({"reset": true})));
                st.aliases.clear();
                st.labels.clear();
                st.folder = None;
            }
        });
        ui.end_row();
    });
}

// ---------------------------------------------------------------- the gallery

/// A decoded snapshot: its texture and pixels (for colour sampling).
struct Loaded {
    modified: u64,
    tex: egui::TextureHandle,
    image: Arc<egui::ColorImage>,
    used: f64,
}

/// The Console Effect Gallery window.
#[derive(Default)]
pub struct Gallery {
    pub open: bool,
    /// The selected snapshots (up to four are compared side by side).
    pub selected: Vec<String>,
    /// 100 % (Z) instead of fitting the preview.
    pub actual_size: bool,
    /// Thumbnail height (points); 0 = the default.
    pub thumb: f32,
    /// List again next frame.
    pub dirty: bool,
    shots: Vec<gallery::Shot>,
    listed_at: f64,
    images: HashMap<String, Loaded>,
    /// The window's rect last frame (its keys apply while the pointer is over it).
    rect: Option<Rect>,
}

/// The gallery's keys, taken before the app's shortcuts (Delete must not delete a layer, Z must
/// not pick the Zoom tool) while the pointer is over the gallery.
pub fn gallery_keys(app: &mut EffectcraftApp, ctx: &egui::Context) {
    let g = &app.console_gallery;
    if !g.open || app.dialog.is_some() || ctx.egui_wants_keyboard_input() {
        return;
    }
    let Some(rect) = g.rect else { return };
    if !ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| rect.contains(p)) {
        return;
    }
    let (down, up, shift_down, shift_up, z, delete, copy) = ctx.input_mut(|i| {
        let shift_down = i.consume_key(Modifiers::SHIFT, Key::ArrowDown);
        let shift_up = i.consume_key(Modifiers::SHIFT, Key::ArrowUp);
        let copy = i.consume_key(Modifiers::COMMAND, Key::C) || {
            let n = i.events.len();
            i.events.retain(|e| !matches!(e, egui::Event::Copy));
            n != i.events.len()
        };
        (
            i.consume_key(Modifiers::NONE, Key::ArrowDown),
            i.consume_key(Modifiers::NONE, Key::ArrowUp),
            shift_down,
            shift_up,
            i.consume_key(Modifiers::NONE, Key::Z),
            i.consume_key(Modifiers::NONE, Key::Delete) || i.consume_key(Modifiers::NONE, Key::Backspace),
            copy,
        )
    });
    if down || up || shift_down || shift_up {
        step(app, if down || shift_down { 1 } else { -1 }, shift_down || shift_up);
    }
    if z {
        app.console_gallery.actual_size = !app.console_gallery.actual_size;
    }
    if delete {
        delete_selected(app);
    }
    if copy && let Err(e) = copy_snapshot(app, ctx) {
        app.ui.status = e;
    }
}

/// Move the selection by `d` snapshots (`extend`: add to the comparison).
fn step(app: &mut EffectcraftApp, d: i32, extend: bool) {
    let g = &mut app.console_gallery;
    if g.shots.is_empty() {
        return;
    }
    let cur = g.selected.last().and_then(|n| g.shots.iter().position(|s| &s.name == n));
    let next = match cur {
        Some(i) => (i as i64 + i64::from(d)).clamp(0, g.shots.len() as i64 - 1) as usize,
        None => 0,
    };
    let Some(name) = g.shots.get(next).map(|s| s.name.clone()) else { return };
    if extend {
        g.selected.retain(|n| n != &name);
        g.selected.push(name);
        let over = g.selected.len().saturating_sub(COMPARE_MAX);
        g.selected.drain(..over);
    } else {
        g.selected = vec![name];
    }
}

fn delete_selected(app: &mut EffectcraftApp) {
    let names = app.console_gallery.selected.clone();
    if names.is_empty() {
        return;
    }
    if exec(app, "consoleEffect.deleteSnapshots", json!({"names": names})).is_some() {
        app.console_gallery.selected.clear();
        app.console_gallery.dirty = true;
    }
}

/// Copy the (first) selected snapshot to the clipboard.
fn copy_snapshot(app: &mut EffectcraftApp, ctx: &egui::Context) -> Result<(), String> {
    let name = app.console_gallery.selected.first().cloned().ok_or("Select a snapshot first")?;
    let path = gallery::resolve(&app.session, &name)?;
    let bytes = app.session.services.read_file(&path.to_string_lossy()).map_err(|e| e.to_string())?;
    let (w, h, rgba) = gallery::decode(&bytes)?;
    ctx.copy_image(egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba));
    app.toast = Some((tr("Snapshot copied to the clipboard").into(), ctx.input(|i| i.time)));
    Ok(())
}

/// The snapshot `shot` as a texture (decoded once; a few per frame).
fn image(app: &mut EffectcraftApp, ctx: &egui::Context, shot: &gallery::Shot, budget: &mut usize) -> Option<(egui::TextureId, egui::Vec2)> {
    let now = ctx.input(|i| i.time);
    if let Some(l) = app.console_gallery.images.get_mut(&shot.path)
        && l.modified == shot.modified
    {
        l.used = now;
        return Some((l.tex.id(), l.tex.size_vec2()));
    }
    if *budget == 0 {
        ctx.request_repaint();
        return None;
    }
    *budget -= 1;
    let bytes = app.session.services.read_file(&shot.path).ok()?;
    let (w, h, rgba) = gallery::decode(&bytes).ok()?;
    let img = Arc::new(egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba));
    let tex = ctx.load_texture(format!("console-gallery:{}", shot.name), egui::ImageData::Color(img.clone()), egui::TextureOptions::LINEAR);
    let out = (tex.id(), tex.size_vec2());
    let images = &mut app.console_gallery.images;
    images.insert(shot.path.clone(), Loaded { modified: shot.modified, tex, image: img, used: now });
    // Keep the most recently used.
    while images.len() > IMAGE_CACHE {
        let Some(old) = images.iter().min_by(|a, b| a.1.used.total_cmp(&b.1.used)).map(|(k, _)| k.clone()) else { break };
        images.remove(&old);
    }
    Some(out)
}

/// `size` fitted inside `area`, centred.
fn fit(size: egui::Vec2, area: Rect) -> Rect {
    if size.x <= 0.0 || size.y <= 0.0 {
        return Rect::from_center_size(area.center(), egui::Vec2::ZERO);
    }
    let k = (area.width() / size.x).min(area.height() / size.y);
    Rect::from_center_size(area.center(), size * k)
}

/// Draw the gallery window (Window ▸ Console Effect Gallery).
pub fn gallery_window(app: &mut EffectcraftApp, ctx: &egui::Context) {
    if !app.console_gallery.open {
        app.console_gallery.rect = None;
        return;
    }
    let t = app.tokens;
    let now = ctx.input(|i| i.time);
    if app.console_gallery.dirty || now - app.console_gallery.listed_at > 2.0 {
        app.console_gallery.shots = gallery::list(&app.session);
        app.console_gallery.listed_at = now;
        app.console_gallery.dirty = false;
        let names: Vec<String> = app.console_gallery.shots.iter().map(|s| s.name.clone()).collect();
        app.console_gallery.selected.retain(|n| names.contains(n));
        if app.console_gallery.selected.is_empty()
            && let Some(first) = names.first()
        {
            app.console_gallery.selected.push(first.clone());
        }
    }
    let mut open = true;
    let mut actions: Vec<(&'static str, Value)> = vec![];
    let mut budget = 3usize;
    let resp = egui::Window::new(tr("Console Effect Gallery"))
        .id(egui::Id::new("console-gallery"))
        .open(&mut open)
        .resizable(true)
        .collapsible(false)
        .default_size([820.0, 520.0])
        .min_size([380.0, 260.0])
        .show(ctx, |ui| {
            gallery_toolbar(app, ui, ctx, &mut actions);
            ui.separator();
            let body = ui.available_rect_before_wrap();
            let thumb = if app.console_gallery.thumb > 0.0 { app.console_gallery.thumb } else { 72.0 };
            let left_w = (thumb * 16.0 / 9.0 + 24.0).clamp(110.0, (body.width() * 0.45).max(110.0));
            let left = Rect::from_min_size(body.min, vec2(left_w, body.height()));
            let right = Rect::from_min_max(pos2(left.max.x + 8.0, body.min.y), body.max);
            let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(left));
            thumbnails(app, &mut list_ui, ctx, thumb, &mut budget);
            preview(app, ui, ctx, right, &mut budget, &t);
            // (The window keeps the body's size; no widget over the thumbnails and the preview.)
            ui.advance_cursor_after_rect(body);
        });
    app.console_gallery.rect = resp.as_ref().map(|r| r.response.rect);
    if let Some(r) = &resp {
        app.auto.add("consoleGallery.window", r.response.rect, "Console Effect Gallery");
    }
    if !open {
        app.console_gallery.open = false;
    }
    for (id, p) in actions {
        match id {
            "copy" => {
                if let Err(e) = copy_snapshot(app, ctx) {
                    app.ui.status = e;
                }
            }
            "delete" => delete_selected(app),
            "snapshot" => snapshot(app),
            id => {
                if let Err(e) = crate::menus::invoke(app, ctx, id, p) {
                    app.ui.status = e;
                }
                app.console_gallery.dirty = true;
            }
        }
    }
}

fn gallery_toolbar(app: &mut EffectcraftApp, ui: &mut egui::Ui, _ctx: &egui::Context, actions: &mut Vec<(&'static str, Value)>) {
    let names = app.console_gallery.selected.clone();
    let any = !names.is_empty();
    ui.horizontal(|ui| {
        let mut button = |ui: &mut egui::Ui, id: &str, label: &str, enabled: bool| {
            let b = ui.add_enabled(enabled, egui::Button::new(tr(label)));
            app.auto.add(&format!("consoleGallery.{id}"), b.rect, label);
            b.clicked()
        };
        if button(ui, "snapshot", "Snapshot", true) {
            actions.push(("snapshot", json!({})));
        }
        if button(ui, "copy", "Copy", any) {
            actions.push(("copy", json!({})));
        }
        if button(ui, "import", "Import into Project", any) {
            actions.push(("consoleEffect.importSnapshots", json!({"names": names})));
        }
        if button(ui, "export", "Export...", any) {
            actions.push(("consoleEffect.exportSnapshots", json!({"names": names})));
        }
        if button(ui, "delete", "Delete", any) {
            actions.push(("delete", json!({})));
        }
        if button(ui, "reveal", "Reveal", true) {
            actions.push(("consoleEffect.revealGallery", json!({})));
        }
        ui.separator();
        let mut actual = app.console_gallery.actual_size;
        let r = ui.toggle_value(&mut actual, "100%").on_hover_text(tr("Full resolution (Z)"));
        app.auto.add("consoleGallery.actualSize", r.rect, "100%");
        app.console_gallery.actual_size = actual;
        let mut th = if app.console_gallery.thumb > 0.0 { app.console_gallery.thumb } else { 72.0 };
        let r = ui.add(egui::Slider::new(&mut th, 36.0..=180.0).show_value(false).text(tr("Thumbnails")));
        app.auto.add("consoleGallery.thumbSize", r.rect, "Thumbnail size");
        app.console_gallery.thumb = th;
    });
}

fn thumbnails(app: &mut EffectcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, thumb: f32, budget: &mut usize) {
    let t = app.tokens;
    let shots = app.console_gallery.shots.clone();
    if shots.is_empty() {
        ui.label(egui::RichText::new(tr("No snapshots yet: take one with the camera button in the console or this window.")).color(t.text_faint));
        return;
    }
    let mut clicked: Option<(String, Modifiers)> = None;
    let mut menu: Option<(&'static str, String)> = None;
    egui::ScrollArea::vertical().id_salt("console-gallery-thumbs").auto_shrink([false, false]).show(ui, |ui| {
        for (i, shot) in shots.iter().enumerate() {
            let w = ui.available_width();
            let (r, resp) = ui.allocate_exact_size(vec2(w, thumb + 18.0), Sense::click_and_drag());
            if !ui.is_rect_visible(r) {
                continue;
            }
            let selected = app.console_gallery.selected.contains(&shot.name);
            let p = ui.painter();
            if selected {
                p.rect_filled(r, 4.0, t.row_selected);
            } else if resp.hovered() {
                p.rect_filled(r, 4.0, t.hover);
            }
            let img_area = Rect::from_min_max(r.min + vec2(4.0, 4.0), pos2(r.max.x - 4.0, r.max.y - 18.0));
            if let Some((tex, size)) = image(app, ctx, shot, budget) {
                let fr = fit(size, img_area);
                checker(ui.painter(), fr);
                ui.painter().image(tex, fr, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            widgets::text_fit(ui.painter(), pos2(r.min.x + 4.0, r.max.y - 9.0), Align2::LEFT_CENTER, &shot.name, Tokens::ui(10.5), r.width() - 8.0, t.text_dim);
            app.auto.add(&format!("consoleGallery.shot.{i}"), r, &shot.name);
            if resp.clicked() {
                clicked = Some((shot.name.clone(), ui.input(|i| i.modifiers)));
            }
            // Drag a snapshot into the Project panel (import) or the Timeline (import and add).
            if resp.drag_started() {
                egui::DragAndDrop::set_payload(ctx, DragPayload::Files(vec![shot.path.clone()]));
            }
            resp.context_menu(|ui| {
                for (id, label) in [
                    ("copy", "Copy to Clipboard"),
                    ("consoleEffect.importSnapshots", "Import into Project"),
                    ("consoleEffect.exportSnapshots", "Export..."),
                    ("consoleEffect.revealGallery", "Reveal in File Manager"),
                    ("delete", "Delete"),
                ] {
                    if ui.button(tr(label)).clicked() {
                        menu = Some((id, shot.name.clone()));
                        ui.close();
                    }
                }
            });
        }
    });
    if let Some((name, m)) = clicked {
        let g = &mut app.console_gallery;
        if m.command || m.shift {
            if g.selected.contains(&name) {
                g.selected.retain(|n| n != &name);
            } else {
                g.selected.push(name);
                let over = g.selected.len().saturating_sub(COMPARE_MAX);
                g.selected.drain(..over);
            }
        } else {
            g.selected = vec![name];
        }
    }
    if let Some((id, name)) = menu {
        if !app.console_gallery.selected.contains(&name) {
            app.console_gallery.selected = vec![name.clone()];
        }
        match id {
            "copy" => {
                if let Err(e) = copy_snapshot(app, ctx) {
                    app.ui.status = e;
                }
            }
            "delete" => delete_selected(app),
            id => {
                let p = if id == "consoleEffect.revealGallery" { json!({"name": name}) } else { json!({"names": app.console_gallery.selected.clone()}) };
                if let Err(e) = crate::menus::invoke(app, ctx, id, p) {
                    app.ui.status = e;
                }
            }
        }
    }
}

/// A transparency checkerboard behind `r`.
fn checker(p: &egui::Painter, r: Rect) {
    p.rect_filled(r, 0.0, Color32::from_gray(0x66));
    let s = 8.0;
    let (nx, ny) = ((r.width() / s).ceil() as i32, (r.height() / s).ceil() as i32);
    if nx * ny > 40_000 {
        return;
    }
    for y in 0..ny {
        for x in (y % 2..nx).step_by(2) {
            let c = Rect::from_min_size(pos2(r.min.x + x as f32 * s, r.min.y + y as f32 * s), vec2(s, s)).intersect(r);
            p.rect_filled(c, 0.0, Color32::from_gray(0x99));
        }
    }
}

/// The selected snapshots side by side (fitted, or at 100 %); an armed Effect Controls
/// eyedropper samples them.
fn preview(app: &mut EffectcraftApp, ui: &mut egui::Ui, ctx: &egui::Context, area: Rect, budget: &mut usize, t: &Tokens) {
    let shots: Vec<gallery::Shot> =
        app.console_gallery.selected.iter().filter_map(|n| app.console_gallery.shots.iter().find(|s| &s.name == n).cloned()).collect();
    let p = ui.painter().with_clip_rect(area);
    p.rect_filled(area, 4.0, t.app_bg);
    if shots.is_empty() {
        return;
    }
    let n = shots.len();
    let (cols, rows) = match n {
        1 => (1, 1),
        2 => (2, 1),
        _ => (2, 2),
    };
    let cell = vec2(area.width() / cols as f32, area.height() / rows as f32);
    let pick = app.ui.fx_pick.clone().filter(|p| p.kind == "color");
    let mut sampled: Option<[f32; 3]> = None;
    for (i, shot) in shots.iter().enumerate() {
        let c = Rect::from_min_size(area.min + vec2((i % cols) as f32 * cell.x, (i / cols) as f32 * cell.y), cell).shrink(6.0);
        let caption = Rect::from_min_max(pos2(c.min.x, c.max.y - 16.0), c.max);
        let room = Rect::from_min_max(c.min, pos2(c.max.x, caption.min.y - 2.0));
        let Some((tex, size)) = image(app, ctx, shot, budget) else { continue };
        let r = if app.console_gallery.actual_size { Rect::from_center_size(room.center(), size / ctx.pixels_per_point()) } else { fit(size, room) };
        let pc = p.with_clip_rect(room);
        checker(&pc, r.intersect(room));
        pc.image(tex, r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        widgets::text_fit(&p, caption.left_center(), Align2::LEFT_CENTER, &shot.name, Tokens::ui(11.0), caption.width(), t.text_dim);
        let resp = ui.interact(room, egui::Id::new(("console-gallery-preview", i)), Sense::click());
        app.auto.add(&format!("consoleGallery.preview.{i}"), room, &shot.name);
        // The eyedropper: the snapshot's real colour under the pointer.
        if pick.is_some()
            && let Some(hp) = resp.hover_pos()
            && r.contains(hp)
            && let Some(img) = app.console_gallery.images.get(&shot.path).map(|l| l.image.clone())
        {
            ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
            let u = ((hp.x - r.min.x) / r.width() * img.size[0] as f32) as usize;
            let v = ((hp.y - r.min.y) / r.height() * img.size[1] as f32) as usize;
            if let Some(px) = img.pixels.get(v.min(img.size[1].saturating_sub(1)) * img.size[0] + u.min(img.size[0].saturating_sub(1))) {
                let [r8, g8, b8, _] = px.to_srgba_unmultiplied();
                let sw = Rect::from_min_size(hp + vec2(14.0, 14.0), vec2(26.0, 18.0));
                ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("console-gallery-swatch"))).rect_filled(
                    sw,
                    2.0,
                    Color32::from_rgb(r8, g8, b8),
                );
                if resp.clicked() {
                    sampled = Some([f32::from(r8) / 255.0, f32::from(g8) / 255.0, f32::from(b8) / 255.0]);
                }
            }
        }
    }
    if let (Some(c), Some(pick)) = (sampled, pick) {
        exec(app, "prop.set", json!({"layer": pick.layer, "prop": pick.prop, "value": [c[0], c[1], c[2], 1.0]}));
        app.ui.fx_pick = None;
    }
}
