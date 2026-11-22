mod connection;
mod rules;
mod settings;
pub(crate) use connection::ConnectionState;
#[cfg(test)]
mod rule_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
use crate::twitch::StreamFilter;
mod poll;
mod services;
mod worker;
pub use services::check_once;
use services::run_services;
use settings::Preferences;
pub(crate) use settings::PreferencesUpdate;
use settings::Settings;
use worker::run_worker;

use crate::{
    config::{Config, Mode},
    credentials::Credentials,
    http,
    notification::DesktopNotifier,
    startup::{set_startup, startup_enabled},
    store::{Delivery, History, Store},
    twitch::{Category, Stream, Twitch},
    web,
};
use anyhow::{Context, Result};
use serde::Serialize;
use std::{
    future::Future,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::{
    net::TcpListener,
    sync::{mpsc, watch},
    time::Instant,
};
use tracing::{error, info, warn};

const SETTINGS_PATH: &str = "data/settings.json";
const RETRY_MESSAGE: &str = "Une vérification est en attente de réessai. La prochaine tentative se lancera automatiquement.";
const SETTINGS_ERROR: &str = "Impossible d'enregistrer les réglages locaux.";
const STORAGE_ERROR: &str = "La surveillance est arrêtée car la base locale est inaccessible.";
#[cfg(windows)]
const NOTIFICATION_ERROR: &str =
    "Windows n'a pas accepté une notification. Vérifiez les autorisations de notification.";

#[cfg(not(windows))]
const NOTIFICATION_ERROR: &str = "Les notifications de bureau nécessitent une session Windows.";

#[derive(Clone, Serialize)]
pub(crate) struct Status {
    app_id: &'static str,
    process_id: u32,
    app_version: &'static str,
    pub running: bool,
    pub enabled: bool,
    pub active_rule_count: usize,
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
    rule_id: String,
    rule_name: String,
    id: String,
    user_name: String,
    title: String,
    url: String,
    viewer_count: u64,
    started_at: String,
}

impl LiveStream {
    fn new(rule: &crate::rules::Rule, stream: &Stream) -> Self {
        Self {
            rule_id: rule.id.clone(),
            rule_name: rule.name.clone(),
            id: stream.id.clone(),
            user_name: stream.user_name.clone(),
            title: stream.title.clone(),
            url: stream.url(),
            viewer_count: stream.viewer_count,
            started_at: stream.started_at.clone(),
        }
    }
}

#[derive(Debug)]
pub(crate) enum ControlError {
    Invalid(&'static str),
    Unavailable(&'static str),
    Operation(&'static str),
}

pub(crate) struct Control {
    status: Mutex<Status>,
    connection: Mutex<ConnectionState>,
    pub(crate) mail: crate::mail::MailService,
    settings: watch::Sender<Settings>,
    settings_path: PathBuf,
    database_path: Option<PathBuf>,
    twitch: watch::Sender<Option<Twitch>>,
    changes: tokio::sync::Mutex<()>,
    checks: mpsc::Sender<()>,
    shutdown: watch::Sender<bool>,
}

impl Control {
    fn new(
        settings_path: PathBuf,
        settings: Settings,
        twitch: Option<Twitch>,
        database_path: Option<PathBuf>,
    ) -> (Arc<Self>, mpsc::Receiver<()>) {
        let twitch = twitch.filter(|_| !settings.disconnected);
        let configured = twitch.is_some();
        let worker_available = database_path.is_some();
        let (twitch, _) = watch::channel(twitch);
        let (settings_sender, mail_settings) = watch::channel(settings.clone());
        let (shutdown, _) = watch::channel(false);
        let (checks, check_receiver) = mpsc::channel(1);
        let control = Self {
            connection: Mutex::new(ConnectionState::new(configured)),
            mail: crate::mail::MailService::new(
                settings_path
                    .parent()
                    .unwrap_or_else(|| Path::new("data"))
                    .to_path_buf(),
            )
            .with_database(database_path.clone())
            .with_eligibility(move || mail_settings.borrow().active_mail_ids()),
            status: Mutex::new(Status {
                app_id: "twitch-live-alert",
                process_id: std::process::id(),
                app_version: env!("CARGO_PKG_VERSION"),
                running: configured
                    && worker_available
                    && settings.enabled
                    && settings.active_count() > 0,
                enabled: settings.enabled,
                active_rule_count: settings.active_count(),
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
            twitch,
            database_path,
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

    pub async fn change_preferences(&self, update: PreferencesUpdate) -> Result<(), ControlError> {
        let _guard = self.changes.lock().await;
        let previous = self.settings.borrow().clone();
        let mut preferences = update.merge(&previous.preferences);
        preferences.filter.category = preferences.filter.category.trim().to_owned();
        preferences.filter.language = preferences.filter.language.trim().to_ascii_lowercase();
        if !preferences.filter.valid() {
            return Err(ControlError::Invalid(
                "Saisissez une catégorie Twitch et un code de langue valide.",
            ));
        }
        if !(15..=3600).contains(&preferences.poll_interval_seconds) {
            return Err(ControlError::Invalid(
                "L'intervalle doit être compris entre 15 et 3600 secondes.",
            ));
        }
        let connection = self.twitch.borrow().clone();
        if preferences.filter.category != previous.preferences.filter.category {
            let mut twitch = connection.ok_or(ControlError::Unavailable(
                "Connectez votre application Twitch pour valider la nouvelle catégorie.",
            ))?;
            let category = twitch
                .category(&preferences.filter.category)
                .await
                .map_err(category_error)?;
            preferences.filter.category = category.name;
        }
        let mut settings = previous;
        if preferences.filter != settings.preferences.filter
            && let Some(rules) = &mut settings.rules
            && let Some(legacy) = rules.iter_mut().find(|r| r.id == crate::rules::LEGACY_ID)
        {
            legacy.filter = preferences.filter.clone();
            legacy.box_art_url.clear();
        }
        settings.preferences = preferences.clone();
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
        let available = {
            let status = self.lock_status();
            status.configured && status.worker_available
        };
        if enabled && !available {
            return Err(ControlError::Unavailable(
                "La surveillance est indisponible. Vérifiez le message d'état.",
            ));
        }
        let settings = Settings {
            enabled,
            ..self.settings.borrow().clone()
        };
        if settings.save(&self.settings_path).is_err() {
            return Err(ControlError::Operation(SETTINGS_ERROR));
        }
        self.apply(settings);
        Ok(())
    }

    fn apply(&self, settings: Settings) {
        let mut status = self.lock_status();
        if self.settings.borrow().rules() != settings.rules() {
            status.live_streams.clear();
            status.retry_not_before = None;
            status.next_check_at = None;
            status.last_checked_at = None;
            if status.worker_available {
                status.last_error = None;
            }
        }
        status.preferences = settings.preferences.clone();
        status.enabled = settings.enabled;
        status.active_rule_count = settings.active_count();
        status.running = status.configured
            && status.worker_available
            && settings.enabled
            && status.active_rule_count > 0
            && !*self.shutdown.borrow();
        if !status.running {
            status.next_check_at = None;
        }
        self.settings.send_replace(settings);
    }

    pub async fn credentials(&self, pair: Credentials) -> Result<(), ControlError> {
        if !pair.valid() {
            return Err(ControlError::Invalid(
                "Renseignez les deux identifiants Twitch sans espace.",
            ));
        }
        let _guard = self.changes.lock().await;
        let client = http::client()
            .map_err(|_| ControlError::Operation("Impossible de contacter Twitch."))?;
        let twitch = Twitch::new(client, pair.client_id.clone(), pair.client_secret.clone());
        self.install_credentials(pair, twitch).await
    }

    async fn install_credentials(
        &self,
        pair: Credentials,
        twitch: Twitch,
    ) -> Result<(), ControlError> {
        twitch
            .validate_credentials()
            .await
            .map_err(connection::credential_error)?;
        pair.save(&self.settings_path.with_file_name("twitch.dpapi"))
            .map_err(|_| {
                ControlError::Operation(
                    "Impossible de protéger les identifiants dans ce profil Windows.",
                )
            })?;
        let mut settings = self.settings.borrow().clone();
        if settings.disconnected {
            settings.disconnected = false;
            settings
                .save(&self.settings_path)
                .map_err(|_| ControlError::Operation(SETTINGS_ERROR))?;
            self.apply(settings);
        }
        *self.connection.lock().expect("connection lock") = ConnectionState::verified();
        let mut status = self.lock_status();
        status.configured = true;
        status.running = status.worker_available
            && status.enabled
            && status.active_rule_count > 0
            && !*self.shutdown.borrow();
        status.retry_not_before = None;
        if status.worker_available {
            status.last_error = None;
        }
        self.twitch.send_replace(Some(twitch));
        Ok(())
    }

    pub async fn categories(&self, query: String) -> Result<Vec<Category>, ControlError> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        if query.chars().count() > 200 {
            return Err(ControlError::Invalid("Recherche trop longue."));
        }
        let twitch = self
            .twitch
            .borrow()
            .clone()
            .ok_or(ControlError::Unavailable(
                "Connectez votre application Twitch pour rechercher une catégorie.",
            ))?;
        twitch
            .search_categories(query)
            .await
            .map_err(category_error)
    }

    pub async fn history(
        &self,
        query: String,
        offset: u32,
        limit: u32,
    ) -> Result<History, ControlError> {
        let path = self
            .database_path
            .clone()
            .ok_or(ControlError::Operation(STORAGE_ERROR))?;
        if !(1..=200).contains(&limit) || query.chars().count() > 256 {
            return Err(ControlError::Invalid("Pagination ou recherche invalide."));
        }
        tokio::task::spawn_blocking(move || Store::history_at(&path, &query, offset, limit))
            .await
            .ok()
            .and_then(Result::ok)
            .ok_or(ControlError::Operation(
                "Impossible de lire le journal des alertes.",
            ))
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

pub async fn import_credentials(profile: &Path) -> Result<()> {
    let value = |key| {
        std::env::var(key)
            .ok()
            .map(|v| v.trim().to_owned())
            .unwrap_or_default()
    };
    let pair = Credentials {
        client_id: value("TWITCH_CLIENT_ID"),
        client_secret: value("TWITCH_CLIENT_SECRET"),
    };
    anyhow::ensure!(
        pair.valid(),
        "Les identifiants TWITCH_CLIENT_ID et TWITCH_CLIENT_SECRET doivent être renseignés."
    );
    let twitch = Twitch::new(
        http::client()?,
        pair.client_id.clone(),
        pair.client_secret.clone(),
    );
    twitch.validate_credentials().await?;
    pair.save(&profile.join(crate::credentials::CREDENTIALS_FILE))?;
    let path = profile.join(SETTINGS_PATH);
    let mut settings = Settings::load(&path, Settings::defaults(None));
    if settings.disconnected {
        settings.disconnected = false;
        settings.save(&path)?;
    }
    Ok(())
}

pub async fn serve(
    port: u16,
    config: Result<Config>,
    shutdown_signal: impl Future<Output = Result<()>>,
) -> Result<()> {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .with_context(|| format!("Impossible d'ouvrir l'interface locale sur le port {port}. Vérifiez si elle est déjà lancée."))?;
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
    let store = Store::open_for_rules(&database_path, settings.rules.as_deref())
        .and_then(|store| Ok((store.recent_history()?, store)));
    let twitch = config
        .ok()
        .map(|config| {
            Ok::<_, anyhow::Error>(Twitch::new(
                http::client()?,
                config.client_id,
                config.client_secret,
            ))
        })
        .transpose()?;
    let (control, receiver) = Control::new(
        path,
        settings,
        twitch,
        store.is_ok().then_some(database_path),
    );
    let store = match store {
        Ok((history, store)) => {
            control.lock_status().notifications = history;
            Some(store)
        }
        Err(error) => {
            error!("{error:#}");
            control.fail_worker(STORAGE_ERROR);
            None
        }
    };
    info!("Interface locale ouverte sur http://127.0.0.1:{port}");
    let worker = async {
        if let Some(store) = store
            && let Err(error) = run_services(store, &control, receiver).await
        {
            error!("{error:#}");
            control.fail_worker(STORAGE_ERROR);
        }
    };
    let mut shutdown = control.shutdown.subscribe();
    let server = async {
        axum::serve(listener, web::router(control.clone(), port))
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
    let mut settings = Settings::load(Path::new(SETTINGS_PATH), Settings::defaults(Some(&config)));
    // --no-web is an explicit command to start watching from the terminal.
    settings.enabled = true;
    let store = Store::open_for_rules(&config.database_path, settings.rules.as_deref())?;
    let twitch = Twitch::new(http::client()?, config.client_id, config.client_secret);
    let (control, receiver) = Control::new(
        PathBuf::from(SETTINGS_PATH),
        settings,
        Some(twitch),
        Some(config.database_path),
    );
    let worker = run_services(store, &control, receiver);
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

fn category_error(error: anyhow::Error) -> ControlError {
    if let Some(api) = error.downcast_ref::<http::ApiError>() {
        if api.reason == "catégorie introuvable" {
            return ControlError::Invalid(
                "Cette catégorie est introuvable. Choisissez son nom exact sur Twitch.",
            );
        }
        if api.status.is_some_and(|status| status.as_u16() == 429) {
            return ControlError::Unavailable(
                "Twitch limite les requêtes. Réessayez après le délai indiqué par Twitch.",
            );
        }
    }
    ControlError::Unavailable("Impossible de consulter les catégories Twitch. Réessayez plus tard.")
}
