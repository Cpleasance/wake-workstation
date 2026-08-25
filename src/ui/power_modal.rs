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
        let modal_w = (screen.width() - 40.0).clamp(360.0, 540.0);
        let modal_h = (screen.height() - 40.0).clamp(300.0, 480.0);

        egui::Window::new("Remote Power Management")
            .collapsible(false)
            .resizable(true)
            .default_size(Vec2::new(modal_w, modal_h))
            .min_size(Vec2::new(320.0, 240.0))
            .max_size(Vec2::new(screen.width() - 16.0, screen.height() - 16.0))
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
                ui.add_space(metrics::SPACE_SM);

                let footer_reserved = 48.0;
                let avail_scroll_h = (ui.available_height() - footer_reserved).max(100.0);

                egui::ScrollArea::vertical()
                    .max_height(avail_scroll_h)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
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

                            ui.add_space(metrics::SPACE_MD);

                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(metrics::SPACE_SM, metrics::SPACE_XS);
                                let btn_color = if action_code == "shutdown" {
                                    colors::ERR
                                } else {
                                    colors::WARN
                                };
                                let action_btn_text = format!("Confirm {}  [Enter]", action_code.to_uppercase());
                                let confirm_label_color = colors::BTN_LABEL_ON_LIGHT;

                                let confirm_btn = flow_button(ui, &action_btn_text, btn_color, confirm_label_color);
                                if confirm_btn.clicked() {
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
                                confirm_btn.on_hover_text("Execute remote power command immediately (Shortcut: Enter or Space)");

                                let back_btn = flow_button(ui, "Back  [Esc]", colors::BTN_BG, colors::TEXT_MUTED);
                                if back_btn.clicked() {
                                    self.pending_power_action = None;
                                }
                                back_btn.on_hover_text("Return to action selection (Shortcut: Esc)");
                            });
                        } else {
                            ui.label(
                                RichText::new("Select a power action to execute on the remote host (Shortcuts: 1, 2, 3):")
                                    .size(metrics::FS_LABEL)
                                    .color(colors::TEXT_MUTED),
                            );
                            ui.add_space(metrics::SPACE_SM);

                            self.render_action_card(
                                ui,
                                "[1] Suspend / Sleep",
                                "Puts host into low-power sleep mode (systemctl suspend)",
                                "suspend",
                                colors::WARN,
                            );
                            ui.add_space(metrics::SPACE_SM);

                            self.render_action_card(
                                ui,
                                "[2] Reboot Target",
                                "Restarts operating system safely (sudo systemctl reboot)",
                                "reboot",
                                colors::ACCENT,
                            );
                            ui.add_space(metrics::SPACE_SM);

                            self.render_action_card(
                                ui,
                                "[3] Power Off / Shutdown",
                                "Completely powers down machine (sudo systemctl poweroff)",
                                "shutdown",
                                colors::ERR,
                            );
                        }
                    });

                if self.pending_power_action.is_none() {
                    ui.add_space(metrics::SPACE_SM);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let cancel_btn = flow_button(ui, "Cancel  [Esc]", colors::BTN_BG, colors::TEXT_MUTED);
                        if cancel_btn.clicked() {
                            self.show_power_modal = false;
                        }
                        cancel_btn.on_hover_text("Close power management dialog (Shortcut: Esc)");
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
                    let avail_for_text = (ui.available_width() - 90.0).max(120.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(avail_for_text, 0.0),
                        Layout::top_down(Align::Min),
                        |ui| {
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
                        },
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let execute = egui::Button::new(
                            RichText::new("Execute")
                                .size(metrics::FS_LABEL)
                                .color(accent_color),
                        )
                        .fill(colors::PANEL)
                        .stroke(Stroke::new(1.0_f32, accent_color))
                        .rounding(Rounding::same(metrics::ROUNDING_MD));
                        let btn = ui.add(execute);
                        if btn.clicked() {
                            self.pending_power_action = Some((action_code, title));
                        }
                        btn.on_hover_text(format!("Proceed to confirm '{}' action", action_code));
                    });
                });
            });
    }
}
