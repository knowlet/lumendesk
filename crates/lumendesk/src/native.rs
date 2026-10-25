//! GPU-drawn Rust controls. This module has no HTML, JavaScript or webview dependency.
use crate::{DesktopError, FormState, WindowOptions};
pub use eframe;
pub use eframe::egui;
use std::time::{Duration, Instant};

/// Run an arbitrary egui application in an actual desktop window.
pub fn run_native(options: WindowOptions, app: eframe::AppCreator<'_>) -> Result<(), DesktopError> {
    options.validate()?;
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(&options.title)
            .with_app_id(
                std::env::var("LUMENDESK_APP_ID")
                    .unwrap_or_else(|_| "org.lumendesk.reference".into()),
            )
            .with_inner_size([options.width, options.height])
            .with_min_inner_size([160.0, 120.0])
            .with_resizable(options.resizable)
            .with_decorations(options.decorated)
            .with_icon(egui::IconData::default()),
        centered: true,
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(&options.title, native_options, app)
        .map_err(|e| DesktopError::Backend(e.to_string()))
}

/// Semantic control bounds exposed for deterministic headless interaction tests.
#[derive(Default)]
pub struct FormControls {
    pub name: Option<egui::Rect>,
    pub submit: Option<egui::Rect>,
    pub enabled: bool,
}
/// Render the reference form; all state lives in ordinary Rust values.
pub fn form_ui(ui: &mut egui::Ui, state: &mut FormState) -> FormControls {
    let mut controls = FormControls::default();
    ui.heading("A little hello");
    ui.label("Native Rust controls · no webview");
    ui.add_space(18.0);
    ui.horizontal(|ui| {
        let label = ui.label("Your name");
        let input = ui.add(
            egui::TextEdit::singleline(&mut state.name)
                .id_salt("person-name")
                .hint_text("Orion"),
        );
        let input = input.labelled_by(label.id);
        controls.name = Some(input.rect);
    });
    ui.checkbox(&mut state.enthusiastic, "Add a little enthusiasm");
    controls.enabled = !state.name.trim().is_empty();
    let response = ui.add_enabled(controls.enabled, egui::Button::new("Say hello"));
    controls.submit = Some(response.rect);
    if response.clicked() {
        state.submit();
    }
    ui.add_space(12.0);
    if !state.greeting.is_empty() {
        ui.heading(&state.greeting);
    }
    ui.label(format!("Greetings sent: {}", state.submissions));
    egui::ScrollArea::vertical()
        .id_salt("details-scroll")
        .show(ui, |ui| {
            ui.separator();
            ui.label("Try Tab and Space to use the controls with your keyboard.");
            ui.label("The editable field, checkbox, and button use stable identities.");
        });
    controls
}

struct FormApp {
    state: FormState,
    started: Instant,
    smoke_after: Option<Duration>,
    reject_first_close: bool,
    close_attempts: usize,
}
impl eframe::App for FormApp {
    fn logic(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        if context.input(|i| i.viewport().close_requested()) {
            self.close_attempts += 1;
            if self.reject_first_close && self.close_attempts == 1 {
                context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                eprintln!("lumendesk: first close cancelled");
            }
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = ui.ctx().clone();
        egui::CentralPanel::default().show(ui, |ui| {
            form_ui(ui, &mut self.state);
        });
        if let Some(duration) = self.smoke_after {
            if self.started.elapsed() >= duration {
                eprintln!(
                    "lumendesk: native smoke complete; state={}",
                    serde_json::to_string(&self.state).unwrap()
                );
                context.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                context.request_repaint_after(Duration::from_millis(50));
            }
        }
    }
}
/// Launch the original reference form. `LUMENDESK_SMOKE_MS` makes automated smoke runs exit.
pub fn run_form(options: WindowOptions) -> Result<(), DesktopError> {
    let smoke_after = std::env::var("LUMENDESK_SMOKE_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_millis);
    let reject_first_close = std::env::var_os("LUMENDESK_CANCEL_FIRST_CLOSE").is_some();
    run_native(
        options,
        Box::new(move |_cc| {
            Ok(Box::new(FormApp {
                state: FormState::default(),
                started: Instant::now(),
                smoke_after,
                reject_first_close,
                close_attempts: 0,
            }))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn draw(ctx: &egui::Context, state: &mut FormState, events: Vec<egui::Event>) -> FormControls {
        let mut out = FormControls::default();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    out = form_ui(ui, state);
                });
            },
        );
        output.drop_without_applying_deltas();
        out
    }
    fn click(ctx: &egui::Context, state: &mut FormState, pos: egui::Pos2) {
        draw(
            ctx,
            state,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        draw(
            ctx,
            state,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
    }
    fn close_test_app() -> FormApp {
        FormApp {
            state: FormState::default(),
            started: Instant::now(),
            smoke_after: None,
            reject_first_close: true,
            close_attempts: 0,
        }
    }
    fn close_input(minimized: bool) -> egui::RawInput {
        let mut input = egui::RawInput::default();
        let viewport = input.viewports.get_mut(&egui::ViewportId::ROOT).unwrap();
        viewport.minimized = Some(minimized);
        viewport.events.push(egui::ViewportEvent::Close);
        input
    }
    #[test]
    fn native_minimized_logic_cancels_first_close_only() {
        use eframe::App as _;
        let ctx = egui::Context::default();
        let mut app = close_test_app();
        let mut frame = eframe::Frame::_new_kittest();
        let input = close_input(true);
        let first = ctx.run_logic(&input, |ctx| app.logic(ctx, &mut frame));
        assert!(
            first
                .viewport_commands
                .get(&egui::ViewportId::ROOT)
                .is_some_and(|commands| commands.contains(&egui::ViewportCommand::CancelClose))
        );
        assert_eq!(app.close_attempts, 1);
        let mut idle = input.clone();
        idle.viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .events
            .clear();
        let _ = ctx.run_logic(&idle, |ctx| app.logic(ctx, &mut frame));
        assert_eq!(app.close_attempts, 1);
        let second = ctx.run_logic(&input, |ctx| app.logic(ctx, &mut frame));
        assert!(
            !second
                .viewport_commands
                .get(&egui::ViewportId::ROOT)
                .is_some_and(|commands| commands.contains(&egui::ViewportCommand::CancelClose))
        );
        assert_eq!(app.close_attempts, 2);
    }
    #[test]
    fn native_visible_logic_and_ui_count_each_close_once() {
        use eframe::App as _;
        let ctx = egui::Context::default();
        let mut app = close_test_app();
        let mut frame = eframe::Frame::_new_kittest();
        for attempt in 1..=2 {
            let output = ctx.run_ui(close_input(false), |ui| {
                app.logic(ui.ctx(), &mut frame);
                app.ui(ui, &mut frame);
            });
            let cancelled = output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .contains(&egui::ViewportCommand::CancelClose);
            assert_eq!(cancelled, attempt == 1);
            assert_eq!(app.close_attempts, attempt);
            output.drop_without_applying_deltas();
        }
    }
    #[test]
    fn native_view_click_and_disabled_semantics() {
        let ctx = egui::Context::default();
        let mut state = FormState::default();
        let controls = draw(&ctx, &mut state, vec![]);
        assert!(!controls.enabled);
        click(&ctx, &mut state, controls.submit.unwrap().center());
        assert_eq!(state.submissions, 0);
        click(&ctx, &mut state, controls.name.unwrap().center());
        draw(&ctx, &mut state, vec![egui::Event::Text("Orion".into())]);
        assert_eq!(state.name, "Orion");
        let controls = draw(&ctx, &mut state, vec![]);
        click(&ctx, &mut state, controls.submit.unwrap().center());
        assert_eq!(state.greeting, "Hello, Orion.");
        assert_eq!(state.submissions, 1);
    }
    fn press(ctx: &egui::Context, state: &mut FormState, key: egui::Key) {
        for pressed in [true, false] {
            draw(
                ctx,
                state,
                vec![egui::Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed,
                    repeat: false,
                    modifiers: Default::default(),
                }],
            );
        }
    }
    #[test]
    fn native_keyboard_focus_and_activation() {
        let ctx = egui::Context::default();
        let mut state = FormState::default();
        let controls = draw(&ctx, &mut state, vec![]);
        click(&ctx, &mut state, controls.name.unwrap().center());
        draw(&ctx, &mut state, vec![egui::Event::Text("Orion".into())]);
        let name_focus = ctx.memory(|m| m.focused());
        assert!(name_focus.is_some());
        press(&ctx, &mut state, egui::Key::Tab);
        let checkbox_focus = ctx.memory(|m| m.focused());
        assert_ne!(name_focus, checkbox_focus);
        press(&ctx, &mut state, egui::Key::Space);
        assert!(state.enthusiastic);
        press(&ctx, &mut state, egui::Key::Tab);
        assert_ne!(checkbox_focus, ctx.memory(|m| m.focused()));
        press(&ctx, &mut state, egui::Key::Space);
        assert_eq!(state.greeting, "Hello, Orion!");
        assert_eq!(state.submissions, 1);
    }
}
