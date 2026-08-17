use crate::config::HostProfile;
use crate::net::{check_ping, check_tcp_port, execute_power_action, format_latency, launch_moonlight, send_magic_packet, send_ssh_wol};
use chrono::Local;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Standby,
    Wol,
    Ping,
    Ssh,
    Sunshine,
    Ready,
    Timeout,
}

impl Stage {
    #[allow(dead_code)]
    pub fn name(&self) -> &'static str {
        match self {
            Stage::Standby => "STANDBY",
            Stage::Wol => "1. WOL",
            Stage::Ping => "2. PING",
            Stage::Ssh => "3. SSH",
            Stage::Sunshine => "4. STREAM",
            Stage::Ready => "READY",
            Stage::Timeout => "TIMEOUT",
        }
    }

    #[allow(dead_code)]
    pub fn description(&self) -> &'static str {
        match self {
            Stage::Standby => "System in Safe Standby",
            Stage::Wol => "Magic Packet Sent",
            Stage::Ping => "ICMP Network Ping",
            Stage::Ssh => "SSH Daemon (Port 22)",
            Stage::Sunshine => "Sunshine Stream (:47989)",
            Stage::Ready => "Target Ready to Stream",
            Stage::Timeout => "Max Wait Timeout Exceeded",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Ok,
    Warn,
    Err,
    Action,
}

impl LogLevel {
    pub fn tag(&self) -> &'static str {
        match self {
            LogLevel::Info => "INFO",
            LogLevel::Ok => "OK",
            LogLevel::Warn => "WARN",
            LogLevel::Err => "ERR",
            LogLevel::Action => "ACTION",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: chrono::DateTime<Local>,
    pub level: LogLevel,
    pub message: String,
}

/// Result of a background ping/port check performed off the UI thread.
struct CheckResult {
    ping_alive: bool,
    latency: Option<f32>,
    port_open: bool,
}

pub struct PipelineState {
    pub stage: Stage,
    pub start_time: Option<Instant>,
    pub elapsed_secs: u64,
    pub last_latency: Option<f32>,
    pub wol_count: u32,
    pub last_wol_time: Option<Instant>,
    pub is_finished: bool,
    pub auto_launched_moonlight: bool,
    pub status_message: String,
    pub logs: Vec<LogEntry>,
    // Background check in flight for `stage`, if any. Only one check runs at a
    // time; a new thread is spawned only once the previous one's result has
    // been consumed (or the stage changed), so tick() never blocks the caller.
    pending_check: Option<(Stage, Receiver<CheckResult>)>,
}

impl Default for PipelineState {
    fn default() -> Self {
        Self::new()
    }
}

impl PipelineState {
    pub fn new() -> Self {
        let mut state = Self {
            stage: Stage::Wol,
            start_time: None,
            elapsed_secs: 0,
            last_latency: None,
            wol_count: 0,
            last_wol_time: None,
            is_finished: false,
            auto_launched_moonlight: false,
            status_message: "Initializing wake sequence...".to_string(),
            logs: Vec::new(),
            pending_check: None,
        };
        state.log(LogLevel::Info, "Antigravity Wake Pipeline initialized");
        state
    }

    pub fn log(&mut self, level: LogLevel, msg: impl Into<String>) {
        let entry = LogEntry {
            timestamp: Local::now(),
            level,
            message: msg.into(),
        };
        self.logs.push(entry);
    }

    pub fn clear_logs(&mut self) {
        self.logs.clear();
    }

    pub fn formatted_logs_text(&self) -> String {
        self.logs
            .iter()
            .map(|l| {
                format!(
                    "{} [{}] {}",
                    l.timestamp.format("%H:%M:%S"),
                    l.level.tag(),
                    l.message
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn enter_standby(&mut self, reason: &str) {
        self.stage = Stage::Standby;
        self.start_time = None;
        self.is_finished = true;
        self.status_message = format!("Standby: {}", reason);
        self.pending_check = None;
        self.log(LogLevel::Warn, format!("Safe Standby engaged: {}", reason));
    }

    pub fn reset(&mut self, config: &HostProfile) {
        self.stage = Stage::Wol;
        self.start_time = Some(Instant::now());
        self.elapsed_secs = 0;
        self.last_latency = None;
        self.wol_count = 0;
        self.last_wol_time = None;
        self.is_finished = false;
        self.auto_launched_moonlight = false;
        self.status_message = "Wake sequence started".to_string();
        self.pending_check = None;
        self.log(
            LogLevel::Action,
            format!(
                "Starting wake pipeline for {} ({})",
                config.thinkstation_ip, config.mac_address
            ),
        );
        self.send_wol(config);
    }

    pub fn send_wol(&mut self, config: &HostProfile) {
        self.wol_count += 1;
        self.last_wol_time = Some(Instant::now());

        if config.direct_wol || config.pi_host.trim().is_empty() {
            self.log(
                LogLevel::Action,
                format!(
                    "Sending direct local UDP WOL magic packet to {} ({}:{}) [attempt #{}]",
                    config.mac_address, config.broadcast_addr, config.direct_wol_port, self.wol_count
                ),
            );
            match send_magic_packet(
                &config.mac_address,
                &config.broadcast_addr,
                config.direct_wol_port,
            ) {
                Ok(()) => {
                    self.log(
                        LogLevel::Ok,
                        format!("Direct WOL packet broadcast successfully (attempt #{})", self.wol_count),
                    );
                }
                Err(e) => {
                    self.log(LogLevel::Err, format!("Direct WOL failed: {}", e));
                }
            }
        } else {
            self.log(
                LogLevel::Action,
                format!(
                    "Dispatching WOL packet via Pi gateway {}@{} [attempt #{}]",
                    config.pi_user, config.pi_host, self.wol_count
                ),
            );
            match send_ssh_wol(
                &config.pi_user,
                &config.pi_host,
                &config.broadcast_addr,
                &config.mac_address,
            ) {
                Ok(msg) => {
                    let detail = if msg.is_empty() {
                        "WOL command executed on gateway".to_string()
                    } else {
                        msg
                    };
                    self.log(
                        LogLevel::Ok,
                        format!("Gateway WOL sent: {} (attempt #{})", detail, self.wol_count),
                    );
                }
                Err(e) => {
                    self.log(
                        LogLevel::Warn,
                        format!("SSH gateway WOL failed ({}), falling back to direct local UDP packet...", e),
                    );
                    let _ = send_magic_packet(
                        &config.mac_address,
                        &config.broadcast_addr,
                        config.direct_wol_port,
                    );
                }
            }
        }

        if self.stage == Stage::Wol || self.stage == Stage::Standby || self.stage == Stage::Timeout {
            self.stage = Stage::Ping;
            self.is_finished = false;
            if self.start_time.is_none() {
                self.start_time = Some(Instant::now());
            }
        }
    }

    pub fn tick(&mut self, config: &HostProfile, auto_launch_moonlight: bool) {
        if self.stage == Stage::Standby {
            return;
        }

        if self.start_time.is_none() {
            self.start_time = Some(Instant::now());
            self.send_wol(config);
        }

        if let Some(start) = self.start_time {
            self.elapsed_secs = start.elapsed().as_secs();
        }

        // Check overall timeout
        if self.elapsed_secs >= config.max_wait_seconds && self.stage != Stage::Ready {
            self.stage = Stage::Timeout;
            self.status_message = format!(
                "Timed out after {} seconds waiting for host {}",
                config.max_wait_seconds, config.thinkstation_ip
            );
            self.log(LogLevel::Err, self.status_message.clone());
            self.is_finished = true;
            return;
        }

        // Periodic WOL re-send if still waiting on Ping or SSH
        if (self.stage == Stage::Wol || self.stage == Stage::Ping) && config.wol_resend_interval > 0 {
            if let Some(last_wol) = self.last_wol_time {
                if last_wol.elapsed().as_secs() >= config.wol_resend_interval {
                    self.log(
                        LogLevel::Action,
                        format!(
                            "WOL resend interval ({}s) elapsed without ping response. Re-sending magic packet...",
                            config.wol_resend_interval
                        ),
                    );
                    self.send_wol(config);
                }
            }
        }

        // State Machine progression. Ping/port checks are handed off to a
        // background thread (see poll_check/spawn_check) so this never blocks.
        match self.stage {
            Stage::Standby | Stage::Timeout => {}
            Stage::Wol => {
                self.send_wol(config);
            }
            Stage::Ping => {
                self.status_message = format!(
                    "Waiting for ICMP ping response from {} (elapsed {}s)...",
                    config.thinkstation_ip, self.elapsed_secs
                );
                if let Some(result) = self.poll_check(config, Stage::Ping) {
                    if let Some(lat) = result.latency {
                        self.last_latency = Some(lat);
                    }
                    if result.ping_alive {
                        self.log(
                            LogLevel::Ok,
                            format!(
                                "Host {} responded to ICMP ping (latency: {})",
                                config.thinkstation_ip,
                                format_latency(self.last_latency)
                            ),
                        );
                        self.stage = Stage::Ssh;
                        self.status_message = format!(
                            "Host reachable ({}), checking SSH daemon...",
                            format_latency(self.last_latency)
                        );
                    }
                }
            }
            Stage::Ssh => {
                self.status_message = format!(
                    "Ping active ({}), waiting for SSH daemon port {}...",
                    format_latency(self.last_latency),
                    config.ssh_port
                );
                if let Some(result) = self.poll_check(config, Stage::Ssh) {
                    if let Some(lat) = result.latency {
                        self.last_latency = Some(lat);
                    }
                    if result.port_open {
                        self.log(
                            LogLevel::Ok,
                            format!(
                                "SSH service verified listening on {}:{}",
                                config.thinkstation_ip, config.ssh_port
                            ),
                        );
                        self.stage = Stage::Sunshine;
                        self.status_message = format!(
                            "SSH online, checking Sunshine streaming port (:{})...",
                            config.sunshine_port
                        );
                    } else {
                        self.status_message = format!(
                            "Ping active ({}), waiting for SSH daemon port {}...",
                            format_latency(self.last_latency),
                            config.ssh_port
                        );
                    }
                }
            }
            Stage::Sunshine => {
                self.status_message = format!(
                    "SSH up, waiting for Sunshine port {} to open...",
                    config.sunshine_port
                );
                if let Some(result) = self.poll_check(config, Stage::Sunshine) {
                    if let Some(lat) = result.latency {
                        self.last_latency = Some(lat);
                    }
                    if result.port_open {
                        self.log(
                            LogLevel::Ok,
                            format!(
                                "Sunshine stream service active on {}:{}",
                                config.thinkstation_ip, config.sunshine_port
                            ),
                        );
                        self.stage = Stage::Ready;
                        self.status_message = "Host is fully ready and online for game streaming!".to_string();
                        self.is_finished = true;

                        if auto_launch_moonlight && !self.auto_launched_moonlight {
                            self.auto_launched_moonlight = true;
                            self.log(LogLevel::Action, "Launching Moonlight streaming client...");
                            if launch_moonlight() {
                                self.log(LogLevel::Ok, "Moonlight client spawned successfully");
                            } else {
                                self.log(LogLevel::Warn, "Could not automatically find Moonlight binary to launch");
                            }
                        }
                    } else {
                        self.status_message = format!(
                            "SSH up, waiting for Sunshine port {} to open...",
                            config.sunshine_port
                        );
                    }
                }
            }
            Stage::Ready => {
                // Keep polling ping latency for real-time connection telemetry
                if let Some(result) = self.poll_check(config, Stage::Ready) {
                    if let Some(lat) = result.latency {
                        self.last_latency = Some(lat);
                    }
                }
            }
        }
    }

    /// Poll the in-flight background check for `stage`, spawning one if none
    /// is currently running. Returns `Some` only once a result has arrived;
    /// clears the in-flight slot on completion (or on a dead sender) so the
    /// next tick spawns a fresh check rather than re-polling a stale one.
    fn poll_check(&mut self, config: &HostProfile, stage: Stage) -> Option<CheckResult> {
        let need_new = !matches!(&self.pending_check, Some((s, _)) if *s == stage);
        if need_new {
            self.pending_check = Some((stage, spawn_check(config, stage)));
        }

        match self.pending_check.as_ref().map(|(_, rx)| rx.try_recv()) {
            Some(Ok(result)) => {
                self.pending_check = None;
                Some(result)
            }
            Some(Err(TryRecvError::Empty)) => None,
            Some(Err(TryRecvError::Disconnected)) => {
                self.pending_check = None;
                None
            }
            None => None,
        }
    }

    pub fn trigger_power_action(
        &mut self,
        config: &HostProfile,
        action: &str,
    ) -> Result<String, String> {
        self.log(
            LogLevel::Action,
            format!(
                "Initiating remote power action '{}' on {}@{}",
                action, config.thinkstation_user, config.thinkstation_ip
            ),
        );

        let res = execute_power_action(
            &config.thinkstation_ip,
            &config.thinkstation_user,
            action,
            config.ssh_port,
        );

        match res {
            Ok(msg) => {
                self.log(LogLevel::Ok, &msg);
                if action == "suspend" || action == "shutdown" || action == "poweroff" {
                    self.enter_standby(&format!("Host {}", action));
                } else if action == "reboot" {
                    self.reset(config);
                }
                Ok(msg)
            }
            Err(err) => {
                self.log(LogLevel::Err, format!("Power action '{}' failed: {}", action, err));
                Err(err)
            }
        }
    }
}

/// Spawn a background thread performing the blocking ping/port check(s) for
/// `stage` and return the receiver its result will arrive on.
fn spawn_check(config: &HostProfile, stage: Stage) -> Receiver<CheckResult> {
    let (tx, rx) = mpsc::channel();
    let host = config.thinkstation_ip.clone();
    let ssh_port = config.ssh_port;
    let sunshine_port = config.sunshine_port;

    thread::spawn(move || {
        let result = match stage {
            Stage::Ping => {
                let (alive, latency) = check_ping(&host, 1);
                CheckResult {
                    ping_alive: alive,
                    latency,
                    port_open: false,
                }
            }
            Stage::Ssh => {
                let (_, latency) = check_ping(&host, 1);
                let port_open = check_tcp_port(&host, ssh_port, 400);
                CheckResult {
                    ping_alive: false,
                    latency,
                    port_open,
                }
            }
            Stage::Sunshine => {
                let (_, latency) = check_ping(&host, 1);
                let port_open = check_tcp_port(&host, sunshine_port, 400);
                CheckResult {
                    ping_alive: false,
                    latency,
                    port_open,
                }
            }
            Stage::Ready => {
                let (_, latency) = check_ping(&host, 1);
                CheckResult {
                    ping_alive: false,
                    latency,
                    port_open: false,
                }
            }
            Stage::Standby | Stage::Wol | Stage::Timeout => CheckResult {
                ping_alive: false,
                latency: None,
                port_open: false,
            },
        };
        // Ignore send errors: the receiver may have been dropped if the
        // pipeline moved on (reset, stage change) before this finished.
        let _ = tx.send(result);
    });

    rx
}

/// Run pipeline in CLI terminal mode.
pub fn run_cli_pipeline(config: HostProfile, auto_launch_moonlight: bool) -> Result<(), String> {
    let mut state = PipelineState::new();
    state.reset(&config);

    println!(
        "\n======================================================\n\
         🚀 WAKE THINKSTATION (Rust CLI)\n\
         Target:      {} ({})\n\
         MAC:         {}\n\
         Max Timeout: {}s\n\
         ======================================================\n",
        config.thinkstation_ip, config.thinkstation_user, config.mac_address, config.max_wait_seconds
    );

    let mut last_log_idx = 0;
    while !state.is_finished {
        state.tick(&config, auto_launch_moonlight);

        // Print new log entries
        while last_log_idx < state.logs.len() {
            let l = &state.logs[last_log_idx];
            println!(
                "{} [{}] {}",
                l.timestamp.format("%H:%M:%S"),
                l.level.tag(),
                l.message
            );
            last_log_idx += 1;
        }

        if state.is_finished {
            break;
        }

        std::thread::sleep(std::time::Duration::from_millis(800));
    }

    // Flush any remaining logs
    while last_log_idx < state.logs.len() {
        let l = &state.logs[last_log_idx];
        println!(
            "{} [{}] {}",
            l.timestamp.format("%H:%M:%S"),
            l.level.tag(),
            l.message
        );
        last_log_idx += 1;
    }

    println!("\n------------------------------------------------------");
    if state.stage == Stage::Ready {
        println!("✅ SUCCESS: Target is online and ready!");
        Ok(())
    } else {
        println!("❌ FAILED: Pipeline ended in stage {:?}", state.stage);
        Err(state.status_message)
    }
}
