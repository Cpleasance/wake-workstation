use crate::pipeline::Stage;
use crate::ui::theme::{colors, metrics, panel_frame};
use crate::ui::WakeApp;
use eframe::egui::{self, Color32, Frame, RichText, Rounding, Stroke, Vec2};

impl WakeApp {
    pub fn render_stepper(&mut self, ui: &mut egui::Ui) {
        let stages = [
            (
                Stage::Wol,
                "1. WOL BROADCAST".to_string(),
                "Magic packet payload".to_string(),
                format!("Broadcast magic packet to MAC {}", self.current_profile.mac_address),
            ),
            (
                Stage::Ping,
                "2. ICMP PING".to_string(),
                "Host reachability check".to_string(),
                format!("ICMP echo request to target IP {}", self.current_profile.thinkstation_ip),
            ),
            (
                Stage::Ssh,
                "3. SSH DAEMON".to_string(),
                format!("Port {} readiness", self.current_profile.ssh_port),
                format!("TCP handshake check on port {}", self.current_profile.ssh_port),
            ),
            (
                Stage::Sunshine,
                "4. SUNSHINE STREAM".to_string(),
                format!("Streaming port {}", self.current_profile.sunshine_port),
                format!("Sunshine service check on port {}", self.current_profile.sunshine_port),
            ),
        ];

        let cur_stage = self.pipeline.stage;
        let stalled_stage = self.pipeline.stalled_stage;
        let style_for = |stage: Stage| -> (Color32, Color32, Color32, &'static str) {
            if cur_stage == Stage::Timeout {
                let stalled = stalled_stage.unwrap_or(Stage::Wol);
                if stage == stalled {
                    (colors::ERR_BG, colors::ERR, colors::ERR, "\u{2717}")
                } else if Self::stage_order(stalled) > Self::stage_order(stage) {
                    (colors::OK_BG, colors::OK, colors::TEXT, "\u{2713}")
                } else {
                    (colors::CARD_BG, colors::BORDER, colors::TEXT_DIM, "\u{25cb}")
                }
            } else if cur_stage == Stage::Standby {
                (colors::CARD_BG, colors::BORDER, colors::TEXT_DIM, "\u{25cb}")
            } else if cur_stage == Stage::Ready {
                (colors::OK_BG, colors::OK, colors::TEXT, "\u{2713}")
            } else if cur_stage == stage {
                (colors::WARN_BG, colors::WARN, colors::TEXT, "\u{25b8}")
            } else if Self::stage_order(cur_stage) > Self::stage_order(stage) {
                (colors::OK_BG, colors::OK, colors::TEXT, "\u{2713}")
            } else {
                (colors::CARD_BG, colors::BORDER, colors::TEXT_DIM, "\u{25cb}")
            }
        };

        let render_card = |ui: &mut egui::Ui,
                           card_width: f32,
                           label: &str,
                           sub: &str,
                           tt: &str,
                           stage: Stage| {
            let (bg, border, fg, badge) = style_for(stage);
            let margin = Vec2::new(8.0, 6.0);
            let inner_w = (card_width - 2.0 * margin.x).max(10.0);
            Frame::none()
                .fill(bg)
                .stroke(Stroke::new(1.0_f32, border))
                .rounding(Rounding::same(metrics::ROUNDING_MD))
                .inner_margin(margin)
                .show(ui, |ui| {
                    ui.set_min_width(inner_w);
                    ui.set_max_width(inner_w);
                    ui.vertical_centered(|ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("{} {}", badge, label))
                                    .size(metrics::FS_LABEL)
                                    .strong()
                                    .color(fg),
                            )
                            .wrap_mode(egui::TextWrapMode::Wrap),
                        );
                        ui.add_space(2.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(sub)
                                    .size(metrics::FS_SMALL)
                                    .color(colors::TEXT_MUTED),
                            )
                            .wrap_mode(egui::TextWrapMode::Wrap),
                        );
                    });
                })
                .response
                .on_hover_text(tt);
        };

        panel_frame().show(ui, |ui| {
            let gap = metrics::SPACE_XS;
            let avail_width = ui.available_width();
            const MIN_COMFORTABLE_CARD: f32 = 130.0;
            let one_row_card_width = (avail_width - 3.0 * gap) / 4.0;

            if one_row_card_width >= MIN_COMFORTABLE_CARD {
                ui.horizontal(|ui| {
                    for (i, (stage, label, sub, tt)) in stages.iter().enumerate() {
                        render_card(ui, one_row_card_width, label, sub, tt, *stage);
                        if i < 3 {
                            ui.add_space(gap);
                        }
                    }
                });
            } else {
                let two_col_card_width = ((avail_width - gap) / 2.0).max(90.0);
                ui.vertical(|ui| {
                    for (row_idx, row) in stages.chunks(2).enumerate() {
                        ui.horizontal(|ui| {
                            for (i, (stage, label, sub, tt)) in row.iter().enumerate() {
                                render_card(ui, two_col_card_width, label, sub, tt, *stage);
                                if i == 0 {
                                    ui.add_space(gap);
                                }
                            }
                        });
                        if row_idx == 0 {
                            ui.add_space(gap);
                        }
                    }
                });
            }
        });
    }

    fn stage_order(stage: Stage) -> u8 {
        match stage {
            Stage::Standby => 0,
            Stage::Wol => 1,
            Stage::Ping => 2,
            Stage::Ssh => 3,
            Stage::Sunshine => 4,
            Stage::Ready => 5,
            Stage::Timeout => 0,
        }
    }
}
