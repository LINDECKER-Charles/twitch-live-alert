use anyhow::Result;
#[cfg(windows)]
use std::{path::PathBuf, time::Duration};

const STARTUP_ERROR: &str = "Impossible de modifier le démarrage automatique de Windows.";

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

#[cfg(windows)]
fn desktop_command() -> Result<Option<String>> {
    let Some(value) = std::env::var_os("TWITCH_LIVE_ALERT_DESKTOP_EXE") else {
        return Ok(None);
    };
    let path = PathBuf::from(value);
    anyhow::ensure!(path.is_absolute() && path.is_file(), STARTUP_ERROR);
    Ok(Some(format!(
        "\"{}\" --background",
        path.to_str()
            .ok_or_else(|| anyhow::anyhow!(STARTUP_ERROR))?
    )))
}
#[cfg(windows)]
pub(crate) fn startup_enabled() -> Result<bool> {
    if let Some(command) = desktop_command()? {
        let key = windows_registry::CURRENT_USER.open(RUN_KEY);
        return Ok(key
            .and_then(|k| k.get_string("TwitchLiveAlert"))
            .is_ok_and(|value| value == command));
    }
    use windows::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_Startup, KF_FLAG_DONT_VERIFY, SHGetKnownFolderPath},
    };
    let raw = unsafe { SHGetKnownFolderPath(&FOLDERID_Startup, KF_FLAG_DONT_VERIFY, None)? };
    let path = unsafe { raw.to_string() };
    unsafe { CoTaskMemFree(Some(raw.0.cast())) };
    Ok(PathBuf::from(path?).join("Twitch Live Alert.lnk").is_file())
}

#[cfg(not(windows))]
pub(crate) fn startup_enabled() -> Result<bool> {
    Ok(false)
}

#[cfg(windows)]
pub(crate) async fn set_startup(enabled: bool) -> Result<(), &'static str> {
    if let Some(command) = desktop_command().map_err(|_| STARTUP_ERROR)? {
        let key = windows_registry::CURRENT_USER
            .create(RUN_KEY)
            .map_err(|_| STARTUP_ERROR)?;
        if enabled {
            key.set_string("TwitchLiveAlert", command)
                .map_err(|_| STARTUP_ERROR)?;
        } else if key.get_string("TwitchLiveAlert").is_ok() {
            key.remove_value("TwitchLiveAlert")
                .map_err(|_| STARTUP_ERROR)?;
        }
        return Ok(());
    }
    let windows_root = std::env::var_os("SystemRoot").ok_or(STARTUP_ERROR)?;
    let powershell =
        PathBuf::from(windows_root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
    let script = if enabled {
        "install-startup.ps1"
    } else {
        "uninstall-startup.ps1"
    };
    let path = std::env::current_dir()
        .map_err(|_| STARTUP_ERROR)?
        .join("scripts")
        .join(script);
    let mut command = tokio::process::Command::new(powershell);
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "RemoteSigned",
            "-File",
        ])
        .arg(path)
        .env_remove("TWITCH_CLIENT_ID")
        .env_remove("TWITCH_CLIENT_SECRET")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x08000000)
        .kill_on_drop(true);
    let status = tokio::time::timeout(Duration::from_secs(15), command.status())
        .await
        .map_err(|_| STARTUP_ERROR)?
        .map_err(|_| STARTUP_ERROR)?;
    if !status.success() || startup_enabled().map_err(|_| STARTUP_ERROR)? != enabled {
        return Err(STARTUP_ERROR);
    }
    Ok(())
}

#[cfg(not(windows))]
pub(crate) async fn set_startup(_: bool) -> Result<(), &'static str> {
    Err(STARTUP_ERROR)
}
