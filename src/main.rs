use anyhow::{Context, Result};
use clap::Parser;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;
use twitch_live_alert::{
    config::{Config, Mode},
    notification::DesktopNotifier,
    runtime,
};

#[derive(Parser)]
#[command(
    version,
    about = "Notifications Windows pour les directs Twitch de la catégorie et de la langue sélectionnées"
)]
#[command(group(clap::ArgGroup::new("mode").args(["once", "dry_run", "test_notification", "check_config", "no_web", "import_credentials"]).multiple(false)))]
struct Args {
    /// Exécuter un seul cycle de surveillance puis quitter
    #[arg(long)]
    once: bool,
    /// Afficher les lives correspondants sans envoyer d'alerte ni modifier la base
    #[arg(long)]
    dry_run: bool,
    /// Afficher une notification Windows de test, sans contacter Twitch
    #[arg(long)]
    test_notification: bool,
    /// Valider la configuration sans réseau ni création de base
    #[arg(long)]
    check_config: bool,
    /// Importer les identifiants Twitch de l'environnement ou du .env local dans ce profil Windows
    #[arg(long, value_name = "PROFIL")]
    import_credentials: Option<std::path::PathBuf>,
    /// Port HTTP de l'interface locale
    #[arg(long, default_value_t = 37219, value_parser = clap::value_parser!(u16).range(1..))]
    port: u16,
    /// Détacher la console pour un lancement discret sous Windows
    #[arg(long)]
    background: bool,
    /// Surveiller dans le terminal sans démarrer l'interface web
    #[arg(long)]
    no_web: bool,
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let args = Args::parse();
    if args.background {
        #[cfg(windows)]
        unsafe {
            let _ = windows::Win32::System::Console::FreeConsole();
        }
    }
    let dotenv = if std::env::var_os("TWITCH_LIVE_ALERT_DESKTOP_EXE").is_some() {
        Ok(())
    } else {
        load_dotenv()
    };
    // Keep third-party HTTP tracing from exposing credentials.
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
    match run(args, dotenv).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            error!("{error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run(args: Args, dotenv: Result<()>) -> Result<()> {
    if let Some(profile) = args.import_credentials {
        dotenv?;
        runtime::import_credentials(&profile).await?;
        info!("Identifiants Twitch validés et enregistrés dans le profil Windows");
        return Ok(());
    }
    if !(args.check_config || args.test_notification || args.dry_run || args.once || args.no_web) {
        return runtime::serve(
            args.port,
            dotenv.and_then(|()| Config::from_env(Mode::Watch)),
            shutdown_signal(),
        )
        .await;
    }
    dotenv?;
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
    if args.test_notification {
        DesktopNotifier::new()?.test_notification()?;
        info!("Notification de test transmise à Windows");
        return Ok(());
    }
    if args.no_web {
        return runtime::watch_in_terminal(config, shutdown_signal()).await;
    }
    runtime::check_once(config, args.dry_run).await
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
    match dotenvy::from_path(".env") {
        Ok(()) => {}
        Err(dotenvy::Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => {
            anyhow::bail!("Impossible de charger .env : fichier illisible ou syntaxe invalide")
        }
    }
    Ok(())
}
