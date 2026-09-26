use crate::{
    config::{Config, Preferences, atomic_write},
    model::*,
};
use anyhow::{Context, Result, anyhow, ensure};
use chrono::{Datelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, BinaryHeap},
    fs,
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

#[derive(Clone)]
pub struct Client {
    pub(crate) generation: Option<(Arc<std::sync::atomic::AtomicU64>, u64)>,
    pub cache: PathBuf,
    pub offline: bool,
    agent: ureq::Agent,
    cache_lock: Arc<std::sync::Mutex<()>>,
    network_lock: Arc<std::sync::Mutex<()>>,
}
#[derive(Serialize, Deserialize)]
struct Cached<T = Value> {
    at: i64,
    data: T,
}
pub const WEATHER_TTL: i64 = 30 * 60;
pub const HISTORY_TTL: i64 = 180 * 24 * 60 * 60;

/// Earliest expiry among displayed sources; failed sources use the retry deadline.
pub fn refresh_wait(report: &Report, now: i64, retry_after: Duration) -> Duration {
    if report.period == Period::Recent
        && chrono::DateTime::from_timestamp(now, 0).is_some_and(|t| {
            report.cities.iter().any(|r| {
                r.baseline
                    .as_ref()
                    .is_some_and(|b| b.end_year < t.year() - 1)
            })
        })
    {
        return Duration::ZERO;
    }
    let ttl = if report.mode == Mode::Historical {
        HISTORY_TTL
    } else {
        WEATHER_TTL
    };
    report
        .cities
        .iter()
        .flat_map(|row| {
            let ttl = if report.period == Period::Future {
                HISTORY_TTL
            } else if report.period != Period::Now {
                i64::MAX / 2
            } else {
                ttl
            };
            let retry_after = row.retry_at.map_or(retry_after, |at| {
                Duration::from_secs(at.saturating_sub(now).max(0) as u64)
            });
            let missing = row.provenance.is_empty().then_some(retry_after);
            let base_wait = row
                .comparison_base
                .as_ref()
                .filter(|_| report.mode == Mode::Comparison)
                .and_then(|base| {
                    let retry_after = base.retry_at.map_or(retry_after, |at| {
                        Duration::from_secs(at.saturating_sub(now).max(0) as u64)
                    });
                    base.provenance
                        .values()
                        .map(|meta| {
                            if meta.stale || meta.warning.is_some() {
                                retry_after
                            } else {
                                let ttl = if base.baseline.is_none() {
                                    WEATHER_TTL
                                } else {
                                    i64::MAX / 2
                                };
                                Duration::from_secs(
                                    meta.fetched_at
                                        .saturating_add(ttl)
                                        .saturating_sub(now)
                                        .max(0) as u64,
                                )
                            }
                        })
                        .chain(
                            (base.error.is_some() || base.retry_at.is_some())
                                .then_some(retry_after),
                        )
                        .min()
                });
            row.provenance
                .values()
                .map(move |meta| {
                    if meta.stale || meta.warning.is_some() {
                        retry_after
                    } else {
                        Duration::from_secs(
                            meta.fetched_at
                                .saturating_add(ttl)
                                .saturating_sub(now)
                                .max(0) as u64,
                        )
                    }
                })
                .chain(missing)
                .chain(base_wait)
                .chain(row.retry_at.map(|_| retry_after))
        })
        .min()
        .unwrap_or(retry_after)
}
impl Client {
    pub fn new(cache: PathBuf, offline: bool) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(12)))
            .build();
        Self {
            generation: None,
            cache,
            offline,
            agent: config.into(),
            cache_lock: Default::default(),
            network_lock: Default::default(),
        }
    }
    pub fn get(
        &self,
        endpoint: &str,
        params: &[(&str, String)],
        ttl: i64,
        refresh: bool,
    ) -> Result<(Value, Provenance)> {
        self.get_inner(endpoint, params, ttl, refresh, true)
    }
    pub(crate) fn transient(
        &self,
        endpoint: &str,
        params: &[(&str, String)],
    ) -> Result<(Value, Provenance)> {
        self.get_inner(endpoint, params, 0, true, false)
    }
    fn get_inner(
        &self,
        endpoint: &str,
        params: &[(&str, String)],
        ttl: i64,
        refresh: bool,
        persist: bool,
    ) -> Result<(Value, Provenance)> {
        let mut url = url::Url::parse(endpoint)?;
        if !params.is_empty() {
            url.query_pairs_mut()
                .extend_pairs(params.iter().map(|(k, v)| (*k, v.as_str())));
        }
        let file = self.cache.join(format!(
            "{:x}.json",
            Sha256::digest(url.as_str().as_bytes())
        ));
        let saved = fs::metadata(&file)
            .ok()
            .filter(|m| m.len() <= 33 * 1024 * 1024)
            .and_then(|_| fs::read(&file).ok())
            .and_then(|b| serde_json::from_slice::<Cached>(&b).ok())
            .filter(|s| s.data.is_object());
        let now = Utc::now().timestamp();
        if let Some(s) = &saved
            && (self.offline || (!refresh && (0..ttl.max(WEATHER_TTL)).contains(&(now - s.at))))
        {
            let provenance = Provenance {
                retry_at: None,
                cached: true,
                stale: !(0..ttl.max(WEATHER_TTL)).contains(&(now - s.at)),
                fetched_at: s.at,
                warning: None,
            };
            return Ok((saved.expect("cache entry checked above").data, provenance));
        }
        let fetch = || -> Result<Value> {
            let _network = self.network_lock.lock().unwrap_or_else(|e| e.into_inner());
            let now = Utc::now().timestamp();
            ensure!(
                self.generation
                    .as_ref()
                    .is_none_or(|(current, expected)| current.load(Ordering::Relaxed) == *expected),
                "Superseded request"
            );
            ensure!(!self.offline, "Offline: no cached response");
            let archive = matches!(
                url.host_str(),
                Some("archive-api.open-meteo.com" | "climate-api.open-meteo.com")
            );
            let budget_file = self.cache.join("limits/archive.json");
            let meteo = url
                .host_str()
                .is_some_and(|h| h == "open-meteo.com" || h.ends_with(".open-meteo.com"));
            if meteo {
                let _guard = self.cache_lock.lock().unwrap_or_else(|e| e.into_inner());
                let mut budget = fs::read(&budget_file)
                    .ok()
                    .and_then(|b| serde_json::from_slice::<crate::rate_limit::Budget>(&b).ok())
                    .unwrap_or_default();
                budget.reserve_with_priority(now, crate::rate_limit::weight(params), !archive)?;
                atomic_write(&budget_file, &serde_json::to_vec(&budget)?)?;
            }
            let settle = |charged: f64| -> Result<()> {
                if meteo {
                    let _guard = self.cache_lock.lock().unwrap_or_else(|e| e.into_inner());
                    let mut budget: crate::rate_limit::Budget =
                        serde_json::from_slice(&fs::read(&budget_file)?)?;
                    budget.settle(now, crate::rate_limit::weight(params), charged);
                    atomic_write(&budget_file, &serde_json::to_vec(&budget)?)?;
                }
                Ok(())
            };
            let response = self
                .agent
                .get(url.as_str())
                .header(
                    "User-Agent",
                    concat!(
                        "nubila/",
                        env!("CARGO_PKG_VERSION"),
                        " (personal weather client)"
                    ),
                )
                .config()
                // A failed redirected connection does not prove the initial
                // provider request was unsent. These fixed API URLs need none.
                .max_redirects(if meteo { 0 } else { 10 })
                .http_status_as_error(false)
                .build()
                .call();
            let mut response = match response {
                Ok(response) => response,
                Err(error) => {
                    if crate::rate_limit::unsent(&error) {
                        settle(0.0).context("Release unsent request quota reservation")?;
                    }
                    return Err(error.into());
                }
            };
            let status = response.status();
            let retry_header = response
                .headers()
                .get("retry-after")
                .and_then(|h| h.to_str().ok())
                .map(str::to_owned);
            let data = response
                .body_mut()
                .with_config()
                .limit(32 * 1024 * 1024)
                .read_json::<Value>();
            if status.as_u16() == 429 {
                let data = data.unwrap_or_default();
                let reason = data["reason"].as_str().unwrap_or("API rate limit exceeded");
                let until = crate::rate_limit::retry_deadline(
                    Utc::now().timestamp(),
                    retry_header.as_deref(),
                    reason,
                );
                if meteo {
                    let _guard = self.cache_lock.lock().unwrap_or_else(|e| e.into_inner());
                    let mut budget = fs::read(&budget_file)
                        .ok()
                        .and_then(|b| serde_json::from_slice::<crate::rate_limit::Budget>(&b).ok())
                        .unwrap_or_default();
                    budget.blocked_until = budget.blocked_until.max(until);
                    // Upstream rejects before incrementing usage. Do not count
                    // failed reservations as successfully downloaded data.
                    budget.rejected(now, crate::rate_limit::weight(params));
                    atomic_write(&budget_file, &serde_json::to_vec(&budget)?)?;
                }
                return Err(crate::rate_limit::Deferred {
                    until,
                    reason: format!("HTTP 429: {reason}"),
                }
                .into());
            }
            if data
                .as_ref()
                .ok()
                .is_some_and(|data| provider_error_cost(status.as_u16(), data).is_some())
            {
                // Upstream's withFreeApiRateLimiter charges one credit when
                // the API handler throws. Proxy errors are not proof of that.
                settle(1.0).context("Reconcile rejected request quota cost")?;
            }
            let data = data.with_context(|| format!("HTTP {status}: invalid JSON response"))?;
            ensure!(
                status.is_success(),
                "HTTP {status}: {}",
                data.get("reason").unwrap_or(&data)
            );
            ensure!(data.is_object(), "Provider returned invalid JSON structure");
            ensure!(
                data.get("error").and_then(Value::as_bool) != Some(true),
                "Provider: {}",
                data.get("reason").unwrap_or(&data)
            );
            Ok(data)
        };
        match fetch() {
            Ok(data) => {
                if !persist {
                    return Ok((
                        data,
                        Provenance {
                            fetched_at: now,
                            ..Default::default()
                        },
                    ));
                }
                let bytes = serde_json::to_vec(&Cached {
                    at: now,
                    data: &data,
                })?;
                let _cache_guard = self.cache_lock.lock().unwrap_or_else(|e| e.into_inner());
                let warning = atomic_write(&file, &bytes)
                    .err()
                    .map(|e| format!("Cache write failed: {e:#}"));
                let _ = tapp_ui::storage::prune_cache(&self.cache, "json", 512, 128 * 1024 * 1024);
                Ok((
                    data,
                    Provenance {
                        retry_at: None,
                        cached: false,
                        stale: false,
                        fetched_at: now,
                        warning,
                    },
                ))
            }
            Err(error) => match saved {
                Some(s) => Ok((
                    s.data,
                    Provenance {
                        cached: true,
                        stale: true,
                        fetched_at: s.at,
                        warning: Some(format!("{error:#}")),
                        retry_at: error
                            .downcast_ref::<crate::rate_limit::Deferred>()
                            .map(|e| e.until),
                    },
                )),
                None => Err(error),
            },
        }
    }
    pub fn search(&self, name: &str) -> Result<Value> {
        let (v, _) = self.get(
            "https://geocoding-api.open-meteo.com/v1/search",
            &[
                ("name", name.into()),
                ("count", "10".into()),
                ("language", "en".into()),
                ("format", "json".into()),
            ],
            2_592_000,
            false,
        )?;
        Ok(v.get("results").cloned().unwrap_or(serde_json::json!([])))
    }
    pub fn detect(&self) -> Result<(City, Provenance)> {
        let (v, meta) = self.get("https://ipapi.co/json/", &[], 86400, false)?;
        let city = City {
            id: "__current__".into(),
            name: v["city"].as_str().unwrap_or("Current location").into(),
            latitude: v["latitude"]
                .as_f64()
                .context("Location missing latitude")?,
            longitude: v["longitude"]
                .as_f64()
                .context("Location missing longitude")?,
        };
        city.validate()?;
        Ok((city, meta))
    }
}
pub fn merge_current(cities: &mut Vec<City>, mut current: City) -> String {
    for city in cities.iter() {
        let a = city.latitude.to_radians();
        let b = current.latitude.to_radians();
        let d = ((b - a) / 2.0).sin().powi(2)
            + a.cos()
                * b.cos()
                * ((current.longitude - city.longitude).to_radians() / 2.0)
                    .sin()
                    .powi(2);
        if 6371.0 * 2.0 * d.sqrt().min(1.0).asin() <= 25.0 {
            return city.id.clone();
        }
    }
    while cities.iter().any(|c| c.id == current.id) {
        current.id.push('_');
    }
    let id = current.id.clone();
    cities.push(current);
    id
}
fn number(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite())
}
fn liquid_rain(rain: &Value, showers: &Value) -> Option<f64> {
    number(rain).zip(number(showers)).map(|(a, b)| a + b)
}
fn coords(city: &City) -> Vec<(&'static str, String)> {
    vec![
        ("latitude", city.latitude.to_string()),
        ("longitude", city.longitude.to_string()),
    ]
}
fn forecast(
    client: &Client,
    city: &City,
    source: Source,
    refresh: bool,
    past_days: u32,
) -> Result<Weather> {
    let fields = FIELDS
        .iter()
        .map(|f| f.0)
        .chain(["showers"])
        .collect::<Vec<_>>()
        .join(",");
    let mut params = coords(city);
    params.extend([
        ("timezone", "auto".into()),
        ("timeformat", "unixtime".into()),
        ("current", format!("{fields},weather_code,is_day")),
        ("hourly", format!("{fields},weather_code,is_day")),
        ("daily", "weather_code,apparent_temperature_mean,apparent_temperature_min,apparent_temperature_max,temperature_2m_mean,temperature_2m_min,temperature_2m_max,precipitation_sum,rain_sum,showers_sum,snowfall_sum,wind_speed_10m_max,cloud_cover_mean,relative_humidity_2m_mean,surface_pressure_mean".into()),
        ("forecast_days", "16".into()),
        ("past_days", past_days.to_string()),
    ]);
    match source {
        Source::Gfs => params.push(("models", "gfs_seamless".into())),
        Source::Icon => params.push(("models", "icon_seamless".into())),
        _ => {}
    }
    let (v, meta) = client.get(
        "https://api.open-meteo.com/v1/forecast",
        &params,
        WEATHER_TTL,
        refresh,
    )?;
    let mut weather = parse_forecast(city, &v, Utc::now())?;
    for condition in weather
        .condition
        .iter_mut()
        .chain(
            weather
                .hourly
                .iter_mut()
                .filter_map(|h| h.condition.as_mut()),
        )
        .chain(
            weather
                .day_hourly
                .iter_mut()
                .filter_map(|h| h.condition.as_mut()),
        )
        .chain(
            weather
                .daily
                .iter_mut()
                .filter_map(|d| d.condition.as_mut()),
        )
    {
        condition.source = source.label().into();
    }
    weather.sources.push(source.label().into());
    weather.provenance.insert(source.label().into(), meta);
    Ok(weather)
}
pub fn hourly_ranges(hours: &[Hour]) -> Ranges {
    FIELDS
        .iter()
        .map(|(key, _)| {
            let values: Vec<_> = hours
                .iter()
                .filter_map(|h| h.values.get(*key).copied().flatten())
                .filter(|v| v.is_finite())
                .collect();
            (
                (*key).into(),
                Range {
                    min: values.iter().copied().reduce(f64::min),
                    max: values.iter().copied().reduce(f64::max),
                    samples: values.len(),
                },
            )
        })
        .collect()
}
pub fn parse_forecast(city: &City, v: &Value, now: chrono::DateTime<Utc>) -> Result<Weather> {
    let current = v.get("current").context("Missing current forecast")?;
    let stamp = current["time"]
        .as_i64()
        .context("Missing forecast timestamp")?;
    let timezone: chrono_tz::Tz = v["timezone"]
        .as_str()
        .context("Missing forecast timezone")?
        .parse()?;
    let today = now.with_timezone(&timezone).date_naive();
    let times = v["hourly"]["time"]
        .as_array()
        .context("Missing hourly forecast")?;
    let mut weather = Weather::empty(city.clone());
    weather.time = chrono::DateTime::from_timestamp(stamp, 0)
        .context("Invalid current timestamp")?
        .format("%Y-%m-%dT%H:%M")
        .to_string();
    weather.range_date = today.to_string();
    weather.range_timezone = timezone.to_string();
    weather.condition = condition(&current["weather_code"], &current["is_day"]);
    weather.values = FIELDS
        .iter()
        .map(|(k, _)| {
            (
                (*k).into(),
                if *k == "rain" {
                    liquid_rain(&current[k], &current["showers"])
                } else {
                    number(&current[k])
                },
            )
        })
        .collect();
    for (i, t) in times.iter().enumerate() {
        if let Some(time) = t
            .as_i64()
            .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
        {
            let hour = Hour {
                time: time.format("%Y-%m-%dT%H:%M").to_string(),
                values: FIELDS
                    .iter()
                    .map(|(k, _)| {
                        (
                            (*k).into(),
                            if *k == "rain" {
                                liquid_rain(&v["hourly"][k][i], &v["hourly"]["showers"][i])
                            } else {
                                number(&v["hourly"][k][i])
                            },
                        )
                    })
                    .collect(),
                contributors: BTreeMap::new(),
                condition: condition(&v["hourly"]["weather_code"][i], &v["hourly"]["is_day"][i]),
            };
            if time.with_timezone(&timezone).date_naive() == today {
                weather.day_hourly.push(hour.clone());
            }
            if hour.condition.is_some() || hour.values.values().any(Option::is_some) {
                weather.hourly.push(hour);
            }
        }
    }
    if let Some(times) = v["daily"]["time"].as_array() {
        for (i, t) in times.iter().enumerate() {
            let Some(time) = t
                .as_i64()
                .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
            else {
                continue;
            };
            let date = time.with_timezone(&timezone).date_naive();
            if date == today {
                let d = &v["daily"];
                weather.daily_totals = Values::from([
                    ("precipitation".into(), number(&d["precipitation_sum"][i])),
                    (
                        "rain".into(),
                        liquid_rain(&d["rain_sum"][i], &d["showers_sum"][i]),
                    ),
                    ("snowfall".into(), number(&d["snowfall_sum"][i])),
                ]);
            }
            if date <= today {
                continue;
            }
            let d = &v["daily"];
            let day = Day {
                solar: Values::new(),
                date: date.to_string(),
                feels_mean: number(&d["apparent_temperature_mean"][i]),
                feels_min: number(&d["apparent_temperature_min"][i]),
                feels_max: number(&d["apparent_temperature_max"][i]),
                temperature_min: number(&d["temperature_2m_min"][i]),
                temperature_mean: number(&d["temperature_2m_mean"][i]),
                temperature_max: number(&d["temperature_2m_max"][i]),
                precipitation_sum: number(&d["precipitation_sum"][i]),
                rain_sum: liquid_rain(&d["rain_sum"][i], &d["showers_sum"][i]),
                snowfall_sum: number(&d["snowfall_sum"][i]),
                wind_speed_max: number(&d["wind_speed_10m_max"][i]),
                cloud_cover: number(&d["cloud_cover_mean"][i]),
                humidity: number(&d["relative_humidity_2m_mean"][i]),
                pressure: number(&d["surface_pressure_mean"][i]),
                condition: condition(&d["weather_code"][i], &Value::Null),
            };
            if day.available() {
                weather.daily.push(day);
            }
        }
    }
    weather.ranges = hourly_ranges(&weather.day_hourly);
    if weather.day_hourly.is_empty() {
        weather
            .warnings
            .push("No hourly coverage for today's local-date min/max".into());
    }
    Ok(weather)
}
fn condition(code: &Value, day: &Value) -> Option<Condition> {
    Some(Condition {
        code: u8::try_from(code.as_u64()?).ok()?,
        is_day: day.as_u64().map(|v| v != 0),
        source: "auto".into(),
    })
}
pub fn blend(parts: &[Weather]) -> Result<Weather> {
    let mut out = Weather::empty(
        parts
            .first()
            .context("No models available for blend")?
            .city
            .clone(),
    );
    out.time = parts
        .iter()
        .map(|p| p.time.as_str())
        .max()
        .unwrap_or_default()
        .into();
    let aligned: Vec<_> = parts.iter().filter(|p| p.time == out.time).collect();
    out.condition = aligned.iter().find_map(|p| p.condition.clone());
    if aligned.len() != parts.len() {
        out.warnings
            .push("Current timestamps differ; current values use latest timestamp only".into());
    }
    for &(key, _) in FIELDS {
        let values: Vec<_> = aligned
            .iter()
            .map(|p| p.values.get(key).copied().flatten())
            .collect();
        out.contributors
            .insert(key.into(), values.iter().flatten().count());
        out.values.insert(key.into(), mean(values.into_iter()));
    }
    let times: BTreeSet<_> = parts
        .iter()
        .flat_map(|p| p.hourly.iter().map(|h| h.time.clone()))
        .collect();
    for time in times {
        let mut hour = Hour {
            time: time.clone(),
            values: Values::new(),
            contributors: BTreeMap::new(),
            condition: parts.iter().find_map(|p| {
                p.hourly
                    .iter()
                    .find(|h| h.time == time)
                    .and_then(|h| h.condition.clone())
            }),
        };
        for &(key, _) in FIELDS {
            let values: Vec<_> = parts
                .iter()
                .map(|p| {
                    p.hourly
                        .iter()
                        .find(|h| h.time == time)
                        .and_then(|h| h.values.get(key).copied().flatten())
                })
                .collect();
            hour.contributors
                .insert(key.into(), values.iter().flatten().count());
            hour.values.insert(key.into(), mean(values.into_iter()));
        }
        out.hourly.push(hour);
    }
    for part in parts {
        out.sources.extend(part.sources.clone());
        out.provenance.extend(part.provenance.clone());
    }
    if let Some(latest) = aligned.first() {
        out.range_date = latest.range_date.clone();
        out.range_timezone = latest.range_timezone.clone();
    }
    // Compute extrema of the blended curve, not averages of model extrema.
    let day_parts: Vec<_> = parts
        .iter()
        .filter(|p| p.range_date == out.range_date && p.range_timezone == out.range_timezone)
        .collect();
    for key in ["precipitation", "rain", "snowfall"] {
        out.daily_totals.insert(
            key.into(),
            mean(
                day_parts
                    .iter()
                    .map(|p| p.daily_totals.get(key).copied().flatten()),
            ),
        );
    }
    let times: BTreeSet<_> = day_parts
        .iter()
        .flat_map(|p| p.day_hourly.iter().map(|h| h.time.clone()))
        .collect();
    for time in times {
        let mut hour = Hour {
            time: time.clone(),
            values: Values::new(),
            contributors: BTreeMap::new(),
            condition: day_parts.iter().find_map(|p| {
                p.day_hourly
                    .iter()
                    .find(|h| h.time == time)
                    .and_then(|h| h.condition.clone())
            }),
        };
        for &(key, _) in FIELDS {
            let values: Vec<_> = day_parts
                .iter()
                .map(|p| {
                    p.day_hourly
                        .iter()
                        .find(|h| h.time == time)
                        .and_then(|h| h.values.get(key).copied().flatten())
                })
                .collect();
            hour.contributors
                .insert(key.into(), values.iter().flatten().count());
            hour.values.insert(key.into(), mean(values.into_iter()));
        }
        out.day_hourly.push(hour);
    }
    out.ranges = hourly_ranges(&out.day_hourly);
    let dates: BTreeSet<_> = parts
        .iter()
        .flat_map(|p| p.daily.iter().map(|d| d.date.clone()))
        .collect();
    for date in dates {
        let days: Vec<_> = parts
            .iter()
            .filter_map(|p| p.daily.iter().find(|d| d.date == date))
            .collect();
        let average = |field: fn(&Day) -> Option<f64>| mean(days.iter().map(|d| field(d)));
        out.daily.push(Day {
            solar: Values::new(),
            date,
            feels_mean: average(|d| d.feels_mean),
            feels_min: average(|d| d.feels_min),
            feels_max: average(|d| d.feels_max),
            temperature_min: average(|d| d.temperature_min),
            temperature_mean: average(|d| d.temperature_mean),
            temperature_max: average(|d| d.temperature_max),
            precipitation_sum: average(|d| d.precipitation_sum),
            rain_sum: average(|d| d.rain_sum),
            snowfall_sum: average(|d| d.snowfall_sum),
            wind_speed_max: average(|d| d.wind_speed_max),
            cloud_cover: average(|d| d.cloud_cover),
            humidity: average(|d| d.humidity),
            pressure: average(|d| d.pressure),
            condition: days.iter().find_map(|d| d.condition.clone()),
        });
    }
    Ok(out)
}
const HISTORY: &[(&str, &str)] = &[
    ("temperature_2m", "temperature_2m_mean"),
    ("high", "temperature_2m_max"),
    ("low", "temperature_2m_min"),
    ("precipitation", "precipitation_sum"),
    ("rain", "rain_sum"),
    ("snowfall", "snowfall_sum"),
    ("wind_speed_10m", "wind_speed_10m_mean"),
    ("apparent_temperature", "apparent_temperature_mean"),
    ("relative_humidity_2m", "relative_humidity_2m_mean"),
    ("surface_pressure", "surface_pressure_mean"),
    ("cloud_cover", "cloud_cover_mean"),
];
pub fn aggregate_history(daily: &Value) -> Result<Vec<Month>> {
    let times = daily["time"]
        .as_array()
        .context("Archive missing daily timestamps")?;
    ensure!(!times.is_empty(), "Archive has no daily records");
    Ok((1..=12)
        .map(|month| {
            let indices: Vec<_> = times
                .iter()
                .enumerate()
                .filter_map(|(i, t)| {
                    (t.as_str()?.get(5..7)?.parse::<u32>().ok()? == month).then_some(i)
                })
                .collect();
            let mut result = Month {
                month,
                values: Values::new(),
                sample_counts: BTreeMap::new(),
                ranges: Ranges::new(),
            };
            for &(key, field) in HISTORY {
                let values: Vec<_> = indices.iter().map(|&i| number(&daily[field][i])).collect();
                result
                    .sample_counts
                    .insert(key.into(), values.iter().flatten().count());
                result.values.insert(key.into(), mean(values.into_iter()));
            }
            for &(key, _) in FIELDS {
                let lows: Vec<_> = indices
                    .iter()
                    .map(|&i| number(&daily[format!("{key}_min")][i]))
                    .collect();
                let highs: Vec<_> = indices
                    .iter()
                    .map(|&i| number(&daily[format!("{key}_max")][i]))
                    .collect();
                result.ranges.insert(
                    key.into(),
                    Range {
                        samples: lows
                            .iter()
                            .flatten()
                            .count()
                            .min(highs.iter().flatten().count()),
                        min: mean(lows.into_iter()),
                        max: mean(highs.into_iter()),
                    },
                );
            }
            result
        })
        .collect())
}
pub fn aggregate_rain_ranges(months: &mut [Month], hourly: &Value) {
    let Some(times) = hourly["time"].as_array() else {
        return;
    };
    let mut days: BTreeMap<String, Vec<Option<f64>>> = BTreeMap::new();
    for (i, time) in times.iter().enumerate() {
        if let Some(date) = time.as_str().and_then(|t| t.get(..10)) {
            let values = days.entry(date.into()).or_default();
            values.push(number(&hourly["precipitation"][i]));
        }
    }
    for month in months {
        // Incomplete days would bias mean extrema; retain complete hourly days only.
        let days: Vec<_> = days
            .iter()
            .filter(|(d, v)| {
                d[5..7].parse::<u32>().ok() == Some(month.month)
                    && v.len() >= 23
                    && v.iter().all(Option::is_some)
            })
            .map(|(_, v)| v)
            .collect();
        month.ranges.insert(
            "precipitation".into(),
            Range {
                min: mean(
                    days.iter()
                        .map(|v| v.iter().flatten().copied().reduce(f64::min)),
                ),
                max: mean(
                    days.iter()
                        .map(|v| v.iter().flatten().copied().reduce(f64::max)),
                ),
                samples: days.len(),
            },
        );
    }
}
fn historical(
    client: &Client,
    city: &City,
    years: u32,
    month: u32,
    refresh: bool,
) -> Result<Weather> {
    let end = Utc::now().year() - 1;
    let start = end - years as i32 + 1;
    let mut params = coords(city);
    params.extend([
        ("start_date", format!("{start}-01-01")),
        ("end_date", format!("{end}-12-31")),
        (
            "daily",
            HISTORY
                .iter()
                .map(|f| f.1.to_string())
                .chain(
                    FIELDS
                        .iter()
                        .filter(|(k, _)| !crate::model::is_precipitation(k))
                        .flat_map(|(k, _)| [format!("{k}_min"), format!("{k}_max")]),
                )
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join(","),
        ),
        ("timezone", "auto".into()),
        ("models", "era5".into()),
    ]);
    let (v, meta) = client.get(
        "https://archive-api.open-meteo.com/v1/archive",
        &params,
        HISTORY_TTL,
        refresh,
    )?;
    let mut out = Weather::empty(city.clone());
    out.monthly = aggregate_history(&v["daily"])?;
    out.values = out.monthly[(month - 1) as usize].values.clone();
    out.ranges = out.monthly[(month - 1) as usize].ranges.clone();
    out.range_date = format!("{start}–{end}, month {month}");
    out.range_timezone = v["timezone"].as_str().unwrap_or("local").into();
    out.time = format!("{start}–{end}, month {month}");
    out.baseline = Some(Baseline { start_year: start, end_year: end, month, method: "Monthly means and mean daily minima/maxima over completed years. Rain value: mm/day; rain range: hourly mm. Not official climate normals.".into() });
    out.sources.push("era5".into());
    out.provenance.insert("era5".into(), meta);
    Ok(out)
}
pub fn fixture(city: &City, mode: Mode, years: u32, month: u32) -> Weather {
    let mut out = Weather::empty(city.clone());
    let offset = city.latitude / 10.0;
    out.time = "2026-09-19T00:00".into();
    out.sources.push("demo".into());
    out.values = FIELDS
        .iter()
        .zip([
            18.0 + offset,
            17.0 + offset,
            65.0,
            0.2,
            0.2,
            0.0,
            12.0,
            1012.0,
            30.0,
        ])
        .map(|((k, _), v)| ((*k).into(), Some(v)))
        .collect();
    if mode == Mode::Historical {
        out.monthly = (1..=12)
            .map(|m| {
                let temp = 14.0
                    + offset
                    + 8.0 * ((f64::from(m) - 3.0) / 12.0 * std::f64::consts::TAU).sin();
                Month {
                    month: m,
                    values: [
                        ("temperature_2m", temp),
                        ("high", temp + 4.0),
                        ("low", temp - 4.0),
                        ("precipitation", 2.1),
                        ("rain", 1.8),
                        ("snowfall", 0.2),
                        ("wind_speed_10m", 15.0),
                        ("apparent_temperature", temp - 1.0),
                        ("relative_humidity_2m", 65.0),
                        ("surface_pressure", 1012.0),
                        ("cloud_cover", 30.0),
                    ]
                    .map(|(k, v)| (k.into(), Some(v)))
                    .into(),
                    sample_counts: BTreeMap::new(),
                    ranges: [
                        ("temperature_2m", temp - 4.0, temp + 4.0),
                        ("apparent_temperature", temp - 5.0, temp + 3.0),
                        ("relative_humidity_2m", 50.0, 80.0),
                        ("precipitation", 0.0, 1.1),
                        ("wind_speed_10m", 5.0, 25.0),
                        ("surface_pressure", 1008.0, 1016.0),
                        ("cloud_cover", 10.0, 50.0),
                    ]
                    .map(|(k, min, max)| {
                        (
                            k.into(),
                            Range {
                                min: Some(min),
                                max: Some(max),
                                samples: 300,
                            },
                        )
                    })
                    .into(),
                }
            })
            .collect();
        out.values = out.monthly[(month - 1) as usize].values.clone();
        out.ranges = out.monthly[(month - 1) as usize].ranges.clone();
        out.range_date = format!("DEMO month {month}");
        out.range_timezone = "local".into();
        out.time = "DEMO monthly baseline".into();
        out.baseline = Some(Baseline {
            start_year: Utc::now().year() - years as i32,
            end_year: Utc::now().year() - 1,
            month,
            method: "DEMO fixture; not observed history".into(),
        });
    } else {
        out.condition = Some(Condition {
            code: 2,
            is_day: Some(true),
            source: "demo".into(),
        });
        out.hourly = (0..48)
            .map(|i| {
                let mut values = out.values.clone();
                let wave = (f64::from(i) / 24.0 * std::f64::consts::TAU).sin();
                for (key, swing) in [
                    ("temperature_2m", 4.0),
                    ("apparent_temperature", 5.0),
                    ("relative_humidity_2m", 15.0),
                    ("precipitation", 0.2),
                    ("rain", 0.2),
                    ("snowfall", 0.1),
                    ("wind_speed_10m", 8.0),
                    ("surface_pressure", 4.0),
                    ("cloud_cover", 20.0),
                ] {
                    if let Some(Some(value)) = values.get_mut(key) {
                        *value += swing * wave;
                    }
                }
                Hour {
                    time: format!("2026-09-{:02}T{:02}:00", 18 + i / 24, i % 24),
                    values,
                    contributors: BTreeMap::new(),
                    condition: Some(Condition {
                        code: [0, 2, 3, 61, 71, 95][(i as usize / 4) % 6],
                        is_day: Some((6..18).contains(&(i % 24))),
                        source: "demo".into(),
                    }),
                }
            })
            .collect();
        out.day_hourly = out.hourly[24..].to_vec();
        out.range_date = "2026-09-19".into();
        out.range_timezone = "UTC".into();
        out.ranges = hourly_ranges(&out.day_hourly);
        out.daily = (1..=15)
            .map(|i| Day {
                solar: Values::new(),
                date: (chrono::NaiveDate::from_ymd_opt(2026, 9, 19).unwrap()
                    + chrono::Duration::days(i))
                .to_string(),
                feels_mean: Some(17.5 + offset + (i as f64 / 2.0).sin() * 3.0),
                feels_min: Some(13.0 + offset + (i as f64 / 2.0).sin() * 3.0),
                feels_max: Some(22.0 + offset + (i as f64 / 2.0).sin() * 3.0),
                temperature_min: Some(14.0 + offset + (i as f64 / 2.0).sin() * 3.0),
                temperature_mean: Some(18.5 + offset + (i as f64 / 2.0).sin() * 3.0),
                temperature_max: Some(23.0 + offset + (i as f64 / 2.0).sin() * 3.0),
                precipitation_sum: Some(if i % 3 == 0 { 5.4 } else { 0.0 }),
                rain_sum: Some(if i % 3 == 0 { 5.4 } else { 0.0 }),
                snowfall_sum: Some(if i % 5 == 0 { 1.2 } else { 0.0 }),
                wind_speed_max: Some(12.0 + i as f64),
                cloud_cover: Some((i * 7 % 100) as f64),
                humidity: Some(65.0),
                pressure: Some(1012.0),
                condition: Some(Condition {
                    code: [0, 2, 3, 61, 71, 95][i as usize % 6],
                    is_day: None,
                    source: "demo".into(),
                }),
            })
            .collect();
    }
    out
}
#[derive(Debug, Clone)]
pub struct FetchOptions {
    pub offline: bool,
    pub refresh: bool,
    pub demo: bool,
    pub no_location: bool,
    pub years: u32,
    pub month: u32,
    pub city_ids: Vec<String>,
    pub past_days: u32,
}
#[derive(Clone)]
pub struct Service {
    pub config: Config,
    pub options: FetchOptions,
    pub client: Client,
}

fn fetch_blend_models(
    fetch: impl Fn(Source) -> Result<Weather> + Sync,
) -> [(Source, Result<Weather>); 2] {
    std::thread::scope(|scope| {
        let gfs = scope.spawn(|| fetch(Source::Gfs));
        let icon = fetch(Source::Icon);
        [
            (
                Source::Gfs,
                gfs.join()
                    .unwrap_or_else(|_| Err(anyhow!("GFS worker panicked"))),
            ),
            (Source::Icon, icon),
        ]
    })
}

impl Service {
    /// No network: configured cities, cached location (if any), and offline clocks.
    pub fn initial(&self, prefs: &Preferences) -> Result<Report> {
        let mut cached = self.clone();
        cached.client.offline = true;
        let (cities, current, _) = cached.cities();
        let rows = cities
            .into_iter()
            .filter(|c| self.options.city_ids.is_empty() || self.options.city_ids.contains(&c.id))
            .map(|city| {
                let mut row = Weather::empty(city);
                row.range_timezone = crate::view::coordinate_timezone(&row.city).into();
                row
            })
            .collect();
        self.report(prefs, current, vec![], rows)
    }
    pub fn cities(&self) -> (Vec<City>, Option<String>, Vec<String>) {
        let mut cities = self.config.cities.clone();
        let mut current = None;
        let mut warnings = vec![];
        if self.config.auto_location && !self.options.no_location && !self.options.demo {
            match self.client.detect() {
                Ok((city, meta)) => {
                    current = Some(merge_current(&mut cities, city));
                    if meta.stale {
                        warnings.push("Location uses stale cached estimate".into());
                    }
                }
                Err(e) => warnings.push(format!("Location unavailable: {e:#}")),
            }
        }
        (cities, current, warnings)
    }
    fn one(&self, city: &City, prefs: &Preferences, refresh: bool) -> Weather {
        let mut row = self.one_absolute(city, prefs, refresh);
        if prefs.mode == Mode::Comparison
            && let Some(period) = prefs.comparison_period
        {
            let mut base_prefs = prefs.clone();
            base_prefs.period = period;
            base_prefs.mode = Mode::Normal;
            base_prefs.comparison_period = None;
            if prefs.period == Period::Now && period != Period::Now {
                base_prefs.month = crate::view::city_now(&row).map_or(1, |d| d.month());
                base_prefs.last_city = None;
            }
            let base = self.one_absolute(city, &base_prefs, refresh);
            if let Some(error) = &base.error {
                row.warnings.push(format!("Comparison period: {error}"));
            }
            row.comparison_base = Some(Box::new(base));
        }
        row
    }
    fn one_absolute(&self, city: &City, prefs: &Preferences, refresh: bool) -> Weather {
        let result = (|| -> Result<Weather> {
            if prefs.period != Period::Now {
                if self.options.demo {
                    let mut row = fixture(city, Mode::Historical, 5, 1);
                    let (start, end) = prefs.period.years();
                    if let Some(b) = &mut row.baseline {
                        b.start_year = start;
                        b.end_year = end;
                        b.month = prefs.month;
                    }
                    for m in &mut row.monthly {
                        for key in ["precipitation", "rain", "snowfall"] {
                            if let Some(v) = m.values.get_mut(key) {
                                *v = v.map(|v| v * 30.);
                            }
                            m.ranges.remove(key);
                        }
                    }
                    if prefs.period == Period::Future {
                        for month in &mut row.monthly {
                            for key in ["apparent_temperature", "surface_pressure"] {
                                month.values.insert(key.into(), None);
                            }
                            month.ranges.retain(|k, _| {
                                matches!(k.as_str(), "temperature_2m" | "wind_speed_10m")
                            });
                            if let Some(w) = month.ranges.get_mut("wind_speed_10m") {
                                w.min = None;
                            }
                        }
                    }
                    let mut annual = row.monthly[0].clone();
                    annual.month = 0;
                    for (key, v) in &mut annual.values {
                        *v = mean(
                            row.monthly
                                .iter()
                                .map(|m| m.values.get(key).copied().flatten()),
                        );
                        if is_precipitation(key) {
                            *v = v.map(|v| v * 12.);
                        }
                    }
                    for (key, r) in &mut annual.ranges {
                        r.min = mean(
                            row.monthly
                                .iter()
                                .map(|m| m.ranges.get(key).and_then(|r| r.min)),
                        );
                        r.max = mean(
                            row.monthly
                                .iter()
                                .map(|m| m.ranges.get(key).and_then(|r| r.max)),
                        );
                    }
                    row.annual = Some(annual);
                    crate::climate::select(&mut row, prefs.month);
                    row.time = format!(
                        "{}{start}–{end}",
                        if prefs.period == Period::Future {
                            "Projected · "
                        } else {
                            ""
                        }
                    );
                    row.range_date = format!(
                        "{} · {}",
                        prefs.period.label(),
                        crate::climate::MONTHS[prefs.month as usize]
                    );
                    return Ok(row);
                }
                return crate::climate::fetch(
                    &self.client,
                    city,
                    prefs,
                    prefs.last_city.is_some(),
                    refresh,
                );
            }
            if self.options.demo {
                return Ok(fixture(
                    city,
                    prefs.mode,
                    self.options.years,
                    self.options.month,
                ));
            }
            if prefs.mode == Mode::Historical {
                return historical(
                    &self.client,
                    city,
                    self.options.years,
                    self.options.month,
                    refresh,
                );
            }
            if prefs.source != Source::Blend {
                return forecast(
                    &self.client,
                    city,
                    prefs.source,
                    refresh,
                    self.options.past_days,
                );
            }
            let mut parts = vec![];
            let mut errors = vec![];
            let results = fetch_blend_models(|source| {
                forecast(&self.client, city, source, refresh, self.options.past_days)
            });
            for (source, result) in results {
                match result {
                    Ok(p) => parts.push(p),
                    Err(e) => errors.push(format!("{}: {e:#}", source.label())),
                }
            }
            if parts.is_empty() {
                return Err(anyhow!(errors.join("; ")));
            }
            let mut out = blend(&parts)?;
            if !errors.is_empty() {
                out.warnings
                    .push(format!("Degraded blend: {}", errors.join("; ")));
            }
            Ok(out)
        })();
        let mut row = result.unwrap_or_else(|e| {
            let mut row = Weather::empty(city.clone());
            if prefs.period != Period::Now {
                let (start_year, end_year) = prefs.period.years();
                row.baseline = Some(Baseline {
                    start_year,
                    end_year,
                    month: prefs.month,
                    method: "Waiting for complete period data; completed months are cached.".into(),
                });
                row.monthly = (1..=12)
                    .map(|month| Month {
                        month,
                        values: FIELDS
                            .iter()
                            .map(|(key, _)| ((*key).into(), None))
                            .collect(),
                        ranges: Ranges::new(),
                        sample_counts: BTreeMap::new(),
                    })
                    .collect();
            }
            row.error = Some(format!("{e:#}"));
            row.retry_at = e
                .downcast_ref::<crate::rate_limit::Deferred>()
                .map(|e| e.until);
            row
        });
        row.retry_at = row
            .retry_at
            .or_else(|| row.provenance.values().filter_map(|p| p.retry_at).min());
        if row.range_timezone.parse::<chrono_tz::Tz>().is_err() {
            row.range_timezone = crate::view::coordinate_timezone(city).into();
        }
        crate::solar::enrich(&mut row);
        row
    }
    pub fn load(&self, prefs: &Preferences, refresh: bool) -> Result<Report> {
        let mut report = self.load_progress(prefs, refresh, |_| {})?;
        report.present(prefs.mode, &prefs.reference)?;
        if !self.options.city_ids.is_empty() {
            report
                .cities
                .retain(|r| self.options.city_ids.contains(&r.city.id));
        }
        Ok(report)
    }
    /// Publish each completed city without waiting for the rest of the batch.
    /// The callback runs on bounded fetch workers; callers must reject old generations.
    /// Returns absolute data (including a hidden reference) so the host can apply
    /// its latest presentation choices, then restrict to the requested city IDs.
    pub fn load_progress(
        &self,
        prefs: &Preferences,
        refresh: bool,
        on_city: impl Fn(&Weather) + Sync,
    ) -> Result<Report> {
        self.load_progress_prioritized(prefs, refresh, None, on_city)
    }

    /// Publish the visible city's data before location lookup or background cities.
    pub fn load_progress_prioritized(
        &self,
        prefs: &Preferences,
        refresh: bool,
        priority: Option<&str>,
        on_city: impl Fn(&Weather) + Sync,
    ) -> Result<Report> {
        let priority_city = priority
            .filter(|id| {
                self.options.city_ids.is_empty()
                    || self.options.city_ids.iter().any(|selected| selected == id)
            })
            .and_then(|id| {
                self.config
                    .cities
                    .iter()
                    .find(|c| c.id == id)
                    .cloned()
                    .or_else(|| {
                        self.initial(prefs)
                            .ok()?
                            .cities
                            .into_iter()
                            .find(|r| r.city.id == id)
                            .map(|r| r.city)
                    })
            });
        let prefetched = priority_city.map(|city| {
            let row = self.one(&city, prefs, refresh || self.options.refresh);
            on_city(&row);
            row
        });
        let (mut cities, current_city, warnings) = self.cities();
        let selected = &self.options.city_ids;
        for id in selected {
            ensure!(cities.iter().any(|c| &c.id == id), "Unknown city ID: {id}");
        }
        if !selected.is_empty() {
            cities.retain(|c| {
                selected.contains(&c.id)
                    || (prefs.mode == Mode::Comparison
                        && prefs.comparison_period.is_none()
                        && c.id == prefs.reference)
            });
        }
        // Visible/explicit cities precede pinned, current location and background
        // cities. Stable insertion order breaks ties. Superseded views cancel at
        // every HTTP boundary, so a newly requested city jumps ahead of backlog.
        let jobs = std::sync::Mutex::new(
            cities
                .iter()
                .enumerate()
                .map(|(i, city)| {
                    let rank = if selected.len() < cities.len() && selected.contains(&city.id) {
                        3
                    } else if city.id == prefs.reference {
                        2
                    } else if current_city.as_ref() == Some(&city.id) {
                        1
                    } else {
                        0
                    };
                    (rank, std::cmp::Reverse(i))
                })
                .collect::<BinaryHeap<_>>(),
        );
        // Cached blend sources can resolve concurrently; the client serializes
        // actual HTTP. Climate requests are durable per completed month.
        let workers = if prefs.period != Period::Now || prefs.mode == Mode::Historical {
            1
        } else {
            3
        };
        let rows = std::thread::scope(|scope| {
            let prefetched_index = prefetched
                .as_ref()
                .and_then(|r| cities.iter().position(|c| c.id == r.city.id));
            let handles: Vec<_> =
                (0..cities.len().min(workers))
                    .map(|_| {
                        let jobs = &jobs;
                        let cities = &cities;
                        let on_city = &on_city;
                        scope.spawn(move || {
                            let mut rows = vec![];
                            loop {
                                if self.client.generation.as_ref().is_some_and(
                                    |(current, expected)| {
                                        current.load(Ordering::Relaxed) != *expected
                                    },
                                ) {
                                    break;
                                }
                                let Some((_, std::cmp::Reverse(i))) =
                                    jobs.lock().unwrap_or_else(|e| e.into_inner()).pop()
                                else {
                                    break;
                                };
                                if Some(i) == prefetched_index {
                                    continue;
                                }
                                let row =
                                    self.one(&cities[i], prefs, refresh || self.options.refresh);
                                if selected.is_empty() || selected.contains(&row.city.id) {
                                    on_city(&row);
                                }
                                rows.push((i, row));
                            }
                            rows
                        })
                    })
                    .collect();
            let mut rows = vec![];
            if let Some(row) = prefetched
                && let Some(i) = cities.iter().position(|c| c.id == row.city.id)
            {
                rows.push((i, row));
            }
            for handle in handles {
                rows.extend(
                    handle
                        .join()
                        .map_err(|_| anyhow!("Weather worker panicked"))?,
                );
            }
            rows.sort_by_key(|(i, _)| *i);
            Ok::<_, anyhow::Error>(rows.into_iter().map(|(_, row)| row).collect::<Vec<_>>())
        })?;
        let mut absolute_prefs = prefs.clone();
        if prefs.mode == Mode::Comparison {
            absolute_prefs.mode = Mode::Normal;
        }
        self.report(&absolute_prefs, current_city, warnings, rows)
    }
    fn report(
        &self,
        prefs: &Preferences,
        current_city: Option<String>,
        warnings: Vec<String>,
        rows: Vec<Weather>,
    ) -> Result<Report> {
        let mut hourly_units: BTreeMap<String, String> = FIELDS
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect();
        for key in crate::solar::KEYS {
            hourly_units.insert(
                key.into(),
                if key == "daylight" {
                    "minutes"
                } else {
                    "minutes after local midnight"
                }
                .into(),
            );
        }
        let mut units = hourly_units.clone();
        if prefs.mode == Mode::Historical || prefs.period != Period::Now {
            units.insert("precipitation".into(), "mm/day".into());
            if prefs.period != Period::Now {
                units.insert(
                    "precipitation".into(),
                    if prefs.month == 0 {
                        "mm/year"
                    } else {
                        "mm/month"
                    }
                    .into(),
                );
            }
            units.insert("high".into(), "°C".into());
            units.insert("low".into(), "°C".into());
        }
        if prefs.mode == Mode::Comparison {
            for k in ["relative_humidity_2m", "cloud_cover"] {
                units.insert(k.into(), "percentage points".into());
            }
        }
        let mut range_units = hourly_units.clone();
        range_units.insert("precipitation".into(), "mm (hourly total)".into());
        if prefs.mode == Mode::Comparison {
            for k in ["relative_humidity_2m", "cloud_cover"] {
                range_units.insert(k.into(), "percentage points".into());
            }
        }
        Ok(Report { comparison_period: prefs.comparison_period, period: prefs.period, schema_version: 1, mode: prefs.mode, source: prefs.source, reference: prefs.reference.clone(), demo: self.options.demo, current_city, timezone: "UTC forecast timestamps; city-local calendar days for ranges and history".into(), units, hourly_units, range_units,
            comparison_scope: "Displayed values, ranges and monthly statistics subtract the selected pinned city or older period; annual, hourly, daily, day_hourly and absolute_* remain absolute in JSON. The TUI also projects hourly and daily details".into(), attribution: "Weather: Open-Meteo / NOAA GFS / DWD ICON / ERA5 / CMIP6 HighResMIP (CC BY 4.0). Geocoding: GeoNames. Location: ipapi.co.".into(), warnings, cities: rows })
    }

    pub fn more_hours(&self, city: &City, prefs: &Preferences, past_days: u32) -> Weather {
        let mut service = self.clone();
        service.options.past_days = past_days.min(92);
        service.one(city, prefs, false)
    }
}

#[cfg(test)]
mod fetch_tests {
    use super::*;

    #[test]
    fn blend_starts_both_sources_without_waiting_for_the_first() {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let rx = std::sync::Mutex::new(rx);
        let results = fetch_blend_models(|source| {
            if source == Source::Gfs {
                rx.lock().unwrap().recv_timeout(Duration::from_secs(2))?;
            } else {
                tx.send(())?;
            }
            Ok(Weather::empty(City {
                id: "test".into(),
                name: "Test".into(),
                latitude: 0.0,
                longitude: 0.0,
            }))
        });
        assert_eq!(results[0].0, Source::Gfs);
        assert_eq!(results[1].0, Source::Icon);
        assert!(results.into_iter().all(|(_, result)| result.is_ok()));
    }
}

fn provider_error_cost(status: u16, data: &Value) -> Option<f64> {
    // A gateway may have failed after upstream completed the request. Retain
    // its full reservation, as with malformed/truncated successful responses.
    (matches!(status, 400..=500)
        && status != 429
        && data.get("error").and_then(Value::as_bool) == Some(true)
        && data.get("reason").and_then(Value::as_str).is_some())
    .then_some(1.0)
}

#[cfg(test)]
mod accounting_response_tests {
    use super::*;
    #[test]
    fn only_structured_api_errors_reduce_the_reserved_weight() {
        let error = serde_json::json!({"error":true,"reason":"Invalid variable"});
        for status in [400, 401, 403, 404, 422, 500] {
            assert_eq!(provider_error_cost(status, &error), Some(1.0));
        }
        for status in [200, 429, 502, 503, 504] {
            assert_eq!(provider_error_cost(status, &error), None);
        }
        assert_eq!(
            provider_error_cost(500, &serde_json::json!({"message":"gateway"})),
            None
        );
        assert_eq!(
            provider_error_cost(200, &serde_json::json!({"daily":{}})),
            None
        );
    }
}

#[cfg(test)]
mod quota_http_tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::{SocketAddr, TcpListener},
    };
    use ureq::unversioned::{
        resolver::{ResolvedSocketAddrs, Resolver},
        transport::{DefaultConnector, NextTimeout},
    };
    #[derive(Debug)]
    struct LocalResolver(Option<SocketAddr>);
    impl Resolver for LocalResolver {
        fn resolve(
            &self,
            _: &ureq::http::Uri,
            _: &ureq::config::Config,
            _: NextTimeout,
        ) -> std::result::Result<ResolvedSocketAddrs, ureq::Error> {
            let address = self.0.ok_or(ureq::Error::HostNotFound)?;
            let mut addresses = self.empty();
            addresses.push(address);
            Ok(addresses)
        }
    }
    fn client(cache: PathBuf, address: Option<SocketAddr>) -> Client {
        let mut client = Client::new(cache, false);
        client.agent = ureq::Agent::with_parts(
            ureq::Agent::config_builder()
                .proxy(None)
                .timeout_global(Some(Duration::from_secs(3)))
                .build(),
            DefaultConnector::default(),
            LocalResolver(address),
        );
        client
    }
    fn total(client: &Client) -> (f64, f64) {
        let value: Value =
            serde_json::from_slice(&fs::read(client.cache.join("limits/archive.json")).unwrap())
                .unwrap();
        (
            value["requests"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r[1].as_f64().unwrap())
                .sum(),
            value["refunded_credits"].as_f64().unwrap(),
        )
    }
    fn params() -> [(&'static str, String); 3] {
        [
            ("daily", "a,b,c,d,e,f,g,h,i,j".into()),
            ("start_date", "1950-01-01".into()),
            ("end_date", "1950-12-31".into()),
        ]
    }
    #[test]
    fn http_outcomes_settle_persisted_reservations_without_refunding_uncertainty() {
        for (status, body, expected) in [
            (200, r#"{"daily":{}}"#, 365.0 / 14.0),
            (200, "broken JSON", 365.0 / 14.0),
            (400, r#"{"error":true,"reason":"Invalid variable"}"#, 1.0),
            (500, r#"{"error":true,"reason":"API failure"}"#, 1.0),
            (
                502,
                r#"{"error":true,"reason":"Gateway failure"}"#,
                365.0 / 14.0,
            ),
            (
                429,
                r#"{"error":true,"reason":"Minutely API request limit exceeded"}"#,
                0.0,
            ),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut buffer = [0; 4096];
                assert!(stream.read(&mut buffer).unwrap() > 0);
                write!(stream,"HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            });
            let tmp = tempfile::tempdir().unwrap();
            let client = client(tmp.path().into(), Some(address));
            let _ = client.get(
                "http://archive-api.open-meteo.com/v1/archive",
                &params(),
                1800,
                false,
            );
            server.join().unwrap();
            let (charged, refunded) = total(&client);
            assert!(
                (charged - expected).abs() < 1e-9,
                "status {status}: {charged}"
            );
            assert!((charged + refunded - 365.0 / 14.0).abs() < 1e-9);
        }
    }
    #[test]
    fn dns_failure_returns_reserved_capacity_and_records_refund() {
        let tmp = tempfile::tempdir().unwrap();
        let client = client(tmp.path().into(), None);
        assert!(
            client
                .get(
                    "http://archive-api.open-meteo.com/v1/archive",
                    &params(),
                    1800,
                    false
                )
                .is_err()
        );
        let (charged, refunded) = total(&client);
        assert_eq!(charged, 0.0);
        assert!((refunded - 365.0 / 14.0).abs() < 1e-9);
    }
}
