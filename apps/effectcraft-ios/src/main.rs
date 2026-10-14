//! Native iOS entry point. UIKit's event loop is owned by winit through eframe.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod display;
mod files;
#[cfg(any(target_os = "ios", test))]
mod keyboard;
mod mobile;

// Opt-in debug automation for simulator development, using the desktop protocol unchanged.
#[cfg(debug_assertions)]
#[path = "../../effectcraft/src/control_server.rs"]
mod control_server;

// Both frontends use the same safe cpal adapter, without importing desktop windowing or menus.
#[path = "../../effectcraft/src/audio_out.rs"]
mod audio_out;

fn main() -> eframe::Result {
    effectcraft_engine::logging::install();
    effectcraft_engine::logging::install_panic_hook();
    if let Some(dir) = effectcraft_host::config_dir()
        && let Err(e) = effectcraft_engine::logging::attach_session_log(&effectcraft_engine::logging::log_dir(Some(&dir)))
    {
        log::warn!("Cannot write the iOS session log: {e}");
    }
    log::info!("EffectCraft iOS: starting UIKit and Metal");
    let mut graphics = eframe::WgpuConfiguration::default();
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut graphics.wgpu_setup {
        // The simulator advertises fewer varyings than eframe's default device requests.
        // Request the adapter's actual limits, as the desktop frontend does.
        setup.device_descriptor = std::sync::Arc::new(|adapter| eframe::wgpu::DeviceDescriptor {
            label: Some("EffectCraft iOS"),
            required_limits: adapter.limits(),
            ..Default::default()
        });
    }
    let options = eframe::NativeOptions {
        run_and_return: false,
        viewport: egui::ViewportBuilder::default().with_title("EffectCraft"),
        wgpu_options: graphics,
        ..Default::default()
    };
    eframe::run_native("EffectCraft", options, Box::new(|cc| Ok(Box::new(mobile::MobileApp::new(cc)))))
}
