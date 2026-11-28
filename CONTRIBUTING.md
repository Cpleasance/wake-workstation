# Contributing to Wake Workstation

Thank you for your interest in contributing to **Wake Workstation**! We welcome contributions of all kinds: bug fixes, new features, documentation improvements, UI enhancements, and test coverage expansion.

Please take a moment to review this guide before opening a pull request or submitting an issue.

---

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Prerequisites & Development Environment](#prerequisites--development-environment)
- [Project Architecture](#project-architecture)
- [Local Development Workflow](#local-development-workflow)
  - [Building from Source](#building-from-source)
  - [Running the Application](#running-the-application)
  - [Running Tests](#running-tests)
  - [Code Formatting & Linting](#code-formatting--linting)
- [Design & Coding Guidelines](#design--coding-guidelines)
  - [Cross-Platform Compatibility](#cross-platform-compatibility)
  - [Security & Shell Safety](#security--shell-safety)
  - [UI Performance & Non-Blocking Architecture](#ui-performance--non-blocking-architecture)
  - [Error Handling](#error-handling)
- [Submitting a Pull Request](#submitting-a-pull-request)
- [Reporting Issues](#reporting-issues)

---

## Code of Conduct

We are committed to providing a welcoming, inclusive, and harassment-free environment for everyone. Please be respectful, constructive, and kind in all discussions, issues, and pull requests.

---

## Prerequisites & Development Environment

- **Rust Toolchain**: Stable Rust (edition 2021, MSRV 1.75+ recommended). Install via [rustup.rs](https://rustup.rs/):
  ```sh
  rustup update stable
  rustup component add rustfmt clippy
  ```

### Platform-Specific Dependencies

- **Windows**:
  - MSVC (`x86_64-pc-windows-msvc`) or MinGW (`x86_64-pc-windows-gnu`) toolchains.
  - Standard Windows system libraries (`user32`, `shell32`, `opengl32`).
- **Linux**:
  - Standard X11 and/or Wayland development headers for compiling `eframe`/`egui`/`winit`:
    - **Ubuntu / Debian**: `sudo apt install build-essential libx11-dev libxcursor-dev libxrandr-dev libxi-dev libxinerama-dev libxkbcommon-dev libgl1-mesa-dev pkg-config libgtk-3-dev`
    - **Arch Linux**: `sudo pacman -S base-devel libx11 libxcursor libxrandr libxi libxinerama libxkbcommon mesa gtk3`
    - **Fedora**: `sudo dnf install @development-tools libX11-devel libXcursor-devel libXrandr-devel libXi-devel libXinerama-devel libxkbcommon-devel mesa-libGL-devel gtk3-devel`
- **macOS**:
  - Xcode Command Line Tools (`xcode-select --install`).

---

## Project Architecture

The codebase is organized into modular components with clean separation of concerns:

```
wake-workstation/
├── src/
│   ├── main.rs              # CLI parser (clap), entry point, GUI launcher, GPU exports
│   ├── config.rs            # AppConfig, HostProfile, JSON serialization, validation, atomic saves
│   ├── net.rs               # WOL UDP packets, SSH relay, ping latency regex, TCP checks, Moonlight launch
│   ├── pipeline.rs          # Stage machine (Standby -> Wol -> Ping -> Ssh -> Sunshine -> Ready), workers
│   ├── tests.rs             # Exhaustive test suite (60+ unit, integration, stress, and benchmark tests)
│   └── ui/
│       ├── mod.rs           # WakeApp state container, event loop, modal router, toast manager
│       ├── header.rs        # Profile selector, config action buttons, reachability badge
│       ├── hero.rs          # Main power button, stage status, latency sparklines, progress bars
│       ├── stepper.rs       # 4-stage visual stepper (WOL, Ping, SSH, Sunshine)
│       ├── console.rs       # Real-time event logger, log level filters, auto-scroll
│       ├── settings_modal.rs# Profile configuration modal, tabbed editor, validation
│       ├── power_modal.rs   # Remote power actions (Suspend, Reboot, Shutdown) confirmation
│       ├── new_profile_modal.rs # New profile creation dialog
│       ├── json_config_modal.rs # Direct JSON config import/export viewer
│       ├── theme.rs         # Ergonomic color palette, typography metrics, custom egui styles
│       └── toast.rs         # Non-intrusive toast notifications
├── Cargo.toml               # Package manifest and dependencies
└── build.rs                 # Windows resource compilation (icon and version metadata)
```

---

## Local Development Workflow

### Building from Source

Debug build:
```sh
cargo build
```

Optimized release build:
```sh
cargo build --release
```

### Running the Application

- **Default Desktop GUI**:
  ```sh
  cargo run
  ```

- **Headless / Terminal CLI Mode**:
  ```sh
  cargo run -- --cli
  ```

- **Using a Specific Host Profile**:
  ```sh
  cargo run -- --profile thinkstation
  ```

- **Immediate Remote Power Action**:
  ```sh
  cargo run -- --power suspend
  # or:
  cargo run -- --power reboot
  cargo run -- --power shutdown
  ```

- **Overriding Settings on the Command Line**:
  ```sh
  cargo run -- --cli --direct-wol --target 192.168.1.50 --mac AA:BB:CC:DD:EE:FF
  ```

### Running Tests

Run the full test suite:
```sh
cargo test
```

Run a specific test by name:
```sh
cargo test test_magic_packet_structure
cargo test test_ping_latency_parsing
```

Run tests with verbose stdout output:
```sh
cargo test -- --nocapture
```

### Code Formatting & Linting

Before committing your changes, ensure your code matches the project formatting and clippy rules:

```sh
# 1. Format code
cargo fmt -- --check
# Automatically format:
cargo fmt

# 2. Run Clippy linter
cargo clippy --all-targets -- -D warnings
```

---

## Design & Coding Guidelines

### Cross-Platform Compatibility
- Always use `std::path::Path` and `PathBuf` for file operations; do not hardcode forward slashes or backslashes in paths.
- On Windows, child processes must be spawned using `hidden_command()` in `src/net.rs` to set `CREATE_NO_WINDOW (0x08000000)` and prevent unsightly flashing console windows.
- In CLI mode, support both Windows console terminals and Unix pseudo-terminals gracefully.

### Security & Shell Safety
- **Never concatenate unsanitized user strings into shell commands.**
- Use `validate_host_token()` in `src/net.rs` to strictly allow-list hostnames, IP addresses, MACs, and SSH usernames before passing them to `ssh` or remote commands.
- SSH connections for remote power actions use `BatchMode=yes` to guarantee they will never hang waiting for an interactive password prompt.

### UI Performance & Non-Blocking Architecture
- The egui GUI runs at a smooth 60 FPS. **Never perform blocking I/O (network pings, TCP socket handshakes, SSH executions) on the main UI thread.**
- Background polling tasks must be delegated via `std::thread::spawn` or worker channels (see `spawn_check()` in `src/pipeline.rs`).
- Telemetry buffers (e.g. latency history sparkline) are capped to fixed-size ring buffers to prevent memory leaks during long-running sessions.

### Error Handling
- Use structured error types (e.g. `ConfigError`) in core modules.
- Prefer returning descriptive `Result<T, E>` over `unwrap()` or `expect()` in library and runtime logic.
- Provide clear, actionable remediation messages in errors (e.g., advising how to configure passwordless sudo when a power command fails).

---

## Submitting a Pull Request

1. **Fork the Repository** and clone your fork locally.
2. **Create a Feature Branch**:
   ```sh
   git checkout -b feat/my-new-feature
   # or for bug fixes:
   git checkout -b fix/issue-description
   ```
3. **Write Clean, Documented Code**:
   - Follow existing architecture patterns.
   - Include doc comments for public functions and structs.
4. **Add Unit Tests**:
   - Add corresponding test cases in `src/tests.rs` verifying both happy paths and edge cases.
5. **Verify All Checks**:
   ```sh
   cargo fmt -- --check
   cargo clippy --all-targets -- -D warnings
   cargo test
   ```
6. **Commit with Clear Messages**:
   - Use imperative commit messages (e.g. `feat: add retry backoff for WOL broadcast` or `fix: handle empty ARP table gracefully`).
7. **Open a Pull Request**:
   - Fill out the PR description with context, motivation, and manual test evidence (screenshots or CLI output).

---

## Reporting Issues

If you encounter bugs, unexpected behavior, or have ideas for new features:

- Check existing [GitHub Issues](https://github.com/Cpleasance/wake-workstation/issues) to avoid duplicates.
- Submit a new issue using our structured [Bug Report](.github/ISSUE_TEMPLATE/bug_report.md) or [Feature Request](.github/ISSUE_TEMPLATE/feature_request.md) templates.
- When filing bugs, include sanitized logs, OS details, and network topology context to help us diagnose the issue quickly.
