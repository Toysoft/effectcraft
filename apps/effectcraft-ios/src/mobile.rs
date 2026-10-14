//! Native window integration around the complete shared desktop editor.
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use effectcraft_ui_egui::{EffectcraftApp, dock};
use serde_json::json;

use crate::files::Files;

pub struct MobileApp {
    editor: EffectcraftApp,
    files: Files,
    import_on_start: bool,
    display: Arc<Mutex<crate::display::DisplaySettings>>,
    cutout: crate::display::CutoutSide,
    #[cfg(target_os = "ios")]
    applied_display: crate::display::DisplaySettings,
    #[cfg(target_os = "ios")]
    native: Option<objc2::rc::Retained<objc2_ui_kit::UIWindow>>,
    #[cfg(target_os = "ios")]
    keyboard: crate::keyboard::Keyboard,
    focused: bool,
    error: Option<String>,
}

fn limit_memory(s: &mut effectcraft_engine::Session) {
    // Persisted desktop-sized values cannot make the first mobile preview allocate gigabytes.
    s.prefs.memory.layer_cache_mb = s.prefs.memory.layer_cache_mb.min(128);
    s.prefs.memory.media_cache_mb = s.prefs.memory.media_cache_mb.min(128);
    s.prefs.memory.preview_cache_mb = s.prefs.memory.preview_cache_mb.min(256);
    s.prefs.disk.disk_cache_max_gb = s.prefs.disk.disk_cache_max_gb.min(2);
    s.prefs.previews.cache_frames_when_idle = false;
    s.apply_cache_budgets();
    s.configure_disk_cache();
}

impl MobileApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut window_error: Option<String> = None;
        #[cfg(target_os = "ios")]
        let mut native = None;
        #[cfg(target_os = "ios")]
        {
            if let Some(window) = cc.winit_window() {
                use winit::platform::ios::{ValidOrientations, WindowExtIOS};
                window.set_valid_orientations(ValidOrientations::Landscape);
                match attach_window_scene(window) {
                    Ok(window) => native = Some(window),
                    Err(error) => window_error = Some(error),
                }
                window.request_redraw();
            } else {
                window_error = Some("The native editor window is unavailable".into());
            }
        }
        let failures = effectcraft_ui_egui::gpu_failure::GpuFailureBridge::new(&cc.egui_ctx);
        if let Some(rs) = &cc.wgpu_render_state {
            let errors = failures.clone();
            rs.device.on_uncaptured_error(Arc::new(move |error| {
                errors.report(&format!("uncaptured GPU error: {error}"), false);
            }));
            let lost = failures.clone();
            rs.device.set_device_lost_callback(move |reason, message| {
                lost.report(&format!("GPU device lost ({reason:?}): {message}"), true);
            });
        }
        let mut session = effectcraft_host::session();
        if let Some(dir) = effectcraft_host::config_dir() {
            session.config = Some(Arc::new(effectcraft_engine::config::DirConfig::new(dir)));
        }
        session.load_settings();
        let display = crate::display::DisplaySettings::load(&mut session);
        limit_memory(&mut session);
        if cfg!(debug_assertions)
            && std::env::args().any(|a| a == "--demo")
            && let Err(e) = session.execute("file.openDemoProject", json!({}))
        {
            session.toast(format!("Cannot open demo: {e}"));
        }
        session.check_footage_on_open = true;
        session.autosave.background = true;
        let recovery = session.begin_recovery();
        let root = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("Documents");
        let mut error = std::fs::create_dir_all(&root).err().map(|e| format!("Cannot open Documents: {e}"));
        if let Some(e) = window_error.take() {
            log::error!("{e}");
            error = Some(e);
        }
        // Queue outputs must land in writable sandbox storage, even for unsaved projects.
        let exports = root.join("Exports");
        if let Err(e) = std::fs::create_dir_all(&exports) {
            error = Some(format!("Cannot create Exports folder: {e}"));
        }
        session.prefs.export.default_output_folder = exports.to_string_lossy().into_owned();
        session.prefs_changed();
        let files = Files::new(root);
        let mut editor = EffectcraftApp::new(session);
        editor.set_gpu_failure_bridge(failures);
        #[cfg(debug_assertions)]
        if let Some(port) = std::env::args().find_map(|a| a.strip_prefix("--control=").map(str::to_string)) {
            match port.parse::<u16>() {
                Ok(port) if port > 0 => editor = editor.with_control(crate::control_server::start(port, cc.egui_ctx.clone())),
                _ => editor.session.toast("Control port must be from 1 to 65535"),
            }
        }
        editor.ui.start_screen = false;
        if display.initialize_workspace {
            editor.ui.workspace = "Small Screen".into();
            editor.ui.dock = dock::workspace("Small Screen");
        }
        editor.apply_prefs(&cc.egui_ctx);
        #[cfg(target_os = "ios")]
        if let Some(window) = &native {
            display.apply_native(window);
        }
        #[cfg(target_os = "ios")]
        let applied_display = display.clone();
        let display = Arc::new(Mutex::new(display));
        editor.hooks.display_preferences = Some(Box::new(crate::display::DisplayPreferences::new(display.clone())));
        editor.hooks.file_dialog_async = Some(files.hook(&cc.egui_ctx));
        editor.hooks.file_dialog_written = Some(files.write_hook(&cc.egui_ctx));
        #[cfg(target_os = "ios")]
        {
            editor.hooks.clipboard_text = Some(Box::new(crate::keyboard::clipboard_text));
        }
        editor.hooks.audio_device = Some(Box::new(crate::audio_out::open));
        editor.hooks.audio_devices = Some(Box::new(crate::audio_out::devices));
        if let Some(r) = recovery {
            editor.offer_recovery(r);
        }
        if editor.session.config.is_none() {
            error = Some("Settings storage is unavailable; save projects in Documents.".into());
        }
        Self {
            editor,
            files,
            import_on_start: cfg!(debug_assertions) && std::env::args().any(|arg| arg == "--import"),
            display,
            cutout: crate::display::CutoutSide::Unknown,
            #[cfg(target_os = "ios")]
            applied_display,
            focused: true,
            error,
            #[cfg(target_os = "ios")]
            native,
            #[cfg(target_os = "ios")]
            keyboard: crate::keyboard::Keyboard::default(),
        }
    }
}

/// Associate winit's retained UIWindow with the scene UIKit creates from Info.plist.
/// The user authorized this one interoperability exception on 2026-10-14; no reference escapes.
#[cfg(target_os = "ios")]
#[allow(unsafe_code)]
fn attach_window_scene(window: &winit::window::Window) -> Result<objc2::rc::Retained<objc2_ui_kit::UIWindow>, String> {
    use objc2::MainThreadMarker;
    use objc2_ui_kit::{UIApplication, UIView, UIWindowScene};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let main = MainThreadMarker::new().ok_or("UIKit window setup requires the main thread")?;
    let app = UIApplication::sharedApplication(main);
    let scene = app.connectedScenes().iter().find_map(|s| s.downcast::<UIWindowScene>().ok()).ok_or("UIKit has not connected the editor's window scene")?;
    let handle = window.window_handle().map_err(|e| e.to_string())?;
    let RawWindowHandle::UiKit(handle) = handle.as_raw() else { return Err("Expected a UIKit window handle".into()) };
    // SAFETY: winit owns and retains this UIView. Its borrowed Window and the main-thread
    // marker stay alive for the whole operation. The typed reference is used locally only.
    let view = unsafe { handle.ui_view.cast::<UIView>().as_ref() };
    let native = view.window().ok_or("The editor view has no UIKit window")?;
    native.setWindowScene(Some(&scene));
    native.makeKeyAndVisible();
    log::info!("Editor window attached to its UIKit scene");
    Ok(native)
}

impl eframe::App for MobileApp {
    fn raw_input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        #[cfg(target_os = "ios")]
        if let Some(window) = &self.native {
            self.cutout = crate::display::sync_window(window, ctx, input);
        }
        #[cfg(target_os = "ios")]
        {
            let modal =
                self.native.as_ref().and_then(|window| window.rootViewController()).is_some_and(|controller| controller.presentedViewController().is_some());
            self.keyboard.pump(ctx, input, modal);
        }
        self.editor.raw_input_hook(ctx, input);
    }

    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let focused = ctx.input(|i| i.focused);
        if self.focused && !focused {
            self.editor.stop();
            if self.editor.session.is_dirty() {
                // Commit synchronously before UIKit suspends the process.
                self.editor.session.autosave.background = false;
                if let Err(e) = self.editor.session.autosave_now() {
                    self.error = Some(e.to_string());
                }
                self.editor.session.autosave.background = true;
            }
        }
        self.focused = focused;
        self.editor.logic(ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let content = ui.ctx().input(|input| input.content_rect());
        let display = self.display.lock().unwrap_or_else(PoisonError::into_inner).clone();
        #[cfg(target_os = "ios")]
        if display != self.applied_display
            && let Some(window) = &self.native
        {
            display.apply_native(window);
            self.applied_display = display.clone();
        }
        let (rect, chrome) = display.layout(ui.max_rect(), content, ui.ctx().zoom_factor(), self.cutout);
        self.editor.hooks.chrome_safe_rect = Some(chrome);
        ui.painter().rect_filled(ui.max_rect(), 0.0, self.editor.tokens.app_bg);
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.set_clip_rect(rect);
            self.editor.ui(ui, frame);
        });
        self.editor.auto.add("ios.layout.workspace", rect, "Workspace bounds");
        self.editor.auto.add("ios.layout.chrome", chrome, "Navigation safe bounds");
        if std::mem::take(&mut self.import_on_start)
            && let Err(error) = effectcraft_ui_egui::menus::invoke(&mut self.editor, ui.ctx(), "file.import", json!({}))
        {
            self.error = Some(error);
        }
        // Export only final worker outputs, including the whole folder for frame sequences.
        for item in &self.editor.session.project.render_queue {
            if matches!(item.status, effectcraft_engine::project::render_queue::RenderStatus::Done)
                && let Some(actual) = &item.last_output
            {
                self.files.render_completed(&item.output.output, actual, item.output.format.is_sequence());
            }
        }
        #[cfg(target_os = "ios")]
        self.keyboard.copy_output(ui.ctx());
        self.files.show(ui.ctx(), &mut self.editor.auto);
        if let Some(error) = self.files.take_error() {
            self.error = Some(error);
        }
        if let Some(error) = self.error.clone() {
            egui::Window::new("EffectCraft").collapsible(false).show(ui.ctx(), |ui| {
                ui.label(error);
                let dismiss = ui.button("Dismiss");
                self.editor.auto.add_response("ios.error.dismiss", &dismiss, "Dismiss error");
                if dismiss.clicked() {
                    self.error = None;
                }
            });
        }
    }

    fn on_exit(&mut self) {
        self.editor.on_exit();
    }
}
