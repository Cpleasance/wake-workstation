use crate::ui::theme::{colors, flow_button, metrics};
use crate::ui::WakeApp;
use eframe::egui::{self, Align, Frame, Layout, RichText, Rounding, Stroke, Vec2};

impl WakeApp {
    pub fn render_shortcuts_modal(&mut self, ctx: &egui::Context) {
        if !self.show_shortcuts_modal {
            return;
        }

        let screen = ctx.screen_rect();
        let modal_w = (screen.width() - 40.0).clamp(420.0, 580.0);
        let modal_h = (screen.height() - 40.0).clamp(380.0, 560.0);

        egui::Window::new("Keyboard Shortcuts & Help")
            .collapsible(false)
            .resizable(true)
            .default_size(Vec2::new(modal_w, modal_h))
            .min_size(Vec2::new(360.0, 280.0))
            .max_size(Vec2::new(screen.width() - 16.0, screen.height() - 16.0))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.add_space(metrics::SPACE_XS);

                ui.label(
                    RichText::new("Quick reference for keyboard shortcuts and productivity controls.")
                        .size(metrics::FS_LABEL)
                        .color(colors::TEXT_MUTED),
                );

                ui.add_space(metrics::SPACE_SM);

                let footer_reserved = 48.0;
                let avail_scroll_h = (ui.available_height() - footer_reserved).max(120.0);

                egui::ScrollArea::vertical()
                    .max_height(avail_scroll_h)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let render_section = |ui: &mut egui::Ui, title: &str, items: &[(&[&str], &str, &str)]| {
                            ui.label(
                                RichText::new(title)
                                    .size(metrics::FS_TITLE)
                                    .strong()
                                    .color(colors::TEXT),
                            );
                            ui.add_space(metrics::SPACE_XS);

                            Frame::none()
                                .fill(colors::CARD_BG)
                                .stroke(Stroke::new(1.0_f32, colors::BORDER))
                                .rounding(Rounding::same(metrics::ROUNDING_MD))
                                .inner_margin(10.0)
                                .show(ui, |ui| {
                                    for (i, (keys, action, desc)) in items.iter().enumerate() {
                                        if i > 0 {
                                            ui.add_space(6.0);
                                            ui.separator();
                                            ui.add_space(6.0);
                                        }
                                        ui.horizontal(|ui| {
                                            // Key combo badges
                                            for (k_idx, key) in keys.iter().enumerate() {
                                                if k_idx > 0 {
                                                    ui.label(
                                                        RichText::new("+")
                                                            .size(metrics::FS_LABEL)
                                                            .color(colors::TEXT_DIM),
                                                    );
                                                }
                                                Frame::none()
                                                    .fill(colors::PANEL)
                                                    .stroke(Stroke::new(1.0_f32, colors::BORDER_HI))
                                                    .rounding(Rounding::same(4.0))
                                                    .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                                    .show(ui, |ui| {
                                                        ui.label(
                                                            RichText::new(*key)
                                                                .size(metrics::FS_LABEL)
                                                                .monospace()
                                                                .strong()
                                                                .color(colors::ACCENT),
                                                        );
                                                    });
                                            }

                                            ui.add_space(metrics::SPACE_SM);

                                            ui.vertical(|ui| {
                                                ui.label(
                                                    RichText::new(*action)
                                                        .size(metrics::FS_BODY)
                                                        .strong()
                                                        .color(colors::TEXT),
                                                );
                                                if !desc.is_empty() {
                                                    ui.label(
                                                        RichText::new(*desc)
                                                            .size(metrics::FS_LABEL)
                                                            .color(colors::TEXT_DIM),
                                                    );
                                                }
                                            });
                                        });
                                    }
                                });

                            ui.add_space(metrics::SPACE_MD);
                        };

                        // Section 1: Main Wake & Streaming
                        render_section(
                            ui,
                            "Wake & Streaming Controls",
                            &[
                                (
                                    &["Space"],
                                    "Wake Workstation / Launch Moonlight",
                                    "Broadcasts WOL in Standby or launches Moonlight stream when Ready",
                                ),
                                (
                                    &["F5"],
                                    "Re-send WOL Packet",
                                    "Broadcasts Wake-on-LAN magic packet immediately (also Ctrl+R)",
                                ),
                            ],
                        );

                        // Section 2: Remote Power Actions
                        render_section(
                            ui,
                            "Remote Power Management",
                            &[
                                (
                                    &["Ctrl", "P"],
                                    "Sleep Host (Suspend)",
                                    "Dispatches immediate remote suspend via SSH (requires Ready state)",
                                ),
                                (
                                    &["Ctrl", "Shift", "P"],
                                    "Turn Off Host (Power Off)",
                                    "Shuts down workstation completely via systemctl poweroff",
                                ),
                                (
                                    &["Alt", "P"],
                                    "Power Management Menu",
                                    "Opens dialog for Suspend (1), Reboot (2), or Power Off (3)",
                                ),
                            ],
                        );

                        // Section 3: App Navigation & Settings
                        render_section(
                            ui,
                            "Navigation & Settings",
                            &[
                                (
                                    &["Ctrl", ","],
                                    "Settings & Profile Manager",
                                    "Configure IPs, ports, MAC address, and manage host profiles",
                                ),
                                (
                                    &["Ctrl", "T"],
                                    "Toggle Activity Log",
                                    "Collapse or expand the real-time execution console",
                                ),
                                (
                                    &["Ctrl", "L"],
                                    "Clear Activity Log",
                                    "Clears log entries (also Ctrl+K)",
                                ),
                                (
                                    &["F1"],
                                    "Help & Shortcuts",
                                    "Toggles this keyboard shortcuts reference sheet",
                                ),
                                (
                                    &["Esc"],
                                    "Dismiss / Cancel",
                                    "Closes any active modal dialog or cancels pending action",
                                ),
                            ],
                        );
                    });

                ui.add_space(metrics::SPACE_SM);
                ui.separator();
                ui.add_space(metrics::SPACE_XS);

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let close_btn = flow_button(ui, "Close  [Esc]", colors::BTN_BG, colors::TEXT);
                    if close_btn.clicked() {
                        self.show_shortcuts_modal = false;
                    }
                });
            });
    }
}
