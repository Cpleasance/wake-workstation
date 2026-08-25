# Security Policy

## Overview

We take the security of **Wake Workstation** seriously. This document outlines our security policies, supported versions, and the process for responsibly reporting vulnerabilities.

---

## Supported Versions

Security updates and patches are actively maintained for the following versions:

| Version | Supported          | Notes                              |
| ------- | ------------------ | ---------------------------------- |
| 0.1.x   | :white_check_mark: | Current active release branch      |
| < 0.1.0 | :x:                | Pre-release / development versions |

---

## Reporting a Vulnerability

If you discover a security vulnerability or potential security weakness in Wake Workstation, please report it responsibly. **Do not create public GitHub issues or discussions for security vulnerabilities.**

### Reporting Methods

1. **GitHub Private Vulnerability Reporting (Preferred)**:
   - Navigate to the **Security** tab of this repository on GitHub.
   - Click **Report a vulnerability** to open an advisory draft.
   - This keeps all communication encrypted and confidential until a patch is prepared and published.

2. **Email**:
   - Alternatively, report vulnerabilities directly to the maintainer via email.
   - Include `[SECURITY] Wake Workstation Vulnerability Report` in the subject line.

### What to Include in Your Report

To help us investigate and triage the issue quickly, please provide:
- A clear description of the vulnerability and its potential security impact.
- Step-by-step reproduction instructions or a minimal Proof of Concept (PoC).
- The affected version(s), operating system, and architecture.
- Any relevant logs, stack traces, or configuration files (with sensitive hostnames/tokens redacted).
- Any proposed remediations or patches (if available).

### Response SLA & Coordination

- **Acknowledgment**: Within **48 hours** of receiving the report.
- **Triage & Assessment**: Within **5 business days**, confirming severity and scope.
- **Remediation & Patch**: A fix will be developed in a private security fork.
- **Coordinated Disclosure**: Once a patch is ready and tested, a new release and accompanying security advisory (with CVE if applicable) will be published. We will credit you in the release notes unless you prefer to remain anonymous.

---

## Security Architecture & Design Principles

Wake Workstation adheres to strict security hygiene and defensive programming practices:

1. **Strict Input Allowlisting & Shell Injection Prevention**:
   - All network tokens (IP addresses, hostnames, usernames, ports) are validated against strict character allowlists prior to interpolation into SSH commands or network sockets via `validate_host_token()`.
   - Arguments beginning with `-` are strictly rejected to prevent option injection attacks against underlying CLI tools (`ssh`, `wakeonlan`, `ping`, `arp`).

2. **Zero Hardcoded Secrets & Credentials**:
   - The application does not store, hardcode, or log private keys, passwords, or authentication tokens.
   - SSH authentication relies exclusively on the system OpenSSH agent, authorized keys (`~/.ssh/authorized_keys`), or Tailscale SSH in `BatchMode=yes`.

3. **Atomic File I/O & File Permission Hardening**:
   - Configuration files are written to temporary staging files and atomically renamed to prevent file corruption during system power events.
   - On Unix systems, sensitive configuration files are created with restricted file permissions (`0600`), readable only by the owning user.

4. **Safe Concurrency & Memory Safety**:
   - Developed in pure, memory-safe Rust with safe concurrency primitives (`mpsc` channels, `OnceLock`, scoped background worker threads).
   - Rate-limiting gates prevent network flooding or Denial of Service conditions against LAN gateways.
