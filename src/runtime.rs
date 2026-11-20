use crate::{
    config::{Config, Mode},
    http,
    notification::{DesktopNotifier, NotificationError},
    store::{Delivery, Store},
    twitch::{Stream, Twitch},
    watcher::Watcher,
    web,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    future::Future,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    net::TcpListener,
    sync::{mpsc, watch},
    time::{Instant, sleep_until},
};
use tracing::{error, info, warn};

pub(crate) const ADDRESS: &str = "127.0.0.1:37219";
const SETTINGS_PATH: &str = "data/settings.json";
const RETRY_MESSAGE: &str = "Une vérification est en attente de réessai. La prochaine tentative se lancera automatiquement.";
const SETTINGS_ERROR: &str = "Impossible d'enregistrer les réglages locaux.";
const STARTUP_ERROR: &str = "Impossible de modifier le démarrage automatique de Windows.";
const STORAGE_ERROR: &str = "La surveillance est arrêtée car la base locale est inaccessible.";
const NOTIFICATION_ERROR: &str =
    "Windows n'a pas accepté une notification. Vérifiez les autorisations de notification.";

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Preferences {
    pub poll_interval_seconds: u64,
    pub notify_existing_on_first_run: bool,
    pub start_with_windows: bool,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    preferences: Preferences,
    enabled: bool,
}

impl Settings {
    fn defaults(config: Option<&Config>) -> Self {
        Self {
            preferences: Preferences {
                poll_interval_seconds: config.map_or(60, |value| value.poll_interval.as_secs()),
                notify_existing_on_first_run: config.is_none_or(|value| value.notify_existing),
                start_with_windows: false,
            },
            enabled: true,
        }
    }

    fn load(path: &Path, defaults: Self) -> Self {
        match fs::read(path) {
            Ok(bytes) => match serde_json::from_slice::<Self>(&bytes) {
                Ok(settings) if settings.valid() => settings,
                _ => {
                    warn!("Réglages locaux invalides, valeurs par défaut utilisées");
                    defaults
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => defaults,
            Err(_) => {
                warn!("Réglages locaux illisibles, valeurs par défaut utilisées");
                defaults
            }
        }
    }

    fn valid(self) -> bool {
        (15..=3600).contains(&self.preferences.poll_interval_seconds)
    }

    fn save(self, path: &Path) -> Result<()> {
        let directory = path.parent().context("Chemin de réglages invalide")?;
        fs::create_dir_all(directory)?;
        let mut file = tempfile::NamedTempFile::new_in(directory)?;
        serde_json::to_writer_pretty(&mut file, &self)?;
        file.write_all(b"\n")?;
        file.as_file().sync_all()?;
        file.persist(path)?;
        Ok(())
    }

    fn interval(self) -> Duration {
        Duration::from_secs(self.preferences.poll_interval_seconds)
    }
}

#[derive(Clone, Serialize)]
pub(crate) struct Status {
    app_id: &'static str,
    process_id: u32,
    app_version: &'static str,
    pub running: bool,
    pub configured: bool,
    #[serde(flatten)]
    preferences: Preferences,
    last_checked_at: Option<u64>,
    next_check_at: Option<u64>,
    last_error: Option<String>,
    notifications_available: bool,
    live_streams: Vec<LiveStream>,
    notifications: Vec<Delivery>,
    #[serde(skip)]
    worker_available: bool,
    #[serde(skip)]
    retry_not_before: Option<Instant>,
}

#[derive(Clone, Serialize)]
struct LiveStream {
    id: String,
    user_name: String,
    title: String,
    url: String,
    viewer_count: u64,
    started_at: String,
}

impl From<&Stream> for LiveStream {
    fn from(stream: &Stream) -> Self {
        Self {
            id: stream.id.clone(),
            user_name: stream.user_name.clone(),
            title: stream.title.clone(),
            url: stream.url(),
            viewer_count: stream.viewer_count,
            started_at: stream.started_at.clone(),
        }
    }
}

pub(crate) enum ControlError {
    Invalid(&'static str),
    Unavailable(&'static str),
    Operation(&'static str),
}

pub(crate) struct Control {
    status: Mutex<Status>,
    settings: watch::Sender<Settings>,
    settings_path: PathBuf,
    changes: tokio::sync::Mutex<()>,
    checks: mpsc::Sender<()>,
    shutdown: watch::Sender<bool>,
}

impl Control {
    fn new(
        settings_path: PathBuf,
        settings: Settings,
        configured: bool,
        worker_available: bool,
    ) -> (Arc<Self>, mpsc::Receiver<()>) {
        let (settings_sender, _) = watch::channel(settings);
        let (shutdown, _) = watch::channel(false);
        let (checks, check_receiver) = mpsc::channel(1);
        let control = Self {
            status: Mutex::new(Status {
                app_id: "bloodborne-fr-live",
                process_id: std::process::id(),
                app_version: env!("CARGO_PKG_VERSION"),
                running: worker_available && settings.enabled,
                configured,
                preferences: settings.preferences,
                last_checked_at: None,
                next_check_at: None,
                last_error: None,
                notifications_available: false,
                live_streams: Vec::new(),
                notifications: Vec::new(),
                worker_available,
                retry_not_before: None,
            }),
            settings: settings_sender,
            settings_path,
            changes: tokio::sync::Mutex::new(()),
            checks,
            shutdown,
        };
        (Arc::new(control), check_receiver)
    }

    fn lock_status(&self) -> std::sync::MutexGuard<'_, Status> {
        self.status.lock().expect("status lock")
    }

    pub fn status(&self) -> Status {
        let mut status = self.lock_status().clone();
        status.notifications_available = DesktopNotifier::available();
        status.preferences.start_with_windows = startup_enabled().unwrap_or(false);
        status
    }

    pub async fn change_preferences(&self, preferences: Preferences) -> Result<(), ControlError> {
        let _guard = self.changes.lock().await;
        let settings = Settings {
            preferences,
            ..*self.settings.borrow()
        };
        if !settings.valid() {
            return Err(ControlError::Invalid(
                "L'intervalle doit être compris entre 15 et 3600 secondes.",
            ));
        }
        let existing_startup = startup_enabled().unwrap_or(false);
        if existing_startup != preferences.start_with_windows {
            set_startup(preferences.start_with_windows)
                .await
                .map_err(ControlError::Operation)?;
        }
        if settings.save(&self.settings_path).is_err() {
            if existing_startup != preferences.start_with_windows
                && set_startup(existing_startup).await.is_err()
            {
                warn!("Impossible de rétablir le raccourci de démarrage");
            }
            return Err(ControlError::Operation(SETTINGS_ERROR));
        }
        self.apply(settings);
        Ok(())
    }

    pub async fn change_monitoring(&self, enabled: bool) -> Result<(), ControlError> {
        let _guard = self.changes.lock().await;
        if enabled && !self.lock_status().worker_available {
            return Err(ControlError::Unavailable(
                "La surveillance est indisponible. Vérifiez le message d'état.",
            ));
        }
        let settings = Settings {
            enabled,
            ..*self.settings.borrow()
        };
        if settings.save(&self.settings_path).is_err() {
            return Err(ControlError::Operation(SETTINGS_ERROR));
        }
        self.apply(settings);
        Ok(())
    }

    fn apply(&self, settings: Settings) {
        let mut status = self.lock_status();
        status.preferences = settings.preferences;
        status.running = status.worker_available && settings.enabled && !*self.shutdown.borrow();
        if !status.running {
            status.next_check_at = None;
        }
        self.settings.send_replace(settings);
    }

    pub fn check_now(&self) -> Result<(), ControlError> {
        let status = self.lock_status();
        if !status.running {
            return Err(ControlError::Unavailable(
                "La surveillance doit être active pour vérifier les lives.",
            ));
        }
        if status
            .retry_not_before
            .is_some_and(|deadline| Instant::now() < deadline)
        {
            return Err(ControlError::Unavailable(RETRY_MESSAGE));
        }
        match self.checks.try_send(()) {
            Ok(()) | Err(mpsc::error::TrySendError::Full(())) => Ok(()),
            Err(mpsc::error::TrySendError::Closed(())) => Err(ControlError::Unavailable(
                "La surveillance est indisponible. Vérifiez le message d'état.",
            )),
        }
    }

    pub fn stop(&self) {
        let mut status = self.lock_status();
        status.running = false;
        status.next_check_at = None;
        self.shutdown.send_replace(true);
    }

    fn fail_worker(&self, message: impl Into<String>) {
        let mut status = self.lock_status();
        status.worker_available = false;
        status.running = false;
        status.next_check_at = None;
        status.last_error = Some(message.into());
    }
}

pub async fn serve(
    config: Result<Config>,
    shutdown_signal: impl Future<Output = Result<()>>,
) -> Result<()> {
    let listener = TcpListener::bind(ADDRESS)
        .await
        .context("Impossible d'ouvrir l'interface locale sur le port 37219. Vérifiez si elle est déjà lancée.")?;
    let fallback = config
        .is_err()
        .then(|| Config::from_env(Mode::TestNotification).ok())
        .flatten();
    let path = PathBuf::from(SETTINGS_PATH);
    let settings = Settings::load(
        &path,
        Settings::defaults(config.as_ref().ok().or(fallback.as_ref())),
    );
    let database_path = config.as_ref().ok().or(fallback.as_ref()).map_or_else(
        || PathBuf::from("data/alerts.sqlite3"),
        |config| config.database_path.clone(),
    );
    let store = Store::open(&database_path).and_then(|store| Ok((store.recent_history()?, store)));
    let (control, receiver) = Control::new(
        path,
        settings,
        config.is_ok(),
        config.is_ok() && store.is_ok(),
    );
    let watcher = match (config, store) {
        (Ok(config), Ok((history, store))) => {
            control.lock_status().notifications = history;
            let notifier = DesktopNotifier::new().unwrap_or(DesktopNotifier);
            Some(watcher(config, store, notifier)?)
        }
        (Err(error), Ok((history, _))) => {
            control.lock_status().notifications = history;
            control.fail_worker(format!(
                "Twitch n'est pas configuré. {error} Relancez l'application après configuration."
            ));
            None
        }
        (_, Err(error)) => {
            error!("{error:#}");
            control.fail_worker(STORAGE_ERROR);
            None
        }
    };
    info!("Interface locale ouverte sur http://{ADDRESS}");
    let worker = async {
        if let Some(watcher) = watcher
            && let Err(error) = run_worker(watcher, &control, receiver).await
        {
            error!("{error:#}");
            control.fail_worker(STORAGE_ERROR);
        }
    };
    let mut shutdown = control.shutdown.subscribe();
    let server = async {
        axum::serve(listener, web::router(control.clone()))
            .with_graceful_shutdown(async move {
                let _ = shutdown.wait_for(|stop| *stop).await;
            })
            .await
            .context("L'interface locale s'est arrêtée")
    };
    tokio::pin!(server, worker, shutdown_signal);
    let result = tokio::select! {
        result = &mut server => {
            control.stop();
            worker.await;
            result
        }
        result = &mut shutdown_signal => {
            control.stop();
            worker.await;
            server.await?;
            result
        }
        () = &mut worker => {
            tokio::select! {
                result = &mut server => result,
                result = &mut shutdown_signal => {
                    control.stop();
                    server.await?;
                    result
                }
            }
        }
    };
    info!("Surveillance arrêtée");
    result
}

pub async fn watch_in_terminal(
    config: Config,
    shutdown_signal: impl Future<Output = Result<()>>,
) -> Result<()> {
    let settings = Settings::defaults(Some(&config));
    let store = Store::open(&config.database_path)?;
    let watcher = watcher(config, store, DesktopNotifier::new()?)?;
    let (control, receiver) = Control::new(PathBuf::new(), settings, true, true);
    let worker = run_worker(watcher, &control, receiver);
    tokio::pin!(worker, shutdown_signal);
    tokio::select! {
        result = &mut worker => result,
        result = &mut shutdown_signal => {
            control.stop();
            worker.await?;
            result
        }
    }
}

fn watcher(
    config: Config,
    store: Store,
    notifier: DesktopNotifier,
) -> Result<Watcher<DesktopNotifier>> {
    Ok(Watcher {
        twitch: Twitch::new(http::client()?, config.client_id, config.client_secret),
        notifier,
        store,
        notify_existing: config.notify_existing,
        last_streams: Vec::new(),
    })
}

async fn run_worker(
    mut watcher: Watcher<DesktopNotifier>,
    control: &Control,
    mut checks: mpsc::Receiver<()>,
) -> Result<()> {
    let mut settings = control.settings.subscribe();
    let mut shutdown = control.shutdown.subscribe();
    let mut last_finished = None;
    let mut retry_until: Option<Instant> = None;
    let mut failures = 0u32;
    while !*shutdown.borrow() {
        let current = *settings.borrow_and_update();
        if !current.enabled {
            last_finished = None;
            while checks.try_recv().is_ok() {}
            tokio::select! {
                _ = shutdown.changed() => break,
                _ = settings.changed() => continue,
            }
        }
        let now = Instant::now();
        let due = last_finished
            .map_or(now, |last: Instant| last + current.interval())
            .max(retry_until.unwrap_or(now));
        {
            let mut status = control.lock_status();
            if status.running {
                status.next_check_at = Some(
                    http::unix_seconds()
                        .saturating_add(due.saturating_duration_since(now).as_secs()),
                );
            }
        }
        tokio::select! {
            biased;
            _ = shutdown.changed() => break,
            _ = settings.changed() => continue,
            _ = sleep_until(due) => { let _ = checks.try_recv(); },
            _ = async {
                if let Some(deadline) = retry_until {
                    sleep_until(deadline).await;
                }
                checks.recv().await
            } => {}
        }
        control.lock_status().next_check_at = None;
        watcher.notify_existing = current.preferences.notify_existing_on_first_run;
        let (cancel, mut cancel_receiver) = watch::channel(false);
        let (result, cancelled) = {
            let cycle = watcher.cycle_with_shutdown(&mut cancel_receiver);
            tokio::pin!(cycle);
            tokio::select! {
                biased;
                _ = shutdown.wait_for(|stop| *stop) => {
                    cancel.send_replace(true);
                    (cycle.await, true)
                }
                _ = settings.wait_for(|value| !value.enabled) => {
                    cancel.send_replace(true);
                    (cycle.await, true)
                }
                result = &mut cycle => (result, false),
            }
        };
        let last_error = match result {
            Ok(_) => {
                if !cancelled {
                    failures = 0;
                    retry_until = None;
                }
                None
            }
            Err(error) => {
                // Stop if an alert could have been delivered without a durable record.
                let api_error = error.downcast_ref::<http::ApiError>();
                if api_error.is_none() && error.downcast_ref::<NotificationError>().is_none() {
                    return Err(error);
                }
                failures = failures.saturating_add(1);
                let delay = http::retry_delay(&error, failures, current.interval());
                retry_until = Instant::now()
                    .checked_add(delay)
                    .or_else(|| Some(Instant::now() + Duration::from_secs(3600)));
                error!(retry_seconds = delay.as_secs_f64(), "{error:#}");
                Some(api_error.map_or_else(|| NOTIFICATION_ERROR.to_owned(), ToString::to_string))
            }
        };
        let notifications = watcher.store.recent_history()?;
        let mut status = control.lock_status();
        status.notifications = notifications;
        status.retry_not_before = retry_until;
        status.live_streams = watcher.last_streams.iter().map(LiveStream::from).collect();
        if !cancelled || last_error.is_some() {
            status.last_checked_at = Some(http::unix_seconds());
            status.last_error = last_error;
            last_finished = Some(Instant::now());
        }
    }
    Ok(())
}

#[cfg(windows)]
fn startup_enabled() -> Result<bool> {
    use windows::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_Startup, KF_FLAG_DONT_VERIFY, SHGetKnownFolderPath},
    };
    let raw = unsafe { SHGetKnownFolderPath(&FOLDERID_Startup, KF_FLAG_DONT_VERIFY, None)? };
    let path = unsafe { raw.to_string() };
    unsafe { CoTaskMemFree(Some(raw.0.cast())) };
    Ok(PathBuf::from(path?)
        .join("Bloodborne FR Live.lnk")
        .is_file())
}

#[cfg(not(windows))]
fn startup_enabled() -> Result<bool> {
    Ok(false)
}

#[cfg(windows)]
async fn set_startup(enabled: bool) -> Result<(), &'static str> {
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
async fn set_startup(_: bool) -> Result<(), &'static str> {
    Err(STARTUP_ERROR)
}

#[cfg(test)]
mod settings_tests {
    use super::*;

    #[test]
    fn persists_pause_and_replaces_settings_atomically() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let settings_path = directory.path().join("data/settings.json");
        let defaults = Settings::defaults(None);
        defaults.save(&settings_path)?;
        let paused = Settings {
            preferences: Preferences {
                poll_interval_seconds: 120,
                notify_existing_on_first_run: false,
                start_with_windows: false,
            },
            enabled: false,
        };
        paused.save(&settings_path)?;
        let reopened = Settings::load(&settings_path, defaults);
        assert_eq!(paused.preferences, reopened.preferences);
        assert!(!reopened.enabled);
        Ok(())
    }

    #[test]
    fn check_now_rejects_retry_cooldown_and_can_resume_afterward() {
        let defaults = Settings::defaults(None);
        let (control, mut receiver) = Control::new(PathBuf::new(), defaults, true, true);
        control.lock_status().retry_not_before = Some(Instant::now() + Duration::from_secs(60));

        assert!(matches!(
            control.check_now(),
            Err(ControlError::Unavailable(RETRY_MESSAGE))
        ));
        assert_eq!(receiver.try_recv(), Err(mpsc::error::TryRecvError::Empty));

        control.lock_status().retry_not_before = Some(Instant::now());
        assert!(control.check_now().is_ok());
        assert_eq!(receiver.try_recv(), Ok(()));
    }
}
