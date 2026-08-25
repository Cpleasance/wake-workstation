use crate::config::{
    save_config_to_path, validate_broadcast_ip, validate_mac_address, validate_settings,
    validate_target_ip, validate_username,
};
use crate::pipeline::LogLevel;
use crate::ui::theme::{colors, flow_button, metrics};
use crate::ui::toast::Toast;
use crate::ui::WakeApp;
use eframe::egui::{self, Align, Frame, Layout, RichText, Rounding, Stroke, Vec2};

impl WakeApp {
    pub fn save_settings_from_modal(&mut self) -> bool {
        match validate_settings(&self.settings_edit_profile) {
            Ok(()) => {
                let prof_name = if self.settings_profile_name.trim().is_empty() {
                    "thinkstation".to_string()
                } else {
                    self.settings_profile_name.trim().to_string()
                };

                let is_new = !self.app_config.profiles.contains_key(&prof_name);

                self.app_config
                    .set_profile(&prof_name, self.settings_edit_profile.clone());
                self.app_config.active_profile = prof_name.clone();
                self.current_profile = self.settings_edit_profile.clone();

                if let Err(e) = save_config_to_path(&self.app_config, &self.config_path) {
                    self.settings_error = Some(format!("Failed to save config: {}", e));
                    self.add_toast(Toast::error(format!("Failed to save config: {}", e)));
                    false
                } else {
                    self.pipeline
                        .log(LogLevel::Ok, "Configuration saved successfully");
                    if is_new {
                        self.add_toast(Toast::success(format!(
                            "Profile '{}' created and saved!",
                            prof_name
                        )));
                    } else {
                        self.add_toast(Toast::success(format!(
                            "Profile '{}' saved successfully!",
                            prof_name
                        )));
                    }
                    self.show_settings = false;
                    self.settings_error = None;
                    true
                }
            }
            Err(err) => {
                self.settings_error = Some(err.clone());
                self.add_toast(Toast::error(format!("Validation failed: {}", err)));
                false
            }
        }
    }

    pub fn render_settings_modal(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }

        let screen = ctx.screen_rect();
        let modal_w = (screen.width() - 40.0).clamp(420.0, 780.0);
        let modal_h = (screen.height() - 40.0).clamp(360.0, 680.0);
        let scroll_h = (modal_h - 180.0).max(140.0);

        egui::Window::new("Configuration & Target Profiles")
            .collapsible(false)
            .resizable(true)
            .default_size(Vec2::new(modal_w, modal_h))
            .min_size(Vec2::new(360.0, 280.0))
            .max_size(Vec2::new(screen.width() - 16.0, screen.height() - 16.0))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.add_space(2.0);

                // Profile Management Bar
                self.render_profile_management_bar(ui);
                ui.add_space(metrics::SPACE_SM);
                ui.separator();
                ui.add_space(metrics::SPACE_SM);

                if let Some(err) = &self.settings_error {
                    Frame::none()
                        .fill(colors::ERR_BG)
                        .stroke(Stroke::new(1.0_f32, colors::ERR))
                        .rounding(Rounding::same(metrics::ROUNDING_MD))
                        .inner_margin(10.0)
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(format!("Validation Error: {}", err))
                                    .size(metrics::FS_LABEL)
                                    .strong()
                                    .color(colors::ERR),
                            );
                        });
                    ui.add_space(metrics::SPACE_SM);
                }

                // Settings Tabs
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(metrics::SPACE_XS, metrics::SPACE_XS);
                    let tabs = ["Target Host", "Gateway & WOL", "Ports & Timing"];
                    for (idx, name) in tabs.iter().enumerate() {
                        let selected = self.settings_tab == idx;
                        let (fill, text_color) = if selected {
                            (colors::ACCENT_BG, colors::ACCENT)
                        } else {
                            (colors::PANEL_ALT, colors::TEXT_MUTED)
                        };

                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(*name)
                                        .size(metrics::FS_BODY)
                                        .strong()
                                        .color(text_color),
                                )
                                .fill(fill)
                                .rounding(Rounding::same(metrics::ROUNDING_MD)),
                            )
                            .clicked()
                        {
                            self.settings_tab = idx;
                        }
                    }
                });

                ui.add_space(metrics::SPACE_SM);

                egui::ScrollArea::vertical()
                    .max_height(scroll_h)
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.settings_tab {
                        0 => self.render_settings_tab_target(ui),
                        1 => self.render_settings_tab_gateway(ui),
                        2 => self.render_settings_tab_ports(ui),
                        _ => {}
                    });

                ui.add_space(metrics::SPACE_MD);

                // Modal Footer
                let avail_w = ui.available_width();
                if avail_w >= 540.0 {
                    ui.horizontal(|ui| {
                        let exp_btn = flow_button(
                            ui,
                            "📤 Export Config (JSON)",
                            colors::BTN_BG,
                            colors::TEXT,
                        );
                        if exp_btn.clicked() {
                            self.export_config_json(ctx);
                        }
                        exp_btn.on_hover_text(
                            "Export entire configuration to JSON string, file & clipboard",
                        );

                        ui.add_space(metrics::SPACE_XS);

                        let imp_btn = flow_button(
                            ui,
                            "📥 Import Config (JSON)",
                            colors::BTN_BG,
                            colors::TEXT,
                        );
                        if imp_btn.clicked() {
                            self.open_import_config_dialog(ctx);
                        }
                        imp_btn
                            .on_hover_text("Import configuration from raw JSON text or clipboard");

                        ui.add_space(metrics::SPACE_SM);

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let save_btn = flow_button(
                                ui,
                                "Save & Apply  [Ctrl+S]",
                                colors::OK,
                                colors::BTN_LABEL_ON_LIGHT,
                            );
                            if save_btn.clicked() {
                                self.save_settings_from_modal();
                            }
                            save_btn.on_hover_text(
                                "Validate and save current profile settings (Ctrl+S)",
                            );

                            ui.add_space(metrics::SPACE_SM);

                            let cancel_btn =
                                flow_button(ui, "Cancel", colors::BTN_BG, colors::TEXT_MUTED);
                            if cancel_btn.clicked() {
                                self.show_settings = false;
                            }
                            cancel_btn.on_hover_text("Discard edits and close modal (Esc)");
                        });
                    });
                } else {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing =
                            Vec2::new(metrics::SPACE_SM, metrics::SPACE_XS);
                        let save_btn = flow_button(
                            ui,
                            "Save & Apply  [Ctrl+S]",
                            colors::OK,
                            colors::BTN_LABEL_ON_LIGHT,
                        );
                        if save_btn.clicked() {
                            self.save_settings_from_modal();
                        }
                        save_btn
                            .on_hover_text("Validate and save current profile settings (Ctrl+S)");

                        let cancel_btn =
                            flow_button(ui, "Cancel", colors::BTN_BG, colors::TEXT_MUTED);
                        if cancel_btn.clicked() {
                            self.show_settings = false;
                        }
                        cancel_btn.on_hover_text("Discard edits and close modal (Esc)");

                        let exp_btn =
                            flow_button(ui, "📤 Export JSON", colors::BTN_BG, colors::TEXT);
                        if exp_btn.clicked() {
                            self.export_config_json(ctx);
                        }
                        exp_btn.on_hover_text(
                            "Export entire configuration to JSON string, file & clipboard",
                        );

                        let imp_btn =
                            flow_button(ui, "📥 Import JSON", colors::BTN_BG, colors::TEXT);
                        if imp_btn.clicked() {
                            self.open_import_config_dialog(ctx);
                        }
                        imp_btn
                            .on_hover_text("Import configuration from raw JSON text or clipboard");
                    });
                }
            });
    }

    fn render_profile_management_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(metrics::SPACE_SM, metrics::SPACE_XS);
            ui.label(
                RichText::new("Active Profile:")
                    .size(metrics::FS_LABEL)
                    .strong()
                    .color(colors::TEXT_MUTED),
            );

            let current_editing = self.settings_profile_name.clone();
            let mut selected_prof = current_editing.clone();

            egui::ComboBox::from_id_salt("settings_profile_selector")
                .selected_text(
                    RichText::new(&selected_prof)
                        .size(metrics::FS_LABEL)
                        .monospace()
                        .strong()
                        .color(colors::ACCENT),
                )
                .show_ui(ui, |ui| {
                    let mut keys: Vec<String> = self.app_config.profiles.keys().cloned().collect();
                    keys.sort();
                    for name in keys {
                        ui.selectable_value(&mut selected_prof, name.clone(), &name);
                    }
                });

            if selected_prof != current_editing {
                if let Some(prof) = self.app_config.profiles.get(&selected_prof) {
                    self.settings_profile_name = selected_prof;
                    self.settings_edit_profile = prof.clone();
                    self.settings_error = None;
                }
            }

            if ui
                .button(
                    RichText::new("+ New")
                        .size(metrics::FS_LABEL)
                        .color(colors::OK),
                )
                .on_hover_text("Create a new host configuration profile")
                .clicked()
            {
                self.new_profile_name_input =
                    format!("profile-{}", self.app_config.profiles.len() + 1);
                self.show_new_profile_dialog = true;
            }

            if ui
                .button(
                    RichText::new("📋 Duplicate")
                        .size(metrics::FS_LABEL)
                        .color(colors::TEXT),
                )
                .on_hover_text("Duplicate current profile settings into a new profile")
                .clicked()
            {
                match validate_settings(&self.settings_edit_profile) {
                    Ok(()) => {
                        let dup_name = format!("{}-copy", self.settings_profile_name);
                        self.app_config
                            .set_profile(&dup_name, self.settings_edit_profile.clone());
                        self.settings_profile_name = dup_name.clone();
                        self.add_toast(Toast::success(format!(
                            "Profile duplicated as '{}'",
                            dup_name
                        )));
                        let _ = save_config_to_path(&self.app_config, &self.config_path);
                    }
                    Err(err) => {
                        self.settings_error = Some(err.clone());
                        self.add_toast(Toast::error(format!(
                            "Cannot duplicate invalid profile: {}",
                            err
                        )));
                    }
                }
            }

            let can_delete = self.app_config.profiles.len() > 1;
            let del_btn = ui
                .add_enabled(
                    can_delete,
                    egui::Button::new(RichText::new("🗑 Delete").size(metrics::FS_LABEL).color(
                        if can_delete {
                            colors::ERR
                        } else {
                            colors::TEXT_DIM
                        },
                    )),
                )
                .on_hover_text(if can_delete {
                    "Delete this profile permanently"
                } else {
                    "Cannot delete the only remaining profile"
                });

            if del_btn.clicked() {
                let deleted_name = self.settings_profile_name.clone();
                if self.app_config.delete_profile(&deleted_name) {
                    self.settings_profile_name = self.app_config.active_profile.clone();
                    self.settings_edit_profile = self.app_config.get_active_profile();
                    self.current_profile = self.settings_edit_profile.clone();
                    let _ = save_config_to_path(&self.app_config, &self.config_path);
                    self.add_toast(Toast::warning(format!(
                        "Profile '{}' deleted!",
                        deleted_name
                    )));
                }
            }

            if ui
                .button(
                    RichText::new("📤 Export JSON")
                        .size(metrics::FS_LABEL)
                        .color(colors::TEXT),
                )
                .on_hover_text("Export entire configuration to JSON string, file & clipboard")
                .clicked()
            {
                self.export_config_json(ui.ctx());
            }

            if ui
                .button(
                    RichText::new("📥 Import JSON")
                        .size(metrics::FS_LABEL)
                        .color(colors::TEXT),
                )
                .on_hover_text("Import configuration from raw JSON text or clipboard")
                .clicked()
            {
                self.open_import_config_dialog(ui.ctx());
            }
        });
    }

    fn render_settings_tab_target(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("PROFILE & TARGET HOST")
                .size(metrics::FS_LABEL)
                .strong()
                .color(colors::ACCENT),
        );
        ui.add_space(metrics::SPACE_XS);

        let ip_err = validate_target_ip(&self.settings_edit_profile.thinkstation_ip).err();
        let user_err = validate_username(&self.settings_edit_profile.thinkstation_user).err();
        let mac_err = validate_mac_address(&self.settings_edit_profile.mac_address).err();
        let bcast_err = validate_broadcast_ip(&self.settings_edit_profile.broadcast_addr).err();

        egui::Grid::new("settings_grid_target")
            .num_columns(2)
            .spacing([metrics::SPACE_LG, metrics::SPACE_SM])
            .min_col_width(150.0)
            .show(ui, |ui| {
                let profile_name_err = if self.settings_profile_name.trim().is_empty() {
                    Some("Profile name cannot be empty".to_string())
                } else {
                    None
                };
                render_validated_row(
                    ui,
                    "Active Profile Name:",
                    &mut self.settings_profile_name,
                    profile_name_err,
                    "Unique name for this host configuration profile",
                );

                render_validated_row(
                    ui,
                    "Target IP / Host:",
                    &mut self.settings_edit_profile.thinkstation_ip,
                    ip_err,
                    "Target machine LAN IP address or hostname (e.g. 192.168.1.50 or workstation.local)",
                );

                render_validated_row(
                    ui,
                    "Target SSH Username:",
                    &mut self.settings_edit_profile.thinkstation_user,
                    user_err,
                    "SSH username on target machine for state checks & power commands",
                );

                render_validated_row(
                    ui,
                    "MAC Address:",
                    &mut self.settings_edit_profile.mac_address,
                    mac_err,
                    "Target network card MAC address (12 hex digits, e.g. AA:BB:CC:DD:EE:FF)",
                );

                render_validated_row(
                    ui,
                    "Broadcast IP:",
                    &mut self.settings_edit_profile.broadcast_addr,
                    bcast_err,
                    "LAN subnet broadcast address for WOL magic packet (e.g. 192.168.1.255)",
                );
            });

        ui.add_space(metrics::SPACE_MD);
        ui.separator();
        ui.add_space(metrics::SPACE_SM);

        ui.horizontal(|ui| {
            let is_testing = matches!(
                self.reachability_state,
                crate::ui::ReachabilityState::Testing { .. }
            );
            let btn_text = if is_testing {
                "⏳ Testing Reachability..."
            } else {
                "⚡ Test Ping / Check Reachability"
            };

            let test_btn = ui.add_enabled(
                !is_testing,
                egui::Button::new(
                    RichText::new(btn_text)
                        .size(metrics::FS_BODY)
                        .strong()
                        .color(if is_testing {
                            colors::TEXT_DIM
                        } else {
                            colors::ACCENT
                        }),
                )
                .fill(colors::CARD_BG)
                .stroke(Stroke::new(
                    1.0_f32,
                    if is_testing {
                        colors::BORDER
                    } else {
                        colors::ACCENT
                    },
                ))
                .rounding(Rounding::same(metrics::ROUNDING_MD)),
            );

            if test_btn.clicked() {
                self.trigger_reachability_test();
            }
            test_btn.on_hover_text("Send an immediate ICMP ping and test the target SSH port to verify reachability without starting the full wake pipeline");
        });

        self.render_reachability_status_card(ui);
    }

    pub fn trigger_reachability_test(&mut self) {
        let host = self
            .settings_edit_profile
            .thinkstation_ip
            .trim()
            .to_string();
        if host.is_empty() {
            self.add_toast(Toast::warning("Please enter a Target IP / Host to test"));
            return;
        }

        let ssh_port = self.settings_edit_profile.ssh_port;
        let (tx, rx) = std::sync::mpsc::channel();
        self.reachability_rx = Some(rx);
        self.reachability_state = crate::ui::ReachabilityState::Testing {
            host: host.clone(),
            started_at: std::time::Instant::now(),
        };

        let host_for_thread = host;
        std::thread::spawn(move || {
            let (ping_ok, latency) = crate::net::check_ping(&host_for_thread, 2);
            let ssh_ok = if ping_ok && ssh_port > 0 {
                crate::net::check_tcp_port(&host_for_thread, ssh_port, 800)
            } else {
                false
            };
            let _ = tx.send((host_for_thread, ping_ok, latency, ssh_port, ssh_ok));
        });
    }

    fn render_reachability_status_card(&self, ui: &mut egui::Ui) {
        match &self.reachability_state {
            crate::ui::ReachabilityState::Idle => {}
            crate::ui::ReachabilityState::Testing { host, .. } => {
                ui.add_space(metrics::SPACE_SM);
                Frame::none()
                    .fill(colors::CARD_BG)
                    .stroke(Stroke::new(1.0_f32, colors::BORDER_HI))
                    .rounding(Rounding::same(metrics::ROUNDING_MD))
                    .inner_margin(Vec2::new(12.0, 8.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.add_space(metrics::SPACE_XS);
                            ui.label(
                                RichText::new(format!(
                                    "Testing reachability to '{}' (ICMP ping & SSH)...",
                                    host
                                ))
                                .size(metrics::FS_LABEL)
                                .color(colors::TEXT_MUTED),
                            );
                        });
                    });
            }
            crate::ui::ReachabilityState::Result {
                host,
                ping_ok,
                latency_ms,
                ssh_port,
                ssh_ok,
                checked_at,
            } => {
                ui.add_space(metrics::SPACE_SM);
                let elapsed = checked_at.elapsed().as_secs();
                let time_str = if elapsed == 0 {
                    "just now".to_string()
                } else {
                    format!("{}s ago", elapsed)
                };

                let (bg, border, fg, icon, title, detail) = if *ping_ok {
                    let lat_str = crate::net::format_latency(*latency_ms);
                    let ssh_desc = if *ssh_ok {
                        format!("SSH port {} is OPEN and responding.", ssh_port)
                    } else {
                        format!("SSH port {} is CLOSED / not responding.", ssh_port)
                    };
                    (
                        colors::OK_BG,
                        colors::OK,
                        colors::OK,
                        "✔",
                        format!("Host '{}' is online & reachable!", host),
                        format!("ICMP RTT: {}  •  {}", lat_str, ssh_desc),
                    )
                } else {
                    (
                        colors::ERR_BG,
                        colors::ERR,
                        colors::ERR,
                        "✖",
                        format!("Host '{}' is unreachable", host),
                        "No ICMP echo reply received. Machine may be sleeping, offline, or ICMP is disabled.".to_string(),
                    )
                };

                Frame::none()
                    .fill(bg)
                    .stroke(Stroke::new(1.0_f32, border))
                    .rounding(Rounding::same(metrics::ROUNDING_MD))
                    .inner_margin(Vec2::new(12.0, 8.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(icon)
                                    .size(metrics::FS_BODY)
                                    .strong()
                                    .color(fg),
                            );
                            ui.add_space(metrics::SPACE_XS);
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&title)
                                            .size(metrics::FS_LABEL)
                                            .strong()
                                            .color(fg),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.label(
                                            RichText::new(&time_str)
                                                .size(metrics::FS_SMALL)
                                                .color(colors::TEXT_DIM),
                                        );
                                    });
                                });
                                ui.add_space(2.0);
                                ui.label(
                                    RichText::new(&detail)
                                        .size(metrics::FS_SMALL)
                                        .color(colors::TEXT),
                                );
                            });
                        });
                    });
            }
        }
    }

    fn render_settings_tab_gateway(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("GATEWAY & WAKE-ON-LAN")
                .size(metrics::FS_LABEL)
                .strong()
                .color(colors::ACCENT),
        );
        ui.add_space(metrics::SPACE_XS);

        ui.checkbox(
            &mut self.settings_edit_profile.direct_wol,
            " Direct local UDP Wake-on-LAN (bypass Pi gateway)",
        )
        .on_hover_text(
            "Send WOL packet directly via local socket instead of SSH command via Pi gateway",
        );
        ui.add_space(metrics::SPACE_SM);

        let pi_h_err = if !self.settings_edit_profile.pi_host.trim().is_empty() {
            validate_target_ip(&self.settings_edit_profile.pi_host).err()
        } else {
            None
        };

        let pi_u_err = if !self.settings_edit_profile.pi_user.trim().is_empty() {
            validate_username(&self.settings_edit_profile.pi_user).err()
        } else {
            None
        };

        let dwp_err = if self.settings_edit_profile.direct_wol_port == 0 {
            Some("Port must be between 1 and 65535".to_string())
        } else {
            None
        };

        let pi_sp_err = if self.settings_edit_profile.pi_ssh_port == 0 {
            Some("Port must be between 1 and 65535".to_string())
        } else {
            None
        };

        egui::Grid::new("settings_grid_gateway")
            .num_columns(2)
            .spacing([metrics::SPACE_LG, metrics::SPACE_SM])
            .min_col_width(150.0)
            .show(ui, |ui| {
                let mut dwp_s = self.settings_edit_profile.direct_wol_port.to_string();
                render_validated_row(
                    ui,
                    "Direct WOL UDP Port:",
                    &mut dwp_s,
                    dwp_err,
                    "Local UDP port used for direct WOL packets (default: 9)",
                );
                if let Ok(p) = dwp_s.parse::<u16>() {
                    self.settings_edit_profile.direct_wol_port = p;
                }

                render_validated_row(
                    ui,
                    "Pi Gateway Host IP:",
                    &mut self.settings_edit_profile.pi_host,
                    pi_h_err,
                    "Raspberry Pi / SSH gateway LAN IP address (e.g. 192.168.1.10)",
                );

                render_validated_row(
                    ui,
                    "Pi Gateway SSH User:",
                    &mut self.settings_edit_profile.pi_user,
                    pi_u_err,
                    "SSH username on Pi gateway (default: pi)",
                );

                let mut pi_sp_s = self.settings_edit_profile.pi_ssh_port.to_string();
                render_validated_row(
                    ui,
                    "Pi Gateway SSH Port:",
                    &mut pi_sp_s,
                    pi_sp_err,
                    "SSH port on Pi gateway (default: 22)",
                );
                if let Ok(p) = pi_sp_s.parse::<u16>() {
                    self.settings_edit_profile.pi_ssh_port = p;
                }
            });
    }

    fn render_settings_tab_ports(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("PORTS & TIMING")
                .size(metrics::FS_LABEL)
                .strong()
                .color(colors::ACCENT),
        );
        ui.add_space(metrics::SPACE_XS);

        let ssh_p_err = if self.settings_edit_profile.ssh_port == 0 {
            Some("Port must be between 1 and 65535".to_string())
        } else {
            None
        };
        let sun_p_err = if self.settings_edit_profile.sunshine_port == 0 {
            Some("Port must be between 1 and 65535".to_string())
        } else {
            None
        };
        let wait_err = if self.settings_edit_profile.max_wait_seconds == 0 {
            Some("Timeout must be > 0 seconds".to_string())
        } else {
            None
        };
        let resend_err = if self.settings_edit_profile.wol_resend_interval == 0 {
            Some("Interval must be > 0 seconds".to_string())
        } else {
            None
        };

        egui::Grid::new("settings_grid_ports")
            .num_columns(2)
            .spacing([metrics::SPACE_LG, metrics::SPACE_SM])
            .min_col_width(150.0)
            .show(ui, |ui| {
                let mut port_s = self.settings_edit_profile.ssh_port.to_string();
                render_validated_row(
                    ui,
                    "Target SSH Port:",
                    &mut port_s,
                    ssh_p_err,
                    "Target SSH daemon port (default: 22)",
                );
                if let Ok(p) = port_s.parse::<u16>() {
                    self.settings_edit_profile.ssh_port = p;
                }

                let mut sun_s = self.settings_edit_profile.sunshine_port.to_string();
                render_validated_row(
                    ui,
                    "Sunshine Port:",
                    &mut sun_s,
                    sun_p_err,
                    "Sunshine streaming service port (default: 47989)",
                );
                if let Ok(p) = sun_s.parse::<u16>() {
                    self.settings_edit_profile.sunshine_port = p;
                }

                render_validated_row(
                    ui,
                    "Sunshine Service Name:",
                    &mut self.settings_edit_profile.sunshine_service,
                    None,
                    "Systemd user unit name for Sunshine stream server",
                );

                let mut wait_s = self.settings_edit_profile.max_wait_seconds.to_string();
                render_validated_row(
                    ui,
                    "Max Wait Timeout (sec):",
                    &mut wait_s,
                    wait_err,
                    "Maximum seconds to wait for target boot sequence before timing out",
                );
                if let Ok(s) = wait_s.parse::<u64>() {
                    self.settings_edit_profile.max_wait_seconds = s;
                }

                let mut resend_s = self.settings_edit_profile.wol_resend_interval.to_string();
                render_validated_row(
                    ui,
                    "WOL Resend Interval (sec):",
                    &mut resend_s,
                    resend_err,
                    "Seconds between automatic magic packet resends during boot polling",
                );
                if let Ok(s) = resend_s.parse::<u64>() {
                    self.settings_edit_profile.wol_resend_interval = s;
                }
            });

        ui.add_space(metrics::SPACE_SM);
        ui.checkbox(
            &mut self.settings_edit_profile.auto_close,
            " Auto-close window when workstation is Ready",
        )
        .on_hover_text(
            "Automatically close application once Sunshine streaming host is fully operational",
        );
    }
}

fn render_validated_row(
    ui: &mut egui::Ui,
    label: &str,
    text: &mut String,
    err: Option<String>,
    hint_and_tooltip: &str,
) {
    ui.label(
        RichText::new(label)
            .size(metrics::FS_LABEL)
            .color(colors::TEXT_MUTED),
    );

    ui.vertical(|ui| {
        let (stroke, bg) = if err.is_some() {
            (Stroke::new(1.5_f32, colors::ERR), colors::ERR_BG)
        } else {
            (Stroke::new(1.0_f32, colors::BORDER), colors::PANEL_ALT)
        };

        Frame::none()
            .fill(bg)
            .stroke(stroke)
            .rounding(Rounding::same(metrics::ROUNDING_SM))
            .inner_margin(Vec2::new(6.0, 3.0))
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::singleline(text)
                        .hint_text(hint_and_tooltip)
                        .desired_width(f32::INFINITY)
                        .frame(false),
                )
                .on_hover_text(hint_and_tooltip);
            });

        if let Some(err_msg) = err {
            ui.add_space(2.0);
            ui.label(
                RichText::new(format!("⚠ {}", err_msg))
                    .size(metrics::FS_SMALL)
                    .color(colors::ERR),
            );
        }
    });
    ui.end_row();
}
