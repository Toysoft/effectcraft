//! Persisted display preferences; UIKit retains control of the outer window geometry.
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, PoisonError};

// The desktop development shell has no native cutout; tests exercise both phone sides.
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub enum CutoutSide {
    Left,
    Right,
    #[default]
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplaySettings {
    #[serde(skip)]
    pub initialize_workspace: bool,
    #[serde(default)]
    layout_version: u32,
    edge_padding: u32,
    min_width: u32,
    min_height: u32,
    max_width: u32,
    max_height: u32,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self { initialize_workspace: false, layout_version: 2, edge_padding: 0, min_width: 320, min_height: 240, max_width: 8192, max_height: 8192 }
    }
}

impl DisplaySettings {
    fn normalize(&mut self) {
        if self.layout_version == 0 {
            // Migrate the original boxed layout's default, preserving other chosen spacing.
            if self.edge_padding == 8 {
                self.edge_padding = 0;
            }
            self.layout_version = 2;
        }
        self.layout_version = 2;
        self.edge_padding = self.edge_padding.min(48);
        self.min_width = self.min_width.clamp(240, 4096);
        self.min_height = self.min_height.clamp(180, 4096);
        self.max_width = self.max_width.clamp(self.min_width, 8192);
        self.max_height = self.max_height.clamp(self.min_height, 8192);
    }

    pub fn load(session: &mut effectcraft_engine::Session) -> Self {
        let result = session.config.as_ref().map(|store| store.read_bounded("ios-display.json", 4096));
        let mut settings = match result {
            Some(Ok(Some(text))) => match serde_json::from_str(&text) {
                Ok(settings) => settings,
                Err(error) => {
                    session.toast(format!("Display settings were reset: {error}"));
                    Self::default()
                }
            },
            Some(Err(error)) => {
                session.toast(format!("Cannot read display settings: {error}"));
                Self::default()
            }
            _ => {
                session.prefs.appearance.ui_scale = 60;
                session.prefs_changed();
                session.save_prefs();
                Self { initialize_workspace: true, ..Self::default() }
            }
        };
        if settings.layout_version < 2 && session.prefs.appearance.ui_scale == 40 {
            session.prefs.appearance.ui_scale = 60;
            session.prefs_changed();
            session.save_prefs();
        }
        settings.normalize();
        if let Err(error) = settings.save(session) {
            session.toast(error);
        }
        settings
    }

    /// Keep chrome in UIKit's conservative safe rectangle. Below it, the corner-only
    /// side can use more space; the actual cutout side and home indicator stay reserved.
    pub fn layout(&self, viewport: egui::Rect, safe_area: egui::Rect, zoom: f32, cutout: CutoutSide) -> (egui::Rect, egui::Rect) {
        let safe = viewport.intersect(safe_area);
        let mut workspace = safe;
        match cutout {
            CutoutSide::Left => workspace.max.x += (viewport.right() - safe.right()).max(0.0) * 0.5,
            CutoutSide::Right => workspace.min.x -= (safe.left() - viewport.left()).max(0.0) * 0.5,
            CutoutSide::Unknown => {}
        }
        let padding = self.edge_padding.min(48) as f32 / zoom.max(0.25);
        let inset = |rect: egui::Rect| rect.shrink(padding.min((rect.width().min(rect.height()) - 1.0).max(0.0) / 2.0));
        (inset(workspace), inset(safe))
    }

    pub fn save(&self, session: &effectcraft_engine::Session) -> Result<(), String> {
        let store = session.config.as_ref().ok_or("Settings storage is unavailable")?;
        let text = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
        store.write("ios-display.json", &text).map_err(|error| format!("Cannot save display settings: {error}"))
    }

    fn show_controls(&mut self, ui: &mut egui::Ui, registry: &mut effectcraft_ui_egui::automation::Registry) {
        ui.add_space(8.0);
        ui.heading("iOS Display");
        ui.separator();
        ui.label("The default UI scale is 60%. Change it above for mouse or touch use.");
        let response = ui.add(egui::Slider::new(&mut self.edge_padding, 0..=48).text("Extra edge spacing (points)"));
        registry.add_response("ios.settings.edgePadding", &response, "Extra edge spacing (points)");
        ui.label("Menus avoid the notch and corners. Panels use the wider space below them.");
        let size = ui.ctx().input(|input| input.content_rect().size()) * ui.ctx().zoom_factor();
        ui.label(format!("Available window: {:.0} × {:.0} points", size.x, size.y));
        ui.separator();
        ui.label("Resizable windows • Duo / iPhone Mirroring / iPad");
        ui.label("Drag the system window edges to resize. These limits use iOS points.");
        for (label, id, value, range) in [
            ("Minimum width", "minWidth", &mut self.min_width, 240..=4096),
            ("Minimum height", "minHeight", &mut self.min_height, 180..=4096),
            ("Maximum width", "maxWidth", &mut self.max_width, 240..=8192),
            ("Maximum height", "maxHeight", &mut self.max_height, 180..=8192),
        ] {
            ui.horizontal(|ui| {
                ui.label(label);
                let response = ui.add(egui::DragValue::new(value).range(range).update_while_editing(false));
                registry.add_response(&format!("ios.settings.{id}"), &response, label);
            });
        }
        #[cfg(target_os = "ios")]
        if !has_resizable_scene() {
            ui.label("Window limits take effect when iOS provides a resizable scene.");
        }
        let reset = ui.button("Reset display spacing and window limits");
        registry.add_response("ios.settings.reset", &reset, "Reset display spacing and window limits");
        if reset.clicked() {
            *self = Self::default();
        }
        self.normalize();
    }

    #[cfg(target_os = "ios")]
    pub fn apply_native(&self, window: &objc2_ui_kit::UIWindow) {
        use objc2_core_foundation::CGSize;
        if let Some(scene) = window.windowScene()
            && let Some(limits) = scene.sizeRestrictions()
        {
            // Widen first so switching between two valid ranges cannot temporarily invert them.
            limits.setMinimumSize(CGSize::new(0.0, 0.0));
            limits.setMaximumSize(CGSize::new(self.max_width as f64, self.max_height as f64));
            limits.setMinimumSize(CGSize::new(self.min_width as f64, self.min_height as f64));
        }
    }
}

pub struct DisplayPreferences {
    pub settings: Arc<Mutex<DisplaySettings>>,
    snapshot: Option<DisplaySettings>,
}

impl DisplayPreferences {
    pub fn new(settings: Arc<Mutex<DisplaySettings>>) -> Self {
        Self { settings, snapshot: None }
    }
}

impl effectcraft_ui_egui::HostDisplayPreferences for DisplayPreferences {
    fn opened(&mut self) {
        self.snapshot = Some(self.settings.lock().unwrap_or_else(PoisonError::into_inner).clone());
    }
    fn show(&mut self, ui: &mut egui::Ui, registry: &mut effectcraft_ui_egui::automation::Registry) {
        self.settings.lock().unwrap_or_else(PoisonError::into_inner).show_controls(ui, registry);
    }
    fn closed(&mut self, save: bool, session: &mut effectcraft_engine::Session) {
        let mut settings = self.settings.lock().unwrap_or_else(PoisonError::into_inner);
        if save {
            if let Err(error) = settings.save(session) {
                session.toast(error);
            }
        } else if let Some(snapshot) = self.snapshot.take() {
            *settings = snapshot;
        }
        self.snapshot = None;
    }
}

#[cfg(target_os = "ios")]
fn has_resizable_scene() -> bool {
    use objc2::MainThreadMarker;
    use objc2_ui_kit::{UIApplication, UIWindowScene};
    MainThreadMarker::new().is_some_and(|main| {
        UIApplication::sharedApplication(main)
            .connectedScenes()
            .iter()
            .any(|scene| scene.downcast::<UIWindowScene>().ok().is_some_and(|scene| scene.sizeRestrictions().is_some()))
    })
}

#[cfg(target_os = "ios")]
pub fn sync_window(window: &objc2_ui_kit::UIWindow, ctx: &egui::Context, input: &mut egui::RawInput) -> CutoutSide {
    // UIKit changes UIWindow bounds during folding and window resizing. Keep winit's root
    // rendering view fitted to those bounds so its layout callback resizes the Metal surface.
    if let Some(controller) = window.rootViewController()
        && let Some(view) = controller.view()
    {
        let bounds = window.bounds();
        if view.frame() != bounds {
            view.setFrame(bounds);
            ctx.request_repaint();
        }
        let insets = window.safeAreaInsets();
        let zoom = ctx.zoom_factor().max(0.25);
        input.safe_area_insets = Some(egui::SafeAreaInsets(egui::epaint::MarginF32 {
            left: (insets.left as f32 / zoom).max(0.0),
            top: (insets.top as f32 / zoom).max(0.0),
            right: (insets.right as f32 / zoom).max(0.0),
            bottom: (insets.bottom as f32 / zoom).max(0.0),
        }));
        // On a normal landscape phone, UIKit also reserves the opposite rounded corner.
        // Only relax that gutter when both sides have the familiar symmetric cutout inset.
        // Asymmetric Duo/resizable safe areas remain authoritative.
        if bounds.size.width > bounds.size.height && insets.left >= 40.0 && (insets.left - insets.right).abs() < 1.0 {
            #[allow(deprecated)]
            let orientation = window.windowScene().map(|scene| scene.interfaceOrientation());
            return cutout_from_orientation(orientation);
        }
    }
    CutoutSide::Unknown
}

#[cfg(target_os = "ios")]
fn cutout_from_orientation(orientation: Option<objc2_ui_kit::UIInterfaceOrientation>) -> CutoutSide {
    use objc2_ui_kit::UIInterfaceOrientation;
    // UIInterfaceOrientation reverses UIDeviceOrientation's landscape directions.
    // The physical portrait-top cutout is left when the interface is LandscapeRight.
    match orientation {
        Some(UIInterfaceOrientation::LandscapeLeft) => CutoutSide::Right,
        Some(UIInterfaceOrientation::LandscapeRight) => CutoutSide::Left,
        _ => CutoutSide::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "ios")]
    #[test]
    fn native_orientation_never_relaxes_the_physical_notch_side() {
        use objc2_ui_kit::UIInterfaceOrientation;
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(844.0, 390.0));
        let safe = egui::Rect::from_min_max(egui::pos2(47.0, 0.0), egui::pos2(797.0, 369.0));
        let settings = DisplaySettings::default();
        let left = cutout_from_orientation(Some(UIInterfaceOrientation::LandscapeRight));
        let right = cutout_from_orientation(Some(UIInterfaceOrientation::LandscapeLeft));
        assert_eq!(left, CutoutSide::Left);
        assert_eq!(right, CutoutSide::Right);
        assert_eq!(settings.layout(viewport, safe, 1.0, left).0.left(), 47.0);
        assert_eq!(settings.layout(viewport, safe, 1.0, right).0.right(), 797.0);
        assert_eq!(cutout_from_orientation(None), CutoutSide::Unknown);
    }

    #[test]
    fn wider_workspace_preserves_notch_and_safe_navigation_on_both_sides() {
        let settings = DisplaySettings::default();
        for zoom in [0.25, 0.4, 1.0, 2.0] {
            let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(844.0, 390.0) / zoom);
            let safe = egui::Rect::from_min_max(egui::pos2(47.0 / zoom, 0.0), egui::pos2(797.0 / zoom, 369.0 / zoom));
            for side in [CutoutSide::Left, CutoutSide::Right] {
                let (workspace, chrome) = settings.layout(viewport, safe, zoom, side);
                assert_eq!(chrome, safe);
                assert_eq!(workspace.top(), 0.0);
                assert_eq!(workspace.bottom(), safe.bottom());
                assert!((workspace.width() * zoom - 773.5).abs() < 0.001);
                match side {
                    CutoutSide::Left => assert_eq!(workspace.left(), safe.left()),
                    CutoutSide::Right => assert_eq!(workspace.right(), safe.right()),
                    CutoutSide::Unknown => assert_eq!(workspace, safe),
                }
            }
            assert_eq!(settings.layout(viewport, safe, zoom, CutoutSide::Unknown), (safe, safe));
        }
    }

    #[test]
    fn old_default_spacing_migrates_without_resetting_custom_spacing() {
        for (old, expected) in [(8, 0), (12, 12)] {
            let mut settings = DisplaySettings { layout_version: 0, edge_padding: old, ..DisplaySettings::default() };
            settings.normalize();
            assert_eq!(settings.edge_padding, expected);
            assert_eq!(settings.layout_version, 2);
        }
    }

    #[test]
    fn display_settings_round_trip_preserves_user_scale() {
        use std::sync::Arc;
        let mut session = effectcraft_engine::Session::new();
        session.config = Some(Arc::new(effectcraft_engine::config::MemoryConfig::default()));
        let first = DisplaySettings::load(&mut session);
        assert!(first.initialize_workspace);
        // The non-shipping macOS development shell retains the desktop engine clamp.
        assert_eq!(session.prefs.appearance.ui_scale, if cfg!(target_os = "ios") { 60 } else { 75 });
        let chosen = if cfg!(target_os = "ios") { 65 } else { 85 };
        session.prefs.appearance.ui_scale = chosen;
        let second = DisplaySettings::load(&mut session);
        assert!(!second.initialize_workspace);
        assert_eq!(session.prefs.appearance.ui_scale, chosen);
    }

    #[test]
    fn preferences_cancel_restores_display_and_ok_persists_it() {
        use effectcraft_ui_egui::HostDisplayPreferences;
        let settings = Arc::new(Mutex::new(DisplaySettings::default()));
        let mut page = DisplayPreferences::new(settings.clone());
        let mut session = effectcraft_engine::Session::new();
        session.config = Some(Arc::new(effectcraft_engine::config::MemoryConfig::default()));
        page.opened();
        settings.lock().unwrap().edge_padding = 12;
        page.closed(false, &mut session);
        assert_eq!(settings.lock().unwrap().edge_padding, 0);
        page.opened();
        settings.lock().unwrap().edge_padding = 16;
        page.closed(true, &mut session);
        assert_eq!(DisplaySettings::load(&mut session).edge_padding, 16);
    }

    #[test]
    fn old_scale_default_migrates_once_and_preserves_custom_scales() {
        let migrated = if cfg!(target_os = "ios") { 60 } else { 75 };
        for (scale, expected) in [(40, migrated), (60, 60), (75, 75)] {
            let mut session = effectcraft_engine::Session::new();
            session.config = Some(Arc::new(effectcraft_engine::config::MemoryConfig::default()));
            session.prefs.appearance.ui_scale = scale;
            DisplaySettings { layout_version: 1, ..DisplaySettings::default() }.save(&session).unwrap();
            DisplaySettings::load(&mut session);
            assert_eq!(session.prefs.appearance.ui_scale, expected);
            session.prefs.appearance.ui_scale = 40;
            DisplaySettings::load(&mut session);
            assert_eq!(session.prefs.appearance.ui_scale, 40, "later user choices are preserved");
        }
    }

    #[test]
    fn malformed_window_limits_are_bounded_and_ordered() {
        let mut settings = DisplaySettings {
            initialize_workspace: false,
            layout_version: 2,
            edge_padding: u32::MAX,
            min_width: u32::MAX,
            min_height: 0,
            max_width: 1,
            max_height: u32::MAX,
        };
        settings.normalize();
        assert_eq!(settings.edge_padding, 48);
        assert_eq!((settings.min_width, settings.min_height), (4096, 180));
        assert_eq!((settings.max_width, settings.max_height), (4096, 8192));
    }
}
