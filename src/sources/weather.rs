//! Current weather and the next hours from Open-Meteo (free, no key).

use std::time::Duration;

use serde::Deserialize;

use crate::config::Weather as Cfg;

#[derive(Clone, Debug, Default)]
pub struct Weather {
    pub place: String,
    pub temp: f64,
    pub high: f64,
    pub low: f64,
    pub code: i64,
    pub is_day: bool,
    pub error: Option<String>,
}

#[derive(Deserialize)]
struct Geo {
    results: Option<Vec<Place>>,
}

#[derive(Deserialize)]
struct Place {
    name: String,
    latitude: f64,
    longitude: f64,
}

#[derive(Deserialize)]
struct Forecast {
    current: Current,
    daily: Daily,
}

#[derive(Deserialize)]
struct Current {
    temperature_2m: f64,
    weather_code: i64,
    is_day: i64,
}

#[derive(Deserialize)]
struct Daily {
    temperature_2m_max: Vec<f64>,
    temperature_2m_min: Vec<f64>,
}

pub fn watch(cfg: Cfg, tx: async_channel::Sender<Weather>) {
    std::thread::spawn(move || {
        let mut place = None;
        loop {
            if place.is_none() {
                place = locate(&cfg).ok();
            }
            let w = match &place {
                Some((name, lat, lon)) => fetch(&cfg, name, *lat, *lon).unwrap_or_else(|e| Weather { error: Some(e), ..Default::default() }),
                None => Weather { error: Some(format!("Can't find \"{}\"", cfg.location)), ..Default::default() },
            };
            if tx.send_blocking(w).is_err() {
                return;
            }
            std::thread::sleep(Duration::from_secs(15 * 60));
        }
    });
}

fn get<T: for<'de> Deserialize<'de>>(url: &str) -> Result<T, String> {
    ureq::get(url)
        .header("User-Agent", "hyprwidgets/0.1")
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())
        .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
}

fn locate(cfg: &Cfg) -> Result<(String, f64, f64), String> {
    if let (Some(lat), Some(lon)) = (cfg.latitude, cfg.longitude) {
        return Ok((cfg.location.clone(), lat, lon));
    }
    let name: String = cfg.location.chars().map(|c| if c == ' ' { '+' } else { c }).collect();
    let geo: Geo = get(&format!("https://geocoding-api.open-meteo.com/v1/search?count=1&name={name}"))?;
    let p = geo.results.and_then(|r| r.into_iter().next()).ok_or("not found")?;
    Ok((p.name, p.latitude, p.longitude))
}

fn fetch(cfg: &Cfg, place: &str, lat: f64, lon: f64) -> Result<Weather, String> {
    let unit = if cfg.fahrenheit { "&temperature_unit=fahrenheit" } else { "" };
    let f: Forecast = get(&format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}&current=temperature_2m,weather_code,is_day\
         &daily=temperature_2m_max,temperature_2m_min&timezone=auto&forecast_days=2{unit}"
    ))?;
    Ok(Weather {
        place: place.into(),
        temp: f.current.temperature_2m,
        code: f.current.weather_code,
        is_day: f.current.is_day == 1,
        high: f.daily.temperature_2m_max.first().copied().unwrap_or(0.0),
        low: f.daily.temperature_2m_min.first().copied().unwrap_or(0.0),
        error: None,
    })
}

/// WMO weather code -> (symbolic icon name, label).
pub fn describe(code: i64, day: bool) -> (&'static str, &'static str) {
    match code {
        0 if day => ("weather-clear-symbolic", "Clear"),
        0 => ("weather-clear-night-symbolic", "Clear"),
        1 | 2 if day => ("weather-few-clouds-symbolic", "Partly cloudy"),
        1 | 2 => ("weather-few-clouds-night-symbolic", "Partly cloudy"),
        3 => ("weather-overcast-symbolic", "Cloudy"),
        45 | 48 => ("weather-fog-symbolic", "Fog"),
        51..=57 => ("weather-showers-scattered-symbolic", "Drizzle"),
        61..=67 | 80..=82 => ("weather-showers-symbolic", "Rain"),
        71..=77 | 85 | 86 => ("weather-snow-symbolic", "Snow"),
        95..=99 => ("weather-storm-symbolic", "Thunderstorm"),
        _ => ("weather-overcast-symbolic", "—"),
    }
}
