use super::LiveStream;
use crate::{
    http,
    notification::{NotificationError, Notifier},
    rules::Rule,
    store::Store,
    twitch::{StreamFilter, Twitch},
};
use anyhow::Result;
use chrono::{DateTime, TimeDelta, Timelike, Utc};
use std::time::Duration;
use tokio::sync::watch;

#[derive(Default)]
pub(super) struct Report {
    pub live: Vec<LiveStream>,
    pub warning: Option<anyhow::Error>,
}

impl Report {
    fn warning(&mut self, error: anyhow::Error) {
        let wait = |error: &anyhow::Error| {
            error
                .downcast_ref::<http::ApiError>()
                .and_then(|e| e.retry_after)
        };
        if self
            .warning
            .as_ref()
            .is_none_or(|previous| wait(&error) > wait(previous))
        {
            self.warning = Some(error);
        }
    }
}

pub(super) fn due_groups(rules: &[Rule], now: DateTime<Utc>) -> Vec<(StreamFilter, Vec<&Rule>)> {
    let mut groups: Vec<(StreamFilter, Vec<&Rule>)> = Vec::new();
    for rule in rules
        .iter()
        .filter(|r| r.enabled && r.detection.allows(now))
    {
        if let Some((_, rules)) = groups.iter_mut().find(|(f, _)| f == &rule.filter) {
            rules.push(rule);
        } else {
            groups.push((rule.filter.clone(), vec![rule]));
        }
    }
    groups
}

/// Wake when a detection window opens even if it is shorter than the polling interval.
pub(super) fn next_delay(rules: &[Rule], last: DateTime<Utc>, interval: Duration) -> Duration {
    let scheduled: Vec<_> = rules
        .iter()
        .filter(|r| r.enabled && !r.detection.windows.is_empty())
        .collect();
    if scheduled.is_empty() {
        return interval;
    }
    let stop = last + TimeDelta::from_std(interval).expect("bounded polling interval");
    let mut minute =
        last.with_second(0).unwrap().with_nanosecond(0).unwrap() + TimeDelta::minutes(1);
    while minute < stop {
        if scheduled.iter().any(|r| {
            r.detection.allows(minute) && !r.detection.allows(minute - TimeDelta::seconds(1))
        }) {
            return (minute - last).to_std().unwrap_or_default();
        }
        minute += TimeDelta::minutes(1);
    }
    interval
}

pub(super) struct Poller<'a, N> {
    pub store: &'a mut Store,
    pub notifier: &'a N,
    pub twitch: &'a mut Twitch,
    pub notify_existing: bool,
}

impl<N: Notifier> Poller<'_, N> {
    pub async fn poll(
        &mut self,
        rules: &[Rule],
        shutdown: &mut watch::Receiver<bool>,
    ) -> Result<Report> {
        let mut report = Report::default();
        for (filter, rules) in due_groups(rules, Utc::now()) {
            if *shutdown.borrow() {
                break;
            }
            let result = tokio::select! {
                biased;
                _ = shutdown.changed() => break,
                result = self.twitch.streams(&filter) => result,
            };
            match result {
                Ok(streams) => {
                    let now = Utc::now();
                    for rule in &rules {
                        if !rule.detection.allows(now) {
                            continue;
                        }
                        self.store
                            .observe(rule, &streams, self.notify_existing, now)?;
                        report
                            .live
                            .extend(streams.iter().map(|s| LiveStream::new(rule, s)));
                    }
                }
                Err(error) if error.is::<http::ApiError>() => report.warning(error),
                Err(error) => return Err(error),
            }
            for rule in rules {
                if !rule.channels.desktop || !rule.detection.allows(Utc::now()) {
                    continue;
                }
                for (name, stream) in self.store.pending_desktop(&rule.id)? {
                    tokio::task::yield_now().await;
                    if *shutdown.borrow() {
                        return Ok(report);
                    }
                    // Once started, finish and record the native operation even if the rule changes.
                    match self.notifier.notify_rule(&rule.id, &name, &stream).await {
                        Ok(()) => self.store.confirm_desktop(&rule.id, &stream.id)?,
                        Err(error) => {
                            report.warning(NotificationError(error.to_string()).into());
                            break;
                        }
                    }
                }
            }
        }
        Ok(report)
    }
}
