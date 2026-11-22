use crate::twitch::Stream;
use anyhow::Result;
use std::{fmt, future::Future};

pub const APP_ID: &str = "TwitchLiveAlert.Desktop";

pub trait Notifier {
    fn notify(&self, stream: &Stream) -> impl Future<Output = Result<()>>;

    fn notify_rule(
        &self,
        _rule_id: &str,
        _rule_name: &str,
        stream: &Stream,
    ) -> impl Future<Output = Result<()>> {
        self.notify(stream)
    }
}

#[derive(Debug)]
pub struct NotificationError(pub String);
impl fmt::Display for NotificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Notifications Windows : {}", self.0)
    }
}
impl std::error::Error for NotificationError {}

pub struct DesktopNotifier;

impl DesktopNotifier {
    pub fn new() -> Result<Self> {
        #[cfg(windows)]
        native::register()?;
        #[cfg(not(windows))]
        anyhow::bail!(
            "Les notifications locales nécessitent Windows 10 ou 11 et une session utilisateur ouverte"
        );
        #[cfg(windows)]
        Ok(Self)
    }

    pub fn available() -> bool {
        #[cfg(windows)]
        {
            native::available()
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    pub fn test_notification(&self) -> Result<()> {
        self.show(
            "test",
            "Notifications Twitch Live Alert activées",
            "Les alertes des chaînes que vous surveillez apparaîtront ici.",
            "https://www.twitch.tv/directory",
        )
    }

    fn show(&self, tag: &str, title: &str, body: &str, url: &str) -> Result<()> {
        #[cfg(windows)]
        {
            native::show(tag, title, body, url, false)
        }
        #[cfg(not(windows))]
        {
            let _ = (tag, title, body, url);
            Err(
                NotificationError("Les notifications locales nécessitent Windows 10 ou 11".into())
                    .into(),
            )
        }
    }
}

impl Notifier for DesktopNotifier {
    async fn notify(&self, stream: &Stream) -> Result<()> {
        stream.validate()?;
        self.show(
            &stream.id,
            &format!("{} est en direct", stream.user_name),
            &stream.title,
            &stream.url(),
        )
    }

    async fn notify_rule(&self, rule_id: &str, rule_name: &str, stream: &Stream) -> Result<()> {
        stream.validate()?;
        self.show(
            &format!("{rule_id}:{}", stream.id),
            &format!("{} est en direct", stream.user_name),
            &format!("{rule_name} · {}", stream.title),
            &stream.url(),
        )
    }
}

#[cfg(any(windows, test))]
pub(crate) fn toast_xml(title: &str, body: &str, url: &str, logo_uri: &str) -> String {
    let title = escape_xml(title, 200);
    let body = escape_xml(body, 600);
    let url = escape_xml(url, 2048);
    let logo = escape_xml(logo_uri, 32768);
    format!(
        r#"<toast activationType="protocol" launch="{url}" duration="long"><visual><binding template="ToastGeneric"><text>{title}</text><text>{body}</text><image placement="appLogoOverride" src="{logo}" alt="Twitch Live Alert"/><text placement="attribution">Twitch Live Alert</text></binding></visual><actions><action content="Voir sur Twitch" activationType="protocol" arguments="{url}"/></actions><audio src="ms-winsoundevent:Notification.Default"/></toast>"#
    )
}

#[cfg(any(windows, test))]
fn escape_xml(value: &str, limit: usize) -> String {
    let mut text = String::new();
    for ch in value
        .chars()
        .filter(|c| {
            matches!(c, '\t' | '\n' | '\r') || (*c >= ' ' && !matches!(*c, '\u{fffe}' | '\u{ffff}'))
        })
        .take(limit)
    {
        match ch {
            '&' => text.push_str("&amp;"),
            '<' => text.push_str("&lt;"),
            '>' => text.push_str("&gt;"),
            '"' => text.push_str("&quot;"),
            '\'' => text.push_str("&apos;"),
            _ => text.push(ch),
        }
    }
    text
}

#[cfg(windows)]
mod native {
    use super::*;
    use windows::{
        Data::Xml::Dom::XmlDocument,
        UI::Notifications::{NotificationSetting, ToastNotification, ToastNotificationManager},
        core::HSTRING,
    };

    fn failure(operation: &str, error: impl fmt::Display) -> anyhow::Error {
        NotificationError(format!("{operation} : {error}")).into()
    }

    fn save_logo(dir: &std::path::Path) -> Result<std::path::PathBuf> {
        use std::{fs, io::Write};
        const PNG: &[u8] = include_bytes!("notification/logo.png");
        let file = dir.join("notification-logo.png");
        if fs::read(&file).as_deref().is_ok_and(|stored| stored == PNG) {
            return Ok(file);
        }
        fs::create_dir_all(dir)?;
        let mut pending = tempfile::NamedTempFile::new_in(dir)?;
        pending.write_all(PNG)?;
        pending.persist(&file)?;
        Ok(file)
    }

    pub fn register() -> Result<reqwest::Url> {
        let dir = std::env::var_os("LOCALAPPDATA")
            .map(std::path::PathBuf::from)
            .ok_or_else(|| failure("emplacement du logo", "LOCALAPPDATA indisponible"))?;
        let logo =
            save_logo(&dir.join("TwitchLiveAlert")).map_err(|e| failure("écriture du logo", e))?;
        let uri = reqwest::Url::from_file_path(&logo)
            .map_err(|_| failure("emplacement du logo", "chemin absolu requis"))?;
        let key = windows_registry::CURRENT_USER
            .create(format!(r"Software\Classes\AppUserModelId\{APP_ID}"))
            .map_err(|e| failure("enregistrement de l’identité", e))?;
        key.set_string("DisplayName", "Twitch Live Alert")
            .map_err(|e| failure("enregistrement du nom", e))?;
        key.set_string("IconUri", logo.to_string_lossy())
            .map_err(|e| failure("enregistrement du logo", e))?;
        // Protocol-only activation is handled by Windows, including after this app exits.
        key.set_string("CustomActivator", "{1D0E707F-2F09-49DC-BF84-E75B83951F5C}")
            .map_err(|e| failure("enregistrement de l’activateur", e))?;
        Ok(uri)
    }

    fn enabled(notifier: &windows::UI::Notifications::ToastNotifier) -> Result<bool> {
        match notifier.Setting() {
            Ok(setting) => Ok(setting == NotificationSetting::Enabled),
            // Windows creates the sender settings on its first successful Show.
            Err(e) if e.code() == windows::Win32::Foundation::ERROR_NOT_FOUND.to_hresult() => {
                Ok(true)
            }
            Err(e) => Err(failure("lecture des autorisations", e)),
        }
    }

    pub fn available() -> bool {
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))
            .is_ok_and(|notifier| enabled(&notifier).unwrap_or(false))
    }

    pub fn show(tag: &str, title: &str, body: &str, url: &str, silent: bool) -> Result<()> {
        let logo_uri = register()?;
        let xml = toast_xml(title, body, url, logo_uri.as_str());
        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))
            .map_err(|e| failure("création de l’émetteur", e))?;
        if !enabled(&notifier)? {
            return Err(NotificationError("autorisations désactivées. Activez Twitch Live Alert dans Paramètres > Système > Notifications".into()).into());
        }
        let document = XmlDocument::new().map_err(|e| failure("création du document XML", e))?;
        document
            .LoadXml(&HSTRING::from(xml))
            .map_err(|e| failure("lecture du document XML", e))?;
        let toast = ToastNotification::CreateToastNotification(&document)
            .map_err(|e| failure("création de la notification", e))?;
        // Stable tags replace an uncertain retry in Action Center instead of accumulating copies.
        toast
            .SetTag(&HSTRING::from(notification_tag(tag)))
            .map_err(|e| failure("affectation du tag", e))?;
        toast
            .SetGroup(&HSTRING::from("twitch-live-alert"))
            .map_err(|e| failure("affectation du groupe", e))?;
        if silent {
            toast
                .SetSuppressPopup(true)
                .map_err(|e| failure("notification silencieuse", e))?;
        }
        notifier.Show(&toast).map_err(|e| failure("affichage", e))?;
        Ok(())
    }

    #[test]
    fn logo_is_available_for_windows_after_the_service_exits() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("logo espace & accent é");
        let logo = save_logo(&dir).unwrap();
        let uri = reqwest::Url::from_file_path(&logo).unwrap();
        assert_eq!(uri.to_file_path().unwrap(), logo);
        let png = std::fs::read(&logo).unwrap();
        assert_eq!(&png[16..24], &[0, 0, 1, 0, 0, 0, 1, 0]);
        std::fs::write(&logo, b"invalid").unwrap();
        assert_eq!(std::fs::read(save_logo(&dir).unwrap()).unwrap(), png);
        let doc = XmlDocument::new().unwrap();
        doc.LoadXml(&HSTRING::from(toast_xml(
            "Live",
            "Twitch",
            "https://www.twitch.tv/",
            uri.as_str(),
        )))
        .unwrap();
        let image = doc
            .SelectSingleNode(&HSTRING::from("//image[@placement='appLogoOverride']/@src"))
            .unwrap();
        assert_eq!(image.InnerText().unwrap().to_string(), uri.as_str());
    }

    pub(super) fn notification_tag(value: &str) -> String {
        // Twitch stream IDs currently fit; a deterministic 64-bit hash keeps all tags <= 16 chars.
        let hash = value.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        });
        format!("{hash:016x}")
    }
}

#[cfg(all(test, windows))]
mod desktop_tests {
    use super::*;
    use windows::{UI::Notifications::ToastNotificationManager, core::HSTRING};

    #[test]
    #[ignore = "requires Windows Notification Center in a logged-in user session"]
    fn notification_is_saved_in_windows_notification_center() {
        let app_id = HSTRING::from(APP_ID);
        let history = ToastNotificationManager::History().unwrap();
        let tag = format!("logo-check-{}", std::process::id());
        let windows_tag = HSTRING::from(native::notification_tag(&tag));
        let group = HSTRING::from("twitch-live-alert");
        let title = "Vérification silencieuse du logo";
        native::show(
            &tag,
            title,
            "Twitch Live Alert",
            "https://www.twitch.tv/directory",
            true,
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let stored_logo = loop {
            let entries = history.GetHistoryWithId(&app_id).unwrap();
            let mut uri = None;
            for i in 0..entries.Size().unwrap() {
                let toast = entries.GetAt(i).unwrap();
                if toast.Tag().unwrap() == windows_tag && toast.Group().unwrap() == group {
                    let xml = toast.Content().unwrap();
                    uri = xml
                        .SelectSingleNode(&HSTRING::from(
                            "//image[@placement='appLogoOverride']/@src",
                        ))
                        .and_then(|node| node.InnerText())
                        .ok();
                    break;
                }
            }
            if uri.is_some() || std::time::Instant::now() >= deadline {
                break uri;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        history
            .RemoveGroupedTagWithId(&windows_tag, &group, &app_id)
            .unwrap();
        let uri = reqwest::Url::parse(
            &stored_logo
                .expect("logo absent du centre de notifications")
                .to_string(),
        )
        .unwrap();
        assert_eq!(
            std::fs::read(uri.to_file_path().unwrap()).unwrap(),
            include_bytes!("notification/logo.png")
        );
    }
}
