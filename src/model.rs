use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Period {
    #[default]
    Now,
    Baseline,
    Recent,
    Future,
}
impl Period {
    pub fn rank(self) -> u8 {
        match self {
            Self::Baseline => 0,
            Self::Recent => 1,
            Self::Now => 2,
            Self::Future => 3,
        }
    }
    pub const ALL: [Self; 4] = [Self::Now, Self::Baseline, Self::Recent, Self::Future];
    pub fn label(self) -> &'static str {
        match self {
            Self::Now => "Now",
            Self::Baseline => "1950~1969",
            Self::Recent => "Recent 5 years",
            Self::Future => "2040~2049",
        }
    }
    pub fn years(self) -> (i32, i32) {
        use chrono::Datelike;
        match self {
            Self::Baseline => (1950, 1969),
            Self::Recent => {
                let y = chrono::Utc::now().year();
                (y - 5, y - 1)
            }
            Self::Future => (2040, 2049),
            Self::Now => (0, 0),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Normal,
    Comparison,
    #[value(skip)]
    Historical,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    #[default]
    Auto,
    Gfs,
    Icon,
    Blend,
}
impl Source {
    pub fn next(self) -> Self {
        match self {
            Self::Auto => Self::Gfs,
            Self::Gfs => Self::Icon,
            Self::Icon => Self::Blend,
            Self::Blend => Self::Auto,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Gfs => "gfs",
            Self::Icon => "icon",
            Self::Blend => "blend",
        }
    }
}
impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Comparison => "comparison",
            Self::Historical => "historical",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct City {
    pub id: String,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
}
impl City {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.id.trim().is_empty() && !self.name.trim().is_empty(),
            "City needs an ID and name"
        );
        anyhow::ensure!(
            (-90.0..=90.0).contains(&self.latitude) && (-180.0..=180.0).contains(&self.longitude),
            "Invalid coordinates for {}",
            self.name
        );
        Ok(())
    }
}
pub const FIELDS: &[(&str, &str)] = &[
    ("temperature_2m", "°C"),
    ("apparent_temperature", "°C"),
    ("relative_humidity_2m", "%"),
    ("precipitation", "mm"),
    ("rain", "mm"),
    ("snowfall", "cm"),
    ("wind_speed_10m", "km/h"),
    ("surface_pressure", "hPa"),
    ("cloud_cover", "%"),
];
pub type Values = BTreeMap<String, Option<f64>>;
pub fn is_precipitation(key: &str) -> bool {
    matches!(key, "precipitation" | "rain" | "snowfall")
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Range {
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub samples: usize,
}
pub type Ranges = BTreeMap<String, Range>;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    pub code: u8,
    pub is_day: Option<bool>,
    pub source: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Day {
    #[serde(default)]
    pub feels_mean: Option<f64>,
    #[serde(default)]
    pub feels_min: Option<f64>,
    #[serde(default)]
    pub feels_max: Option<f64>,
    #[serde(default)]
    pub temperature_mean: Option<f64>,
    #[serde(default)]
    pub solar: Values,
    pub date: String,
    pub temperature_min: Option<f64>,
    pub temperature_max: Option<f64>,
    pub precipitation_sum: Option<f64>,
    #[serde(default)]
    pub rain_sum: Option<f64>,
    #[serde(default)]
    pub snowfall_sum: Option<f64>,
    pub wind_speed_max: Option<f64>,
    pub cloud_cover: Option<f64>,
    pub humidity: Option<f64>,
    pub pressure: Option<f64>,
    pub condition: Option<Condition>,
}
impl Day {
    pub fn available(&self) -> bool {
        self.condition.is_some()
            || [
                self.temperature_min,
                self.temperature_max,
                self.precipitation_sum,
                self.temperature_mean,
                self.feels_mean,
                self.feels_min,
                self.feels_max,
                self.rain_sum,
                self.snowfall_sum,
                self.wind_speed_max,
                self.cloud_cover,
                self.humidity,
                self.pressure,
            ]
            .iter()
            .any(Option::is_some)
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Provenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_at: Option<i64>,
    pub cached: bool,
    pub stale: bool,
    pub fetched_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hour {
    pub time: String,
    #[serde(flatten)]
    pub values: Values,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub contributors: BTreeMap<String, usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<Condition>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Month {
    pub month: u32,
    #[serde(flatten)]
    pub values: Values,
    pub sample_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub ranges: Ranges,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline {
    pub start_year: i32,
    pub end_year: i32,
    pub month: u32,
    pub method: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Weather {
    /// Provider totals for the current local day, independent of current-interval values.
    #[serde(default)]
    pub daily_totals: Values,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison_label: Option<String>,
    /// Absolute same-city data for a period comparison; never recursive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison_base: Option<Box<Weather>>,
    /// Absolute annual summary; monthly holds calendar months 1..=12 only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annual: Option<Month>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub absolute_monthly: Vec<Month>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_at: Option<i64>,
    pub city: City,
    pub values: Values,
    pub time: String,
    pub hourly: Vec<Hour>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<Condition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub daily: Vec<Day>,
    #[serde(default)]
    pub ranges: Ranges,
    #[serde(default)]
    pub range_date: String,
    #[serde(default)]
    pub range_timezone: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub day_hourly: Vec<Hour>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absolute_ranges: Option<Ranges>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub monthly: Vec<Month>,
    pub sources: Vec<String>,
    pub provenance: BTreeMap<String, Provenance>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub absolute_values: Option<Values>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline: Option<Baseline>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub contributors: BTreeMap<String, usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
impl Weather {
    pub fn empty(city: City) -> Self {
        Self {
            daily_totals: Values::new(),
            comparison_base: None,
            comparison_label: None,
            annual: None,
            absolute_monthly: vec![],
            retry_at: None,
            city,
            values: Values::new(),
            time: String::new(),
            hourly: vec![],
            condition: None,
            daily: vec![],
            ranges: Ranges::new(),
            range_date: String::new(),
            range_timezone: String::new(),
            day_hourly: vec![],
            absolute_ranges: None,
            monthly: vec![],
            sources: vec![],
            provenance: BTreeMap::new(),
            absolute_values: None,
            baseline: None,
            contributors: BTreeMap::new(),
            warnings: vec![],
            error: None,
        }
    }
    pub fn flagged(&self) -> bool {
        self.error.is_some()
            || !self.warnings.is_empty()
            || self
                .provenance
                .values()
                .any(|p| p.stale || p.warning.is_some())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison_period: Option<Period>,
    #[serde(default)]
    pub period: Period,
    pub schema_version: u32,
    pub mode: Mode,
    pub source: Source,
    #[serde(rename = "pinned", alias = "reference")]
    pub reference: String,
    pub demo: bool,
    pub current_city: Option<String>,
    pub timezone: String,
    pub units: BTreeMap<String, String>,
    pub hourly_units: BTreeMap<String, String>,
    #[serde(default)]
    pub range_units: BTreeMap<String, String>,
    pub comparison_scope: String,
    pub attribution: String,
    pub warnings: Vec<String>,
    pub cities: Vec<Weather>,
}
impl Report {
    /// Change forecast presentation without discarding hourly data or provenance.
    pub fn present(&mut self, mode: Mode, reference: &str) -> anyhow::Result<()> {
        if mode == Mode::Comparison && self.comparison_period.is_none() {
            anyhow::ensure!(
                self.cities
                    .iter()
                    .any(|r| r.city.id == reference && r.error.is_none()),
                "Pinned city weather unavailable; cannot calculate differences"
            );
        }
        {
            for row in &mut self.cities {
                row.comparison_label = None;
                if !row.absolute_monthly.is_empty() {
                    row.monthly = std::mem::take(&mut row.absolute_monthly);
                }
                if let Some(values) = row.absolute_values.take() {
                    row.values = values;
                }
                if let Some(ranges) = row.absolute_ranges.take() {
                    row.ranges = ranges;
                }
                row.warnings
                    .retain(|w| w != "Comparison timestamps differ; inspect provenance");
            }
            self.units = self.hourly_units.clone();
            if self.mode == Mode::Historical && self.period == Period::Now {
                self.units.insert("precipitation".into(), "mm/day".into());
            }
            if self.period != Period::Now {
                self.units.insert(
                    "precipitation".into(),
                    if self
                        .cities
                        .first()
                        .and_then(|r| r.baseline.as_ref())
                        .is_none_or(|b| b.month == 0)
                    {
                        "mm/year"
                    } else {
                        "mm/month"
                    }
                    .into(),
                );
            }
            self.range_units = self.hourly_units.clone();
            self.range_units
                .insert("precipitation".into(), "mm (hourly total)".into());
            if mode == Mode::Comparison {
                if self.comparison_period.is_some() {
                    for row in &mut self.cities {
                        let base = row.comparison_base.take();
                        if let Some(base) = &base {
                            compare_pair(row, base, true);
                        }
                        row.comparison_base = base;
                    }
                } else {
                    compare(&mut self.cities, reference)?;
                }
                for key in crate::solar::KEYS {
                    self.units.insert(key.into(), "minutes difference".into());
                }
                if self.comparison_period.is_some()
                    && (self.period == Period::Now || self.comparison_period == Some(Period::Now))
                {
                    self.units.insert("precipitation".into(), "mm/day".into());
                }
                for key in ["relative_humidity_2m", "cloud_cover"] {
                    self.units.insert(key.into(), "percentage points".into());
                    self.range_units
                        .insert(key.into(), "percentage points".into());
                }
            }
        }
        for (key, unit) in [("rain", "mm"), ("snowfall", "cm")] {
            let per = if self.period != Period::Now {
                if mode == Mode::Comparison && self.comparison_period == Some(Period::Now) {
                    "/day"
                } else if self
                    .cities
                    .first()
                    .and_then(|r| r.baseline.as_ref())
                    .is_none_or(|b| b.month == 0)
                {
                    "/year"
                } else {
                    "/month"
                }
            } else if self.mode == Mode::Historical
                || (mode == Mode::Comparison && self.comparison_period.is_some())
            {
                "/day"
            } else {
                ""
            };
            self.units.insert(key.into(), format!("{unit}{per}"));
            self.range_units
                .insert(key.into(), format!("{unit} (hourly total)"));
        }
        self.mode = mode;
        self.reference = reference.into();
        Ok(())
    }
}
pub fn mean(values: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    let values: Vec<f64> = values.flatten().filter(|v| v.is_finite()).collect();
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}
pub fn compare(rows: &mut [Weather], reference: &str) -> anyhow::Result<()> {
    let base = rows
        .iter()
        .find(|r| r.city.id == reference && r.error.is_none())
        .ok_or_else(|| {
            anyhow::anyhow!("Pinned city weather unavailable; cannot calculate differences")
        })?;
    let base = base.clone();
    for row in rows {
        compare_pair(row, &base, false);
    }
    Ok(())
}

pub fn compare_pair(row: &mut Weather, base: &Weather, period: bool) {
    row.comparison_label = Some(if period {
        base.baseline.as_ref().map_or_else(
            || "Now (today)".into(),
            |b| format!("{}~{}", b.start_year, b.end_year),
        )
    } else {
        format!("Pinned · {}", base.city.name)
    });
    let mixed = crate::comparison::mixed(row, base);
    let totals: Values = ["precipitation", "rain", "snowfall"]
        .into_iter()
        .map(|key| {
            (
                key.into(),
                crate::comparison::daily_precipitation(row, key)
                    .zip(crate::comparison::daily_precipitation(base, key))
                    .map(|(a, b)| a - b),
            )
        })
        .collect();
    let values = &base.values;
    let ranges = &base.ranges;
    let stamp = &base.time;
    let months = &base.monthly;
    if !row.monthly.is_empty() {
        row.absolute_monthly = row.monthly.clone();
        for m in &mut row.monthly {
            {
                let b = months.iter().find(|b| b.month == m.month);
                let month_values = b
                    .map(|b| &b.values)
                    .or_else(|| base.baseline.is_none().then_some(values));
                for (k, v) in &mut m.values {
                    *v = v
                        .zip(month_values.and_then(|b| b.get(k).copied().flatten()))
                        .map(|(a, b)| crate::solar::difference(k, a, b));
                    if mixed && is_precipitation(k) {
                        let absolute = row
                            .absolute_monthly
                            .iter()
                            .find(|b| b.month == m.month)
                            .and_then(|b| b.values.get(k).copied().flatten());
                        *v = absolute
                            .zip(crate::comparison::daily_precipitation(base, k))
                            .map(|(a, b)| {
                                a / crate::comparison::days(row.baseline.as_ref().unwrap(), m.month)
                                    - b
                            });
                    }
                }
                for (k, r) in &mut m.ranges {
                    let b = b.map_or(&base.ranges, |b| &b.ranges).get(k);
                    r.min = r.min.zip(b.and_then(|r| r.min)).map(|(a, b)| a - b);
                    r.max = r.max.zip(b.and_then(|r| r.max)).map(|(a, b)| a - b);
                }
            }
        }
    }
    row.absolute_values = Some(row.values.clone());
    row.absolute_ranges = Some(row.ranges.clone());
    for (key, range) in &mut row.ranges {
        let reference = ranges.get(key);
        range.min = range
            .min
            .zip(reference.and_then(|r| r.min))
            .map(|(a, b)| a - b);
        range.max = range
            .max
            .zip(reference.and_then(|r| r.max))
            .map(|(a, b)| a - b);
    }
    for (k, v) in &mut row.values {
        *v = v
            .zip(values.get(k).copied().flatten())
            .map(|(a, b)| crate::solar::difference(k, a, b));
        if mixed && is_precipitation(k) {
            *v = totals[k];
        }
    }
    if !stamp.is_empty()
        && !row.time.is_empty()
        && row.time != *stamp
        && row.city.id != base.city.id
    {
        row.warnings
            .push("Comparison timestamps differ; inspect provenance".into());
    }
}
