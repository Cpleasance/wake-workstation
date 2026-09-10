use crate::config::validate_settings;
use crate::net::format_latency;
use crate::ui::theme::{chip_frame, colors, flow_button, metrics, panel_frame};
use crate::ui::toast::Toast;
use crate::ui::WakeApp;
use eframe::egui::{self, Align, Layout, RichText, Vec2};

impl WakeApp {
    pub fn render_header(&mut self, ui: &mut egui::Ui) {
        panel_frame().show(ui, |ui| {
            const STACK_BELOW_WIDTH: f32 = 680.0;
            let avail_width = ui.available_width();

            if avail_width < STACK_BELOW_WIDTH {
                self.render_header_identity(ui);
                ui.add_space(metrics::SPACE_SM);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(metrics::SPACE_SM, metrics::SPACE_XS);
                    self.render_header_status_cluster(ui, false);
                });
            } else {
                ui.horizontal(|ui| {
                    let right_width = 280.0;
                    let left_width = (avail_width - right_width - metrics::SPACE_SM).max(250.0);

                    ui.allocate_ui_with_layout(
                        Vec2::new(left_width, ui.available_height()),
                        Layout::top_down(Align::Min),
                        |ui| {
                            self.render_header_identity(ui);
                        },
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        self.render_header_status_cluster(ui, true);
                    });
                });
            }
        });
    }

    fn render_header_identity(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(metrics::SPACE_SM, metrics::SPACE_XS);
            ui.label(
                RichText::new("WAKE WORKSTATION")
                    .size(metrics::FS_HEADER)
                    .strong()
                    .color(colors::TEXT),
            );

            let current_selected = self.app_config.active_profile.clone();
            let mut selected_profile = current_selected.clone();
            let mut trigger_create = false;

            egui::ComboBox::from_id_salt("profile_selector")
                .selected_text(
                    RichText::new(format!("PROFILE: {}", selected_profile.to_uppercase()))
                        .size(metrics::FS_LABEL)
                        .monospace()
                        .strong()
                        .color(colors::ACCENT),
                )
                .show_ui(ui, |ui| {
                    let mut keys: Vec<String> = self.app_config.profiles.keys().cloned().collect();
                    keys.sort();
                    for name in keys {
                        ui.selectable_value(&mut selected_profile, name.clone(), &name);
                    }
                    ui.separator();
                    if ui
                        .selectable_label(
                            false,
                            RichText::new("+ Add New Profile...").color(colors::OK),
                        )
                        .clicked()
                    {
                        trigger_create = true;
                    }
                });

            if trigger_create {
                self.new_profile_name_input =
                    format!("profile-{}", self.app_config.profiles.len() + 1);
                self.show_new_profile_dialog = true;
            } else if selected_profile != current_selected {
                // Profiles are loaded verbatim from disk (or hand-edited/imported
                // JSON) and are only guaranteed to have passed `validate_settings`
                // at the moment they were saved/imported via the Settings UI. A
                // profile switch reads directly out of the in-memory map, so
                // re-validate here before it becomes `current_profile` and feeds
                // `pipeline.reset` -- this keeps every path that can trigger a
                // live WOL/SSH action gated on the same validation used for
                // save/import, rather than relying solely on the lower-level
                // defense-in-depth checks inside `net::send_ssh_wol` /
                // `net::execute_power_action`.
                match self.app_config.profiles.get(&selected_profile).cloned() {
                    Some(profile) => match validate_settings(&profile) {
                        Ok(()) => {
                            self.app_config.active_profile = selected_profile.clone();
                            self.current_profile = profile;
                            self.pipeline.reset(&self.current_profile);
                            self.add_toast(Toast::info(format!(
                                "Switched to profile '{}'",
                                selected_profile
                            )));
                        }
                        Err(e) => {
                            self.add_toast(Toast::error(format!(
                                "Cannot switch to profile '{}': {} (fix it in Settings first)",
                                selected_profile, e
                            )));
                        }
                    },
                    None => {
                        self.add_toast(Toast::error(format!(
                            "Profile '{}' not found",
                            selected_profile
                        )));
                    }
                }
            }
        });

        ui.add_space(metrics::SPACE_XS);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(metrics::SPACE_SM, metrics::SPACE_XS);
            ui.label(
                RichText::new(format!(
                    "{}@{}",
                    self.current_profile.thinkstation_user, self.current_profile.thinkstation_ip
                ))
                .size(metrics::FS_LABEL)
                .monospace()
                .color(colors::TEXT_MUTED),
            )
            .on_hover_text("Target machine SSH user and LAN IP");

            ui.label(
                RichText::new(&self.current_profile.mac_address)
                    .size(metrics::FS_LABEL)
                    .monospace()
                    .color(colors::TEXT_MUTED),
            )
            .on_hover_text("Target network card MAC address for Wake-on-LAN");

            let mode_desc = if self.current_profile.direct_wol
                || self.current_profile.pi_host.trim().is_empty()
            {
                "Direct local WOL".to_string()
            } else {
                format!("via gateway {}", self.current_profile.pi_host)
            };
            ui.label(
                RichText::new(mode_desc)
                    .size(metrics::FS_SMALL)
                    .monospace()
                    .color(colors::TEXT_DIM),
            )
            .on_hover_text("Wake-on-LAN dispatch routing method");
        });
    }

    fn render_header_status_cluster(&mut self, ui: &mut egui::Ui, is_right_aligned: bool) {
        let max_w = self.current_profile.max_wait_seconds;
        let elapsed = self.pipeline.elapsed_secs;
        let timer_text = format!(
            "{:02}:{:02} / {:02}:{:02}",
            elapsed / 60,
            elapsed % 60,
            max_w / 60,
            max_w % 60
        );

        let (lat_color, lat_dot, lat_desc) = match self.pipeline.last_latency {
            Some(lat) if lat < 20.0 => (colors::OK, "\u{25cf}", "Excellent response time"),
            Some(lat) if lat < 100.0 => (colors::WARN, "\u{25cf}", "Moderate latency"),
            Some(_) => (colors::ERR, "\u{25cf}", "High response latency"),
            None => (
                colors::TEXT_DIM,
                "\u{25cb}",
                "Host reachability offline or unverified",
            ),
        };

        let render_chip = |ui: &mut egui::Ui| {
            chip_frame().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&timer_text)
                            .size(metrics::FS_LABEL)
                            .monospace()
                            .strong()
                            .color(colors::TEXT_MUTED),
                    )
                    .on_hover_text("Elapsed execution time / Maximum wait timeout");

                    ui.add_space(metrics::SPACE_SM);
                    ui.label(
                        RichText::new(lat_dot)
                            .size(metrics::FS_SMALL)
                            .color(lat_color),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{} RTT",
                            format_latency(self.pipeline.last_latency)
                        ))
                        .size(metrics::FS_LABEL)
                        .monospace()
                        .strong()
                        .color(if self.pipeline.last_latency.is_some() {
                            colors::OK
                        } else {
                            colors::TEXT_DIM
                        }),
                    )
                    .on_hover_text(lat_desc);
                });
            });
        };

        let mut open_settings = false;
        let mut open_shortcuts = false;

        let render_shortcuts_btn = |ui: &mut egui::Ui, open: &mut bool| {
            let btn = flow_button(ui, "?", colors::BTN_BG, colors::TEXT);
            if btn.clicked() {
                *open = true;
            }
            btn.on_hover_text("Keyboard Shortcuts & Help (Shortcut: F1 or ?)");
        };

        let render_settings_btn = |ui: &mut egui::Ui, open: &mut bool| {
            let settings_btn = flow_button(ui, "Settings", colors::BTN_BG, colors::TEXT);
            if settings_btn.clicked() {
                *open = true;
            }
            settings_btn.on_hover_text(
                "Open settings & profile manager (Shortcut: Ctrl+, or click; Ctrl+S to save in settings)",
            );
        };

        if is_right_aligned {
            render_shortcuts_btn(ui, &mut open_shortcuts);
            ui.add_space(metrics::SPACE_XS);
            render_settings_btn(ui, &mut open_settings);
            ui.add_space(metrics::SPACE_SM);
            render_chip(ui);
        } else {
            render_chip(ui);
            ui.add_space(metrics::SPACE_SM);
            render_shortcuts_btn(ui, &mut open_shortcuts);
            ui.add_space(metrics::SPACE_XS);
            render_settings_btn(ui, &mut open_settings);
        }

        if open_shortcuts {
            self.show_shortcuts_modal = true;
        }

        if open_settings {
            self.settings_edit_profile = self.current_profile.clone();
            self.settings_profile_name = self.app_config.active_profile.clone();
            self.settings_error = None;
            self.show_settings = true;
        }
    }
}
