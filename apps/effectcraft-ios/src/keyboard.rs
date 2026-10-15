//! Hardware key transitions supplied by Apple's GameController framework. UIKit keeps text/IME.
use std::collections::BTreeSet;

#[derive(Default)]
struct Keys {
    held: BTreeSet<isize>,
}

impl Keys {
    fn modifiers(&self) -> egui::Modifiers {
        let down = |a, b| self.held.contains(&a) || self.held.contains(&b);
        let command = down(0xe3, 0xe7);
        egui::Modifiers { ctrl: down(0xe0, 0xe4), shift: down(0xe1, 0xe5), alt: down(0xe2, 0xe6), mac_cmd: command, command }
    }

    fn transition(&mut self, code: isize, pressed: bool) -> Option<egui::Event> {
        if !(0xe0..=0xe7).contains(&code) && hid_key(code).is_none() {
            return None;
        }
        if pressed {
            self.held.insert(code);
        } else {
            self.held.remove(&code);
        }
        if (0xe0..=0xe7).contains(&code) {
            return Some(egui::Event::ModifiersChanged(self.modifiers()));
        }
        let key = hid_key(code)?;
        Some(egui::Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers: self.modifiers() })
    }
}

fn hid_key(code: isize) -> Option<egui::Key> {
    use egui::Key::*;
    const LETTERS: &[egui::Key] = &[A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z];
    const NUMBERS: &[egui::Key] = &[Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9, Num0];
    const FUNCTIONS: &[egui::Key] = &[F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12];
    match code {
        4..=29 => LETTERS.get(usize::try_from(code - 4).ok()?).copied(),
        30..=39 => NUMBERS.get(usize::try_from(code - 30).ok()?).copied(),
        58..=69 => FUNCTIONS.get(usize::try_from(code - 58).ok()?).copied(),
        40 | 88 => Some(Enter),
        41 => Some(Escape),
        42 => Some(Backspace),
        43 => Some(Tab),
        44 => Some(Space),
        45 | 86 => Some(Minus),
        46 => Some(Equals),
        47 => Some(OpenBracket),
        48 => Some(CloseBracket),
        49 | 100 => Some(Backslash),
        51 => Some(Semicolon),
        52 => Some(Quote),
        53 => Some(Backtick),
        54 => Some(Comma),
        55 | 99 => Some(Period),
        56 | 84 => Some(Slash),
        74 => Some(Home),
        75 => Some(PageUp),
        76 => Some(Delete),
        77 => Some(End),
        78 => Some(PageDown),
        79 => Some(ArrowRight),
        80 => Some(ArrowLeft),
        81 => Some(ArrowDown),
        82 => Some(ArrowUp),
        87 => Some(Plus),
        89..=98 => NUMBERS.get(usize::try_from(code - 89).ok()?).copied(),
        _ => None,
    }
}

#[cfg(target_os = "ios")]
mod native {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::{Duration, Instant};

    #[derive(Default)]
    struct Queue {
        events: VecDeque<(isize, bool)>,
        overflow: bool,
    }

    #[derive(Default)]
    pub struct Keyboard {
        profile: Option<cidre::arc::R<cidre::gc::KeyboardInput>>,
        queue: Arc<Mutex<Queue>>,
        keys: Keys,
        repeat: Option<(isize, Instant)>,
    }

    impl Keyboard {
        pub fn pump(&mut self, ctx: &egui::Context, input: &mut egui::RawInput, modal: bool) {
            if modal || !input.focused {
                if let Some(mut profile) = self.profile.take() {
                    profile.set_key_changed_handler(None);
                }
                for code in self.keys.held.clone() {
                    if let Some(event) = self.keys.transition(code, false) {
                        input.events.push(event);
                    }
                }
                self.repeat = None;
                self.queue.lock().unwrap_or_else(PoisonError::into_inner).events.clear();
                ctx.request_repaint_after(Duration::from_millis(250));
                return;
            }
            let current = cidre::gc::Keyboard::coalesced().and_then(|keyboard| keyboard.keyboard_input());
            let same = match (&current, &self.profile) {
                (Some(a), Some(b)) => std::ptr::eq(&**a, &**b),
                (None, None) => true,
                _ => false,
            };
            let mut generated = Vec::new();
            if !same {
                if let Some(profile) = &mut self.profile {
                    profile.set_key_changed_handler(None);
                }
                self.profile = current;
                self.repeat = None;
                for code in self.keys.held.clone() {
                    if let Some(event) = self.keys.transition(code, false) {
                        generated.push(event);
                    }
                }
                generated.push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
                if let Some(profile) = &mut self.profile {
                    let queue = self.queue.clone();
                    let repaint = ctx.clone();
                    let mut handler = cidre::gc::KeyboardValueChangedHandler::new4(
                        move |_: &cidre::gc::KeyboardInput, _: &cidre::gc::DeviceButtonInput, code: cidre::gc::KeyCode, pressed: bool| {
                            let mut queue = queue.lock().unwrap_or_else(PoisonError::into_inner);
                            if queue.events.len() >= 512 {
                                queue.events.clear();
                                queue.overflow = true;
                            }
                            queue.events.push_back((code, pressed));
                            repaint.request_repaint();
                        },
                    );
                    profile.set_key_changed_handler(Some(&mut handler));
                    for code in 0xe0..=0xe7 {
                        if profile.button_for_key_code(code).is_some_and(|button| button.is_pressed())
                            && let Some(event) = self.keys.transition(code, true)
                        {
                            generated.push(event);
                        }
                    }
                    log::info!("Hardware keyboard connected; Command shortcuts enabled");
                }
            }
            // Detect keyboards connected/disconnected while the editor is idle; key callbacks wake immediately.
            ctx.request_repaint_after(Duration::from_millis(250));
            let (events, overflow) = {
                let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
                (std::mem::take(&mut queue.events), std::mem::take(&mut queue.overflow))
            };
            if self.profile.is_none() {
                input.events.extend(generated);
                return;
            }
            if !input.focused || overflow {
                self.repeat = None;
                for code in self.keys.held.clone() {
                    if let Some(event) = self.keys.transition(code, false) {
                        generated.push(event);
                    }
                }
                if !input.focused {
                    input.events.extend(generated);
                    return;
                }
            }
            // winit 0.30 produces modifier-less key taps from insertText. Retain UIKit text/IME,
            // but use physical transitions for keys so plain keys don't fire twice.
            input.events.retain(|event| !matches!(event, egui::Event::Key { .. } | egui::Event::ModifiersChanged(_)));
            input.events.extend(generated);
            let mut shortcut_text = false;
            for (code, pressed) in events {
                if pressed && matches!(code, 42 | 74..=82) {
                    self.repeat = Instant::now().checked_add(Duration::from_millis(450)).map(|at| (code, at));
                } else if !pressed && self.repeat.is_some_and(|(held, _)| held == code) {
                    self.repeat = None;
                }
                if pressed && !ctx.egui_wants_keyboard_input() && (code == 85 || code == 37 && self.keys.modifiers().shift) {
                    input.events.push(egui::Event::Text("*".into()));
                }
                if let Some(event) = self.keys.transition(code, pressed) {
                    shortcut_text |= pressed && (self.keys.modifiers().command || self.keys.modifiers().ctrl);
                    if pressed && self.keys.modifiers().command && ctx.egui_wants_keyboard_input() {
                        match hid_key(code) {
                            Some(egui::Key::C) => input.events.push(egui::Event::Copy),
                            Some(egui::Key::X) => input.events.push(egui::Event::Cut),
                            Some(egui::Key::V) => {
                                if let Some(text) = clipboard_text() {
                                    input.events.push(egui::Event::Paste(text));
                                }
                            }
                            _ => {}
                        }
                    }
                    input.events.push(event);
                }
            }
            if let Some((code, at)) = self.repeat {
                let now = Instant::now();
                if now >= at && self.keys.held.contains(&code) {
                    if let Some(key) = hid_key(code) {
                        input.events.push(egui::Event::Key { key, physical_key: Some(key), pressed: true, repeat: true, modifiers: self.keys.modifiers() });
                    }
                    self.repeat = now.checked_add(Duration::from_millis(40)).map(|at| (code, at));
                }
                if let Some((_, deadline)) = self.repeat {
                    ctx.request_repaint_after(deadline.saturating_duration_since(now));
                }
            }
            input.events.push(egui::Event::ModifiersChanged(self.keys.modifiers()));
            if shortcut_text || self.keys.modifiers().command || self.keys.modifiers().ctrl {
                input.events.retain(|event| !matches!(event, egui::Event::Text(_)));
            }
        }

        pub fn copy_output(&self, ctx: &egui::Context) {
            use cidre::{ns, ui::Pasteboard};
            if objc2::MainThreadMarker::new().is_some() {
                for command in ctx.output(|out| out.commands.clone()) {
                    if let egui::OutputCommand::CopyText(text) = command {
                        Pasteboard::general().set_string(Some(&ns::String::with_str(&text)));
                    }
                }
            }
        }
    }

    impl Drop for Keyboard {
        fn drop(&mut self) {
            if let Some(profile) = &mut self.profile {
                profile.set_key_changed_handler(None);
            }
        }
    }

    pub fn clipboard_text() -> Option<String> {
        let _main = objc2::MainThreadMarker::new()?;
        cidre::ui::Pasteboard::general().string().map(|text| text.to_string())
    }
}

#[cfg(target_os = "ios")]
pub use native::{Keyboard, clipboard_text};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn command_is_distinct_from_control_and_survives_key_releases() {
        let mut keys = Keys::default();
        keys.transition(0xe3, true);
        let event = keys.transition(22, true);
        assert!(matches!(event, Some(egui::Event::Key { key: egui::Key::S, .. })));
        let modifiers = match event {
            Some(egui::Event::Key { modifiers, .. }) => modifiers,
            _ => egui::Modifiers::NONE,
        };
        assert!(modifiers.command && modifiers.mac_cmd && !modifiers.ctrl);
        keys.transition(22, false);
        assert!(keys.modifiers().command);
        keys.transition(0xe3, false);
        keys.transition(0xe0, true);
        assert!(keys.modifiers().ctrl && !keys.modifiers().command);
        let (command, _) = effectcraft_ui_egui::menus::parse_shortcut("Cmd+S").unwrap();
        #[cfg(target_os = "ios")]
        assert!(command.mac_cmd && !command.ctrl);
        let _ = command;
    }
    #[test]
    fn invalid_key_codes_are_ignored() {
        let mut keys = Keys::default();
        for code in [isize::MIN, -1, 0, 999, isize::MAX] {
            assert!(hid_key(code).is_none());
            assert!(keys.transition(code, true).is_none());
        }
        assert!(keys.held.is_empty());
    }
}
