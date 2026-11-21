use super::{Control, LiveStream, NOTIFICATION_ERROR};
use crate::{
    http, notification::NotificationError, store::Store, twitch::StreamFilter, watcher::Watcher,
};
use anyhow::Result;
use std::time::Duration;
use tokio::{
    sync::{mpsc, watch},
    time::{Instant, sleep_until},
};
use tracing::error;

pub(super) async fn run_worker(
    store: Store,
    notifier: impl crate::notification::Notifier,
    control: &Control,
    mut checks: mpsc::Receiver<()>,
) -> Result<()> {
    let mut settings = control.settings.subscribe();
    let mut shutdown = control.shutdown.subscribe();
    let mut credentials = control.twitch.subscribe();
    let twitch = tokio::select! {
        biased;
        _ = shutdown.wait_for(|stopped| *stopped) => return Ok(()),
        result = credentials.wait_for(Option::is_some) => result?.clone().expect("configured"),
    };
    let mut watcher = Watcher {
        twitch,
        store,
        notifier,
        filter: StreamFilter::default(),
        notify_existing: true,
        last_streams: Vec::new(),
    };
    let mut last_finished = None;
    let mut retry_until: Option<Instant> = None;
    let mut failures = 0u32;
    while !*shutdown.borrow() {
        let current = settings.borrow_and_update().clone();
        let latest = credentials.borrow_and_update().clone().expect("configured");
        if !watcher.twitch.same_session(&latest) || watcher.filter != current.preferences.filter {
            // A new filter or account gets an immediate first check. The shared Twitch
            // session independently enforces any outstanding provider rate limit.
            watcher.twitch = latest;
            watcher.filter = current.preferences.filter.clone();
            watcher.last_streams.clear();
            last_finished = None;
            retry_until = None;
            failures = 0;
        }
        if !current.enabled {
            last_finished = None;
            while checks.try_recv().is_ok() {}
            tokio::select! {
                _ = shutdown.changed() => break,
                _ = credentials.changed() => continue,
                _ = settings.changed() => continue,
            }
        }
        let now = Instant::now();
        let due = last_finished
            .map_or(now, |last: Instant| last + current.interval())
            .max(retry_until.unwrap_or(now));
        {
            let mut status = control.lock_status();
            if status.running {
                status.next_check_at = Some(
                    http::unix_seconds()
                        .saturating_add(due.saturating_duration_since(now).as_secs()),
                );
            }
        }
        tokio::select! {
            biased;
            _ = shutdown.changed() => break,
            _ = settings.changed() => continue,
            _ = credentials.changed() => continue,
            _ = sleep_until(due) => { let _ = checks.try_recv(); },
            _ = async {
                if let Some(deadline) = retry_until {
                    sleep_until(deadline).await;
                }
                checks.recv().await
            } => {}
        }
        control.lock_status().next_check_at = None;
        watcher.notify_existing = current.preferences.notify_existing_on_first_run;
        let (cancel, mut cancel_receiver) = watch::channel(false);
        let (result, cancelled) = {
            let cycle = watcher.cycle_with_shutdown(&mut cancel_receiver);
            tokio::pin!(cycle);
            tokio::select! {
                biased;
                _ = shutdown.wait_for(|stop| *stop) => {
                    cancel.send_replace(true);
                    (cycle.await, true)
                }
                _ = credentials.changed() => {
                    cancel.send_replace(true);
                    (cycle.await, true)
                }
                _ = settings.wait_for(|value| !value.enabled || value.preferences.filter != current.preferences.filter) => {
                    cancel.send_replace(true);
                    (cycle.await, true)
                }
                result = &mut cycle => (result, false),
            }
        };
        let last_error = match result {
            Ok(_) => {
                if !cancelled {
                    failures = 0;
                    retry_until = None;
                }
                None
            }
            Err(error) => {
                // Stop if an alert could have been delivered without a durable record.
                let api_error = error.downcast_ref::<http::ApiError>();
                if api_error.is_none() && error.downcast_ref::<NotificationError>().is_none() {
                    return Err(error);
                }
                failures = failures.saturating_add(1);
                let delay = http::retry_delay(&error, failures, current.interval());
                retry_until = Instant::now()
                    .checked_add(delay)
                    .or_else(|| Some(Instant::now() + Duration::from_secs(3600)));
                error!(retry_seconds = delay.as_secs_f64(), "{error:#}");
                Some(api_error.map_or_else(|| NOTIFICATION_ERROR.to_owned(), ToString::to_string))
            }
        };
        let notifications = watcher.store.recent_history()?;
        let mut status = control.lock_status();
        status.notifications = notifications;
        if cancelled
            || !status.running
            || status.preferences.filter != current.preferences.filter
            || !control
                .twitch
                .borrow()
                .as_ref()
                .is_some_and(|t| t.same_session(&watcher.twitch))
        {
            last_finished = None;
            continue;
        }
        status.retry_not_before = retry_until;
        status.live_streams = watcher.last_streams.iter().map(LiveStream::from).collect();
        status.last_checked_at = Some(http::unix_seconds());
        status.last_error = last_error;
        last_finished = Some(Instant::now());
    }
    Ok(())
}
