use anyhow::{Result, bail, ensure};
use chrono::{
    DateTime, Datelike, Duration, Local, NaiveDateTime, NaiveTime, TimeZone, Timelike, Utc,
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

    fn first(&self, wall: NaiveDateTime) -> Option<DateTime<Utc>> {
        match self.0 {
            Some(zone) => zone
                .from_local_datetime(&wall)
                .earliest()
                .map(|t| t.to_utc()),
            None => Local
                .from_local_datetime(&wall)
                .earliest()
                .map(|t| t.to_utc()),
        }
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
        // Step through actual UTC minutes, including both occurrences of a folded
        // local hour and skipping the nonexistent minutes of a spring transition.
        let mut minute = now.with_second(0).unwrap().with_nanosecond(0).unwrap();
        for _ in 0..(15 * 24 * 60) {
            minute += Duration::minutes(1);
            if self.allows_wall(zone.wall(minute)) {
                return Ok(minute);
            }
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
                    if let Some(value) = zone.first(wall) {
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
