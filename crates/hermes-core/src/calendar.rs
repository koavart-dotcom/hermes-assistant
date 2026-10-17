//! Today's events from Google Calendar for the morning briefing. Read-only,
//! through the calendar's secret iCal address (Google Calendar → Settings →
//! the calendar → «Секретный адрес в формате iCal») kept in Hermes' `.env` as
//! `GOOGLE_CALENDAR_ICS_URL` (several addresses — comma-separated). No OAuth:
//! the address alone grants read access, so it is masked like any key.
//! Recurring events are expanded with `rrule` (EXDATE and moved instances —
//! `RECURRENCE-ID` — included).

use std::collections::HashSet;

use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::{CoreError, Layout, Result};

pub const ENV_URL: &str = "GOOGLE_CALENDAR_ICS_URL";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CalendarEvent {
    /// Local `HH:MM`; empty for all-day events.
    pub start: String,
    pub end: String,
    pub summary: String,
    pub all_day: bool,
}

pub fn urls(layout: &Layout) -> Vec<String> {
    crate::envkeys::value(layout, ENV_URL)
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Events of `day` (local time) from all configured calendars, sorted by start.
pub async fn events_on(urls: &[String], day: NaiveDate) -> Result<Vec<CalendarEvent>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .expect("reqwest client");
    let mut all = Vec::new();
    for url in urls {
        let resp = client
            .get(url)
            .send()
            .await
            .map_err(|e| CoreError::Other(format!("календарь: {e}")))?;
        if !resp.status().is_success() {
            return Err(CoreError::Other(format!(
                "календарь: HTTP {} (проверьте секретный адрес iCal)",
                resp.status()
            )));
        }
        let text = resp
            .text()
            .await
            .map_err(|e| CoreError::Other(format!("календарь: {e}")))?;
        all.extend(parse_day(&text, day, &Local));
    }
    all.sort_by(|a, b| (!a.all_day, &a.start).cmp(&(!b.all_day, &b.start)));
    all.dedup();
    Ok(all)
}

/// One property line: `NAME;PARAM=..:value` → (name, params, value).
struct Prop {
    name: String,
    params: String,
    value: String,
    /// The original line (used verbatim for the rrule parser).
    raw: String,
}

fn unfold(ics: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in ics.lines() {
        let line = line.trim_end_matches('\r');
        if (line.starts_with(' ') || line.starts_with('\t')) && !out.is_empty() {
            out.last_mut().unwrap().push_str(&line[1..]);
        } else {
            out.push(line.to_string());
        }
    }
    out
}

fn prop(line: &str) -> Option<Prop> {
    let colon = line.find(':')?;
    let (head, value) = (&line[..colon], &line[colon + 1..]);
    let (name, params) = match head.find(';') {
        Some(i) => (&head[..i], &head[i + 1..]),
        None => (head, ""),
    };
    Some(Prop {
        name: name.to_ascii_uppercase(),
        params: params.to_string(),
        value: value.to_string(),
        raw: line.to_string(),
    })
}

fn param<'a>(params: &'a str, key: &str) -> Option<&'a str> {
    params.split(';').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        k.eq_ignore_ascii_case(key).then(|| v.trim_matches('"'))
    })
}

fn unescape(s: &str) -> String {
    s.replace("\\n", " ")
        .replace("\\N", " ")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
}

/// A DTSTART/DTEND/RECURRENCE-ID value as an instant; `true` when it is a date.
fn instant<Z: TimeZone>(p: &Prop, tz: &Z) -> Option<(DateTime<Utc>, bool)> {
    let v = p.value.trim();
    if param(&p.params, "VALUE") == Some("DATE") || v.len() == 8 {
        let d = NaiveDate::parse_from_str(v, "%Y%m%d").ok()?;
        let local = tz
            .from_local_datetime(&d.and_hms_opt(0, 0, 0)?)
            .earliest()?;
        return Some((local.with_timezone(&Utc), true));
    }
    if let Some(stripped) = v.strip_suffix('Z') {
        let n = NaiveDateTime::parse_from_str(stripped, "%Y%m%dT%H%M%S").ok()?;
        return Some((Utc.from_utc_datetime(&n), false));
    }
    let n = NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M%S").ok()?;
    let dt = match param(&p.params, "TZID").and_then(|t| t.parse::<chrono_tz::Tz>().ok()) {
        Some(tz) => tz.from_local_datetime(&n).earliest()?.with_timezone(&Utc),
        None => tz.from_local_datetime(&n).earliest()?.with_timezone(&Utc),
    };
    Some((dt, false))
}

struct VEvent {
    uid: String,
    summary: String,
    start: Option<Prop>,
    end: Option<Prop>,
    duration: Option<String>,
    rules: Vec<String>,
    recurrence_id: Option<Prop>,
    cancelled: bool,
}

fn events(ics: &str) -> Vec<VEvent> {
    let mut out = Vec::new();
    let mut cur: Option<VEvent> = None;
    for line in unfold(ics) {
        if line.eq_ignore_ascii_case("BEGIN:VEVENT") {
            cur = Some(VEvent {
                uid: String::new(),
                summary: String::new(),
                start: None,
                end: None,
                duration: None,
                rules: Vec::new(),
                recurrence_id: None,
                cancelled: false,
            });
            continue;
        }
        if line.eq_ignore_ascii_case("END:VEVENT") {
            out.extend(cur.take());
            continue;
        }
        let (Some(ev), Some(p)) = (cur.as_mut(), prop(&line)) else {
            continue;
        };
        match p.name.as_str() {
            "UID" => ev.uid = p.value.clone(),
            "SUMMARY" => ev.summary = unescape(&p.value),
            "DTSTART" => ev.start = Some(p),
            "DTEND" => ev.end = Some(p),
            "DURATION" => ev.duration = Some(p.value.clone()),
            "RRULE" | "EXDATE" | "RDATE" => ev.rules.push(p.raw.clone()),
            "RECURRENCE-ID" => ev.recurrence_id = Some(p),
            "STATUS" => ev.cancelled = p.value.eq_ignore_ascii_case("CANCELLED"),
            _ => {}
        }
    }
    out
}

/// `PT1H30M`, `P1D` → duration (only what Google writes).
fn iso_duration(s: &str) -> Option<Duration> {
    let s = s.strip_prefix('P')?;
    let (days, time) = s.split_once('T').unwrap_or((s, ""));
    let mut total = Duration::zero();
    let mut num = String::new();
    for c in days.chars().chain(['|']).chain(time.chars()) {
        match c {
            '0'..='9' => num.push(c),
            'W' => total += Duration::weeks(num.parse().ok()?),
            'D' => total += Duration::days(num.parse().ok()?),
            'H' => total += Duration::hours(num.parse().ok()?),
            'M' => total += Duration::minutes(num.parse().ok()?),
            'S' => total += Duration::seconds(num.parse().ok()?),
            _ => {}
        }
        if c.is_ascii_alphabetic() {
            num.clear();
        }
    }
    Some(total)
}

/// Events overlapping `day` in time zone `tz` (local time on the PC).
pub fn parse_day<Z: TimeZone>(ics: &str, day: NaiveDate, tz: &Z) -> Vec<CalendarEvent> {
    let day_start = tz
        .from_local_datetime(&day.and_hms_opt(0, 0, 0).unwrap())
        .earliest()
        .map(|d| d.with_timezone(&Utc));
    let Some(day_start) = day_start else {
        return Vec::new();
    };
    let day_end = day_start + Duration::days(1);
    let all = events(ics);

    // Moved or cancelled instances of recurring events: (uid, original start).
    let overridden: HashSet<(String, DateTime<Utc>)> = all
        .iter()
        .filter_map(|e| {
            let (t, _) = instant(e.recurrence_id.as_ref()?, tz)?;
            Some((e.uid.clone(), t))
        })
        .collect();

    let mut out = Vec::new();
    let mut seen: HashSet<(String, DateTime<Utc>)> = HashSet::new();
    for e in &all {
        if e.cancelled {
            continue;
        }
        let Some(sp) = &e.start else { continue };
        let Some((start, all_day)) = instant(sp, tz) else {
            continue;
        };
        let length = match (&e.end, &e.duration) {
            (Some(ep), _) => instant(ep, tz).map(|(t, _)| t - start),
            (None, Some(d)) => iso_duration(d),
            _ => None,
        }
        .unwrap_or(if all_day {
            Duration::days(1)
        } else {
            Duration::zero()
        });

        let starts: Vec<DateTime<Utc>> = if e
            .rules
            .iter()
            .any(|r| r.to_ascii_uppercase().starts_with("RRULE"))
        {
            match expand(sp, &e.rules, day_start - length, day_end) {
                Some(v) => v,
                None => {
                    log::warn!("calendar: cannot expand `{}`", e.summary);
                    continue;
                }
            }
            .into_iter()
            .filter(|t| !overridden.contains(&(e.uid.clone(), *t)))
            .collect()
        } else {
            vec![start]
        };

        for s in starts {
            let end = s + length;
            let overlaps = if all_day {
                s < day_end && end > day_start
            } else {
                s < day_end && (end > day_start || s >= day_start)
            };
            if !overlaps || !seen.insert((e.uid.clone(), s)) {
                continue;
            }
            let hm = |t: DateTime<Utc>| {
                t.with_timezone(tz)
                    .naive_local()
                    .format("%H:%M")
                    .to_string()
            };
            out.push(CalendarEvent {
                start: if all_day { String::new() } else { hm(s) },
                end: if all_day { String::new() } else { hm(end) },
                summary: if e.summary.is_empty() {
                    "(без названия)".into()
                } else {
                    e.summary.clone()
                },
                all_day,
            });
        }
    }
    out.sort_by(|a, b| (!a.all_day, &a.start).cmp(&(!b.all_day, &b.start)));
    out
}

fn expand(
    start: &Prop,
    rules: &[String],
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Option<Vec<DateTime<Utc>>> {
    let mut text = start.raw.clone();
    for r in rules {
        text.push('\n');
        text.push_str(r);
    }
    let set: rrule::RRuleSet = text.parse().ok()?;
    let tz = rrule::Tz::UTC;
    let res = set
        .after((from - Duration::seconds(1)).with_timezone(&tz))
        .before(to.with_timezone(&tz))
        .all(100);
    Some(
        res.dates
            .into_iter()
            .map(|d| d.with_timezone(&Utc))
            .collect(),
    )
}

/// «10:00 Планёрка», «весь день — Отпуск».
pub fn line(e: &CalendarEvent) -> String {
    if e.all_day {
        format!("• весь день — {}", e.summary)
    } else {
        format!("• {}–{} {}", e.start, e.end, e.summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICS: &str = "BEGIN:VCALENDAR\r
BEGIN:VEVENT\r
UID:once\r
DTSTART:20261019T070000Z\r
DTEND:20261019T080000Z\r
SUMMARY:Созвон с\r
  Deka\\, бэкенд\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:weekly\r
DTSTART;TZID=Europe/Kyiv:20261005T093000\r
DTEND;TZID=Europe/Kyiv:20261005T100000\r
RRULE:FREQ=WEEKLY;BYDAY=MO\r
EXDATE;TZID=Europe/Kyiv:20261012T093000\r
SUMMARY:Планёрка\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:weekly\r
RECURRENCE-ID;TZID=Europe/Kyiv:20261026T093000\r
DTSTART;TZID=Europe/Kyiv:20261026T120000\r
DTEND;TZID=Europe/Kyiv:20261026T123000\r
SUMMARY:Планёрка (перенесена)\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:allday\r
DTSTART;VALUE=DATE:20261019\r
DTEND;VALUE=DATE:20261020\r
SUMMARY:День рождения\r
END:VEVENT\r
BEGIN:VEVENT\r
UID:gone\r
DTSTART:20261019T100000Z\r
DTEND:20261019T110000Z\r
STATUS:CANCELLED\r
SUMMARY:Отменено\r
END:VEVENT\r
END:VCALENDAR\r
";

    fn kyiv() -> chrono_tz::Tz {
        chrono_tz::Europe::Kyiv
    }

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn single_recurring_and_cancelled() {
        let ev = parse_day(ICS, day(2026, 10, 19), &kyiv());
        let names: Vec<String> = ev
            .iter()
            .map(|e| format!("{} {}", e.start, e.summary))
            .collect();
        assert_eq!(
            names,
            vec![
                " День рождения".to_string(),
                "09:30 Планёрка".to_string(),
                "10:00 Созвон с Deka, бэкенд".to_string(),
            ]
        );
        assert_eq!(ev[2].end, "11:00");
    }

    #[test]
    fn exdate_skips_instance() {
        let ev = parse_day(ICS, day(2026, 10, 12), &kyiv());
        assert!(ev.iter().all(|e| e.summary != "Планёрка"), "{ev:?}");
    }

    #[test]
    fn moved_instance_replaces_original() {
        let ev = parse_day(ICS, day(2026, 10, 26), &kyiv());
        let names: Vec<&str> = ev.iter().map(|e| e.summary.as_str()).collect();
        assert_eq!(names, vec!["Планёрка (перенесена)"]);
        assert_eq!(ev[0].start, "12:00");
    }

    #[test]
    fn durations() {
        assert_eq!(iso_duration("PT1H30M"), Some(Duration::minutes(90)));
        assert_eq!(iso_duration("P1D"), Some(Duration::days(1)));
    }
}
