use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HostProfile {
    #[serde(default = "default_pi_host")]
    pub pi_host: String,
    #[serde(default = "default_pi_user")]
    pub pi_user: String,
    #[serde(default = "default_thinkstation_ip")]
    pub thinkstation_ip: String,
    #[serde(default = "default_thinkstation_user")]
    pub thinkstation_user: String,
    #[serde(default = "default_mac_address")]
    pub mac_address: String,
    #[serde(default = "default_broadcast_addr")]
    pub broadcast_addr: String,
    #[serde(default = "default_ssh_port")]
    pub ssh_port: u16,
    #[serde(default = "default_sunshine_port")]
    pub sunshine_port: u16,
    #[serde(default = "default_sunshine_service")]
    pub sunshine_service: String,
    #[serde(default = "default_max_wait_seconds")]
    pub max_wait_seconds: u64,
    #[serde(default = "default_wol_resend_interval")]
    pub wol_resend_interval: u64,
    #[serde(default = "default_sunshine_grace_seconds")]
    pub sunshine_grace_seconds: u64,
    #[serde(default = "default_direct_wol")]
    pub direct_wol: bool,
    #[serde(default = "default_direct_wol_port")]
    pub direct_wol_port: u16,
    #[serde(default = "default_auto_close")]
    pub auto_close: bool,
}

fn default_pi_host() -> String {
    // Placeholder example address (RFC 5737 / documentation-style). Replace
    // with your own Raspberry Pi gateway's LAN or Tailscale IP.
    "192.168.1.10".to_string()
}
fn default_pi_user() -> String {
    "pi".to_string()
}
fn default_thinkstation_ip() -> String {
    // Placeholder example address. Replace with your own target host's IP.
    "192.168.1.50".to_string()
}
fn default_thinkstation_user() -> String {
    "user".to_string()
}
fn default_mac_address() -> String {
    // Placeholder documentation-style MAC (IEEE reserved AA:BB:CC prefix).
    "AA:BB:CC:DD:EE:FF".to_string()
}
fn default_broadcast_addr() -> String {
    "192.168.1.255".to_string()
}
fn default_ssh_port() -> u16 {
    22
}
fn default_sunshine_port() -> u16 {
    47989
}
fn default_sunshine_service() -> String {
    "app-dev.lizardbyte.app.Sunshine.service".to_string()
}
fn default_max_wait_seconds() -> u64 {
    300
}
fn default_wol_resend_interval() -> u64 {
    60
}
fn default_sunshine_grace_seconds() -> u64 {
    30
}
fn default_direct_wol() -> bool {
    false
}
fn default_direct_wol_port() -> u16 {
    9
}
fn default_auto_close() -> bool {
    false
}

impl Default for HostProfile {
    fn default() -> Self {
        Self {
            pi_host: default_pi_host(),
            pi_user: default_pi_user(),
            thinkstation_ip: default_thinkstation_ip(),
            thinkstation_user: default_thinkstation_user(),
            mac_address: default_mac_address(),
            broadcast_addr: default_broadcast_addr(),
            ssh_port: default_ssh_port(),
            sunshine_port: default_sunshine_port(),
            sunshine_service: default_sunshine_service(),
            max_wait_seconds: default_max_wait_seconds(),
            wol_resend_interval: default_wol_resend_interval(),
            sunshine_grace_seconds: default_sunshine_grace_seconds(),
            direct_wol: default_direct_wol(),
            direct_wol_port: default_direct_wol_port(),
            auto_close: default_auto_close(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
    #[serde(default = "default_active_profile")]
    pub active_profile: String,
    #[serde(default)]
    pub profiles: HashMap<String, HostProfile>,
}

fn default_active_profile() -> String {
    "thinkstation".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        let mut profiles = HashMap::new();
        profiles.insert("thinkstation".to_string(), HostProfile::default());
        Self {
            active_profile: "thinkstation".to_string(),
            profiles,
        }
    }
}

impl AppConfig {
    pub fn get_active_profile(&self) -> HostProfile {
        if let Some(prof) = self.profiles.get(&self.active_profile) {
            prof.clone()
        } else if let Some((_, first_prof)) = self.profiles.iter().next() {
            first_prof.clone()
        } else {
            HostProfile::default()
        }
    }

    pub fn set_profile(&mut self, name: &str, profile: HostProfile) {
        self.profiles.insert(name.to_string(), profile);
    }
}

pub fn default_config_path() -> PathBuf {
    if cfg!(windows) {
        if let Ok(appdata) = std::env::var("APPDATA") {
            PathBuf::from(appdata).join("wake-thinkstation").join("config.json")
        } else if let Ok(userprofile) = std::env::var("USERPROFILE") {
            PathBuf::from(userprofile).join(".config").join("wake-thinkstation").join("config.json")
        } else {
            PathBuf::from("config.json")
        }
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config").join("wake-thinkstation").join("config.json")
    } else {
        PathBuf::from(".config/wake-thinkstation/config.json")
    }
}

pub fn validate_settings(settings: &HostProfile) -> Result<(), String> {
    let target_ip = settings.thinkstation_ip.trim();
    if target_ip.is_empty() {
        return Err("Target IP / Host cannot be empty".to_string());
    }

    let mac = settings.mac_address.trim();
    if mac.is_empty() {
        return Err("MAC address cannot be empty".to_string());
    }

    if mac.starts_with(':')
        || mac.ends_with(':')
        || mac.starts_with('-')
        || mac.ends_with('-')
        || mac.starts_with('.')
        || mac.ends_with('.')
    {
        return Err(format!(
            "Invalid leading or trailing delimiter in MAC address '{}'.",
            mac
        ));
    }

    for c in mac.chars() {
        if !c.is_ascii_hexdigit() && c != ':' && c != '-' && c != '.' && !c.is_whitespace() {
            return Err(format!(
                "Invalid character '{}' in MAC address '{}'.",
                c, mac
            ));
        }
    }

    let clean_mac: String = mac.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if clean_mac.len() != 12 {
        return Err(format!(
            "Invalid MAC address format '{}'. Must contain 12 hexadecimal characters.",
            mac
        ));
    }

    // These values ultimately get interpolated into SSH targets and, in the
    // case of broadcast_addr/mac_address, into a command string executed by
    // the *remote* gateway's shell (see net::send_ssh_wol). Restrict them to
    // a safe character allow-list here so a malicious/corrupted config.json
    // can't smuggle shell metacharacters through to that remote shell.
    crate::net::validate_host_token(target_ip, "Target IP / Host")?;
    crate::net::validate_host_token(settings.thinkstation_user.trim(), "Target SSH user")?;
    crate::net::validate_host_token(settings.broadcast_addr.trim(), "Broadcast address")?;
    if !settings.pi_host.trim().is_empty() {
        crate::net::validate_host_token(settings.pi_host.trim(), "Pi gateway host")?;
        crate::net::validate_host_token(settings.pi_user.trim(), "Pi gateway user")?;
    }

    if settings.ssh_port == 0 {
        return Err("SSH port must be between 1 and 65535".to_string());
    }
    if settings.sunshine_port == 0 {
        return Err("Sunshine port must be between 1 and 65535".to_string());
    }
    if settings.direct_wol_port == 0 {
        return Err("Direct WOL port must be between 1 and 65535".to_string());
    }

    if settings.max_wait_seconds == 0 {
        return Err("max_wait_seconds must be a positive integer".to_string());
    }
    if settings.wol_resend_interval == 0 {
        return Err("wol_resend_interval must be a positive integer".to_string());
    }

    Ok(())
}

pub fn load_config_from_path(
    config_file: &Path,
    profile_override: Option<&str>,
) -> (AppConfig, HostProfile) {
    if !config_file.exists() {
        let default_app_config = AppConfig::default();
        let _ = save_config_to_path(&default_app_config, config_file);
        let active = default_app_config.get_active_profile();
        return (default_app_config, active);
    }

    let data = match fs::read_to_string(config_file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Warning: Could not read config file {}: {}. Using defaults.", config_file.display(), e);
            let default_app = AppConfig::default();
            let active = default_app.get_active_profile();
            return (default_app, active);
        }
    };

    // Try parsing as multi-profile AppConfig first
    if let Ok(mut app_cfg) = serde_json::from_str::<AppConfig>(&data) {
        if !app_cfg.profiles.is_empty() {
            if let Some(prof_name) = profile_override {
                if app_cfg.profiles.contains_key(prof_name) {
                    app_cfg.active_profile = prof_name.to_string();
                } else {
                    eprintln!(
                        "Warning: Profile '{}' not found in {}. Falling back to '{}'.",
                        prof_name,
                        config_file.display(),
                        app_cfg.active_profile
                    );
                }
            }
            let active = app_cfg.get_active_profile();
            return (app_cfg, active);
        }
    }

    // Try parsing as legacy flat HostProfile
    if let Ok(flat_profile) = serde_json::from_str::<HostProfile>(&data) {
        let active_name = profile_override.unwrap_or("thinkstation").to_string();
        let mut profiles = HashMap::new();
        profiles.insert(active_name.clone(), flat_profile.clone());
        let app_cfg = AppConfig {
            active_profile: active_name,
            profiles,
        };
        return (app_cfg, flat_profile);
    }

    // Try parsing generic JSON Value to salvage fields
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data) {
        if let Some(obj) = val.as_object() {
            if let Some(profiles_val) = obj.get("profiles").and_then(|v| v.as_object()) {
                let mut profiles = HashMap::new();
                for (k, v) in profiles_val {
                    if let Ok(hp) = serde_json::from_value::<HostProfile>(v.clone()) {
                        profiles.insert(k.clone(), hp);
                    }
                }
                let active = obj
                    .get("active_profile")
                    .and_then(|v| v.as_str())
                    .unwrap_or("thinkstation")
                    .to_string();
                let chosen_active = profile_override.unwrap_or(&active).to_string();
                let app_cfg = AppConfig {
                    active_profile: chosen_active,
                    profiles,
                };
                let active_prof = app_cfg.get_active_profile();
                return (app_cfg, active_prof);
            }
        }
    }

    eprintln!(
        "Warning: Could not parse config in {}. Using defaults.",
        config_file.display()
    );
    let default_app = AppConfig::default();
    let active = default_app.get_active_profile();
    (default_app, active)
}

pub fn save_config_to_path(config: &AppConfig, config_file: &Path) -> Result<(), String> {
    if let Some(parent) = config_file.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }

    let json_text = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(config_file, json_text).map_err(|e| e.to_string())?;

    // Restrict the config file to the owning user on Unix (it contains LAN
    // topology: hostnames, IPs, MAC addresses, and SSH usernames). Windows
    // relies on the per-user ACLs already applied to %APPDATA% instead.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(config_file) {
            let mut perms = metadata.permissions();
            perms.set_mode(0o600);
            let _ = fs::set_permissions(config_file, perms);
        }
    }

    Ok(())
}
