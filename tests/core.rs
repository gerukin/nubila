use nubila::{
    config::{Config, Preferences, atomic_write},
    model::{City, Mode, Source, compare},
    service::{Client, FetchOptions, Service, aggregate_history, blend, fixture, merge_current},
    view,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use unicode_width::UnicodeWidthStr;

fn city(id: &str, latitude: f64) -> City {
    City {
        id: id.into(),
        name: id.into(),
        latitude,
        longitude: 139.0,
    }
}
fn service() -> Service {
    let config: Config = toml::from_str(nubila::config::EXAMPLE).unwrap();
    Service {
        config,
        options: FetchOptions {
            offline: true,
            refresh: false,
            demo: true,
            no_location: true,
            years: 10,
            month: 9,
            city_ids: vec![],
            past_days: 2,
        },
        client: Client::new(std::env::temp_dir().join("nubila-tests-unused"), true),
    }
}
#[test]
fn period_base_quota_retries_only_while_comparing_and_now_rain_is_daily() {
    use nubila::{model::Period, service::refresh_wait};
    use std::time::Duration;
    let mut service = service();
    service.options.city_ids = vec!["tokyo".into()];
    let prefs = Preferences {
        period: Period::Now,
        comparison_period: Some(Period::Baseline),
        mode: Mode::Comparison,
        ..Default::default()
    };
    let mut report = service.load(&prefs, false).unwrap();
    assert_eq!(report.units["precipitation"], "mm/day");
    let row = &report.cities[0];
    let mut absolute = row.clone();
    absolute.values = row.absolute_values.clone().unwrap();
    assert_eq!(
        row.values["precipitation"],
        nubila::comparison::daily_rain(&absolute)
            .zip(nubila::comparison::daily_rain(
                row.comparison_base.as_ref().unwrap()
            ))
            .map(|(a, b)| a - b)
    );
    let row = &mut report.cities[0];
    row.provenance
        .values_mut()
        .for_each(|p| p.fetched_at = 10_000);
    let base = row.comparison_base.as_mut().unwrap();
    base.error = Some("quota".into());
    base.retry_at = Some(10_120);
    base.provenance.clear();
    assert_eq!(
        refresh_wait(&report, 10_000, Duration::from_secs(1800)),
        Duration::from_secs(120)
    );
    report.present(Mode::Normal, "").unwrap();
    assert_eq!(
        refresh_wait(&report, 10_000, Duration::from_secs(1800)),
        Duration::from_secs(1800)
    );
}

#[test]
fn period_comparison_uses_same_city_and_restores_absolute_solar_values() {
    use nubila::model::Period;
    let mut service = service();
    service.options.city_ids = vec!["paris".into()];
    let prefs = Preferences {
        period: Period::Future,
        comparison_period: Some(Period::Baseline),
        mode: Mode::Comparison,
        reference: "tokyo".into(),
        month: 7,
        ..Default::default()
    };
    let mut report = service.load(&prefs, false).unwrap();
    assert_eq!(report.cities.len(), 1);
    let row = &report.cities[0];
    let base = row.comparison_base.as_ref().unwrap();
    assert_eq!(base.city.id, row.city.id);
    assert!(base.comparison_base.is_none());
    assert!(row.values["daylight"].is_some());
    assert_eq!(report.units["sunrise"], "minutes difference");
    let absolute = row.absolute_values.clone().unwrap();
    let delta = row.values.clone();
    report.present(Mode::Normal, "tokyo").unwrap();
    assert_eq!(report.cities[0].values, absolute);
    report.present(Mode::Comparison, "tokyo").unwrap();
    assert_eq!(report.cities[0].values, delta);
    nubila::climate::select_report(&mut report, 1, Mode::Comparison, "tokyo").unwrap();
    let row = &report.cities[0];
    let base = row.comparison_base.as_ref().unwrap();
    let expected = nubila::solar::difference(
        "sunrise",
        row.absolute_values.as_ref().unwrap()["sunrise"].unwrap(),
        base.values["sunrise"].unwrap(),
    );
    assert_eq!(row.values["sunrise"], Some(expected));
}

#[test]
fn unavailable_climate_details_keep_monthly_placeholders_and_city_scope() {
    let tmp = tempfile::tempdir().unwrap();
    let mut service = service();
    service.client = Client::new(tmp.path().into(), true);
    service.options.demo = false;
    service.options.city_ids = vec!["tokyo".into()];
    let prefs = Preferences {
        period: nubila::model::Period::Future,
        last_city: Some("tokyo".into()),
        ..Default::default()
    };
    let report = service.load(&prefs, false).unwrap();
    assert_eq!(report.cities.len(), 1);
    let row = &report.cities[0];
    assert_eq!(row.city.id, "tokyo");
    assert!(row.error.is_some());
    assert!(row.baseline.is_some());
    assert_eq!(row.monthly.len(), 12);
    assert!(
        row.monthly
            .iter()
            .all(|m| m.values["temperature_2m"].is_none())
    );
    assert!(row.monthly.iter().all(|m| m.values["daylight"].is_some()));
}

#[test]
fn auto_refresh_tracks_source_expiry_and_backs_off_failures() {
    use nubila::{
        model::Provenance,
        service::{HISTORY_TTL, WEATHER_TTL, refresh_wait},
    };
    use std::time::Duration;
    let mut report = service().load(&Preferences::default(), false).unwrap();
    report.cities.truncate(1);
    report.cities[0].provenance.clear();
    report.cities[0].provenance.insert(
        "gfs".into(),
        Provenance {
            fetched_at: 10_000,
            ..Default::default()
        },
    );
    let retry = Duration::from_secs(1700);
    assert_eq!(
        refresh_wait(&report, 11_000, retry),
        Duration::from_secs(800)
    );
    assert_eq!(
        refresh_wait(&report, 10_000 + WEATHER_TTL, retry),
        Duration::ZERO
    );
    report.mode = Mode::Comparison;
    assert_eq!(
        refresh_wait(&report, 10_000 + WEATHER_TTL, retry),
        Duration::ZERO
    );
    report.mode = Mode::Historical;
    assert_eq!(
        refresh_wait(&report, 11_000, retry),
        Duration::from_secs((HISTORY_TTL - 1000) as u64)
    );
    report.mode = Mode::Normal;
    report.cities[0].provenance.get_mut("gfs").unwrap().stale = true;
    assert_eq!(refresh_wait(&report, 12_000, retry), retry);
    report.cities[0].provenance.insert(
        "icon".into(),
        Provenance {
            fetched_at: 10_300,
            ..Default::default()
        },
    );
    assert_eq!(
        refresh_wait(&report, 12_000, retry),
        Duration::from_secs(100)
    );
    report.cities[0].provenance.clear();
    assert_eq!(refresh_wait(&report, 12_000, retry), retry);
}

#[test]
fn startup_has_cities_and_local_time_without_weather_requests() {
    let service = service();
    let started = std::time::Instant::now();
    let report = service.initial(&Preferences::default()).unwrap();
    eprintln!("Offline city preparation: {:?}", started.elapsed());
    assert_eq!(report.cities.len(), service.config.cities.len());
    for row in report.cities {
        assert!(row.time.is_empty());
        assert!(row.range_timezone.parse::<chrono_tz::Tz>().is_ok());
        assert!(view::clock_text(&row, false).contains(':'));
        assert!(row.values.values().all(Option::is_none));
    }
}

#[test]
fn ordinary_requests_reuse_fresh_cache_but_manual_refresh_attempts_network() {
    let tmp = tempfile::tempdir().unwrap();
    let client = Client::new(tmp.path().into(), false);
    let url = "http://127.0.0.1:1/";
    let path = tmp
        .path()
        .join(format!("{:x}.json", Sha256::digest(url.as_bytes())));
    let at = chrono::Utc::now().timestamp() - 1790;
    atomic_write(
        &path,
        &serde_json::to_vec(&json!({"at":at,"data":{"answer":42}})).unwrap(),
    )
    .unwrap();
    let (data, meta) = client.get(url, &[], 900, false).unwrap();
    assert_eq!(data["answer"], 42);
    assert!(meta.cached && !meta.stale && meta.warning.is_none());
    assert_eq!(meta.fetched_at, at);
    let before = fs::read(&path).unwrap();
    let (data, meta) = client.get(url, &[], 900, true).unwrap();
    assert_eq!(data["answer"], 42);
    assert!(meta.cached && meta.stale && meta.warning.is_some());
    assert_eq!(
        fs::read(&path).unwrap(),
        before,
        "Failed forced refresh must preserve the last successful response"
    );
    let expired = chrono::Utc::now().timestamp() - 1801;
    atomic_write(
        &path,
        &serde_json::to_vec(&json!({"at":expired,"data":{"answer":42}})).unwrap(),
    )
    .unwrap();
    let (_, meta) = client.get(url, &[], 1800, false).unwrap();
    assert!(meta.cached && meta.stale && meta.warning.is_some());
}

#[test]
fn comparison_does_not_mutate_reference_during_iteration() {
    let mut rows = vec![
        fixture(&city("a", 35.0), Mode::Normal, 10, 9),
        fixture(&city("b", 49.0), Mode::Normal, 10, 9),
    ];
    compare(&mut rows, "a").unwrap();
    assert_eq!(rows[0].values["temperature_2m"], Some(0.0));
    assert!((rows[1].values["temperature_2m"].unwrap() - 1.4).abs() < 1e-9);
    assert_eq!(
        rows[0].absolute_values.as_ref().unwrap()["temperature_2m"],
        Some(21.5)
    );
    assert_eq!(rows[0].hourly[0].values["temperature_2m"], Some(21.5));
}
#[test]
fn missing_reference_fails_and_missing_values_stay_missing() {
    let mut rows = vec![fixture(&city("a", 35.0), Mode::Normal, 10, 9)];
    assert!(compare(&mut rows, "missing").is_err());
    rows[0].values.insert("precipitation".into(), None);
    compare(&mut rows, "a").unwrap();
    assert_eq!(rows[0].values["precipitation"], None);
}
#[test]
fn blend_aligns_timestamps_and_counts_each_field() {
    let mut a = fixture(&city("a", 35.0), Mode::Normal, 10, 9);
    let mut b = a.clone();
    a.values.insert("temperature_2m".into(), Some(10.0));
    b.values.insert("temperature_2m".into(), Some(20.0));
    a.hourly[0].values.insert("temperature_2m".into(), None);
    b.hourly.reverse();
    let out = blend(&[a.clone(), b.clone()]).unwrap();
    assert_eq!(out.values["temperature_2m"], Some(15.0));
    assert_eq!(out.hourly[0].values["temperature_2m"], Some(21.5));
    assert_eq!(out.hourly[0].contributors["temperature_2m"], 1);
    b.time = "2026-09-20T00:00".into();
    let out = blend(&[a, b]).unwrap();
    assert_eq!(out.values["temperature_2m"], Some(20.0));
    assert!(!out.warnings.is_empty());
}
#[test]
fn monthly_aggregation_ignores_missing_samples() {
    let out=aggregate_history(&json!({"time":["2024-01-01","2024-01-02","2024-02-01"],"temperature_2m_mean":[10,null,30],"precipitation_sum":[0,4,100]})).unwrap();
    assert_eq!(out.len(), 12);
    assert_eq!(out[0].values["temperature_2m"], Some(10.0));
    assert_eq!(out[0].values["precipitation"], Some(2.0));
    assert_eq!(out[0].sample_counts["temperature_2m"], 1);
    assert_eq!(out[2].values["temperature_2m"], None);
}
#[test]
fn current_location_deduplicates_by_distance() {
    let mut cities = vec![city("favorite", 35.0)];
    assert_eq!(merge_current(&mut cities, city("here", 35.01)), "favorite");
    assert_eq!(cities.len(), 1);
    assert_eq!(merge_current(&mut cities, city("here", 50.0)), "here");
    assert_eq!(cities.len(), 2);
}
#[test]
fn atomic_config_validates_before_replacement() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");
    let mut config: Config = toml::from_str(nubila::config::EXAMPLE).unwrap();
    config.write(&path).unwrap();
    let before = fs::read(&path).unwrap();
    config.cities.clear();
    assert!(config.write(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
}
#[test]
fn offline_cache_reports_age_and_corruption_is_a_miss() {
    let tmp = tempfile::tempdir().unwrap();
    let client = Client::new(tmp.path().into(), true);
    let url = "https://example.invalid/";
    let path = tmp
        .path()
        .join(format!("{:x}.json", Sha256::digest(url.as_bytes())));
    atomic_write(
        &path,
        &serde_json::to_vec(&json!({"at":1,"data":{"answer":42}})).unwrap(),
    )
    .unwrap();
    let (data, meta) = client.get(url, &[], 900, false).unwrap();
    assert_eq!(data["answer"], 42);
    assert!(meta.stale && meta.cached);
    fs::write(path, "[1,2]").unwrap();
    assert!(
        client
            .get(url, &[], 900, false)
            .unwrap_err()
            .to_string()
            .contains("Offline")
    );
}
#[test]
fn requested_subset_still_uses_hidden_reference() {
    let mut service = service();
    service.options.city_ids = vec!["paris".into()];
    let prefs = Preferences {
        reference: "tokyo".into(),
        mode: Mode::Comparison,
        ..Default::default()
    };
    let report = service.load(&prefs, false).unwrap();
    assert_eq!(report.cities.len(), 1);
    assert!((report.cities[0].values["temperature_2m"].unwrap() - 1.31804).abs() < 1e-8);
    assert_eq!(report.units["relative_humidity_2m"], "percentage points");
}
#[test]
fn history_units_are_explicit() {
    let service = service();
    let prefs = Preferences {
        reference: "tokyo".into(),
        mode: Mode::Historical,
        ..Default::default()
    };
    let report = service.load(&prefs, false).unwrap();
    assert_eq!(report.units["precipitation"], "mm/day");
    assert_eq!(report.cities[0].monthly.len(), 12);
}
#[test]
fn unicode_and_responsive_output_fit_terminal_cells() {
    assert_eq!(view::fit("東京とParis", 5).width(), 5);
    let service = service();
    let report = service
        .load(
            &Preferences {
                reference: "tokyo".into(),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    for width in [32, 55, 80, 110, 180] {
        for line in view::summary(&report, width).lines().skip(1) {
            assert!(line.width() <= usize::from(width), "{width}: {line}");
        }
    }
}
#[test]
fn source_cycle_is_complete() {
    assert_eq!(Source::Auto.next().next().next().next(), Source::Auto);
}
#[test]
fn json_roundtrip_preserves_full_hourly_data() {
    let service = service();
    let report = service
        .load(
            &Preferences {
                reference: "tokyo".into(),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    let json = serde_json::to_string(&report).unwrap();
    let parsed: nubila::model::Report = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.cities[0].hourly.len(), 48);
    assert_eq!(parsed.schema_version, 1);
}

#[test]
fn local_day_extrema_include_past_hours_and_exclude_adjacent_dates() {
    use chrono::{TimeZone, Utc};
    let start = Utc
        .with_ymd_and_hms(2026, 9, 18, 14, 0, 0)
        .unwrap()
        .timestamp();
    let times: Vec<_> = (0..27).map(|i| start + i * 3600).collect();
    let values: Vec<_> = (0..27)
        .map(|i| {
            if (1..=24).contains(&i) {
                i as f64
            } else {
                999.0
            }
        })
        .collect();
    let mut hourly = json!({"time":times});
    for &(key, _) in nubila::model::FIELDS {
        hourly[key] = json!(values);
    }
    hourly["showers"] = json!(vec![0.; 27]);
    let now = Utc.with_ymd_and_hms(2026, 9, 19, 3, 0, 0).unwrap();
    let data = json!({"timezone":"Asia/Tokyo","current":{"time":now.timestamp(),"temperature_2m":13},"hourly":hourly});
    let out = nubila::service::parse_forecast(&city("tokyo", 35.0), &data, now).unwrap();
    assert_eq!(out.range_date, "2026-09-19");
    for range in out.ranges.values() {
        assert_eq!(range.min, Some(1.0));
        assert_eq!(range.max, Some(24.0));
        assert_eq!(range.samples, 24);
    }
    assert_eq!(out.day_hourly.len(), 24);
    assert_eq!(out.hourly.len(), 27);
    let current = nubila::detail::State::current_hour_at(&out, &out.time);
    assert_eq!(out.hourly[current].time, "2026-09-19T03:00");
    assert!(current > 0);
}
#[test]
fn rain_includes_showers_while_snow_keeps_its_own_depth_and_daily_totals() {
    use chrono::{TimeZone, Utc};
    let now = Utc.with_ymd_and_hms(2026, 9, 19, 3, 0, 0).unwrap();
    let data = json!({
        "timezone":"Asia/Tokyo",
        "current":{"time":now.timestamp(),"rain":1.0,"showers":2.0,"snowfall":4.0,"precipitation":7.0},
        "hourly":{"time":[now.timestamp()],"rain":[1.0],"showers":[2.0],"snowfall":[4.0]},
        "daily":{"time":[now.timestamp(), now.timestamp()+86400],"temperature_2m_mean":[3.0,4.0],"rain_sum":[5.0,6.0],"showers_sum":[2.0,3.0],"snowfall_sum":[8.0,9.0]}
    });
    let out = nubila::service::parse_forecast(&city("tokyo", 35.0), &data, now).unwrap();
    assert_eq!(out.values["rain"], Some(3.0));
    assert_eq!(out.values["snowfall"], Some(4.0));
    assert_eq!(out.values["precipitation"], Some(7.0));
    assert_eq!(out.hourly[0].values["rain"], Some(3.0));
    assert_eq!(out.daily_totals["rain"], Some(7.0));
    assert_eq!(out.daily_totals["snowfall"], Some(8.0));
    assert_eq!(out.daily[0].rain_sum, Some(9.0));
    assert_eq!(out.daily[0].temperature_mean, Some(4.0));
    assert_eq!(out.daily[0].snowfall_sum, Some(9.0));
    let mut missing = data;
    missing["current"]["showers"] = serde_json::Value::Null;
    let out = nubila::service::parse_forecast(&city("tokyo", 35.0), &missing, now).unwrap();
    assert_eq!(out.values["rain"], None);
}

#[test]
fn local_day_handles_twenty_five_hour_dst_day() {
    use chrono::{TimeZone, Utc};
    let start = Utc
        .with_ymd_and_hms(2026, 11, 1, 4, 0, 0)
        .unwrap()
        .timestamp();
    let now = Utc.with_ymd_and_hms(2026, 11, 1, 17, 0, 0).unwrap();
    let data = json!({"timezone":"America/New_York","current":{"time":now.timestamp()},"hourly":{"time":(0..26).map(|i|start+i*3600).collect::<Vec<_>>(),"temperature_2m":(0..26).collect::<Vec<_>>()}});
    let out = nubila::service::parse_forecast(&city("ny", 40.0), &data, now).unwrap();
    assert_eq!(out.ranges["temperature_2m"].samples, 25);
    assert_eq!(out.ranges["temperature_2m"].max, Some(24.0));
}
#[test]
fn comparisons_subtract_matching_extrema_without_sorting_deltas() {
    use nubila::model::Range;
    let mut a = fixture(&city("a", 35.0), Mode::Normal, 10, 9);
    let mut b = fixture(&city("b", 45.0), Mode::Normal, 10, 9);
    a.ranges.insert(
        "temperature_2m".into(),
        Range {
            min: Some(12.0),
            max: Some(25.0),
            samples: 24,
        },
    );
    b.ranges.insert(
        "temperature_2m".into(),
        Range {
            min: Some(20.0),
            max: Some(22.0),
            samples: 24,
        },
    );
    let mut rows = vec![a, b];
    compare(&mut rows, "a").unwrap();
    assert_eq!(rows[0].ranges["temperature_2m"].min, Some(0.0));
    assert_eq!(rows[1].ranges["temperature_2m"].min, Some(8.0));
    assert_eq!(rows[1].ranges["temperature_2m"].max, Some(-3.0));
    assert_eq!(
        rows[1].absolute_ranges.as_ref().unwrap()["temperature_2m"].max,
        Some(22.0)
    );
}
#[test]
fn blended_ranges_are_extrema_of_blended_curve() {
    let mut a = fixture(&city("a", 35.0), Mode::Normal, 10, 9);
    let mut b = a.clone();
    a.day_hourly.truncate(2);
    b.day_hourly.truncate(2);
    for (i, (x, y)) in [(0.0, 10.0), (10.0, 0.0)].into_iter().enumerate() {
        a.day_hourly[i]
            .values
            .insert("temperature_2m".into(), Some(x));
        b.day_hourly[i]
            .values
            .insert("temperature_2m".into(), Some(y));
    }
    let out = blend(&[a, b]).unwrap();
    assert_eq!(out.ranges["temperature_2m"].min, Some(5.0));
    assert_eq!(out.ranges["temperature_2m"].max, Some(5.0));
}
#[test]
fn historical_ranges_are_mean_daily_extrema_not_record_extremes() {
    let out=aggregate_history(&json!({"time":["2024-09-01","2025-09-02","2025-10-01"],"temperature_2m_min":[10,20,-50],"temperature_2m_max":[30,60,100],"surface_pressure_min":[1000,null,900]})).unwrap();
    assert_eq!(out[8].ranges["temperature_2m"].min, Some(15.0));
    assert_eq!(out[8].ranges["temperature_2m"].max, Some(45.0));
    assert_eq!(out[8].ranges["temperature_2m"].samples, 2);
    assert_eq!(out[8].ranges["surface_pressure"].min, Some(1000.0));
    assert_eq!(out[8].ranges["surface_pressure"].max, None);
}
#[test]
fn historical_rain_extrema_skip_incomplete_days() {
    let mut months = aggregate_history(&json!({"time":["2024-09-01"]})).unwrap();
    let mut times = vec![];
    let mut rain = vec![];
    for day in 1..=3 {
        for hour in 0..24 {
            times.push(format!("2024-09-{day:02}T{hour:02}:00"));
            rain.push(if day == 3 && hour == 0 {
                None
            } else {
                Some(if hour == 12 { day as f64 * 2.0 } else { 0.0 })
            });
        }
    }
    nubila::service::aggregate_rain_ranges(
        &mut months,
        &json!({"time":times,"precipitation":rain}),
    );
    assert_eq!(months[8].ranges["precipitation"].min, Some(0.0));
    assert_eq!(months[8].ranges["precipitation"].max, Some(3.0));
    assert_eq!(months[8].ranges["precipitation"].samples, 2);
}
#[test]
fn every_mode_exports_all_main_ranges_in_json_and_csv() {
    for mode in [Mode::Normal, Mode::Comparison, Mode::Historical] {
        let report = service()
            .load(
                &Preferences {
                    mode,
                    reference: "tokyo".into(),
                    ..Default::default()
                },
                false,
            )
            .unwrap();
        for row in &report.cities {
            for &(key, _) in nubila::model::FIELDS {
                if mode == Mode::Historical && matches!(key, "rain" | "snowfall") {
                    assert!(!row.ranges.contains_key(key));
                    continue;
                }
                assert!(row.ranges[key].min.is_some(), "{mode:?}: {key}");
                assert!(row.ranges[key].max.is_some());
            }
        }
        let csv = view::csv(&report);
        assert!(csv.contains("temperature_2m_min"));
        assert!(csv.contains("cloud_cover_max"));
        let value = serde_json::to_value(&report).unwrap();
        assert!(value["cities"][0]["ranges"]["temperature_2m"]["min"].is_number());
        let table = view::summary(&report, 110);
        assert!(
            table
                .lines()
                .nth(1)
                .unwrap()
                .trim_start()
                .starts_with("City ")
        );
        assert!(!table.contains("City (*"));
    }
}

#[test]
fn forecast_keeps_full_horizon_and_local_daily_conditions() {
    use chrono::{TimeZone, Utc};
    let now = Utc.with_ymd_and_hms(2026, 9, 19, 0, 0, 0).unwrap();
    let midnight = now.timestamp() - 9 * 3600;
    let data = json!({
        "timezone":"Asia/Tokyo",
        "current":{"time":now.timestamp(),"weather_code":0,"is_day":0},
        "hourly":{"time":(0..72).map(|i|now.timestamp()+i*3600).collect::<Vec<_>>(),
                  "weather_code":vec![71;72]},
        "daily":{"time":[midnight,midnight+86400,midnight+172800],
                 "weather_code":[0,61,null],"cloud_cover_mean":[10,80,null],
                 "temperature_2m_min":[12,13,null]}
    });
    let out = nubila::service::parse_forecast(&city("tokyo", 35.0), &data, now).unwrap();
    assert_eq!(out.hourly.len(), 72);
    assert_eq!(view::weather_symbol(out.condition.as_ref()), "☾");
    assert_eq!(view::weather_symbol(out.hourly[0].condition.as_ref()), "❄");
    assert_eq!(out.daily.len(), 1);
    assert_eq!(out.daily[0].date, "2026-09-20");
    assert_eq!(out.daily[0].cloud_cover, Some(80.0));
    assert_eq!(out.daily[0].condition.as_ref().unwrap().code, 61);
}

#[test]
fn blended_conditions_preserve_source_and_daily_numbers_are_averaged() {
    let mut a = fixture(&city("a", 35.0), Mode::Normal, 10, 9);
    let mut b = a.clone();
    a.condition.as_mut().unwrap().code = 0;
    a.condition.as_mut().unwrap().source = "gfs".into();
    b.condition.as_mut().unwrap().code = 95;
    a.daily[0].temperature_min = Some(10.0);
    b.daily[0].temperature_min = Some(20.0);
    let out = blend(&[a.clone(), b]).unwrap();
    assert_eq!(out.condition, a.condition);
    assert_eq!(out.daily[0].temperature_min, Some(15.0));
    assert_eq!(out.daily.len(), 15);
    let history = fixture(&city("a", 35.0), Mode::Historical, 10, 9);
    assert!(history.condition.is_none() && history.daily.is_empty());
    assert!(history.values["cloud_cover"].is_some());
}

#[test]
fn sort_all_columns_missing_last_and_stable_ties() {
    use nubila::sort::Sort;
    let mut a = fixture(&city("a", 35.0), Mode::Normal, 10, 9);
    let mut b = a.clone();
    b.city.id = "b".into();
    b.city.name = "b".into();
    for sort in Sort::ALL.into_iter().filter(|s| *s != Sort::City) {
        a.range_timezone = "Etc/GMT-1".into();
        b.range_timezone = "Etc/GMT-2".into();
        for (row, value) in [(&mut a, 10.0), (&mut b, 20.0)] {
            for range in row.ranges.values_mut() {
                range.min = Some(value);
                range.max = Some(value);
            }
        }
        a.values.insert(sort.key().into(), Some(10.0));
        b.values.insert(sort.key().into(), Some(20.0));
        a.condition.as_mut().unwrap().code = 1;
        b.condition = a.condition.clone();
        b.condition.as_mut().unwrap().code = 3;
        assert!(sort.compare(&a, &b, false).is_lt());
        assert!(sort.compare(&a, &b, true).is_gt());
        b.values.insert(sort.key().into(), None);
        b.condition = None;
        b.range_timezone.clear();
        for range in b.ranges.values_mut() {
            range.min = None;
            range.max = None;
        }
        assert!(sort.compare(&a, &b, true).is_lt());
        b.values.insert(sort.key().into(), Some(10.0));
        b.condition = a.condition.clone();
        b.range_timezone = a.range_timezone.clone();
        b.ranges = a.ranges.clone();
        assert!(sort.compare(&a, &b, true).is_lt());
    }
    let mut config: Config = toml::from_str(nubila::config::EXAMPLE).unwrap();
    config.reference.clear();
    assert!(config.validate().is_ok());
}

#[test]
fn intensity_colors_respect_units_boundaries_and_monochrome() {
    use nubila::theme::Theme;
    use ratatui::style::Color;
    let t = Theme::new(false);
    for (v, c) in [
        (-1.0, Color::Blue),
        (0.0, Color::Cyan),
        (25.0, Color::Yellow),
        (29.9, Color::Yellow),
        (30.0, Color::Red),
        (35.0, Color::Magenta),
    ] {
        assert_eq!(t.metric("temperature_2m", Some(v), false).fg, Some(c));
    }
    assert_eq!(
        t.metric("precipitation", Some(8.0), false).fg,
        Some(Color::LightBlue)
    );
    assert_eq!(
        t.metric("precipitation", Some(8.0), true).fg,
        Some(Color::Cyan)
    );
    assert_eq!(
        t.metric("wind_speed_10m", Some(60.0), false).fg,
        Some(Color::Magenta)
    );
    assert_eq!(
        Theme::new(true)
            .metric("temperature_2m", Some(40.0), false)
            .fg,
        None
    );
}

#[test]
fn precipitation_symbols_preserve_type_and_available_intensity_in_compact_cells() {
    use nubila::model::Condition;
    for (code, icon, label) in [
        (51, "┊", "Light drizzle"),
        (55, "┊", "Dense drizzle"),
        (61, "☂", "Light rain"),
        (63, "☂", "Moderate rain"),
        (65, "☂", "Heavy rain"),
        (80, "☔︎", "Light rain showers"),
        (82, "☔︎", "Violent rain showers"),
        (71, "❄", "Light snow"),
        (73, "❄", "Moderate snow"),
        (75, "❄", "Heavy snow"),
        (77, "❅", "Snow grains"),
        (85, "❄", "Light snow showers"),
        (86, "❄", "Heavy snow showers"),
    ] {
        let c = Condition {
            code,
            is_day: Some(true),
            source: "test".into(),
        };
        assert_eq!(view::weather_symbol(Some(&c)), icon);
        assert_eq!(view::weather_label(Some(&c)), label);
        assert!(icon.width() <= usize::from(view::ICON_WIDTH));
    }
}

#[test]
fn neutral_primary_values_and_secondary_intensity_are_independent() {
    use nubila::theme::Theme;
    use ratatui::style::Modifier;
    let theme = Theme::new(false);
    for (key, v) in [
        ("temperature_2m", 20.0),
        ("precipitation", 0.0),
        ("wind_speed_10m", 10.0),
        ("relative_humidity_2m", 60.0),
        ("surface_pressure", 1010.0),
        ("cloud_cover", 40.0),
    ] {
        let style = theme.metric(key, Some(v), false);
        assert!(style.fg.is_none());
        assert!(!style.add_modifier.contains(Modifier::DIM));
    }
    let line = theme.secondary_range("-2/30", "temperature_2m", Some(-2.0), Some(30.0), false);
    assert!(
        line.spans
            .iter()
            .all(|s| s.style.add_modifier.contains(Modifier::DIM))
    );
    assert_eq!(
        line.spans[0].style.fg,
        theme.metric("temperature_2m", Some(-2.0), false).fg
    );
}

#[test]
fn local_hour_handles_rollover_and_dst_and_now_ignores_stale_updates() {
    use chrono::{Duration, Utc};
    assert_eq!(
        view::local_time("2026-09-19T07:00", "Asia/Tokyo"),
        "2026-09-19T16:00"
    );
    assert_eq!(
        view::local_time("2026-09-19T20:00", "Asia/Tokyo"),
        "2026-09-20T05:00"
    );
    assert_eq!(
        view::local_time("2026-03-08T06:00", "America/New_York"),
        "2026-03-08T01:00"
    );
    assert_eq!(
        view::local_time("2026-03-08T07:00", "America/New_York"),
        "2026-03-08T03:00"
    );
    let mut row = fixture(&city("a", 35.0), Mode::Normal, 10, 9);
    row.sources = vec!["auto".into()];
    let now = Utc::now();
    for (i, h) in row.hourly.iter_mut().enumerate() {
        h.time = (now + Duration::hours(i as i64 - 24))
            .format("%Y-%m-%dT%H:00")
            .to_string();
    }
    row.time = row.hourly[0].time.clone(); // stale model timestamp
    assert_eq!(nubila::detail::State::current_hour(&row), 24);
}

#[test]
fn graph_types_and_bounds_match_measurements() {
    use nubila::detail::Metric;
    use ratatui::widgets::GraphType;
    assert_eq!(Metric::Temperature.graph_type(), GraphType::Line);
    assert_eq!(Metric::Wind.graph_type(), GraphType::Line);
    assert_eq!(Metric::Rain.graph_type(), GraphType::Area);
    assert_eq!(Metric::Cloud.graph_type(), GraphType::Area);
    assert_eq!(
        Metric::Cloud.bounds(&[Some(30.0), Some(50.0)]),
        [0.0, 100.0]
    );
    assert_eq!(Metric::Rain.bounds(&[Some(0.0), None]), [0.0, 1.0]);
    assert_eq!(
        Metric::Temperature.bounds(&[Some(-4.0), Some(2.0)]),
        [-4.5, 2.5]
    );
    assert_eq!(Metric::Rain.unit(true), "mm/month");
    assert_eq!(Metric::Rain.unit(false), "mm");
}

#[test]
fn graph_colors_cross_thresholds_between_samples_and_preserve_gaps() {
    use nubila::{
        detail::{Metric, graph_segments},
        theme::Theme,
    };
    use ratatui::style::Color;
    let theme = Theme::new(false);
    for values in [[Some(-5.0), Some(40.0)], [Some(40.0), Some(-5.0)]] {
        let segments = graph_segments(Metric::Temperature, &values, false);
        assert_eq!(segments.len(), 6);
        let mut colors = segments
            .iter()
            .map(|s| theme.metric("temperature_2m", Some(s.value), false).fg)
            .collect::<Vec<_>>();
        if values[0] > values[1] {
            colors.reverse();
        }
        assert_eq!(
            colors,
            vec![
                Some(Color::Blue),
                Some(Color::Cyan),
                None,
                Some(Color::Yellow),
                Some(Color::Red),
                Some(Color::Magenta)
            ]
        );
        for pair in segments.windows(2) {
            assert_eq!(pair[0].points.last(), pair[1].points.first());
        }
        for segment in &segments {
            for &(x, y) in &segment.points {
                let expected = values[0].unwrap() + x * (values[1].unwrap() - values[0].unwrap());
                assert!((y - expected).abs() < 1e-9);
            }
        }
    }
    let segments = graph_segments(
        Metric::Wind,
        &[Some(10.0), None, Some(70.0), Some(10.0)],
        false,
    );
    assert_eq!(segments.len(), 5);
    assert!(
        segments
            .iter()
            .all(|s| s.points.iter().all(|p| p.0 == 0.0) || s.points.iter().all(|p| p.0 >= 2.0))
    );
    assert!(graph_segments(Metric::Temperature, &[None, Some(f64::NAN)], false).is_empty());
}

#[test]
fn filled_rain_bars_use_peak_color_for_the_entire_bar() {
    use nubila::{
        detail::{Metric, graph_segments},
        theme::Theme,
    };
    use ratatui::style::Color;
    let values = [Some(1.0), Some(5.0), Some(10.0)];
    let bars = graph_segments(Metric::Rain, &values, false);
    let theme = Theme::new(false);
    for ((bar, value), color) in
        bars.iter()
            .zip(values)
            .zip([Color::Cyan, Color::Blue, Color::LightBlue])
    {
        assert_eq!(bar.value, value.unwrap());
        assert!(bar.points.iter().all(|p| p.1 == bar.value));
        assert_eq!(
            theme.metric("precipitation", Some(bar.value), false).fg,
            Some(color)
        );
        assert_eq!(
            Theme::new(true)
                .metric("precipitation", Some(bar.value), false)
                .fg,
            None
        );
    }
    let cloud = graph_segments(Metric::Cloud, &[Some(10.0), Some(90.0)], false);
    assert_eq!(cloud.len(), 1);
    assert_eq!(
        theme.metric("cloud_cover", Some(cloud[0].value), false).fg,
        None
    );
}

#[test]
fn colored_graph_segments_preserve_all_braille_dots_at_shared_cells() {
    use nubila::detail::render_graph;
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        style::{Color, Style},
        symbols::Marker,
        widgets::{Axis, Block, Chart, Dataset, GraphType, Widget},
    };
    for kind in [GraphType::Line, GraphType::Area] {
        for width in [3, 9, 32] {
            let area = Rect::new(0, 0, width, 5);
            let points = [(0.0, 0.5), (3.0, 0.5)];
            let slices = [
                [(0.0, 0.5), (1.0, 0.5)],
                [(1.0, 0.5), (2.0, 0.5)],
                [(2.0, 0.5), (3.0, 0.5)],
            ];
            let x = Axis::default().bounds([0.0, 3.0]);
            let y = Axis::default().bounds([0.0, 1.0]);
            let dataset = |data| {
                Dataset::default()
                    .data(data)
                    .marker(Marker::Braille)
                    .graph_type(kind)
            };
            let mut expected = Buffer::empty(area);
            Chart::new(vec![dataset(&points)])
                .x_axis(x.clone())
                .y_axis(y.clone())
                .render(area, &mut expected);
            let mut actual = Buffer::empty(area);
            render_graph(
                slices
                    .iter()
                    .zip([Color::Cyan, Color::Yellow, Color::Red])
                    .map(|(points, color)| dataset(points).style(Style::default().fg(color)))
                    .collect(),
                Block::default(),
                x,
                y,
                area,
                &mut actual,
            );
            for (a, b) in actual.content.iter().zip(&expected.content) {
                assert_eq!(
                    a.symbol(),
                    b.symbol(),
                    "missing dots at width {width}, {kind:?}"
                );
            }
            assert!(actual.content.iter().any(|c| c.fg == Color::Red));
        }
    }
}

#[test]
fn zero_reference_preserves_temperature_curve_dots_at_crossings() {
    use nubila::detail::{Metric, graph_segments, render_graph};
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        style::{Color, Style},
        symbols::Marker,
        widgets::{Axis, Block, Dataset, GraphType},
    };
    for width in [9, 32, 90] {
        let area = Rect::new(0, 0, width, 12);
        let segments = graph_segments(
            Metric::Temperature,
            &[Some(-5.0), Some(5.0), Some(-3.0)],
            false,
        );
        let zero = [(0.0, 0.0), (2.0, 0.0)];
        let dataset = |points| {
            Dataset::default()
                .data(points)
                .graph_type(GraphType::Line)
                .marker(Marker::Braille)
        };
        let render = |datasets| {
            let mut buffer = Buffer::empty(area);
            render_graph(
                datasets,
                Block::default(),
                Axis::default().bounds([0.0, 2.0]),
                Axis::default().bounds([-6.0, 6.0]),
                area,
                &mut buffer,
            );
            buffer
        };
        let curve = || {
            segments
                .iter()
                .map(|s| dataset(&s.points).style(Style::default().fg(Color::Cyan)))
        };
        let weather = render(curve().collect());
        let reference = render(vec![dataset(&zero)]);
        let combined = render(
            std::iter::once(dataset(&zero).style(Style::default().fg(Color::DarkGray)))
                .chain(curve())
                .collect(),
        );
        let mask = |cell: &ratatui::buffer::Cell| {
            let c = cell.symbol().chars().next().unwrap_or(' ') as u32;
            if (0x2800..=0x28ff).contains(&c) {
                c & 0xff
            } else {
                0
            }
        };
        for ((actual, weather), reference) in combined
            .content
            .iter()
            .zip(&weather.content)
            .zip(&reference.content)
        {
            assert_eq!(
                mask(actual),
                mask(weather) | mask(reference),
                "lost dots at width {width}"
            );
            if mask(weather) != 0 {
                assert_eq!(actual.fg, Color::Cyan);
            }
        }
    }
}

#[test]
fn local_clock_uses_city_dst_and_unknown_zones_stay_unknown() {
    let mut row = fixture(&city("ny", 40.7), Mode::Normal, 10, 9);
    row.range_timezone = "America/New_York".into();
    row.time = "2026-07-01T07:00".into();
    assert_eq!(view::clock_text(&row, true), "03:00 EDT");
    row.time = "2026-01-01T07:00".into();
    assert_eq!(view::clock_text(&row, true), "02:00 EST");
    row.range_timezone = "local".into();
    assert_eq!(view::clock_text(&row, true), "—");
}

#[test]
fn cities_are_published_before_the_slowest_city_finishes() {
    use std::sync::{Mutex, mpsc};
    use std::time::Duration;
    let service = service();
    let (updates, received) = mpsc::channel();
    let (release, blocked) = mpsc::sync_channel(1);
    let blocked = Mutex::new(blocked);
    let worker = std::thread::spawn(move || {
        service
            .load_progress(&Preferences::default(), false, |row| {
                updates.send(row.city.id.clone()).unwrap();
                if row.city.id == "tokyo" {
                    blocked
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(3))
                        .unwrap();
                }
            })
            .unwrap()
    });
    let mut ids = Vec::new();
    while !ids.iter().any(|id| id == "tokyo") || !ids.iter().any(|id| id == "paris") {
        ids.push(received.recv_timeout(Duration::from_secs(2)).unwrap());
    }
    assert!(
        !worker.is_finished(),
        "Paris was published while Tokyo was still pending"
    );
    release.send(()).unwrap();
    assert_eq!(worker.join().unwrap().cities.len(), 3);
}

#[test]
fn visible_city_is_published_first_once_and_report_keeps_all_cities() {
    let service = service();
    let updates = std::sync::Mutex::new(Vec::new());
    let report = service
        .load_progress_prioritized(&Preferences::default(), false, Some("paris"), |row| {
            updates.lock().unwrap().push(row.city.id.clone());
        })
        .unwrap();
    let updates = updates.into_inner().unwrap();
    assert_eq!(updates.first().map(String::as_str), Some("paris"));
    assert_eq!(updates.iter().filter(|id| *id == "paris").count(), 1);
    assert_eq!(updates.len(), service.config.cities.len());
    assert_eq!(
        report.cities.iter().map(|r| &r.city.id).collect::<Vec<_>>(),
        service
            .config
            .cities
            .iter()
            .map(|c| &c.id)
            .collect::<Vec<_>>()
    );
}

#[test]
fn historical_cache_is_fresh_for_six_months() {
    let tmp = tempfile::tempdir().unwrap();
    let client = Client::new(tmp.path().into(), false);
    let url = "http://127.0.0.1:1/";
    let path = tmp
        .path()
        .join(format!("{:x}.json", Sha256::digest(url.as_bytes())));
    let at = chrono::Utc::now().timestamp() - 179 * 86400;
    atomic_write(
        &path,
        &serde_json::to_vec(&json!({"at":at,"data":{"answer":42}})).unwrap(),
    )
    .unwrap();
    let (_, meta) = client
        .get(url, &[], nubila::service::HISTORY_TTL, false)
        .unwrap();
    assert!(meta.cached && !meta.stale && meta.warning.is_none());
    assert_eq!(nubila::service::HISTORY_TTL, 180 * 86400);
}

#[test]
fn rate_limit_errors_preserve_reason_and_retry_without_caching_failure() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        for (status, body) in [
            (
                "429 Too Many Requests",
                r#"{"error":true,"reason":"Hourly API request limit exceeded"}"#,
            ),
            ("200 OK", r#"{"answer":42}"#),
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut buffer = [0; 4096];
            assert!(stream.read(&mut buffer).unwrap() > 0);
            write!(stream, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nRetry-After: 120\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
    });
    let tmp = tempfile::tempdir().unwrap();
    let client = Client::new(tmp.path().into(), false);
    let error = client.get(&url, &[], 1800, false).unwrap_err().to_string();
    assert!(error.contains("HTTP 429: Hourly API request limit exceeded"));
    assert!(error.contains("retry after"));
    assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 0);
    let (data, meta) = client.get(&url, &[], 1800, false).unwrap();
    assert_eq!(data["answer"], 42);
    assert!(!meta.cached);
    server.join().unwrap();
}

#[test]
fn deferred_history_uses_its_deadline_instead_of_generic_retry_delay() {
    let mut report = service().load(&Preferences::default(), false).unwrap();
    report.cities.truncate(1);
    report.cities[0].provenance.clear();
    report.cities[0].retry_at = Some(1061);
    assert_eq!(
        nubila::service::refresh_wait(&report, 1001, std::time::Duration::from_secs(1800)),
        std::time::Duration::from_secs(60)
    );
    assert_eq!(
        nubila::service::refresh_wait(&report, 1061, std::time::Duration::from_secs(1800)),
        std::time::Duration::ZERO
    );
}

#[test]
fn daily_temperature_graph_values_compare_corresponding_statistics() {
    let mut row = nubila::service::fixture(&city("a", 10.0), Mode::Normal, 5, 9);
    let mut base = nubila::service::fixture(&city("b", 20.0), Mode::Normal, 5, 9);
    row.daily[0].temperature_mean = Some(20.);
    row.daily[0].temperature_min = Some(12.);
    row.daily[0].temperature_max = Some(25.);
    base.daily[0].temperature_mean = Some(16.);
    base.daily[0].temperature_min = Some(13.);
    base.daily[0].temperature_max = Some(22.);
    nubila::climate::compare_details(&mut row, &base);
    assert_eq!(row.daily[0].temperature_mean, Some(4.));
    assert_eq!(row.daily[0].temperature_min, Some(-1.));
    assert_eq!(row.daily[0].temperature_max, Some(3.));
}

#[test]
fn forecast_daily_feels_like_preserves_provider_values_and_missing_data() {
    let city = service().config.cities[0].clone();
    let now = chrono::DateTime::parse_from_rfc3339("2026-09-26T12:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let payload = serde_json::json!({
        "timezone":"UTC",
        "current":{"time":now.timestamp()},
        "hourly":{"time":[]},
        "daily":{"time":[now.timestamp()+43200,now.timestamp()+129600],
            "temperature_2m_mean":[20.0,21.0],
            "apparent_temperature_mean":[18.5,null],
            "apparent_temperature_min":[12.5,null],
            "apparent_temperature_max":[24.5,null]}
    });
    let row = nubila::service::parse_forecast(&city, &payload, now).unwrap();
    assert_eq!(row.daily.len(), 2);
    assert_eq!(
        (
            row.daily[0].feels_mean,
            row.daily[0].feels_min,
            row.daily[0].feels_max
        ),
        (Some(18.5), Some(12.5), Some(24.5))
    );
    assert_eq!(row.daily[1].feels_mean, None);
    let json = serde_json::to_value(&row.daily[0]).unwrap();
    assert_eq!(json["feels_mean"], 18.5);
    let mut other = row.clone();
    other.daily[0].feels_mean = Some(22.5);
    other.daily[0].feels_min = Some(16.5);
    other.daily[0].feels_max = Some(28.5);
    let blended = nubila::service::blend(&[row.clone(), other.clone()]).unwrap();
    assert_eq!(blended.daily[0].feels_mean, Some(20.5));
    let mut compared = other;
    nubila::climate::compare_details(&mut compared, &row);
    assert_eq!(
        (
            compared.daily[0].feels_mean,
            compared.daily[0].feels_min,
            compared.daily[0].feels_max
        ),
        (Some(4.0), Some(4.0), Some(4.0))
    );
    assert_eq!(compared.daily[1].feels_mean, None);
}
