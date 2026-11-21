use crate::win::{self, Process};
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use std::{
    env, fs,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

const DEFAULT_PORT: u16 = 37219;

#[derive(Deserialize)]
struct Identity {
    app_id: String,
    process_id: u32,
}

pub fn data_dir() -> Result<PathBuf> {
    let path = env::var_os("TWITCH_LIVE_ALERT_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("TwitchLiveAlert")))
        .context("Dossier de données Windows introuvable.")?;
    ensure!(
        path.is_absolute(),
        "Le dossier de données doit avoir un chemin absolu."
    );
    fs::create_dir_all(&path).context("Création du dossier de données Windows")?;
    Ok(path)
}

pub struct Service {
    pub process: Process,
    pub origin: String,
    port: u16,
    http: ureq::Agent,
    // Windows drops this handle on process termination, including forced installer upgrades.
    job: Option<std::os::windows::io::OwnedHandle>,
}

impl Service {
    pub fn start(data: &Path) -> Result<Self> {
        let desktop = env::current_exe()?;
        let binary = desktop.with_file_name("twitch-live-alert.exe").canonicalize()
            .context("Service Twitch Live Alert introuvable à côté de l’application. Réinstallez Twitch Live Alert.")?;
        let port = match env::var_os("TWITCH_LIVE_ALERT_PORT") {
            Some(value) => value
                .to_str()
                .and_then(|value| value.parse::<std::num::NonZeroU16>().ok())
                .context("TWITCH_LIVE_ALERT_PORT doit être un entier entre 1 et 65535.")?
                .get(),
            None => DEFAULT_PORT,
        };
        let origin = format!("http://127.0.0.1:{port}");
        let existing = win::listener(port)?;
        let mut child = if existing.is_none() {
            Some(
                Command::new(&binary)
                    .args(["--background", "--port", &port.to_string()])
                    .current_dir(data)
                    .env("TWITCH_LIVE_ALERT_DESKTOP_EXE", &desktop)
                    .creation_flags(CREATE_NO_WINDOW)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .context("Démarrage du service Twitch Live Alert")?,
            )
        } else {
            None
        };
        let result = child
            .as_ref()
            .map(win::child_job)
            .transpose()
            .and_then(|job| {
                Self::identify(&binary, child.as_mut(), existing, port, &origin)
                    .map(|(process, http)| (process, http, job))
            });
        match result {
            Ok((process, http, job)) => Ok(Self {
                job,
                process,
                http,
                port,
                origin,
            }),
            Err(error) => {
                if let Some(mut child) = child {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                Err(error)
            }
        }
    }

    fn identify(
        binary: &Path,
        mut child: Option<&mut Child>,
        existing: Option<u32>,
        port: u16,
        origin: &str,
    ) -> Result<(Process, ureq::Agent)> {
        let pid = existing
            .or_else(|| child.as_ref().map(|child| child.id()))
            .context("PID introuvable.")?;
        let process = Process::verify(pid, binary, port)?;
        let http: ureq::Agent = ureq::Agent::config_builder()
            .proxy(None)
            .timeout_global(Some(Duration::from_secs(2)))
            .max_redirects(0)
            .build()
            .into();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(owner) = win::listener(port)? {
                ensure!(
                    owner == pid,
                    "Le port {port} est occupé par une autre application (PID {owner})."
                );
                let mut response = http
                    .get(format!("{origin}/api/status"))
                    .call()
                    .context("Lecture de l’identité du service local")?;
                ensure!(
                    response.status() == 200,
                    "Le service local a refusé de s’identifier."
                );
                let body = response
                    .body_mut()
                    .with_config()
                    .limit(16 * 1024 * 1024)
                    .read_to_string()?;
                let identity: Identity =
                    serde_json::from_str(&body).context("Identité du service local invalide")?;
                ensure!(
                    identity.app_id == "twitch-live-alert" && identity.process_id == pid,
                    "L’identité du service sur le port {port} ne correspond pas à cette installation."
                );
                ensure!(
                    !process.exited(0)? && win::listener(port)? == Some(pid),
                    "Le service local s’est arrêté."
                );
                return Ok((process, http));
            }
            if let Some(child) = child.as_deref_mut()
                && let Some(status) = child.try_wait()?
            {
                bail!(
                    "Le service n’a pas démarré (code {status}). Vérifiez le dossier de données et la disponibilité du port {port}."
                );
            }
            ensure!(
                Instant::now() < deadline,
                "Le service local ne répond pas sur le port {port}."
            );
            thread::sleep(Duration::from_millis(100));
        }
    }

    pub fn available(&self) -> bool {
        matches!(self.process.exited(0), Ok(false))
            && matches!(win::listener(self.port), Ok(Some(pid)) if pid == self.process.pid)
    }

    pub fn shutdown(&self) {
        if self.job.is_some() && self.available() {
            let _ = self
                .http
                .post(format!("{}/api/shutdown", self.origin))
                .header("Origin", self.origin.as_str())
                .header("X-Twitch-Live-Alert", "local")
                .header("Content-Type", "application/json")
                .send("{}");
            let _ = self.process.exited(4_000);
            // An unresponsive child is stopped when Windows closes the job on desktop exit.
        }
    }
}
