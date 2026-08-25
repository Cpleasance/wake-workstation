use crate::pipeline::Stage;
use crate::ui::theme::{colors, metrics, panel_frame};
use crate::ui::WakeApp;
use eframe::egui::{self, Align, Frame, Layout, RichText, Rounding, Stroke, Vec2};

impl WakeApp {
    pub fn render_hero_status(&mut self, ui: &mut egui::Ui) {
        panel_frame().show(ui, |ui| {
            let fraction = self.pipeline.progress_fraction();
            let pct = (fraction * 100.0).round() as u32;

            let status_color = match self.pipeline.stage {
                Stage::Ready => colors::OK,
                Stage::Standby => colors::WARN,
                Stage::Timeout => colors::ERR,
                _ => colors::ACCENT,
            };

            let avail_width = ui.available_width();
            const HERO_STACK_WIDTH: f32 = 480.0;

            if avail_width < HERO_STACK_WIDTH {
                // Stacked responsive layout for narrow viewports
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{}  {}%", self.pipeline.stage.name(), pct))
                            .size(metrics::FS_LABEL)
                            .monospace()
                            .strong()
                            .color(status_color),
                    )
                    .on_hover_text("Pipeline stage execution progress");

                    if let Some(lat) = self.pipeline.last_latency {
                        ui.add_space(metrics::SPACE_SM);
                        ui.label(
                            RichText::new(format!("{:.1} ms", lat))
                                .size(metrics::FS_SMALL)
                                .monospace()
                                .strong()
                                .color(colors::OK),
                        )
                        .on_hover_text("Latest ICMP ping RTT latency");
                    }
                });

                ui.add_space(metrics::SPACE_XS);

                ui.add(
                    egui::Label::new(
                        RichText::new(&self.pipeline.status_message)
                            .size(metrics::FS_BODY)
                            .strong()
                            .color(status_color),
                    )
                    .wrap_mode(egui::TextWrapMode::Wrap),
                )
                .on_hover_text("Current pipeline operation status");
            } else {
                // Single-line responsive layout: status message on left with wrap, stage & latency pinned right
                ui.horizontal(|ui| {
                    let right_width = if self.pipeline.last_latency.is_some() {
                        200.0
                    } else {
                        140.0
                    };
                    let left_width = (ui.available_width() - right_width - metrics::SPACE_SM).max(100.0);

                    ui.allocate_ui_with_layout(
                        Vec2::new(left_width, ui.available_height()),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&self.pipeline.status_message)
                                        .size(metrics::FS_BODY)
                                        .strong()
                                        .color(status_color),
                                )
                                .wrap_mode(egui::TextWrapMode::Wrap),
                            )
                            .on_hover_text("Current pipeline operation status");
                        },
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{}  {}%", self.pipeline.stage.name(), pct))
                                .size(metrics::FS_LABEL)
                                .monospace()
                                .strong()
                                .color(status_color),
                        )
                        .on_hover_text("Pipeline stage execution progress");

                        if let Some(lat) = self.pipeline.last_latency {
                            ui.add_space(metrics::SPACE_SM);
                            ui.label(
                                RichText::new(format!("{:.1} ms", lat))
                                    .size(metrics::FS_SMALL)
                                    .monospace()
                                    .strong()
                                    .color(colors::OK),
                            )
                            .on_hover_text("Latest ICMP ping RTT latency");
                        }
                    });
                });
            }

            // Live mini RTT Latency Sparkline Graph using ui.painter()
            if !self.pipeline.latency_history.is_empty() {
                ui.add_space(metrics::SPACE_XS);
                self.render_latency_sparkline(ui);
            }

            ui.add_space(metrics::SPACE_SM);

            let bar_color = match self.pipeline.stage {
                Stage::Ready => colors::OK,
                Stage::Standby => colors::PENDING,
                Stage::Timeout => colors::ERR,
                _ => colors::ACCENT,
            };

            let avail_width = ui.available_width();
            let height = 6.0;

            track_frame(0.0).show(ui, |ui| {
                    let (rect, _response) = ui.allocate_exact_size(
                        Vec2::new(avail_width, height),
                        egui::Sense::hover(),
                    );
                    if fraction > 0.0 {
                        let mut filled_rect = rect;
                        filled_rect.max.x = rect.min.x + rect.width() * fraction;
                        ui.painter().rect_filled(
                            filled_rect,
                            Rounding::same(metrics::ROUNDING_SM),
                            bar_color,
                        );
                    }
                });
        });
    }

    fn render_latency_sparkline(&self, ui: &mut egui::Ui) {
        let history = &self.pipeline.latency_history;
        if history.is_empty() {
            return;
        }

        let avail_w = ui.available_width();
        let height = 26.0;

        track_frame(Vec2::new(4.0, 3.0)).show(ui, |ui| {
                let (rect, response) = ui.allocate_exact_size(
                    Vec2::new(avail_w - 8.0, height),
                    egui::Sense::hover(),
                );
                let painter = ui.painter_at(rect);

                let n = history.len();
                let max_val = history.iter().copied().fold(10.0_f32, f32::max);
                let min_val = history.iter().copied().fold(f32::INFINITY, f32::min).min(0.0);
                let range = (max_val - min_val).max(1.0);

                let points: Vec<egui::Pos2> = history
                    .iter()
                    .enumerate()
                    .map(|(i, &val)| {
                        let x_ratio = if n > 1 {
                            i as f32 / (n - 1) as f32
                        } else {
                            1.0
                        };
                        let x = rect.min.x + x_ratio * rect.width();
                        let normalized_y = ((val - min_val) / range).clamp(0.0, 1.0);
                        let y = rect.max.y - normalized_y * rect.height();
                        egui::Pos2::new(x, y)
                    })
                    .collect();

                if points.len() >= 2 {
                    let mut fill_points = points.clone();
                    fill_points.push(egui::Pos2::new(rect.max.x, rect.max.y));
                    fill_points.push(egui::Pos2::new(rect.min.x, rect.max.y));
                    painter.add(egui::Shape::convex_polygon(
                        fill_points,
                        egui::Color32::from_rgba_unmultiplied(0x63, 0x66, 0xF1, 35),
                        Stroke::NONE,
                    ));

                    for window in points.windows(2) {
                        painter.line_segment([window[0], window[1]], Stroke::new(1.5_f32, colors::ACCENT));
                    }
                } else if let Some(&pt) = points.first() {
                    painter.circle_filled(pt, 2.0, colors::ACCENT);
                }

                if let Some(&last_pt) = points.last() {
                    painter.circle_filled(last_pt, 2.5, colors::OK);
                }

                let last_lat = history.last().copied().unwrap_or(0.0);
                let avg_lat: f32 = history.iter().sum::<f32>() / n as f32;
                response.on_hover_text(format!(
                    "Live RTT Latency Sparkline Graph\nCurrent: {:.1} ms | Avg: {:.1} ms | Min: {:.1} ms | Max: {:.1} ms ({} samples)",
                    last_lat, avg_lat, min_val, max_val, n
                ));
            });
    }
}

/// The recessed "track" background shared by the progress bar and the
/// latency sparkline -- same fill/border/rounding, only the inner margin
/// differs between the two call sites.
fn track_frame(margin: impl Into<egui::Margin>) -> Frame {
    Frame::none()
        .fill(colors::PANEL_ALT)
        .stroke(Stroke::new(1.0_f32, colors::BORDER))
        .rounding(Rounding::same(metrics::ROUNDING_SM))
        .inner_margin(margin)
}
