use crate::pipeline::LogLevel;
use crate::ui::theme::{colors, inset_frame, metrics, panel_frame};
use crate::ui::toast::Toast;
use crate::ui::WakeApp;
use eframe::egui::{self, Align, Frame, Layout, RichText, Rounding, Vec2};

impl WakeApp {
    pub fn render_console(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, height: f32) {
        panel_frame().show(ui, |ui| {
            let start_y = ui.cursor().min.y;
            let total_count = self.pipeline.logs.len();
            let filtered_count = self.pipeline.filtered_logs(
                self.console_level_filter,
                &self.console_search_query,
            ).len();

            let avail_width = ui.available_width();
            const CONSOLE_TOOLBAR_BREAKPOINT: f32 = 680.0;

            if avail_width >= CONSOLE_TOOLBAR_BREAKPOINT {
                // Wide toolbar: Left cluster (Title + Badge + Filter Tabs), Right cluster (Auto-scroll, Copy, Clear, Collapse)
                ui.horizontal(|ui| {
                    let icon = if self.console_collapsed { "▶" } else { "▼" };
                    let toggle_title = ui.button(
                        RichText::new(format!("{} LIVE ACTIVITY LOG", icon))
                            .size(metrics::FS_LABEL)
                            .strong()
                            .color(colors::TEXT_MUTED),
                    )
                    .on_hover_text("Toggle collapsible terminal log panel (Shortcut: Ctrl+T)");

                    if toggle_title.clicked() {
                        self.console_collapsed = !self.console_collapsed;
                    }

                    ui.add_space(metrics::SPACE_XS);

                    let badge_text = if filtered_count == total_count {
                        format!("({})", total_count)
                    } else {
                        format!("({}/{})", filtered_count, total_count)
                    };
                    ui.label(
                        RichText::new(badge_text)
                            .size(metrics::FS_SMALL)
                            .monospace()
                            .color(colors::TEXT_DIM),
                    );

                    if !self.console_collapsed {
                        ui.add_space(metrics::SPACE_SM);
                        self.render_console_filter_tabs(ui);
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let collapse_label = if self.console_collapsed {
                            "Expand [Ctrl+T]"
                        } else {
                            "Collapse [Ctrl+T]"
                        };
                        if ui
                            .button(RichText::new(collapse_label).size(metrics::FS_SMALL).color(colors::ACCENT))
                            .on_hover_text("Toggle activity log visibility (Shortcut: Ctrl+T)")
                            .clicked()
                        {
                            self.console_collapsed = !self.console_collapsed;
                        }

                        if !self.console_collapsed {
                            let clear_btn = ui
                                .button(RichText::new("Clear").size(metrics::FS_LABEL).color(colors::TEXT_DIM))
                                .on_hover_text("Clear all activity logs (Shortcut: Ctrl+L)");
                            if clear_btn.clicked() {
                                self.pipeline.clear_logs();
                                self.add_toast(Toast::info("Activity logs cleared"));
                            }

                            let copy_btn = ui
                                .button(RichText::new("Copy").size(metrics::FS_LABEL).color(colors::TEXT_DIM))
                                .on_hover_text("Copy formatted activity logs to system clipboard");
                            if copy_btn.clicked() {
                                let text = self.pipeline.formatted_logs_text();
                                ctx.output_mut(|o| o.copied_text = text);
                                self.pipeline.log(LogLevel::Info, "Logs copied to system clipboard");
                                self.add_toast(Toast::info("Logs copied to clipboard"));
                            }

                            ui.checkbox(&mut self.console_auto_scroll, "Auto-scroll")
                                .on_hover_text("Automatically scroll console to newest log entries");
                        }
                    });
                });
            } else {
                // Responsive wrapped toolbar for narrower widths
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(metrics::SPACE_SM, metrics::SPACE_XS);

                    let icon = if self.console_collapsed { "▶" } else { "▼" };
                    let toggle_title = ui.button(
                        RichText::new(format!("{} LIVE ACTIVITY LOG", icon))
                            .size(metrics::FS_LABEL)
                            .strong()
                            .color(colors::TEXT_MUTED),
                    )
                    .on_hover_text("Toggle collapsible terminal log panel (Shortcut: Ctrl+T)");

                    if toggle_title.clicked() {
                        self.console_collapsed = !self.console_collapsed;
                    }

                    let badge_text = if filtered_count == total_count {
                        format!("({})", total_count)
                    } else {
                        format!("({}/{})", filtered_count, total_count)
                    };
                    ui.label(
                        RichText::new(badge_text)
                            .size(metrics::FS_SMALL)
                            .monospace()
                            .color(colors::TEXT_DIM),
                    );

                    let collapse_label = if self.console_collapsed {
                        "Expand [Ctrl+T]"
                    } else {
                        "Collapse [Ctrl+T]"
                    };
                    if ui
                        .button(RichText::new(collapse_label).size(metrics::FS_SMALL).color(colors::ACCENT))
                        .on_hover_text("Toggle activity log visibility (Shortcut: Ctrl+T)")
                        .clicked()
                    {
                        self.console_collapsed = !self.console_collapsed;
                    }

                    if !self.console_collapsed {
                        let copy_btn = ui
                            .button(RichText::new("Copy").size(metrics::FS_LABEL).color(colors::TEXT_DIM))
                            .on_hover_text("Copy formatted activity logs to system clipboard");
                        if copy_btn.clicked() {
                            let text = self.pipeline.formatted_logs_text();
                            ctx.output_mut(|o| o.copied_text = text);
                            self.pipeline.log(LogLevel::Info, "Logs copied to system clipboard");
                            self.add_toast(Toast::info("Logs copied to clipboard"));
                        }

                        let clear_btn = ui
                            .button(RichText::new("Clear").size(metrics::FS_LABEL).color(colors::TEXT_DIM))
                            .on_hover_text("Clear all activity logs (Shortcut: Ctrl+L)");
                        if clear_btn.clicked() {
                            self.pipeline.clear_logs();
                            self.add_toast(Toast::info("Activity logs cleared"));
                        }

                        ui.checkbox(&mut self.console_auto_scroll, "Auto-scroll")
                            .on_hover_text("Automatically scroll console to newest log entries");
                    }
                });

                if !self.console_collapsed {
                    ui.add_space(metrics::SPACE_XS);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);
                        self.render_console_filter_tabs(ui);
                    });
                }
            }

            if !self.console_collapsed {
                ui.add_space(metrics::SPACE_XS);

                // Responsive Search input field
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Search:").size(metrics::FS_SMALL).color(colors::TEXT_DIM));
                    let search_w = (ui.available_width() - 30.0).clamp(120.0, 320.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.console_search_query)
                            .hint_text("Filter log output...")
                            .desired_width(search_w),
                    )
                    .on_hover_text("Real-time log filter query");
                });

                ui.add_space(metrics::SPACE_SM);

                let header_consumed = (ui.cursor().min.y - start_y).max(0.0);
                let panel_margin_y = 2.0 * metrics::SPACE_MD;
                let inset_margin_y = 16.0;
                let inner_height = (height - panel_margin_y - header_consumed - inset_margin_y).max(60.0);

                inset_frame().show(ui, |ui| {
                    let mut scroll = egui::ScrollArea::vertical()
                        .max_height(inner_height)
                        .auto_shrink([false, false]);

                    if self.console_auto_scroll {
                        scroll = scroll.stick_to_bottom(true);
                    }

                    scroll.show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        let filtered_entries = self.pipeline.filtered_logs(
                            self.console_level_filter,
                            &self.console_search_query,
                        );
                        for entry in filtered_entries {
                            let (tag_color, tag_bg) = match entry.level {
                                LogLevel::Info => (colors::TEXT_MUTED, colors::CARD_BG),
                                LogLevel::Ok => (colors::OK, colors::OK_BG),
                                LogLevel::Warn => (colors::WARN, colors::WARN_BG),
                                LogLevel::Err => (colors::ERR, colors::ERR_BG),
                                LogLevel::Action => (colors::ACCENT, colors::ACCENT_BG),
                            };

                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = Vec2::new(metrics::SPACE_XS, 1.0);
                                ui.label(
                                    RichText::new(entry.timestamp.format("%H:%M:%S").to_string())
                                        .size(metrics::FS_SMALL)
                                        .monospace()
                                        .color(colors::TEXT_DIM),
                                );

                                Frame::none()
                                    .fill(tag_bg)
                                    .rounding(Rounding::same(metrics::ROUNDING_SM))
                                    .inner_margin(Vec2::new(4.0, 1.0))
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new(entry.level.tag())
                                                .size(metrics::FS_SMALL - 1.0)
                                                .monospace()
                                                .strong()
                                                .color(tag_color),
                                        );
                                    });

                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&entry.message)
                                            .size(metrics::FS_LABEL)
                                            .color(colors::TEXT),
                                    )
                                    .wrap_mode(egui::TextWrapMode::Wrap),
                                );
                            });
                            ui.add_space(2.0);
                        }
                    });
                });
            }
        });
    }

    fn render_console_filter_tabs(&mut self, ui: &mut egui::Ui) {
        let levels = [
            (None, "ALL"),
            (Some(LogLevel::Info), "INFO"),
            (Some(LogLevel::Ok), "OK"),
            (Some(LogLevel::Warn), "WARN"),
            (Some(LogLevel::Err), "ERR"),
            (Some(LogLevel::Action), "ACTION"),
        ];

        for (lvl, label) in levels {
            let selected = self.console_level_filter == lvl;
            let (fill, text_color) = if selected {
                (colors::ACCENT_BG, colors::ACCENT)
            } else {
                (colors::PANEL_ALT, colors::TEXT_MUTED)
            };

            if ui
                .add(
                    egui::Button::new(
                        RichText::new(label)
                            .size(metrics::FS_SMALL)
                            .monospace()
                            .strong()
                            .color(text_color),
                    )
                    .fill(fill)
                    .rounding(Rounding::same(metrics::ROUNDING_SM)),
                )
                .on_hover_text(format!("Filter logs by severity: {}", label))
                .clicked()
            {
                self.console_level_filter = lvl;
            }
            ui.add_space(2.0);
        }
    }
}
