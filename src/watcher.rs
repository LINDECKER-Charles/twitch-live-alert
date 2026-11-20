use crate::{discord::Discord, store::Store, twitch::Twitch};
use anyhow::Result;
use tracing::{info, warn};

pub struct Watcher {
    pub twitch: Twitch,
    pub discord: Discord,
    pub store: Store,
    pub notify_existing: bool,
}

impl Watcher {
    pub async fn cycle(&mut self) -> Result<usize> {
        let (_sender, mut shutdown) = tokio::sync::watch::channel(false);
        self.cycle_with_shutdown(&mut shutdown).await
    }

    pub async fn cycle_with_shutdown(
        &mut self,
        shutdown: &mut tokio::sync::watch::Receiver<bool>,
    ) -> Result<usize> {
        if *shutdown.borrow() {
            return Ok(0);
        }
        // Fetch every page before touching state: a partial response is not a snapshot.
        let streams = tokio::select! {
            biased;
            _ = shutdown.changed() => return Ok(0),
            streams = self.twitch.french_bloodborne_streams() => streams?,
        };
        if *shutdown.borrow() {
            return Ok(0);
        }
        self.store.initialize(&streams, self.notify_existing)?;
        let mut sent = 0;
        let mut first_error = None;
        for stream in &streams {
            if *shutdown.borrow() {
                break;
            }
            if self.store.contains(&stream.id)? {
                continue;
            }
            match self.discord.notify(stream).await {
                Ok(message_id) => {
                    self.store.mark_sent(&stream.id, &message_id)?;
                    sent += 1;
                    info!(stream_id = %stream.id, channel = %stream.user_login, "Alerte Discord envoyée");
                }
                Err(error) => {
                    // A rate limit or invalid webhook applies to every following message.
                    // Stop this batch; successful messages are already durable.
                    warn!(stream_id = %stream.id, "Envoi non confirmé, nouvelle tentative au prochain cycle");
                    first_error = Some(error);
                    break;
                }
            }
        }
        info!(
            live_count = streams.len(),
            alerts_sent = sent,
            "Vérification Bloodborne français terminée"
        );
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(sent)
    }
}
