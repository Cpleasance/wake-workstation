// Rust links Windows binaries as console-subsystem executables by default,
// so double-clicking the .exe pops a console window instead of going
// straight to the GUI. Suppress that in release builds only, so `--cli`/
// `--power` output (and eprintln! during development) still show up when
// run from an existing terminal.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod net;
mod pipeline;
mod ui;

#[cfg(test)]
mod tests;

// On hybrid-GPU laptops (integrated + discrete), Windows' automatic GPU
// selection for an unrecognized/freshly-built executable can route it to a
// GPU/driver combination where eframe's OpenGL (WGL) context negotiation
// misbehaves -- observed here as the window being created but permanently
// stuck at a degenerate ~5x5px size instead of its requested dimensions,
// with no error reported. These exported symbols are the standard NVIDIA
// Optimus / AMD PowerXpress hints that tell the OS to always run this
// process on the high-performance discrete GPU instead of leaving the
// choice to the driver's own heuristic.
#[cfg(windows)]
#[no_mangle]
pub static NvOptimusEnablement: i32 = 1;
#[cfg(windows)]
#[no_mangle]
pub static AmdPowerXpressRequestHighPerformance: i32 = 1;

use clap::Parser;
use config::{default_config_path, load_config_from_path, validate_settings, HostProfile};
use eframe::egui;
use net::execute_power_action;
use pipeline::run_cli_pipeline;
use std::path::PathBuf;
use ui::WakeApp;

#[derive(Parser, Debug, Default, Clone)]
#[command(
    name = "wake-workstation",
    about = "Wake-on-LAN and Moonlight Stream orchestrator for Remote Workstations & Gaming PCs",
    version
)]
pub struct CliArgs {
    /// Run in terminal CLI mode without GUI
    #[arg(long)]
    pub cli: bool,

    /// Configuration profile to activate (e.g. thinkstation, server)
    #[arg(short, long)]
    pub profile: Option<String>,

    /// Execute immediate remote power command (suspend, reboot, shutdown) and exit
    #[arg(long)]
    pub power: Option<String>,

    /// Override target IP / hostname
    #[arg(long, visible_alias = "ip")]
    pub target: Option<String>,

    /// Override target SSH username
    #[arg(long)]
    pub user: Option<String>,

    /// Override target MAC address
    #[arg(long)]
    pub mac: Option<String>,

    /// Override broadcast IP address
    #[arg(long)]
    pub broadcast: Option<String>,

    /// Override Pi gateway host
    #[arg(long)]
    pub pi_host: Option<String>,

    /// Override Pi gateway user
    #[arg(long)]
    pub pi_user: Option<String>,

    /// Override Pi gateway SSH port
    #[arg(long)]
    pub pi_ssh_port: Option<u16>,

    /// Force direct local UDP Wake-on-LAN
    #[arg(long)]
    pub direct_wol: bool,

    /// Override direct WOL port
    #[arg(long)]
    pub direct_wol_port: Option<u16>,

    /// Override SSH port
    #[arg(long)]
    pub ssh_port: Option<u16>,

    /// Override Sunshine port
    #[arg(long)]
    pub sunshine_port: Option<u16>,

    /// Override maximum wait timeout in seconds
    #[arg(long, visible_alias = "max-wait-seconds")]
    pub max_wait: Option<u64>,

    /// Override WOL resend interval in seconds
    #[arg(long, visible_alias = "wol-resend-interval")]
    pub wol_resend: Option<u64>,

    /// Do not automatically launch Moonlight when host becomes ready
    #[arg(long)]
    pub no_moonlight: bool,

    /// Auto-close GUI window once target is ready
    #[arg(long)]
    pub auto_close: bool,

    /// Path to custom JSON configuration file
    #[arg(short, long)]
    pub config: Option<PathBuf>,
}

pub fn apply_cli_overrides(mut profile: HostProfile, args: &CliArgs) -> HostProfile {
    if let Some(target) = &args.target {
        profile.thinkstation_ip = target.clone();
    }
    if let Some(user) = &args.user {
        profile.thinkstation_user = user.clone();
    }
    if let Some(mac) = &args.mac {
        profile.mac_address = mac.clone();
    }
    if let Some(bcast) = &args.broadcast {
        profile.broadcast_addr = bcast.clone();
    }
    if let Some(pi_h) = &args.pi_host {
        profile.pi_host = pi_h.clone();
    }
    if let Some(pi_u) = &args.pi_user {
        profile.pi_user = pi_u.clone();
    }
    if let Some(pi_sp) = args.pi_ssh_port {
        profile.pi_ssh_port = pi_sp;
    }
    if args.direct_wol {
        profile.direct_wol = true;
    }
    if let Some(dwp) = args.direct_wol_port {
        profile.direct_wol_port = dwp;
    }
    if let Some(sp) = args.ssh_port {
        profile.ssh_port = sp;
    }
    if let Some(sun_p) = args.sunshine_port {
        profile.sunshine_port = sun_p;
    }
    if let Some(mw) = args.max_wait {
        profile.max_wait_seconds = mw;
    }
    if let Some(wri) = args.wol_resend {
        profile.wol_resend_interval = wri;
    }
    if args.auto_close {
        profile.auto_close = true;
    }
    profile
}

/// Constructs the optimized desktop GUI `NativeOptions` with balanced viewport
/// sizing (800x600 default, 520x360 minimum) and robust GPU/rendering settings
/// for smooth window resizing, split-screen snapping, and high-DPI scaling.
pub fn default_native_options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Wake Workstation")
            .with_app_id("wake-workstation")
            .with_inner_size([800.0, 600.0])
            .with_min_inner_size([520.0, 360.0])
            .with_resizable(true)
            .with_active(true)
            .with_decorations(true)
            .with_transparent(false)
            .with_maximize_button(true)
            .with_minimize_button(true)
            .with_close_button(true)
            .with_drag_and_drop(false),
        vsync: true,
        multisampling: 0,
        depth_buffer: 0,
        stencil_buffer: 0,
        hardware_acceleration: eframe::HardwareAcceleration::Preferred,
        renderer: eframe::Renderer::Glow,
        centered: true,
        persist_window: false,
        dithering: true,
        ..Default::default()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = CliArgs::parse();

    let config_path = args
        .config
        .clone()
        .unwrap_or_else(default_config_path);

    let (mut app_cfg, mut active_profile) = load_config_from_path(
        &config_path,
        args.profile.as_deref(),
    );

    active_profile = apply_cli_overrides(active_profile, &args);

    if let Err(err) = validate_settings(&active_profile) {
        eprintln!("Configuration Error: {}", err);
        std::process::exit(1);
    }

    app_cfg.set_profile(&app_cfg.active_profile.clone(), active_profile.clone());

    // Immediate remote power command execution
    if let Some(ref action) = args.power {
        println!(
            "Executing remote power action '{}' on {}@{} (port {})...",
            action,
            active_profile.thinkstation_user,
            active_profile.thinkstation_ip,
            active_profile.ssh_port
        );
        match execute_power_action(
            &active_profile.thinkstation_ip,
            &active_profile.thinkstation_user,
            action,
            active_profile.ssh_port,
        ) {
            Ok(msg) => {
                println!("✅ Success: {}", msg);
                return Ok(());
            }
            Err(err) => {
                eprintln!("❌ Error: {}", err);
                std::process::exit(1);
            }
        }
    }

    let is_headless = if cfg!(target_os = "linux") {
        std::env::var("DISPLAY").is_err() && std::env::var("WAYLAND_DISPLAY").is_err()
    } else {
        false
    };

    if args.cli || is_headless {
        if is_headless && !args.cli {
            println!("Notice: No graphical display detected (DISPLAY/WAYLAND_DISPLAY not set). Running in CLI mode.");
        }
        let auto_launch = !args.no_moonlight;
        if let Err(err) = run_cli_pipeline(active_profile, auto_launch) {
            eprintln!("Pipeline terminated with error: {}", err);
            std::process::exit(1);
        }
        return Ok(());
    }

    // Launch Desktop GUI with eframe
    let native_options = default_native_options();

    let auto_launch = !args.no_moonlight;
    eframe::run_native(
        "Wake Workstation",
        native_options,
        Box::new(move |cc| {
            Ok(Box::new(WakeApp::new(
                cc,
                app_cfg,
                config_path,
                auto_launch,
            )))
        }),
    )
    .map_err(|e| format!("GUI Error: {}", e))?;

    Ok(())
}
