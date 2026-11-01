//! Weather from Open-Meteo (no key): current temperature and condition,
//! today's high / low. One thread fetches on start and every 30 minutes
//! (sleeping in 5-minute steps against the wall clock, so a resume from
//! suspend refreshes promptly). Downloads go through the same capped,
//! https-only `curl` as album art; the JSON is treated as untrusted.
//!
//! Location: `weather = <lat>,<lon>` in ~/.config/nimbus/config, else the
//! city of the system time zone (Europe/Brussels → Brussels), geocoded
//! once through Open-Meteo and cached in the config.

use std::{
    thread,
    time::{Duration, SystemTime},
};

use calloop::channel::Sender;

use crate::services::Fact;

const EVERY: Duration = Duration::from_secs(30 * 60);
const RETRY: Duration = Duration::from_secs(5 * 60);
const STEP: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Debug, PartialEq)]
pub struct Weather {
    /// °C
    pub temp: f32,
    pub hi: f32,
    pub lo: f32,
    /// WMO weather code.
    pub code: u8,
    pub day: bool,
    pub place: String,
}

impl Weather {
    /// Adwaita symbolic icon for the condition.
    pub fn icon(&self) -> &'static str {
        match self.code {
            0 if self.day => "weather-clear-symbolic",
            0 => "weather-clear-night-symbolic",
            1 | 2 if self.day => "weather-few-clouds-symbolic",
            1 | 2 => "weather-few-clouds-night-symbolic",
            3 => "weather-overcast-symbolic",
            45 | 48 => "weather-fog-symbolic",
            51..=57 => "weather-showers-scattered-symbolic",
            61..=67 | 80..=82 => "weather-showers-symbolic",
            71..=77 | 85 | 86 => "weather-snow-symbolic",
            95..=99 => "weather-storm-symbolic",
            _ => "weather-overcast-symbolic",
        }
    }

    pub fn label(&self) -> &'static str {
        match self.code {
            0 => "Clear",
            1 => "Mostly clear",
            2 => "Partly cloudy",
            3 => "Cloudy",
            45 | 48 => "Fog",
            51..=57 => "Drizzle",
            61..=67 => "Rain",
            71..=77 => "Snow",
            80..=82 => "Showers",
            85 | 86 => "Snow showers",
            95..=99 => "Thunderstorm",
            _ => "—",
        }
    }
}

pub fn spawn(tx: Sender<Fact>) {
    let _ = thread::Builder::new().name("nimbus-weather".into()).stack_size(256 * 1024).spawn(move || {
        let mut last: Option<Weather> = None;
        loop {
            let w = location(true).and_then(|(lat, lon, place)| fetch(lat, lon, place));
            let ok = w.is_some();
            if w != last && (ok || last.is_some()) {
                // Keep showing the last reading through a failed refresh.
                if ok {
                    last = w.clone();
                    if tx.send(Fact::Weather(w)).is_err() {
                        return;
                    }
                }
            }
            let wait = if ok { EVERY } else { RETRY };
            let since = SystemTime::now();
            loop {
                thread::sleep(STEP);
                if since.elapsed().map_or(true, |e| e >= wait) {
                    break;
                }
            }
        }
    });
}

/// (lat, lon, place name).
fn location(cache: bool) -> Option<(f64, f64, String)> {
    let place = crate::config::get("weather_place").unwrap_or_default();
    if let Some(v) = crate::config::get("weather")
        && let Some((a, b)) = v.split_once(',')
        && let (Ok(lat), Ok(lon)) = (a.trim().parse::<f64>(), b.trim().parse::<f64>())
        && (-90.0..=90.0).contains(&lat)
        && (-180.0..=180.0).contains(&lon)
    {
        return Some((lat, lon, place));
    }
    let city = tz_city()?;
    let url = format!("https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=en&format=json", urlenc(&city));
    let v: serde_json::Value = serde_json::from_slice(&crate::media::download(&url)?).ok()?;
    let r = v.get("results")?.get(0)?;
    let (lat, lon) = (r.get("latitude")?.as_f64()?, r.get("longitude")?.as_f64()?);
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return None;
    }
    let name: String = r.get("name").and_then(|n| n.as_str()).unwrap_or(&city).chars().take(48).collect();
    if cache {
        crate::config::set("weather", &format!("{lat:.4},{lon:.4}"));
        crate::config::set("weather_place", &name);
    }
    Some((lat, lon, name))
}

/// "Europe/Brussels" → "Brussels", from $TZ or /etc/localtime.
fn tz_city() -> Option<String> {
    let tz = std::env::var("TZ")
        .ok()
        .map(|t| t.trim_start_matches(':').to_string())
        .or_else(|| std::fs::read_link("/etc/localtime").ok().map(|p| p.to_string_lossy().into_owned()))
        .or_else(|| std::fs::read_to_string("/etc/timezone").ok())?;
    let tz = tz.trim();
    let zone = tz.rsplit_once("zoneinfo/").map_or(tz, |(_, z)| z);
    let city = zone.rsplit('/').next()?.replace('_', " ");
    (!city.is_empty() && city.chars().all(|c| c.is_alphabetic() || c == ' ' || c == '-')).then_some(city)
}

fn urlenc(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b == b'-' { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

fn fetch(lat: f64, lon: f64, place: String) -> Option<Weather> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat:.4}&longitude={lon:.4}&current=temperature_2m,weather_code,is_day&daily=temperature_2m_max,temperature_2m_min&timezone=auto&forecast_days=1"
    );
    parse(&crate::media::download(&url)?, place)
}

fn parse(bytes: &[u8], place: String) -> Option<Weather> {
    let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let cur = v.get("current")?;
    let daily = v.get("daily")?;
    let t = |x: Option<&serde_json::Value>| x.and_then(|x| x.as_f64()).filter(|t| t.is_finite() && (-100.0..=100.0).contains(t)).map(|t| t as f32);
    let temp = t(cur.get("temperature_2m"))?;
    let code = cur.get("weather_code")?.as_u64().filter(|c| *c < 100)? as u8;
    let day = cur.get("is_day").and_then(|d| d.as_u64()).unwrap_or(1) == 1;
    let hi = t(daily.get("temperature_2m_max").and_then(|a| a.get(0))).unwrap_or(temp);
    let lo = t(daily.get("temperature_2m_min").and_then(|a| a.get(0))).unwrap_or(temp);
    Some(Weather { temp, hi, lo, code, day, place })
}

/// `nimbus weather-dump`: resolve the location and fetch once (without
/// caching anything in the config).
pub fn dump() {
    match location(false) {
        Some((lat, lon, place)) => {
            println!("location: {place} ({lat:.4}, {lon:.4})");
            match fetch(lat, lon, place) {
                Some(w) => println!("weather: {:.1} °C, {} ({}), high {:.0} / low {:.0}, icon {}", w.temp, w.label(), w.code, w.hi, w.lo, w.icon()),
                None => println!("weather: fetch failed"),
            }
        }
        None => println!("location: unknown (set `weather = <lat>,<lon>` in ~/.config/nimbus/config)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_open_meteo() {
        let j = br#"{"current":{"temperature_2m":13.4,"weather_code":3,"is_day":1},"daily":{"temperature_2m_max":[16.1],"temperature_2m_min":[8.2]}}"#;
        let w = parse(j, "Brussels".into()).unwrap();
        assert_eq!((w.code, w.day), (3, true));
        assert_eq!(w.icon(), "weather-overcast-symbolic");
        assert!((w.hi - 16.1).abs() < 1e-3);
        assert!(parse(br#"{"current":{"temperature_2m":1e9,"weather_code":3}}"#, String::new()).is_none());
        assert!(parse(b"garbage", String::new()).is_none());
    }

    #[test]
    fn url_encoding() {
        assert_eq!(urlenc("Los Angeles"), "Los%20Angeles");
    }
}
