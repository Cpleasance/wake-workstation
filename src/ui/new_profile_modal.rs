use crate::config::{save_config_to_path, HostProfile};
use crate::ui::theme::{colors, flow_button, metrics};
use crate::ui::toast::Toast;
use crate::ui::WakeApp;
use eframe::egui::{self, Align, Layout, RichText, Vec2};

impl WakeApp {
    pub fn render_new_profile_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_new_profile_dialog {
            return;
        }

        let mut submit = false;

        let screen = ctx.screen_rect();
        let modal_w = (screen.width() - 40.0).clamp(320.0, 420.0);
        let modal_h = (screen.height() - 40.0).clamp(160.0, 220.0);

        egui::Window::new("Create New Target Profile")
            .collapsible(false)
            .resizable(false)
            .default_size(Vec2::new(modal_w, modal_h))
            .min_size(Vec2::new(300.0, 150.0))
            .max_size(Vec2::new(screen.width() - 16.0, screen.height() - 16.0))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Enter name for new host profile:")
                        .size(metrics::FS_LABEL)
                        .color(colors::TEXT_MUTED),
                );
                ui.add_space(metrics::SPACE_XS);

                let text_edit = egui::TextEdit::singleline(&mut self.new_profile_name_input)
                    .hint_text("e.g. server, gaming-pc, media-center")
                    .desired_width(f32::INFINITY);

                let edit_response = ui.add(text_edit);

                // Auto-request keyboard focus on initial appearance
                if !edit_response.has_focus() && self.new_profile_name_input.starts_with("profile-")
                {
                    edit_response.request_focus();
                }

                // Ergonomic Enter key support: submit if Enter is pressed in text edit or while window is focused
                if edit_response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    submit = true;
                }
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    submit = true;
                }

                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.show_new_profile_dialog = false;
                }

                ui.add_space(metrics::SPACE_MD);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(metrics::SPACE_SM, metrics::SPACE_XS);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let create_btn = flow_button(
                            ui,
                            "Create Profile  [Enter]",
                            colors::OK,
                            colors::BTN_LABEL_ON_LIGHT,
                        );
                        if create_btn.clicked() {
                            submit = true;
                        }
                        create_btn.on_hover_text(
                            "Create and switch to new host profile (Shortcut: Enter)",
                        );

                        ui.add_space(metrics::SPACE_SM);

                        let cancel_btn =
                            flow_button(ui, "Cancel  [Esc]", colors::BTN_BG, colors::TEXT_MUTED);
                        if cancel_btn.clicked() {
                            self.show_new_profile_dialog = false;
                        }
                        cancel_btn.on_hover_text("Discard and close dialog (Shortcut: Esc)");
                    });
                });
            });

        if submit {
            let name = self.new_profile_name_input.trim().to_string();
            if name.is_empty() {
                self.add_toast(Toast::warning("Profile name cannot be empty"));
            } else if let Err(e) = crate::net::validate_host_token(&name, "Profile name") {
                self.add_toast(Toast::error(format!("Invalid profile name: {}", e)));
            } else if self.app_config.profiles.contains_key(&name) {
                self.add_toast(Toast::warning(format!("Profile '{}' already exists", name)));
            } else {
                let new_profile = HostProfile::default();
                self.app_config.set_profile(&name, new_profile.clone());
                self.app_config.active_profile = name.clone();
                self.settings_profile_name = name.clone();
                self.settings_edit_profile = new_profile.clone();
                self.current_profile = new_profile;
                self.pipeline.enter_standby("New profile configured");
                self.show_new_profile_dialog = false;
                self.show_settings = true;

                let _ = save_config_to_path(&self.app_config, &self.config_path);
                self.add_toast(Toast::success(format!("Profile '{}' created!", name)));
            }
        }
    }
}
