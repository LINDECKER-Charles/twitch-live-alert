# Windows distribution

[Français](../DISTRIBUTION.md) · [User guide](../../README.en.md)

## Install

[Download Twitch Live Alert 0.3.0 for Windows x64](https://github.com/LINDECKER-Charles/twitch-live-alert/releases/download/v0.3.0/Twitch-Live-Alert_0.3.0_x64-setup.exe). This installer is unsigned. Compare the SHA-256 shown on the release page with the result of this command from the download folder. [Release notes and checksum](https://github.com/LINDECKER-Charles/twitch-live-alert/releases/tag/v0.3.0)

```powershell
Get-FileHash -Algorithm SHA256 .\Twitch-Live-Alert_0.3.0_x64-setup.exe
```

Run the installer under the Windows account that will monitor Twitch. By default the desktop program and service are installed together under `%LOCALAPPDATA%\Twitch Live Alert\`. The installer downloads Microsoft Edge WebView2 if Windows needs it. Choose **Twitch Live Alert** in the Start menu. For an existing installation, see the [migration guide](MIGRATION.md).

Closing the window leaves monitoring active. The tray menu can open the local dashboard in your browser. **Quitter** stops the desktop and the service it launched. Enabling startup creates the `TwitchLiveAlert` value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, launching the desktop with `--background`.

## Build an installer

Use Windows x64 with the Rust MSVC toolchain, Visual Studio C++ Build Tools and the Windows SDK. The project CI uses Rust 1.99.1. Install the Tauri CLI with `cargo install tauri-cli --version 2.12.0 --locked`, or let the build script use Node and `npx` to obtain that version.

```powershell
.\scripts\build-desktop.ps1 -PublicRelease
```

The script builds the Rust service, stages it alongside the Tauri desktop binary and packages both with NSIS. The public-release option remaps Rust source paths and drops the host directory from the linker’s PDB path. Under the default Cargo configuration the resulting installer is in `src-tauri/target/release/bundle/nsis/`. Public GitHub Actions jobs run on standard runners and test the installer in runner-local storage. Published downloads are attached to releases. [Tauri Windows packaging](https://v2.tauri.app/distribute/windows-installer/)

## Verify a candidate

Build the installer once so the service binary is available to the desktop checks.

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
$installer = Get-ChildItem 'src-tauri/target/release/bundle/nsis/*-setup.exe' | Select-Object -First 1
.\scripts\smoke-install.ps1 -Installer $installer.FullName -PortNumber 47219
```

Run the installation smoke test in a dedicated test account. It installs the package, checks how the launcher handles a busy port, starts an unconfigured service, verifies the dashboard over HTTP, opens and reopens the desktop window, then uninstalls it. For an isolated desktop profile set `TWITCH_LIVE_ALERT_DATA_DIR` to an absolute path and choose an unused port with `TWITCH_LIVE_ALERT_PORT`. The default is `127.0.0.1:37219`.

Uninstallation removes the executables and their login entry. The per-user profile, including its journal, stays in `%LOCALAPPDATA%\TwitchLiveAlert\`. Exit the application before copying it. Windows DPAPI credentials belong to the Windows account and machine that saved them. Re-enter credentials when moving to another PC.
