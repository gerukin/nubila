//! Period climate data. Only Open-Meteo; immutable historical month summaries.
use crate::{
    config::{Preferences, atomic_write},
    model::*,
    service::{Client, HISTORY_TTL, aggregate_history},
};
use anyhow::{Result, ensure};
use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs};

// Three distinct model families with temperature, wind, humidity and cloud data.
pub const MODELS: [&str; 3] = ["MRI_AGCM3_2_S", "EC_Earth3P_HR", "MPI_ESM1_2_XR"];
pub const MONTHS: [&str; 13] = [
    "Annual", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
#[derive(Clone, Serialize, Deserialize)]
struct Chunk {
    #[serde(default)]
    split_precipitation: bool,
    at: i64,
    days: usize,
    month: Month,
}
fn fields(future: bool) -> String {
    let mut fields = vec![
        "temperature_2m_mean".into(),
        "temperature_2m_min".into(),
        "temperature_2m_max".into(),
        "precipitation_sum".into(),
        "wind_speed_10m_mean".into(),
        "wind_speed_10m_max".into(),
        "relative_humidity_2m_mean".into(),
        "cloud_cover_mean".into(),
    ];
    if !future {
        for key in [
            "apparent_temperature",
            "relative_humidity_2m",
            "surface_pressure",
            "cloud_cover",
        ] {
            for suffix in ["mean", "min", "max"] {
                fields.push(format!("{key}_{suffix}"));
            }
        }
        fields.push("wind_speed_10m_min".into());
    }
    fields.sort();
    fields.dedup();
    fields.join(",")
}
fn days(year: i32, month: u32) -> usize {
    let start = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
    let end = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    }
    .unwrap();
    (end - start).num_days() as usize
}
fn combine(month: u32, chunks: &[&Chunk], annual: bool) -> Month {
    let mut out = Month {
        month,
        values: Values::new(),
        ranges: Ranges::new(),
        sample_counts: BTreeMap::new(),
    };
    for key in FIELDS.iter().map(|f| f.0).chain(["high", "low"]) {
        let mut sum = 0.;
        let mut count = 0;
        for chunk in chunks {
            if let Some(v) = chunk.month.values.get(key).copied().flatten() {
                let n = chunk.month.sample_counts.get(key).copied().unwrap_or(0);
                if is_precipitation(key) && n != chunk.days {
                    continue;
                }
                sum += v * n as f64;
                count += n;
            }
        }
        let v = if is_precipitation(key) {
            // mm/day × valid days = total, then average totals over years/models.
            let periods = chunks
                .iter()
                .filter(|c| {
                    c.month.values.get(key).copied().flatten().is_some()
                        && c.month.sample_counts.get(key).copied() == Some(c.days)
                })
                .count();
            (periods > 0 && ((!annual && key == "precipitation") || periods == chunks.len()))
                .then(|| sum / periods as f64 * if annual { 12. } else { 1. })
        } else {
            (count > 0).then(|| sum / count as f64)
        };
        out.values.insert(key.into(), v);
        out.sample_counts.insert(key.into(), count);
        if is_precipitation(key) {
            continue;
        }
        let bound = |low: bool| {
            let mut total = 0.;
            let mut n = 0;
            for c in chunks {
                if let Some(r) = c.month.ranges.get(key)
                    && let Some(v) = if low { r.min } else { r.max }
                {
                    total += v * c.days as f64;
                    n += c.days;
                }
            }
            (n > 0).then(|| total / n as f64)
        };
        out.ranges.insert(
            key.into(),
            Range {
                min: bound(true),
                max: bound(false),
                samples: count,
            },
        );
    }
    out
}
pub fn select(row: &mut Weather, month: u32) {
    if let Some(selected) = if month == 0 {
        row.annual.as_ref()
    } else {
        row.monthly.iter().find(|m| m.month == month)
    } {
        row.values = selected.values.clone();
        row.ranges = selected.ranges.clone();
    } else {
        row.values.clear();
        row.ranges.clear();
    }
}
pub fn fetch(
    client: &Client,
    city: &City,
    prefs: &Preferences,
    full: bool,
    refresh: bool,
) -> Result<Weather> {
    fetch_using(client, city, prefs, full, refresh, |endpoint, params| {
        client.transient(endpoint, params)
    })
}

fn fetch_using(
    client: &Client,
    city: &City,
    prefs: &Preferences,
    full: bool,
    refresh: bool,
    mut download: impl FnMut(&str, &[(&str, String)]) -> Result<(serde_json::Value, Provenance)>,
) -> Result<Weather> {
    let future = prefs.period == Period::Future;
    let (start, end) = prefs.period.years();
    let models: Vec<&str> = if future {
        MODELS.to_vec()
    } else {
        vec!["era5"]
    };
    let wanted: Vec<u32> = if full || prefs.month == 0 {
        (1..=12).collect()
    } else {
        vec![prefs.month]
    };
    let root = client.cache.join("climate-v1");
    let mut chunks = vec![];
    let mut warning = None;
    let mut retry_at = None;
    let mut fetched = 0;
    let mut oldest = i64::MAX;
    for model in &models {
        for year in start..=end {
            let fingerprint = format!(
                "{}:{}:{model}:{}",
                city.latitude,
                city.longitude,
                fields(future)
            );
            let dir = root.join(format!("{:x}", Sha256::digest(fingerprint.as_bytes())));
            if !future
                && wanted
                    .iter()
                    .any(|m| !dir.join(format!("{year}-{m:02}.json")).exists())
            {
                // Corrupt obsolete cache entries are misses, never a reason to
                // suppress the normal fetch/retry path.
                let _ = import_previous_cache(client, city, &dir, year);
            }
            let now = Utc::now().timestamp();
            let mut saved: BTreeMap<u32, Chunk> = BTreeMap::new();
            let mut missing = vec![];
            for &m in &wanted {
                let file = dir.join(format!("{year}-{m:02}.json"));
                let chunk = fs::read(file)
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Chunk>(&b).ok());
                if let Some(c) = chunk {
                    let fresh = !future || now - c.at < HISTORY_TTL;
                    if refresh || !fresh {
                        missing.push(m);
                    }
                    saved.insert(m, c);
                } else {
                    missing.push(m);
                }
            }
            // Consecutive missing months share a request. Already-cached months are
            // never downloaded again during ordinary incremental loads.
            let mut cursor = 0;
            while cursor < missing.len() {
                let first = missing[cursor];
                let mut last = first;
                cursor += 1;
                while cursor < missing.len() && missing[cursor] == last + 1 {
                    last = missing[cursor];
                    cursor += 1;
                }
                let params = vec![
                    ("latitude", city.latitude.to_string()),
                    ("longitude", city.longitude.to_string()),
                    ("models", model.to_string()),
                    ("start_date", format!("{year}-{first:02}-01")),
                    (
                        "end_date",
                        format!("{year}-{last:02}-{:02}", days(year, last)),
                    ),
                    ("daily", format!("{},rain_sum,snowfall_sum", fields(future))),
                    ("timezone", if future { "GMT" } else { "auto" }.into()),
                ];
                let endpoint = if future {
                    "https://climate-api.open-meteo.com/v1/climate"
                } else {
                    "https://archive-api.open-meteo.com/v1/archive"
                };
                match download(endpoint, &params) {
                    Ok((v, meta)) => {
                        let daily = if v.get("daily").is_some() {
                            &v["daily"]
                        } else {
                            &v[0]["daily"]
                        };
                        let months = aggregate_history(daily)?;
                        for m in first..=last {
                            let month = months[(m - 1) as usize].clone();
                            let valid = month
                                .sample_counts
                                .get("temperature_2m")
                                .copied()
                                .unwrap_or(0);
                            ensure!(
                                valid == days(year, m),
                                "Incomplete {model} history for {year}-{m:02}; retry later"
                            );
                            let chunk = Chunk {
                                split_precipitation: true,
                                at: meta.fetched_at,
                                days: days(year, m),
                                month,
                            };
                            atomic_write(
                                &dir.join(format!("{year}-{m:02}.json")),
                                &serde_json::to_vec(&chunk)?,
                            )?;
                            saved.insert(m, chunk);
                            fetched += 1;
                        }
                    }
                    Err(e) => {
                        if missing.iter().any(|m| !saved.contains_key(m)) {
                            return Err(e.context(format!(
                                "{}/{} monthly samples saved; next retry resumes missing data",
                                chunks.len() + saved.len(),
                                models.len() * (end - start + 1) as usize * wanted.len()
                            )));
                        }
                        retry_at = e
                            .downcast_ref::<crate::rate_limit::Deferred>()
                            .map(|e| e.until);
                        warning = Some(format!("{e:#}"));
                        break;
                    }
                }
            }
            // Enrich old climate chunks using just the new variables. The original
            // field fingerprint stays stable so temperature/wind/etc are reused.
            let additions: Vec<_> = wanted
                .iter()
                .copied()
                .filter(|m| saved.get(m).is_some_and(|c| !c.split_precipitation))
                .collect();
            let mut cursor = 0;
            while cursor < additions.len() && warning.is_none() {
                let first = additions[cursor];
                let mut last = first;
                cursor += 1;
                while cursor < additions.len() && additions[cursor] == last + 1 {
                    last = additions[cursor];
                    cursor += 1;
                }
                let params = vec![
                    ("latitude", city.latitude.to_string()),
                    ("longitude", city.longitude.to_string()),
                    ("models", model.to_string()),
                    ("start_date", format!("{year}-{first:02}-01")),
                    (
                        "end_date",
                        format!("{year}-{last:02}-{:02}", days(year, last)),
                    ),
                    ("daily", "rain_sum,snowfall_sum".into()),
                    ("timezone", if future { "GMT" } else { "auto" }.into()),
                ];
                let endpoint = if future {
                    "https://climate-api.open-meteo.com/v1/climate"
                } else {
                    "https://archive-api.open-meteo.com/v1/archive"
                };
                match download(endpoint, &params).and_then(|(v, _)| {
                    let expected: usize = (first..=last).map(|m| days(year, m)).sum();
                    ensure!(
                        v["daily"]["time"]
                            .as_array()
                            .is_some_and(|times| times.len() == expected),
                        "Incomplete rain/snow calendar interval; retry later"
                    );
                    aggregate_history(&v["daily"])
                }) {
                    Ok(months) => {
                        for m in first..=last {
                            let c = saved.get_mut(&m).unwrap();
                            let added = &months[(m - 1) as usize];
                            for key in ["rain", "snowfall"] {
                                c.month
                                    .values
                                    .insert(key.into(), added.values.get(key).copied().flatten());
                                c.month.sample_counts.insert(
                                    key.into(),
                                    added.sample_counts.get(key).copied().unwrap_or(0),
                                );
                            }
                            c.split_precipitation = true;
                            atomic_write(
                                &dir.join(format!("{year}-{m:02}.json")),
                                &serde_json::to_vec(c)?,
                            )?;
                            fetched += 1;
                        }
                    }
                    Err(e) => {
                        retry_at = e
                            .downcast_ref::<crate::rate_limit::Deferred>()
                            .map(|e| e.until);
                        warning = Some(format!("Rain/snow split pending: {e:#}"));
                    }
                }
            }
            for m in &wanted {
                if let Some(c) = saved.remove(m) {
                    oldest = oldest.min(c.at);
                    chunks.push(c);
                }
            }
        }
    }
    let mut row = Weather::empty(city.clone());
    row.monthly = wanted
        .iter()
        .map(|m| {
            combine(
                *m,
                &chunks
                    .iter()
                    .filter(|c| c.month.month == *m)
                    .collect::<Vec<_>>(),
                false,
            )
        })
        .collect();
    if wanted.len() == 12 {
        row.annual = Some(combine(0, &chunks.iter().collect::<Vec<_>>(), true));
    }
    select(&mut row, prefs.month);
    row.baseline = Some(Baseline {
        start_year: start,
        end_year: end,
        month: prefs.month,
        method: if future {
            format!(
                "Projected climate, equal-weight mean of {}. Model simulations, not a weather forecast. Missing metrics are unavailable.",
                MODELS.join(", ")
            )
        } else {
            "ERA5. Day-weighted means; mean daily minima/maxima. Rain: average calendar-period total. No hourly rainfall.".into()
        },
    });
    row.time = format!("{}{start}–{end}", if future { "Projected · " } else { "" });
    row.range_date = format!("{start}–{end} · {}", MONTHS[prefs.month as usize]);
    row.range_timezone = crate::view::coordinate_timezone(city).into();
    row.sources = models.iter().map(|s| s.to_string()).collect();
    row.retry_at = retry_at;
    row.provenance.insert(
        if future { "climate" } else { "era5" }.into(),
        Provenance {
            cached: fetched == 0,
            stale: warning.is_some(),
            fetched_at: oldest,
            warning,
            retry_at,
        },
    );
    Ok(row)
}

/// Select a list period, retaining all monthly data for details and comparisons.
pub fn select_report(report: &mut Report, month: u32, mode: Mode, reference: &str) -> Result<()> {
    report.present(Mode::Normal, reference)?;
    for row in &mut report.cities {
        select(row, month);
        if let Some(base) = &mut row.comparison_base
            && base.baseline.is_some()
        {
            select(base, month);
            if let Some(b) = &mut base.baseline {
                b.month = month;
            }
        }
        if let Some(b) = &mut row.baseline {
            b.month = month;
            row.range_date = format!(
                "{}–{} · {}",
                b.start_year, b.end_year, MONTHS[month as usize]
            );
        }
    }
    report.present(mode, reference)
}

pub fn compare_details(row: &mut Weather, reference: &Weather) {
    if row.baseline.is_none() && reference.baseline.is_some() && row.city.id == reference.city.id {
        for hour in &mut row.hourly {
            for (key, value) in &mut hour.values {
                *value = if is_precipitation(key) {
                    None
                } else {
                    value
                        .zip(reference.values.get(key).copied().flatten())
                        .map(|(a, b)| a - b)
                };
            }
        }
        for day in &mut row.daily {
            day.feels_mean = day
                .feels_mean
                .zip(
                    reference
                        .values
                        .get("apparent_temperature")
                        .copied()
                        .flatten(),
                )
                .map(|(a, b)| a - b);
            let feels = reference.ranges.get("apparent_temperature");
            day.feels_min = day
                .feels_min
                .zip(feels.and_then(|r| r.min))
                .map(|(a, b)| a - b);
            day.feels_max = day
                .feels_max
                .zip(feels.and_then(|r| r.max))
                .map(|(a, b)| a - b);
            day.temperature_mean = day
                .temperature_mean
                .zip(reference.values.get("temperature_2m").copied().flatten())
                .map(|(a, b)| a - b);
            let temp = reference.ranges.get("temperature_2m");
            day.temperature_min = day
                .temperature_min
                .zip(temp.and_then(|r| r.min))
                .map(|(a, b)| a - b);
            day.temperature_max = day
                .temperature_max
                .zip(temp.and_then(|r| r.max))
                .map(|(a, b)| a - b);
            day.precipitation_sum = day
                .precipitation_sum
                .zip(crate::comparison::daily_rain(reference))
                .map(|(a, b)| a - b);
            day.rain_sum = day
                .rain_sum
                .zip(crate::comparison::daily_precipitation(reference, "rain"))
                .map(|(a, b)| a - b);
            day.snowfall_sum = day
                .snowfall_sum
                .zip(crate::comparison::daily_precipitation(
                    reference, "snowfall",
                ))
                .map(|(a, b)| a - b);
            day.wind_speed_max = day
                .wind_speed_max
                .zip(reference.ranges.get("wind_speed_10m").and_then(|r| r.max))
                .map(|(a, b)| a - b);
            day.cloud_cover = day
                .cloud_cover
                .zip(reference.values.get("cloud_cover").copied().flatten())
                .map(|(a, b)| a - b);
            day.humidity = day
                .humidity
                .zip(
                    reference
                        .values
                        .get("relative_humidity_2m")
                        .copied()
                        .flatten(),
                )
                .map(|(a, b)| a - b);
            day.pressure = day
                .pressure
                .zip(reference.values.get("surface_pressure").copied().flatten())
                .map(|(a, b)| a - b);
            for (key, value) in &mut day.solar {
                *value = value
                    .zip(reference.values.get(key).copied().flatten())
                    .map(|(a, b)| crate::solar::difference(key, a, b));
            }
        }
        return;
    }
    for hour in &mut row.hourly {
        if let Some(base) = reference.hourly.iter().find(|h| h.time == hour.time) {
            for (k, v) in &mut hour.values {
                *v = v
                    .zip(base.values.get(k).copied().flatten())
                    .map(|(a, b)| a - b);
            }
        } else {
            for v in hour.values.values_mut() {
                *v = None;
            }
        }
    }
    for day in &mut row.daily {
        let b = reference.daily.iter().find(|b| b.date == day.date);
        macro_rules! delta {($($field:ident),*)=>{$(day.$field=day.$field.zip(b.and_then(|b|b.$field)).map(|(a,b)|a-b);)*};}
        delta!(
            feels_mean,
            feels_min,
            feels_max,
            temperature_min,
            temperature_mean,
            temperature_max,
            precipitation_sum,
            rain_sum,
            snowfall_sum,
            wind_speed_max,
            cloud_cover,
            humidity,
            pressure
        );
        for (key, value) in &mut day.solar {
            *value = value
                .zip(b.and_then(|b| b.solar.get(key).copied().flatten()))
                .map(|(a, b)| crate::solar::difference(key, a, b));
        }
    }
}

// Reuse the app's previous successful multi-year archive response when present.
// This is a local conversion: no API call, and the old file is retained until all
// complete month summaries have been durably written.
fn import_previous_cache(
    client: &Client,
    city: &City,
    dir: &std::path::Path,
    year: i32,
) -> Result<()> {
    use chrono::Datelike;
    let end = Utc::now().year() - 1;
    if year < end - 29 || year > end {
        return Ok(());
    }
    for years in 1..=30 {
        let start = end - years + 1;
        if year < start {
            continue;
        }
        let mut url = url::Url::parse("https://archive-api.open-meteo.com/v1/archive")?;
        url.query_pairs_mut().extend_pairs([
            ("latitude", city.latitude.to_string()),
            ("longitude", city.longitude.to_string()),
            ("start_date", format!("{start}-01-01")),
            ("end_date", format!("{end}-12-31")),
            ("daily", fields(false)),
            ("hourly", "precipitation".into()),
            ("timezone", "auto".into()),
            ("models", "era5".into()),
        ]);
        let path = client.cache.join(format!(
            "{:x}.json",
            Sha256::digest(url.as_str().as_bytes())
        ));
        if !path.is_file() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
        let daily = &value["data"]["daily"];
        let Some(times) = daily["time"].as_array() else {
            continue;
        };
        let at = value["at"].as_i64().unwrap_or(0);
        let mut complete = 0;
        for y in start..=end {
            let prefix = y.to_string();
            let indices: Vec<_> = times
                .iter()
                .enumerate()
                .filter(|(_, t)| t.as_str().is_some_and(|t| t.starts_with(&prefix)))
                .map(|(i, _)| i)
                .collect();
            let mut subset = serde_json::Map::new();
            if let Some(fields) = daily.as_object() {
                for (k, v) in fields {
                    if let Some(a) = v.as_array() {
                        subset.insert(
                            k.clone(),
                            indices
                                .iter()
                                .map(|&i| a.get(i).cloned().unwrap_or_default())
                                .collect(),
                        );
                    }
                }
            }
            if indices.is_empty() {
                continue;
            }
            for month in aggregate_history(&serde_json::Value::Object(subset))? {
                let count = days(y, month.month);
                if month.sample_counts.get("temperature_2m").copied() != Some(count) {
                    continue;
                }
                let file = dir.join(format!("{y}-{:02}.json", month.month));
                if !file.exists() {
                    atomic_write(
                        &file,
                        &serde_json::to_vec(&Chunk {
                            split_precipitation: false,
                            at,
                            days: count,
                            month,
                        })?,
                    )?;
                }
                complete += 1;
            }
        }
        if complete == (end - start + 1) * 12 {
            let _ = fs::remove_file(path);
        }
        return Ok(());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn chunk(year: i32, month: u32, temp: f64, rain: f64) -> Chunk {
        let n = days(year, month);
        Chunk {
            split_precipitation: true,
            at: 1,
            days: n,
            month: Month {
                month,
                values: BTreeMap::from([
                    ("temperature_2m".into(), Some(temp)),
                    ("precipitation".into(), Some(rain)),
                ]),
                sample_counts: BTreeMap::from([
                    ("temperature_2m".into(), n),
                    ("precipitation".into(), n),
                ]),
                ranges: Ranges::new(),
            },
        }
    }
    #[test]
    fn annual_weighting_and_rain_totals_are_calendar_correct() {
        let chunks: Vec<_> = (1..=12)
            .map(|m| chunk(2024, m, if m == 2 { 0. } else { 10. }, 2.))
            .collect();
        let refs: Vec<_> = chunks.iter().collect();
        let annual = combine(0, &refs, true);
        assert_eq!(annual.values["precipitation"], Some(732.));
        assert!((annual.values["temperature_2m"].unwrap() - 3370. / 366.).abs() < 1e-9);
        assert_eq!(
            combine(2, &[&chunks[1]], false).values["precipitation"],
            Some(58.)
        );
        assert!(!annual.ranges.contains_key("precipitation"));
    }
    #[test]
    fn monthly_cache_is_permanent_and_missing_months_are_not_implied() {
        let tmp = tempfile::tempdir().unwrap();
        let client = Client::new(tmp.path().into(), true);
        let city = City {
            id: "a".into(),
            name: "Tokyo".into(),
            latitude: 35.,
            longitude: 139.,
        };
        let fingerprint = format!(
            "{}:{}:era5:{}",
            city.latitude,
            city.longitude,
            fields(false)
        );
        let dir = tmp
            .path()
            .join("climate-v1")
            .join(format!("{:x}", Sha256::digest(fingerprint.as_bytes())));
        for year in 1950..=1969 {
            let c = chunk(year, 1, 10., 2.);
            atomic_write(
                &dir.join(format!("{year}-01.json")),
                &serde_json::to_vec(&c).unwrap(),
            )
            .unwrap();
        }
        let prefs = Preferences {
            period: Period::Baseline,
            month: 1,
            ..Default::default()
        };
        let row = fetch(&client, &city, &prefs, false, false).unwrap();
        assert_eq!(row.monthly.len(), 1);
        assert_eq!(row.values["precipitation"], Some(62.));
        assert!(row.provenance["era5"].cached && !row.provenance["era5"].stale);
        assert!(fetch(&client, &city, &prefs, true, false).is_err());
        assert!(
            fetch(
                &client,
                &city,
                &Preferences { month: 2, ..prefs },
                false,
                false
            )
            .is_err()
        );
    }
    #[test]
    fn projection_download_resumes_across_quota_windows_and_client_restarts() {
        let tmp = tempfile::tempdir().unwrap();
        let city = City {
            id: "tokyo".into(),
            name: "Tokyo".into(),
            latitude: 35.,
            longitude: 139.,
        };
        let prefs = Preferences {
            period: Period::Future,
            ..Default::default()
        };
        let mut successful = std::collections::BTreeSet::new();
        // A fresh client each time models closing and reopening during a wait.
        for attempt in 0..3 {
            let client = Client::new(tmp.path().into(), true);
            let mut count = 0;
            let result = fetch_using(&client, &city, &prefs, true, false, |_, params| {
                if count == 11 {
                    return Err(crate::rate_limit::Deferred {
                        until: 123,
                        reason: "Test quota".into(),
                    }
                    .into());
                }
                let param = |key| params.iter().find(|(k, _)| *k == key).unwrap().1.clone();
                let start = param("start_date");
                let end = param("end_date");
                assert!(
                    successful.insert((param("models"), start.clone(), end.clone())),
                    "re-downloaded a saved interval"
                );
                count += 1;
                let mut date = NaiveDate::parse_from_str(&start, "%Y-%m-%d").unwrap();
                let end = NaiveDate::parse_from_str(&end, "%Y-%m-%d").unwrap();
                let mut times = vec![];
                while date <= end {
                    times.push(date.to_string());
                    date = date.succ_opt().unwrap();
                }
                let data = serde_json::json!({"daily": {
                    "time": times,
                    "temperature_2m_mean": vec![10.; times.len()],
                    "precipitation_sum": vec![1.; times.len()]
                }});
                Ok((
                    data,
                    Provenance {
                        fetched_at: Utc::now().timestamp(),
                        ..Default::default()
                    },
                ))
            });
            if attempt < 2 {
                let error = result.unwrap_err();
                assert!(
                    error
                        .downcast_ref::<crate::rate_limit::Deferred>()
                        .is_some()
                );
                assert!(error.to_string().contains("monthly samples saved"));
            } else {
                let row = result.unwrap();
                assert_eq!(row.monthly.len(), 12);
                assert_eq!(row.values["temperature_2m"], Some(10.));
            }
        }
        assert_eq!(successful.len(), 30);
        let client = Client::new(tmp.path().into(), true);
        fetch_using(&client, &city, &prefs, true, false, |_, _| {
            panic!("fully cached detail fetched again")
        })
        .unwrap();
        let month = Preferences { month: 2, ..prefs };
        let row = fetch_using(&client, &city, &month, false, false, |_, _| {
            panic!("list failed to reuse detail cache")
        })
        .unwrap();
        assert_eq!(row.monthly.len(), 1);
    }

    #[test]
    fn old_climate_chunks_fetch_only_split_fields_and_resume_after_quota() {
        let tmp = tempfile::tempdir().unwrap();
        let city = City {
            id: "a".into(),
            name: "Tokyo".into(),
            latitude: 35.,
            longitude: 139.,
        };
        let fingerprint = format!(
            "{}:{}:era5:{}",
            city.latitude,
            city.longitude,
            fields(false)
        );
        let dir = tmp
            .path()
            .join("climate-v1")
            .join(format!("{:x}", Sha256::digest(fingerprint.as_bytes())));
        for year in 1950..=1969 {
            let mut c = chunk(year, 1, 10., 2.);
            c.split_precipitation = false;
            atomic_write(
                &dir.join(format!("{year}-01.json")),
                &serde_json::to_vec(&c).unwrap(),
            )
            .unwrap();
        }
        let prefs = Preferences {
            period: Period::Baseline,
            month: 1,
            ..Default::default()
        };
        let mut seen = std::collections::BTreeSet::new();
        for attempt in 0..2 {
            let client = Client::new(tmp.path().into(), true);
            let mut requests = 0;
            let row = fetch_using(&client,&city,&prefs,false,false,|_,params| {
                if attempt == 0 && requests == 3 {
                    return Err(crate::rate_limit::Deferred { until:123, reason:"quota".into() }.into());
                }
                requests += 1;
                let param = |key| params.iter().find(|(k,_)| *k == key).unwrap().1.clone();
                assert_eq!(param("daily"), "rain_sum,snowfall_sum");
                let start = param("start_date");
                assert!(seen.insert(start.clone()), "redownloaded saved split fields");
                let year: i32 = start[..4].parse().unwrap();
                let times: Vec<_> = (1..=31).map(|d| format!("{year}-01-{d:02}")).collect();
                Ok((serde_json::json!({"daily":{"time":times,"rain_sum":vec![1.;31],"snowfall_sum":vec![2.;31]}}), Provenance::default()))
            }).unwrap();
            assert_eq!(row.values["temperature_2m"], Some(10.));
            if attempt == 0 {
                assert_eq!(row.retry_at, Some(123));
                assert_eq!(row.values["rain"], None);
            } else {
                assert_eq!(row.values["rain"], Some(31.));
                assert_eq!(row.values["snowfall"], Some(62.));
            }
        }
        assert_eq!(seen.len(), 20);
        let client = Client::new(tmp.path().into(), true);
        fetch_using(&client, &city, &prefs, false, false, |_, _| {
            panic!("split fields not cached")
        })
        .unwrap();
    }

    #[test]
    fn rain_is_daily_only_and_projection_fields_are_supported_subset() {
        assert_eq!(fields(false).split(',').count(), 19);
        assert_eq!(fields(true).split(',').count(), 8);
        assert!(!fields(true).contains("apparent_temperature"));
        assert!(!fields(true).contains("surface_pressure"));
    }
}
