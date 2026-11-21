use crate::{
    notification::Notifier,
    store::Store,
    twitch::{Stream, StreamFilter, Twitch},
};
use anyhow::Result;
use tracing::{info, warn};

pub struct Watcher<N: Notifier> {
    pub twitch: Twitch,
    pub notifier: N,
    pub store: Store,
    pub notify_existing: bool,
    pub filter: StreamFilter,
    pub last_streams: Vec<Stream>,
}

impl<N: Notifier> Watcher<N> {
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
        let streams = tokio::select! {
            biased;
            _ = shutdown.changed() => return Ok(0),
            streams = self.twitch.streams(&self.filter) => streams?,
        };
        if *shutdown.borrow() {
            return Ok(0);
        }
        self.last_streams = streams.clone();
        self.store
            .initialize(&self.filter, &streams, self.notify_existing)?;
        let mut sent = 0;
        for stream in &streams {
            // Native toast delivery may finish synchronously. Give the controller a
            // chance to cancel between alerts even when the whole page is new.
            tokio::task::yield_now().await;
            if *shutdown.borrow() {
                break;
            }
            if self.store.contains(&stream.id, &self.filter)? {
                continue;
            }
            if let Err(error) = self.notifier.notify(stream).await {
                warn!(stream_id = %stream.id, "Notification non confirmée, nouvelle tentative au prochain cycle");
                return Err(error);
            }
            self.store.mark_sent(stream)?;
            sent += 1;
            info!(stream_id = %stream.id, channel = %stream.user_login, "Notification locale transmise à Windows");
        }
        info!(
            live_count = streams.len(),
            alerts_sent = sent,
            "Vérification des directs terminée"
        );
        Ok(sent)
    }
}
