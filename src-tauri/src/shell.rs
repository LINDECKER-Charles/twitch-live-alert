use crate::{
    service::{self, Service},
    win,
};
use anyhow::{Context, Result, ensure};
use std::{env, fs, path::PathBuf, sync::Arc, thread};
use tauri::{
    AppHandle, Manager, RunEvent, Url, WebviewUrl, WebviewWindowBuilder, WindowEvent,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    webview::NewWindowResponse,
};

struct Desktop {
    service: Arc<Service>,
    data: PathBuf,
}

fn external(url: &Url) -> bool {
    if url.scheme() != "https"
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    match url.host_str() {
        Some("dev.twitch.tv") => {
            url.path() == "/console/apps" || url.path() == "/console/apps/create"
        }
        Some("www.twitch.tv" | "twitch.tv") => {
            let login = url.path().trim_start_matches('/');
            !login.is_empty()
                && login.len() <= 25
                && login
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_')
        }
        _ => false,
    }
}

fn open_dashboard(app: &AppHandle) -> Result<()> {
    let state = app.state::<Desktop>();
    ensure!(
        state.service.available(),
        "Le service local s’est arrêté. Relancez Twitch Live Alert."
    );
    if let Some(window) = app.get_webview_window("dashboard") {
        window.unminimize()?;
        window.show()?;
        window.set_focus()?;
        return Ok(());
    }
    let origin: Url = state.service.origin.parse()?;
    let service = state.service.clone();
    WebviewWindowBuilder::new(app, "dashboard", WebviewUrl::External(origin.clone()))
        .title("Twitch Live Alert")
        .inner_size(1180., 820.)
        .min_inner_size(400., 480.)
        .data_directory(state.data.join("webview"))
        .on_navigation(move |url| {
            if url.origin() == origin.origin()
                && url.username().is_empty()
                && url.password().is_none()
            {
                return service.available();
            }
            if external(url) {
                win::open_url(url);
            }
            false
        })
        .on_new_window(|url, _| {
            if external(&url) {
                win::open_url(&url);
            }
            NewWindowResponse::Deny
        })
        .build()
        .context("Ouverture de la fenêtre Windows/WebView2")?;
    Ok(())
}

fn show(app: &AppHandle) {
    if let Err(error) = open_dashboard(app) {
        win::show_error(&format!("{error:#}"));
    }
}

fn setup(app: &mut tauri::App, background: bool) -> Result<()> {
    let data = service::data_dir()?;
    let service = Arc::new(Service::start(&data)?);
    app.manage(Desktop {
        service: service.clone(),
        data,
    });
    let open = MenuItem::with_id(app, "open", "Ouvrir Twitch Live Alert", true, None::<&str>)?;
    let browser = MenuItem::with_id(
        app,
        "browser",
        "Ouvrir dans le navigateur",
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quitter", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &browser, &quit])?;
    let icon = app
        .default_window_icon()
        .context("Icône du bureau introuvable.")?
        .clone();
    TrayIconBuilder::new()
        .icon(icon)
        .tooltip("Twitch Live Alert")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show(app),
            "browser" => {
                let state = app.state::<Desktop>();
                if state.service.available() {
                    win::open_url(
                        &Url::parse(&state.service.origin).expect("validated loopback origin"),
                    );
                } else {
                    win::show_error("Le service local s’est arrêté.");
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show(tray.app_handle());
            }
        })
        .build(app)?;
    if !background {
        open_dashboard(app.handle())?;
    }
    // Close a stale dashboard immediately when its exact service process exits.
    let app = app.handle().clone();
    thread::spawn(move || {
        let _ = service.process.exited(u32::MAX);
        app.exit(0);
    });
    Ok(())
}

pub fn report(error: &anyhow::Error, background: bool) {
    let message = format!("{error:#}");
    if let Ok(data) = service::data_dir() {
        let _ = fs::write(data.join("desktop-startup.log"), &message);
    }
    if !background {
        win::show_error(&message);
    }
}

pub fn run(background: bool) -> Result<i32> {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if app.try_state::<Desktop>().is_some() && !args.iter().any(|a| a == "--background") {
                show(app);
            }
        }))
        .setup(move |app| {
            if let Err(error) = setup(app, background) {
                report(&error, background);
                app.handle().exit(1);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())?;
    Ok(app.run_return(|app, event| {
        if let RunEvent::Exit = event
            && let Some(state) = app.try_state::<Desktop>()
        {
            state.service.shutdown();
        }
    }))
}

pub fn background() -> bool {
    env::args_os().skip(1).any(|arg| arg == "--background")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opens_only_channel_and_console_https_links() {
        for link in [
            "https://www.twitch.tv/chasseur_42",
            "https://dev.twitch.tv/console/apps",
        ] {
            assert!(external(&Url::parse(link).unwrap()));
        }
        for link in [
            "file:///C:/windows",
            "http://www.twitch.tv/name",
            "https://www.twitch.tv.example/name",
            "https://user@www.twitch.tv/name",
            "https://www.twitch.tv:444/name",
            "https://www.twitch.tv/a/b",
            "https://dev.twitch.tv/other",
        ] {
            assert!(!external(&Url::parse(link).unwrap()));
        }
    }
}
