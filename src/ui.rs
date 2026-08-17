use crate::config::{save_config_to_path, validate_settings, AppConfig, HostProfile};
use crate::net::{format_latency, launch_moonlight};
use crate::pipeline::{LogLevel, PipelineState, Stage};
use eframe::egui::{self, Align, Color32, Layout, RichText, Rounding, Stroke, Vec2};
use std::path::PathBuf;
use std::time::Duration;

// --- Design Tokens ---
#[allow(dead_code)]
pub mod colors {
    use eframe::egui::Color32;
    pub const BG: Color32 = Color32::from_rgb(0x0b, 0x0f, 0x14);
    pub const PANEL: Color32 = Color32::from_rgb(0x15, 0x1b, 0x23);
    pub const PANEL_ALT: Color32 = Color32::from_rgb(0x0d, 0x11, 0x17);
    pub const CARD_BG: Color32 = Color32::from_rgb(0x1b, 0x22, 0x2d);
    pub const CARD_HOVER: Color32 = Color32::from_rgb(0x23, 0x2c, 0x3b);
    pub const BORDER: Color32 = Color32::from_rgb(0x25, 0x2f, 0x3e);
    pub const BORDER_HI: Color32 = Color32::from_rgb(0x3b, 0x4a, 0x60);
    pub const TEXT: Color32 = Color32::from_rgb(0xf0, 0xf6, 0xfc);
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x94, 0xa3, 0xb8);
    pub const TEXT_DIM: Color32 = Color32::from_rgb(0x64, 0x74, 0x8b);
    pub const ACCENT: Color32 = Color32::from_rgb(0x38, 0xbd, 0xf8);
    pub const ACCENT_BG: Color32 = Color32::from_rgb(0x0e, 0x2a, 0x38);
    pub const OK: Color32 = Color32::from_rgb(0x10, 0xb9, 0x81);
    pub const OK_BG: Color32 = Color32::from_rgb(0x09, 0x2e, 0x20);
    pub const WARN: Color32 = Color32::from_rgb(0xf5, 0x9e, 0x0b);
    pub const WARN_BG: Color32 = Color32::from_rgb(0x38, 0x20, 0x06);
    pub const ERR: Color32 = Color32::from_rgb(0xef, 0x44, 0x44);
    pub const ERR_BG: Color32 = Color32::from_rgb(0x38, 0x12, 0x14);
    pub const BTN_BG: Color32 = Color32::from_rgb(0x1e, 0x26, 0x33);
    pub const BTN_HOVER: Color32 = Color32::from_rgb(0x2b, 0x36, 0x48);
    pub const PENDING: Color32 = Color32::from_rgb(0x47, 0x55, 0x69);
    // Dark label used on light/bright button fills (OK / ACCENT) for contrast.
    pub const BTN_LABEL_ON_ACCENT: Color32 = Color32::from_rgb(0x0b, 0x0f, 0x17);
}

#[allow(dead_code)]
pub mod metrics {
    // Rounding scale: pills/badges -> buttons/step cards -> outer panels.
    pub const ROUNDING_SM: f32 = 4.0;
    pub const ROUNDING_MD: f32 = 6.0;
    pub const ROUNDING_LG: f32 = 8.0;

    // Spacing scale for section rhythm and inter-element gaps.
    pub const SPACE_XS: f32 = 4.0;
    pub const SPACE_SM: f32 = 8.0;
    pub const SPACE_MD: f32 = 12.0;
    pub const SPACE_LG: f32 = 16.0;

    // A small, fixed type scale. Every label in the app uses one of these
    // four sizes rather than one-off values, so text weight/size reads as a
    // deliberate hierarchy instead of arbitrary per-widget tuning.
    pub const FS_TITLE: f32 = 14.0;
    pub const FS_BODY: f32 = 12.0;
    pub const FS_LABEL: f32 = 11.0;
    pub const FS_SMALL: f32 = 10.0;

    // Layout constants used to keep the window responsive at any size.
    pub const MAX_CONTENT_WIDTH: f32 = 920.0;
    pub const ACTION_BAR_HEIGHT: f32 = 36.0;
    pub const MIN_CONSOLE_HEIGHT: f32 = 120.0;
    pub const OUTER_MARGIN: f32 = 18.0;
}

// All four top-level sections (header, stepper, hero, console) share this
// exact frame; identical fill/stroke/rounding/margin, so one helper covers them.
fn panel_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(colors::PANEL)
        .stroke(Stroke::new(1.0_f32, colors::BORDER))
        .rounding(Rounding::same(metrics::ROUNDING_LG))
        .inner_margin(metrics::SPACE_MD)
}

// Minimal, borderless badge used for compact inline status (timer, latency,
// log-level tags). Kept deliberately free of a border/shadow so it reads as
// a quiet label rather than another nested "card" competing for attention.
fn chip_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(colors::PANEL_ALT)
        .rounding(Rounding::same(metrics::ROUNDING_SM))
        .inner_margin(Vec2::new(8.0, 5.0))
}

/// Add a button whose height is fixed but whose width follows its label, so
/// rows of these buttons reflow naturally instead of relying on hand-picked
/// pixel widths that break at other window sizes.
fn flow_button(ui: &mut egui::Ui, label: &str, fill: Color32, text_color: Color32) -> egui::Response {
    ui.scope(|ui| {
        ui.style_mut().spacing.button_padding = Vec2::new(16.0, 9.0);
        ui.add(
            egui::Button::new(RichText::new(label).size(metrics::FS_BODY).strong().color(text_color))
                .fill(fill)
                .rounding(Rounding::same(metrics::ROUNDING_MD)),
        )
    })
    .inner
}

pub struct WakeApp {
    pub app_config: AppConfig,
    pub current_profile: HostProfile,
    pub config_path: PathBuf,
    pub auto_launch_moonlight: bool,
    pub pipeline: PipelineState,

    // Modal states
    pub show_settings: bool,
    pub show_power_modal: bool,
    pub pending_power_action: Option<(&'static str, &'static str)>,
    pub settings_edit_profile: HostProfile,
    pub settings_profile_name: String,
    pub settings_error: Option<String>,
    pub power_status: Option<String>,
}

impl WakeApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        app_config: AppConfig,
        config_path: PathBuf,
        auto_launch_moonlight: bool,
    ) -> Self {
        Self::apply_theme(&cc.egui_ctx);

        let current_profile = app_config.get_active_profile();
        let settings_edit_profile = current_profile.clone();
        let settings_profile_name = app_config.active_profile.clone();

        let mut pipeline = PipelineState::new();
        pipeline.reset(&current_profile);

        Self {
            app_config,
            current_profile,
            config_path,
            auto_launch_moonlight,
            pipeline,
            show_settings: false,
            show_power_modal: false,
            pending_power_action: None,
            settings_edit_profile,
            settings_profile_name,
            settings_error: None,
            power_status: None,
        }
    }

    pub fn apply_theme(ctx: &egui::Context) {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = colors::BG;
        visuals.window_fill = colors::PANEL;
        visuals.faint_bg_color = colors::PANEL_ALT;
        visuals.extreme_bg_color = colors::PANEL_ALT;

        visuals.widgets.noninteractive.bg_fill = colors::CARD_BG;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, colors::BORDER);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, colors::TEXT);
        visuals.widgets.noninteractive.rounding = Rounding::same(metrics::ROUNDING_MD);

        visuals.widgets.inactive.bg_fill = colors::BTN_BG;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, colors::BORDER);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, colors::TEXT);
        visuals.widgets.inactive.rounding = Rounding::same(metrics::ROUNDING_MD);

        visuals.widgets.hovered.bg_fill = colors::BTN_HOVER;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, colors::BORDER_HI);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, colors::TEXT);
        visuals.widgets.hovered.rounding = Rounding::same(metrics::ROUNDING_MD);

        visuals.widgets.active.bg_fill = colors::PANEL;
        visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, colors::ACCENT);
        visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, colors::TEXT);
        visuals.widgets.active.rounding = Rounding::same(metrics::ROUNDING_MD);

        visuals.selection.bg_fill = colors::ACCENT_BG;
        visuals.selection.stroke = Stroke::new(1.0_f32, colors::ACCENT);

        ctx.set_visuals(visuals);
    }
}

impl eframe::App for WakeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Advance pipeline tick
        if self.pipeline.stage != Stage::Standby && !self.pipeline.is_finished {
            self.pipeline.tick(&self.current_profile, self.auto_launch_moonlight);
            // Request frequent repaint while actively waiting
            ctx.request_repaint_after(Duration::from_millis(400));
        } else {
            // Slower polling when idle / ready / standby
            ctx.request_repaint_after(Duration::from_millis(1500));
        }

        // Auto close if requested and ready
        if self.pipeline.stage == Stage::Ready && self.current_profile.auto_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(colors::BG).inner_margin(metrics::OUTER_MARGIN))
            .show(ctx, |ui| {
                // Cap content width and center it so the layout doesn't turn
                // into a sparse strip of controls on a maximized/ultrawide
                // window; on narrow windows content_width just tracks
                // available width, so nothing is ever clipped.
                let avail_width = ui.available_width();
                let content_width = avail_width.min(metrics::MAX_CONTENT_WIDTH);
                let side_pad = ((avail_width - content_width) / 2.0).max(0.0);
                let avail_height = ui.available_height();

                ui.horizontal(|ui| {
                    if side_pad > 0.5 {
                        ui.add_space(side_pad);
                    }
                    // A hard-bounded child Ui: unlike `set_width`, this
                    // actually clamps the rect nested Frames/panels measure
                    // `available_width()` against, so content can never
                    // stretch past `content_width` and spill past the
                    // window edge on a maximized/ultrawide window.
                    ui.allocate_ui_with_layout(
                        Vec2::new(content_width, avail_height),
                        Layout::top_down(Align::Min),
                        |ui| {
                            self.render_header(ui);
                            ui.add_space(metrics::SPACE_MD);

                            self.render_stepper(ui);
                            ui.add_space(metrics::SPACE_MD);

                            self.render_hero_status(ui);
                            ui.add_space(metrics::SPACE_MD);

                            // The console is the one section that should
                            // absorb any extra vertical space when the
                            // window is tall (maximized/fullscreen), so the
                            // action bar stays pinned to the bottom instead
                            // of floating with a dead gap underneath it.
                            let reserved_below = metrics::ACTION_BAR_HEIGHT + metrics::SPACE_MD;
                            let console_height =
                                (ui.available_height() - reserved_below).max(metrics::MIN_CONSOLE_HEIGHT);
                            self.render_console(ui, ctx, console_height);
                            ui.add_space(metrics::SPACE_MD);

                            self.render_action_bar(ui, ctx);
                        },
                    );
                });
            });

        self.render_settings_modal(ctx);
        self.render_power_modal(ctx);
    }
}

impl WakeApp {
    fn render_header_identity(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("ThinkStation Remote")
                    .size(metrics::FS_TITLE)
                    .strong()
                    .color(colors::TEXT),
            );

            // Profile Picker ComboBox
            let current_selected = self.app_config.active_profile.clone();
            let mut selected_profile = current_selected.clone();

            egui::ComboBox::from_id_salt("profile_selector")
                .selected_text(
                    RichText::new(selected_profile.to_uppercase())
                        .size(metrics::FS_LABEL)
                        .monospace()
                        .strong()
                        .color(colors::ACCENT),
                )
                .show_ui(ui, |ui| {
                    for name in self.app_config.profiles.keys() {
                        ui.selectable_value(&mut selected_profile, name.clone(), name);
                    }
                });

            if selected_profile != current_selected {
                self.app_config.active_profile = selected_profile;
                self.current_profile = self.app_config.get_active_profile();
                self.pipeline.reset(&self.current_profile);
            }
        });

        ui.add_space(metrics::SPACE_XS);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = metrics::SPACE_SM;
            ui.label(
                RichText::new(format!(
                    "{}@{}",
                    self.current_profile.thinkstation_user,
                    self.current_profile.thinkstation_ip
                ))
                .size(metrics::FS_LABEL)
                .monospace()
                .color(colors::TEXT_MUTED),
            );
            ui.label(
                RichText::new(&self.current_profile.mac_address)
                    .size(metrics::FS_LABEL)
                    .monospace()
                    .color(colors::TEXT_MUTED),
            );

            let mode_desc = if self.current_profile.direct_wol || self.current_profile.pi_host.trim().is_empty() {
                "Direct local WOL".to_string()
            } else {
                format!("via gateway {}", self.current_profile.pi_host)
            };
            ui.label(
                RichText::new(mode_desc)
                    .size(metrics::FS_SMALL)
                    .monospace()
                    .color(colors::TEXT_DIM),
            );
        });
    }

    fn render_header_status_cluster(&mut self, ui: &mut egui::Ui) {
        if flow_button(ui, "Settings", colors::BTN_BG, colors::TEXT).clicked() {
            self.settings_edit_profile = self.current_profile.clone();
            self.settings_profile_name = self.app_config.active_profile.clone();
            self.settings_error = None;
            self.show_settings = true;
        }

        ui.add_space(metrics::SPACE_SM);

        let max_w = self.current_profile.max_wait_seconds;
        let elapsed = self.pipeline.elapsed_secs;
        let timer_text =
            format!("{:02}:{:02} / {:02}:{:02}", elapsed / 60, elapsed % 60, max_w / 60, max_w % 60);

        let (lat_color, lat_dot) = match self.pipeline.last_latency {
            Some(lat) if lat < 20.0 => (colors::OK, "\u{25cf}"),
            Some(lat) if lat < 100.0 => (colors::WARN, "\u{25cf}"),
            Some(_) => (colors::ERR, "\u{25cf}"),
            None => (colors::TEXT_DIM, "\u{25cb}"),
        };

        chip_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(timer_text)
                        .size(metrics::FS_LABEL)
                        .monospace()
                        .strong()
                        .color(colors::TEXT_MUTED),
                );
                ui.add_space(metrics::SPACE_SM);
                ui.label(RichText::new(lat_dot).size(metrics::FS_SMALL).color(lat_color));
                ui.label(
                    RichText::new(format!("{} rtt", format_latency(self.pipeline.last_latency)))
                        .size(metrics::FS_LABEL)
                        .monospace()
                        .strong()
                        .color(if self.pipeline.last_latency.is_some() { colors::OK } else { colors::TEXT_DIM }),
                );
            });
        });
    }

    fn render_header(&mut self, ui: &mut egui::Ui) {
        panel_frame()
            .show(ui, |ui| {
                // Below this width the identity block and the Settings/status
                // cluster can't both fit on one row without crowding or
                // overlapping, so stack them instead of forcing a squeeze.
                const STACK_BELOW_WIDTH: f32 = 620.0;
                let avail_width = ui.available_width();

                if avail_width < STACK_BELOW_WIDTH {
                    self.render_header_identity(ui);
                    ui.add_space(metrics::SPACE_SM);
                    ui.horizontal(|ui| {
                        self.render_header_status_cluster(ui);
                    });
                } else {
                    // Plain horizontal + a right-to-left nested layout for the
                    // right cluster: `with_layout(right_to_left)` anchors to
                    // the ui's true right edge, so (unlike an explicit
                    // hard-bounded sub-rect sized ahead of time) it always
                    // fills the full panel width correctly regardless of how
                    // much space its own content actually needs.
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            self.render_header_identity(ui);
                        });

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            self.render_header_status_cluster(ui);
                        });
                    });
                }
            });
    }

    fn render_stepper(&mut self, ui: &mut egui::Ui) {
        let stages = [
            (Stage::Wol, "1 WOL", "Magic packet"),
            (Stage::Ping, "2 PING", "ICMP network"),
            (Stage::Ssh, "3 SSH", "Daemon :22"),
            (Stage::Sunshine, "4 STREAM", "Sunshine :47989"),
        ];

        let cur_stage = self.pipeline.stage;
        let style_for = |stage: Stage| -> (Color32, Color32, Color32, &'static str) {
            if cur_stage == Stage::Standby {
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

        let render_card = |ui: &mut egui::Ui, card_width: f32, label: &str, sub: &str, stage: Stage| {
            let (bg, border, fg, badge) = style_for(stage);
            egui::Frame::none()
                .fill(bg)
                .stroke(Stroke::new(1.0_f32, border))
                .rounding(Rounding::same(metrics::ROUNDING_MD))
                .inner_margin(Vec2::new(10.0, 8.0))
                .show(ui, |ui| {
                    ui.set_min_width(card_width);
                    ui.set_max_width(card_width);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new(format!("{} {}", badge, label)).size(metrics::FS_LABEL).strong().color(fg));
                        ui.add_space(2.0);
                        ui.label(RichText::new(sub).size(metrics::FS_SMALL).color(colors::TEXT_MUTED));
                    });
                });
        };

        panel_frame()
            .show(ui, |ui| {
                let gap = metrics::SPACE_XS;
                let avail_width = ui.available_width();
                // A card needs roughly this much width for its label/sub text
                // not to wrap awkwardly; below it, switch from one row of 4
                // to a 2x2 grid instead of forcing cards past their floor.
                const MIN_COMFORTABLE_CARD: f32 = 130.0;
                let one_row_card_width = (avail_width - 3.0 * gap) / 4.0;

                if one_row_card_width >= MIN_COMFORTABLE_CARD {
                    ui.horizontal(|ui| {
                        for (i, (stage, label, sub)) in stages.iter().enumerate() {
                            render_card(ui, one_row_card_width, label, sub, *stage);
                            if i < 3 {
                                ui.add_space(gap);
                            }
                        }
                    });
                } else {
                    let two_col_card_width = ((avail_width - gap) / 2.0).max(90.0);
                    ui.vertical(|ui| {
                        for row in stages.chunks(2) {
                            ui.horizontal(|ui| {
                                for (i, (stage, label, sub)) in row.iter().enumerate() {
                                    render_card(ui, two_col_card_width, label, sub, *stage);
                                    if i == 0 {
                                        ui.add_space(gap);
                                    }
                                }
                            });
                            ui.add_space(gap);
                        }
                    });
                }
            });
    }

    fn render_hero_status(&mut self, ui: &mut egui::Ui) {
        panel_frame()
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let status_color = match self.pipeline.stage {
                        Stage::Ready => colors::OK,
                        Stage::Standby => colors::WARN,
                        Stage::Timeout => colors::ERR,
                        _ => colors::TEXT,
                    };

                    ui.label(
                        RichText::new(&self.pipeline.status_message)
                            .size(metrics::FS_BODY)
                            .strong()
                            .color(status_color),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(self.pipeline.stage.name())
                                .size(metrics::FS_LABEL)
                                .monospace()
                                .strong()
                                .color(colors::TEXT_DIM),
                        );
                    });
                });

                ui.add_space(metrics::SPACE_SM);

                // Progress Bar Track
                let fraction = match self.pipeline.stage {
                    Stage::Ready => 1.0,
                    Stage::Standby => 0.0,
                    Stage::Timeout => 1.0,
                    _ => {
                        let max_w = self.current_profile.max_wait_seconds.max(1) as f32;
                        (self.pipeline.elapsed_secs as f32 / max_w).clamp(0.0, 1.0)
                    }
                };

                let bar_color = match self.pipeline.stage {
                    Stage::Ready => colors::OK,
                    Stage::Standby => colors::PENDING,
                    Stage::Timeout => colors::ERR,
                    _ => colors::ACCENT,
                };

                let avail_width = ui.available_width();
                let height = 6.0;

                egui::Frame::none()
                    .fill(colors::PANEL_ALT)
                    .stroke(Stroke::new(1.0_f32, colors::BORDER))
                    .rounding(Rounding::same(metrics::ROUNDING_SM))
                    .inner_margin(0.0)
                    .show(ui, |ui| {
                        let (rect, _response) = ui.allocate_exact_size(Vec2::new(avail_width, height), egui::Sense::hover());
                        if fraction > 0.0 {
                            let mut filled_rect = rect;
                            filled_rect.max.x = rect.min.x + rect.width() * fraction;
                            ui.painter().rect_filled(filled_rect, Rounding::same(metrics::ROUNDING_SM), bar_color);
                        }
                    });
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

    fn render_console(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, height: f32) {
        panel_frame()
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("LIVE ACTIVITY LOG")
                            .size(metrics::FS_LABEL)
                            .strong()
                            .color(colors::TEXT_MUTED),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .button(RichText::new("Clear").size(metrics::FS_LABEL).color(colors::TEXT_DIM))
                            .clicked()
                        {
                            self.pipeline.clear_logs();
                        }

                        if ui
                            .button(RichText::new("Copy").size(metrics::FS_LABEL).color(colors::TEXT_DIM))
                            .clicked()
                        {
                            let text = self.pipeline.formatted_logs_text();
                            ctx.output_mut(|o| o.copied_text = text);
                            self.pipeline.log(LogLevel::Info, "Logs copied to system clipboard");
                        }
                    });
                });

                ui.add_space(metrics::SPACE_SM);

                // Scrollable Console Log Area. auto_shrink is disabled so
                // this box always fills the height its caller computed from
                // the window's actual remaining space, instead of shrinking
                // to content and leaving a dead gap above the action bar.
                let inner_height = (height - 2.0 * metrics::SPACE_MD - 24.0).max(60.0);
                egui::Frame::none()
                    .fill(colors::PANEL_ALT)
                    .stroke(Stroke::new(1.0_f32, colors::BORDER))
                    .rounding(Rounding::same(metrics::ROUNDING_MD))
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(inner_height)
                            .auto_shrink([false, false])
                            .stick_to_bottom(true)
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                for entry in &self.pipeline.logs {
                                    let (tag_color, tag_bg) = match entry.level {
                                        LogLevel::Info => (colors::TEXT_MUTED, colors::CARD_BG),
                                        LogLevel::Ok => (colors::OK, colors::OK_BG),
                                        LogLevel::Warn => (colors::WARN, colors::WARN_BG),
                                        LogLevel::Err => (colors::ERR, colors::ERR_BG),
                                        LogLevel::Action => (colors::ACCENT, colors::ACCENT_BG),
                                    };

                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(entry.timestamp.format("%H:%M:%S").to_string())
                                                .size(metrics::FS_SMALL)
                                                .monospace()
                                                .color(colors::TEXT_DIM),
                                        );

                                        egui::Frame::none()
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

                                        ui.label(
                                            RichText::new(&entry.message)
                                                .size(metrics::FS_LABEL)
                                                .color(colors::TEXT),
                                        );
                                    });
                                    ui.add_space(2.0);
                                }
                            });
                    });
            });
    }

    fn render_action_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            let is_standby = self.pipeline.stage == Stage::Standby;
            let is_ready = self.pipeline.stage == Stage::Ready;

            // Primary Action: Connect / Launch Moonlight / Wake
            let (btn_text, btn_color) = if is_standby {
                ("Wake Workstation", colors::OK)
            } else if is_ready {
                ("Launch Moonlight", colors::OK)
            } else {
                ("Connect Now", colors::ACCENT)
            };

            if flow_button(ui, btn_text, btn_color, colors::BTN_LABEL_ON_ACCENT).clicked() {
                if is_standby {
                    self.pipeline.reset(&self.current_profile);
                } else {
                    self.pipeline.log(LogLevel::Action, "Manual Moonlight launch requested");
                    if launch_moonlight() {
                        self.pipeline.log(LogLevel::Ok, "Moonlight streaming client opened");
                    } else {
                        self.pipeline.log(LogLevel::Warn, "Could not find Moonlight executable in standard paths/Flatpak/Snap");
                    }
                }
            }

            ui.add_space(metrics::SPACE_SM);

            if flow_button(ui, "Re-send WOL", colors::BTN_BG, colors::TEXT).clicked() {
                self.pipeline.send_wol(&self.current_profile);
            }

            ui.add_space(metrics::SPACE_SM);

            if flow_button(ui, "Power", colors::BTN_BG, colors::TEXT).clicked() {
                self.power_status = None;
                self.pending_power_action = None;
                self.show_power_modal = true;
            }

            // Close button right aligned; wraps onto its own line rather
            // than overlapping the other buttons if the window is narrow.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if flow_button(ui, "Close", colors::BTN_BG, colors::TEXT_MUTED).clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });
    }

    fn render_settings_modal(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }

        // Bound the modal to the actual viewport so it can never render
        // partially off-window at the app's minimum size; the internal
        // ScrollArea absorbs whatever doesn't fit.
        let screen = ctx.screen_rect();
        let modal_h = (screen.height() - 80.0).clamp(320.0, 620.0);
        let modal_w = (screen.width() - 80.0).clamp(360.0, 560.0);
        let scroll_h = (modal_h - 140.0).max(180.0);

        egui::Window::new("Configuration & Target Profiles")
            .collapsible(false)
            .resizable(true)
            .default_size(Vec2::new(modal_w, modal_h))
            .min_size(Vec2::new(360.0, 300.0))
            .max_size(Vec2::new(screen.width() - 20.0, screen.height() - 20.0))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.add_space(2.0);

                if let Some(err) = &self.settings_error {
                    egui::Frame::none()
                        .fill(colors::ERR_BG)
                        .stroke(Stroke::new(1.0_f32, colors::ERR))
                        .rounding(Rounding::same(metrics::ROUNDING_MD))
                        .inner_margin(10.0)
                        .show(ui, |ui| {
                            ui.label(RichText::new(format!("Validation error: {}", err)).size(metrics::FS_LABEL).strong().color(colors::ERR));
                        });
                    ui.add_space(metrics::SPACE_SM);
                }

                egui::ScrollArea::vertical().max_height(scroll_h).show(ui, |ui| {
                    // Section 1: Profile & Target Host
                    ui.label(RichText::new("PROFILE & TARGET HOST").size(metrics::FS_LABEL).strong().color(colors::ACCENT));
                    ui.add_space(metrics::SPACE_XS);

                    egui::Grid::new("settings_grid_target")
                        .num_columns(2)
                        .spacing([metrics::SPACE_LG, metrics::SPACE_SM])
                        .min_col_width(140.0)
                        .show(ui, |ui| {
                            ui.label(RichText::new("Active Profile Name:").size(metrics::FS_LABEL).color(colors::TEXT_MUTED));
                            ui.add(egui::TextEdit::singleline(&mut self.settings_profile_name).desired_width(f32::INFINITY));
                            ui.end_row();

                            ui.label(RichText::new("Target IP / Host:").size(metrics::FS_LABEL).color(colors::TEXT_MUTED));
                            ui.add(egui::TextEdit::singleline(&mut self.settings_edit_profile.thinkstation_ip).desired_width(f32::INFINITY));
                            ui.end_row();

                            ui.label(RichText::new("Target SSH Username:").size(metrics::FS_LABEL).color(colors::TEXT_MUTED));
                            ui.add(egui::TextEdit::singleline(&mut self.settings_edit_profile.thinkstation_user).desired_width(f32::INFINITY));
                            ui.end_row();

                            ui.label(RichText::new("MAC Address:").size(metrics::FS_LABEL).color(colors::TEXT_MUTED));
                            ui.add(egui::TextEdit::singleline(&mut self.settings_edit_profile.mac_address).desired_width(f32::INFINITY));
                            ui.end_row();

                            ui.label(RichText::new("Broadcast IP:").size(metrics::FS_LABEL).color(colors::TEXT_MUTED));
                            ui.add(egui::TextEdit::singleline(&mut self.settings_edit_profile.broadcast_addr).desired_width(f32::INFINITY));
                            ui.end_row();
                        });

                    ui.add_space(metrics::SPACE_MD);
                    ui.separator();
                    ui.add_space(metrics::SPACE_SM);

                    // Section 2: Gateway & Wake-on-LAN
                    ui.label(RichText::new("GATEWAY & WAKE-ON-LAN").size(metrics::FS_LABEL).strong().color(colors::ACCENT));
                    ui.add_space(metrics::SPACE_XS);

                    ui.checkbox(&mut self.settings_edit_profile.direct_wol, " Direct local UDP Wake-on-LAN (bypass Pi gateway)");
                    ui.add_space(metrics::SPACE_XS);

                    egui::Grid::new("settings_grid_gateway")
                        .num_columns(2)
                        .spacing([metrics::SPACE_LG, metrics::SPACE_SM])
                        .min_col_width(140.0)
                        .show(ui, |ui| {
                            ui.label(RichText::new("Direct WOL UDP Port:").size(metrics::FS_LABEL).color(colors::TEXT_MUTED));
                            let mut dwp_s = self.settings_edit_profile.direct_wol_port.to_string();
                            if ui.add(egui::TextEdit::singleline(&mut dwp_s).desired_width(f32::INFINITY)).changed() {
                                if let Ok(p) = dwp_s.parse::<u16>() {
                                    self.settings_edit_profile.direct_wol_port = p;
                                }
                            }
                            ui.end_row();

                            ui.label(RichText::new("Pi Gateway Host IP:").size(metrics::FS_LABEL).color(colors::TEXT_MUTED));
                            ui.add(egui::TextEdit::singleline(&mut self.settings_edit_profile.pi_host).desired_width(f32::INFINITY));
                            ui.end_row();

                            ui.label(RichText::new("Pi Gateway SSH User:").size(metrics::FS_LABEL).color(colors::TEXT_MUTED));
                            ui.add(egui::TextEdit::singleline(&mut self.settings_edit_profile.pi_user).desired_width(f32::INFINITY));
                            ui.end_row();
                        });

                    ui.add_space(metrics::SPACE_MD);
                    ui.separator();
                    ui.add_space(metrics::SPACE_SM);

                    // Section 3: Ports & Timing
                    ui.label(RichText::new("PORTS & TIMING").size(metrics::FS_LABEL).strong().color(colors::ACCENT));
                    ui.add_space(metrics::SPACE_XS);

                    egui::Grid::new("settings_grid_ports")
                        .num_columns(2)
                        .spacing([metrics::SPACE_LG, metrics::SPACE_SM])
                        .min_col_width(140.0)
                        .show(ui, |ui| {
                            ui.label(RichText::new("SSH Port:").size(metrics::FS_LABEL).color(colors::TEXT_MUTED));
                            let mut port_s = self.settings_edit_profile.ssh_port.to_string();
                            if ui.add(egui::TextEdit::singleline(&mut port_s).desired_width(f32::INFINITY)).changed() {
                                if let Ok(p) = port_s.parse::<u16>() {
                                    self.settings_edit_profile.ssh_port = p;
                                }
                            }
                            ui.end_row();

                            ui.label(RichText::new("Sunshine Port:").size(metrics::FS_LABEL).color(colors::TEXT_MUTED));
                            let mut sun_s = self.settings_edit_profile.sunshine_port.to_string();
                            if ui.add(egui::TextEdit::singleline(&mut sun_s).desired_width(f32::INFINITY)).changed() {
                                if let Ok(p) = sun_s.parse::<u16>() {
                                    self.settings_edit_profile.sunshine_port = p;
                                }
                            }
                            ui.end_row();

                            ui.label(RichText::new("Max Wait Timeout (sec):").size(metrics::FS_LABEL).color(colors::TEXT_MUTED));
                            let mut wait_s = self.settings_edit_profile.max_wait_seconds.to_string();
                            if ui.add(egui::TextEdit::singleline(&mut wait_s).desired_width(f32::INFINITY)).changed() {
                                if let Ok(s) = wait_s.parse::<u64>() {
                                    self.settings_edit_profile.max_wait_seconds = s;
                                }
                            }
                            ui.end_row();

                            ui.label(RichText::new("WOL Resend Interval (sec):").size(metrics::FS_LABEL).color(colors::TEXT_MUTED));
                            let mut resend_s = self.settings_edit_profile.wol_resend_interval.to_string();
                            if ui.add(egui::TextEdit::singleline(&mut resend_s).desired_width(f32::INFINITY)).changed() {
                                if let Ok(s) = resend_s.parse::<u64>() {
                                    self.settings_edit_profile.wol_resend_interval = s;
                                }
                            }
                            ui.end_row();
                        });

                    ui.add_space(metrics::SPACE_SM);
                    ui.checkbox(&mut self.settings_edit_profile.auto_close, " Auto-close window when workstation is Ready");
                });

                ui.add_space(metrics::SPACE_LG);

                // Modal Footer
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if flow_button(ui, "Save & Apply", colors::OK, colors::BTN_LABEL_ON_ACCENT).clicked() {
                            match validate_settings(&self.settings_edit_profile) {
                                Ok(()) => {
                                    let prof_name = if self.settings_profile_name.trim().is_empty() {
                                        "thinkstation".to_string()
                                    } else {
                                        self.settings_profile_name.trim().to_string()
                                    };

                                    self.app_config.set_profile(&prof_name, self.settings_edit_profile.clone());
                                    self.app_config.active_profile = prof_name;
                                    self.current_profile = self.settings_edit_profile.clone();

                                    if let Err(e) = save_config_to_path(&self.app_config, &self.config_path) {
                                        self.settings_error = Some(format!("Failed to save config: {}", e));
                                    } else {
                                        self.pipeline.log(LogLevel::Ok, "Configuration saved successfully");
                                        self.show_settings = false;
                                    }
                                }
                                Err(err) => {
                                    self.settings_error = Some(err);
                                }
                            }
                        }

                        ui.add_space(metrics::SPACE_SM);

                        if flow_button(ui, "Cancel", colors::BTN_BG, colors::TEXT_MUTED).clicked() {
                            self.show_settings = false;
                        }
                    });
                });
            });
    }

    fn render_power_modal(&mut self, ctx: &egui::Context) {
        if !self.show_power_modal {
            return;
        }

        let screen = ctx.screen_rect();
        let modal_h = (screen.height() - 80.0).clamp(280.0, 400.0);
        let modal_w = (screen.width() - 80.0).clamp(340.0, 480.0);

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
                    egui::Frame::none()
                        .fill(colors::CARD_BG)
                        .stroke(Stroke::new(1.0_f32, colors::BORDER_HI))
                        .rounding(Rounding::same(metrics::ROUNDING_MD))
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            ui.label(RichText::new(status).size(metrics::FS_LABEL).color(colors::ACCENT));
                        });
                    ui.add_space(metrics::SPACE_SM);
                }

                // Check whether we are in Confirmation Screen or Action Selection Screen
                if let Some((action_code, action_title)) = self.pending_power_action {
                    // Confirmation View
                    let (banner_bg, banner_border, banner_fg) = if action_code == "shutdown" {
                        (colors::ERR_BG, colors::ERR, colors::ERR)
                    } else {
                        (colors::WARN_BG, colors::WARN, colors::WARN)
                    };

                    egui::Frame::none()
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
                        let btn_color = if action_code == "shutdown" { colors::ERR } else { colors::WARN };
                        let action_btn_text = format!("Confirm {}", action_code.to_uppercase());
                        let confirm_label_color = if action_code == "shutdown" { colors::TEXT } else { colors::BTN_LABEL_ON_ACCENT };

                        if flow_button(ui, &action_btn_text, btn_color, confirm_label_color).clicked() {
                            match self.pipeline.trigger_power_action(&self.current_profile, action_code) {
                                Ok(m) => {
                                    self.power_status = Some(m);
                                    self.pending_power_action = None;
                                    self.show_power_modal = false;
                                }
                                Err(e) => {
                                    self.power_status = Some(format!("Error: {}", e));
                                    self.pending_power_action = None;
                                }
                            }
                        }

                        ui.add_space(metrics::SPACE_SM);

                        if flow_button(ui, "Back", colors::BTN_BG, colors::TEXT_MUTED).clicked() {
                            self.pending_power_action = None;
                        }
                    });
                } else {
                    // Action Selection View
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
                        if flow_button(ui, "Cancel", colors::BTN_BG, colors::TEXT_MUTED).clicked() {
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
        accent_color: Color32,
    ) {
        egui::Frame::none()
            .fill(colors::CARD_BG)
            .stroke(Stroke::new(1.0_f32, colors::BORDER))
            .rounding(Rounding::same(metrics::ROUNDING_MD))
            .inner_margin(Vec2::new(12.0, 8.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(title).size(metrics::FS_LABEL).strong().color(colors::TEXT));
                        ui.add_space(1.0);
                        ui.label(RichText::new(sub).size(metrics::FS_SMALL).color(colors::TEXT_DIM));
                    });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let execute = egui::Button::new(RichText::new("Execute").size(metrics::FS_LABEL).color(accent_color))
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
