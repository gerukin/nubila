//! Local solar estimates: NOAA fractional-year equations, no API requests.
//! https://gml.noaa.gov/grad/solcalc/solareqns.PDF
use crate::model::{City, Values, Weather};
use chrono::{Datelike, NaiveDate, Offset, TimeZone};

pub const KEYS: [&str; 3] = ["sunrise", "sunset", "daylight"];

/// Clock times are minutes after local midnight; duration is minutes.
pub fn day(city: &City, zone: &str, date: NaiveDate) -> Values {
    let gamma = std::f64::consts::TAU * f64::from(date.ordinal0())
        / if date.leap_year() { 366.0 } else { 365.0 };
    let eq = 229.18
        * (0.000075 + 0.001868 * gamma.cos()
            - 0.032077 * gamma.sin()
            - 0.014615 * (2.0 * gamma).cos()
            - 0.040849 * (2.0 * gamma).sin());
    let decl = 0.006918 - 0.399912 * gamma.cos() + 0.070257 * gamma.sin()
        - 0.006758 * (2.0 * gamma).cos()
        + 0.000907 * (2.0 * gamma).sin()
        - 0.002697 * (3.0 * gamma).cos()
        + 0.00148 * (3.0 * gamma).sin();
    let lat = city.latitude.to_radians();
    let cos = 90.833_f64.to_radians().cos() / (lat.cos() * decl.cos()) - lat.tan() * decl.tan();
    let angle = cos.clamp(-1.0, 1.0).acos().to_degrees();
    let noon = 720.0 - 4.0 * city.longitude - eq;
    let zone = zone.parse::<chrono_tz::Tz>().ok();
    let local = |minutes: f64| {
        let utc = date.and_hms_opt(0, 0, 0)?.and_utc()
            + chrono::Duration::seconds((minutes * 60.0).round() as i64);
        let offset = zone?
            .offset_from_utc_datetime(&utc.naive_utc())
            .fix()
            .local_minus_utc();
        Some((minutes + f64::from(offset) / 60.0).rem_euclid(1440.0))
    };
    Values::from([
        (
            "sunrise".into(),
            if cos.abs() <= 1.0 {
                local(noon - 4.0 * angle)
            } else {
                None
            },
        ),
        (
            "sunset".into(),
            if cos.abs() <= 1.0 {
                local(noon + 4.0 * angle)
            } else {
                None
            },
        ),
        ("daylight".into(), Some(8.0 * angle)),
    ])
}

fn average(city: &City, zone: &str, years: (i32, i32), month: u32) -> Values {
    let mut sums = [0.0; 3];
    let mut cosines = [0.0; 2];
    let mut counts = [0usize; 3];
    for year in years.0..=years.1 {
        let mut date = NaiveDate::from_ymd_opt(year, month.max(1), 1).unwrap();
        while date.year() == year && (month == 0 || date.month() == month) {
            let values = day(city, zone, date);
            for (i, key) in KEYS.iter().enumerate() {
                if let Some(v) = values[*key] {
                    if i < 2 {
                        let angle = v * std::f64::consts::TAU / 1440.0;
                        sums[i] += angle.sin();
                        cosines[i] += angle.cos();
                    } else {
                        sums[i] += v;
                    }
                    counts[i] += 1;
                }
            }
            date = date.succ_opt().unwrap();
        }
    }
    KEYS.into_iter()
        .enumerate()
        .map(|(i, key)| {
            (
                key.into(),
                (counts[i] > 0).then(|| {
                    let mean = sums[i] / counts[i] as f64;
                    if i < 2 {
                        (sums[i].atan2(cosines[i]) * 1440.0 / std::f64::consts::TAU)
                            .rem_euclid(1440.0)
                    } else {
                        mean
                    }
                }),
            )
        })
        .collect()
}

pub fn enrich(row: &mut Weather) {
    if let Some(b) = &row.baseline {
        let years = (b.start_year, b.end_year);
        for month in &mut row.monthly {
            month
                .values
                .extend(average(&row.city, &row.range_timezone, years, month.month));
        }
        if row.annual.is_none() && b.month == 0 {
            row.annual = Some(crate::model::Month {
                month: 0,
                values: Values::new(),
                ranges: Default::default(),
                sample_counts: Default::default(),
            });
        }
        if let Some(annual) = &mut row.annual {
            annual
                .values
                .extend(average(&row.city, &row.range_timezone, years, 0));
        }
        if let Some(selected) = if b.month == 0 {
            row.annual.as_ref()
        } else {
            row.monthly.iter().find(|m| m.month == b.month)
        } {
            row.values.extend(
                KEYS.into_iter()
                    .map(|key| (key.into(), selected.values.get(key).copied().flatten())),
            );
        }
    } else {
        let date = NaiveDate::parse_from_str(&row.range_date, "%Y-%m-%d")
            .ok()
            .or_else(|| crate::view::city_now(row).map(|d| d.date_naive()));
        if let Some(date) = date {
            row.values.extend(day(&row.city, &row.range_timezone, date));
        }
    }
    for forecast in &mut row.daily {
        if let Ok(date) = NaiveDate::parse_from_str(&forecast.date, "%Y-%m-%d") {
            forecast.solar = day(&row.city, &row.range_timezone, date);
        }
    }
}

pub fn difference(key: &str, value: f64, base: f64) -> f64 {
    if matches!(key, "sunrise" | "sunset") {
        (value - base + 720.0).rem_euclid(1440.0) - 720.0
    } else {
        value - base
    }
}

pub fn text(key: &str, value: Option<f64>, delta: bool) -> String {
    let Some(v) = value.filter(|v| v.is_finite()) else {
        return "—".into();
    };
    let minutes = v.round() as i64;
    if delta {
        return format!("{minutes:+}m");
    }
    if key == "daylight" {
        format!("{}h{:02}m", minutes / 60, minutes % 60)
    } else {
        let minutes = minutes.rem_euclid(1440);
        format!("{:02}:{:02}", minutes / 60, minutes % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tokyo_summer_and_polar_days() {
        let mut city = City {
            id: "a".into(),
            name: "Tokyo".into(),
            latitude: 35.68,
            longitude: 139.69,
        };
        let date = NaiveDate::from_ymd_opt(2026, 6, 21).unwrap();
        let v = day(&city, "Asia/Tokyo", date);
        assert!((260.0..275.0).contains(&v["sunrise"].unwrap()));
        assert!((1130.0..1150.0).contains(&v["sunset"].unwrap()));
        assert!((870.0..900.0).contains(&v["daylight"].unwrap()));
        city.latitude = 80.;
        let v = day(&city, "UTC", date);
        assert_eq!(v["sunrise"], None);
        assert_eq!(v["daylight"], Some(1440.));
        assert_eq!(difference("sunset", 5., 1435.), 10.);
        assert_eq!(text("sunrise", Some(-15.), true), "-15m");
    }
}
