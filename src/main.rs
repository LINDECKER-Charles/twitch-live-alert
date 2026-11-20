use alert_bloodborne_fr_live::{
    config::{Config, Mode},
    discord::Discord,
    http,
    store::Store,
    twitch::Twitch,
    watcher::Watcher,
};
use anyhow::{Context, Result};
use clap::Parser;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    version,
    about = "Alertes Discord pour tous les lives Twitch Bloodborne déclarés en français"
)]
#[command(group(clap::ArgGroup::new("mode").args(["once", "dry_run", "test_notification", "check_config"]).multiple(false)))]
struct Args {
    /// Exécuter un seul cycle de surveillance puis quitter
    #[arg(long)]
    once: bool,
    /// Afficher les lives correspondants sans envoyer d'alerte ni modifier la base
    #[arg(long)]
    dry_run: bool,
    /// Envoyer un message de test Discord, sans contacter Twitch
    #[arg(long)]
    test_notification: bool,
    /// Valider la configuration sans réseau ni création de base
    #[arg(long)]
    check_config: bool,
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let args = Args::parse();
    if let Err(error) = load_dotenv() {
        eprintln!("{error:#}");
        return std::process::ExitCode::FAILURE;
    }
    // Third-party HTTP tracing can expose webhook URLs even at debug level.
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"))
        .add_directive("reqwest=off".parse().unwrap())
        .add_directive("hyper=off".parse().unwrap())
        .add_directive("hyper_util=off".parse().unwrap())
        .add_directive("h2=off".parse().unwrap())
        .add_directive("rustls=off".parse().unwrap())
        .add_directive("tower_http=off".parse().unwrap());
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
    match run(args).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            error!("{error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> Result<()> {
    let mode = if args.test_notification {
        Mode::TestNotification
    } else if args.dry_run {
        Mode::DryRun
    } else {
        Mode::Watch
    };
    let config = Config::from_env(mode)?;
    if args.check_config {
        info!("Configuration valide (aucun appel réseau effectué)");
        return Ok(());
    }
    let client = http::client()?;
    if args.test_notification {
        Discord::new(client, config.webhook.expect("validated webhook"))
            .test_notification()
            .await?;
        info!("Message de test confirmé par Discord");
        return Ok(());
    }
    let mut twitch = Twitch::new(client.clone(), config.client_id, config.client_secret);
    if args.dry_run {
        let streams = twitch.french_bloodborne_streams().await?;
        for stream in &streams {
            println!(
                "{} — {} — {}",
                stream.user_name,
                stream.url(),
                stream.title.replace(['\n', '\r'], " ")
            );
        }
        info!(
            live_count = streams.len(),
            "Simulation terminée, aucun envoi ni écriture d'état"
        );
        return Ok(());
    }
    let mut watcher = Watcher {
        twitch,
        discord: Discord::new(client, config.webhook.expect("validated webhook")),
        store: Store::open(&config.database_path)?,
        notify_existing: config.notify_existing,
    };
    if args.once {
        watcher.cycle().await?;
        return Ok(());
    }
    info!(
        interval_seconds = config.poll_interval.as_secs(),
        "Surveillance Bloodborne français démarrée"
    );
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);
    let (stop_sender, mut stop_receiver) = tokio::sync::watch::channel(false);
    let mut failures = 0u32;
    loop {
        let result = {
            let cycle = watcher.cycle_with_shutdown(&mut stop_receiver);
            tokio::pin!(cycle);
            tokio::select! {
                biased;
                result = &mut shutdown => {
                    result?;
                    info!("Arrêt demandé, confirmation de l'envoi en cours avant sortie");
                    let _ = stop_sender.send(true);
                    // Finish any Discord POST already in flight, including its SQLite write.
                    cycle.await?;
                    break;
                },
                result = &mut cycle => result,
            }
        };
        let delay = match result {
            Ok(_) => {
                failures = 0;
                config.poll_interval
            }
            Err(error) => {
                // State failures must stop the process to avoid repeated unrecorded deliveries.
                if error.downcast_ref::<http::ApiError>().is_none() {
                    return Err(error);
                }
                failures = failures.saturating_add(1);
                let delay = http::retry_delay(&error, failures, config.poll_interval);
                error!(retry_seconds = delay.as_secs_f64(), "{error:#}");
                delay
            }
        };
        tokio::select! {
            result = &mut shutdown => { result?; break; },
            _ = tokio::time::sleep(delay) => {},
        }
    }
    info!("Surveillance arrêtée");
    Ok(())
}

async fn shutdown_signal() -> Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .context("Impossible d'écouter SIGTERM")?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result.context("Impossible d'écouter Ctrl+C")?,
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c()
        .await
        .context("Impossible d'écouter Ctrl+C")?;
    Ok(())
}

fn load_dotenv() -> Result<()> {
    match dotenvy::dotenv() {
        Ok(_) => {}
        Err(dotenvy::Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => {
            anyhow::bail!("Impossible de charger .env : fichier illisible ou syntaxe invalide")
        }
    }
    Ok(())
}
