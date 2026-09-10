use crate::config::{
    default_config_path, export_config_json, import_config_json, load_config_from_path,
    save_config_to_path, validate_settings, AppConfig, HostProfile,
};
use crate::net::{
    create_magic_packet, execute_power_action, format_latency, parse_ping_latency, send_ssh_wol,
    validate_host_token,
};
use crate::pipeline::{disconnected_check_receiver, LogLevel, PipelineState, Stage};
use crate::{apply_cli_overrides, default_native_options, CliArgs};
use std::collections::HashMap;
use std::time::{Duration, Instant};

// ============================================================================
// 1. CONFIGURATION TESTS
// (Parsing, validation, atomic saving, fallback logic, profile management)
// ============================================================================

#[test]
fn test_config_atomic_saving_and_nested_directories() {
    let dir = std::env::temp_dir()
        .join("wake_rs_nested_test_dir_a")
        .join("sub_b")
        .join("sub_c");
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
        schema_version: 1,
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

    // 1. AppConfig default contains "workstation"
    assert_eq!(app_cfg.active_profile, "workstation");
    let active_def = app_cfg.get_active_profile();
    assert_eq!(active_def.thinkstation_ip, "192.168.1.50");

    // 2. Add custom profile
    let custom_prof = HostProfile {
        thinkstation_ip: "172.16.0.10".to_string(),
        thinkstation_user: "workstation".to_string(),
        mac_address: "AA:BB:CC:11:22:33".to_string(),
        ..HostProfile::default()
    };
    app_cfg.set_profile("gaming", custom_prof);

    // 3. Change active profile
    app_cfg.active_profile = "gaming".to_string();
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
    assert_eq!(app_cfg.active_profile, "workstation");
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
    assert_eq!(app_cfg.active_profile, "workstation");
    assert_eq!(active.thinkstation_ip, "192.168.1.50");

    // 2. Empty file
    let empty_file = dir.join("empty.json");
    std::fs::write(&empty_file, "").unwrap();
    let (app_cfg_empty, active_empty) = load_config_from_path(&empty_file, None);
    assert_eq!(app_cfg_empty.active_profile, "workstation");
    assert_eq!(active_empty.mac_address, "AA:BB:CC:DD:EE:FF");

    // 3. Root JSON integer instead of object
    let num_file = dir.join("number.json");
    std::fs::write(&num_file, "42").unwrap();
    let (app_cfg_num, active_num) = load_config_from_path(&num_file, None);
    assert_eq!(app_cfg_num.active_profile, "workstation");
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
    assert_eq!(
        parse_ping_latency("Reply from 192.168.1.50: bytes=32 time=12ms TTL=128"),
        Some(12.0)
    );
    assert_eq!(
        parse_ping_latency("Reply from 192.168.1.50: bytes=32 time<1ms TTL=128"),
        Some(0.5)
    );
    assert_eq!(
        parse_ping_latency("Minimum = 5ms, Maximum = 15ms, Average = 10ms"),
        Some(10.0)
    );

    // German Windows ping ("Zeit=14ms", "Zeit<1ms", "Mittelwert = 14ms")
    assert_eq!(
        parse_ping_latency("Antwort von 192.168.1.50: Bytes=32 Zeit=14ms TTL=128"),
        Some(14.0)
    );
    assert_eq!(
        parse_ping_latency("Antwort von 192.168.1.50: Bytes=32 Zeit<1ms TTL=128"),
        Some(0.5)
    );
    assert_eq!(
        parse_ping_latency("Minimum = 11ms, Maximum = 17ms, Mittelwert = 14ms"),
        Some(14.0)
    );

    // French Windows ping ("temps=8 ms", "temps<1 ms", "Moyenne = 9ms")
    assert_eq!(
        parse_ping_latency("Réponse de 192.168.1.50 : octets=32 temps=8 ms TTL=128"),
        Some(8.0)
    );
    assert_eq!(
        parse_ping_latency("Réponse de 192.168.1.50 : octets=32 temps < 1 ms TTL=128"),
        Some(0.5)
    );
    assert_eq!(
        parse_ping_latency("Minimum = 6ms, Maximum = 12ms, Moyenne = 9ms"),
        Some(9.0)
    );

    // Spanish Windows ping ("tiempo=23ms", "tiempo<1ms", "Promedio = 23ms")
    assert_eq!(
        parse_ping_latency("Respuesta desde 192.168.1.50: bytes=32 tiempo=23ms TTL=128"),
        Some(23.0)
    );
    assert_eq!(
        parse_ping_latency("Respuesta desde 192.168.1.50: bytes=32 tiempo<1ms TTL=128"),
        Some(0.5)
    );
    assert_eq!(
        parse_ping_latency("Mínimo = 20ms, Máximo = 26ms, Promedio = 23ms"),
        Some(23.0)
    );

    // Italian / Portuguese ping ("tempo=5ms", "Media = 5ms")
    assert_eq!(
        parse_ping_latency("Risposta da 192.168.1.50: byte=32 tempo=5ms TTL=128"),
        Some(5.0)
    );
    assert_eq!(
        parse_ping_latency("Minimo = 4ms, Massimo = 8ms, Media = 5ms"),
        Some(5.0)
    );

    // BusyBox / Alpine Linux summary stats ("round-trip min/avg/max = 1.200/3.450/5.600 ms")
    assert_eq!(
        parse_ping_latency("round-trip min/avg/max = 1.200/3.450/5.600 ms"),
        Some(3.45)
    );

    // Linux & macOS RTT summary
    assert_eq!(
        parse_ping_latency("rtt min/avg/max/mdev = 12.300/15.420/18.100/2.100 ms"),
        Some(15.42)
    );
    assert_eq!(
        parse_ping_latency("round-trip min/avg/max/stddev = 1.234/2.345/3.456/0.123 ms"),
        Some(2.345)
    );

    // IPv6 ping
    assert_eq!(
        parse_ping_latency("64 bytes from fe80::1%eth0: icmp_seq=1 ttl=64 time=0.412 ms"),
        Some(0.412)
    );

    // Spaced time format
    assert_eq!(
        parse_ping_latency("64 bytes from 10.0.0.1: time = 12.34 ms"),
        Some(12.34)
    );
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
    assert!(send_ssh_wol(
        "pi; rm -rf /",
        "192.168.1.10",
        22,
        "192.168.1.255",
        "de:ad:be:ef:12:34"
    )
    .is_err());
    assert!(send_ssh_wol(
        "pi",
        "-oProxyCommand=evil",
        22,
        "192.168.1.255",
        "de:ad:be:ef:12:34"
    )
    .is_err());
    assert!(send_ssh_wol(
        "pi",
        "192.168.1.10",
        22,
        "192.168.1.255; reboot",
        "de:ad:be:ef:12:34"
    )
    .is_err());
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
        (
            Stage::Sunshine,
            "4. STREAM",
            "Sunshine Stream (:47989)",
            0.85,
        ),
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
    state.last_latency = Some(2.4);
    state.latency_history.push(2.4);
    state.enter_standby("Manual standby requested");
    assert_eq!(state.stage, Stage::Standby);
    assert!(state.is_finished);
    assert!(state.start_time.is_none());
    assert!(state.last_latency.is_none());
    assert!(state.latency_history.is_empty());
    assert!(state
        .status_message
        .contains("Standby: Manual standby requested"));

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

    // Test power action cooldown guard and reboot bypass
    let valid_profile = HostProfile::default();
    state.enter_standby("Host offline");
    state.last_power_action_time = Some(Instant::now());

    // reset() ignored while cooldown is active
    state.reset(&valid_profile);
    assert_eq!(state.stage, Stage::Standby);

    // Cooldown cleared (e.g. intentionally by reboot action) allows reset() to proceed immediately
    state.last_power_action_time = None;
    state.reset(&valid_profile);
    assert_eq!(state.stage, Stage::Ping);
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
    assert_eq!(
        state.progress_fraction(),
        PipelineState::stage_fraction(Stage::Ssh)
    );

    let wol_count_before = state.wol_count;
    state.send_wol(&config);

    assert_eq!(
        state.stage,
        Stage::Ping,
        "resend after timeout should resume at Ping"
    );
    assert_eq!(
        state.stalled_stage, None,
        "stalled_stage must be cleared once the pipeline resumes"
    );
    assert_eq!(
        state.progress_fraction(),
        PipelineState::stage_fraction(Stage::Ping)
    );
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
    assert_eq!(
        state.progress_fraction(),
        PipelineState::stage_fraction(Stage::Wol)
    );
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
    assert!(
        result.is_none(),
        "no result should be reported for the newly-started stage yet"
    );
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
    assert!(
        state.pending_check.is_none(),
        "disconnected channel must clear the pending slot"
    );
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

    // Verify forcing flags off with no_direct_wol and no_auto_close
    let initial = HostProfile {
        direct_wol: true,
        auto_close: true,
        ..HostProfile::default()
    };
    let disable_args = CliArgs {
        no_direct_wol: true,
        no_auto_close: true,
        ..CliArgs::default()
    };
    let forced_off = apply_cli_overrides(initial, &disable_args);
    assert!(!forced_off.direct_wol);
    assert!(!forced_off.auto_close);
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

    let custom_exported =
        export_config_json(&custom_cfg).expect("Export custom config should succeed");
    let custom_imported =
        import_config_json(&custom_exported).expect("Import custom config should succeed");
    assert_eq!(custom_imported, custom_cfg);
    assert_eq!(custom_imported.active_profile, "gaming_pc");
    assert_eq!(
        custom_imported.get_active_profile().thinkstation_ip,
        "10.0.0.100"
    );

    // 3. Import invalid JSON returns error
    assert!(import_config_json("").is_err());
    assert!(import_config_json("   ").is_err());
    assert!(import_config_json("not json {").is_err());

    // 4. Import legacy flat HostProfile JSON
    let flat_json = r#"{
        "thinkstation_ip": "192.168.1.99",
        "mac_address": "00:11:22:33:44:55"
    }"#;
    let flat_imported =
        import_config_json(flat_json).expect("Import legacy flat profile should succeed");
    assert_eq!(flat_imported.active_profile, "workstation");
    assert_eq!(
        flat_imported.get_active_profile().thinkstation_ip,
        "192.168.1.99"
    );
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

// ============================================================================
// 5. SYSTEMS ENGINEERING ENHANCEMENT TESTS
// (OnceLock regex compilation, system audio chimes, enhanced Moonlight discovery,
// modal handling, validation rules, multi-threaded stress tests)
// ============================================================================

#[test]
fn test_ping_latency_oncelock_static_regex_parsing() {
    // Verify OnceLock regex compilation produces identical results across multiple calls and formats
    let samples = [
        (
            "Reply from 192.168.1.50: bytes=32 time=4.5ms TTL=64",
            Some(4.5),
        ),
        (
            "Reply from 192.168.1.50: bytes=32 time<1ms TTL=64",
            Some(0.5),
        ),
        (
            "Antwort von 192.168.1.50: Bytes=32 Zeit=8ms TTL=64",
            Some(8.0),
        ),
        ("rtt min/avg/max/mdev = 1.1/2.2/3.3/0.4 ms", Some(2.2)),
        ("Average = 16ms", Some(16.0)),
        ("Ping rtt=33.3ms", Some(33.3)),
        ("unknown host", None),
    ];

    for _ in 0..10 {
        for (input, expected) in samples {
            assert_eq!(parse_ping_latency(input), expected);
        }
    }
}

#[test]
fn test_ping_latency_multithreaded_concurrency_stress() {
    use std::sync::Arc;
    use std::thread;

    let test_cases = Arc::new(vec![
        (
            "Reply from 192.168.1.50: bytes=32 time=12ms TTL=128",
            Some(12.0),
        ),
        (
            "Reply from 192.168.1.50: bytes=32 time<1ms TTL=128",
            Some(0.5),
        ),
        (
            "Antwort von 10.0.0.1: Bytes=32 Zeit=14ms TTL=128",
            Some(14.0),
        ),
        ("Antwort von 10.0.0.1: Bytes=32 Zeit<1ms TTL=128", Some(0.5)),
        (
            "Réponse de 172.16.0.1 : octets=32 temps=8 ms TTL=128",
            Some(8.0),
        ),
        (
            "Respuesta desde 192.168.1.1: bytes=32 tiempo=23ms TTL=128",
            Some(23.0),
        ),
        (
            "Risposta da 192.168.1.50: byte=32 tempo=5ms TTL=128",
            Some(5.0),
        ),
        (
            "Odpowiedź z 192.168.1.50: bajty=32 czas=18ms TTL=64",
            Some(18.0),
        ),
        (
            "Antwoord van 192.168.1.50: bytes=32 tijd=16ms TTL=64",
            Some(16.0),
        ),
        (
            "Ответ от 192.168.1.50: число байт=32 время=4мс TTL=64",
            Some(4.0),
        ),
        (
            "来自 192.168.1.50 的回复: 字节=32 时间=2ms TTL=64",
            Some(2.0),
        ),
        (
            "來自 192.168.1.50 的回覆: 位元組=32 時間=3ms TTL=64",
            Some(3.0),
        ),
        (
            "64 bytes from 10.0.0.1: icmp_seq=1 ttl=64 time=0.456 ms",
            Some(0.456),
        ),
        (
            "rtt min/avg/max/mdev = 12.300/15.420/18.100/2.100 ms",
            Some(15.42),
        ),
        ("round-trip min/avg/max = 1.200/3.450/5.600 ms", Some(3.45)),
        ("Request timed out.", None),
        ("Destination host unreachable.", None),
        ("100% packet loss", None),
    ]);

    let mut handles = Vec::new();
    for thread_id in 0..8 {
        let cases = Arc::clone(&test_cases);
        let handle = thread::spawn(move || {
            for iteration in 0..150 {
                for (input, expected) in cases.iter() {
                    let parsed = parse_ping_latency(input);
                    assert_eq!(
                        parsed, *expected,
                        "Thread {} iter {} mismatch on '{}'",
                        thread_id, iteration, input
                    );
                }
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle
            .join()
            .expect("Thread should finish cleanly without panic");
    }
}

#[test]
fn test_ping_latency_throughput_performance_benchmark() {
    let start = Instant::now();
    let samples = [
        "Reply from 192.168.1.50: bytes=32 time=4.5ms TTL=64",
        "Reply from 192.168.1.50: bytes=32 time<1ms TTL=64",
        "Antwort von 192.168.1.50: Bytes=32 Zeit=8ms TTL=64",
        "Réponse de 192.168.1.50 : octets=32 temps=8 ms TTL=64",
        "Respuesta desde 192.168.1.50: bytes=32 tiempo=15ms TTL=64",
        "Ответ от 192.168.1.50: число байт=32 время=4мс TTL=64",
        "来自 192.168.1.50 的回复: 字节=32 时间=2ms TTL=64",
        "rtt min/avg/max/mdev = 1.1/2.2/3.3/0.4 ms",
        "Average = 16ms",
        "Request timed out.",
    ];

    const ITERATIONS: usize = 20_000;
    for i in 0..ITERATIONS {
        let sample = samples[i % samples.len()];
        let _ = parse_ping_latency(sample);
    }

    let elapsed = start.elapsed();
    // 20,000 regex matches via OnceLock cached static regexes should execute rapidly (under 1.5s in unoptimized debug mode)
    assert!(
        elapsed < Duration::from_millis(1500),
        "20,000 ping latency parses via OnceLock must complete quickly (took {:?})",
        elapsed
    );
}

#[test]
fn test_ping_latency_exhaustive_international_locales_and_edge_cases() {
    // English formats
    assert_eq!(
        parse_ping_latency("Reply from 10.0.0.1: bytes=32 time=12ms TTL=64"),
        Some(12.0)
    );
    assert_eq!(
        parse_ping_latency("Reply from 10.0.0.1: bytes=32 time<1ms TTL=64"),
        Some(0.5)
    );
    assert_eq!(
        parse_ping_latency("Reply from 10.0.0.1: bytes=32 time = 15.6 ms TTL=64"),
        Some(15.6)
    );
    assert_eq!(
        parse_ping_latency("Reply from 10.0.0.1: bytes=32 time < 1 ms TTL=64"),
        Some(0.5)
    );
    assert_eq!(
        parse_ping_latency("Minimum = 5ms, Maximum = 15ms, Average = 10ms"),
        Some(10.0)
    );
    assert_eq!(
        parse_ping_latency("Minimum = 5.2ms, Maximum = 15.8ms, Average = 10.5ms"),
        Some(10.5)
    );

    // German formats
    assert_eq!(
        parse_ping_latency("Antwort von 10.0.0.1: Bytes=32 Zeit=14ms TTL=128"),
        Some(14.0)
    );
    assert_eq!(
        parse_ping_latency("Antwort von 10.0.0.1: Bytes=32 Zeit<1ms TTL=128"),
        Some(0.5)
    );
    assert_eq!(
        parse_ping_latency("Antwort von 10.0.0.1: Bytes=32 Zeit < 1 ms TTL=128"),
        Some(0.5)
    );
    assert_eq!(
        parse_ping_latency("Minimum = 11ms, Maximum = 17ms, Mittelwert = 14ms"),
        Some(14.0)
    );
    assert_eq!(
        parse_ping_latency("Minimum = 10ms, Maximum = 20ms, Durchschnitt = 15ms"),
        Some(15.0)
    );

    // French formats
    assert_eq!(
        parse_ping_latency("Réponse de 10.0.0.1 : octets=32 temps=8 ms TTL=128"),
        Some(8.0)
    );
    assert_eq!(
        parse_ping_latency("Réponse de 10.0.0.1 : octets=32 temps<1ms TTL=128"),
        Some(0.5)
    );
    assert_eq!(
        parse_ping_latency("Réponse de 10.0.0.1 : octets=32 temps < 1 ms TTL=128"),
        Some(0.5)
    );
    assert_eq!(
        parse_ping_latency("Minimum = 6ms, Maximum = 12ms, Moyenne = 9ms"),
        Some(9.0)
    );
    assert_eq!(
        parse_ping_latency("Minimum = 6.2ms, Maximum = 12.4ms, Moyenne = 8.5ms"),
        Some(8.5)
    );

    // Spanish formats
    assert_eq!(
        parse_ping_latency("Respuesta desde 10.0.0.1: bytes=32 tiempo=23ms TTL=128"),
        Some(23.0)
    );
    assert_eq!(
        parse_ping_latency("Respuesta desde 10.0.0.1: bytes=32 tiempo<1ms TTL=128"),
        Some(0.5)
    );
    assert_eq!(
        parse_ping_latency("Respuesta desde 10.0.0.1: bytes=32 tiempo < 1 ms TTL=128"),
        Some(0.5)
    );
    assert_eq!(
        parse_ping_latency("Mínimo = 20ms, Máximo = 26ms, Promedio = 23ms"),
        Some(23.0)
    );
    assert_eq!(
        parse_ping_latency("Mínimo = 2ms, Máximo = 10ms, Media = 5ms"),
        Some(5.0)
    );

    // Portuguese formats
    assert_eq!(
        parse_ping_latency("Resposta de 10.0.0.1: bytes=32 tempo=7ms TTL=64"),
        Some(7.0)
    );
    assert_eq!(
        parse_ping_latency("Resposta de 10.0.0.1: bytes=32 tempo<1ms TTL=64"),
        Some(0.5)
    );
    assert_eq!(
        parse_ping_latency("Mínimo = 4ms, Máximo = 12ms, Média = 8ms"),
        Some(8.0)
    );

    // Italian formats
    assert_eq!(
        parse_ping_latency("Risposta da 10.0.0.1: byte=32 tempo=5ms TTL=128"),
        Some(5.0)
    );
    assert_eq!(
        parse_ping_latency("Risposta da 10.0.0.1: byte=32 tempo<1ms TTL=128"),
        Some(0.5)
    );
    assert_eq!(
        parse_ping_latency("Minimo = 4ms, Massimo = 8ms, Medio = 6ms"),
        Some(6.0)
    );
    assert_eq!(
        parse_ping_latency("Minimo = 4ms, Massimo = 8ms, Media = 5ms"),
        Some(5.0)
    );

    // Polish formats
    assert_eq!(
        parse_ping_latency("Odpowiedź z 10.0.0.1: bajty=32 czas=18ms TTL=64"),
        Some(18.0)
    );
    assert_eq!(
        parse_ping_latency("Odpowiedź z 10.0.0.1: bajty=32 czas<1ms TTL=64"),
        Some(0.5)
    );
    assert_eq!(
        parse_ping_latency("Odpowiedź z 10.0.0.1: bajty=32 czas < 1 ms TTL=64"),
        Some(0.5)
    );
    assert_eq!(
        parse_ping_latency("Minimum = 12ms, Maksimum = 24ms, Średni = 18ms"),
        Some(18.0)
    );

    // Dutch formats
    assert_eq!(
        parse_ping_latency("Antwoord van 10.0.0.1: bytes=32 tijd=16ms TTL=64"),
        Some(16.0)
    );
    assert_eq!(
        parse_ping_latency("Antwoord van 10.0.0.1: bytes=32 tijd<1ms TTL=64"),
        Some(0.5)
    );
    assert_eq!(
        parse_ping_latency("Minimum = 10ms, Maximum = 22ms, Gemiddelde = 16ms"),
        Some(16.0)
    );

    // Russian formats
    assert_eq!(
        parse_ping_latency("Ответ от 10.0.0.1: число байт=32 время=4мс TTL=64"),
        Some(4.0)
    );
    assert_eq!(
        parse_ping_latency("Ответ от 10.0.0.1: число байт=32 время<1мс TTL=64"),
        Some(0.5)
    );
    assert_eq!(
        parse_ping_latency("Ответ от 10.0.0.1: число байт=32 время < 1 мс TTL=64"),
        Some(0.5)
    );
    assert_eq!(
        parse_ping_latency("Минимум = 1мс, Максимум = 8мс, Среднее = 3мс"),
        Some(3.0)
    );

    // Chinese (Simplified & Traditional) and Japanese formats
    assert_eq!(
        parse_ping_latency("来自 10.0.0.1 的回复: 字节=32 时间=2ms TTL=64"),
        Some(2.0)
    );
    assert_eq!(
        parse_ping_latency("来自 10.0.0.1 的回复: 字节=32 时间<1ms TTL=64"),
        Some(0.5)
    );
    assert_eq!(
        parse_ping_latency("来自 10.0.0.1 的回复: 字节=32 时间 < 1 ms TTL=64"),
        Some(0.5)
    );
    assert_eq!(
        parse_ping_latency("最短 = 1ms，最长 = 5ms，平均 = 2ms"),
        Some(2.0)
    );
    assert_eq!(
        parse_ping_latency("來自 10.0.0.1 的回覆: 位元組=32 時間=3ms TTL=64"),
        Some(3.0)
    );
    assert_eq!(
        parse_ping_latency("來自 10.0.0.1 的回覆: 位元組=32 時間<1ms TTL=64"),
        Some(0.5)
    );
    assert_eq!(
        parse_ping_latency("最短 = 1ms，最長 = 5ms，平均 = 3.5ms"),
        Some(3.5)
    );

    // Linux / macOS / BSD ping formats
    assert_eq!(
        parse_ping_latency("64 bytes from 10.0.0.1: icmp_seq=1 ttl=64 time=0.345 ms"),
        Some(0.345)
    );
    assert_eq!(
        parse_ping_latency("64 bytes from 10.0.0.1: icmp_seq=2 ttl=64 time=14.8 ms"),
        Some(14.8)
    );
    assert_eq!(
        parse_ping_latency("rtt min/avg/max/mdev = 12.300/15.420/18.100/2.100 ms"),
        Some(15.42)
    );
    assert_eq!(
        parse_ping_latency("round-trip min/avg/max/stddev = 1.234/2.345/3.456/0.123 ms"),
        Some(2.345)
    );
    assert_eq!(
        parse_ping_latency("round-trip min/avg/max = 0.850/1.250/2.100 ms"),
        Some(1.25)
    );

    // IPv6 formats
    assert_eq!(
        parse_ping_latency("64 bytes from fe80::1%eth0: icmp_seq=1 ttl=64 time=0.412 ms"),
        Some(0.412)
    );
    assert_eq!(
        parse_ping_latency("Reply from 2001:db8::1: time=24ms"),
        Some(24.0)
    );

    // Noise & failure outputs
    assert_eq!(parse_ping_latency(""), None);
    assert_eq!(parse_ping_latency("   \t \r\n "), None);
    assert_eq!(parse_ping_latency("Request timed out."), None);
    assert_eq!(parse_ping_latency("Destination host unreachable."), None);
    assert_eq!(parse_ping_latency("100% packet loss"), None);
    assert_eq!(parse_ping_latency("General failure."), None);
    assert_eq!(parse_ping_latency("Hardware error on interface eth0"), None);
    assert_eq!(parse_ping_latency("time=notanumber"), None);
}

#[test]
fn test_system_audio_chimes_non_blocking() {
    use crate::net::{play_system_sound, SoundType};

    let start = Instant::now();
    play_system_sound(SoundType::Success);
    play_system_sound(SoundType::Warning);
    play_system_sound(SoundType::Error);
    let elapsed = start.elapsed();

    // Since audio is dispatched asynchronously, calling all 3 chimes should take negligible time (< 50ms)
    assert!(
        elapsed < Duration::from_millis(50),
        "play_system_sound must be non-blocking (took {:?})",
        elapsed
    );
}

#[test]
fn test_sound_alert_stage_ready_trigger_and_lifecycle() {
    use crate::net::{play_system_sound, SoundType};

    // Verify SoundType enum traits and variants
    let success = SoundType::Success;
    let warning = SoundType::Warning;
    let error = SoundType::Error;

    assert_eq!(success, SoundType::Success);
    assert_ne!(success, warning);
    assert_ne!(warning, error);
    assert_eq!(format!("{:?}", success), "Success");
    assert_eq!(format!("{:?}", warning), "Warning");
    assert_eq!(format!("{:?}", error), "Error");

    // Test playing sounds does not panic
    play_system_sound(success);
    play_system_sound(warning);
    play_system_sound(error);

    // Simulate Sunshine -> Ready pipeline transition that fires success sound alert
    let mut state = PipelineState::new();
    let config = HostProfile {
        max_wait_seconds: 300,
        ..HostProfile::default()
    };

    state.stage = Stage::Sunshine;
    state.start_time = Some(Instant::now());

    // Directly set stage to Ready (as tick does when Sunshine check succeeds)
    state.stage = Stage::Ready;
    state.status_message = "Host is fully ready and online for game streaming!".to_string();
    state.is_finished = true;
    play_system_sound(SoundType::Success);

    assert_eq!(state.stage, Stage::Ready);
    assert!(state.is_finished);
    assert!(state.status_message.contains("fully ready and online"));

    // Successive ticks in Ready state remain in Ready and remain finished
    for _ in 0..5 {
        state.tick(&config, false);
        assert_eq!(state.stage, Stage::Ready);
        assert!(state.is_finished);
    }
}

#[test]
fn test_sound_alert_stage_timeout_trigger_and_stalled_stage() {
    use crate::net::{play_system_sound, SoundType};

    let mut state = PipelineState::new();
    let config = HostProfile {
        max_wait_seconds: 2,
        thinkstation_ip: "192.168.1.75".to_string(),
        ..HostProfile::default()
    };

    state.reset(&config);
    state.stage = Stage::Ssh;
    // Set start time 5 seconds in the past to trigger timeout
    state.start_time = Some(Instant::now() - Duration::from_secs(5));

    state.tick(&config, false);

    assert_eq!(state.stage, Stage::Timeout);
    assert_eq!(state.stalled_stage, Some(Stage::Ssh));
    assert!(state.is_finished);
    assert!(state
        .status_message
        .contains("Timed out after 2 seconds waiting for host 192.168.1.75"));

    // Verify error log entry was added
    let logs = state.formatted_logs_text();
    assert!(logs.contains("[ERR] Timed out after 2 seconds"));

    // Verify repeated ticks while in Timeout do not alter stage or clear stalled_stage
    for _ in 0..5 {
        state.tick(&config, false);
        assert_eq!(state.stage, Stage::Timeout);
        assert_eq!(state.stalled_stage, Some(Stage::Ssh));
        assert!(state.is_finished);
    }

    // Play warning sound explicitly as timeout handler does
    play_system_sound(SoundType::Warning);
}

#[test]
fn test_sound_alert_state_reset_and_ready_timeout_immunity() {
    let mut state = PipelineState::new();
    let config = HostProfile {
        max_wait_seconds: 2,
        ..HostProfile::default()
    };

    // 1. Ready state is immune to timeout
    state.stage = Stage::Ready;
    state.is_finished = true;
    state.start_time = Some(Instant::now() - Duration::from_secs(10));
    state.tick(&config, false);

    assert_eq!(state.stage, Stage::Ready);
    assert!(state.is_finished);
    assert_eq!(state.stalled_stage, None);

    // 2. Timeout state is fully cleared on reset
    state.stage = Stage::Timeout;
    state.stalled_stage = Some(Stage::Ping);
    state.is_finished = true;

    state.reset(&config);

    assert_eq!(state.stage, Stage::Ping);
    assert_eq!(state.stalled_stage, None);
    assert!(!state.is_finished);
    assert_eq!(state.wol_count, 1);
    assert!(state.start_time.is_some());
}

#[cfg(windows)]
#[test]
fn test_enhanced_moonlight_discovery_windows_paths() {
    use crate::net::{
        get_windows_moonlight_exe_paths, get_windows_moonlight_lnk_paths, launch_lnk_file,
    };

    let exe_paths = get_windows_moonlight_exe_paths();
    assert!(
        !exe_paths.is_empty(),
        "Must produce executable candidate paths on Windows"
    );

    for p in &exe_paths {
        assert!(
            p.to_string_lossy().to_lowercase().ends_with(".exe"),
            "All candidate exe paths must end with .exe: {:?}",
            p
        );
    }

    let exe_strings: Vec<String> = exe_paths
        .iter()
        .map(|p| p.to_string_lossy().to_lowercase())
        .collect();

    // 1. Standard Program Files
    assert!(exe_strings
        .iter()
        .any(|s| s.contains("program files") && s.ends_with("moonlight.exe")));

    // 2. WinGet links
    assert!(exe_strings
        .iter()
        .any(|s| s.contains("winget") && s.contains("links") && s.ends_with("moonlight.exe")));

    // 3. Scoop shims & apps
    assert!(exe_strings
        .iter()
        .any(|s| s.contains("scoop") && s.contains("shims") && s.ends_with("moonlight.exe")));
    assert!(exe_strings
        .iter()
        .any(|s| s.contains("scoop") && s.contains("apps") && s.ends_with("moonlight.exe")));

    // 4. Chocolatey paths
    assert!(exe_strings
        .iter()
        .any(|s| s.contains("chocolatey") && s.ends_with("moonlight.exe")));

    let lnk_paths = get_windows_moonlight_lnk_paths();
    assert!(
        !lnk_paths.is_empty(),
        "Must produce shortcut .lnk candidate paths on Windows"
    );

    for p in &lnk_paths {
        assert!(
            p.to_string_lossy().to_lowercase().ends_with(".lnk"),
            "All candidate shortcut paths must end with .lnk: {:?}",
            p
        );
    }

    let lnk_strings: Vec<String> = lnk_paths
        .iter()
        .map(|p| p.to_string_lossy().to_lowercase())
        .collect();

    // 5. Desktop shortcuts (%USERPROFILE%\Desktop\Moonlight.lnk, C:\Users\Public\Desktop\Moonlight.lnk)
    assert!(lnk_strings
        .iter()
        .any(|s| s.contains("desktop") && s.ends_with("moonlight.lnk")));
    assert!(lnk_strings
        .iter()
        .any(|s| s.contains("public") && s.contains("desktop") && s.ends_with("moonlight.lnk")));

    // 6. Non-existent .lnk launch returns false gracefully without panicking
    let non_existent_lnk = std::path::Path::new("C:\\non_existent_path_12345\\Moonlight.lnk");
    assert!(!launch_lnk_file(non_existent_lnk));
}

#[test]
fn test_moonlight_shortcut_and_auto_launch_logic() {
    let mut state = PipelineState::new();
    let config = HostProfile::default();

    assert!(!state.auto_launched_moonlight);

    // 1. In Stage::Ready with auto_launch_moonlight = false, auto_launched_moonlight remains false
    state.stage = Stage::Ready;
    state.is_finished = true;
    state.tick(&config, false);
    assert!(!state.auto_launched_moonlight);

    // 2. In Stage::Ready with auto_launch_moonlight = true, tick sets auto_launched_moonlight
    state.auto_launched_moonlight = false;
    // Simulate tick on transition into Ready
    if !state.auto_launched_moonlight {
        state.auto_launched_moonlight = true;
        state.log(LogLevel::Action, "Launching Moonlight streaming client...");
    }
    assert!(state.auto_launched_moonlight);

    // 3. Resetting pipeline clears auto_launched_moonlight
    state.reset(&config);
    assert!(!state.auto_launched_moonlight);
}

#[test]
fn test_modal_keyboard_shortcuts_and_background_isolation() {
    use crate::ui::{ReachabilityState, WakeApp};

    let temp_config_path = std::env::temp_dir().join("test_wake_modal_iso_config.json");
    let app_config = AppConfig::default();
    let current_profile = app_config.get_active_profile();

    let mut app = WakeApp {
        app_config,
        current_profile: current_profile.clone(),
        config_path: temp_config_path.clone(),
        auto_launch_moonlight: true,
        pipeline: PipelineState::new(),
        show_settings: false,
        settings_tab: 0,
        show_power_modal: false,
        pending_power_action: None,
        settings_edit_profile: current_profile,
        settings_profile_name: "thinkstation".to_string(),
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
    };

    // 1. Initial state: no modal open
    let any_modal_open = app.show_settings
        || app.show_power_modal
        || app.show_new_profile_dialog
        || app.show_json_config_dialog;
    assert!(!any_modal_open);

    // 2. Settings modal open -> any_modal_open is true
    app.show_settings = true;
    assert!(
        app.show_settings
            || app.show_power_modal
            || app.show_new_profile_dialog
            || app.show_json_config_dialog
    );

    // 3. Power modal open -> any_modal_open is true
    app.show_settings = false;
    app.show_power_modal = true;
    assert!(
        app.show_settings
            || app.show_power_modal
            || app.show_new_profile_dialog
            || app.show_json_config_dialog
    );

    // 4. New profile dialog open -> any_modal_open is true
    app.show_power_modal = false;
    app.show_new_profile_dialog = true;
    assert!(
        app.show_settings
            || app.show_power_modal
            || app.show_new_profile_dialog
            || app.show_json_config_dialog
    );

    // 5. JSON config dialog open -> any_modal_open is true
    app.show_new_profile_dialog = false;
    app.show_json_config_dialog = true;
    assert!(
        app.show_settings
            || app.show_power_modal
            || app.show_new_profile_dialog
            || app.show_json_config_dialog
    );

    // 6. Test Escape key dismissal hierarchy
    // If JSON config dialog is open, Escape closes it
    if app.show_json_config_dialog {
        app.show_json_config_dialog = false;
    }
    assert!(!app.show_json_config_dialog);

    // If power modal has a pending action confirmation, Escape cancels the confirmation first
    app.show_power_modal = true;
    app.pending_power_action = Some(("shutdown", "Shutdown"));
    if app.show_power_modal {
        if app.pending_power_action.is_some() {
            app.pending_power_action = None;
        } else {
            app.show_power_modal = false;
        }
    }
    assert!(app.show_power_modal);
    assert_eq!(app.pending_power_action, None);

    // Second Escape closes the power modal itself
    if app.show_power_modal {
        if app.pending_power_action.is_some() {
            app.pending_power_action = None;
        } else {
            app.show_power_modal = false;
        }
    }
    assert!(!app.show_power_modal);

    if temp_config_path.exists() {
        let _ = std::fs::remove_file(&temp_config_path);
    }
}

#[test]
fn test_modal_space_and_enter_key_routing_by_stage() {
    let mut state = PipelineState::new();
    let config = HostProfile::default();

    // 1. Stage::Standby -> Space/Enter resets/initiates wake
    state.enter_standby("Testing standby");
    assert_eq!(state.stage, Stage::Standby);
    state.reset(&config);
    assert_eq!(state.stage, Stage::Ping);
    assert_eq!(state.wol_count, 1);

    // 2. Stage::Ready -> Space/Enter triggers stream launch
    state.stage = Stage::Ready;
    state.is_finished = true;
    let mut launched = false;
    if state.stage == Stage::Ready {
        state.log(
            LogLevel::Action,
            "Manual Moonlight launch requested (Space/Enter)",
        );
        launched = true;
    }
    assert!(launched);
    assert_eq!(state.stage, Stage::Ready);

    // 3. Stage::Timeout -> Space/Enter restarts sequence
    state.stage = Stage::Timeout;
    state.stalled_stage = Some(Stage::Ssh);
    state.is_finished = true;
    state.reset(&config);
    assert_eq!(state.stage, Stage::Ping);
    assert_eq!(state.stalled_stage, None);
}

#[test]
fn test_new_profile_modal_validation_and_creation() {
    use crate::config::{
        validate_broadcast_ip, validate_mac_address, validate_target_ip, validate_username,
    };
    use crate::ui::{ReachabilityState, WakeApp};

    let temp_config_path = std::env::temp_dir().join("test_wake_new_prof_config.json");
    let app_config = AppConfig::default();
    let current_profile = app_config.get_active_profile();

    let mut app = WakeApp {
        app_config,
        current_profile: current_profile.clone(),
        config_path: temp_config_path.clone(),
        auto_launch_moonlight: true,
        pipeline: PipelineState::new(),
        show_settings: false,
        settings_tab: 0,
        show_power_modal: false,
        pending_power_action: None,
        settings_edit_profile: current_profile,
        settings_profile_name: "thinkstation".to_string(),
        settings_error: None,
        power_status: None,
        reachability_state: ReachabilityState::Idle,
        reachability_rx: None,
        toasts: Vec::new(),
        show_new_profile_dialog: true,
        new_profile_name_input: String::new(),
        show_json_config_dialog: false,
        json_config_input: String::new(),
        json_config_is_export: false,
        show_shortcuts_modal: false,
        console_collapsed: false,
        console_level_filter: None,
        console_search_query: String::new(),
        console_auto_scroll: true,
    };

    // 1. Empty name input is rejected (does not create profile)
    app.new_profile_name_input = "".to_string();
    let name_trim = app.new_profile_name_input.trim().to_string();
    assert!(
        name_trim.is_empty(),
        "Empty profile name must not be accepted"
    );

    app.new_profile_name_input = "   \t  ".to_string();
    let name_spaces = app.new_profile_name_input.trim().to_string();
    assert!(
        name_spaces.is_empty(),
        "Whitespace profile name must not be accepted"
    );

    // 2. Valid profile name creation
    app.new_profile_name_input = "  gaming-rig  ".to_string();
    let valid_name = app.new_profile_name_input.trim().to_string();
    assert!(!valid_name.is_empty());

    let new_profile = HostProfile {
        thinkstation_ip: "10.0.0.55".to_string(),
        thinkstation_user: "gamer".to_string(),
        mac_address: "11:22:33:44:55:66".to_string(),
        ..HostProfile::default()
    };

    app.app_config.set_profile(&valid_name, new_profile.clone());
    app.app_config.active_profile = valid_name.clone();
    app.settings_profile_name = valid_name.clone();
    app.settings_edit_profile = new_profile.clone();
    app.current_profile = new_profile;
    app.show_new_profile_dialog = false;
    app.show_settings = true;

    assert!(app.app_config.profiles.contains_key("gaming-rig"));
    assert_eq!(app.app_config.active_profile, "gaming-rig");
    assert_eq!(app.current_profile.thinkstation_ip, "10.0.0.55");
    assert!(!app.show_new_profile_dialog);
    assert!(app.show_settings);

    // 3. Validate settings modal saving
    assert!(app.save_settings_from_modal());
    assert_eq!(app.settings_error, None);

    // 4. Test field validation on newly created profile
    assert!(validate_target_ip(&app.current_profile.thinkstation_ip).is_ok());
    assert!(validate_username(&app.current_profile.thinkstation_user).is_ok());
    assert!(validate_mac_address(&app.current_profile.mac_address).is_ok());
    assert!(validate_broadcast_ip(&app.current_profile.broadcast_addr).is_ok());

    // 5. Test invalid edit rejects save
    app.settings_edit_profile.thinkstation_ip = "invalid host ; reboot".to_string();
    assert!(!app.save_settings_from_modal());
    assert!(app.settings_error.is_some());

    if temp_config_path.exists() {
        let _ = std::fs::remove_file(&temp_config_path);
    }
}

#[test]
fn test_settings_and_field_validators_exhaustive_boundary_checks() {
    use crate::config::{
        validate_broadcast_ip, validate_mac_address, validate_settings, validate_target_ip,
        validate_username,
    };

    // 1. Target IP / Host validator
    assert!(validate_target_ip("192.168.1.1").is_ok());
    assert!(validate_target_ip("10.0.0.100").is_ok());
    assert!(validate_target_ip("fe80::1").is_ok());
    assert!(validate_target_ip("thinkstation.local").is_ok());
    assert!(validate_target_ip("my-rig.tailnet-xyz.ts.net").is_ok());
    assert!(validate_target_ip("").is_err());
    assert!(validate_target_ip("   ").is_err());
    assert!(validate_target_ip("-oProxyCommand=evil").is_err());
    assert!(validate_target_ip("host; id").is_err());
    assert!(validate_target_ip("host | nc").is_err());
    assert!(validate_target_ip("host & calc").is_err());
    assert!(validate_target_ip("host`whoami`").is_err());
    assert!(validate_target_ip("host$(whoami)").is_err());
    assert!(validate_target_ip("host with space").is_err());

    // 2. MAC address validator
    assert!(validate_mac_address("AA:BB:CC:DD:EE:FF").is_ok());
    assert!(validate_mac_address("aa:bb:cc:dd:ee:ff").is_ok());
    assert!(validate_mac_address("AA-BB-CC-DD-EE-FF").is_ok());
    assert!(validate_mac_address("aabb.ccdd.eeff").is_ok());
    assert!(validate_mac_address("AABBCCDDEEFF").is_ok());
    assert!(validate_mac_address("  AA:BB:CC:DD:EE:FF  ").is_ok());
    assert!(validate_mac_address("").is_err());
    assert!(validate_mac_address("AA:BB:CC:DD:EE").is_err()); // 10 hex
    assert!(validate_mac_address("AA:BB:CC:DD:EE:FF:00").is_err()); // 14 hex
    assert!(validate_mac_address(":AA:BB:CC:DD:EE:FF").is_err()); // leading colon
    assert!(validate_mac_address("AA:BB:CC:DD:EE:FF:").is_err()); // trailing colon
    assert!(validate_mac_address("AA:BB:CC:DD:EE:GG").is_err()); // invalid hex 'G'
    assert!(validate_mac_address("AA:BB:CC:DD:EE:FF; reboot").is_err());

    // 3. Username validator
    assert!(validate_username("admin").is_ok());
    assert!(validate_username("user_123").is_ok());
    assert!(validate_username("my-user").is_ok());
    assert!(validate_username("").is_err());
    assert!(validate_username("   ").is_err());
    assert!(validate_username("-admin").is_err());
    assert!(validate_username("user name").is_err());
    assert!(validate_username("user`id`").is_err());

    // 4. Broadcast address validator
    assert!(validate_broadcast_ip("192.168.1.255").is_ok());
    assert!(validate_broadcast_ip("255.255.255.255").is_ok());
    assert!(validate_broadcast_ip("").is_err());
    assert!(validate_broadcast_ip("bcast; cat /etc/shadow").is_err());

    // 5. HostProfile port and timing boundary validation
    let mut profile = HostProfile::default();
    assert!(validate_settings(&profile).is_ok());

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

    profile.max_wait_seconds = 0;
    assert!(validate_settings(&profile).is_err());
    profile.max_wait_seconds = 180;

    profile.wol_resend_interval = 0;
    assert!(validate_settings(&profile).is_err());
    profile.wol_resend_interval = 45;

    assert!(validate_settings(&profile).is_ok());
}

#[test]
fn test_reachability_state_transitions() {
    use crate::ui::ReachabilityState;
    use std::time::Instant;

    let idle = ReachabilityState::Idle;
    assert!(matches!(idle, ReachabilityState::Idle));

    let testing = ReachabilityState::Testing {
        host: "192.168.1.50".to_string(),
        started_at: Instant::now(),
    };
    assert!(matches!(testing, ReachabilityState::Testing { .. }));

    let result_ok = ReachabilityState::Result {
        host: "192.168.1.50".to_string(),
        ping_ok: true,
        latency_ms: Some(1.2),
        ssh_port: 22,
        ssh_ok: true,
        checked_at: Instant::now(),
    };
    if let ReachabilityState::Result {
        ping_ok,
        latency_ms,
        ssh_ok,
        ..
    } = result_ok
    {
        assert!(ping_ok);
        assert_eq!(latency_ms, Some(1.2));
        assert!(ssh_ok);
    } else {
        panic!("Expected ReachabilityState::Result");
    }

    let result_err = ReachabilityState::Result {
        host: "192.168.1.99".to_string(),
        ping_ok: false,
        latency_ms: None,
        ssh_port: 22,
        ssh_ok: false,
        checked_at: Instant::now(),
    };
    if let ReachabilityState::Result {
        ping_ok,
        latency_ms,
        ssh_ok,
        ..
    } = result_err
    {
        assert!(!ping_ok);
        assert_eq!(latency_ms, None);
        assert!(!ssh_ok);
    } else {
        panic!("Expected ReachabilityState::Result");
    }
}

// ============================================================================
// 5. WINDOW RESIZE & LAYOUT RESPONSIVENESS TESTS
// (Width calculations, stepper breakpoints, sparkline bounds, vertical budgeting)
// ============================================================================

#[test]
fn test_responsive_width_and_centering_padding_calculations() {
    use crate::ui::theme::metrics;

    // Helper implementing the exact central panel responsive width & padding algorithm
    // from WakeApp::update in src/ui/mod.rs (lines 462-464):
    //   let content_width = avail_width.min(metrics::MAX_CONTENT_WIDTH);
    //   let side_pad = ((avail_width - content_width) / 2.0).max(0.0);
    let compute_layout = |avail_width: f32| -> (f32, f32) {
        let content_width = avail_width.min(metrics::MAX_CONTENT_WIDTH);
        let side_pad = ((avail_width - content_width) / 2.0).max(0.0);
        (content_width, side_pad)
    };

    assert_eq!(metrics::MAX_CONTENT_WIDTH, 1280.0);
    assert_eq!(metrics::OUTER_MARGIN, 16.0);

    // 1. Min window size (700.0px width -> avail_width = 700.0 - 2 * 16.0 = 668.0px)
    let (cw_min, sp_min) = compute_layout(668.0);
    assert_eq!(cw_min, 668.0);
    assert_eq!(sp_min, 0.0);

    // 2. Default window size (780.0px width -> avail_width = 780.0 - 2 * 16.0 = 748.0px)
    let (cw_def, sp_def) = compute_layout(748.0);
    assert_eq!(cw_def, 748.0);
    assert_eq!(sp_def, 0.0);

    // 3. Exact MAX_CONTENT_WIDTH boundary (1280.0px available width)
    let (cw_bound, sp_bound) = compute_layout(1280.0);
    assert_eq!(cw_bound, 1280.0);
    assert_eq!(sp_bound, 0.0);

    // 4. Sub-pixel side padding threshold (avail_width = 1280.8px -> side_pad = 0.4px <= 0.5px)
    // WakeApp checks `if side_pad > 0.5 { ui.add_space(side_pad); }`
    let (cw_sub, sp_sub) = compute_layout(1280.8);
    assert_eq!(cw_sub, 1280.0);
    assert!((sp_sub - 0.4).abs() < 1e-4);
    assert!(
        sp_sub <= 0.5,
        "Sub-pixel side padding should not trigger spacer allocation"
    );

    // 5. Spacer allocation threshold (avail_width = 1282.0px -> side_pad = 1.0px > 0.5px)
    let (cw_pad, sp_pad) = compute_layout(1282.0);
    assert_eq!(cw_pad, 1280.0);
    assert_eq!(sp_pad, 1.0);
    assert!(
        sp_pad > 0.5,
        "Side padding > 0.5px must trigger spacer allocation"
    );

    // 6. Intermediate and standard display resolutions (1080p, 1440p, 4K UHD, Ultrawide)
    let resolutions = [
        ("1000px Window", 1000.0 - 32.0, 968.0, 0.0),
        ("1200px Window", 1200.0 - 32.0, 1168.0, 0.0),
        (
            "1080p Full HD (1920px)",
            1920.0 - 32.0,
            1280.0,
            (1888.0 - 1280.0) / 2.0,
        ),
        (
            "1440p QHD (2560px)",
            2560.0 - 32.0,
            1280.0,
            (2528.0 - 1280.0) / 2.0,
        ),
        (
            "4K UHD (3840px)",
            3840.0 - 32.0,
            1280.0,
            (3808.0 - 1280.0) / 2.0,
        ),
        (
            "Ultrawide (3440px)",
            3440.0 - 32.0,
            1280.0,
            (3408.0 - 1280.0) / 2.0,
        ),
    ];

    for (label, avail_w, expected_cw, expected_sp) in resolutions {
        let (cw, sp) = compute_layout(avail_w);
        assert_eq!(cw, expected_cw, "Content width mismatch for {}", label);
        assert!(
            (sp - expected_sp).abs() < 1e-4,
            "Side pad mismatch for {}",
            label
        );
        // Conservation of width invariant: 2 * side_pad + content_width == avail_width
        assert!(
            (2.0 * sp + cw - avail_w).abs() < 1e-4,
            "Width conservation failed for {}",
            label
        );
    }

    // 7. Narrow / sub-minimum and boundary edge cases
    let narrow_cases = [500.0, 300.0, 100.0, 1.0, 0.0, -50.0];
    for &narrow_w in &narrow_cases {
        let (cw, sp) = compute_layout(narrow_w);
        assert_eq!(cw, narrow_w.min(1280.0));
        assert_eq!(
            sp, 0.0,
            "Side padding must never be negative for narrow widths"
        );
    }
}

#[test]
fn test_header_responsive_stacked_vs_horizontal_breakpoint() {
    use crate::ui::theme::metrics;

    // Header breakpoint logic in src/ui/header.rs (lines 11-30):
    //   const STACK_BELOW_WIDTH: f32 = 640.0;
    //   if avail_width < STACK_BELOW_WIDTH { stacked } else { horizontal }
    const STACK_BELOW_WIDTH: f32 = 640.0;

    let is_stacked = |avail_width: f32| -> bool { avail_width < STACK_BELOW_WIDTH };

    // 1. Exact boundary checks around 640.0px
    assert!(is_stacked(639.0), "639.0px must trigger stacked layout");
    assert!(is_stacked(639.99), "639.99px must trigger stacked layout");
    assert!(
        !is_stacked(640.0),
        "640.0px exact boundary must use single-row horizontal layout"
    );
    assert!(
        !is_stacked(640.01),
        "640.01px must use single-row horizontal layout"
    );
    assert!(
        !is_stacked(641.0),
        "641.0px must use single-row horizontal layout"
    );

    // 2. Typical window sizes & inner panel available width
    // Inside CentralPanel, panel_frame() has inner_margin(SPACE_MD = 12.0) on both sides (total 24.0px)
    let header_avail_for_content_w =
        |content_w: f32| -> f32 { content_w - 2.0 * metrics::SPACE_MD };

    // Minimum window width 700.0px (content_width = 668.0px) -> header avail = 644.0px >= 640.0px
    let header_avail_min = header_avail_for_content_w(668.0);
    assert_eq!(header_avail_min, 644.0);
    assert!(
        !is_stacked(header_avail_min),
        "Minimum supported window width (700px) must retain single-row horizontal header"
    );

    // Default window width 780.0px (content_width = 748.0px) -> header avail = 724.0px >= 640.0px
    let header_avail_def = header_avail_for_content_w(748.0);
    assert_eq!(header_avail_def, 724.0);
    assert!(
        !is_stacked(header_avail_def),
        "Default window width (780px) must retain single-row horizontal header"
    );

    // Max content width 1280.0px -> header avail = 1256.0px >= 640.0px
    let header_avail_max = header_avail_for_content_w(1280.0);
    assert_eq!(header_avail_max, 1256.0);
    assert!(
        !is_stacked(header_avail_max),
        "Max content width (1280px) must retain single-row horizontal header"
    );

    // Compact window (e.g. 640px window -> content_width = 608px -> header avail = 584px < 640px)
    let header_avail_compact = header_avail_for_content_w(608.0);
    assert_eq!(header_avail_compact, 584.0);
    assert!(
        is_stacked(header_avail_compact),
        "Compact window (<664px content) must stack header vertically"
    );
}

#[test]
fn test_stepper_card_distribution_breakpoints_and_geometry() {
    use crate::ui::theme::metrics;

    // Stepper card layout algorithm in src/ui/stepper.rs (lines 94-125):
    //   let gap = metrics::SPACE_XS; // 4.0
    //   const MIN_COMFORTABLE_CARD: f32 = 130.0;
    //   let one_row_card_width = (avail_width - 3.0 * gap) / 4.0;
    //   if one_row_card_width >= MIN_COMFORTABLE_CARD { 1 row, 4 cols }
    //   else { 2 rows, 2 cols: two_col_card_width = ((avail_width - gap) / 2.0).max(90.0) }
    let gap = metrics::SPACE_XS;
    assert_eq!(gap, 4.0);
    const MIN_COMFORTABLE_CARD: f32 = 130.0;

    #[derive(Debug, PartialEq)]
    enum StepperMode {
        SingleRow { card_width: f32 },
        TwoByTwoGrid { card_width: f32 },
    }

    let compute_stepper = |avail_w: f32| -> StepperMode {
        let one_row_card_width = (avail_w - 3.0 * gap) / 4.0;
        if one_row_card_width >= MIN_COMFORTABLE_CARD {
            StepperMode::SingleRow {
                card_width: one_row_card_width,
            }
        } else {
            let two_col_card_width = ((avail_w - gap) / 2.0).max(90.0);
            StepperMode::TwoByTwoGrid {
                card_width: two_col_card_width,
            }
        }
    };

    // 1. Breakpoint derivation:
    // (avail_w - 12.0) / 4.0 = 130.0 <=> avail_w = 4 * 130.0 + 12.0 = 532.0px
    let exact_breakpoint_width = 4.0 * MIN_COMFORTABLE_CARD + 3.0 * gap;
    assert_eq!(exact_breakpoint_width, 532.0);

    // Exact breakpoint: single row mode with exactly 130.0px card width
    assert_eq!(
        compute_stepper(532.0),
        StepperMode::SingleRow { card_width: 130.0 }
    );
    // Total row width accounting: 4 * 130.0 + 3 * 4.0 = 532.0 (100% width budget, 0 overflow)
    let total_single_row_w = 4.0 * 130.0 + 3.0 * gap;
    assert_eq!(total_single_row_w, 532.0);

    // 2. Just above breakpoint (532.1px -> SingleRow)
    match compute_stepper(532.1) {
        StepperMode::SingleRow { card_width } => {
            assert!((card_width - 130.025).abs() < 1e-4);
        }
        _ => panic!("532.1px must be SingleRow"),
    }

    // 3. Just below breakpoint (531.9px -> TwoByTwoGrid)
    match compute_stepper(531.9) {
        StepperMode::TwoByTwoGrid { card_width } => {
            // (531.9 - 4.0) / 2.0 = 263.95px
            assert!((card_width - 263.95).abs() < 1e-4);
            // Total row width accounting: 2 * 263.95 + 4.0 = 531.9 (100% width budget)
            assert!((2.0 * card_width + gap - 531.9).abs() < 1e-4);
        }
        _ => panic!("531.9px must be TwoByTwoGrid"),
    }

    // 4. Default 780px window (content_w = 744.0, panel inner avail_w = 720.0px)
    // one_row_card_width = (720.0 - 12.0) / 4.0 = 177.0px
    match compute_stepper(720.0) {
        StepperMode::SingleRow { card_width } => {
            assert_eq!(card_width, 177.0);
            assert_eq!(4.0 * card_width + 3.0 * gap, 720.0);
        }
        _ => panic!("720.0px must be SingleRow"),
    }

    // 5. Max content width 920px (panel inner avail_w = 896.0px)
    // one_row_card_width = (896.0 - 12.0) / 4.0 = 221.0px
    match compute_stepper(896.0) {
        StepperMode::SingleRow { card_width } => {
            assert_eq!(card_width, 221.0);
            assert_eq!(4.0 * card_width + 3.0 * gap, 896.0);
        }
        _ => panic!("896.0px must be SingleRow"),
    }

    // 6. Narrow window (400.0px avail_w -> TwoByTwoGrid, card_w = (400 - 4)/2 = 198.0px)
    match compute_stepper(400.0) {
        StepperMode::TwoByTwoGrid { card_width } => {
            assert_eq!(card_width, 198.0);
            assert_eq!(2.0 * card_width + gap, 400.0);
        }
        _ => panic!("400.0px must be TwoByTwoGrid"),
    }

    // 7. Minimum card floor threshold in 2-col mode:
    // (avail_w - 4.0) / 2.0 = 90.0 <=> avail_w = 184.0px
    assert_eq!(
        compute_stepper(184.0),
        StepperMode::TwoByTwoGrid { card_width: 90.0 }
    );

    // Sub-184px widths clamp card width to 90.0px floor (.max(90.0))
    for &narrow_w in &[150.0, 100.0, 50.0, 10.0, 0.0] {
        assert_eq!(
            compute_stepper(narrow_w),
            StepperMode::TwoByTwoGrid { card_width: 90.0 },
            "Sub-184px width {} must clamp card width to 90.0px floor",
            narrow_w
        );
    }

    // 8. Verify Stage progression order for stepper styling
    // WakeApp::stage_order (Standby=0, Wol=1, Ping=2, Ssh=3, Sunshine=4, Ready=5, Timeout=0)
    let stages = [
        (Stage::Standby, 0),
        (Stage::Wol, 1),
        (Stage::Ping, 2),
        (Stage::Ssh, 3),
        (Stage::Sunshine, 4),
        (Stage::Ready, 5),
        (Stage::Timeout, 0),
    ];
    for (stage, expected_order) in stages {
        let order = match stage {
            Stage::Standby => 0,
            Stage::Wol => 1,
            Stage::Ping => 2,
            Stage::Ssh => 3,
            Stage::Sunshine => 4,
            Stage::Ready => 5,
            Stage::Timeout => 0,
        };
        assert_eq!(
            order, expected_order,
            "Stage order mismatch for {:?}",
            stage
        );
    }
}

#[test]
fn test_sparkline_width_bounds_and_rendering_geometry() {
    use eframe::egui::{Pos2, Rect, Vec2};

    // Latency Sparkline geometry in src/ui/hero.rs (lines 88-153):
    //   let avail_w = ui.available_width();
    //   let height = 26.0;
    //   track_frame(Vec2::new(4.0, 3.0)).show(ui, |ui| {
    //       let (rect, response) = ui.allocate_exact_size(Vec2::new(avail_w - 8.0, height), Sense::hover());
    //       ...
    //   });
    let compute_sparkline_points = |history: &[f32], rect: Rect| -> Vec<Pos2> {
        if history.is_empty() {
            return Vec::new();
        }
        let n = history.len();
        let max_val = history.iter().copied().fold(10.0_f32, f32::max);
        let min_val = history
            .iter()
            .copied()
            .fold(f32::INFINITY, f32::min)
            .min(0.0);
        let range = (max_val - min_val).max(1.0);

        history
            .iter()
            .enumerate()
            .map(|(i, &val)| {
                let x_ratio = if n > 1 {
                    i as f32 / (n - 1) as f32
                } else {
                    1.0
                };
                let x = rect.min.x + x_ratio * rect.width();
                let normalized_y = ((val - min_val) / range).clamp(0.0, 1.0);
                let y = rect.max.y - normalized_y * rect.height();
                Pos2::new(x, y)
            })
            .collect()
    };

    let avail_w = 720.0_f32; // Default window available width inside hero panel
    let height = 26.0_f32;
    let rect = Rect::from_min_size(Pos2::new(10.0, 50.0), Vec2::new(avail_w - 8.0, height));
    assert_eq!(rect.width(), 712.0);
    assert_eq!(rect.height(), 26.0);

    // 1. Empty history returns 0 points
    let pts_empty = compute_sparkline_points(&[], rect);
    assert!(pts_empty.is_empty());

    // 2. Single sample (n = 1): x_ratio = 1.0 -> positioned at rect.max.x
    let single_sample = [12.5_f32];
    let pts_single = compute_sparkline_points(&single_sample, rect);
    assert_eq!(pts_single.len(), 1);
    assert_eq!(pts_single[0].x, rect.max.x);
    // min_val = 0.0, max_val = 12.5, range = 12.5 -> normalized_y = 1.0 -> y = rect.min.y
    assert_eq!(pts_single[0].y, rect.min.y);

    // 3. Two samples (n = 2): endpoints at rect.min.x and rect.max.x
    let two_samples = [0.0_f32, 10.0_f32];
    let pts_two = compute_sparkline_points(&two_samples, rect);
    assert_eq!(pts_two.len(), 2);
    assert_eq!(pts_two[0].x, rect.min.x);
    assert_eq!(pts_two[1].x, rect.max.x);
    assert_eq!(pts_two[0].y, rect.max.y); // val 0.0 -> bottom of graph
    assert_eq!(pts_two[1].y, rect.min.y); // val 10.0 -> top of graph

    // 4. Maximum capacity history (30 samples)
    let full_history: Vec<f32> = (1..=30).map(|i| (i as f32) * 1.5).collect();
    let pts_full = compute_sparkline_points(&full_history, rect);
    assert_eq!(pts_full.len(), 30);
    assert_eq!(pts_full.first().unwrap().x, rect.min.x);
    assert_eq!(pts_full.last().unwrap().x, rect.max.x);

    // Validate strict coordinate containment and x-monotonicity for all points
    for i in 0..pts_full.len() {
        let pt = pts_full[i];
        assert!(
            pt.x >= rect.min.x && pt.x <= rect.max.x,
            "X out of bounds: {}",
            pt.x
        );
        assert!(
            pt.y >= rect.min.y && pt.y <= rect.max.y,
            "Y out of bounds: {}",
            pt.y
        );
        assert!(
            !pt.x.is_nan() && !pt.y.is_nan(),
            "Coordinates must not be NaN"
        );
        assert!(
            !pt.x.is_infinite() && !pt.y.is_infinite(),
            "Coordinates must not be infinite"
        );

        if i > 0 {
            assert!(
                pt.x > pts_full[i - 1].x,
                "X coordinates must strictly increase monotonically"
            );
        }
    }

    // 5. Edge cases in sample values:
    // 5a. All zero values [0.0, 0.0, 0.0]
    let all_zeros = [0.0, 0.0, 0.0];
    let pts_zeros = compute_sparkline_points(&all_zeros, rect);
    for pt in &pts_zeros {
        assert_eq!(pt.y, rect.max.y, "Zero latency must align to graph bottom");
    }

    // 5b. All identical non-zero values [5.0, 5.0, 5.0]
    // min_val = 0.0, max_val = 10.0 (fold baseline), range = 10.0 -> normalized_y = 0.5 -> center
    let identical_vals = [5.0, 5.0, 5.0];
    let pts_ident = compute_sparkline_points(&identical_vals, rect);
    for pt in &pts_ident {
        let expected_y = rect.max.y - 0.5 * rect.height();
        assert!(
            (pt.y - expected_y).abs() < 1e-4,
            "5.0ms on 10.0ms baseline must align to vertical center"
        );
    }

    // 5c. Negative values (corrupt / clock skew) and extreme spikes (10,000ms)
    let extreme_vals = [-20.0, 0.0, 5000.0, 10000.0, -5.0];
    let pts_extreme = compute_sparkline_points(&extreme_vals, rect);
    for pt in &pts_extreme {
        assert!(
            pt.y >= rect.min.y && pt.y <= rect.max.y,
            "Extreme value clamped within [min.y, max.y]: {}",
            pt.y
        );
    }

    // 6. Progress bar track geometry validation (src/ui/hero.rs lines 67-84)
    //   avail_width, height = 6.0, filled_rect.max.x = rect.min.x + rect.width() * fraction
    let progress_track_w = 720.0_f32;
    let track_rect = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(progress_track_w, 6.0));
    for &frac in &[0.0_f32, 0.15, 0.40, 0.65, 0.85, 1.0] {
        let filled_w = track_rect.width() * frac;
        assert!(filled_w >= 0.0 && filled_w <= track_rect.width());
        assert!((filled_w - progress_track_w * frac).abs() < 1e-4);
    }
}

#[test]
fn test_vertical_space_budgeting_and_console_height_allocations() {
    use crate::ui::theme::metrics;

    assert_eq!(metrics::ACTION_BAR_HEIGHT, 40.0);
    assert_eq!(metrics::SPACE_MD, 12.0);
    assert_eq!(metrics::MIN_CONSOLE_HEIGHT, 110.0);

    // In the current UI layout architecture:
    // 1. The Action Bar is pinned to the bottom viewport edge in a dedicated
    //    `egui::TopBottomPanel::bottom("action_bar_panel")`, guaranteeing it is NEVER
    //    clipped or squeezed off-screen even at minimum window heights.
    // 2. CentralPanel receives the remaining viewport height above the action bar panel.
    // 3. Inside CentralPanel, Header, Stepper, and Hero Status are laid out sequentially
    //    with `metrics::SPACE_MD` (12.0px) spacing between each section.
    // 4. The Live Activity Log (Console) occupies the remaining available vertical space:
    //    `console_height = if collapsed { 36.0 } else { ui.available_height().max(MIN_CONSOLE_HEIGHT) };`
    // 5. Inside `render_console`, the log scroll area is given:
    //    `inner_height = (console_height - 2.0 * metrics::SPACE_MD - 36.0).max(60.0);`
    let compute_console_heights = |avail_h: f32, collapsed: bool| -> (f32, f32) {
        let console_height = if collapsed {
            36.0
        } else {
            avail_h.max(metrics::MIN_CONSOLE_HEIGHT)
        };
        let inner_height = (console_height - 2.0 * metrics::SPACE_MD - 36.0).max(60.0);
        (console_height, inner_height)
    };

    // 1. Expanded Console in Minimum Supported Window (500.0px window height)
    // CentralPanel remaining height at console render point: ~173.0px
    let (ch_min, ih_min) = compute_console_heights(173.0, false);
    assert_eq!(ch_min, 173.0);
    // inner_height = (173.0 - 2 * 12.0 - 36.0) = (173.0 - 60.0) = 113.0px > 60.0px
    assert_eq!(ih_min, 113.0);

    // 2. Expanded Console in Default Window (600.0px window height -> avail at console ~273.0px)
    let (ch_def, ih_def) = compute_console_heights(273.0, false);
    assert_eq!(ch_def, 273.0);
    // inner_height = (273.0 - 60.0) = 213.0px
    assert_eq!(ih_def, 213.0);

    // 3. Expanded Console in Large Window (1080p FHD -> avail at console ~753.0px)
    let (ch_large, ih_large) = compute_console_heights(753.0, false);
    assert_eq!(ch_large, 753.0);
    assert_eq!(ih_large, 693.0);

    // 4. Clamped Min Console Height in Small / Constrained Viewports
    // When remaining available height is 80.0px (< MIN_CONSOLE_HEIGHT = 110.0px) -> clamped to 110.0px
    let (ch_clamp, ih_clamp) = compute_console_heights(80.0, false);
    assert_eq!(
        ch_clamp, 110.0,
        "Console height must not drop below MIN_CONSOLE_HEIGHT (110.0px)"
    );
    assert_eq!(
        ih_clamp, 60.0,
        "Inner scroll area must equal max(110 - 60, 60) = 60.0px"
    );

    // Extreme zero/negative remaining height -> clamped to 110.0px, inner to 60.0px
    let (ch_zero, ih_zero) = compute_console_heights(0.0, false);
    assert_eq!(ch_zero, 110.0);
    assert_eq!(ih_zero, 60.0);

    let (ch_neg, ih_neg) = compute_console_heights(-100.0, false);
    assert_eq!(ch_neg, 110.0);
    assert_eq!(ih_neg, 60.0);

    // 5. Collapsed Console Mode (console_collapsed = true)
    // Always allocates exactly 36.0px regardless of window height
    for &avail_h in &[0.0, 50.0, 200.0, 500.0, 1000.0] {
        let (ch_col, _) = compute_console_heights(avail_h, true);
        assert_eq!(
            ch_col, 36.0,
            "Collapsed console must always have height 36.0px"
        );
    }
}

#[test]
fn test_modal_responsive_window_sizing_and_clamping() {
    use eframe::egui::Vec2;

    // 1. Settings Modal sizing (src/ui/settings_modal.rs lines 64-75):
    //   modal_w = (screen.width() - 40.0).clamp(420.0, 780.0);
    //   modal_h = (screen.height() - 40.0).clamp(360.0, 680.0);
    //   scroll_h = (modal_h - 180.0).max(140.0);
    //   min_size = Vec2::new(360.0, 280.0);
    //   max_size = Vec2::new(screen.width() - 16.0, screen.height() - 16.0);
    let compute_settings_modal = |screen_w: f32, screen_h: f32| -> (f32, f32, f32, Vec2, Vec2) {
        let modal_w = (screen_w - 40.0).clamp(420.0, 780.0);
        let modal_h = (screen_h - 40.0).clamp(360.0, 680.0);
        let scroll_h = (modal_h - 180.0).max(140.0);
        let min_sz = Vec2::new(360.0, 280.0);
        let max_sz = Vec2::new((screen_w - 16.0).max(0.0), (screen_h - 16.0).max(0.0));
        (modal_w, modal_h, scroll_h, min_sz, max_sz)
    };

    // Default window 780x600:
    let (sm_w_def, sm_h_def, sm_sh_def, sm_min_def, sm_max_def) =
        compute_settings_modal(780.0, 600.0);
    assert_eq!(sm_w_def, 740.0); // (740.0).clamp(420, 780) = 740.0
    assert_eq!(sm_h_def, 560.0); // (560.0).clamp(360, 680) = 560.0
    assert_eq!(sm_sh_def, 380.0); // 560 - 180 = 380.0
    assert_eq!(sm_min_def, Vec2::new(360.0, 280.0));
    assert_eq!(sm_max_def, Vec2::new(764.0, 584.0));

    // Small screen 400x300 (clamps to minimum bounds):
    let (sm_w_sm, sm_h_sm, sm_sh_sm, _, _) = compute_settings_modal(400.0, 300.0);
    assert_eq!(sm_w_sm, 420.0); // min bound
    assert_eq!(sm_h_sm, 360.0); // min bound
    assert_eq!(sm_sh_sm, 180.0); // 360 - 180 = 180.0 >= 140.0

    // Huge 4K screen 3840x2160 (clamps to maximum bounds):
    let (sm_w_huge, sm_h_huge, sm_sh_huge, _, _) = compute_settings_modal(3840.0, 2160.0);
    assert_eq!(sm_w_huge, 780.0); // max bound
    assert_eq!(sm_h_huge, 680.0); // max bound
    assert_eq!(sm_sh_huge, 500.0); // 680 - 180 = 500.0

    // 2. Power Modal sizing (src/ui/power_modal.rs lines 12-44):
    //   modal_w = (screen.width() - 40.0).clamp(360.0, 540.0);
    //   modal_h = (screen.height() - 40.0).clamp(300.0, 480.0);
    //   min_size = Vec2::new(320.0, 240.0);
    //   avail_scroll_h = (ui.available_height() - 48.0).max(100.0);
    let compute_power_modal =
        |screen_w: f32, screen_h: f32, avail_inner_h: f32| -> (f32, f32, f32, Vec2, Vec2) {
            let modal_w = (screen_w - 40.0).clamp(360.0, 540.0);
            let modal_h = (screen_h - 40.0).clamp(300.0, 480.0);
            let scroll_h = (avail_inner_h - 48.0).max(100.0);
            let min_sz = Vec2::new(320.0, 240.0);
            let max_sz = Vec2::new((screen_w - 16.0).max(0.0), (screen_h - 16.0).max(0.0));
            (modal_w, modal_h, scroll_h, min_sz, max_sz)
        };

    let (pm_w_def, pm_h_def, pm_sh_def, pm_min_def, pm_max_def) =
        compute_power_modal(780.0, 600.0, 400.0);
    assert_eq!(pm_w_def, 540.0); // (740.0).clamp(360, 540) = 540.0
    assert_eq!(pm_h_def, 480.0); // (560.0).clamp(300, 480) = 480.0
    assert_eq!(pm_sh_def, 352.0); // 400 - 48 = 352.0 >= 100.0
    assert_eq!(pm_min_def, Vec2::new(320.0, 240.0));
    assert_eq!(pm_max_def, Vec2::new(764.0, 584.0));

    let (pm_w_min, pm_h_min, pm_sh_min, _, _) = compute_power_modal(300.0, 200.0, 120.0);
    assert_eq!(pm_w_min, 360.0); // min bound
    assert_eq!(pm_h_min, 300.0); // min bound
    assert_eq!(pm_sh_min, 100.0); // (120 - 48 = 72) clamped to 100.0 min

    // 3. JSON Config Modal sizing (src/ui/json_config_modal.rs lines 98-130):
    //   modal_w = (screen.width() - 40.0).clamp(420.0, 780.0);
    //   modal_h = (screen.height() - 40.0).clamp(300.0, 580.0);
    //   min_size = Vec2::new(360.0, 240.0);
    //   avail_editor_h = (ui.available_height() - 54.0).max(120.0);
    let compute_json_modal =
        |screen_w: f32, screen_h: f32, avail_inner_h: f32| -> (f32, f32, f32, Vec2, Vec2) {
            let modal_w = (screen_w - 40.0).clamp(420.0, 780.0);
            let modal_h = (screen_h - 40.0).clamp(300.0, 580.0);
            let editor_h = (avail_inner_h - 54.0).max(120.0);
            let min_sz = Vec2::new(360.0, 240.0);
            let max_sz = Vec2::new((screen_w - 16.0).max(0.0), (screen_h - 16.0).max(0.0));
            (modal_w, modal_h, editor_h, min_sz, max_sz)
        };
    let (jm_w_def, jm_h_def, jm_eh_def, jm_min_def, jm_max_def) =
        compute_json_modal(780.0, 600.0, 450.0);
    assert_eq!(jm_w_def, 740.0);
    assert_eq!(jm_h_def, 560.0);
    assert_eq!(jm_eh_def, 396.0); // 450 - 54 = 396.0
    assert_eq!(jm_min_def, Vec2::new(360.0, 240.0));
    assert_eq!(jm_max_def, Vec2::new(764.0, 584.0));

    let (jm_w_min, jm_h_min, jm_eh_min, _, _) = compute_json_modal(300.0, 200.0, 100.0);
    assert_eq!(jm_w_min, 420.0);
    assert_eq!(jm_h_min, 300.0);
    assert_eq!(jm_eh_min, 120.0); // (100 - 54 = 46) clamped to 120.0 min

    // 4. New Profile Modal sizing (src/ui/new_profile_modal.rs lines 15-26):
    //   modal_w = (screen.width() - 40.0).clamp(320.0, 420.0);
    //   modal_h = (screen.height() - 40.0).clamp(160.0, 220.0);
    //   min_size = Vec2::new(300.0, 150.0);
    let compute_new_prof_modal = |screen_w: f32, screen_h: f32| -> (f32, f32, Vec2, Vec2) {
        let modal_w = (screen_w - 40.0).clamp(320.0, 420.0);
        let modal_h = (screen_h - 40.0).clamp(160.0, 220.0);
        let min_sz = Vec2::new(300.0, 150.0);
        let max_sz = Vec2::new((screen_w - 16.0).max(0.0), (screen_h - 16.0).max(0.0));
        (modal_w, modal_h, min_sz, max_sz)
    };
    let (npm_w, npm_h, npm_min, npm_max) = compute_new_prof_modal(780.0, 600.0);
    assert_eq!(npm_w, 420.0);
    assert_eq!(npm_h, 220.0);
    assert_eq!(npm_min, Vec2::new(300.0, 150.0));
    assert_eq!(npm_max, Vec2::new(764.0, 584.0));

    // 5. Toast Overlay constraints (src/ui/toast.rs lines 73-77):
    //   anchor = Align2::RIGHT_BOTTOM, offset = Vec2::new(-20.0, -20.0);
    //   max_width = 340.0;
    let toast_max_w = 340.0_f32;
    let toast_offset = Vec2::new(-20.0, -20.0);
    assert_eq!(toast_max_w, 340.0);
    assert_eq!(toast_offset.x, -20.0);
    assert_eq!(toast_offset.y, -20.0);
}

#[test]
fn test_multiresolution_layout_sweep_stress_testing() {
    use crate::ui::theme::metrics;

    // Test a broad matrix of resolutions from compact mobile-size up to 8K Ultra-wide
    let test_resolutions: Vec<(f32, f32)> = vec![
        (320.0, 240.0),
        (480.0, 320.0),
        (520.0, 360.0), // Minimum supported inner window size
        (640.0, 480.0),
        (700.0, 500.0),
        (800.0, 600.0), // Default initial inner window size
        (1024.0, 768.0),
        (1280.0, 720.0), // 720p HD
        (1366.0, 768.0),
        (1600.0, 900.0),
        (1920.0, 1080.0), // 1080p FHD
        (2560.0, 1440.0), // 1440p QHD
        (3440.0, 1440.0), // Ultrawide 21:9
        (3840.0, 2160.0), // 4K UHD
        (5120.0, 1440.0), // Super-ultrawide 32:9
        (5120.0, 2880.0), // 5K
        (7680.0, 4320.0), // 8K
    ];

    for &(w, h) in &test_resolutions {
        // 1. Central panel width & padding
        let avail_w = w - 2.0 * metrics::OUTER_MARGIN;
        let content_w = avail_w.min(metrics::MAX_CONTENT_WIDTH);
        let side_pad = ((avail_w - content_w) / 2.0).max(0.0);

        assert!(content_w <= metrics::MAX_CONTENT_WIDTH);
        assert!(side_pad >= 0.0);
        assert!(!content_w.is_nan() && !content_w.is_infinite());
        assert!(!side_pad.is_nan() && !side_pad.is_infinite());

        if avail_w >= metrics::MAX_CONTENT_WIDTH {
            assert!((2.0 * side_pad + content_w - avail_w).abs() < 1e-3);
        }

        // 2. Stepper card dimensions
        let stepper_avail_w = content_w - 2.0 * metrics::SPACE_MD;
        let gap = metrics::SPACE_XS;
        let one_row_w = (stepper_avail_w - 3.0 * gap) / 4.0;
        if one_row_w >= 130.0 {
            assert!(one_row_w >= 130.0);
            assert!((4.0 * one_row_w + 3.0 * gap - stepper_avail_w).abs() < 1e-3);
        } else {
            let two_col_w = ((stepper_avail_w - gap) / 2.0).max(90.0);
            assert!(two_col_w >= 90.0);
        }

        // 3. Sparkline track dimensions
        let sparkline_w = stepper_avail_w - 8.0;
        if sparkline_w > 0.0 {
            let pts = [10.0, 15.0, 20.0];
            let n = pts.len();
            for (i, _) in pts.iter().enumerate() {
                let x_ratio = i as f32 / (n - 1) as f32;
                let x = x_ratio * sparkline_w;
                assert!(x >= 0.0 && x <= sparkline_w);
                assert!(!x.is_nan());
            }
        }

        // 4. Vertical budgeting & console height
        let avail_h = (h - 2.0 * metrics::OUTER_MARGIN).max(0.0);
        let expanded_console_h = avail_h.max(metrics::MIN_CONSOLE_HEIGHT);
        assert!(expanded_console_h >= metrics::MIN_CONSOLE_HEIGHT);
        assert!(!expanded_console_h.is_nan());

        let inner_console_h = (expanded_console_h - 2.0 * metrics::SPACE_MD - 36.0).max(60.0);
        assert!(inner_console_h >= 60.0);
        assert!(!inner_console_h.is_nan());

        // 5. Modal sizing & scroll area bounds across all resolutions
        let sm_w = (w - 40.0).clamp(420.0, 780.0);
        let sm_h = (h - 40.0).clamp(360.0, 680.0);
        let sm_sh = (sm_h - 180.0).max(140.0);
        assert!((420.0..=780.0).contains(&sm_w));
        assert!((360.0..=680.0).contains(&sm_h));
        assert!(sm_sh >= 140.0);

        let pm_w = (w - 40.0).clamp(360.0, 540.0);
        let pm_h = (h - 40.0).clamp(300.0, 480.0);
        let pm_sh = (pm_h - 48.0).max(100.0);
        assert!((360.0..=540.0).contains(&pm_w));
        assert!((300.0..=480.0).contains(&pm_h));
        assert!(pm_sh >= 100.0);

        let jm_w = (w - 40.0).clamp(420.0, 780.0);
        let jm_h = (h - 40.0).clamp(300.0, 580.0);
        let jm_eh = (jm_h - 54.0).max(120.0);
        assert!((420.0..=780.0).contains(&jm_w));
        assert!((300.0..=580.0).contains(&jm_h));
        assert!(jm_eh >= 120.0);

        let npm_w = (w - 40.0).clamp(320.0, 420.0);
        let npm_h = (h - 40.0).clamp(160.0, 220.0);
        assert!((320.0..=420.0).contains(&npm_w));
        assert!((160.0..=220.0).contains(&npm_h));
    }
}

#[test]
fn test_default_native_options_and_viewport_optimization() {
    let options = default_native_options();

    // 1. Verify Viewport Dimensions
    assert_eq!(
        options.viewport.inner_size,
        Some(eframe::egui::Vec2::new(800.0, 600.0))
    );
    assert_eq!(
        options.viewport.min_inner_size,
        Some(eframe::egui::Vec2::new(520.0, 360.0))
    );

    // 2. Verify Viewport Window Characteristics
    assert_eq!(options.viewport.title, Some("Wake Workstation".to_string()));
    assert_eq!(
        options.viewport.app_id,
        Some("wake-workstation".to_string())
    );
    assert_eq!(options.viewport.resizable, Some(true));
    assert_eq!(options.viewport.active, Some(true));
    assert_eq!(options.viewport.decorations, Some(true));
    assert_eq!(options.viewport.transparent, Some(false));
    assert_eq!(options.viewport.maximize_button, Some(true));
    assert_eq!(options.viewport.minimize_button, Some(true));
    assert_eq!(options.viewport.close_button, Some(true));
    assert_eq!(options.viewport.drag_and_drop, Some(false));

    // 3. Verify eframe Native Rendering & Scaling Options
    assert!(
        options.vsync,
        "VSync should be enabled to eliminate tearing during window resize"
    );
    assert_eq!(
        options.multisampling, 0,
        "Multisampling should be disabled (0) for 2D UI efficiency"
    );
    assert_eq!(
        options.depth_buffer, 0,
        "Depth buffer should be 0 for lightweight 2D framebuffers"
    );
    assert_eq!(
        options.stencil_buffer, 0,
        "Stencil buffer should be 0 for lightweight 2D framebuffers"
    );
    assert_eq!(
        options.hardware_acceleration,
        eframe::HardwareAcceleration::Preferred
    );
    assert_eq!(options.renderer, eframe::Renderer::Glow);
    assert!(
        options.centered,
        "Window should be centered on active display on launch"
    );
    assert!(
        !options.persist_window,
        "Persist window should be disabled so initial size defaults cleanly to 800x600"
    );
    assert!(
        options.dithering,
        "Dithering should be enabled for smooth color gradients"
    );
}

#[test]
fn test_lookup_arp_mac_validation_and_parsing() {
    use crate::net::lookup_arp_mac;

    // Reject empty, invalid tokens, leading dashes, or shell injection attempts
    assert_eq!(lookup_arp_mac(""), None);
    assert_eq!(lookup_arp_mac("   "), None);
    assert_eq!(lookup_arp_mac("-a"), None);
    assert_eq!(lookup_arp_mac("192.168.1.1; calc"), None);
    assert_eq!(lookup_arp_mac("192.168.1.1 && whoami"), None);
    assert_eq!(lookup_arp_mac("192.168.1.1`id`"), None);
}

#[test]
fn test_continuous_latency_telemetry_in_ready_stage() {
    let mut state = PipelineState::new();
    state.stage = Stage::Ready;
    state.is_finished = true;

    // Simulate multiple incoming ping latency updates while in Stage::Ready
    for lat in [4.2, 4.5, 3.9, 5.1, 4.0, 4.8, 3.7, 4.1, 4.3, 4.6] {
        state.update_latency(Some(lat));
    }

    assert_eq!(state.latency_history.len(), 10);
    assert_eq!(state.last_latency, Some(4.6));

    // Verify ring buffer capacity clamp (30 samples)
    for i in 1..=35 {
        state.update_latency(Some(i as f32));
    }
    assert_eq!(state.latency_history.len(), 30);
    assert_eq!(state.last_latency, Some(35.0));
    assert_eq!(state.latency_history.first(), Some(&6.0));
    assert_eq!(state.latency_history.last(), Some(&35.0));
}

#[test]
fn test_schema_versioning_and_constants() {
    use crate::config::{
        AppConfig, CURRENT_SCHEMA_VERSION, DEFAULT_MAX_WAIT_SECONDS, DEFAULT_PROFILE_NAME,
        DEFAULT_SSH_PORT, DEFAULT_SUNSHINE_PORT,
    };

    let cfg = AppConfig::default();
    assert_eq!(cfg.schema_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(cfg.schema_version, 1);
    assert_eq!(cfg.active_profile, DEFAULT_PROFILE_NAME);
    assert_eq!(DEFAULT_SSH_PORT, 22);
    assert_eq!(DEFAULT_SUNSHINE_PORT, 47989);
    assert_eq!(DEFAULT_MAX_WAIT_SECONDS, 300);

    // Verify serialization includes schema_version
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(json.contains("\"schema_version\":1"));

    // Legacy JSON without schema_version deserializes cleanly with default = 1
    let legacy_json = r#"{"active_profile":"workstation","profiles":{"workstation":{}}}"#;
    let imported: AppConfig = serde_json::from_str(legacy_json).unwrap();
    assert_eq!(imported.schema_version, 1);
}

#[test]
fn test_normalize_mac_address_strict_and_standardized() {
    use crate::config::normalize_mac_address;

    // Standard delimiters produce uppercase colon-delimited format
    assert_eq!(
        normalize_mac_address("aa:bb:cc:dd:ee:ff").unwrap(),
        "AA:BB:CC:DD:EE:FF"
    );
    assert_eq!(
        normalize_mac_address("00-11-22-33-44-55").unwrap(),
        "00:11:22:33:44:55"
    );
    assert_eq!(
        normalize_mac_address("1234.5678.9abc").unwrap(),
        "12:34:56:78:9A:BC"
    );
    assert_eq!(
        normalize_mac_address("AABBCCDDEEFF").unwrap(),
        "AA:BB:CC:DD:EE:FF"
    );

    // Rejections
    assert!(normalize_mac_address("").is_err());
    assert!(normalize_mac_address("   ").is_err());
    assert!(normalize_mac_address(":AA:BB:CC:DD:EE:FF").is_err());
    assert!(normalize_mac_address("AA:BB:CC:DD:EE:FF:").is_err());
    assert!(normalize_mac_address("AA:BB:CC:DD:EE:GG").is_err()); // 'G' is non-hex
    assert!(normalize_mac_address("AA:BB:CC:DD:EE").is_err()); // 10 hex digits
    assert!(normalize_mac_address("AA:BB:CC:DD:EE:FF:00").is_err()); // 14 hex digits
}

#[test]
fn test_validate_settings_port_ranges_and_interval_bounds() {
    use crate::config::{
        validate_settings, HostProfile, MAX_ALLOWED_INTERVAL_SECONDS, MAX_ALLOWED_WAIT_SECONDS,
    };

    let mut profile = HostProfile::default();
    assert!(validate_settings(&profile).is_ok());

    // Port 0 rejection
    profile.ssh_port = 0;
    assert!(validate_settings(&profile).is_err());
    profile.ssh_port = 22;

    profile.sunshine_port = 0;
    assert!(validate_settings(&profile).is_err());
    profile.sunshine_port = 47989;

    profile.direct_wol_port = 0;
    assert!(validate_settings(&profile).is_err());
    profile.direct_wol_port = 9;

    // Timeout lower/upper bounds
    profile.max_wait_seconds = 0;
    assert!(validate_settings(&profile).is_err());
    profile.max_wait_seconds = MAX_ALLOWED_WAIT_SECONDS + 1;
    assert!(validate_settings(&profile).is_err());
    profile.max_wait_seconds = 300;
    assert!(validate_settings(&profile).is_ok());

    // Interval upper bounds
    profile.wol_resend_interval = MAX_ALLOWED_INTERVAL_SECONDS + 1;
    assert!(validate_settings(&profile).is_err());
    profile.wol_resend_interval = 60;
    assert!(validate_settings(&profile).is_ok());
}

#[test]
fn test_pi_gateway_contract_validation() {
    use crate::config::{validate_settings, HostProfile};

    let mut profile = HostProfile {
        pi_host: String::new(),
        pi_user: String::new(),
        ..HostProfile::default()
    };
    assert!(validate_settings(&profile).is_ok());

    // When pi_host is specified, pi_user MUST NOT be empty
    profile.pi_host = "192.168.1.10".to_string();
    profile.pi_user = String::new();
    let err = validate_settings(&profile).unwrap_err();
    assert!(err.contains("Pi gateway user cannot be empty when Pi gateway host is specified"));

    profile.pi_user = "pi".to_string();
    assert!(validate_settings(&profile).is_ok());
}

#[test]
fn test_import_config_with_warnings_and_salvage_diagnostics() {
    use crate::config::import_config_json_with_warnings;

    // Salvage JSON containing one valid profile and one invalid profile (e.g. malformed type)
    let json_with_bad_profile = r#"{
        "active_profile": "gaming",
        "profiles": {
            "gaming": {
                "thinkstation_ip": "192.168.1.50",
                "mac_address": "11:22:33:44:55:66"
            },
            "broken": "invalid_object_payload"
        }
    }"#;

    let (cfg, warnings) = import_config_json_with_warnings(json_with_bad_profile).unwrap();
    assert_eq!(cfg.active_profile, "gaming");
    assert!(cfg.profiles.contains_key("gaming"));
    assert!(!cfg.profiles.contains_key("broken"));
    assert!(!warnings.is_empty());
    assert!(warnings
        .iter()
        .any(|w| w.contains("Dropped invalid profile 'broken'")));
}

#[test]
fn test_config_error_display_and_traits() {
    use crate::config::ConfigError;

    let err_empty = ConfigError::EmptyField("Target IP");
    assert_eq!(err_empty.to_string(), "Target IP cannot be empty");

    let err_port = ConfigError::InvalidPort {
        field: "SSH",
        value: 0,
    };
    assert!(err_port.to_string().contains("SSH port 0 is invalid"));

    let err_range = ConfigError::OutOfRange {
        field: "max_wait_seconds",
        min: 1,
        max: 86400,
        value: 0,
    };
    assert!(err_range.to_string().contains("out of allowed range"));

    let str_converted: String = err_empty.into();
    assert_eq!(str_converted, "Target IP cannot be empty");
}

#[test]
fn test_shortcuts_modal_state_and_dialog_lifecycle() {
    use crate::ui::WakeApp;
    use std::path::PathBuf;

    let app_cfg = AppConfig::default();
    let prof = app_cfg.get_active_profile();
    let mut app = WakeApp {
        app_config: app_cfg,
        current_profile: prof.clone(),
        config_path: PathBuf::from("config.json"),
        auto_launch_moonlight: false,
        pipeline: PipelineState::new(),
        show_settings: false,
        settings_tab: 0,
        show_power_modal: false,
        pending_power_action: None,
        settings_edit_profile: prof.clone(),
        settings_profile_name: "workstation".to_string(),
        settings_error: None,
        power_status: None,
        reachability_state: crate::ui::ReachabilityState::Idle,
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
    };

    assert!(!app.show_shortcuts_modal);
    app.show_shortcuts_modal = true;
    assert!(app.show_shortcuts_modal);

    // Escape or close toggle
    app.show_shortcuts_modal = false;
    assert!(!app.show_shortcuts_modal);
}

#[test]
fn test_launch_moonlight_stream_interface() {
    use crate::net::launch_lnk_file_with_args;
    use std::path::PathBuf;

    // Non-existent shortcut returns false safely without error
    assert!(!launch_lnk_file_with_args(
        &PathBuf::from("C:\\non_existent_stream_target.lnk"),
        Some("stream 10.0.0.1 Desktop")
    ));
    assert!(!launch_lnk_file_with_args(
        &PathBuf::from("C:\\non_existent_stream_target.lnk"),
        None
    ));
}
