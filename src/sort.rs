use crate::model::Weather;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Sort {
    #[default]
    City,
    Temperature,
    Feels,
    Rain,
    Wind,
    Humidity,
    Pressure,
    Cloud,
    Weather,
    LocalTime,
    TemperatureMin,
    TemperatureMax,
    FeelsMin,
    FeelsMax,
    RainMin,
    RainMax,
    WindMin,
    WindMax,
    HumidityMin,
    HumidityMax,
    PressureMin,
    PressureMax,
    CloudMin,
    CloudMax,
    Sunrise,
    Sunset,
    Daylight,
    Snow,
    SnowMin,
    SnowMax,
}
impl Sort {
    pub const ALL: [Self; 30] = [
        Self::City,
        Self::Temperature,
        Self::Feels,
        Self::Rain,
        Self::Wind,
        Self::Humidity,
        Self::Pressure,
        Self::Cloud,
        Self::Weather,
        Self::LocalTime,
        Self::TemperatureMin,
        Self::TemperatureMax,
        Self::FeelsMin,
        Self::FeelsMax,
        Self::RainMin,
        Self::RainMax,
        Self::WindMin,
        Self::WindMax,
        Self::HumidityMin,
        Self::HumidityMax,
        Self::PressureMin,
        Self::PressureMax,
        Self::CloudMin,
        Self::CloudMax,
        Self::Sunrise,
        Self::Sunset,
        Self::Daylight,
        Self::Snow,
        Self::SnowMin,
        Self::SnowMax,
    ];
    /// Current/minimum/maximum members of each ranged metric.
    pub fn family(self) -> Option<[Self; 3]> {
        use Sort::*;
        [
            [Temperature, TemperatureMin, TemperatureMax],
            [Feels, FeelsMin, FeelsMax],
            [Rain, RainMin, RainMax],
            [Snow, SnowMin, SnowMax],
            [Wind, WindMin, WindMax],
            [Humidity, HumidityMin, HumidityMax],
            [Pressure, PressureMin, PressureMax],
            [Cloud, CloudMin, CloudMax],
        ]
        .into_iter()
        .find(|family| family.contains(&self))
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Sunrise => "sunrise",
            Self::Sunset => "sunset",
            Self::Daylight => "daylight",
            Self::City => "city",
            Self::Temperature => "temperature_2m",
            Self::Feels => "apparent_temperature",
            Self::Rain => "rain",
            Self::Snow => "snowfall",
            Self::SnowMin => "snowfall_min",
            Self::SnowMax => "snowfall_max",
            Self::Wind => "wind_speed_10m",
            Self::Humidity => "relative_humidity_2m",
            Self::Pressure => "surface_pressure",
            Self::Cloud => "cloud_cover",
            Self::Weather => "weather",
            Self::LocalTime => "local_time",
            Self::TemperatureMin => "temperature_min",
            Self::TemperatureMax => "temperature_max",
            Self::FeelsMin => "feels_min",
            Self::FeelsMax => "feels_max",
            Self::RainMin => "rain_min",
            Self::RainMax => "rain_max",
            Self::WindMin => "wind_min",
            Self::WindMax => "wind_max",
            Self::HumidityMin => "humidity_min",
            Self::HumidityMax => "humidity_max",
            Self::PressureMin => "pressure_min",
            Self::PressureMax => "pressure_max",
            Self::CloudMin => "cloud_min",
            Self::CloudMax => "cloud_max",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Sunrise => "Sunrise",
            Self::Sunset => "Sunset",
            Self::Daylight => "Daylight",
            Self::City => "City",
            Self::Temperature => "Temp",
            Self::Feels => "Feels",
            Self::Rain => "Rain",
            Self::Snow => "Snow",
            Self::SnowMin => "Min snow",
            Self::SnowMax => "Max snow",
            Self::Wind => "Wind",
            Self::Humidity => "Humidity",
            Self::Pressure => "Pressure",
            Self::Cloud => "Cloud",
            Self::Weather => "Weather",
            Self::LocalTime => "Local time",
            Self::TemperatureMin => "Min temp",
            Self::TemperatureMax => "Max temp",
            Self::FeelsMin => "Min feels",
            Self::FeelsMax => "Max feels",
            Self::RainMin => "Min rain",
            Self::RainMax => "Max rain",
            Self::WindMin => "Min wind",
            Self::WindMax => "Max wind",
            Self::HumidityMin => "Min humidity",
            Self::HumidityMax => "Max humidity",
            Self::PressureMin => "Min pressure",
            Self::PressureMax => "Max pressure",
            Self::CloudMin => "Min cloud",
            Self::CloudMax => "Max cloud",
        }
    }
    pub fn compare(self, a: &Weather, b: &Weather, reverse: bool) -> Ordering {
        self.compare_at(a, b, reverse, chrono::Utc::now())
    }
    pub fn compare_at(
        self,
        a: &Weather,
        b: &Weather,
        reverse: bool,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Ordering {
        let names = || {
            a.city
                .name
                .to_lowercase()
                .cmp(&b.city.name.to_lowercase())
                .then_with(|| a.city.id.cmp(&b.city.id))
        };
        if self == Self::City {
            return if reverse { names().reverse() } else { names() };
        }
        let number = |row: &Weather| {
            if self == Self::LocalTime {
                use chrono::Timelike;
                crate::view::city_now_at(row, now).map(|t| f64::from(t.hour() * 60 + t.minute()))
            } else if let Some([base, min, _]) = self.family().filter(|f| f[0] != self) {
                row.ranges
                    .get(base.key())
                    .and_then(|r| if self == min { r.min } else { r.max })
                    .filter(|n| n.is_finite())
            } else if self == Self::Weather {
                row.condition.as_ref().map(|c| f64::from(c.code))
            } else {
                row.values
                    .get(self.key())
                    .copied()
                    .flatten()
                    .filter(|n| n.is_finite())
            }
        };
        // Missing data stays last in both directions; ties remain alphabetical.
        match (number(a), number(b)) {
            (Some(a), Some(b)) => {
                let order = a.total_cmp(&b);
                (if reverse { order.reverse() } else { order }).then_with(names)
            }
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            _ => names(),
        }
    }
}
