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

        egui::Window::new("Create New Target Profile")
            .collapsible(false)
            .resizable(false)
            .default_size(Vec2::new(360.0, 160.0))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Enter name for new host profile:")
                        .size(metrics::FS_LABEL)
                        .color(colors::TEXT_MUTED),
                );
                ui.add_space(metrics::SPACE_XS);

                ui.add(
                    egui::TextEdit::singleline(&mut self.new_profile_name_input)
                        .hint_text("e.g. server, gaming-pc, media-center")
                        .desired_width(f32::INFINITY),
                );

                ui.add_space(metrics::SPACE_MD);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if flow_button(ui, "Create Profile", colors::OK, colors::BTN_LABEL_ON_LIGHT)
                            .clicked()
                        {
                            let name = self.new_profile_name_input.trim().to_string();
                            if !name.is_empty() {
                                let new_profile = HostProfile::default();
                                self.app_config.set_profile(&name, new_profile.clone());
                                self.app_config.active_profile = name.clone();
                                self.settings_profile_name = name.clone();
                                self.settings_edit_profile = new_profile.clone();
                                self.current_profile = new_profile;
                                self.show_new_profile_dialog = false;
                                self.show_settings = true;

                                let _ = save_config_to_path(&self.app_config, &self.config_path);
                                self.add_toast(Toast::success(format!("Profile '{}' created!", name)));
                            }
                        }

                        ui.add_space(metrics::SPACE_SM);

                        if flow_button(ui, "Cancel", colors::BTN_BG, colors::TEXT_MUTED).clicked() {
                            self.show_new_profile_dialog = false;
                        }
                    });
                });
            });
    }
}
