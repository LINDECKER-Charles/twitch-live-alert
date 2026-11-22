use super::{Control, ControlError, SETTINGS_ERROR, http};
use serde::Serialize;

#[derive(Clone, Serialize)]
pub(crate) struct ConnectionState {
    state: &'static str,
    verified_at: Option<u64>,
}

impl ConnectionState {
    pub(super) fn new(configured: bool) -> Self {
        Self {
            state: if configured {
                "unverified"
            } else {
                "unconfigured"
            },
            verified_at: None,
        }
    }
    pub(super) fn verified() -> Self {
        Self {
            state: "connected",
            verified_at: Some(http::unix_seconds()),
        }
    }
}

pub(super) fn credential_error(error: anyhow::Error) -> ControlError {
    if error.downcast_ref::<http::ApiError>().is_some_and(|e| {
        e.status
            .is_some_and(|s| matches!(s.as_u16(), 400 | 401 | 403))
            || e.reason == "jeton expiré ou destiné à une autre application"
    }) {
        ControlError::Invalid(
            "Twitch a refusé ces identifiants. Vérifiez le Client ID et le Client Secret.",
        )
    } else {
        ControlError::Unavailable(
            "Impossible de vérifier la connexion auprès de Twitch. Réessayez plus tard.",
        )
    }
}

impl Control {
    pub(crate) fn connection(&self) -> ConnectionState {
        self.connection.lock().expect("connection lock").clone()
    }

    pub(crate) async fn verify(&self, force: bool) -> Result<ConnectionState, ControlError> {
        let _guard = self.changes.lock().await;
        let session = self
            .twitch
            .borrow()
            .clone()
            .ok_or(ControlError::Unavailable(
                "Connectez votre application Twitch.",
            ))?;
        let result = if force {
            session.verify().await
        } else {
            session.validate_credentials().await
        };
        let mut state = self.connection.lock().expect("connection lock");
        match result.map_err(credential_error) {
            Ok(()) => *state = ConnectionState::verified(),
            Err(ControlError::Invalid(_)) => state.state = "invalid",
            Err(_) => state.state = "unavailable",
        }
        Ok(state.clone())
    }

    pub(crate) async fn disconnect(&self) -> Result<ConnectionState, ControlError> {
        let _guard = self.changes.lock().await;
        let mut settings = self.settings.borrow().clone();
        settings.disconnected = true;
        settings
            .save(&self.settings_path)
            .map_err(|_| ControlError::Operation(SETTINGS_ERROR))?;
        self.apply(settings);
        self.twitch.send_replace(None);
        *self.connection.lock().expect("connection lock") = ConnectionState::new(false);
        {
            let mut status = self.lock_status();
            status.running = false;
            status.configured = false;
            status.live_streams.clear();
            status.next_check_at = None;
            status.last_checked_at = None;
            status.retry_not_before = None;
            if status.worker_available {
                status.last_error = None;
            }
        }
        let path = self.settings_path.with_file_name("twitch.dpapi");
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                return Err(ControlError::Operation(
                    "Connexion interrompue. Windows n’a pas pu supprimer les identifiants protégés.",
                ));
            }
        }
        Ok(self.connection())
    }
}
