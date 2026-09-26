use crate::model::{Condition, Mode, Report, Weather};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub fn coordinate_timezone(city: &crate::model::City) -> &'static str {
    static FINDER: std::sync::LazyLock<tzf_rs::EmbeddedFinder> =
        std::sync::LazyLock::new(tzf_rs::EmbeddedFinder::new);
    FINDER.get_tz_name(city.longitude, city.latitude)
}

pub fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}
pub fn fit(text: &str, width: usize) -> String {
    let text = clean(text);
    if text.width() <= width {
        return format!("{}{}", text, " ".repeat(width - text.width()));
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    for g in text.graphemes(true) {
        if out.width() + g.width() > width - 1 {
            break;
        }
        out.push_str(g);
    }
    out.push('…');
    out.push_str(&" ".repeat(width - out.width()));
    out
}
pub fn wrap(lines: &[String], width: usize) -> Vec<String> {
    let mut output = vec![];
    for line in lines {
        let mut chunk = String::new();
        for g in clean(line).graphemes(true) {
            if chunk.width() + g.width() > width.max(1) && !chunk.is_empty() {
                output.push(std::mem::take(&mut chunk));
            }
            chunk.push_str(g);
        }
        output.push(chunk);
    }
    output
}
pub fn number(value: Option<f64>, delta: bool) -> String {
    value
        .map(|n| {
            if delta {
                format!("{n:+.1}")
            } else {
                format!("{n:.1}")
            }
        })
        .unwrap_or_else(|| "—".into())
}
/// Forecast data stays UTC internally; human-facing timestamps use city-local time.
pub fn local_time(time: &str, timezone: &str) -> String {
    let Ok(utc) = chrono::NaiveDateTime::parse_from_str(time, "%Y-%m-%dT%H:%M") else {
        return time.into();
    };
    let zone = timezone.parse::<chrono_tz::Tz>().unwrap_or(chrono_tz::UTC);
    utc.and_utc()
        .with_timezone(&zone)
        .format("%Y-%m-%dT%H:%M")
        .to_string()
}
pub const ICON_WIDTH: u16 = 4;
/// Current wall time in the provider's IANA zone; never guess a missing zone.
pub fn city_now(row: &Weather) -> Option<chrono::DateTime<chrono_tz::Tz>> {
    city_now_at(row, chrono::Utc::now())
}
pub fn city_now_at(
    row: &Weather,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<chrono::DateTime<chrono_tz::Tz>> {
    let zone = row.range_timezone.parse::<chrono_tz::Tz>().ok()?;
    let now = if row.sources.iter().any(|s| s == "demo") {
        chrono::NaiveDateTime::parse_from_str(&row.time, "%Y-%m-%dT%H:%M")
            .ok()
            .map(|t| t.and_utc())
            .unwrap_or(now)
    } else {
        now
    };
    Some(now.with_timezone(&zone))
}
pub fn clock_text(row: &Weather, abbreviation: bool) -> String {
    city_now(row)
        .map(|time| {
            time.format(if abbreviation { "%H:%M %Z" } else { "%H:%M" })
                .to_string()
        })
        .unwrap_or_else(|| "—".into())
}
/// Unicode symbols; text presentation keeps umbrella/lightning terminal-colored.
pub fn weather_symbol(condition: Option<&Condition>) -> &'static str {
    match condition.map(|c| c.code) {
        Some(0 | 1) if condition.is_some_and(|c| c.is_day == Some(false)) => "☾",
        Some(0 | 1) => "☀",
        Some(2) if condition.is_some_and(|c| c.is_day == Some(false)) => "☁",
        Some(2) => "⛅︎",
        Some(3) => "☁",
        Some(45 | 48) => "≋",
        Some(51) => "┊",
        Some(53) => "┊",
        Some(55) => "┊",
        Some(56 | 57) => "┊",
        Some(61) => "☂",
        Some(63) => "☂",
        Some(65) => "☂",
        Some(66 | 67) => "☂",
        Some(80) => "☔︎",
        Some(81) => "☔︎",
        Some(82) => "☔︎",
        Some(71) => "❄",
        Some(73) => "❄",
        Some(75) => "❄",
        Some(77) => "❅",
        Some(85) => "❄",
        Some(86) => "❄",
        Some(95) => "⚡︎",
        Some(96) => "⚡︎",
        Some(99) => "⚡︎",
        _ => "·",
    }
}
pub fn weather_label(condition: Option<&Condition>) -> &'static str {
    match condition.map(|c| c.code) {
        Some(0) => "Clear",
        Some(1) => "Mainly clear",
        Some(2) => "Partly cloudy",
        Some(3) => "Overcast",
        Some(45 | 48) => "Fog",
        Some(51) => "Light drizzle",
        Some(53) => "Moderate drizzle",
        Some(55) => "Dense drizzle",
        Some(56) => "Light freezing drizzle",
        Some(57) => "Dense freezing drizzle",
        Some(61) => "Light rain",
        Some(63) => "Moderate rain",
        Some(65) => "Heavy rain",
        Some(66) => "Light freezing rain",
        Some(67) => "Heavy freezing rain",
        Some(71) => "Light snow",
        Some(73) => "Moderate snow",
        Some(75) => "Heavy snow",
        Some(77) => "Snow grains",
        Some(80) => "Light rain showers",
        Some(81) => "Moderate rain showers",
        Some(82) => "Violent rain showers",
        Some(85) => "Light snow showers",
        Some(86) => "Heavy snow showers",
        Some(95) => "Thunderstorm",
        Some(96) => "Thunderstorm / light hail",
        Some(99) => "Thunderstorm / heavy hail",
        _ => "Unknown",
    }
}
pub fn condition_text(condition: Option<&Condition>) -> String {
    format!("{} {}", weather_symbol(condition), weather_label(condition))
}
pub fn columns(width: u16, mode: Mode) -> Vec<(&'static str, &'static str)> {
    let history = mode == Mode::Historical;
    let mut cols = vec![("temperature_2m", "Temp °C")];
    if width >= 55 {
        cols.extend([
            ("apparent_temperature", "Feels °C"),
            ("rain", if history { "Rain mm/d" } else { "Rain mm" }),
        ]);
    }
    if width >= 80 {
        cols.extend([("snowfall", "Snow cm"), ("wind_speed_10m", "Wind km/h")]);
    }
    if width >= 110 {
        cols.extend([("relative_humidity_2m", "RH %"), ("cloud_cover", "Cloud %")]);
    }
    if width >= 125 {
        cols.push(("local_time", "Time"));
    }
    if width >= 165 {
        cols.extend([("sunrise", "Sunrise"), ("sunset", "Sunset")]);
    }
    if width >= 140 {
        cols.push(("daylight", "Daylight"));
    }
    if width >= 110 {
        cols.push(("surface_pressure", "hPa"));
    }
    cols
}
pub fn metric_text(key: &str, value: Option<f64>, delta: bool) -> String {
    if crate::solar::KEYS.contains(&key) {
        crate::solar::text(key, value, delta)
    } else {
        number(value, delta)
    }
}

pub fn main_column_width(key: &str) -> u16 {
    if key == "sun" { 18 } else { 10 }
}

/// All main-table metrics in stable order; viewport scrolling does not change them.
pub fn main_columns(_width: u16, mode: Mode, _offset: usize) -> Vec<(&'static str, &'static str)> {
    let solar = |key: &str| crate::solar::KEYS.contains(&key);
    let mut all: Vec<_> = columns(180, mode)
        .into_iter()
        .filter(|(key, _)| *key != "local_time" && *key != "surface_pressure" && !solar(key))
        .collect();
    all.push(("sun", "Sun"));
    all.push(("surface_pressure", "hPa"));
    all
}
pub fn range_text(row: &Weather, key: &str, compact: bool) -> String {
    if crate::solar::KEYS.contains(&key) {
        return String::new();
    }
    let Some(range) = row.ranges.get(key) else {
        return "—".into();
    };
    let delta = row.absolute_ranges.is_some();
    let render = |precision: usize| {
        let endpoint = |value: Option<f64>| {
            value
                .map(|n| {
                    let n = if n.abs() < 0.5 * 10_f64.powi(-(precision as i32)) {
                        0.0
                    } else {
                        n
                    };
                    let text = if delta {
                        format!("{n:+.precision$}")
                    } else {
                        format!("{n:.precision$}")
                    };
                    if compact && text.contains('.') {
                        text.trim_end_matches('0').trim_end_matches('.').into()
                    } else {
                        text
                    }
                })
                .unwrap_or_else(|| "—".into())
        };
        format!("{}/{}", endpoint(range.min), endpoint(range.max))
    };
    let precise = render(1);
    if compact && precise.width() > 10 {
        render(0)
    } else {
        precise
    }
}
pub fn name(row: &Weather, reference: &str, current: Option<&str>) -> String {
    format!(
        "{}{}{}{}",
        clean(&row.city.name),
        if row.city.id == reference { " *" } else { "" },
        if Some(row.city.id.as_str()) == current {
            " @"
        } else {
            ""
        },
        if row.flagged() { " !" } else { "" }
    )
}
pub fn summary(report: &Report, width: u16) -> String {
    let cols = columns(width, report.mode);
    let icons = report.mode != Mode::Historical;
    let city_width = usize::from(width)
        .saturating_sub(
            cols.len() * 11
                + if icons {
                    usize::from(ICON_WIDTH + 1)
                } else {
                    0
                },
        )
        .max(1);
    let mut out = format!(
        "Nubila {} · {} · Source: {}\n",
        if report.demo { "[DEMO]" } else { "" },
        report.mode.label(),
        report.source.label()
    );
    out.push_str(&fit("City", city_width));
    if icons {
        out.push_str(&" ".repeat(usize::from(ICON_WIDTH + 1)));
    }
    for (_, label) in &cols {
        out.push_str(&fit(label, 11));
    }
    out.push('\n');
    let pinned = if report.cities.len() > 1 {
        report
            .cities
            .iter()
            .find(|r| r.city.id == report.reference)
            .or_else(|| {
                report
                    .cities
                    .iter()
                    .find(|r| Some(&r.city.id) == report.current_city.as_ref())
            })
    } else {
        None
    };
    for (index, row) in pinned.into_iter().chain(report.cities.iter()).enumerate() {
        out.push_str(&fit(
            &name(row, &report.reference, report.current_city.as_deref()),
            city_width,
        ));
        if icons {
            out.push_str(&fit(
                weather_symbol(row.condition.as_ref()),
                usize::from(ICON_WIDTH + 1),
            ));
        }
        for (key, _) in &cols {
            if *key == "local_time" {
                out.push_str(&fit(&clock_text(row, true), 11));
                continue;
            }
            out.push_str(&fit(
                &metric_text(
                    key,
                    row.values.get(*key).copied().flatten(),
                    report.mode == Mode::Comparison,
                ),
                11,
            ));
        }
        out.push('\n');
        out.push_str(&" ".repeat(city_width));
        if icons {
            out.push_str(&" ".repeat(usize::from(ICON_WIDTH + 1)));
        }
        for (key, _) in &cols {
            if *key == "local_time" {
                out.push_str(&" ".repeat(11));
                continue;
            }
            out.push_str(&fit(&range_text(row, key, true), 11));
        }
        out.push('\n');
        if index == 0 && pinned.is_some() {
            out.push_str(&"─".repeat(usize::from(width)));
            out.push('\n');
        }
    }
    out.push_str(&fit("* ref · @ here · ! status", usize::from(width)));
    out.push('\n');
    out.push_str(&fit(
        if report.mode == Mode::Historical {
            "Typical daily min–max"
        } else if report.mode == Mode::Comparison {
            "Δ min / Δ max vs comparison point"
        } else {
            "Today's local min–max"
        },
        usize::from(width),
    ));
    out.push('\n');
    out
}
pub fn sparkline(values: &[Option<f64>], width: usize) -> String {
    let valid: Vec<_> = values.iter().flatten().copied().collect();
    if valid.is_empty() {
        return "No chart data".into();
    }
    let low = valid.iter().copied().fold(f64::INFINITY, f64::min);
    let high = valid.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let chars: Vec<_> = "▁▂▃▄▅▆▇█".chars().collect();
    let count = width.max(1).min(values.len());
    let chart: String = (0..count)
        .map(|i| {
            values[i * values.len() / count]
                .map(|v| {
                    chars[(((v - low) / (high - low).max(0.01) * 7.0).round() as usize).min(7)]
                })
                .unwrap_or(' ')
        })
        .collect();
    format!("{chart}  {low:.1}/{high:.1} °C")
}
pub const INFO_HEADINGS: &[&str] = &[
    "Location & period",
    "Queue & notices",
    "Daylight",
    "Conditions",
    "Sources & freshness",
    "Measurements",
    "Method & comparison",
];
pub fn overview(row: &Weather) -> Vec<String> {
    let mut lines = vec![
        "Location & period".into(),
        row.city.name.clone(),
        format!(
            "Data time: {} {}",
            local_time(&row.time, &row.range_timezone),
            row.range_timezone,
        ),
    ];
    lines.push(format!("Local time: {}", clock_text(row, true)));
    if row.retry_at.is_some() || row.error.is_some() || !row.warnings.is_empty() {
        lines.extend([String::new(), "Queue & notices".into()]);
    }
    if let Some(at) = row
        .retry_at
        .and_then(|at| chrono::DateTime::from_timestamp(at, 0))
    {
        lines.push(format!(
            "Automatic retry: {}",
            at.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S %Z")
        ));
        lines.push("Completed months cached · only missing data resumes".into());
    }
    if let Some(error) = &row.error {
        lines.push(error.clone());
    }
    lines.extend(row.warnings.clone());
    lines.extend([String::new(), "Daylight".into()]);
    for (key, label) in [
        ("sunrise", "Sunrise"),
        ("sunset", "Sunset"),
        ("daylight", "Daylight"),
    ] {
        lines.push(format!(
            "{label}: {}",
            metric_text(
                key,
                row.values.get(key).copied().flatten(),
                row.absolute_values.is_some()
            )
        ));
    }
    lines.extend([
        "Local astronomical estimates".into(),
        "Climate: period averages".into(),
        "Polar days/nights: no sunrise or sunset".into(),
    ]);
    if row.baseline.is_none() {
        lines.extend([String::new(), "Conditions".into()]);
        lines.push(condition_text(row.condition.as_ref()));
        if let Some(condition) = &row.condition {
            lines.push(format!(
                "Conditions: {} (WMO {})",
                condition.source, condition.code
            ));
        }
    }
    lines.extend([
        String::new(),
        "Sources & freshness".into(),
        format!("Sources: {}", row.sources.join(", ")),
    ]);
    for (source, meta) in &row.provenance {
        let stamp = chrono::DateTime::from_timestamp(meta.fetched_at, 0)
            .map(|t| t.to_rfc3339())
            .unwrap_or_else(|| meta.fetched_at.to_string());
        lines.push(format!(
            "{source}: {} · fetched {stamp}",
            if meta.stale {
                "STALE"
            } else if meta.cached {
                "cached"
            } else {
                "live"
            }
        ));
        if let Some(warning) = &meta.warning {
            lines.push(warning.clone());
        }
    }
    lines.extend([String::new(), "Measurements".into()]);
    if !row.range_date.is_empty() {
        lines.push(format!(
            "Min/max: {} · {}",
            row.range_date, row.range_timezone
        ));
    }
    let labels = [
        ("temperature_2m", "Temperature", "°C"),
        ("apparent_temperature", "Feels like", "°C"),
        ("high", "Mean high", "°C"),
        ("low", "Mean low", "°C"),
        (
            "rain",
            "Rain",
            if row.baseline.is_some() {
                if row.baseline.as_ref().is_some_and(|b| b.month == 0) {
                    "mm/year"
                } else {
                    "mm/month"
                }
            } else {
                "mm"
            },
        ),
        (
            "snowfall",
            "Snowfall",
            if row.baseline.is_some() {
                if row.baseline.as_ref().is_some_and(|b| b.month == 0) {
                    "cm/year"
                } else {
                    "cm/month"
                }
            } else {
                "cm"
            },
        ),
        (
            "precipitation",
            "Precipitation",
            if row.baseline.is_some() {
                if row.baseline.as_ref().is_some_and(|b| b.month == 0) {
                    "mm/year"
                } else {
                    "mm/month"
                }
            } else {
                "mm"
            },
        ),
        ("wind_speed_10m", "Wind", "km/h"),
        (
            "relative_humidity_2m",
            "Humidity",
            if row.absolute_values.is_some() {
                "percentage points"
            } else {
                "%"
            },
        ),
        ("surface_pressure", "Pressure", "hPa"),
        (
            "cloud_cover",
            "Cloud cover",
            if row.absolute_values.is_some() {
                "percentage points"
            } else {
                "%"
            },
        ),
    ];
    for (key, label, unit) in labels {
        if let Some(value) = row.values.get(key) {
            let range = if row.ranges.contains_key(key) {
                format!(
                    "  [{}{}]",
                    range_text(row, key, false),
                    if key == "snowfall" {
                        " cm hourly"
                    } else if crate::model::is_precipitation(key) {
                        " mm hourly"
                    } else {
                        ""
                    }
                )
            } else {
                String::new()
            };
            lines.push(format!(
                "{label}: {} {unit}{range}",
                number(*value, row.absolute_values.is_some())
            ));
        }
    }
    if row.absolute_values.is_some() || row.baseline.is_some() {
        lines.extend([String::new(), "Method & comparison".into()]);
    }
    if row.absolute_values.is_some() {
        lines
            .push("Values, lows and highs: differences from comparison-point values. Hourly stays absolute.".into());
    }
    if let Some(baseline) = &row.baseline {
        lines.push(baseline.method.clone());
    }
    lines
}
pub fn details(row: &Weather, width: usize) -> Vec<String> {
    let mut lines = overview(row);
    if row.error.is_some() {
        return lines;
    }
    lines.push(String::new());
    if row.baseline.is_some() {
        lines.push("Monthly temperature profile".into());
        lines.push(sparkline(
            &row.monthly
                .iter()
                .map(|m| m.values.get("temperature_2m").copied().flatten())
                .collect::<Vec<_>>(),
            width.saturating_sub(20),
        ));
        lines.push("Month      Mean °C     Low °C    High °C    Rain mm    Snow cm  Wind km/h · monthly totals".into());
        for month in &row.monthly {
            lines.push(format!(
                "{:>5} {}",
                month.month,
                [
                    "temperature_2m",
                    "low",
                    "high",
                    "rain",
                    "snowfall",
                    "wind_speed_10m"
                ]
                .map(|k| format!(
                    "{:>11}",
                    number(month.values.get(k).copied().flatten(), false)
                ))
                .join("")
            ));
        }
    } else {
        lines.push(format!(
            "Hourly temperature · {} · absolute",
            row.range_timezone
        ));
        lines.push(sparkline(
            &row.hourly
                .iter()
                .map(|h| h.values.get("temperature_2m").copied().flatten())
                .collect::<Vec<_>>(),
            width.saturating_sub(20),
        ));
        lines.push(format!(
            "{:<17}{}{}",
            "Local time",
            " ".repeat(usize::from(ICON_WIDTH + 1)),
            [
                "Temp °C",
                "Feels °C",
                "Rain mm",
                "Snow cm",
                "Wind km/h",
                "Cloud %"
            ]
            .map(|l| format!("{l:>10}"))
            .join("")
        ));
        for h in &row.hourly {
            lines.push(format!(
                "{:<17}{}{}",
                local_time(&h.time, &row.range_timezone),
                fit(
                    weather_symbol(h.condition.as_ref()),
                    usize::from(ICON_WIDTH + 1)
                ),
                [
                    "temperature_2m",
                    "apparent_temperature",
                    "rain",
                    "snowfall",
                    "wind_speed_10m",
                    "cloud_cover"
                ]
                .map(|k| format!("{:>10}", number(h.values.get(k).copied().flatten(), false)))
                .join("")
            ));
        }
        lines.push(String::new());
        lines.push("Next days · local dates · absolute".into());
        lines.push(format!(
            "{:<11}{}{}",
            "Date",
            " ".repeat(usize::from(ICON_WIDTH + 1)),
            [
                "Low °C",
                "High °C",
                "Feels low",
                "Feels high",
                "Rain mm",
                "Snow cm",
                "Wind max",
                "Cloud %"
            ]
            .map(|l| format!("{l:>10}"))
            .join("")
        ));
        for day in &row.daily {
            lines.push(format!(
                "{} {}{}",
                day.date,
                fit(
                    weather_symbol(day.condition.as_ref()),
                    usize::from(ICON_WIDTH + 1)
                ),
                [
                    day.temperature_min,
                    day.temperature_max,
                    day.feels_min,
                    day.feels_max,
                    day.rain_sum,
                    day.snowfall_sum,
                    day.wind_speed_max,
                    day.cloud_cover
                ]
                .map(|v| format!("{:>10}", number(v, false)))
                .join("")
            ));
        }
    }
    lines
}
pub fn csv(report: &Report) -> String {
    fn escape(v: &str) -> String {
        format!("\"{}\"", v.replace('"', "\"\""))
    }
    let fields: std::collections::BTreeSet<_> = report
        .cities
        .iter()
        .flat_map(|r| r.values.keys().cloned())
        .collect();
    let mut out = vec![
        [
            vec![
                "city_id".into(),
                "city".into(),
                "mode".into(),
                "time".into(),
                "sources".into(),
                "stale".into(),
                "error".into(),
                "weather_code".into(),
                "weather_condition".into(),
                "condition_source".into(),
            ],
            fields.iter().cloned().collect(),
            fields
                .iter()
                .flat_map(|k| {
                    [
                        format!("{k}_min"),
                        format!("{k}_max"),
                        format!("{k}_samples"),
                    ]
                })
                .collect(),
            vec!["range_date".into(), "range_timezone".into()],
        ]
        .concat()
        .iter()
        .map(|s| escape(s))
        .collect::<Vec<_>>()
        .join(","),
    ];
    for row in &report.cities {
        let mut cells = vec![
            row.city.id.clone(),
            row.city.name.clone(),
            report.mode.label().into(),
            row.time.clone(),
            row.sources.join(","),
            row.provenance.values().any(|p| p.stale).to_string(),
            row.error.clone().unwrap_or_default(),
            row.condition
                .as_ref()
                .map(|c| c.code.to_string())
                .unwrap_or_default(),
            row.condition
                .as_ref()
                .map(|c| weather_label(Some(c)).to_string())
                .unwrap_or_default(),
            row.condition
                .as_ref()
                .map(|c| c.source.clone())
                .unwrap_or_default(),
        ];
        cells.extend(fields.iter().map(|k| {
            row.values
                .get(k)
                .copied()
                .flatten()
                .map(|v| v.to_string())
                .unwrap_or_default()
        }));
        for key in &fields {
            let range = row.ranges.get(key);
            cells.push(
                range
                    .and_then(|r| r.min)
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
            );
            cells.push(
                range
                    .and_then(|r| r.max)
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
            );
            cells.push(range.map(|r| r.samples.to_string()).unwrap_or_default());
        }
        cells.extend([row.range_date.clone(), row.range_timezone.clone()]);
        out.push(
            cells
                .iter()
                .map(|s| escape(s))
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    out.join("\n") + "\n"
}
