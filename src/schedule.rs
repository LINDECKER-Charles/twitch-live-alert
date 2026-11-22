use anyhow::{Result, bail, ensure};
use chrono::{
    DateTime, Datelike, Duration, Local, LocalResult, NaiveDateTime, NaiveTime, TimeZone, Utc,
};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Schedule {
    pub timezone: String,
    pub windows: Vec<Window>,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            timezone: "local".into(),
            windows: vec![],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Window {
    pub days: Vec<u8>,
    pub start: String,
    pub end: String,
}

pub(crate) fn time(value: &str) -> Result<NaiveTime> {
    ensure!(
        value.len() == 5 && value.as_bytes()[2] == b':',
        "Heure attendue au format HH:MM."
    );
    NaiveTime::parse_from_str(value, "%H:%M").map_err(|_| anyhow::anyhow!("Heure invalide."))
}

struct Zone(Option<Tz>);

impl Zone {
    fn parse(name: &str) -> Result<Self> {
        if name == "local" {
            Ok(Self(None))
        } else {
            name.parse::<Tz>()
                .map(|z| Self(Some(z)))
                .map_err(|_| anyhow::anyhow!("Fuseau horaire IANA inconnu."))
        }
    }

    fn wall(&self, utc: DateTime<Utc>) -> NaiveDateTime {
        match self.0 {
            Some(zone) => utc.with_timezone(&zone).naive_local(),
            None => utc.with_timezone(&Local).naive_local(),
        }
    }

    fn resolve(&self, wall: NaiveDateTime) -> LocalResult<DateTime<Utc>> {
        match self.0 {
            Some(zone) => zone.from_local_datetime(&wall).map(|t| t.to_utc()),
            None => Local.from_local_datetime(&wall).map(|t| t.to_utc()),
        }
    }

    fn opening(
        &self,
        mut start: NaiveDateTime,
        end: NaiveDateTime,
        now: DateTime<Utc>,
    ) -> Option<DateTime<Utc>> {
        // If the clock skipped the start, take the first representable local
        // minute inside this particular window, or skip the window altogether.
        let edges = loop {
            if start >= end {
                return None;
            }
            let mapped = self.resolve(start);
            if mapped != LocalResult::None {
                break mapped;
            }
            start += Duration::minutes(1);
        };
        let mut next = [edges.earliest(), edges.latest()]
            .into_iter()
            .flatten()
            .filter(|t| *t >= now)
            .min();
        let last = end - Duration::minutes(1);
        if matches!(edges, LocalResult::Single(_))
            && matches!(self.resolve(last), LocalResult::Ambiguous(_, _))
        {
            // A fall-back can reopen a window which started before the fold and
            // ended partway through it. Find the beginning of the repeated hour.
            let (mut lo, mut hi) = (start, last);
            while (hi - lo).num_minutes() > 1 {
                let mid = lo + Duration::minutes((hi - lo).num_minutes() / 2);
                if matches!(self.resolve(mid), LocalResult::Ambiguous(_, _)) {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            let second = self.resolve(hi).latest().filter(|t| *t >= now);
            next = next.into_iter().chain(second).min();
        }
        next
    }
}

impl Window {
    fn includes(&self, day: u8, clock: NaiveTime) -> bool {
        let (Ok(start), Ok(end)) = (time(&self.start), time(&self.end)) else {
            return false;
        };
        let previous = if day == 1 { 7 } else { day - 1 };
        match start.cmp(&end) {
            std::cmp::Ordering::Equal => self.days.contains(&day),
            std::cmp::Ordering::Less => self.days.contains(&day) && (start..end).contains(&clock),
            std::cmp::Ordering::Greater => {
                (clock >= start && self.days.contains(&day))
                    || (clock < end && self.days.contains(&previous))
            }
        }
    }
}

impl Schedule {
    pub fn validate(&self) -> Result<()> {
        Zone::parse(&self.timezone)?;
        ensure!(self.windows.len() <= 32, "Trop de plages horaires.");
        for window in &self.windows {
            ensure!(
                !window.days.is_empty()
                    && window.days.len() <= 7
                    && window.days.iter().all(|d| (1..=7).contains(d)),
                "Choisissez au moins un jour valide."
            );
            time(&window.start)?;
            time(&window.end)?;
        }
        Ok(())
    }

    pub(crate) fn allows_wall_time(&self, day: u8, clock: NaiveTime) -> bool {
        self.windows.is_empty() || self.windows.iter().any(|w| w.includes(day, clock))
    }

    fn allows_wall(&self, wall: NaiveDateTime) -> bool {
        self.allows_wall_time(wall.weekday().number_from_monday() as u8, wall.time())
    }

    pub fn allows(&self, now: DateTime<Utc>) -> bool {
        Zone::parse(&self.timezone).is_ok_and(|z| self.allows_wall(z.wall(now)))
    }

    pub(crate) fn next_window(&self, now: DateTime<Utc>) -> Result<DateTime<Utc>> {
        let zone = Zone::parse(&self.timezone)?;
        let wall = zone.wall(now);
        if self.allows_wall(wall) {
            return Ok(now);
        }
        let windows = self
            .windows
            .iter()
            .map(|w| Ok((&w.days, time(&w.start)?, time(&w.end)?)))
            .collect::<Result<Vec<_>>>()?;
        let mut earliest = None;
        for offset in -1..15 {
            let date = wall.date() + Duration::days(offset);
            if earliest.is_some_and(|utc| date > zone.wall(utc).date()) {
                break;
            }
            let day = date.weekday().number_from_monday() as u8;
            for &(days, start, end) in &windows {
                if !days.contains(&day) {
                    continue;
                }
                let (from, to) = if start == end {
                    let midnight = date.and_time(NaiveTime::MIN);
                    (midnight, midnight + Duration::days(1))
                } else {
                    (
                        date.and_time(start),
                        date.and_time(end) + Duration::days(i64::from(start > end)),
                    )
                };
                let next = zone.opening(from, to, now);
                earliest = earliest.into_iter().chain(next).min();
            }
        }
        if let Some(next) = earliest {
            return Ok(next);
        }
        bail!("Aucune ouverture de plage horaire.")
    }

    pub(crate) fn next_digest(
        &self,
        times: &[String],
        now: DateTime<Utc>,
    ) -> Result<DateTime<Utc>> {
        let zone = Zone::parse(&self.timezone)?;
        let first_day = zone.wall(now).date();
        let mut clocks = times.iter().map(|s| time(s)).collect::<Result<Vec<_>>>()?;
        clocks.sort_unstable();
        clocks.dedup();
        for offset in 0..15 {
            let date = first_day + Duration::days(offset);
            for &clock in &clocks {
                let mut wall = date.and_time(clock);
                if !self.allows_wall(wall) {
                    continue;
                }
                // A skipped hour moves to the first possible minute that day.
                // A repeated hour has only one scheduled digest (earliest offset).
                let candidate = loop {
                    if let Some(value) = zone.resolve(wall).earliest() {
                        break Some(value);
                    }
                    wall += Duration::minutes(1);
                    if wall.date() != date {
                        break None;
                    }
                };
                if let Some(value) = candidate.filter(|t| *t >= now) {
                    return Ok(value);
                }
            }
        }
        bail!("Aucun horaire de récapitulatif dans les plages choisies.")
    }
}
