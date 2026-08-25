use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const CURRENT_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_PROFILE_NAME: &str = "workstation";
pub const DEFAULT_SUNSHINE_SERVICE: &str = "app-dev.lizardbyte.app.Sunshine.service";
pub const DEFAULT_SSH_PORT: u16 = 22;
pub const DEFAULT_SUNSHINE_PORT: u16 = 47989;
pub const DEFAULT_DIRECT_WOL_PORT: u16 = 9;
pub const DEFAULT_MAX_WAIT_SECONDS: u64 = 300;
pub const DEFAULT_WOL_RESEND_INTERVAL: u64 = 60;
pub const DEFAULT_SUNSHINE_GRACE_SECONDS: u64 = 30;
pub const MAX_ALLOWED_WAIT_SECONDS: u64 = 86400; // 24 hours
pub const MAX_ALLOWED_INTERVAL_SECONDS: u64 = 3600; // 1 hour

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    EmptyField(&'static str),
    InvalidPort {
        field: &'static str,
        value: u16,
    },
    OutOfRange {
        field: &'static str,
        min: u64,
        max: u64,
        value: u64,
    },
    InvalidMac(String),
    InvalidHostToken {
        field: &'static str,
        reason: String,
    },
    MissingPiUser,
    IoError(String),
    ParseError(String),
    PartialImportWarnings(Vec<String>),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyField(field) => write!(f, "{} cannot be empty", field),
            Self::InvalidPort { field, value } => {
                write!(
                    f,
                    "{} port {} is invalid (must be between 1 and 65535)",
                    field, value
                )
            }
            Self::OutOfRange {
                field,
                min,
                max,
                value,
            } => {
                write!(
                    f,
                    "{} value {} is out of allowed range ({} to {})",
                    field, value, min, max
                )
            }
            Self::InvalidMac(reason) => write!(f, "Invalid MAC address: {}", reason),
            Self::InvalidHostToken { field, reason } => write!(f, "Invalid {}: {}", field, reason),
            Self::MissingPiUser => write!(
                f,
                "Pi gateway user cannot be empty when Pi gateway host is specified"
            ),
            Self::IoError(msg) => write!(f, "I/O error: {}", msg),
            Self::ParseError(msg) => write!(f, "Parse error: {}", msg),
            Self::PartialImportWarnings(warnings) => {
                write!(f, "Import warnings: {}", warnings.join("; "))
            }
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<ConfigError> for String {
    fn from(err: ConfigError) -> Self {
        err.to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HostProfile {
    #[serde(default = "default_pi_host")]
    pub pi_host: String,
    #[serde(default = "default_pi_user")]
    pub pi_user: String,
    #[serde(default = "default_pi_ssh_port")]
    pub pi_ssh_port: u16,
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
    "192.168.1.10".to_string()
}
fn default_pi_user() -> String {
    "pi".to_string()
}
fn default_pi_ssh_port() -> u16 {
    DEFAULT_SSH_PORT
}
fn default_thinkstation_ip() -> String {
    "192.168.1.50".to_string()
}
fn default_thinkstation_user() -> String {
    "user".to_string()
}
fn default_mac_address() -> String {
    "AA:BB:CC:DD:EE:FF".to_string()
}
fn default_broadcast_addr() -> String {
    "192.168.1.255".to_string()
}
fn default_ssh_port() -> u16 {
    DEFAULT_SSH_PORT
}
fn default_sunshine_port() -> u16 {
    DEFAULT_SUNSHINE_PORT
}
fn default_sunshine_service() -> String {
    DEFAULT_SUNSHINE_SERVICE.to_string()
}
fn default_max_wait_seconds() -> u64 {
    DEFAULT_MAX_WAIT_SECONDS
}
fn default_wol_resend_interval() -> u64 {
    DEFAULT_WOL_RESEND_INTERVAL
}
fn default_sunshine_grace_seconds() -> u64 {
    DEFAULT_SUNSHINE_GRACE_SECONDS
}
fn default_direct_wol() -> bool {
    false
}
fn default_direct_wol_port() -> u16 {
    DEFAULT_DIRECT_WOL_PORT
}
fn default_auto_close() -> bool {
    false
}

impl Default for HostProfile {
    fn default() -> Self {
        Self {
            pi_host: default_pi_host(),
            pi_user: default_pi_user(),
            pi_ssh_port: default_pi_ssh_port(),
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
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default = "default_active_profile")]
    pub active_profile: String,
    #[serde(default)]
    pub profiles: HashMap<String, HostProfile>,
}

fn default_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

fn default_active_profile() -> String {
    DEFAULT_PROFILE_NAME.to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        let mut profiles = HashMap::new();
        profiles.insert(DEFAULT_PROFILE_NAME.to_string(), HostProfile::default());
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            active_profile: DEFAULT_PROFILE_NAME.to_string(),
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

    pub fn delete_profile(&mut self, name: &str) -> bool {
        if self.profiles.len() <= 1 {
            return false;
        }
        if self.profiles.remove(name).is_some() {
            if self.active_profile == name {
                if let Some(next_name) = self.profiles.keys().next().cloned() {
                    self.active_profile = next_name;
                }
            }
            true
        } else {
            false
        }
    }
}

pub fn default_config_path() -> PathBuf {
    let check_candidate = |base: &Path| -> PathBuf {
        let p_new = base.join("wake-workstation").join("config.json");
        let p_old = base.join("wake-thinkstation").join("config.json");
        if p_old.exists() && !p_new.exists() {
            p_old
        } else {
            p_new
        }
    };

    if cfg!(windows) {
        if let Ok(appdata) = std::env::var("APPDATA") {
            check_candidate(Path::new(&appdata))
        } else if let Ok(userprofile) = std::env::var("USERPROFILE") {
            check_candidate(&PathBuf::from(userprofile).join(".config"))
        } else {
            PathBuf::from("config.json")
        }
    } else if let Ok(home) = std::env::var("HOME") {
        check_candidate(&PathBuf::from(home).join(".config"))
    } else {
        PathBuf::from(".config/wake-workstation/config.json")
    }
}

pub fn normalize_mac_address(mac: &str) -> Result<String, String> {
    let trimmed = mac.trim();
    if trimmed.is_empty() {
        return Err("MAC address cannot be empty".to_string());
    }

    if trimmed.starts_with(':')
        || trimmed.ends_with(':')
        || trimmed.starts_with('-')
        || trimmed.ends_with('-')
        || trimmed.starts_with('.')
        || trimmed.ends_with('.')
    {
        return Err(format!(
            "Invalid leading or trailing delimiter in MAC address '{}'.",
            trimmed
        ));
    }

    for c in trimmed.chars() {
        if !c.is_ascii_hexdigit() && c != ':' && c != '-' && c != '.' && !c.is_whitespace() {
            return Err(format!(
                "Invalid character '{}' in MAC address '{}'.",
                c, trimmed
            ));
        }
    }

    let clean: String = trimmed.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if clean.len() != 12 {
        return Err(format!(
            "Invalid MAC address format '{}'. Must contain 12 hexadecimal characters.",
            trimmed
        ));
    }

    let normalized = format!(
        "{}:{}:{}:{}:{}:{}",
        clean[0..2].to_ascii_uppercase(),
        clean[2..4].to_ascii_uppercase(),
        clean[4..6].to_ascii_uppercase(),
        clean[6..8].to_ascii_uppercase(),
        clean[8..10].to_ascii_uppercase(),
        clean[10..12].to_ascii_uppercase(),
    );
    Ok(normalized)
}

pub fn validate_settings(settings: &HostProfile) -> Result<(), String> {
    let target_ip = settings.thinkstation_ip.trim();
    if target_ip.is_empty() {
        return Err("Target IP / Host cannot be empty".to_string());
    }
    crate::net::validate_host_token(target_ip, "Target IP / Host")?;

    normalize_mac_address(&settings.mac_address)?;

    crate::net::validate_host_token(settings.thinkstation_user.trim(), "Target SSH user")?;
    crate::net::validate_host_token(settings.broadcast_addr.trim(), "Broadcast address")?;

    // Contract: Pi gateway host is optional. If specified, Pi user is required and validated.
    let pi_host = settings.pi_host.trim();
    let pi_user = settings.pi_user.trim();
    if !pi_host.is_empty() {
        crate::net::validate_host_token(pi_host, "Pi gateway host")?;
        if pi_user.is_empty() {
            return Err(
                "Pi gateway user cannot be empty when Pi gateway host is specified".to_string(),
            );
        }
        crate::net::validate_host_token(pi_user, "Pi gateway user")?;
    }

    if !settings.sunshine_service.trim().is_empty() {
        crate::net::validate_host_token(settings.sunshine_service.trim(), "Sunshine service name")?;
    }

    // Ports range check: 1..=65535
    for (port, name) in [
        (settings.ssh_port, "SSH"),
        (settings.pi_ssh_port, "Pi gateway SSH"),
        (settings.sunshine_port, "Sunshine"),
        (settings.direct_wol_port, "Direct WOL"),
    ] {
        if !(1..=65535).contains(&port) {
            return Err(format!("{} port must be between 1 and 65535", name));
        }
    }

    // Interval and timeout upper and lower bounds
    if !(1..=MAX_ALLOWED_WAIT_SECONDS).contains(&settings.max_wait_seconds) {
        return Err(format!(
            "max_wait_seconds must be between 1 and {} seconds",
            MAX_ALLOWED_WAIT_SECONDS
        ));
    }
    if !(1..=MAX_ALLOWED_INTERVAL_SECONDS).contains(&settings.wol_resend_interval) {
        return Err(format!(
            "wol_resend_interval must be between 1 and {} seconds",
            MAX_ALLOWED_INTERVAL_SECONDS
        ));
    }
    if !(1..=MAX_ALLOWED_INTERVAL_SECONDS).contains(&settings.sunshine_grace_seconds) {
        return Err(format!(
            "sunshine_grace_seconds must be between 1 and {} seconds",
            MAX_ALLOWED_INTERVAL_SECONDS
        ));
    }

    Ok(())
}
pub fn validate_target_ip(ip: &str) -> Result<(), String> {
    let trimmed = ip.trim();
    if trimmed.is_empty() {
        return Err("Target IP / Host cannot be empty".to_string());
    }
    crate::net::validate_host_token(trimmed, "Target IP / Host")
}

pub fn validate_mac_address(mac: &str) -> Result<String, String> {
    normalize_mac_address(mac)
}

pub fn validate_username(user: &str) -> Result<(), String> {
    let trimmed = user.trim();
    if trimmed.is_empty() {
        return Err("Username cannot be empty".to_string());
    }
    crate::net::validate_host_token(trimmed, "Username")
}

pub fn validate_broadcast_ip(bcast: &str) -> Result<(), String> {
    let trimmed = bcast.trim();
    if trimmed.is_empty() {
        return Err("Broadcast address cannot be empty".to_string());
    }
    crate::net::validate_host_token(trimmed, "Broadcast address")
}

pub fn export_config_json(config: &AppConfig) -> Result<String, String> {
    serde_json::to_string_pretty(config).map_err(|e| e.to_string())
}

pub fn import_config_json_with_warnings(
    json_str: &str,
) -> Result<(AppConfig, Vec<String>), String> {
    let trimmed = json_str.trim();
    if trimmed.is_empty() {
        return Err("Configuration JSON string is empty".to_string());
    }

    let mut warnings = Vec::new();

    // 1. Try parsing as multi-profile AppConfig first
    if let Ok(mut app_cfg) = serde_json::from_str::<AppConfig>(trimmed) {
        if !app_cfg.profiles.is_empty() {
            if !app_cfg.profiles.contains_key(&app_cfg.active_profile) {
                if let Some(first_name) = app_cfg.profiles.keys().next().cloned() {
                    warnings.push(format!(
                        "Active profile '{}' not found in profiles map; defaulted to '{}'",
                        app_cfg.active_profile, first_name
                    ));
                    app_cfg.active_profile = first_name;
                }
            }
            return Ok((app_cfg, warnings));
        }
    }

    // 2. Try parsing as legacy flat HostProfile (only if it doesn't contain a "profiles" section)
    if !trimmed.contains("\"profiles\"") {
        if let Ok(flat_profile) = serde_json::from_str::<HostProfile>(trimmed) {
            let active_name = DEFAULT_PROFILE_NAME.to_string();
            let mut profiles = HashMap::new();
            profiles.insert(active_name.clone(), flat_profile);
            warnings.push("Imported legacy flat profile into multi-profile schema".to_string());
            return Ok((
                AppConfig {
                    schema_version: CURRENT_SCHEMA_VERSION,
                    active_profile: active_name,
                    profiles,
                },
                warnings,
            ));
        }
    }

    // 3. Try parsing generic JSON Value with salvage diagnostics
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(obj) = val.as_object() {
            if let Some(profiles_val) = obj.get("profiles").and_then(|v| v.as_object()) {
                let mut profiles = HashMap::new();
                for (k, v) in profiles_val {
                    match serde_json::from_value::<HostProfile>(v.clone()) {
                        Ok(hp) => {
                            profiles.insert(k.clone(), hp);
                        }
                        Err(e) => {
                            warnings.push(format!("Dropped invalid profile '{}': {}", k, e));
                        }
                    }
                }
                if !profiles.is_empty() {
                    let active = obj
                        .get("active_profile")
                        .and_then(|v| v.as_str())
                        .unwrap_or(DEFAULT_PROFILE_NAME)
                        .to_string();
                    let active_profile = if profiles.contains_key(&active) {
                        active
                    } else {
                        let first = profiles
                            .keys()
                            .next()
                            .cloned()
                            .unwrap_or_else(|| DEFAULT_PROFILE_NAME.to_string());
                        warnings.push(format!("Active profile fallback to '{}'", first));
                        first
                    };
                    return Ok((
                        AppConfig {
                            schema_version: CURRENT_SCHEMA_VERSION,
                            active_profile,
                            profiles,
                        },
                        warnings,
                    ));
                }
            }
        }
    }

    Err("Failed to parse configuration JSON: invalid format or structure".to_string())
}

pub fn import_config_json(json_str: &str) -> Result<AppConfig, String> {
    import_config_json_with_warnings(json_str).map(|(cfg, _)| cfg)
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
            eprintln!(
                "Warning: Could not read config file {}: {}. Using defaults.",
                config_file.display(),
                e
            );
            let default_app = AppConfig::default();
            let active = default_app.get_active_profile();
            return (default_app, active);
        }
    };

    match import_config_json(&data) {
        Ok(mut app_cfg) => {
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
            (app_cfg, active)
        }
        Err(_) => {
            eprintln!(
                "Warning: Could not parse config in {}. Using defaults.",
                config_file.display()
            );
            let default_app = AppConfig::default();
            let active = default_app.get_active_profile();
            (default_app, active)
        }
    }
}

pub fn save_config_to_path(config: &AppConfig, config_file: &Path) -> Result<(), String> {
    let parent = config_file.parent().unwrap_or_else(|| Path::new(""));
    if !parent.as_os_str().is_empty() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let json_text = export_config_json(config)?;

    let file_name = config_file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("config.json");
    let temp_file_name = format!("{}.tmp.{}", file_name, std::process::id());
    let temp_path = if parent.as_os_str().is_empty() {
        PathBuf::from(temp_file_name)
    } else {
        parent.join(temp_file_name)
    };

    if let Err(e) = fs::write(&temp_path, &json_text) {
        let _ = fs::remove_file(&temp_path);
        return Err(format!("Failed to write temporary config file: {}", e));
    }

    // Restrict the config file to the owning user on Unix (it contains LAN
    // topology: hostnames, IPs, MAC addresses, and SSH usernames). Windows
    // relies on the per-user ACLs already applied to %APPDATA% instead.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(&temp_path) {
            let mut perms = metadata.permissions();
            perms.set_mode(0o600);
            let _ = fs::set_permissions(&temp_path, perms);
        }
    }

    if let Err(e) = fs::rename(&temp_path, config_file) {
        let _ = fs::remove_file(&temp_path);
        return Err(format!(
            "Failed to atomically rename temp config file to {}: {}",
            config_file.display(),
            e
        ));
    }

    Ok(())
}
