pub mod console;
pub mod header;
pub mod hero;
pub mod json_config_modal;
pub mod new_profile_modal;
pub mod power_modal;
pub mod settings_modal;
pub mod shortcuts_modal;
pub mod stepper;
pub mod theme;
pub mod toast;

use crate::config::{AppConfig, HostProfile};
use crate::net::launch_moonlight_stream;
use crate::pipeline::{LogLevel, PipelineState, Stage};
use crate::ui::theme::{apply_custom_theme, colors, flow_button, metrics};
use crate::ui::toast::{render_toasts, Toast};
use eframe::egui::{self, Align, Layout, Vec2};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::Duration;

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum ReachabilityState {
    Idle,
    Testing {
        host: String,
        started_at: std::time::Instant,
    },
    Result {
        host: String,
        ping_ok: bool,
        latency_ms: Option<f32>,
        ssh_port: u16,
        ssh_ok: bool,
        checked_at: std::time::Instant,
    },
}

pub type ReachabilityPayload = (String, bool, Option<f32>, u16, bool);

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

    // Reachability test state
    pub reachability_state: ReachabilityState,
    pub reachability_rx: Option<Receiver<ReachabilityPayload>>,

    // Toast notifications
    pub toasts: Vec<Toast>,

    // Profile creation popup state
    pub show_new_profile_dialog: bool,
    pub new_profile_name_input: String,

    // JSON Config Export/Import Modal state
    pub show_json_config_dialog: bool,
    pub json_config_input: String,
    pub json_config_is_export: bool,

    // Keyboard Shortcuts Modal state
    pub show_shortcuts_modal: bool,

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
            reachability_state: ReachabilityState::Idle,
            reachability_rx: None,
            toasts: Vec::new(),
            show_new_profile_dialog: false,
            new_profile_name_input: String::new(),
            show_json_config_dialog: false,
            json_config_input: String::new(),
            json_config_is_export: false,
            show_shortcuts_modal: false,
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
        let mut sleep_host = false;
        let mut shutdown_host = false;
        let mut send_wol = false;
        let mut clear_log = false;
        let mut toggle_console = false;
        let mut toggle_shortcuts = false;
        let mut toggle_settings = false;
        let mut toggle_power_modal = false;
        let mut save_modal = false;
        let mut close_escape = false;
        let mut confirm_power_action = false;
        let mut power_number_select: Option<usize> = None;

        // While any modal is open, shortcuts that fire real side effects on the
        // underlying pipeline (resending WOL, clearing logs) must not fire in
        // the background -- otherwise a user reading e.g. the shutdown
        // confirmation modal can trigger live actions without realizing it.
        let any_modal_open = self.show_settings
            || self.show_power_modal
            || self.show_new_profile_dialog
            || self.show_json_config_dialog
            || self.show_shortcuts_modal;

        // Must be read before entering `ctx.input()` below -- egui's Context
        // locks its input state for the duration of that closure, and
        // `wants_keyboard_input()` needs that same lock internally, so calling
        // it from inside the closure self-deadlocks the whole UI thread on
        // the very first frame (observed as the window getting created but
        // never showing/resizing -- "app won't open").
        let wants_keyboard_input = ctx.wants_keyboard_input();

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

            if self.show_power_modal {
                if self.pending_power_action.is_some() {
                    if i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space) {
                        confirm_power_action = true;
                    }
                } else if !wants_keyboard_input {
                    if i.key_pressed(egui::Key::Num1) {
                        power_number_select = Some(1);
                    } else if i.key_pressed(egui::Key::Num2) {
                        power_number_select = Some(2);
                    } else if i.key_pressed(egui::Key::Num3) {
                        power_number_select = Some(3);
                    }
                }
            }

            if !wants_keyboard_input && !any_modal_open {
                if i.key_pressed(egui::Key::F1)
                    || (i.modifiers.shift && i.key_pressed(egui::Key::Slash))
                {
                    toggle_shortcuts = true;
                }

                if (i.modifiers.command || i.modifiers.ctrl) && i.key_pressed(egui::Key::Comma) {
                    toggle_settings = true;
                }

                if i.modifiers.alt && i.key_pressed(egui::Key::P) {
                    toggle_power_modal = true;
                }

                if (i.modifiers.command || i.modifiers.ctrl) && i.key_pressed(egui::Key::T) {
                    toggle_console = true;
                }

                if i.key_pressed(egui::Key::F5)
                    || ((i.modifiers.command || i.modifiers.ctrl) && i.key_pressed(egui::Key::R))
                {
                    send_wol = true;
                }

                if (i.modifiers.command || i.modifiers.ctrl)
                    && i.key_pressed(egui::Key::P)
                    && self.pipeline.stage == Stage::Ready
                {
                    if i.modifiers.shift {
                        shutdown_host = true;
                    } else {
                        sleep_host = true;
                    }
                }

                if (i.modifiers.command || i.modifiers.ctrl)
                    && (i.key_pressed(egui::Key::L) || i.key_pressed(egui::Key::K))
                {
                    clear_log = true;
                }

                let in_power_cooldown = self
                    .pipeline
                    .last_power_action_time
                    .map(|t| t.elapsed() < std::time::Duration::from_millis(2000))
                    .unwrap_or(false);

                if !in_power_cooldown {
                    if self.pipeline.stage == Stage::Standby {
                        if i.key_pressed(egui::Key::Space) {
                            reset_pipeline = true;
                        }
                    } else if self.pipeline.stage == Stage::Ready {
                        if i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter) {
                            launch_stream = true;
                        }
                    } else if i.key_pressed(egui::Key::Space) {
                        reset_pipeline = true;
                    }
                }
            }
        });

        if close_escape {
            if self.show_shortcuts_modal {
                self.show_shortcuts_modal = false;
            } else if self.show_json_config_dialog {
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

        if toggle_shortcuts {
            self.show_shortcuts_modal = !self.show_shortcuts_modal;
        }

        if toggle_settings {
            self.settings_edit_profile = self.current_profile.clone();
            self.settings_profile_name = self.app_config.active_profile.clone();
            self.settings_error = None;
            self.show_settings = !self.show_settings;
        }

        if toggle_power_modal {
            self.show_power_modal = !self.show_power_modal;
            self.pending_power_action = None;
        }

        if confirm_power_action {
            if let Some((action_code, _action_title)) = self.pending_power_action {
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
                        self.add_toast(Toast::error(format!("Power action failed: {}", e)));
                        self.pending_power_action = None;
                    }
                }
            }
        }

        if let Some(num) = power_number_select {
            match num {
                1 => self.pending_power_action = Some(("suspend", "Suspend / Sleep")),
                2 => self.pending_power_action = Some(("reboot", "Reboot Target")),
                3 => self.pending_power_action = Some(("shutdown", "Power Off / Shutdown")),
                _ => {}
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
            self.pipeline.log(
                LogLevel::Action,
                "Manual Moonlight launch requested (Space/Enter)",
            );
            if launch_moonlight_stream(Some(&self.current_profile.thinkstation_ip), None) {
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

        if sleep_host {
            match self
                .pipeline
                .trigger_power_action(&self.current_profile, "suspend")
            {
                Ok(m) => {
                    self.power_status = Some(m);
                    self.add_toast(Toast::info("Workstation put to sleep"));
                }
                Err(e) => {
                    self.power_status = Some(format!("Error: {}", e));
                    self.add_toast(Toast::error(format!("Sleep failed: {}", e)));
                }
            }
        }

        if shutdown_host {
            match self
                .pipeline
                .trigger_power_action(&self.current_profile, "shutdown")
            {
                Ok(m) => {
                    self.power_status = Some(m);
                    self.add_toast(Toast::info("Workstation powered off"));
                }
                Err(e) => {
                    self.power_status = Some(format!("Error: {}", e));
                    self.add_toast(Toast::error(format!("Shutdown failed: {}", e)));
                }
            }
        }
    }

    fn render_action_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let is_standby = self.pipeline.stage == Stage::Standby;
        let is_ready = self.pipeline.stage == Stage::Ready;
        let is_timeout = self.pipeline.stage == Stage::Timeout;

        let (btn_text, btn_color, btn_label_color, hover_text) = if is_standby {
            (
                "Wake Workstation  [Space]",
                colors::OK,
                colors::BTN_LABEL_ON_LIGHT,
                "Broadcast Wake-on-LAN and start the connection sequence (Shortcut: Space)",
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
                "Restart the wake sequence from scratch after the previous attempt timed out (Shortcut: Space)",
            )
        } else {
            (
                "Restart Sequence  [Space]",
                colors::ACCENT,
                colors::BTN_LABEL_ON_ACCENT,
                "Abandon the current attempt and restart the wake sequence from scratch (Shortcut: Space)",
            )
        };

        let avail_width = ui.available_width();
        let wrap_threshold = if is_ready { 760.0 } else { 560.0 };

        if avail_width >= wrap_threshold {
            ui.horizontal(|ui| {
                let wake_btn = flow_button(ui, btn_text, btn_color, btn_label_color);
                if wake_btn.clicked() {
                    if is_ready {
                        self.pipeline.log(LogLevel::Action, "Manual Moonlight launch requested");
                        if launch_moonlight_stream(Some(&self.current_profile.thinkstation_ip), None) {
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

                if is_ready {
                    let sleep_btn = flow_button(ui, "Sleep Host  [Ctrl+P]", colors::WARN, colors::BTN_LABEL_ON_LIGHT);
                    if sleep_btn.clicked() {
                        match self.pipeline.trigger_power_action(&self.current_profile, "suspend") {
                            Ok(m) => {
                                self.power_status = Some(m);
                                self.add_toast(Toast::info("Workstation put to sleep"));
                            }
                            Err(e) => {
                                self.power_status = Some(format!("Error: {}", e));
                                self.add_toast(Toast::error(format!("Sleep failed: {}", e)));
                            }
                        }
                    }
                    sleep_btn.on_hover_text("Put remote workstation to sleep (systemctl suspend) and enter Standby");
                    ui.add_space(metrics::SPACE_SM);

                    let off_btn = flow_button(ui, "Turn Off  [Ctrl+Shift+P]", colors::ERR, colors::BTN_LABEL_ON_LIGHT);
                    if off_btn.clicked() {
                        match self.pipeline.trigger_power_action(&self.current_profile, "shutdown") {
                            Ok(m) => {
                                self.power_status = Some(m);
                                self.add_toast(Toast::info("Workstation powered off"));
                            }
                            Err(e) => {
                                self.power_status = Some(format!("Error: {}", e));
                                self.add_toast(Toast::error(format!("Shutdown failed: {}", e)));
                            }
                        }
                    }
                    off_btn.on_hover_text("Completely power down machine (sudo systemctl poweroff) and enter Standby");
                    ui.add_space(metrics::SPACE_SM);
                }

                let wol_btn = flow_button(ui, "Re-send WOL  [F5]", colors::BTN_BG, colors::TEXT);
                if wol_btn.clicked() {
                    self.pipeline.send_wol(&self.current_profile);
                    self.add_toast(Toast::info("WOL magic packet broadcasted"));
                }
                wol_btn.on_hover_text("Broadcast magic packet immediately to MAC address (Shortcut: F5)");

                ui.add_space(metrics::SPACE_SM);

                let pwr_btn = flow_button(ui, "Power Menu  [Alt+P]", colors::BTN_BG, colors::TEXT);
                if pwr_btn.clicked() {
                    self.power_status = None;
                    self.pending_power_action = None;
                    self.show_power_modal = true;
                }
                pwr_btn.on_hover_text("Remote power controls (Suspend, Reboot, Shutdown) (Shortcut: Alt+P)");

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let close_btn = flow_button(ui, "Close", colors::BTN_BG, colors::TEXT_MUTED);
                    if close_btn.clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    close_btn.on_hover_text("Close application window");
                });
            });
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(metrics::SPACE_SM, metrics::SPACE_XS);

                let wake_btn = flow_button(ui, btn_text, btn_color, btn_label_color);
                if wake_btn.clicked() {
                    if is_ready {
                        self.pipeline.log(LogLevel::Action, "Manual Moonlight launch requested");
                        if launch_moonlight_stream(Some(&self.current_profile.thinkstation_ip), None) {
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

                if is_ready {
                    let sleep_btn = flow_button(ui, "Sleep Host  [Ctrl+P]", colors::WARN, colors::BTN_LABEL_ON_LIGHT);
                    if sleep_btn.clicked() {
                        match self.pipeline.trigger_power_action(&self.current_profile, "suspend") {
                            Ok(m) => {
                                self.power_status = Some(m);
                                self.add_toast(Toast::info("Workstation put to sleep"));
                            }
                            Err(e) => {
                                self.power_status = Some(format!("Error: {}", e));
                                self.add_toast(Toast::error(format!("Sleep failed: {}", e)));
                            }
                        }
                    }
                    sleep_btn.on_hover_text("Put remote workstation to sleep (systemctl suspend) and enter Standby");

                    let off_btn = flow_button(ui, "Turn Off  [Ctrl+Shift+P]", colors::ERR, colors::BTN_LABEL_ON_LIGHT);
                    if off_btn.clicked() {
                        match self.pipeline.trigger_power_action(&self.current_profile, "shutdown") {
                            Ok(m) => {
                                self.power_status = Some(m);
                                self.add_toast(Toast::info("Workstation powered off"));
                            }
                            Err(e) => {
                                self.power_status = Some(format!("Error: {}", e));
                                self.add_toast(Toast::error(format!("Shutdown failed: {}", e)));
                            }
                        }
                    }
                    off_btn.on_hover_text("Completely power down machine (sudo systemctl poweroff) and enter Standby");
                }

                let wol_btn = flow_button(ui, "Re-send WOL  [F5]", colors::BTN_BG, colors::TEXT);
                if wol_btn.clicked() {
                    self.pipeline.send_wol(&self.current_profile);
                    self.add_toast(Toast::info("WOL magic packet broadcasted"));
                }
                wol_btn.on_hover_text("Broadcast magic packet immediately to MAC address (Shortcut: F5)");

                let pwr_btn = flow_button(ui, "Power Menu  [Alt+P]", colors::BTN_BG, colors::TEXT);
                if pwr_btn.clicked() {
                    self.power_status = None;
                    self.pending_power_action = None;
                    self.show_power_modal = true;
                }
                pwr_btn.on_hover_text("Remote power controls (Suspend, Reboot, Shutdown) (Shortcut: Alt+P)");

                let close_btn = flow_button(ui, "Close", colors::BTN_BG, colors::TEXT_MUTED);
                if close_btn.clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                close_btn.on_hover_text("Close application window");
            });
        }
    }
}

impl eframe::App for WakeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_keyboard_shortcuts(ctx);

        // Drain asynchronous reachability test response if available
        if let Some(rx) = &self.reachability_rx {
            if let Ok((host, ping_ok, latency_ms, ssh_port, ssh_ok)) = rx.try_recv() {
                self.reachability_state = ReachabilityState::Result {
                    host: host.clone(),
                    ping_ok,
                    latency_ms,
                    ssh_port,
                    ssh_ok,
                    checked_at: std::time::Instant::now(),
                };
                self.reachability_rx = None;

                if ping_ok {
                    if let Some(lat) = latency_ms {
                        self.pipeline.update_latency(Some(lat));
                    }
                    let lat_str = crate::net::format_latency(latency_ms);
                    let ssh_str = if ssh_ok {
                        format!(" | SSH port {} open", ssh_port)
                    } else {
                        String::new()
                    };
                    self.add_toast(Toast::success(format!(
                        "Host '{}' reachable ({}{})",
                        host, lat_str, ssh_str
                    )));
                    self.pipeline.log(
                        LogLevel::Ok,
                        format!(
                            "Reachability check: '{}' responded ({} latency{})",
                            host, lat_str, ssh_str
                        ),
                    );
                } else {
                    self.add_toast(Toast::error(format!(
                        "Host '{}' is unreachable (ICMP ping timed out)",
                        host
                    )));
                    self.pipeline.log(
                        LogLevel::Warn,
                        format!("Reachability check: '{}' is unreachable", host),
                    );
                }
                ctx.request_repaint();
            } else {
                ctx.request_repaint_after(Duration::from_millis(80));
            }
        }

        let is_minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));

        // Stage-aware polling and repaint intervals:
        // 1. Standby & Timeout: True idle states. Egui will render on-demand when the user
        //    interacts or when a toast animation is running, conserving CPU/GPU cycles to 0%.
        // 2. Ready: Host is online; continuously poll live connection telemetry. When minimized,
        //    throttle polling to 3000ms to avoid unnecessary background wakeups.
        // 3. Active wake stages (Wol, Ping, Ssh, Sunshine): Tick fast (200ms) for responsive progression.
        match self.pipeline.stage {
            Stage::Standby | Stage::Timeout => {}
            Stage::Ready => {
                self.pipeline
                    .tick(&self.current_profile, self.auto_launch_moonlight);
                if is_minimized {
                    ctx.request_repaint_after(Duration::from_millis(3000));
                } else {
                    ctx.request_repaint_after(Duration::from_millis(600));
                }
            }
            _ => {
                self.pipeline
                    .tick(&self.current_profile, self.auto_launch_moonlight);
                ctx.request_repaint_after(Duration::from_millis(200));
            }
        }

        if self.pipeline.stage == Stage::Ready && self.current_profile.auto_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // TopBottomPanel::bottom guarantees the bottom Action Bar is NEVER clipped or squeezed
        // off-screen even if the user resizes the window vertically down to minimum height.
        egui::TopBottomPanel::bottom("action_bar_panel")
            .frame(
                egui::Frame::none()
                    .fill(colors::BG)
                    .inner_margin(egui::Margin {
                        left: metrics::OUTER_MARGIN,
                        right: metrics::OUTER_MARGIN,
                        top: metrics::SPACE_SM,
                        bottom: metrics::OUTER_MARGIN,
                    }),
            )
            .show(ctx, |ui| {
                let avail_width = ui.available_width();
                let content_width = avail_width.min(metrics::MAX_CONTENT_WIDTH);
                let side_pad = ((avail_width - content_width) / 2.0).max(0.0);

                ui.horizontal(|ui| {
                    if side_pad > 0.5 {
                        ui.add_space(side_pad);
                    }
                    ui.allocate_ui_with_layout(
                        Vec2::new(content_width, ui.available_height()),
                        Layout::top_down(Align::Min),
                        |ui| {
                            self.render_action_bar(ui, ctx);
                        },
                    );
                });
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(colors::BG)
                    .inner_margin(egui::Margin {
                        left: metrics::OUTER_MARGIN,
                        right: metrics::OUTER_MARGIN,
                        top: metrics::OUTER_MARGIN,
                        bottom: metrics::SPACE_XS,
                    }),
            )
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
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    self.render_header(ui);
                                    ui.add_space(metrics::SPACE_MD);

                                    self.render_stepper(ui);
                                    ui.add_space(metrics::SPACE_MD);

                                    self.render_hero_status(ui);
                                    ui.add_space(metrics::SPACE_MD);

                                    let console_height = if self.console_collapsed {
                                        36.0
                                    } else {
                                        ui.available_height().max(metrics::MIN_CONSOLE_HEIGHT)
                                    };
                                    self.render_console(ui, ctx, console_height);
                                });
                        },
                    );
                });
            });

        self.render_settings_modal(ctx);
        self.render_power_modal(ctx);
        self.render_shortcuts_modal(ctx);
        self.render_new_profile_dialog(ctx);
        self.render_json_config_dialog(ctx);

        render_toasts(ctx, &mut self.toasts);
    }
}
