//! Upcoming events from iCal (.ics) feeds, recurring ones expanded.

use std::collections::HashSet;
use std::io::BufReader;
use std::time::Duration;

use chrono::{DateTime, Datelike, Local, Months, NaiveDate, NaiveDateTime, TimeZone, Utc};
use ical::parser::ical::component::IcalEvent;

use crate::config::{self, Calendar, CalendarSource};
use crate::shared::Shared;

#[derive(Clone, Debug)]
pub struct Event {
    pub title: String,
    pub start: DateTime<Local>,
    pub end: DateTime<Local>,
    pub all_day: bool,
    pub calendar: String,
    pub color: String,
    pub location: String,
}

#[derive(Clone, Default)]
pub struct Agenda {
    pub events: Vec<Event>,
    pub error: Option<String>,
}

impl Event {
    /// Still running or ahead.
    pub fn upcoming(&self, now: &DateTime<Local>) -> bool {
        self.end > *now || (self.all_day && self.end.date_naive() > now.date_naive())
    }

    /// Takes place (at least partly) on `day`.
    pub fn on(&self, day: NaiveDate) -> bool {
        let last = if self.all_day || self.end.time() == chrono::NaiveTime::MIN {
            (self.end - chrono::Duration::seconds(1)).date_naive()
        } else {
            self.end.date_naive()
        };
        self.start.date_naive() <= day && day <= last.max(self.start.date_naive())
    }
}

/// Catppuccin colours, handed out in order to sources without one.
const PALETTE: &[&str] = &["#b4befe", "#f38ba8", "#a6e3a1", "#fab387", "#89dceb", "#cba6f7", "#f9e2af"];

pub fn watch(cfg: Calendar, tx: async_channel::Sender<Agenda>) {
    if cfg.sources.is_empty() {
        let _ = tx.send_blocking(Agenda {
            events: Vec::new(),
            error: Some("Add a calendar in config.toml".into()),
        });
        return;
    }
    std::thread::spawn(move || {
        loop {
            if tx.send_blocking(fetch(&cfg)).is_err() {
                return;
            }
            std::thread::sleep(Duration::from_secs(cfg.refresh.max(1) * 60));
        }
    });
}

thread_local! {
    static SHARED: std::cell::OnceCell<Shared<Agenda>> = const { std::cell::OnceCell::new() };
}

/// The events, fetched once however many widgets show them.
pub fn shared(cfg: &Calendar) -> Shared<Agenda> {
    SHARED.with(|s| {
        s.get_or_init(|| {
            let data = Shared::new();
            let (tx, rx) = async_channel::unbounded();
            watch(cfg.clone(), tx);
            data.feed(rx);
            data
        })
        .clone()
    })
}

/// Everything from the start of last month to three months ahead (or `days`, if further), so the
/// month view can page around. Past events stay in; the agenda drops them itself.
fn fetch(cfg: &Calendar) -> Agenda {
    let now = Local::now();
    let today = now.date_naive();
    let first = today.with_day(1).unwrap();
    let start = first.checked_sub_months(Months::new(1)).unwrap_or(first);
    let end = (first.checked_add_months(Months::new(3)).unwrap_or(first)).max(today + chrono::Duration::days(cfg.days.max(1)));
    let local = |d: NaiveDate| Local.from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap()).earliest().unwrap_or(now);
    let (from, until) = (local(start), local(end));
    let mut events = Vec::new();
    let mut errors = Vec::new();
    for (i, source) in cfg.sources.iter().enumerate() {
        let color = source.color.clone().unwrap_or_else(|| PALETTE[i % PALETTE.len()].to_string());
        match load(source) {
            Ok(text) => events.extend(parse(&text, source, &color, from, until)),
            Err(e) => errors.push(format!("{}: {e}", source.name)),
        }
    }
    events.sort_by_key(|e| (e.start.date_naive(), !e.all_day, e.start));
    Agenda { events, error: errors.first().cloned() }
}

/// The feed text: downloaded, falling back to the last good copy when offline.
fn load(source: &CalendarSource) -> Result<String, String> {
    // iCloud and Outlook hand out webcal:// links, which are plain https
    let url = match source.url.strip_prefix("webcal://") {
        Some(rest) => format!("https://{rest}"),
        None => source.url.clone(),
    };
    if !url.starts_with("http") {
        return std::fs::read_to_string(config::expand(&url)).map_err(|e| e.to_string());
    }
    let cache = config::cache_dir().join(format!("cal-{}.ics", sanitize(&source.name)));
    let fresh = ureq::get(&url)
        .header("User-Agent", "hyprwidgets/0.1")
        .call()
        .map_err(|e| e.to_string())
        .and_then(|mut r| r.body_mut().with_config().limit(32 << 20).read_to_string().map_err(|e| e.to_string()));
    match fresh {
        Ok(text) => {
            let _ = std::fs::write(&cache, &text);
            Ok(text)
        }
        Err(e) => std::fs::read_to_string(&cache).map_err(|_| e),
    }
}

fn sanitize(name: &str) -> String {
    name.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect()
}

fn parse(text: &str, source: &CalendarSource, color: &str, from: DateTime<Local>, until: DateTime<Local>) -> Vec<Event> {
    let mut out = Vec::new();
    let mut vevents = Vec::new();
    for cal in ical::IcalParser::new(BufReader::new(text.as_bytes())).flatten() {
        vevents.extend(cal.events);
    }
    // A RECURRENCE-ID event replaces one instance of its series: skip that instance when expanding.
    let overridden: HashSet<(String, i64)> = vevents
        .iter()
        .filter_map(|e| Some((prop(e, "UID")?.0, when(prop(e, "RECURRENCE-ID")?)?.0.timestamp())))
        .collect();
    for ev in &vevents {
        if prop(ev, "STATUS").is_some_and(|(s, _)| s == "CANCELLED") {
            continue;
        }
        let Some((start, all_day)) = prop(ev, "DTSTART").and_then(when) else { continue };
        let end = prop(ev, "DTEND").and_then(when).map(|(e, _)| e).unwrap_or_else(|| {
            let len = prop(ev, "DURATION").and_then(|(d, _)| duration(&d));
            start + len.unwrap_or(if all_day { chrono::Duration::days(1) } else { chrono::Duration::zero() })
        });
        let length = end - start;
        let title = prop(ev, "SUMMARY").map(|(s, _)| unescape(&s)).unwrap_or_else(|| "Busy".into());
        let location = prop(ev, "LOCATION").map(|(s, _)| unescape(&s)).unwrap_or_default();
        let uid = prop(ev, "UID").map(|(u, _)| u).unwrap_or_default();
        let is_override = prop(ev, "RECURRENCE-ID").is_some();

        let starts = match prop(ev, "RRULE") {
            Some(_) if !is_override => expand(ev, from - length, until).unwrap_or_else(|| vec![start]),
            _ => vec![start],
        };
        for s in starts {
            if !is_override && prop(ev, "RRULE").is_some() && overridden.contains(&(uid.clone(), s.timestamp())) {
                continue;
            }
            let e = s + length;
            if e <= from && !(all_day && e.date_naive() > from.date_naive()) || s >= until {
                continue;
            }
            out.push(Event {
                title: title.clone(),
                start: s,
                // an all-day DTEND is exclusive; the widget wants the last day
                end: if all_day { e - chrono::Duration::seconds(1) } else { e },
                all_day,
                calendar: source.name.clone(),
                color: color.into(),
                location: location.clone(),
            });
        }
    }
    out
}

/// A property's value and its TZID/VALUE parameters, flattened into one string for `when`.
fn prop(ev: &IcalEvent, name: &str) -> Option<(String, String)> {
    let p = ev.properties.iter().find(|p| p.name == name)?;
    let params = p
        .params
        .iter()
        .flatten()
        .map(|(k, v)| format!(";{k}={}", v.join(",")))
        .collect::<String>();
    Some((p.value.clone()?, params))
}

/// DTSTART-style value -> local time and whether it is a whole day.
fn when((value, params): (String, String)) -> Option<(DateTime<Local>, bool)> {
    if params.contains("VALUE=DATE") && !params.contains("DATE-TIME") || value.len() == 8 {
        let d = NaiveDate::parse_from_str(&value, "%Y%m%d").ok()?;
        return Some((Local.from_local_datetime(&d.and_hms_opt(0, 0, 0)?).earliest()?, true));
    }
    if let Some(utc) = value.strip_suffix('Z') {
        let n = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S").ok()?;
        return Some((Utc.from_utc_datetime(&n).with_timezone(&Local), false));
    }
    let n = NaiveDateTime::parse_from_str(&value, "%Y%m%dT%H%M%S").ok()?;
    let tz = params.split(';').find_map(|p| p.strip_prefix("TZID=")).and_then(|t| t.trim_matches('"').parse::<chrono_tz::Tz>().ok());
    let local = match tz {
        Some(tz) => tz.from_local_datetime(&n).earliest()?.with_timezone(&Local),
        None => Local.from_local_datetime(&n).earliest()?,
    };
    Some((local, false))
}

/// Instances of a recurring event between two times, through the rrule crate. The series is
/// expanded in its own wall-clock zone so a weekly 17:00 stays at 17:00 across DST changes; a
/// TZID chrono-tz doesn't know (Windows names) falls back to the local zone.
fn expand(ev: &IcalEvent, from: DateTime<Local>, until: DateTime<Local>) -> Option<Vec<DateTime<Local>>> {
    let (start, all_day) = prop(ev, "DTSTART").and_then(when)?;
    let stamp = |t: DateTime<Local>| t.with_timezone(&Utc).format("%Y%m%dT%H%M%SZ").to_string();
    let tz: chrono_tz::Tz = prop(ev, "DTSTART")
        .and_then(|(_, params)| params.split(';').find_map(|p| p.strip_prefix("TZID=")).map(|t| t.trim_matches('"').to_string()))
        .and_then(|t| t.parse().ok())
        .or_else(|| iana_time_zone::get_timezone().ok()?.parse().ok())
        .unwrap_or(chrono_tz::UTC);
    let wall = |t: DateTime<Local>| t.with_timezone(&tz).format("%Y%m%dT%H%M%S").to_string();
    let mut spec = format!("DTSTART;TZID={}:{}\n", tz.name(), wall(start));
    for p in ev.properties.iter().filter(|p| p.name == "RRULE") {
        let mut rule = p.value.clone()?;
        // UNTIL must be in UTC like DTSTART; a bare date means the end of that day
        if let Some(i) = rule.find("UNTIL=") {
            let rest = &rule[i + 6..];
            let raw = rest.split(';').next().unwrap_or("").to_string();
            let fixed = if raw.len() == 8 {
                let d = NaiveDate::parse_from_str(&raw, "%Y%m%d").ok()?.and_hms_opt(23, 59, 59)?;
                stamp(Local.from_local_datetime(&d).earliest()?)
            } else if !raw.ends_with('Z') {
                stamp(when((raw.clone(), String::new()))?.0)
            } else {
                raw.clone()
            };
            rule = rule.replacen(&format!("UNTIL={raw}"), &format!("UNTIL={fixed}"), 1);
        }
        spec += &format!("RRULE:{rule}\n");
    }
    for p in ev.properties.iter().filter(|p| p.name == "EXDATE") {
        let params = p.params.iter().flatten().map(|(k, v)| format!(";{k}={}", v.join(","))).collect::<String>();
        for v in p.value.as_deref().unwrap_or("").split(',') {
            if let Some((t, _)) = when((v.to_string(), params.clone())) {
                spec += &format!("EXDATE;TZID={}:{}\n", tz.name(), wall(t));
            }
        }
    }
    let set: rrule::RRuleSet = spec.parse().ok()?;
    let rtz = rrule::Tz::Tz(tz);
    let set = set.after(from.with_timezone(&rtz)).before(until.with_timezone(&rtz));
    let result = set.all(500);
    Some(
        result
            .dates
            .into_iter()
            .map(|d| {
                let local = d.with_timezone(&Local);
                // keep whole-day events on their wall-clock midnight across DST changes
                if all_day { Local.from_local_datetime(&local.date_naive().and_hms_opt(0, 0, 0).unwrap()).earliest().unwrap_or(local) } else { local }
            })
            .collect(),
    )
}

/// ISO 8601 durations as used by DURATION: P1D, PT1H30M, P2W.
fn duration(s: &str) -> Option<chrono::Duration> {
    let s = s.trim_start_matches(['+', 'P']);
    let (mut total, mut num) = (0i64, String::new());
    for c in s.chars() {
        match c {
            '0'..='9' => num.push(c),
            'T' => {}
            'W' | 'D' | 'H' | 'M' | 'S' => {
                let n: i64 = num.parse().ok()?;
                num.clear();
                total += n * match c {
                    'W' => 604800,
                    'D' => 86400,
                    'H' => 3600,
                    'M' => 60,
                    _ => 1,
                };
            }
            _ => return None,
        }
    }
    Some(chrono::Duration::seconds(total))
}

fn unescape(s: &str) -> String {
    s.replace("\\n", " ").replace("\\,", ",").replace("\\;", ";").replace("\\\\", "\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src() -> CalendarSource {
        CalendarSource { name: "t".into(), url: String::new(), color: None }
    }

    #[test]
    fn weekly_with_exdate_and_override() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nDTSTART;TZID=Europe/Zurich:20260105T090000\r\nDTEND;TZID=Europe/Zurich:20260105T100000\r\nRRULE:FREQ=WEEKLY;BYDAY=MO\r\nEXDATE;TZID=Europe/Zurich:20260112T090000\r\nSUMMARY:Standup\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:a\r\nRECURRENCE-ID;TZID=Europe/Zurich:20260119T090000\r\nDTSTART;TZID=Europe/Zurich:20260119T110000\r\nDTEND;TZID=Europe/Zurich:20260119T120000\r\nSUMMARY:Standup moved\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let from = Local.with_ymd_and_hms(2026, 1, 5, 0, 0, 0).unwrap();
        let until = Local.with_ymd_and_hms(2026, 1, 27, 0, 0, 0).unwrap();
        let mut evs = parse(ics, &src(), "#fff", from, until);
        evs.sort_by_key(|e| e.start);
        let titles: Vec<_> = evs.iter().map(|e| (e.start.format("%m-%d").to_string(), e.title.as_str())).collect();
        assert_eq!(titles, [("01-05".into(), "Standup"), ("01-19".into(), "Standup moved"), ("01-26".into(), "Standup")]);
    }

    #[test]
    fn weekly_keeps_wall_clock_across_dst() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:c\r\nDTSTART;TZID=Europe/Zurich:20260901T170000\r\nDTEND;TZID=Europe/Zurich:20260901T180000\r\nRRULE:FREQ=WEEKLY\r\nSUMMARY:Gym\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let zurich = chrono_tz::Europe::Zurich;
        let from = zurich.with_ymd_and_hms(2026, 11, 3, 0, 0, 0).unwrap().with_timezone(&Local);
        let evs = parse(ics, &src(), "#fff", from, from + chrono::Duration::days(1));
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].start.with_timezone(&zurich).format("%H:%M").to_string(), "17:00");
    }

    #[test]
    fn all_day_spans() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:b\r\nDTSTART;VALUE=DATE:20260110\r\nDTEND;VALUE=DATE:20260112\r\nSUMMARY:Trip\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let from = Local.with_ymd_and_hms(2026, 1, 11, 0, 0, 0).unwrap();
        let until = from + chrono::Duration::days(3);
        let evs = parse(ics, &src(), "#fff", from, until);
        assert_eq!(evs.len(), 1);
        assert!(evs[0].all_day);
        assert_eq!(evs[0].end.date_naive(), NaiveDate::from_ymd_opt(2026, 1, 11).unwrap());
    }

    #[test]
    fn durations() {
        assert_eq!(duration("PT1H30M"), Some(chrono::Duration::minutes(90)));
        assert_eq!(duration("P1D"), Some(chrono::Duration::days(1)));
    }
}
