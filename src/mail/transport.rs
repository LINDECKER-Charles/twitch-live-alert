use super::{
    MailService,
    settings::{Security, Stored},
    template::Rendered,
};
use anyhow::{Context, Result};
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor, message::header::ContentType,
    transport::smtp::authentication::Credentials,
};
use std::time::Duration;

impl MailService {
    pub(super) async fn send(
        &self,
        stored: &Stored,
        batch_id: Option<&str>,
        rendered: Rendered,
    ) -> Result<()> {
        let config = &stored.settings;
        config.validate()?;
        let mut message = Message::builder()
            .from(config.sender.parse().context("Expéditeur SMTP invalide.")?)
            .subject(rendered.subject)
            .message_id(batch_id.map(|id| format!("<{id}@twitch-live-alert.local>")))
            .header(ContentType::TEXT_PLAIN);
        for recipient in &config.recipients {
            message = message.bcc(recipient.parse().context("Destinataire SMTP invalide.")?);
        }
        let message = message
            .body(rendered.body)
            .map_err(|_| anyhow::anyhow!("Construction du message SMTP impossible."))?;
        let builder = match config.security {
            Security::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host),
            Security::Starttls => {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)
            }
        }
        .map_err(|_| anyhow::anyhow!("Initialisation TLS impossible."))?;
        // Only the unit tests may add their short-lived, in-process test CA.
        #[cfg(test)]
        let builder = if let Some(cert) = &self.test_ca {
            use lettre::transport::smtp::client::{Tls, TlsParameters};
            let tls = TlsParameters::builder(config.host.clone())
                .add_root_certificate(cert.clone())
                .build()?;
            builder.tls(match config.security {
                Security::Tls => Tls::Wrapper(tls),
                Security::Starttls => Tls::Required(tls),
            })
        } else {
            builder
        };
        let mut builder = builder
            .port(config.port)
            .timeout(Some(Duration::from_secs(30)));
        if let Some(auth) = stored.auth()? {
            builder = builder.credentials(Credentials::new(
                auth.username.clone(),
                auth.password.clone(),
            ));
        }
        let transport = builder.build();
        let response = tokio::time::timeout(Duration::from_secs(90), transport.send(message)).await;
        match response {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) if e.is_permanent() => anyhow::bail!(
                "Le serveur SMTP refuse le message. Vérifiez les adresses et les identifiants."
            ),
            Ok(Err(e)) if e.is_tls() => anyhow::bail!(
                "Connexion TLS au serveur SMTP impossible. Vérifiez le certificat, le port et le mode TLS."
            ),
            Ok(Err(e)) if e.is_transient() => {
                anyhow::bail!("Le serveur SMTP est temporairement indisponible.")
            }
            Ok(Err(e)) if !e.is_timeout() => anyhow::bail!(
                "Connexion SMTP impossible. Vérifiez le serveur, le port et le mode TLS."
            ),
            _ => anyhow::bail!("Délai de réponse du serveur SMTP dépassé."),
        }
    }
}
