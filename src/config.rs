use crate::model::{City, Mode, Source};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub const EXAMPLE: &str = include_str!("../config.example.toml");
fn yes() -> bool {
    true
}
fn ten() -> u32 {
    10
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default, rename = "pinned", alias = "reference")]
    pub reference: String,
    #[serde(default)]
    pub source: Source,
    #[serde(default = "yes")]
    pub auto_location: bool,
    #[serde(default = "ten")]
    pub history_years: u32,
    pub cities: Vec<City>,
}
impl Config {
    pub fn update(path: &Path, change: impl FnOnce(&mut Self) -> Result<()>) -> Result<Self> {
        let mut saved = None;
        tapp_ui::storage::update(path, |old| {
            let result = (|| -> Result<Vec<u8>> {
                let mut config: Self = toml::from_str(match old {
                    Some(bytes) => std::str::from_utf8(bytes)?,
                    None => EXAMPLE,
                })?;
                config.validate()?;
                change(&mut config)?;
                config.validate()?;
                let bytes = toml::to_string_pretty(&config)?.into_bytes();
                saved = Some(config);
                Ok(bytes)
            })();
            result.map_err(|e| std::io::Error::other(format!("{e:#}")))
        })?;
        Ok(saved.expect("successful update"))
    }
    pub fn read(path: &Path) -> Result<Self> {
        let text = match tapp_ui::storage::read(path).context("Read config")? {
            Some(bytes) => String::from_utf8(bytes).context("Config must be UTF-8")?,
            None => EXAMPLE.into(),
        };
        let config: Self = toml::from_str(&text).context("Parse config TOML")?;
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.cities.is_empty(), "Keep at least one city");
        let mut ids = HashSet::new();
        for city in &self.cities {
            city.validate()?;
            ensure!(ids.insert(&city.id), "Duplicate city ID: {}", city.id);
        }
        ensure!(
            self.reference.is_empty() || ids.contains(&self.reference),
            "Pinned city must match a configured city ID"
        );
        ensure!(
            (1..=30).contains(&self.history_years),
            "history_years must be 1–30"
        );
        Ok(())
    }
    pub fn write(&self, path: &Path) -> Result<()> {
        self.validate()?;
        atomic_write(path, toml::to_string_pretty(self)?.as_bytes())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preferences {
    pub comparison_period: Option<crate::model::Period>,
    pub period: crate::model::Period,
    /// Session-only: zero means annual; 1..=12 selects a calendar month.
    #[serde(skip)]
    pub month: u32,
    pub mode: Mode,
    pub source: Source,
    #[serde(rename = "pinned", alias = "reference")]
    pub reference: String,
    pub filter: String,
    pub reverse: bool,
    pub sort: crate::sort::Sort,
    pub monochrome: bool,
    /// Last primary screen: None is the list; Some is a stable city ID.
    pub last_city: Option<String>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            comparison_period: None,
            period: Default::default(),
            month: 0,
            mode: Mode::Normal,
            source: Source::Auto,
            reference: String::new(),
            filter: String::new(),
            reverse: false,
            sort: crate::sort::Sort::default(),
            monochrome: false,
            last_city: None,
        }
    }
}
impl Preferences {
    pub fn compare_period(&mut self, other: crate::model::Period) {
        if other == self.period {
            return;
        }
        let (older, later) = if other.rank() < self.period.rank() {
            (other, self.period)
        } else {
            (self.period, other)
        };
        self.period = later;
        self.comparison_period = Some(older);
        self.mode = Mode::Comparison;
    }
}
pub fn path(kind: &str, name: &str) -> PathBuf {
    if kind == "CONFIG"
        && let Some(base) = tapp_ui::preferences::config_dir()
    {
        return base.join("nubila").join(name);
    }
    if cfg!(windows)
        && let Some(base) = std::env::var_os("LOCALAPPDATA")
    {
        return PathBuf::from(base).join("nubila").join(name);
    }
    if cfg!(target_os = "macos")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home)
            .join(if kind == "CACHE" {
                "Library/Caches"
            } else {
                "Library/Application Support"
            })
            .join("nubila")
            .join(name);
    }
    let fallback = match kind {
        "CONFIG" => ".config",
        "CACHE" => ".cache",
        _ => ".local/state",
    };
    let base = std::env::var_os(format!("XDG_{kind}_HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into())).join(fallback)
        });
    base.join("nubila").join(name)
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path)
        .map_err(|e| e.error)
        .context("Replace file atomically")?;
    Ok(())
}
