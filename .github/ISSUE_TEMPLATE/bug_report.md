---
name: Bug Report
about: Create a report to help us improve Wake Workstation
title: "[BUG] <concise description of issue>"
labels: ["bug"]
assignees: ""
---

## Description
<!-- A clear and concise description of what the bug is. -->

## Steps to Reproduce
1. Launch Wake Workstation with `...` (or via desktop GUI)
2. Select profile / execute action `...`
3. Observe behavior `...`

## Expected Behavior
<!-- A clear and concise description of what you expected to happen. -->

## Actual Behavior & Logs
<!-- What actually happened? Please paste terminal/console logs or error messages below. -->
```text
<!-- Paste logs, console output, or stack traces here -->
```

## Environment Information
- **OS & Version**: [e.g. Windows 11 23H2, Ubuntu 24.04 LTS, macOS Sequoia 15.0]
- **Architecture**: [e.g. x86_64, aarch64 / ARM64]
- **Wake Workstation Version**: [e.g. `0.1.0` or git commit SHA]
- **Execution Mode**: [e.g. Desktop GUI (`eframe`), Terminal CLI (`--cli`), Remote Power (`--power`)]
- **WOL Strategy**: [e.g. Direct Local UDP Broadcast (`direct_wol: true`), Raspberry Pi SSH Gateway Relay]
- **Target Host OS & Specs**: [e.g. Windows 11 Pro / Arch Linux / Ubuntu Server, Sunshine v0.23.1]
- **Client Moonlight Version**: [e.g. Moonlight v6.1.0 (WinGet / Flatpak / Scoop / AppImage)]

## Configuration (Sanitized)
<!-- If relevant, paste your sanitized config.json (remove any private IPs, MAC addresses, or credentials). -->
```json
{
  "active_profile": "workstation",
  "profiles": {
    "workstation": {
      "direct_wol": false,
      "ssh_port": 22,
      "sunshine_port": 47989,
      "max_wait_seconds": 300,
      "wol_resend_interval": 60
    }
  }
}
```

## Screenshots / Screen Recordings
<!-- If applicable, add screenshots or videos to help explain the problem. -->

## Additional Context
<!-- Add any other context about the problem here (e.g. network topology, VPN/Tailscale configuration, firewall rules, router isolation). -->
