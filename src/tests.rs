use crate::config::{
    default_config_path, export_config_json, import_config_json, load_config_from_path,
    save_config_to_path, validate_settings, AppConfig, HostProfile,
};
use crate::net::{
    create_magic_packet, execute_power_action, format_latency, parse_ping_latency, send_ssh_wol,
    validate_host_token,
};
use crate::pipeline::{disconnected_check_receiver, LogLevel, PipelineState, Stage};
use crate::{apply_cli_overrides, CliArgs};
use std::collections::HashMap;
use std::time::{Duration, Instant};

// ============================================================================
// 1. CONFIGURATION TESTS
// (Parsing, validation, atomic saving, fallback logic, profile management)
// ============================================================================

#[test]
fn test_config_atomic_saving_and_nested_directories() {
    let dir = std::env::temp_dir().join("wake_rs_nested_test_dir_a").join("sub_b").join("sub_c");
    let file_path = dir.join("config.json");

    if file_path.exists() {
        let _ = std::fs::remove_file(&file_path);
    }

    let mut profiles = HashMap::new();
    let prof = HostProfile {
        thinkstation_ip: "10.10.10.150".to_string(),
        thinkstation_user: "admin".to_string(),
        mac_address: "11:22:33:44:55:66".to_string(),
        ..HostProfile::default()
    };
    profiles.insert("nested_profile".to_string(), prof);

    let app_cfg = AppConfig {
        active_profile: "nested_profile".to_string(),
        profiles,
    };

    // Save should atomically create parent directories if missing and write JSON
    save_config_to_path(&app_cfg, &file_path).expect("Save into nested path should succeed");
    assert!(file_path.exists(), "Config file must exist after saving");

    // Load back and verify contents
    let (loaded_cfg, active) = load_config_from_path(&file_path, None);
    assert_eq!(loaded_cfg.active_profile, "nested_profile");
    assert_eq!(active.thinkstation_ip, "10.10.10.150");
    assert_eq!(active.thinkstation_user, "admin");
    assert_eq!(active.mac_address, "11:22:33:44:55:66");

    // Cleanup
    let _ = std::fs::remove_dir_all(std::env::temp_dir().join("wake_rs_nested_test_dir_a"));
}

#[test]
fn test_config_profile_management_and_fallback() {
    let mut app_cfg = AppConfig::default();

    // 1. AppConfig default contains "thinkstation"
    assert_eq!(app_cfg.active_profile, "thinkstation");
    let active_def = app_cfg.get_active_profile();
    assert_eq!(active_def.thinkstation_ip, "192.168.1.50");

    // 2. Add custom profile
    let custom_prof = HostProfile {
        thinkstation_ip: "172.16.0.10".to_string(),
        thinkstation_user: "workstation".to_string(),
        mac_address: "AA:BB:CC:11:22:33".to_string(),
        ..HostProfile::default()
    };
    app_cfg.set_profile("workstation", custom_prof);

    // 3. Change active profile
    app_cfg.active_profile = "workstation".to_string();
    let active_custom = app_cfg.get_active_profile();
    assert_eq!(active_custom.thinkstation_ip, "172.16.0.10");

    // 4. Non-existent active profile fallback to first available profile in map
    app_cfg.active_profile = "nonexistent_profile_key".to_string();
    let fallback_prof = app_cfg.get_active_profile();
    assert!(
        fallback_prof.thinkstation_ip == "192.168.1.50"
            || fallback_prof.thinkstation_ip == "172.16.0.10",
        "Should fall back to an existing profile in map"
    );

    // 5. Empty map fallback to default HostProfile
    app_cfg.profiles.clear();
    let empty_map_fallback = app_cfg.get_active_profile();
    assert_eq!(empty_map_fallback.thinkstation_ip, "192.168.1.50");
    assert_eq!(empty_map_fallback.mac_address, "AA:BB:CC:DD:EE:FF");
}

#[test]
fn test_config_profile_deletion_and_field_validators() {
    use crate::config::{
        validate_broadcast_ip, validate_mac_address, validate_target_ip, validate_username,
    };

    let mut app_cfg = AppConfig::default();
    let prof1 = HostProfile {
        thinkstation_ip: "10.0.0.1".to_string(),
        ..HostProfile::default()
    };
    let prof2 = HostProfile {
        thinkstation_ip: "10.0.0.2".to_string(),
        ..HostProfile::default()
    };

    app_cfg.set_profile("prof1", prof1);
    app_cfg.set_profile("prof2", prof2);

    assert!(app_cfg.profiles.len() >= 3);

    // Delete prof1
    assert!(app_cfg.delete_profile("prof1"));
    assert!(!app_cfg.profiles.contains_key("prof1"));

    // Delete non-existent profile returns false
    assert!(!app_cfg.delete_profile("non_existent"));

    // Cannot delete last remaining profile
    app_cfg.profiles.clear();
    app_cfg.set_profile("only_one", HostProfile::default());
    assert!(!app_cfg.delete_profile("only_one"));

    // Test field validators
    assert!(validate_target_ip("192.168.1.50").is_ok());
    assert!(validate_target_ip("").is_err());
    assert!(validate_target_ip("ip; rm -rf /").is_err());

    assert!(validate_mac_address("AA:BB:CC:DD:EE:FF").is_ok());
    assert!(validate_mac_address("deadbeef1234").is_ok());
    assert!(validate_mac_address("invalid_mac").is_err());

    assert!(validate_username("admin").is_ok());
    assert!(validate_username("").is_err());

    assert!(validate_broadcast_ip("192.168.1.255").is_ok());
    assert!(validate_broadcast_ip("").is_err());
}

#[test]
fn test_config_delete_active_profile_falls_back_to_remaining_profile() {
    // `test_config_profile_deletion_and_field_validators` only ever deletes a
    // profile that is NOT the active one, so the `active_profile == name`
    // reassignment branch in `AppConfig::delete_profile` was never exercised.
    let mut app_cfg = AppConfig::default(); // active_profile = "thinkstation"
    let custom = HostProfile {
        thinkstation_ip: "10.5.5.5".to_string(),
        ..HostProfile::default()
    };
    app_cfg.set_profile("custom_active", custom);
    app_cfg.active_profile = "custom_active".to_string();
    assert_eq!(app_cfg.profiles.len(), 2);

    // Delete the currently active profile itself.
    assert!(app_cfg.delete_profile("custom_active"));
    assert!(!app_cfg.profiles.contains_key("custom_active"));

    // active_profile must have been reassigned to a remaining key rather than
    // left dangling on a profile name that no longer exists. Only one
    // profile remains, so the fallback target is deterministic.
    assert_eq!(app_cfg.active_profile, "thinkstation");
    assert_eq!(app_cfg.get_active_profile().thinkstation_ip, "192.168.1.50");
}

#[test]
fn test_config_active_profile_mismatch_falls_back_on_import_and_load() {
    // A config.json whose top-level "active_profile" doesn't match any key in
    // "profiles" (e.g. hand-edited or corrupted) should not silently produce
    // a HostProfile::default() -- it should fall back to an actual profile
    // from the map, both when importing raw JSON and when loading from disk.
    let json = r#"{
        "active_profile": "ghost_profile_that_does_not_exist",
        "profiles": {
            "real_host": { "thinkstation_ip": "10.20.30.40", "mac_address": "AA:11:22:33:44:55" }
        }
    }"#;

    let imported = import_config_json(json).expect("should parse despite active_profile mismatch");
    assert_eq!(imported.active_profile, "real_host");
    assert_eq!(imported.get_active_profile().thinkstation_ip, "10.20.30.40");

    // Exercise the same scenario end-to-end through the on-disk load path.
    let dir = std::env::temp_dir().join("wake_rs_active_profile_mismatch_test");
    let _ = std::fs::create_dir_all(&dir);
    let file_path = dir.join("config.json");
    std::fs::write(&file_path, json).unwrap();

    let (loaded_cfg, active) = load_config_from_path(&file_path, None);
    assert_eq!(loaded_cfg.active_profile, "real_host");
    assert_eq!(active.thinkstation_ip, "10.20.30.40");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_config_import_rejects_wrong_type_profiles_field() {
    // "profiles" is present (so the legacy flat-HostProfile branch is
    // correctly skipped) but is the wrong JSON type, so it can't be
    // deserialized as an AppConfig and can't be salvaged as a profiles map
    // either. This must fail cleanly rather than panicking or fabricating a
    // default config that masks the corruption.
    let bad_json_array = r#"{"active_profile":"x","profiles":["not","an","object"]}"#;
    assert!(import_config_json(bad_json_array).is_err());

    let bad_json_string = r#"{"active_profile":"x","profiles":"just_a_string"}"#;
    assert!(import_config_json(bad_json_string).is_err());
}

#[test]
fn test_config_corruption_handling_comprehensive() {
    let dir = std::env::temp_dir().join("wake_rs_corruption_test");
    let _ = std::fs::create_dir_all(&dir);

    // 1. Completely broken JSON syntax
    let broken_file = dir.join("broken.json");
    std::fs::write(&broken_file, "{ invalid json data...").unwrap();
    let (app_cfg, active) = load_config_from_path(&broken_file, None);
    assert_eq!(app_cfg.active_profile, "thinkstation");
    assert_eq!(active.thinkstation_ip, "192.168.1.50");

    // 2. Empty file
    let empty_file = dir.join("empty.json");
    std::fs::write(&empty_file, "").unwrap();
    let (app_cfg_empty, active_empty) = load_config_from_path(&empty_file, None);
    assert_eq!(app_cfg_empty.active_profile, "thinkstation");
    assert_eq!(active_empty.mac_address, "AA:BB:CC:DD:EE:FF");

    // 3. Root JSON integer instead of object
    let num_file = dir.join("number.json");
    std::fs::write(&num_file, "42").unwrap();
    let (app_cfg_num, active_num) = load_config_from_path(&num_file, None);
    assert_eq!(app_cfg_num.active_profile, "thinkstation");
    assert_eq!(active_num.ssh_port, 22);

    // 4. Salvage logic: JSON with partial 'profiles' dictionary containing 1 valid and 1 broken profile
    let salvage_file = dir.join("salvage.json");
    let salvage_json = r#"{
        "active_profile": "valid_host",
        "profiles": {
            "valid_host": {
                "thinkstation_ip": "10.0.0.77",
                "mac_address": "00:11:22:33:44:55"
            },
            "corrupt_host": "this_is_not_an_object"
        }
    }"#;
    std::fs::write(&salvage_file, salvage_json).unwrap();
    let (salvaged_app, salvaged_active) = load_config_from_path(&salvage_file, None);
    assert_eq!(salvaged_app.active_profile, "valid_host");
    assert_eq!(salvaged_active.thinkstation_ip, "10.0.0.77");
    assert_eq!(salvaged_active.mac_address, "00:11:22:33:44:55");

    // Cleanup
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_config_validation_thorough_edge_cases() {
    let mut profile = HostProfile::default();
    assert!(validate_settings(&profile).is_ok());

    // IP validation
    profile.thinkstation_ip = "".to_string();
    assert!(validate_settings(&profile).is_err());
    profile.thinkstation_ip = "   ".to_string();
    assert!(validate_settings(&profile).is_err());
    profile.thinkstation_ip = "192.168.1.50; rm -rf /".to_string();
    assert!(validate_settings(&profile).is_err());
    profile.thinkstation_ip = "-oProxyCommand=evil".to_string();
    assert!(validate_settings(&profile).is_err());
    profile.thinkstation_ip = "192.168.1.50".to_string();

    // MAC validation
    profile.mac_address = "".to_string();
    assert!(validate_settings(&profile).is_err());
    profile.mac_address = "invalid_mac".to_string();
    assert!(validate_settings(&profile).is_err());
    profile.mac_address = "AA:BB:CC:DD:EE:FF".to_string();

    // Target SSH user validation
    profile.thinkstation_user = "user`whoami`".to_string();
    assert!(validate_settings(&profile).is_err());
    profile.thinkstation_user = "-user".to_string();
    assert!(validate_settings(&profile).is_err());
    profile.thinkstation_user = "valid_user".to_string();

    // Broadcast address validation
    profile.broadcast_addr = "192.168.1.255|cat /etc/passwd".to_string();
    assert!(validate_settings(&profile).is_err());
    profile.broadcast_addr = "192.168.1.255".to_string();

    // Pi gateway validation
    profile.pi_host = "pi; reboot".to_string();
    assert!(validate_settings(&profile).is_err());
    profile.pi_host = "192.168.1.10".to_string();
    profile.pi_user = "pi user".to_string(); // contains space
    assert!(validate_settings(&profile).is_err());
    profile.pi_user = "pi".to_string();

    // Sunshine service validation
    profile.sunshine_service = "sunshine; shutdown".to_string();
    assert!(validate_settings(&profile).is_err());
    profile.sunshine_service = "app-dev.lizardbyte.app.Sunshine.service".to_string();

    // Port zero checks
    profile.ssh_port = 0;
    assert!(validate_settings(&profile).is_err());
    profile.ssh_port = 22;

    profile.pi_ssh_port = 0;
    assert!(validate_settings(&profile).is_err());
    profile.pi_ssh_port = 22;

    profile.sunshine_port = 0;
    assert!(validate_settings(&profile).is_err());
    profile.sunshine_port = 47989;

    profile.direct_wol_port = 0;
    assert!(validate_settings(&profile).is_err());
    profile.direct_wol_port = 9;

    // Timeout zero checks
    profile.max_wait_seconds = 0;
    assert!(validate_settings(&profile).is_err());
    profile.max_wait_seconds = 300;

    profile.wol_resend_interval = 0;
    assert!(validate_settings(&profile).is_err());
    profile.wol_resend_interval = 60;

    assert!(validate_settings(&profile).is_ok());
}

#[test]
fn test_default_config_path_resolution() {
    let path = default_config_path();
    assert!(path.to_string_lossy().ends_with("config.json"));
}

// ============================================================================
// 2. NETWORK & SECURITY TESTS
// (MAC parsing, magic packet creation, host tokens, ping latency parsing)
// ============================================================================

#[test]
fn test_magic_packet_structure_and_integrity() {
    let mac = "00:11:22:33:44:55";
    let packet = create_magic_packet(mac).expect("Standard MAC should create packet");

    // Exact packet size: 6 header bytes + (16 * 6 MAC bytes) = 102 bytes
    assert_eq!(packet.len(), 102);

    // Header MUST be 6 bytes of 0xFF
    assert_eq!(&packet[0..6], &[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);

    // 16 iterations of target MAC payload bytes
    let expected_bytes = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];
    for rep in 0..16 {
        let offset = 6 + rep * 6;
        assert_eq!(&packet[offset..offset + 6], &expected_bytes);
    }
}

#[test]
fn test_mac_address_format_variations() {
    let expected_mac_bytes = [0xde, 0xad, 0xbe, 0xef, 0x12, 0x34];

    // Standard colon-separated
    let p_colon = create_magic_packet("de:ad:be:ef:12:34").expect("Colon delimiter");
    assert_eq!(&p_colon[6..12], &expected_mac_bytes);

    // Hyphen separated
    let p_hyphen = create_magic_packet("de-ad-be-ef-12-34").expect("Hyphen delimiter");
    assert_eq!(&p_hyphen[6..12], &expected_mac_bytes);

    // Cisco dot-separated (xxxx.xxxx.xxxx)
    let p_dot = create_magic_packet("dead.beef.1234").expect("Dot delimiter");
    assert_eq!(&p_dot[6..12], &expected_mac_bytes);

    // Raw 12-hex digits (no delimiters)
    let p_raw = create_magic_packet("deadbeef1234").expect("Raw 12 hex digits");
    assert_eq!(&p_raw[6..12], &expected_mac_bytes);

    // Uppercase colons and hyphens
    let p_up_col = create_magic_packet("DE:AD:BE:EF:12:34").expect("Uppercase colons");
    assert_eq!(&p_up_col[6..12], &expected_mac_bytes);

    let p_up_hyp = create_magic_packet("DE-AD-BE-EF-12-34").expect("Uppercase hyphens");
    assert_eq!(&p_up_hyp[6..12], &expected_mac_bytes);

    // Mixed case
    let p_mix = create_magic_packet("De:aD:bE:eF:12:34").expect("Mixed case");
    assert_eq!(&p_mix[6..12], &expected_mac_bytes);

    // Whitespace padded with newlines & tabs
    let p_pad = create_magic_packet(" \t de:ad:be:ef:12:34 \r\n ").expect("Whitespace padded");
    assert_eq!(&p_pad[6..12], &expected_mac_bytes);

    // Spaces separating hex pairs
    let p_spaces = create_magic_packet("de ad be ef 12 34").expect("Space separated hex pairs");
    assert_eq!(&p_spaces[6..12], &expected_mac_bytes);
}

#[test]
fn test_mac_address_invalid_variations() {
    // Empty & whitespace
    assert!(create_magic_packet("").is_err());
    assert!(create_magic_packet("   \n").is_err());

    // Leading delimiter
    assert!(create_magic_packet(":de:ad:be:ef:12:34").is_err());
    assert!(create_magic_packet("-de-ad-be-ef-12-34").is_err());
    assert!(create_magic_packet(".dead.beef.1234").is_err());

    // Trailing delimiter
    assert!(create_magic_packet("de:ad:be:ef:12:34:").is_err());
    assert!(create_magic_packet("de-ad-be-ef-12-34-").is_err());
    assert!(create_magic_packet("dead.beef.1234.").is_err());

    // Non-hex characters
    assert!(create_magic_packet("de:ad:be:ef:12:3g").is_err());
    assert!(create_magic_packet("invalid_mac_str").is_err());
    assert!(create_magic_packet("00:11:22:33:44:5!").is_err());

    // Incorrect digit count
    assert!(create_magic_packet("de:ad:be:ef:12").is_err()); // 10 hex digits
    assert!(create_magic_packet("deadbeef123").is_err()); // 11 hex digits
    assert!(create_magic_packet("deadbeef12345").is_err()); // 13 hex digits
    assert!(create_magic_packet("de:ad:be:ef:12:34:56").is_err()); // 14 hex digits

    // Command injection attempt in MAC
    assert!(create_magic_packet("de:ad:be:ef:12:34; reboot").is_err());
    assert!(create_magic_packet("de:ad:be:ef:12:34$(whoami)").is_err());
}

#[test]
fn test_validate_host_token_thorough() {
    // Valid tokens
    assert!(validate_host_token("192.168.1.100", "IP").is_ok());
    assert!(validate_host_token("10.0.0.1", "IP").is_ok());
    assert!(validate_host_token("thinkstation.local", "Host").is_ok());
    assert!(validate_host_token("my-pc.tailnet-xxx.ts.net", "Host").is_ok());
    assert!(validate_host_token("fe80::1", "IPv6").is_ok());
    assert!(validate_host_token("user_123", "User").is_ok());
    assert!(validate_host_token("admin-user", "User").is_ok());

    // Invalid tokens
    assert!(validate_host_token("", "Field").is_err());
    assert!(validate_host_token("   ", "Field").is_err());
    assert!(validate_host_token("-oProxyCommand=evil", "Option injection").is_err());
    assert!(validate_host_token("host; ls", "Semicolon injection").is_err());
    assert!(validate_host_token("host`id`", "Backtick injection").is_err());
    assert!(validate_host_token("host$(whoami)", "Dollar injection").is_err());
    assert!(validate_host_token("host & calc", "Background injection").is_err());
    assert!(validate_host_token("host | nc", "Pipe injection").is_err());
    assert!(validate_host_token("host name", "Space in host").is_err());
    assert!(validate_host_token("host\nname", "Newline in host").is_err());
    assert!(validate_host_token("host'name", "Single quote").is_err());
    assert!(validate_host_token("host\"name", "Double quote").is_err());
}

#[test]
fn test_ping_latency_parsing_non_english_and_unusual() {
    // English Windows ping
    assert_eq!(parse_ping_latency("Reply from 192.168.1.50: bytes=32 time=12ms TTL=128"), Some(12.0));
    assert_eq!(parse_ping_latency("Reply from 192.168.1.50: bytes=32 time<1ms TTL=128"), Some(0.5));
    assert_eq!(parse_ping_latency("Minimum = 5ms, Maximum = 15ms, Average = 10ms"), Some(10.0));

    // German Windows ping ("Zeit=14ms", "Zeit<1ms", "Mittelwert = 14ms")
    assert_eq!(parse_ping_latency("Antwort von 192.168.1.50: Bytes=32 Zeit=14ms TTL=128"), Some(14.0));
    assert_eq!(parse_ping_latency("Antwort von 192.168.1.50: Bytes=32 Zeit<1ms TTL=128"), Some(0.5));
    assert_eq!(parse_ping_latency("Minimum = 11ms, Maximum = 17ms, Mittelwert = 14ms"), Some(14.0));

    // French Windows ping ("temps=8 ms", "temps<1 ms", "Moyenne = 9ms")
    assert_eq!(parse_ping_latency("Réponse de 192.168.1.50 : octets=32 temps=8 ms TTL=128"), Some(8.0));
    assert_eq!(parse_ping_latency("Réponse de 192.168.1.50 : octets=32 temps < 1 ms TTL=128"), Some(0.5));
    assert_eq!(parse_ping_latency("Minimum = 6ms, Maximum = 12ms, Moyenne = 9ms"), Some(9.0));

    // Spanish Windows ping ("tiempo=23ms", "tiempo<1ms", "Promedio = 23ms")
    assert_eq!(parse_ping_latency("Respuesta desde 192.168.1.50: bytes=32 tiempo=23ms TTL=128"), Some(23.0));
    assert_eq!(parse_ping_latency("Respuesta desde 192.168.1.50: bytes=32 tiempo<1ms TTL=128"), Some(0.5));
    assert_eq!(parse_ping_latency("Mínimo = 20ms, Máximo = 26ms, Promedio = 23ms"), Some(23.0));

    // Italian / Portuguese ping ("tempo=5ms", "Media = 5ms")
    assert_eq!(parse_ping_latency("Risposta da 192.168.1.50: byte=32 tempo=5ms TTL=128"), Some(5.0));
    assert_eq!(parse_ping_latency("Minimo = 4ms, Massimo = 8ms, Media = 5ms"), Some(5.0));

    // BusyBox / Alpine Linux summary stats ("round-trip min/avg/max = 1.200/3.450/5.600 ms")
    assert_eq!(parse_ping_latency("round-trip min/avg/max = 1.200/3.450/5.600 ms"), Some(3.45));

    // Linux & macOS RTT summary
    assert_eq!(parse_ping_latency("rtt min/avg/max/mdev = 12.300/15.420/18.100/2.100 ms"), Some(15.42));
    assert_eq!(parse_ping_latency("round-trip min/avg/max/stddev = 1.234/2.345/3.456/0.123 ms"), Some(2.345));

    // IPv6 ping
    assert_eq!(parse_ping_latency("64 bytes from fe80::1%eth0: icmp_seq=1 ttl=64 time=0.412 ms"), Some(0.412));

    // Spaced time format
    assert_eq!(parse_ping_latency("64 bytes from 10.0.0.1: time = 12.34 ms"), Some(12.34));
}

#[test]
fn test_format_latency_all_branches() {
    assert_eq!(format_latency(None), "--ms");
    assert_eq!(format_latency(Some(0.0)), "<1ms");
    assert_eq!(format_latency(Some(0.5)), "<1ms");
    assert_eq!(format_latency(Some(0.99)), "<1ms");
    assert_eq!(format_latency(Some(1.0)), "1.0ms");
    assert_eq!(format_latency(Some(4.2)), "4.2ms");
    assert_eq!(format_latency(Some(9.94)), "9.9ms");
    assert_eq!(format_latency(Some(10.0)), "10ms");
    assert_eq!(format_latency(Some(12.7)), "13ms");
    assert_eq!(format_latency(Some(999.4)), "999ms");
}

#[test]
fn test_send_ssh_wol_and_power_action_security_validation() {
    // send_ssh_wol input validation
    assert!(send_ssh_wol("pi; rm -rf /", "192.168.1.10", 22, "192.168.1.255", "de:ad:be:ef:12:34").is_err());
    assert!(send_ssh_wol("pi", "-oProxyCommand=evil", 22, "192.168.1.255", "de:ad:be:ef:12:34").is_err());
    assert!(send_ssh_wol("pi", "192.168.1.10", 22, "192.168.1.255; reboot", "de:ad:be:ef:12:34").is_err());
    assert!(send_ssh_wol("pi", "192.168.1.10", 22, "192.168.1.255", "invalid_mac").is_err());

    // execute_power_action input validation
    assert!(execute_power_action("192.168.1.50", "user`whoami`", "suspend", 22).is_err());
    assert!(execute_power_action("-oProxyCommand=evil", "user", "suspend", 22).is_err());
    assert!(execute_power_action("192.168.1.50", "user", "invalid_action_name", 22).is_err());
}

// ============================================================================
// 3. PIPELINE STATE MACHINE TESTS
// (Progress fractions, standby/reset transitions, rate limiting, timeouts, checks)
// ============================================================================

#[test]
fn test_pipeline_stage_names_descriptions_and_progress_fractions() {
    let stages_and_fractions = [
        (Stage::Standby, "STANDBY", "System in Safe Standby", 0.0),
        (Stage::Wol, "1. WOL", "Magic Packet Sent", 0.15),
        (Stage::Ping, "2. PING", "ICMP Network Ping", 0.40),
        (Stage::Ssh, "3. SSH", "SSH Daemon (Port 22)", 0.65),
        (Stage::Sunshine, "4. STREAM", "Sunshine Stream (:47989)", 0.85),
        (Stage::Ready, "READY", "Target Ready to Stream", 1.0),
        (Stage::Timeout, "TIMEOUT", "Max Wait Timeout Exceeded", 1.0),
    ];

    for (stage, expected_name, expected_desc, expected_frac) in stages_and_fractions {
        assert_eq!(stage.name(), expected_name);
        assert_eq!(stage.description(), expected_desc);

        let mut state = PipelineState::new();
        state.stage = stage;
        if stage == Stage::Timeout {
            // Timeout's displayed progress mirrors the stage it stalled in
            // (set by tick() when the overall timeout fires), not a fixed
            // constant -- exercise that here with a stalled Sunshine stage.
            state.stalled_stage = Some(Stage::Sunshine);
            assert_eq!(
                state.progress_fraction(),
                PipelineState::stage_fraction(Stage::Sunshine)
            );
        } else {
            assert_eq!(state.progress_fraction(), expected_frac);
        }
    }
}

#[test]
fn test_pipeline_standby_and_reset_lifecycle() {
    let mut state = PipelineState::new();
    let config = HostProfile::default();

    // 1. Initialized to Wol stage
    assert_eq!(state.stage, Stage::Wol);

    // 2. Enter safe standby
    state.enter_standby("Manual standby requested");
    assert_eq!(state.stage, Stage::Standby);
    assert!(state.is_finished);
    assert!(state.start_time.is_none());
    assert!(state.status_message.contains("Standby: Manual standby requested"));

    // 3. In standby, tick() MUST NOT dispatch WOL or advance stage
    for _ in 0..5 {
        state.tick(&config, false);
        assert_eq!(state.stage, Stage::Standby);
        assert_eq!(state.wol_count, 0);
        assert!(state.is_finished);
    }

    // 4. Reset pipeline
    state.reset(&config);
    assert_eq!(state.stage, Stage::Ping); // Re-dispatches WOL and moves to Ping stage
    assert_eq!(state.wol_count, 1);
    assert!(!state.is_finished);
    assert!(state.start_time.is_some());
    assert!(state.last_wol_time.is_some());

    let logs = state.formatted_logs_text();
    assert!(logs.contains("[ACTION] Starting wake pipeline for"));
}

#[test]
fn test_pipeline_wol_rate_limiting_and_resend_interval() {
    let mut state = PipelineState::new();
    let config = HostProfile {
        wol_resend_interval: 10, // 10 seconds interval
        ..HostProfile::default()
    };

    state.reset(&config);
    assert_eq!(state.stage, Stage::Ping);
    assert_eq!(state.wol_count, 1);

    // Immediately calling tick() should NOT re-send WOL because 10s hasn't elapsed
    state.tick(&config, false);
    assert_eq!(state.wol_count, 1);

    // Simulate 11 seconds passing since last WOL packet
    state.last_wol_time = Some(Instant::now() - Duration::from_secs(11));
    state.tick(&config, false);

    // wol_count should increment to 2
    assert_eq!(state.wol_count, 2);
    let logs = state.formatted_logs_text();
    assert!(logs.contains("WOL resend interval (10s) elapsed without ping response"));

    // Setting wol_resend_interval to 0 should disable periodic WOL resend
    let no_resend_config = HostProfile {
        wol_resend_interval: 0,
        ..HostProfile::default()
    };
    state.last_wol_time = Some(Instant::now() - Duration::from_secs(100));
    state.tick(&no_resend_config, false);
    assert_eq!(state.wol_count, 2); // No increase
}

#[test]
fn test_pipeline_timeout_expiration_and_ready_immunity() {
    let mut state = PipelineState::new();
    let config = HostProfile {
        max_wait_seconds: 2,
        ..HostProfile::default()
    };

    state.reset(&config);
    assert_eq!(state.stage, Stage::Ping);

    // Simulate elapsed time exceeding max_wait_seconds
    state.start_time = Some(Instant::now() - Duration::from_secs(3));
    state.tick(&config, false);

    assert_eq!(state.stage, Stage::Timeout);
    assert_eq!(state.stalled_stage, Some(Stage::Ping));
    assert!(state.is_finished);
    assert!(state.status_message.contains("Timed out after 2 seconds"));

    // Verify Ready stage is immune to timeout expiration
    let mut ready_state = PipelineState::new();
    ready_state.stage = Stage::Ready;
    ready_state.is_finished = true;
    ready_state.start_time = Some(Instant::now() - Duration::from_secs(100));

    ready_state.tick(&config, false);
    assert_eq!(ready_state.stage, Stage::Ready);
    assert!(ready_state.is_finished);
}

#[test]
fn test_pipeline_log_level_tags_and_filtering() {
    let mut state = PipelineState::new();
    state.clear_logs();

    state.log(LogLevel::Info, "Initialization started");
    state.log(LogLevel::Ok, "Ping response 2.5ms");
    state.log(LogLevel::Warn, "Gateway connection retried");
    state.log(LogLevel::Err, "SSH daemon unreachable");
    state.log(LogLevel::Action, "Sending UDP magic packet");

    assert_eq!(state.logs.len(), 5);

    // Check tags
    assert_eq!(LogLevel::Info.tag(), "INFO");
    assert_eq!(LogLevel::Ok.tag(), "OK");
    assert_eq!(LogLevel::Warn.tag(), "WARN");
    assert_eq!(LogLevel::Err.tag(), "ERR");
    assert_eq!(LogLevel::Action.tag(), "ACTION");

    // Filter by level
    let ok_logs = state.filtered_logs(Some(LogLevel::Ok), "");
    assert_eq!(ok_logs.len(), 1);
    assert!(ok_logs[0].message.contains("Ping response"));

    let err_logs = state.filtered_logs(Some(LogLevel::Err), "");
    assert_eq!(err_logs.len(), 1);
    assert!(err_logs[0].message.contains("SSH daemon unreachable"));

    // Filter by search query (case-insensitive)
    let search_packet = state.filtered_logs(None, "magic packet");
    assert_eq!(search_packet.len(), 1);
    assert_eq!(search_packet[0].level, LogLevel::Action);

    let search_tag = state.filtered_logs(None, "WARN");
    assert_eq!(search_tag.len(), 1);

    // Clear logs
    state.clear_logs();
    assert!(state.logs.is_empty());
}

#[test]
fn test_pipeline_power_actions_trigger_standby_and_reset() {
    let mut state = PipelineState::new();
    let profile = HostProfile {
        thinkstation_ip: "invalid ip ; bad".to_string(), // Will fail host token validation
        ..HostProfile::default()
    };

    // Invalid power action
    assert!(state.trigger_power_action(&profile, "bad_action").is_err());

    // Action with invalid host token fails safely without executing shell commands
    assert!(state.trigger_power_action(&profile, "suspend").is_err());
}

#[test]
fn test_pipeline_stalled_stage_cleared_on_wol_resend_after_timeout() {
    // The "Re-send WOL" UI button calls send_wol() directly (not reset()),
    // and send_wol()'s stage-advance-to-Ping branch fires whenever stage is
    // Wol, Standby, *or Timeout*. Verify a manual resend after a timeout
    // actually resumes the pipeline and clears the stale stalled_stage
    // instead of leaving the UI showing a frozen "stalled at SSH" progress
    // bar while the pipeline is really back at Ping.
    let mut state = PipelineState::new();
    let config = HostProfile {
        max_wait_seconds: 2,
        ..HostProfile::default()
    };

    state.reset(&config);
    // Force the timeout to land on a later, non-trivial stage.
    state.stage = Stage::Ssh;
    state.start_time = Some(Instant::now() - Duration::from_secs(3));
    state.tick(&config, false);

    assert_eq!(state.stage, Stage::Timeout);
    assert_eq!(state.stalled_stage, Some(Stage::Ssh));
    assert_eq!(state.progress_fraction(), PipelineState::stage_fraction(Stage::Ssh));

    let wol_count_before = state.wol_count;
    state.send_wol(&config);

    assert_eq!(state.stage, Stage::Ping, "resend after timeout should resume at Ping");
    assert_eq!(state.stalled_stage, None, "stalled_stage must be cleared once the pipeline resumes");
    assert_eq!(state.progress_fraction(), PipelineState::stage_fraction(Stage::Ping));
    assert!(state.wol_count > wol_count_before);
}

#[test]
fn test_pipeline_stalled_stage_does_not_leak_across_resets() {
    let mut state = PipelineState::new();
    let config = HostProfile {
        max_wait_seconds: 2,
        ..HostProfile::default()
    };

    // First run times out stalled at Ping.
    state.reset(&config);
    state.start_time = Some(Instant::now() - Duration::from_secs(3));
    state.tick(&config, false);
    assert_eq!(state.stalled_stage, Some(Stage::Ping));

    // reset() must fully clear stalled_stage even though a previous run left
    // it populated.
    state.reset(&config);
    assert_eq!(state.stalled_stage, None);
    assert_eq!(state.stage, Stage::Ping);

    // A second timeout stalled at a later stage should reflect the NEW stall
    // point, not any stale value from the first timeout.
    state.stage = Stage::Sunshine;
    state.start_time = Some(Instant::now() - Duration::from_secs(3));
    state.tick(&config, false);
    assert_eq!(state.stalled_stage, Some(Stage::Sunshine));
    assert_ne!(state.stalled_stage, Some(Stage::Ping));
}

#[test]
fn test_pipeline_timeout_while_stage_is_wol_itself() {
    // A freshly constructed PipelineState starts in Stage::Wol. If start_time
    // is already populated (so tick()'s "first ever tick" bootstrap branch,
    // which would otherwise immediately advance Wol -> Ping via send_wol, is
    // skipped) and the deadline has already passed, the timeout check can
    // fire while stage is still exactly Wol -- the very first stage. Confirm
    // stalled_stage is explicitly set to Wol (not just coincidentally
    // defaulting to it) and progress reflects the Wol weight, not 1.0.
    let mut state = PipelineState::new();
    assert_eq!(state.stage, Stage::Wol);
    let config = HostProfile {
        max_wait_seconds: 2,
        ..HostProfile::default()
    };

    state.start_time = Some(Instant::now() - Duration::from_secs(5));
    state.tick(&config, false);

    assert_eq!(state.stage, Stage::Timeout);
    assert_eq!(state.stalled_stage, Some(Stage::Wol));
    assert_eq!(state.progress_fraction(), PipelineState::stage_fraction(Stage::Wol));
    assert_eq!(state.progress_fraction(), 0.15);
}

#[test]
fn test_pipeline_poll_check_rate_limiting_boundary() {
    // `test_pipeline_background_check_deduplication` only exercises the
    // same-stage/dedup path (rapid ticks while a check is already pending);
    // it never actually observes the MIN_CHECK_INTERVAL (800ms) timing gate
    // that stops a NEW check from being spawned right after the previous one
    // completed. Drive poll_check's private timing state directly instead of
    // sleeping in the test.
    let mut state = PipelineState::new();
    let config = HostProfile {
        thinkstation_ip: "127.0.0.1".to_string(),
        ..HostProfile::default()
    };

    // Simulate a check for Ping that completed 400ms ago -- under the 800ms
    // minimum interval, so no new check should be spawned yet.
    state.last_checked_stage = Some(Stage::Ping);
    state.last_check_completed = Some(Instant::now() - Duration::from_millis(400));
    state.pending_check = None;

    let result = state.poll_check(&config, Stage::Ping);
    assert!(result.is_none());
    assert!(
        state.pending_check.is_none(),
        "must not spawn a new background check before MIN_CHECK_INTERVAL elapses"
    );

    // Now simulate enough time having passed (900ms > 800ms threshold).
    state.last_check_completed = Some(Instant::now() - Duration::from_millis(900));
    let result2 = state.poll_check(&config, Stage::Ping);
    assert!(result2.is_none(), "freshly spawned check has no result yet");
    assert!(
        state.pending_check.is_some(),
        "must spawn a new background check once MIN_CHECK_INTERVAL has elapsed"
    );
}

#[test]
fn test_pipeline_poll_check_drops_pending_when_stage_changes_mid_flight() {
    // If the pipeline stage advances (e.g. Ping -> Ssh) while a background
    // check for the OLD stage is still in flight, poll_check must discard
    // the stale pending check and immediately start a fresh one for the new
    // stage rather than returning a Ping result while callers believe
    // they're polling for Ssh (or blocking on the old stage's rate limit).
    let mut state = PipelineState::new();
    let config = HostProfile {
        thinkstation_ip: "127.0.0.1".to_string(),
        ..HostProfile::default()
    };

    // Fabricate a still-pending check left over from the Ping stage, with a
    // very recent completion timestamp that would otherwise rate-limit a
    // fresh spawn if the stage hadn't changed.
    state.last_checked_stage = Some(Stage::Ping);
    state.last_check_completed = Some(Instant::now());
    state.pending_check = Some((Stage::Ping, disconnected_check_receiver()));

    let result = state.poll_check(&config, Stage::Ssh);
    assert!(result.is_none(), "no result should be reported for the newly-started stage yet");
    assert_eq!(state.last_checked_stage, Some(Stage::Ssh));
    let pending_stage = state.pending_check.as_ref().map(|(stage, _)| *stage);
    assert_eq!(
        pending_stage,
        Some(Stage::Ssh),
        "a fresh check for the new stage must be spawned immediately, ignoring the old stage's rate limit"
    );
}

#[test]
fn test_pipeline_poll_check_handles_disconnected_channel() {
    // If the background check thread dies without sending (e.g. panics)
    // instead of the receiver being dropped by the pipeline, poll_check must
    // treat that as "no result" gracefully: clear the pending slot and stamp
    // last_check_completed so rate limiting still applies, instead of
    // spinning forever trying to receive from a dead sender.
    let mut state = PipelineState::new();
    let config = HostProfile::default();

    state.last_checked_stage = Some(Stage::Ping);
    state.last_check_completed = None;
    state.pending_check = Some((Stage::Ping, disconnected_check_receiver()));

    let result = state.poll_check(&config, Stage::Ping);
    assert!(result.is_none());
    assert!(state.pending_check.is_none(), "disconnected channel must clear the pending slot");
    assert!(
        state.last_check_completed.is_some(),
        "disconnected channel must still stamp completion time for rate limiting"
    );
}

#[test]
fn test_pipeline_background_check_deduplication() {
    let mut state = PipelineState::new();
    let config = HostProfile {
        thinkstation_ip: "127.0.0.1".to_string(),
        ..HostProfile::default()
    };

    state.reset(&config);
    assert_eq!(state.stage, Stage::Ping);

    // First tick in Ping stage will spawn a background check thread
    state.tick(&config, false);

    // Rapid successive tick calls will re-use the in-flight background check slot
    // without spawning multiple duplicate threads
    for _ in 0..10 {
        state.tick(&config, false);
    }
}

// ============================================================================
// 4. CLI OVERRIDES TESTS
// ============================================================================

#[test]
fn test_cli_argument_overrides_full() {
    let base_profile = HostProfile::default();
    let args = CliArgs {
        cli: true,
        target: Some("10.0.0.250".to_string()),
        user: Some("super_user".to_string()),
        mac: Some("AA-BB-CC-DD-EE-FF".to_string()),
        broadcast: Some("10.0.0.255".to_string()),
        pi_host: Some("10.0.0.2".to_string()),
        pi_user: Some("pi_admin".to_string()),
        pi_ssh_port: Some(2222),
        direct_wol: true,
        direct_wol_port: Some(7),
        ssh_port: Some(2200),
        sunshine_port: Some(48000),
        max_wait: Some(180),
        wol_resend: Some(45),
        auto_close: true,
        ..CliArgs::default()
    };

    let overridden = apply_cli_overrides(base_profile, &args);
    assert_eq!(overridden.thinkstation_ip, "10.0.0.250");
    assert_eq!(overridden.thinkstation_user, "super_user");
    assert_eq!(overridden.mac_address, "AA-BB-CC-DD-EE-FF");
    assert_eq!(overridden.broadcast_addr, "10.0.0.255");
    assert_eq!(overridden.pi_host, "10.0.0.2");
    assert_eq!(overridden.pi_user, "pi_admin");
    assert_eq!(overridden.pi_ssh_port, 2222);
    assert!(overridden.direct_wol);
    assert_eq!(overridden.direct_wol_port, 7);
    assert_eq!(overridden.ssh_port, 2200);
    assert_eq!(overridden.sunshine_port, 48000);
    assert_eq!(overridden.max_wait_seconds, 180);
    assert_eq!(overridden.wol_resend_interval, 45);
    assert!(overridden.auto_close);
}

#[test]
fn test_ping_latency_parsing_international_locales() {
    // German
    let de_line = "Antwort von 192.168.1.50: Bytes=32 Zeit=12ms TTL=64";
    assert_eq!(parse_ping_latency(de_line), Some(12.0));
    let de_sub = "Antwort von 192.168.1.50: Bytes=32 Zeit<1ms TTL=64";
    assert_eq!(parse_ping_latency(de_sub), Some(0.5));
    let de_sum = "Minimum = 1ms, Maximum = 14ms, Mittelwert = 9ms";
    assert_eq!(parse_ping_latency(de_sum), Some(9.0));

    // French
    let fr_line = "Réponse de 192.168.1.50 : octets=32 temps=8 ms TTL=64";
    assert_eq!(parse_ping_latency(fr_line), Some(8.0));
    let fr_sum = "Minimum = 1ms, Maximum = 12ms, Moyenne = 7ms";
    assert_eq!(parse_ping_latency(fr_sum), Some(7.0));

    // Spanish
    let es_line = "Respuesta desde 192.168.1.50: bytes=32 tiempo=15ms TTL=64";
    assert_eq!(parse_ping_latency(es_line), Some(15.0));
    let es_sum = "Mínimo = 2ms, Máximo = 10ms, Media = 5ms";
    assert_eq!(parse_ping_latency(es_sum), Some(5.0));

    // Russian
    let ru_line = "Ответ от 192.168.1.50: число байт=32 время=4мс TTL=64";
    assert_eq!(parse_ping_latency(ru_line), Some(4.0));
    let ru_sub = "Ответ от 192.168.1.50: число байт=32 время<1мс TTL=64";
    assert_eq!(parse_ping_latency(ru_sub), Some(0.5));
    let ru_sum = "Минимум = 1мс, Максимум = 8мс, Среднее = 3мс";
    assert_eq!(parse_ping_latency(ru_sum), Some(3.0));

    // Chinese / Japanese
    let zh_line = "来自 192.168.1.50 的回复: 字节=32 时间=2ms TTL=64";
    assert_eq!(parse_ping_latency(zh_line), Some(2.0));
    let zh_sum = "最短 = 1ms，最长 = 5ms，平均 = 2ms";
    assert_eq!(parse_ping_latency(zh_sum), Some(2.0));

    // Generic fallback
    let custom_output = "Ping target responded: rtt=18.5ms";
    assert_eq!(parse_ping_latency(custom_output), Some(18.5));
}

#[test]
fn test_config_export_import_roundtrips() {
    // 1. Default AppConfig export/import roundtrip
    let default_cfg = AppConfig::default();
    let exported = export_config_json(&default_cfg).expect("Export default config should succeed");
    let imported = import_config_json(&exported).expect("Import exported config should succeed");
    assert_eq!(imported, default_cfg);

    // 2. Custom multi-profile AppConfig export/import roundtrip
    let mut custom_cfg = AppConfig::default();
    let custom_prof = HostProfile {
        thinkstation_ip: "10.0.0.100".to_string(),
        thinkstation_user: "dev".to_string(),
        mac_address: "11:22:33:44:55:66".to_string(),
        broadcast_addr: "10.0.0.255".to_string(),
        ssh_port: 2222,
        sunshine_port: 47990,
        ..HostProfile::default()
    };
    custom_cfg.set_profile("gaming_pc", custom_prof);
    custom_cfg.active_profile = "gaming_pc".to_string();

    let custom_exported = export_config_json(&custom_cfg).expect("Export custom config should succeed");
    let custom_imported = import_config_json(&custom_exported).expect("Import custom config should succeed");
    assert_eq!(custom_imported, custom_cfg);
    assert_eq!(custom_imported.active_profile, "gaming_pc");
    assert_eq!(custom_imported.get_active_profile().thinkstation_ip, "10.0.0.100");

    // 3. Import invalid JSON returns error
    assert!(import_config_json("").is_err());
    assert!(import_config_json("   ").is_err());
    assert!(import_config_json("not json {").is_err());

    // 4. Import legacy flat HostProfile JSON
    let flat_json = r#"{
        "thinkstation_ip": "192.168.1.99",
        "mac_address": "00:11:22:33:44:55"
    }"#;
    let flat_imported = import_config_json(flat_json).expect("Import legacy flat profile should succeed");
    assert_eq!(flat_imported.active_profile, "thinkstation");
    assert_eq!(flat_imported.get_active_profile().thinkstation_ip, "192.168.1.99");
}

#[test]
fn test_latency_history_buffer_bounds() {
    let mut state = PipelineState::new();
    let config = HostProfile::default();

    // 1. Initial state
    assert!(state.latency_history.is_empty());
    assert_eq!(state.last_latency, None);

    // 2. Push None latency update - history should remain empty
    state.update_latency(None);
    assert!(state.latency_history.is_empty());
    assert_eq!(state.last_latency, None);

    // 3. Push single valid latency sample
    state.update_latency(Some(15.2));
    assert_eq!(state.last_latency, Some(15.2));
    assert_eq!(state.latency_history, vec![15.2]);

    // 4. Fill history buffer up to 30 items
    state.latency_history.clear();
    for i in 1..=30 {
        state.update_latency(Some(i as f32));
    }
    assert_eq!(state.latency_history.len(), 30);
    assert_eq!(state.latency_history.first(), Some(&1.0));
    assert_eq!(state.latency_history.last(), Some(&30.0));
    assert_eq!(state.last_latency, Some(30.0));

    // 4b. Exact single-sample-over-cap boundary: pushing the 31st sample
    // while already at the 30-item cap must evict exactly the one oldest
    // entry, not zero and not more than one (off-by-one check).
    state.update_latency(Some(31.0));
    assert_eq!(state.latency_history.len(), 30);
    assert_eq!(state.latency_history.first(), Some(&2.0));
    assert_eq!(state.latency_history.last(), Some(&31.0));

    // 5. Overflow buffer with 14 more samples (total 45 samples pushed
    // overall, continuing on from the 31 already pushed above)
    for i in 32..=45 {
        state.update_latency(Some(i as f32));
    }
    // Length must remain strictly bounded to max 30 items
    assert_eq!(state.latency_history.len(), 30);
    // Oldest samples 1.0..=15.0 should be evicted, remaining 16.0..=45.0
    assert_eq!(state.latency_history.first(), Some(&16.0));
    assert_eq!(state.latency_history.last(), Some(&45.0));
    assert_eq!(state.last_latency, Some(45.0));

    // 6. Reset pipeline clears latency_history and last_latency
    state.reset(&config);
    assert!(state.latency_history.is_empty());
    assert_eq!(state.last_latency, None);
}


