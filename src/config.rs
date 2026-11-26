//! Configuration management, host profiles, validation, and JSON serialization.
//!
//! This module defines the [`HostProfile`] and [`AppConfig`] data structures, atomic
//! JSON persistence routines, multi-profile schema migrations, and input validation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The current application configuration schema version.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// The default profile identifier used for unconfigured or newly initialized profiles.
pub const DEFAULT_PROFILE_NAME: &str = "workstation";

/// The default systemd user service name for Sunshine on Linux.
pub const DEFAULT_SUNSHINE_SERVICE: &str = "app-dev.lizardbyte.app.Sunshine.service";

/// The default SSH daemon port.
pub const DEFAULT_SSH_PORT: u16 = 22;

/// The default Sunshine web/streaming port.
pub const DEFAULT_SUNSHINE_PORT: u16 = 47989;

/// The default direct UDP Wake-on-LAN port (standard WoL ports are 9 or 7).
pub const DEFAULT_DIRECT_WOL_PORT: u16 = 9;

/// Default maximum duration (in seconds) to poll the target host before timing out.
pub const DEFAULT_MAX_WAIT_SECONDS: u64 = 300;

/// Default interval (in seconds) between periodic Wake-on-LAN packet re-transmissions.
pub const DEFAULT_WOL_RESEND_INTERVAL: u64 = 60;

/// Default grace duration (in seconds) to wait after Sunshine's port opens before launching Moonlight.
pub const DEFAULT_SUNSHINE_GRACE_SECONDS: u64 = 30;

/// Maximum allowable timeout (24 hours) to prevent integer overflow or indefinite hangs.
pub const MAX_ALLOWED_WAIT_SECONDS: u64 = 86400;

/// Maximum allowable re-transmission or grace interval (1 hour).
pub const MAX_ALLOWED_INTERVAL_SECONDS: u64 = 3600;

/// Domain error representing configuration, parsing, or validation failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// A required configuration field was empty.
    EmptyField(&'static str),
    /// A network port was outside the valid 1..=65535 range.
    InvalidPort { field: &'static str, value: u16 },
    /// An integer parameter was outside the allowed range.
    OutOfRange {
        field: &'static str,
        min: u64,
        max: u64,
        value: u64,
    },
    /// A MAC address failed format validation.
    InvalidMac(String),
    /// A hostname, IP address, or token contained disallowed shell characters.
    InvalidHostToken { field: &'static str, reason: String },
    /// A Pi gateway host was provided without a corresponding username.
    MissingPiUser,
    /// An underlying I/O error occurred during configuration load/save.
    IoError(String),
    /// JSON parsing or serialization failed.
    ParseError(String),
    /// Configuration was loaded with partial warnings (e.g. dropped corrupt profiles).
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

/// Configuration settings for a single target workstation and its wake pipeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HostProfile {
    /// Optional IP address or hostname of an intermediate Raspberry Pi or Linux gateway.
    #[serde(default = "default_pi_host")]
    pub pi_host: String,

    /// SSH username on the intermediate Pi gateway.
    #[serde(default = "default_pi_user")]
    pub pi_user: String,

    /// SSH port on the intermediate Pi gateway.
    #[serde(default = "default_pi_ssh_port")]
    pub pi_ssh_port: u16,

    /// Target workstation IP address or hostname.
    #[serde(default = "default_target_ip", alias = "target_ip")]
    pub thinkstation_ip: String,

    /// SSH username on the target workstation for remote power commands.
    #[serde(default = "default_target_user", alias = "target_user")]
    pub thinkstation_user: String,

    /// Target workstation network interface MAC address.
    #[serde(default = "default_mac_address")]
    pub mac_address: String,

    /// Subnet broadcast address for UDP Wake-on-LAN packets.
    #[serde(default = "default_broadcast_addr")]
    pub broadcast_addr: String,

    /// Target workstation SSH daemon port.
    #[serde(default = "default_ssh_port")]
    pub ssh_port: u16,

    /// When true, enforces strict SSH host key checking (`StrictHostKeyChecking=yes`).
    /// When false (default), uses `StrictHostKeyChecking=accept-new` for smooth setup.
    #[serde(default = "default_ssh_strict_host_key_checking")]
    pub ssh_strict_host_key_checking: bool,

    /// Target workstation Sunshine streaming service port.
    #[serde(default = "default_sunshine_port")]
    pub sunshine_port: u16,

    /// Sunshine systemd user service unit name on Linux.
    #[serde(default = "default_sunshine_service")]
    pub sunshine_service: String,

    /// Maximum duration (in seconds) to wait for the host to reach Ready state.
    #[serde(default = "default_max_wait_seconds")]
    pub max_wait_seconds: u64,

    /// Re-transmission interval (in seconds) for Wake-on-LAN magic packets.
    #[serde(default = "default_wol_resend_interval")]
    pub wol_resend_interval: u64,

    /// Post-Sunshine startup grace duration (in seconds) before launching Moonlight.
    #[serde(default = "default_sunshine_grace_seconds")]
    pub sunshine_grace_seconds: u64,

    /// If true, dispatches UDP magic packets directly from the local machine.
    #[serde(default = "default_direct_wol")]
    pub direct_wol: bool,

    /// UDP port used when sending direct Wake-on-LAN packets.
    #[serde(default = "default_direct_wol_port")]
    pub direct_wol_port: u16,

    /// If true, automatically closes the GUI window when the host reaches Ready.
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
fn default_target_ip() -> String {
    "192.168.1.50".to_string()
}
fn default_target_user() -> String {
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
fn default_ssh_strict_host_key_checking() -> bool {
    false
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
            thinkstation_ip: default_target_ip(),
            thinkstation_user: default_target_user(),
            mac_address: default_mac_address(),
            broadcast_addr: default_broadcast_addr(),
            ssh_port: default_ssh_port(),
            ssh_strict_host_key_checking: default_ssh_strict_host_key_checking(),
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

/// Top-level application configuration containing named host profiles and active selection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
    /// Schema format version for future-proof migrations.
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,

    /// Identifier of the currently selected active profile.
    #[serde(default = "default_active_profile")]
    pub active_profile: String,

    /// Map of profile names to their corresponding [`HostProfile`] configurations.
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
    /// Retrieves the active [`HostProfile`], falling back to the first available profile
    /// or default settings if the selected key is not found.
    pub fn get_active_profile(&self) -> HostProfile {
        if let Some(prof) = self.profiles.get(&self.active_profile) {
            prof.clone()
        } else if let Some((_, first_prof)) = self.profiles.iter().next() {
            first_prof.clone()
        } else {
            HostProfile::default()
        }
    }

    /// Inserts or updates a named [`HostProfile`].
    pub fn set_profile(&mut self, name: &str, profile: HostProfile) {
        self.profiles.insert(name.to_string(), profile);
    }

    /// Deletes a profile by name. Refuses deletion if only one profile remains.
    /// If the deleted profile was active, selects a remaining profile automatically.
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

/// Resolves the default operating-system-specific configuration path.
///
/// On Windows: `%APPDATA%\wake-workstation\config.json`
/// On Linux/macOS: `$HOME/.config/wake-workstation/config.json`
///
/// Supports automatic backward-compatibility fallback to `wake-thinkstation/config.json` if present.
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

/// Normalizes and validates a MAC address string into canonical `AA:BB:CC:DD:EE:FF` format.
///
/// Supports standard delimiters (`:`, `-`, `.`) or raw 12-digit hex strings.
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

/// Validates all parameters of a [`HostProfile`].
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

/// Validates that a target IP or hostname is non-empty and contains safe characters.
pub fn validate_target_ip(ip: &str) -> Result<(), String> {
    let trimmed = ip.trim();
    if trimmed.is_empty() {
        return Err("Target IP / Host cannot be empty".to_string());
    }
    crate::net::validate_host_token(trimmed, "Target IP / Host")
}

/// Validates a MAC address string.
pub fn validate_mac_address(mac: &str) -> Result<String, String> {
    normalize_mac_address(mac)
}

/// Validates a username token.
pub fn validate_username(user: &str) -> Result<(), String> {
    let trimmed = user.trim();
    if trimmed.is_empty() {
        return Err("Username cannot be empty".to_string());
    }
    crate::net::validate_host_token(trimmed, "Username")
}

/// Validates a broadcast IP token.
pub fn validate_broadcast_ip(bcast: &str) -> Result<(), String> {
    let trimmed = bcast.trim();
    if trimmed.is_empty() {
        return Err("Broadcast address cannot be empty".to_string());
    }
    crate::net::validate_host_token(trimmed, "Broadcast address")
}

/// Serializes an [`AppConfig`] to formatted JSON string.
pub fn export_config_json(config: &AppConfig) -> Result<String, String> {
    serde_json::to_string_pretty(config).map_err(|e| e.to_string())
}

/// Migrates JSON configuration payloads from earlier schema versions to [`CURRENT_SCHEMA_VERSION`].
pub fn migrate_config_value(value: &mut serde_json::Value) -> Result<bool, ConfigError> {
    let mut modified = false;
    if let Some(obj) = value.as_object_mut() {
        // Upgrade missing or older schema_version
        let version = obj
            .get("schema_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        if version < CURRENT_SCHEMA_VERSION {
            obj.insert(
                "schema_version".to_string(),
                serde_json::Value::from(CURRENT_SCHEMA_VERSION),
            );
            modified = true;
        }

        // Migrate legacy profile name "thinkstation" to "workstation"
        if let Some(serde_json::Value::String(active)) = obj.get_mut("active_profile") {
            if active == "thinkstation" {
                *active = DEFAULT_PROFILE_NAME.to_string();
                modified = true;
            }
        }

        // Migrate legacy profile dictionary keys if needed
        if let Some(serde_json::Value::Object(profiles)) = obj.get_mut("profiles") {
            if profiles.contains_key("thinkstation") && !profiles.contains_key(DEFAULT_PROFILE_NAME)
            {
                if let Some(val) = profiles.remove("thinkstation") {
                    profiles.insert(DEFAULT_PROFILE_NAME.to_string(), val);
                    modified = true;
                }
            }
        }
    }
    Ok(modified)
}

/// Deserializes an [`AppConfig`] from JSON, applying schema migrations and collecting diagnostic warnings.
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

    // 3. Try parsing generic JSON Value with schema migration & salvage diagnostics
    if let Ok(mut val) = serde_json::from_str::<serde_json::Value>(trimmed) {
        let _ = migrate_config_value(&mut val);

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

/// Deserializes an [`AppConfig`] from JSON string.
pub fn import_config_json(json_str: &str) -> Result<AppConfig, String> {
    import_config_json_with_warnings(json_str).map(|(cfg, _)| cfg)
}

/// Loads configuration from a file path with optional profile override.
///
/// If the file does not exist, writes default configuration and returns it.
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

/// Atomically saves an [`AppConfig`] to the specified file path using a temporary file and atomic rename.
///
/// On Unix platforms, enforces secure `0600` permissions on the written configuration file.
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
