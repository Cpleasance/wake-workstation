pub mod console;
pub mod header;
pub mod hero;
pub mod json_config_modal;
pub mod new_profile_modal;
pub mod power_modal;
pub mod settings_modal;
pub mod stepper;
pub mod theme;
pub mod toast;

use crate::config::{AppConfig, HostProfile};
use crate::net::launch_moonlight;
use crate::pipeline::{LogLevel, PipelineState, Stage};
use crate::ui::theme::{apply_custom_theme, colors, flow_button, metrics};
use crate::ui::toast::{render_toasts, Toast};
use eframe::egui::{self, Align, Layout, Vec2};
use std::path::PathBuf;
use std::time::Duration;

pub struct WakeApp {
    pub app_config: AppConfig,
    pub current_profile: HostProfile,
    pub config_path: PathBuf,
    pub auto_launch_moonlight: bool,
    pub pipeline: PipelineState,

    // Modal & UI Controls state
    pub show_settings: bool,
    pub settings_tab: usize,
    pub show_power_modal: bool,
    pub pending_power_action: Option<(&'static str, &'static str)>,
    pub settings_edit_profile: HostProfile,
    pub settings_profile_name: String,
    pub settings_error: Option<String>,
    pub power_status: Option<String>,

    // Toast notifications
    pub toasts: Vec<Toast>,

    // Profile creation popup state
    pub show_new_profile_dialog: bool,
    pub new_profile_name_input: String,

    // JSON Config Export/Import Modal state
    pub show_json_config_dialog: bool,
    pub json_config_input: String,
    pub json_config_is_export: bool,

    // Console filters & collapse state
    pub console_collapsed: bool,
    pub console_level_filter: Option<LogLevel>,
    pub console_search_query: String,
    pub console_auto_scroll: bool,
}

impl WakeApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        app_config: AppConfig,
        config_path: PathBuf,
        auto_launch_moonlight: bool,
    ) -> Self {
        apply_custom_theme(&cc.egui_ctx);

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
            settings_tab: 0,
            show_power_modal: false,
            pending_power_action: None,
            settings_edit_profile,
            settings_profile_name,
            settings_error: None,
            power_status: None,
            toasts: Vec::new(),
            show_new_profile_dialog: false,
            new_profile_name_input: String::new(),
            show_json_config_dialog: false,
            json_config_input: String::new(),
            json_config_is_export: false,
            console_collapsed: false,
            console_level_filter: None,
            console_search_query: String::new(),
            console_auto_scroll: true,
        }
    }

    pub fn add_toast(&mut self, toast: Toast) {
        self.toasts.push(toast);
    }

    fn handle_keyboard_shortcuts(&mut self, ctx: &egui::Context) {
        let mut reset_pipeline = false;
        let mut launch_stream = false;
        let mut send_wol = false;
        let mut clear_log = false;
        let mut toggle_console = false;
        let mut save_modal = false;
        let mut close_escape = false;

        // While any modal is open, shortcuts that fire real side effects on the
        // underlying pipeline (resending WOL, clearing logs) must not fire in
        // the background -- otherwise a user reading e.g. the shutdown
        // confirmation modal can trigger live actions without realizing it.
        let any_modal_open = self.show_settings
            || self.show_power_modal
            || self.show_new_profile_dialog
            || self.show_json_config_dialog;

        ctx.input(|i| {
            if i.key_pressed(egui::Key::Escape) {
                close_escape = true;
            }

            if self.show_settings
                && (i.modifiers.command || i.modifiers.ctrl)
                && i.key_pressed(egui::Key::S)
            {
                save_modal = true;
            }

            if (i.modifiers.command || i.modifiers.ctrl) && i.key_pressed(egui::Key::T) {
                toggle_console = true;
            }

            if !ctx.wants_keyboard_input() && !any_modal_open {
                if i.key_pressed(egui::Key::F5)
                    || ((i.modifiers.command || i.modifiers.ctrl) && i.key_pressed(egui::Key::R))
                {
                    send_wol = true;
                }

                if (i.modifiers.command || i.modifiers.ctrl)
                    && (i.key_pressed(egui::Key::L) || i.key_pressed(egui::Key::K))
                {
                    clear_log = true;
                }

                if !self.show_settings
                    && !self.show_power_modal
                    && !self.show_new_profile_dialog
                    && !self.show_json_config_dialog
                    && (i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter))
                {
                    if self.pipeline.stage == Stage::Standby {
                        reset_pipeline = true;
                    } else if self.pipeline.stage == Stage::Ready {
                        launch_stream = true;
                    } else {
                        reset_pipeline = true;
                    }
                }
            }
        });

        if close_escape {
            if self.show_json_config_dialog {
                self.show_json_config_dialog = false;
            } else if self.show_new_profile_dialog {
                self.show_new_profile_dialog = false;
            } else if self.show_settings {
                self.show_settings = false;
            } else if self.show_power_modal {
                if self.pending_power_action.is_some() {
                    self.pending_power_action = None;
                } else {
                    self.show_power_modal = false;
                }
            }
        }

        if toggle_console {
            self.console_collapsed = !self.console_collapsed;
            let status_str = if self.console_collapsed {
                "collapsed"
            } else {
                "expanded"
            };
            self.add_toast(Toast::info(format!("Activity log {}", status_str)));
        }

        if save_modal {
            self.save_settings_from_modal();
        }

        if send_wol {
            self.pipeline.send_wol(&self.current_profile);
            self.add_toast(Toast::info("WOL magic packet broadcasted"));
        }

        if clear_log {
            self.pipeline.clear_logs();
            self.add_toast(Toast::info("Activity logs cleared"));
        }

        if reset_pipeline {
            self.pipeline.reset(&self.current_profile);
            self.add_toast(Toast::info("WOL sequence initiated"));
        }

        if launch_stream {
            self.pipeline
                .log(LogLevel::Action, "Manual Moonlight launch requested (Space/Enter)");
            if launch_moonlight() {
                self.pipeline
                    .log(LogLevel::Ok, "Moonlight streaming client opened");
                self.add_toast(Toast::success("Moonlight streaming client opened"));
            } else {
                self.pipeline.log(
                    LogLevel::Warn,
                    "Could not find Moonlight executable in standard paths/Flatpak/Snap",
                );
                self.add_toast(Toast::warning("Moonlight executable not found"));
            }
        }
    }

    fn render_action_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            let is_standby = self.pipeline.stage == Stage::Standby;
            let is_ready = self.pipeline.stage == Stage::Ready;
            let is_timeout = self.pipeline.stage == Stage::Timeout;

            let (btn_text, btn_color, btn_label_color, hover_text) = if is_standby {
                (
                    "Wake Workstation  [Space]",
                    colors::OK,
                    colors::BTN_LABEL_ON_LIGHT,
                    "Broadcast Wake-on-LAN and start the connection sequence (Shortcut: Space or Enter)",
                )
            } else if is_ready {
                (
                    "Launch Moonlight  [Space]",
                    colors::OK,
                    colors::BTN_LABEL_ON_LIGHT,
                    "Open the Moonlight streaming client (Shortcut: Space or Enter)",
                )
            } else if is_timeout {
                (
                    "Retry Wake Sequence  [Space]",
                    colors::ERR,
                    colors::BTN_LABEL_ON_LIGHT,
                    "Restart the wake sequence from scratch after the previous attempt timed out (Shortcut: Space or Enter)",
                )
            } else {
                (
                    "Restart Sequence  [Space]",
                    colors::ACCENT,
                    colors::BTN_LABEL_ON_ACCENT,
                    "Abandon the current attempt and restart the wake sequence from scratch (Shortcut: Space or Enter)",
                )
            };

            let wake_btn = flow_button(ui, btn_text, btn_color, btn_label_color);
            if wake_btn.clicked() {
                if is_ready {
                    self.pipeline.log(LogLevel::Action, "Manual Moonlight launch requested");
                    if launch_moonlight() {
                        self.pipeline.log(LogLevel::Ok, "Moonlight streaming client opened");
                        self.add_toast(Toast::success("Moonlight streaming client opened"));
                    } else {
                        self.pipeline.log(
                            LogLevel::Warn,
                            "Could not find Moonlight executable in standard paths/Flatpak/Snap",
                        );
                        self.add_toast(Toast::warning("Moonlight executable not found"));
                    }
                } else {
                    self.pipeline.reset(&self.current_profile);
                    self.add_toast(Toast::info("WOL sequence initiated"));
                }
            }
            wake_btn.on_hover_text(hover_text);

            ui.add_space(metrics::SPACE_SM);

            let wol_btn = flow_button(ui, "Re-send WOL  [F5]", colors::BTN_BG, colors::TEXT);
            if wol_btn.clicked() {
                self.pipeline.send_wol(&self.current_profile);
                self.add_toast(Toast::info("WOL magic packet broadcasted"));
            }
            wol_btn.on_hover_text("Broadcast magic packet immediately to MAC address (Shortcut: F5)");

            ui.add_space(metrics::SPACE_SM);

            let pwr_btn = flow_button(ui, "Power", colors::BTN_BG, colors::TEXT);
            if pwr_btn.clicked() {
                self.power_status = None;
                self.pending_power_action = None;
                self.show_power_modal = true;
            }
            pwr_btn.on_hover_text("Remote power controls (Suspend, Reboot, Shutdown)");

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let close_btn = flow_button(ui, "Close", colors::BTN_BG, colors::TEXT_MUTED);
                if close_btn.clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                close_btn.on_hover_text("Close application window");
            });
        });
    }
}

impl eframe::App for WakeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_keyboard_shortcuts(ctx);

        if self.pipeline.stage != Stage::Standby && !self.pipeline.is_finished {
            self.pipeline.tick(&self.current_profile, self.auto_launch_moonlight);
            ctx.request_repaint_after(Duration::from_millis(400));
        } else {
            ctx.request_repaint_after(Duration::from_millis(1500));
        }

        if self.pipeline.stage == Stage::Ready && self.current_profile.auto_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(colors::BG).inner_margin(metrics::OUTER_MARGIN))
            .show(ctx, |ui| {
                let avail_width = ui.available_width();
                let content_width = avail_width.min(metrics::MAX_CONTENT_WIDTH);
                let side_pad = ((avail_width - content_width) / 2.0).max(0.0);
                let avail_height = ui.available_height();

                ui.horizontal(|ui| {
                    if side_pad > 0.5 {
                        ui.add_space(side_pad);
                    }
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

                            let reserved_below = metrics::ACTION_BAR_HEIGHT + metrics::SPACE_MD;
                            let console_height = if self.console_collapsed {
                                36.0
                            } else {
                                (ui.available_height() - reserved_below).max(metrics::MIN_CONSOLE_HEIGHT)
                            };
                            self.render_console(ui, ctx, console_height);
                            ui.add_space(metrics::SPACE_MD);

                            self.render_action_bar(ui, ctx);
                        },
                    );
                });
            });

        self.render_settings_modal(ctx);
        self.render_power_modal(ctx);
        self.render_new_profile_dialog(ctx);
        self.render_json_config_dialog(ctx);

        render_toasts(ctx, &mut self.toasts);
    }
}
