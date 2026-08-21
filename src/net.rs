use regex::Regex;
use std::net::{TcpStream, ToSocketAddrs, UdpSocket};
use std::process::Command;
use std::time::Duration;

/// Validate that a value which will be interpolated into a remote shell
/// command string (or passed as an SSH connection target) contains only a
/// conservative allow-list of characters: ASCII letters, digits, `.`, `:`,
/// `-`, and `_`. This covers valid IPv4/IPv6 addresses, hostnames, Tailscale
/// MagicDNS names, and usernames, while rejecting shell metacharacters
/// (`;`, `|`, `&`, `$`, backticks, quotes, whitespace, newlines, etc.) that
/// could otherwise break out of commands such as `send_ssh_wol`'s
/// `wakeonlan -i {broadcast_ip} {mac}` string once it reaches the remote
/// host's shell. Also rejects values starting with `-` so they cannot be
/// misinterpreted as command-line options by `ssh`/`wakeonlan`.
pub fn validate_host_token(value: &str, field_name: &str) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{} cannot be empty.", field_name));
    }
    if value.starts_with('-') {
        return Err(format!(
            "{} '{}' cannot start with '-' (would be misinterpreted as a command-line option).",
            field_name, value
        ));
    }
    if let Some(bad) = value
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '_')))
    {
        return Err(format!(
            "{} '{}' contains invalid character '{}'. Only letters, digits, '.', ':', '-', and '_' are allowed.",
            field_name, value, bad
        ));
    }
    Ok(())
}

/// Generate pure Rust 102-byte WOL Magic Packet payload.
pub fn create_magic_packet(mac: &str) -> Result<[u8; 102], String> {
    let mac = mac.trim();
    if mac.is_empty() {
        return Err("MAC address cannot be empty.".to_string());
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
            "Invalid MAC address format '{}'. Expected 12 hexadecimal digits.",
            mac
        ));
    }

    let mut mac_bytes = [0u8; 6];
    for i in 0..6 {
        mac_bytes[i] = u8::from_str_radix(&clean_mac[i * 2..i * 2 + 2], 16)
            .map_err(|e| format!("Invalid hex digit in MAC address: {}", e))?;
    }

    let mut packet = [0u8; 102];
    // 6 bytes of 0xFF
    packet[..6].fill(0xFF);
    // 16 repetitions of 6-byte MAC
    for rep in 0..16 {
        let offset = 6 + rep * 6;
        packet[offset..offset + 6].copy_from_slice(&mac_bytes);
    }

    Ok(packet)
}

/// Send a Wake-on-LAN magic packet over local UDP broadcast.
pub fn send_magic_packet(mac: &str, broadcast_ip: &str, port: u16) -> Result<(), String> {
    let packet = create_magic_packet(mac)?;

    let socket = UdpSocket::bind("0.0.0.0:0")
        .map_err(|e| format!("Failed to bind local UDP socket: {}", e))?;

    socket
        .set_broadcast(true)
        .map_err(|e| format!("Failed to enable socket broadcast: {}", e))?;

    socket
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| format!("Failed to set socket timeout: {}", e))?;

    let dest = format!("{}:{}", broadcast_ip.trim(), port);
    let mut sent_any = false;

    if socket.send_to(&packet, &dest).is_ok() {
        sent_any = true;
    }

    // Secondary fallback broadcast to global 255.255.255.255 if target isn't already global
    if broadcast_ip.trim() != "255.255.255.255" {
        let global_dest = format!("255.255.255.255:{}", port);
        let _ = socket.send_to(&packet, &global_dest);
    }

    if sent_any {
        Ok(())
    } else {
        Err(format!("Failed to send magic packet to {}", dest))
    }
}

/// Send WOL packet via Raspberry Pi SSH gateway if configured.
pub fn send_ssh_wol(
    pi_user: &str,
    pi_host: &str,
    pi_ssh_port: u16,
    broadcast_ip: &str,
    mac: &str,
) -> Result<String, String> {
    // `broadcast_ip` and `mac` are interpolated directly into a command
    // string that gets executed by the *remote* host's shell (`sh -c ...`)
    // once it reaches `ssh`, so they must be strictly validated here even
    // though callers (config::validate_settings) should already have done
    // so -- this function must never trust its inputs implicitly.
    validate_host_token(pi_user, "Pi gateway user")?;
    validate_host_token(pi_host, "Pi gateway host")?;
    validate_host_token(broadcast_ip, "Broadcast address")?;
    // Reuses create_magic_packet's strict MAC validation; the packet itself
    // is discarded here, only the validation result matters.
    create_magic_packet(mac)?;

    let wol_cmd = format!("wakeonlan -i {} {}", broadcast_ip, mac.trim());
    let port_str = pi_ssh_port.to_string();
    let output = Command::new("ssh")
        .args([
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "ConnectTimeout=8",
            "-o",
            "BatchMode=yes",
            "-p",
            &port_str,
            &format!("{}@{}", pi_user, pi_host),
            &wol_cmd,
        ])
        .output()
        .map_err(|e| format!("Failed to invoke SSH for WOL: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(stdout.trim().to_string())
    } else {
        let err_msg = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else if !stdout.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            format!("SSH exited with status code {:?}", output.status.code())
        };
        Err(err_msg)
    }
}

/// Parse ICMP round-trip latency (RTT in ms) from ping stdout.
pub fn parse_ping_latency(output: &str) -> Option<f32> {
    if output.trim().is_empty() {
        return None;
    }

    // Windows / localized sub-millisecond format "time<1ms", "zeit<1ms", "temps<1ms", "время<1мс", "时间<1ms", etc.
    let re_sub_ms = Regex::new(r"(?i)<\s*1\s*(?:ms|мс)").ok()?;
    if re_sub_ms.is_match(output) {
        return Some(0.5);
    }

    // Standard localized reply line "time=12.4 ms", "zeit=12ms", "temps=12 ms", "tiempo=12ms", "tiempo < 1 ms", "czas=12ms", "tijd=12ms", "время=12мс", "时间=12ms", etc.
    let re_time = Regex::new(
        r"(?i)(?:time|zeit|temps|tiempo|tempo|czas|tijd|время|时间|時間)\s*[=<]\s*([0-9]+(?:\.[0-9]+)?)\s*(?:ms|мс)?",
    )
    .ok()?;
    if let Some(caps) = re_time.captures(output) {
        if let Some(m) = caps.get(1) {
            if let Ok(val) = m.as_str().parse::<f32>() {
                return Some(val);
            }
        }
    }

    // Linux / macOS / BSD / BusyBox summary stats:
    // "rtt min/avg/max/mdev = 12.3/14.5/..." or "round-trip min/avg/max/stddev = ..."
    // or BusyBox "round-trip min/avg/max = ..."
    let re_avg = Regex::new(
        r"(?i)(?:rtt|round-trip|[a-z0-9_-]+)\s+(?:min/avg/max(?:/[a-z0-9_-]+)?|min/avg/max/std-?dev|min/avg/max)\s*=\s*[0-9.]+/([0-9.]+)",
    )
    .ok()?;
    if let Some(caps) = re_avg.captures(output) {
        if let Some(m) = caps.get(1) {
            if let Ok(val) = m.as_str().parse::<f32>() {
                return Some(val);
            }
        }
    }

    // Windows summary average across locales:
    // English "Average = 12ms", German "Mittelwert = 12ms" / "Durchschnitt = 12ms", French "Moyenne = 12ms",
    // Spanish "Media = 12ms" / "Promedio = 12ms", Portuguese "Média = 12ms", Italian "Medio = 12ms",
    // Polish "Średni = 12ms", Dutch "Gemiddelde = 12ms", Russian "Среднее = 12мс", Chinese/Japanese "平均 = 12ms"
    let re_win_avg = Regex::new(
        r"(?i)(?:Average|Mittelwert|Durchschnitt|Moyenne|Media|Promedio|Média|Medio|Średni|Gemiddelde|Среднее|平均)\s*=\s*([0-9]+(?:\.[0-9]+)?)\s*(?:ms|мс)?",
    )
    .ok()?;
    if let Some(caps) = re_win_avg.captures(output) {
        if let Some(m) = caps.get(1) {
            if let Ok(val) = m.as_str().parse::<f32>() {
                return Some(val);
            }
        }
    }

    // Generic fallback: match any numeric value following `=` or `<` and followed by `ms` or `мс`
    let re_fallback = Regex::new(r"(?i)[=<]\s*([0-9]+(?:\.[0-9]+)?)\s*(?:ms|мс)").ok()?;
    if let Some(caps) = re_fallback.captures(output) {
        if let Some(m) = caps.get(1) {
            if let Ok(val) = m.as_str().parse::<f32>() {
                return Some(val);
            }
        }
    }

    None
}

/// Format latency float into a concise display string.
pub fn format_latency(latency_ms: Option<f32>) -> String {
    match latency_ms {
        None => "--ms".to_string(),
        Some(ms) if ms < 1.0 => "<1ms".to_string(),
        Some(ms) if ms < 10.0 => format!("{:.1}ms", ms),
        Some(ms) => format!("{}ms", ms.round() as u32),
    }
}

/// Cross-platform ICMP ping with RTT latency parsing.
pub fn check_ping(host: &str, timeout_secs: u64) -> (bool, Option<f32>) {
    let host = host.trim();
    if host.is_empty() {
        return (false, None);
    }

    let result = if cfg!(windows) {
        let timeout_ms = (timeout_secs * 1000).to_string();
        Command::new("ping")
            .args(["-n", "1", "-w", &timeout_ms, host])
            .output()
    } else {
        let timeout_s = timeout_secs.max(1).to_string();
        Command::new("ping")
            .args(["-c", "1", "-W", &timeout_s, host])
            .output()
    };

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = format!("{}\n{}", stdout, stderr);
            let latency = parse_ping_latency(&combined);
            let combined_lower = combined.to_lowercase();
            let is_failed = combined_lower.contains("100% packet loss")
                || combined_lower.contains("100% loss")
                || combined_lower.contains("100% verlust")
                || combined_lower.contains("100% de perte")
                || combined_lower.contains("100% de pérdidas")
                || combined_lower.contains("100% persi")
                || combined_lower.contains("100% de perda")
                || combined_lower.contains("100% strat")
                || combined_lower.contains("100% verlies")
                || combined_lower.contains("100% потерь")
                || combined_lower.contains("100% 丢失")
                || combined_lower.contains("100% 遺失")
                || combined_lower.contains("destination host unreachable")
                || combined_lower.contains("zielhost nicht erreichbar")
                || combined_lower.contains("impossible de joindre")
                || combined_lower.contains("host de destino inaccesible")
                || combined_lower.contains("host de destino inacessível")
                || combined_lower.contains("host docelowy jest nieosiągalny")
                || combined_lower.contains("doelhost is niet bereikbaar")
                || combined_lower.contains("узел недоступен")
                || combined_lower.contains("无法访问")
                || combined_lower.contains("無法到達")
                || combined_lower.contains("request timed out")
                || combined_lower.contains("zeitüberschreitung")
                || combined_lower.contains("délai d'attente")
                || combined_lower.contains("tiempo de espera agotado")
                || combined_lower.contains("richiesta scaduta")
                || combined_lower.contains("tempo limite")
                || combined_lower.contains("limit czasu")
                || combined_lower.contains("превышен интервал")
                || combined_lower.contains("超时")
                || combined_lower.contains("超時")
                || combined_lower.contains("0 received")
                || combined_lower.contains("0 empfangen")
                || combined_lower.contains("0 reçus")
                || combined_lower.contains("0 recibidos")
                || combined_lower.contains("0 ricevuti")
                || combined_lower.contains("0 odebranych")
                || combined_lower.contains("0 ontvangen")
                || combined_lower.contains("0 получено")
                || combined_lower.contains("0 已接收")
                || combined_lower.contains("unknown host")
                || combined_lower.contains("unbekannter host")
                || combined_lower.contains("hôte inconnu")
                || combined_lower.contains("host desconocido")
                || combined_lower.contains("nieznany host")
                || combined_lower.contains("неизвестный узел");

            let reply_found = combined_lower.contains("bytes from")
                || combined_lower.contains("reply from")
                || combined_lower.contains("antwort von")
                || combined_lower.contains("réponse de")
                || combined_lower.contains("respuesta desde")
                || combined_lower.contains("risposta da")
                || combined_lower.contains("resposta de")
                || combined_lower.contains("odpowiedź z")
                || combined_lower.contains("antwoord van")
                || combined_lower.contains("ответ от")
                || combined_lower.contains("来自");

            let ok = !is_failed
                && (latency.is_some() || output.status.success() || reply_found);
            (ok, latency)
        }
        Err(_) => (false, None),
    }
}

/// Pure Rust TCP socket connectivity check with timeout.
pub fn check_tcp_port(host: &str, port: u16, timeout_ms: u64) -> bool {
    let addr_str = format!("{}:{}", host.trim(), port);
    let addrs = match addr_str.to_socket_addrs() {
        Ok(addrs) => addrs,
        Err(_) => return false,
    };

    let timeout = Duration::from_millis(timeout_ms.max(50));
    for addr in addrs {
        if TcpStream::connect_timeout(&addr, timeout).is_ok() {
            return true;
        }
    }
    false
}

/// Remote SSH execution of power actions (suspend, reboot, shutdown).
pub fn execute_power_action(
    host: &str,
    user: &str,
    action: &str,
    port: u16,
) -> Result<String, String> {
    let cmd_str = match action.to_lowercase().as_str() {
        "suspend" => "sudo -n systemctl suspend || systemctl suspend",
        "reboot" => "sudo -n systemctl reboot || systemctl reboot || sudo -n reboot || reboot",
        "shutdown" | "poweroff" => {
            "sudo -n systemctl poweroff || systemctl poweroff || sudo -n poweroff || poweroff"
        }
        other => return Err(format!("Unknown power action '{}'", other)),
    };

    // Defense-in-depth: host/user ultimately come from config file or CLI
    // args and are folded into the `user@host` SSH target argument. `cmd_str`
    // above is always a fixed literal (never built from `host`/`user`), so
    // there is no remote shell-injection vector here, but we still reject
    // malformed values so they can't be misparsed as SSH options.
    validate_host_token(user, "SSH user")?;
    validate_host_token(host, "SSH host")?;

    let target = format!("{}@{}", user.trim(), host.trim());
    let port_str = port.to_string();

    let output = Command::new("ssh")
        .args([
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "ConnectTimeout=5",
            "-o",
            "BatchMode=yes",
            "-p",
            &port_str,
            &target,
            cmd_str,
        ])
        .output();

    match output {
        Ok(res) => {
            let stdout = String::from_utf8_lossy(&res.stdout).to_string();
            let stderr = String::from_utf8_lossy(&res.stderr).to_string();
            let combined = format!("{}\n{}", stdout, stderr);

            // A remote suspend/shutdown/reboot often closes the connection immediately
            if res.status.success()
                || combined.contains("Closed by remote host")
                || combined.contains("Connection to")
                || combined.contains("closed by remote")
            {
                Ok(format!(
                    "Remote {} command dispatched successfully to {}",
                    action, target
                ))
            } else {
                let err = stderr.trim();
                let msg = if !err.is_empty() {
                    err
                } else if !stdout.trim().is_empty() {
                    stdout.trim()
                } else {
                    "Non-zero exit status returned"
                };

                // OpenSSH's own client-side auth rejection always includes the
                // parenthesized method list ("...Permission denied (publickey,...)"),
                // which distinguishes "SSH login itself was rejected" from a
                // permission error printed by the remote command (e.g. `reboot`
                // failing without sudo) — the latter needs different advice, so
                // don't conflate the two.
                if msg.contains("Permission denied (") {
                    Err(format!(
                        "Remote {} failed: {} — power actions need passwordless SSH access to {} \
                         (an authorized key, or Tailscale SSH via `tailscale set --ssh` on the target); \
                         this is separate from the WOL/ping/port checks, which don't need it.",
                        action, msg, target
                    ))
                } else {
                    Err(format!("Remote {} failed: {}", action, msg))
                }
            }
        }
        Err(e) => Err(format!("Failed to execute SSH command: {}", e)),
    }
}

/// Check if a binary exists in system PATH.
fn binary_in_path(name: &str) -> bool {
    if let Ok(path_var) = std::env::var("PATH") {
        let separator = if cfg!(windows) { ';' } else { ':' };
        for dir in path_var.split(separator) {
            let candidate = std::path::Path::new(dir).join(name);
            if candidate.is_file() {
                return true;
            }
            if cfg!(windows) && !name.to_lowercase().ends_with(".exe") {
                let exe_candidate = std::path::Path::new(dir).join(format!("{}.exe", name));
                if exe_candidate.is_file() {
                    return true;
                }
            }
        }
    }
    false
}

/// Cross-platform Moonlight launcher.
pub fn launch_moonlight() -> bool {
    let candidates = ["moonlight", "Moonlight", "moonlight.exe", "Moonlight.exe"];

    for cand in candidates {
        if binary_in_path(cand) && Command::new(cand).spawn().is_ok() {
            return true;
        }
    }

    if cfg!(target_os = "linux") {
        // Flatpak
        if binary_in_path("flatpak") {
            let check = Command::new("flatpak")
                .args(["info", "com.moonlight_stream.Moonlight"])
                .output();
            if let Ok(res) = check {
                if res.status.success()
                    && Command::new("flatpak")
                        .args(["run", "com.moonlight_stream.Moonlight"])
                        .spawn()
                        .is_ok()
                {
                    return true;
                }
            }
        }

        // Snap
        let snap_path = std::path::Path::new("/snap/bin/moonlight");
        if snap_path.exists() && Command::new(snap_path).spawn().is_ok() {
            return true;
        }
    }

    if cfg!(windows) {
        let program_files = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".to_string());
        let program_files_x86 = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".to_string());
        let local_app_data = std::env::var("LOCALAPPDATA").unwrap_or_default();

        let win_candidates = [
            format!("{}\\Moonlight Game Streaming\\Moonlight.exe", program_files),
            format!("{}\\Moonlight Game Streaming\\Moonlight.exe", program_files_x86),
            format!("{}\\Programs\\Moonlight Game Streaming\\Moonlight.exe", local_app_data),
        ];

        for p in win_candidates {
            let path = std::path::Path::new(&p);
            if path.exists() && Command::new(path).spawn().is_ok() {
                return true;
            }
        }
    }

    false
}
