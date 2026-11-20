use crate::twitch::Stream;
use anyhow::Result;
use std::{fmt, future::Future};

pub const APP_ID: &str = "BloodborneFRLive.Desktop";

pub trait Notifier {
    fn notify(&self, stream: &Stream) -> impl Future<Output = Result<()>>;
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
            "Notifications Bloodborne activées",
            "Les alertes des lives en français apparaîtront ici. Cliquez pour ouvrir Twitch.",
            "https://www.twitch.tv/directory/category/bloodborne",
        )
    }

    fn show(&self, tag: &str, title: &str, body: &str, url: &str) -> Result<()> {
        let xml = toast_xml(title, body, url);
        #[cfg(windows)]
        {
            native::show(tag, &xml)
        }
        #[cfg(not(windows))]
        {
            let _ = (tag, xml);
            anyhow::bail!("Les notifications locales nécessitent Windows 10 ou 11")
        }
    }
}

impl Notifier for DesktopNotifier {
    async fn notify(&self, stream: &Stream) -> Result<()> {
        stream.validate()?;
        self.show(
            &stream.id,
            &format!("{} joue à Bloodborne en français", stream.user_name),
            &stream.title,
            &stream.url(),
        )
    }
}

pub(crate) fn toast_xml(title: &str, body: &str, url: &str) -> String {
    let title = escape_xml(title, 200);
    let body = escape_xml(body, 600);
    let url = escape_xml(url, 2048);
    format!(
        r#"<toast activationType="protocol" launch="{url}" duration="long"><visual><binding template="ToastGeneric"><text>{title}</text><text>{body}</text><text placement="attribution">Bloodborne · Français</text></binding></visual><actions><action content="Voir sur Twitch" activationType="protocol" arguments="{url}"/></actions><audio src="ms-winsoundevent:Notification.Default"/></toast>"#
    )
}

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

    pub fn register() -> Result<()> {
        let key = windows_registry::CURRENT_USER
            .create(format!(r"Software\Classes\AppUserModelId\{APP_ID}"))
            .map_err(|e| failure("enregistrement de l’identité", e))?;
        key.set_string("DisplayName", "Bloodborne FR Live")
            .map_err(|e| failure("enregistrement du nom", e))?;
        // Protocol-only activation is handled by Windows, including after this app exits.
        key.set_string("CustomActivator", "{1D0E707F-2F09-49DC-BF84-E75B83951F5C}")
            .map_err(|e| failure("enregistrement de l’activateur", e))?;
        Ok(())
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

    pub fn show(tag: &str, xml: &str) -> Result<()> {
        register()?;
        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))
            .map_err(|e| failure("création de l’émetteur", e))?;
        if !enabled(&notifier)? {
            return Err(NotificationError("autorisations désactivées. Activez Bloodborne FR Live dans Paramètres > Système > Notifications".into()).into());
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
            .SetGroup(&HSTRING::from("bloodborne-fr"))
            .map_err(|e| failure("affectation du groupe", e))?;
        notifier.Show(&toast).map_err(|e| failure("affichage", e))?;
        Ok(())
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
    #[ignore = "requires an interactive Windows session and shows one real test notification"]
    fn notification_is_saved_in_windows_notification_center() {
        let notifier = DesktopNotifier::new().unwrap();
        let app_id = HSTRING::from(APP_ID);
        let history = ToastNotificationManager::History().unwrap();
        // A previous run can leave the same test alert in Action Center.
        let removed = history.RemoveGroupedTagWithId(
            &HSTRING::from(native::notification_tag("test")),
            &HSTRING::from("bloodborne-fr"),
            &app_id,
        );
        if let Err(error) = removed {
            assert_eq!(
                error.code(),
                windows::Win32::Foundation::ERROR_NOT_FOUND.to_hresult()
            );
        }
        assert!(DesktopNotifier::available());
        notifier.test_notification().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let entries = history.GetHistoryWithId(&app_id).unwrap();
            let found = (0..entries.Size().unwrap()).any(|i| {
                entries
                    .GetAt(i)
                    .unwrap()
                    .Content()
                    .unwrap()
                    .GetXml()
                    .unwrap()
                    .to_string()
                    .contains("Notifications Bloodborne activées")
            });
            if found {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Notification absente du centre Windows"
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}
