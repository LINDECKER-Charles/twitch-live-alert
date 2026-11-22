use super::{Control, NOTIFICATION_ERROR, poll::Poller};
use crate::{http, notification::Notifier, store::Store};
use anyhow::Result;
use std::time::Duration;
use tokio::{
    sync::{mpsc, watch},
    time::{Instant, sleep_until},
};
use tracing::warn;

pub(super) async fn run_worker(
    mut store: Store,
    notifier: impl Notifier,
    control: &Control,
    mut checks: mpsc::Receiver<()>,
) -> Result<()> {
    let mut settings = control.settings.subscribe();
    let mut shutdown = control.shutdown.subscribe();
    let mut credentials = control.twitch.subscribe();
    let mut last_finished: Option<(Instant, chrono::DateTime<chrono::Utc>)> = None;
    let mut retry_until: Option<Instant> = None;
    let mut failures = 0u32;
    let mut previous_rules = Vec::new();
    let mut previous_session = None;
    while !*shutdown.borrow() {
        let current = settings.borrow_and_update().clone();
        let rules = current.rules();
        let session = credentials.borrow_and_update().clone();
        if session
            .as_ref()
            .zip(previous_session.as_ref())
            .is_none_or(|(a, b)| !a.same_session(b))
            || previous_rules != rules
        {
            last_finished = None;
            retry_until = None;
            failures = 0;
            previous_session = session.clone();
            previous_rules.clone_from(&rules);
        }
        let Some(mut twitch) = session.filter(|_| current.enabled && current.active_count() > 0)
        else {
            last_finished = None;
            while checks.try_recv().is_ok() {}
            tokio::select! {
                _ = shutdown.changed() => break,
                _ = credentials.changed() => continue,
                _ = settings.changed() => continue,
            }
        };
        let now = Instant::now();
        let due = last_finished
            .map_or(now, |(last, wall)| {
                last + super::poll::next_delay(&rules, wall, current.interval())
            })
            .max(retry_until.unwrap_or(now));
        control.lock_status().next_check_at =
            Some(http::unix_seconds().saturating_add(due.saturating_duration_since(now).as_secs()));
        tokio::select! {
            biased;
            _ = shutdown.changed() => break,
            _ = credentials.changed() => continue,
            _ = settings.changed() => continue,
            _ = sleep_until(due) => {let _ = checks.try_recv();},
            _ = async { if let Some(deadline) = retry_until { sleep_until(deadline).await; } checks.recv().await } => {}
        }
        control.lock_status().next_check_at = None;
        let (cancel, mut cancel_rx) = watch::channel(false);
        let mut poller = Poller {
            store: &mut store,
            notifier: &notifier,
            twitch: &mut twitch,
            notify_existing: current.preferences.notify_existing_on_first_run,
        };
        let (report, cancelled) = {
            let cycle = poller.poll(&rules, &mut cancel_rx);
            tokio::pin!(cycle);
            tokio::select! {
                biased;
                _ = shutdown.wait_for(|s|*s) => {cancel.send_replace(true);(cycle.await?,true)},
                _ = credentials.changed() => {cancel.send_replace(true);(cycle.await?,true)},
                _ = settings.wait_for(|s|!s.enabled || s.rules()!=rules) => {cancel.send_replace(true);(cycle.await?,true)},
                result = &mut cycle => (result?,false),
            }
        };
        let history = store.recent_history()?;
        let mut status = control.lock_status();
        status.notifications = history;
        if cancelled
            || !status.running
            || control.settings.borrow().rules() != rules
            || !control
                .twitch
                .borrow()
                .as_ref()
                .is_some_and(|s| s.same_session(&twitch))
        {
            last_finished = None;
            continue;
        }
        let error = report.warning.map(|e| {
            failures = failures.saturating_add(1);
            let delay = http::retry_delay(&e, failures, current.interval());
            retry_until = Instant::now()
                .checked_add(delay)
                .or_else(|| Some(Instant::now() + Duration::from_secs(3600)));
            warn!(
                retry_seconds = delay.as_secs(),
                "Une vérification ou une notification sera réessayée"
            );
            e.downcast_ref::<http::ApiError>()
                .map_or_else(|| NOTIFICATION_ERROR.to_owned(), ToString::to_string)
        });
        if error.is_none() {
            failures = 0;
            retry_until = None;
        }
        status.last_error = error;
        status.live_streams = report.live;
        status.last_checked_at = Some(http::unix_seconds());
        status.retry_not_before = retry_until;
        last_finished = Some((Instant::now(), chrono::Utc::now()));
    }
    Ok(())
}
