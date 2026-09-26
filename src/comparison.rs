use crate::model::{Baseline, Weather};
use chrono::NaiveDate;

pub fn days(base: &Baseline, month: u32) -> f64 {
    let mut total = 0i64;
    for year in base.start_year..=base.end_year {
        let start = NaiveDate::from_ymd_opt(year, month.max(1), 1).unwrap();
        let end = if month == 0 || month == 12 {
            NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap()
        } else {
            NaiveDate::from_ymd_opt(year, month + 1, 1).unwrap()
        };
        total += (end - start).num_days();
    }
    total as f64 / f64::from(base.end_year - base.start_year + 1)
}
pub fn daily_rain(row: &Weather) -> Option<f64> {
    daily_precipitation(row, "precipitation")
}
pub fn daily_precipitation(row: &Weather, key: &str) -> Option<f64> {
    if let Some(base) = &row.baseline {
        row.values
            .get(key)
            .copied()
            .flatten()
            .map(|v| v / days(base, base.month))
    } else {
        row.daily_totals.get(key).copied().flatten().or_else(|| {
            row.daily
                .iter()
                .find(|d| d.date == row.range_date)
                .and_then(|d| match key {
                    "rain" => d.rain_sum,
                    "snowfall" => d.snowfall_sum,
                    _ => d.precipitation_sum,
                })
        })
    }
}
pub fn mixed(row: &Weather, base: &Weather) -> bool {
    row.city.id == base.city.id && row.baseline.is_some() != base.baseline.is_some()
}
