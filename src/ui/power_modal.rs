use crate::ui::theme::{colors, flow_button, metrics};
use crate::ui::toast::Toast;
use crate::ui::WakeApp;
use eframe::egui::{self, Align, Frame, Layout, RichText, Rounding, Stroke, Vec2};

impl WakeApp {
    pub fn render_power_modal(&mut self, ctx: &egui::Context) {
        if !self.show_power_modal {
            return;
        }

        let screen = ctx.screen_rect();
        let modal_h = (screen.height() - 80.0).clamp(280.0, 420.0);
        let modal_w = (screen.width() - 80.0).clamp(340.0, 500.0);

        egui::Window::new("Remote Power Management")
            .collapsible(false)
            .resizable(true)
            .default_size(Vec2::new(modal_w, modal_h))
            .min_size(Vec2::new(320.0, 260.0))
            .max_size(Vec2::new(screen.width() - 20.0, screen.height() - 20.0))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.add_space(2.0);

                ui.label(
                    RichText::new(format!(
                        "Target: {}@{} (via SSH)",
                        self.current_profile.thinkstation_user,
                        self.current_profile.thinkstation_ip
                    ))
                    .size(metrics::FS_LABEL)
                    .monospace()
                    .color(colors::TEXT_MUTED),
                );
                ui.add_space(metrics::SPACE_MD);

                if let Some(status) = &self.power_status {
                    Frame::none()
                        .fill(colors::CARD_BG)
                        .stroke(Stroke::new(1.0_f32, colors::BORDER_HI))
                        .rounding(Rounding::same(metrics::ROUNDING_MD))
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(status)
                                    .size(metrics::FS_LABEL)
                                    .color(colors::ACCENT),
                            );
                        });
                    ui.add_space(metrics::SPACE_SM);
                }

                if let Some((action_code, action_title)) = self.pending_power_action {
                    let (banner_bg, banner_border, banner_fg) = if action_code == "shutdown" {
                        (colors::ERR_BG, colors::ERR, colors::ERR)
                    } else {
                        (colors::WARN_BG, colors::WARN, colors::WARN)
                    };

                    Frame::none()
                        .fill(banner_bg)
                        .stroke(Stroke::new(1.0_f32, banner_border))
                        .rounding(Rounding::same(metrics::ROUNDING_MD))
                        .inner_margin(12.0)
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(format!("Confirm {}", action_title.to_uppercase()))
                                    .size(metrics::FS_LABEL)
                                    .strong()
                                    .color(banner_fg),
                            );
                            ui.add_space(metrics::SPACE_XS);
                            ui.label(
                                RichText::new(format!(
                                    "Are you sure you want to execute '{}' on {}?\nAny active streaming session or unsaved work will be interrupted.",
                                    action_title, self.current_profile.thinkstation_ip
                                ))
                                .size(metrics::FS_LABEL)
                                .color(colors::TEXT),
                            );
                        });

                    ui.add_space(metrics::SPACE_LG);

                    ui.horizontal_wrapped(|ui| {
                        let btn_color = if action_code == "shutdown" {
                            colors::ERR
                        } else {
                            colors::WARN
                        };
                        let action_btn_text = format!("Confirm {}", action_code.to_uppercase());
                        // Both ERR and WARN are bright, saturated fills -- white/near-white
                        // text on them fails WCAG contrast (as low as ~2.1:1 for WARN, ~3.7:1
                        // for ERR). Dark text reads at ~5.2-9:1 on both instead.
                        let confirm_label_color = colors::BTN_LABEL_ON_LIGHT;

                        if flow_button(ui, &action_btn_text, btn_color, confirm_label_color).clicked() {
                            match self
                                .pipeline
                                .trigger_power_action(&self.current_profile, action_code)
                            {
                                Ok(m) => {
                                    self.power_status = Some(m.clone());
                                    self.pending_power_action = None;
                                    self.show_power_modal = false;
                                    self.add_toast(Toast::info(format!(
                                        "Remote power command '{}' executed",
                                        action_code
                                    )));
                                }
                                Err(e) => {
                                    self.power_status = Some(format!("Error: {}", e));
                                    self.add_toast(Toast::error(format!(
                                        "Power action failed: {}",
                                        e
                                    )));
                                    self.pending_power_action = None;
                                }
                            }
                        }

                        ui.add_space(metrics::SPACE_SM);

                        if flow_button(ui, "Back  [Esc]", colors::BTN_BG, colors::TEXT_MUTED).clicked()
                        {
                            self.pending_power_action = None;
                        }
                    });
                } else {
                    ui.label(
                        RichText::new("Select a power action to execute on the remote host:")
                            .size(metrics::FS_LABEL)
                            .color(colors::TEXT_MUTED),
                    );
                    ui.add_space(metrics::SPACE_SM);

                    self.render_action_card(
                        ui,
                        "Suspend / Sleep",
                        "Puts host into low-power sleep mode (systemctl suspend)",
                        "suspend",
                        colors::WARN,
                    );
                    ui.add_space(metrics::SPACE_SM);

                    self.render_action_card(
                        ui,
                        "Reboot Target",
                        "Restarts operating system safely (sudo systemctl reboot)",
                        "reboot",
                        colors::ACCENT,
                    );
                    ui.add_space(metrics::SPACE_SM);

                    self.render_action_card(
                        ui,
                        "Power Off / Shutdown",
                        "Completely powers down machine (sudo systemctl poweroff)",
                        "shutdown",
                        colors::ERR,
                    );

                    ui.add_space(metrics::SPACE_LG);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if flow_button(ui, "Cancel  [Esc]", colors::BTN_BG, colors::TEXT_MUTED).clicked()
                        {
                            self.show_power_modal = false;
                        }
                    });
                }
            });
    }

    fn render_action_card(
        &mut self,
        ui: &mut egui::Ui,
        title: &'static str,
        sub: &str,
        action_code: &'static str,
        accent_color: egui::Color32,
    ) {
        Frame::none()
            .fill(colors::CARD_BG)
            .stroke(Stroke::new(1.0_f32, colors::BORDER))
            .rounding(Rounding::same(metrics::ROUNDING_MD))
            .inner_margin(Vec2::new(12.0, 8.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(title)
                                .size(metrics::FS_LABEL)
                                .strong()
                                .color(colors::TEXT),
                        );
                        ui.add_space(1.0);
                        ui.label(
                            RichText::new(sub)
                                .size(metrics::FS_SMALL)
                                .color(colors::TEXT_DIM),
                        );
                    });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let execute = egui::Button::new(
                            RichText::new("Execute")
                                .size(metrics::FS_LABEL)
                                .color(accent_color),
                        )
                        .fill(colors::PANEL)
                        .stroke(Stroke::new(1.0_f32, accent_color))
                        .rounding(Rounding::same(metrics::ROUNDING_MD));
                        if ui.add(execute).clicked() {
                            self.pending_power_action = Some((action_code, title));
                        }
                    });
                });
            });
    }
}
