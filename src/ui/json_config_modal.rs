use crate::config::{export_config_json, import_config_json, save_config_to_path, validate_settings};
use crate::pipeline::LogLevel;
use crate::ui::theme::{colors, flow_button, inset_frame, metrics};
use crate::ui::toast::Toast;
use crate::ui::WakeApp;
use eframe::egui::{self, Align, Layout, RichText, Vec2};

impl WakeApp {
    pub fn export_config_json(&mut self, ctx: &egui::Context) {
        match export_config_json(&self.app_config) {
            Ok(json_str) => {
                ctx.output_mut(|o| o.copied_text = json_str.clone());

                let export_path = self.config_path.with_file_name("config.export.json");
                let file_written = std::fs::write(&export_path, &json_str).is_ok();

                self.json_config_input = json_str;
                self.json_config_is_export = true;
                self.show_json_config_dialog = true;

                if file_written {
                    self.add_toast(Toast::success(format!(
                        "Config exported to clipboard & {}",
                        export_path.file_name().unwrap_or_default().to_string_lossy()
                    )));
                } else {
                    self.add_toast(Toast::success("Configuration JSON copied to clipboard!"));
                }
                self.pipeline.log(LogLevel::Ok, "Configuration JSON exported");
            }
            Err(e) => {
                self.add_toast(Toast::error(format!("Export JSON failed: {}", e)));
            }
        }
    }

    pub fn open_import_config_dialog(&mut self, _ctx: &egui::Context) {
        let initial_text = export_config_json(&self.app_config).unwrap_or_default();
        self.json_config_input = initial_text;
        self.json_config_is_export = false;
        self.show_json_config_dialog = true;
    }

    fn apply_imported_json(&mut self) -> bool {
        let raw = self.json_config_input.trim();
        if raw.is_empty() {
            self.settings_error = Some("Import JSON string is empty".to_string());
            self.add_toast(Toast::error("Import JSON string is empty"));
            return false;
        }

        match import_config_json(raw) {
            Ok(app_cfg) => {
                for (name, profile) in &app_cfg.profiles {
                    if let Err(e) = validate_settings(profile) {
                        let err_msg = format!("Import validation error in profile '{}': {}", name, e);
                        self.settings_error = Some(err_msg.clone());
                        self.add_toast(Toast::error(err_msg));
                        return false;
                    }
                }
                self.app_config = app_cfg;
                self.settings_profile_name = self.app_config.active_profile.clone();
                self.settings_edit_profile = self.app_config.get_active_profile();
                self.current_profile = self.settings_edit_profile.clone();

                if let Err(e) = save_config_to_path(&self.app_config, &self.config_path) {
                    self.add_toast(Toast::error(format!("Failed to save imported config: {}", e)));
                    return false;
                }

                self.pipeline.reset(&self.current_profile);
                self.show_json_config_dialog = false;
                self.settings_error = None;
                self.add_toast(Toast::success("App configuration JSON imported successfully!"));
                self.pipeline.log(LogLevel::Ok, "Configuration imported from JSON");
                true
            }
            Err(e) => {
                self.settings_error = Some(format!("Failed to parse JSON configuration: {}", e));
                self.add_toast(Toast::error(format!("Import failed: {}", e)));
                false
            }
        }
    }

    pub fn render_json_config_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_json_config_dialog {
            return;
        }

        let title = if self.json_config_is_export {
            "Export Configuration (JSON)"
        } else {
            "Import Configuration (JSON)"
        };

        let screen = ctx.screen_rect();
        let modal_w = (screen.width() - 40.0).clamp(420.0, 780.0);
        let modal_h = (screen.height() - 40.0).clamp(300.0, 580.0);

        egui::Window::new(title)
            .collapsible(false)
            .resizable(true)
            .default_size(Vec2::new(modal_w, modal_h))
            .min_size(Vec2::new(360.0, 240.0))
            .max_size(Vec2::new(screen.width() - 16.0, screen.height() - 16.0))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                let info_msg = if self.json_config_is_export {
                    "Configuration JSON copied to clipboard. You can also view or copy it below:"
                } else {
                    "Paste JSON configuration below, then click 'Apply Import':"
                };
                ui.label(
                    RichText::new(info_msg)
                        .size(metrics::FS_LABEL)
                        .color(colors::TEXT_MUTED),
                );
                ui.add_space(metrics::SPACE_SM);

                let footer_reserved = 54.0;
                let avail_editor_h = (ui.available_height() - footer_reserved).max(120.0);

                inset_frame().show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(avail_editor_h)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut self.json_config_input)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(f32::INFINITY)
                                    .code_editor(),
                            );
                        });
                });

                ui.add_space(metrics::SPACE_MD);

                let avail_w = ui.available_width();
                if avail_w >= 440.0 {
                    ui.horizontal(|ui| {
                        let copy_btn = flow_button(ui, "📋 Copy JSON", colors::BTN_BG, colors::TEXT);
                        if copy_btn.clicked() {
                            ctx.output_mut(|o| o.copied_text = self.json_config_input.clone());
                            self.add_toast(Toast::info("Copied JSON to clipboard"));
                        }
                        copy_btn.on_hover_text("Copy configuration JSON string to clipboard");

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if self.json_config_is_export {
                                let done_btn = flow_button(ui, "Done  [Esc]", colors::OK, colors::BTN_LABEL_ON_LIGHT);
                                if done_btn.clicked() {
                                    self.show_json_config_dialog = false;
                                }
                                done_btn.on_hover_text("Close export dialog (Shortcut: Esc)");
                            } else {
                                let mut trigger_import = false;
                                if ui.input(|i| (i.modifiers.command || i.modifiers.ctrl) && i.key_pressed(egui::Key::Enter)) {
                                    trigger_import = true;
                                }

                                let apply_btn = flow_button(
                                    ui,
                                    "Apply Import  [Ctrl+Enter]",
                                    colors::OK,
                                    colors::BTN_LABEL_ON_LIGHT,
                                );
                                if apply_btn.clicked() || trigger_import {
                                    self.apply_imported_json();
                                }
                                apply_btn.on_hover_text("Validate and import JSON configuration (Shortcut: Ctrl+Enter)");

                                ui.add_space(metrics::SPACE_SM);

                                let cancel_btn = flow_button(ui, "Cancel  [Esc]", colors::BTN_BG, colors::TEXT_MUTED);
                                if cancel_btn.clicked() {
                                    self.show_json_config_dialog = false;
                                }
                                cancel_btn.on_hover_text("Discard and close import dialog (Shortcut: Esc)");
                            }
                        });
                    });
                } else {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(metrics::SPACE_SM, metrics::SPACE_XS);
                        if self.json_config_is_export {
                            let done_btn = flow_button(ui, "Done  [Esc]", colors::OK, colors::BTN_LABEL_ON_LIGHT);
                            if done_btn.clicked() {
                                self.show_json_config_dialog = false;
                            }
                            done_btn.on_hover_text("Close export dialog (Shortcut: Esc)");
                        } else {
                            let mut trigger_import = false;
                            if ui.input(|i| (i.modifiers.command || i.modifiers.ctrl) && i.key_pressed(egui::Key::Enter)) {
                                trigger_import = true;
                            }

                            let apply_btn = flow_button(
                                ui,
                                "Apply Import  [Ctrl+Enter]",
                                colors::OK,
                                colors::BTN_LABEL_ON_LIGHT,
                            );
                            if apply_btn.clicked() || trigger_import {
                                self.apply_imported_json();
                            }
                            apply_btn.on_hover_text("Validate and import JSON configuration (Shortcut: Ctrl+Enter)");

                            let cancel_btn = flow_button(ui, "Cancel  [Esc]", colors::BTN_BG, colors::TEXT_MUTED);
                            if cancel_btn.clicked() {
                                self.show_json_config_dialog = false;
                            }
                            cancel_btn.on_hover_text("Discard and close import dialog (Shortcut: Esc)");
                        }

                        let copy_btn = flow_button(ui, "📋 Copy JSON", colors::BTN_BG, colors::TEXT);
                        if copy_btn.clicked() {
                            ctx.output_mut(|o| o.copied_text = self.json_config_input.clone());
                            self.add_toast(Toast::info("Copied JSON to clipboard"));
                        }
                        copy_btn.on_hover_text("Copy configuration JSON string to clipboard");
                    });
                }
            });
    }
}
