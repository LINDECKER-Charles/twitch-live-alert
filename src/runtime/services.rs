use super::{
    Config, Control, DesktopNotifier, Path, PathBuf, Result, SETTINGS_PATH, Settings, Store,
    Twitch, http, mpsc, poll, run_worker, watch,
};
use std::time::Duration;
use tracing::warn;

pub(super) async fn run_services(
    store: Store,
    control: &Control,
    checks: mpsc::Receiver<()>,
) -> Result<()> {
    let mail = async {
        let path = control
            .database_path
            .as_deref()
            .expect("database available");
        let mut shutdown = control.shutdown.subscribe();
        while !*shutdown.borrow() {
            if drain_mail(control, path).await.is_err() {
                warn!("Relève de la file mail indisponible.");
            }
            tokio::select! {
                _ = shutdown.changed()=> break,
                _ = tokio::time::sleep(Duration::from_secs(5))=> {},
            }
        }
        Ok(())
    };
    let validate = async {
        let mut shutdown = control.shutdown.subscribe();
        let mut session = control.twitch.subscribe();
        while !*shutdown.borrow() {
            let connected = session.borrow_and_update().is_some();
            if connected {
                let _ = control.verify(false).await;
            }
            tokio::select! {
                _ = shutdown.changed()=>break,
                _ = session.changed()=>{},
                _ = tokio::time::sleep(Duration::from_secs(3600))=>{},
            }
        }
        Ok(())
    };
    tokio::try_join!(
        run_worker(store, DesktopNotifier, control, checks),
        mail,
        validate
    )?;
    Ok(())
}

async fn drain_mail(control: &Control, path: &Path) -> Result<()> {
    for _ in 0..8 {
        let ids: Vec<_> = {
            let current = control.settings.borrow();
            if !current.enabled || current.disconnected || *control.shutdown.borrow() {
                return Ok(());
            }
            current
                .rules()
                .into_iter()
                .filter(|r| r.enabled && r.channels.email)
                .map(|r| r.id)
                .collect()
        };
        if ids.is_empty()
            || !control
                .mail
                .dispatch_next(path, chrono::Utc::now(), &ids)
                .await?
        {
            break;
        }
    }
    Ok(())
}

pub async fn check_once(config: Config, dry_run: bool) -> Result<()> {
    let path = PathBuf::from(SETTINGS_PATH);
    let mut settings = Settings::load(&path, Settings::defaults(Some(&config)));
    anyhow::ensure!(
        !settings.disconnected,
        "La connexion Twitch est désactivée dans ce profil."
    );
    // Explicit CLI checks run once even when continuous monitoring is paused.
    settings.enabled = true;
    let rules = settings.rules();
    let mut twitch = Twitch::new(http::client()?, config.client_id, config.client_secret);
    if dry_run {
        for (filter, rules) in poll::due_groups(&rules, chrono::Utc::now()) {
            for stream in twitch.streams(&filter).await? {
                for rule in &rules {
                    println!(
                        "[{}] {} — {} — {}",
                        rule.name,
                        stream.user_name,
                        stream.url(),
                        stream.title.replace(['\n', '\r'], " ")
                    );
                }
            }
        }
        return Ok(());
    }
    let mut store = Store::open(&config.database_path)?;
    let (_sender, mut cancel) = watch::channel(false);
    let mut poller = poll::Poller {
        store: &mut store,
        notifier: &DesktopNotifier,
        twitch: &mut twitch,
        notify_existing: settings.preferences.notify_existing_on_first_run,
    };
    let report = poller.poll(&rules, &mut cancel).await?;
    let (control, _) = Control::new(
        path,
        settings,
        Some(twitch),
        Some(config.database_path.clone()),
    );
    drain_mail(&control, &config.database_path).await?;
    if let Some(error) = report.warning {
        return Err(error);
    }
    Ok(())
}
