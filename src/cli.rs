use crate::{
    config::{self, Config, Preferences},
    model::{City, Mode, Source},
    service::{Client, FetchOptions, Service},
    view,
};
use anyhow::{Context, Result, ensure};
use chrono::Datelike;
use clap::{Parser, Subcommand, ValueEnum};
use std::{
    fs,
    io::{IsTerminal, Write},
    path::PathBuf,
};

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum Format {
    #[default]
    Json,
    Table,
    Csv,
}
#[derive(Debug, Parser)]
#[command(
    version,
    about = "Fast Rust weather CLI and Ratatui interface. No API keys required."
)]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Command>,
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
    #[arg(long, global = true)]
    pub cache_dir: Option<PathBuf>,
    #[arg(long, global = true)]
    pub state: Option<PathBuf>,
    #[arg(long, global = true)]
    pub reset_state: bool,
    #[arg(long, global = true, value_enum)]
    pub mode: Option<Mode>,
    #[arg(long, global = true, value_enum)]
    pub period: Option<crate::model::Period>,
    #[arg(long, global = true, value_enum)]
    pub source: Option<Source>,
    #[arg(long = "pinned", alias = "reference", global = true)]
    pub reference: Option<String>,
    /// Compare two periods; displays the later minus the older period.
    #[arg(long, global = true, value_enum)]
    pub compare_period: Option<crate::model::Period>,
    #[arg(long, global = true)]
    pub city: Vec<String>,
    #[arg(long, global = true, value_enum, default_value = "json")]
    pub format: Format,
    #[arg(long, global=true, value_parser=clap::value_parser!(u16).range(32..))]
    pub width: Option<u16>,
    #[arg(long, global = true)]
    pub no_location: bool,
    #[arg(long, global = true)]
    pub offline: bool,
    /// Fetch new responses even when cached data is fresh (unless --offline).
    #[arg(long, global = true)]
    pub refresh: bool,
    #[arg(long, global = true)]
    pub demo: bool,
    #[arg(long, global=true, value_parser=clap::value_parser!(u32).range(1..=30))]
    pub years: Option<u32>,
    #[arg(long, global=true, value_parser=clap::value_parser!(u32).range(0..=12))]
    pub month: Option<u32>,
    #[arg(long, global = true)]
    pub filter: Option<String>,
    #[arg(long, global = true)]
    pub reverse: bool,
    #[arg(long, global = true, value_enum)]
    pub sort: Option<crate::sort::Sort>,
    #[arg(long, global = true, value_parser = ["asc", "desc"])]
    pub order: Option<String>,
    #[arg(long, global = true, default_value_t = 2, value_parser = clap::value_parser!(u32).range(0..=92))]
    pub past_days: u32,
    #[arg(long, global = true)]
    pub monochrome: bool,
}
#[derive(Debug, Subcommand)]
pub enum Command {
    Tui,
    Weather,
    Detail {
        id: String,
    },
    Cities,
    Search {
        name: String,
    },
    Sources,
    /// Read or set the user-wide tapp-ui palette without starting the TUI.
    Theme {
        #[arg(long, value_parser=["terminal","tokyo-night-omarchy"])]
        set: Option<String>,
    },
    Config {
        #[arg(long)]
        init: bool,
        #[arg(long = "set", value_name = "KEY=VALUE")]
        set: Vec<String>,
        #[arg(long, value_name = "JSON")]
        add_city: Option<String>,
        #[arg(long, value_name = "ID")]
        remove_city: Option<String>,
    },
}
fn config_command(args: &Args, file: &std::path::Path) -> Result<()> {
    let Some(Command::Config {
        init,
        set,
        add_city,
        remove_city,
    }) = &args.command
    else {
        unreachable!()
    };
    if *init {
        if let Some(parent) = file.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(file)
            .context("Create config (refuses to overwrite)")?
            .write_all(config::EXAMPLE.as_bytes())?;
    }
    if !set.is_empty() || add_city.is_some() || remove_city.is_some() {
        Config::update(file, |config| {
            for assignment in set {
                let (key, value) = assignment
                    .split_once('=')
                    .context("Expected --set KEY=VALUE")?;
                match key {
                    "pinned" | "reference" => config.reference = value.into(),
                    "source" => {
                        config.source =
                            serde_json::from_value(serde_json::Value::String(value.into()))?
                    }
                    "auto_location" => {
                        config.auto_location = value
                            .parse()
                            .context("auto_location must be true or false")?
                    }
                    "history_years" => config.history_years = value.parse()?,
                    _ => anyhow::bail!("Unknown config key: {key}"),
                }
            }
            if let Some(json) = add_city {
                config.cities.push(serde_json::from_str::<City>(json)?);
            }
            if let Some(id) = remove_city {
                ensure!(
                    config.cities.iter().any(|c| &c.id == id),
                    "Unknown city ID: {id}"
                );
                config.cities.retain(|c| &c.id != id);
            }
            Ok(())
        })?;
    }
    let text = match tapp_ui::storage::read(file)? {
        Some(bytes) => String::from_utf8(bytes)?,
        None => config::EXAMPLE.into(),
    };
    print!("{text}");
    Ok(())
}
pub fn run() -> Result<i32> {
    let mut args = Args::parse();
    if let Some(Command::Theme { set }) = &args.command {
        let path = tapp_ui::preferences::theme_path().context("No shared theme path")?;
        if let Some(id) = set {
            let theme = if id == "terminal" {
                tapp_ui::theme::Theme::Terminal
            } else {
                tapp_ui::theme::Theme::TokyoNightOmarchy
            };
            tapp_ui::preferences::save_theme(&path, theme)?;
        }
        println!(
            "{}",
            serde_json::json!({"theme":tapp_ui::preferences::load_theme(&path)?.id(),"path":path})
        );
        return Ok(0);
    }
    if args.command.is_none() {
        args.command = Some(
            if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
                Command::Tui
            } else {
                Command::Weather
            },
        );
    }
    let config_path = args
        .config
        .clone()
        .unwrap_or_else(|| config::path("CONFIG", "config.toml"));
    if matches!(args.command, Some(Command::Config { .. })) {
        config_command(&args, &config_path)?;
        return Ok(0);
    }
    if matches!(args.command, Some(Command::Sources)) {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({"service":"Open-Meteo", "sources":["auto","gfs","icon","blend"],"models":{"auto":"best_match","gfs":"gfs_seamless","icon":"icon_seamless","blend":["gfs_seamless","icon_seamless"]},"blend":"Equal-weight numeric means at matching UTC hours; per-field contributor counts; degraded blends explicitly reported", "history":"ERA5 monthly means over completed years", "license":"CC BY 4.0; free hosted API is non-commercial"})
            )?
        );
        return Ok(0);
    }
    let config = Config::read(&config_path)?;
    let client = Client::new(
        args.cache_dir
            .clone()
            .unwrap_or_else(|| config::path("CACHE", "responses-rust-v1")),
        args.offline,
    );
    if let Some(Command::Search { name }) = &args.command {
        ensure!(!args.demo, "Search is unavailable in demo mode");
        println!("{}", serde_json::to_string_pretty(&client.search(name)?)?);
        return Ok(0);
    }
    let mut prefs = Preferences {
        reference: config.reference.clone(),
        source: config.source,
        ..Default::default()
    };
    let mut notice = String::new();
    let state_path = args
        .state
        .clone()
        .unwrap_or_else(|| config::path("STATE", "view.json"));
    let tui = matches!(args.command, Some(Command::Tui));
    if tui && !args.reset_state {
        match tapp_ui::storage::read(&state_path) {
            Ok(Some(bytes)) => match serde_json::from_slice(&bytes) {
                Ok(saved) => prefs = saved,
                Err(e) => notice = format!("Preferences invalid: {e}"),
            },
            Ok(None) => {}
            Err(e) => notice = format!("Preferences unavailable: {e}"),
        }
        if !prefs.reference.is_empty() && !config.cities.iter().any(|c| c.id == prefs.reference) {
            prefs.reference = config.reference.clone();
        }
    }
    if let Some(mode) = args.mode {
        prefs.mode = mode;
    }
    if let Some(period) = args.period {
        prefs.period = period;
    }
    if let Some(period) = args.compare_period {
        ensure!(period != prefs.period, "Choose two different periods");
        prefs.compare_period(period);
    }
    if prefs.mode == Mode::Historical {
        prefs.mode = Mode::Normal;
        if args.period.is_none() {
            prefs.period = crate::model::Period::Recent;
        }
    }
    prefs.month = args.month.unwrap_or(0);
    if let Some(Command::Detail { id }) = &args.command {
        prefs.last_city = Some(id.clone());
        if prefs.period != crate::model::Period::Now {
            prefs.month = 0;
        }
    }
    if let Some(source) = args.source {
        prefs.source = source;
    }
    if let Some(reference) = &args.reference {
        ensure!(
            reference.is_empty() || config.cities.iter().any(|c| &c.id == reference),
            "Pinned city must match a configured city ID"
        );
        prefs.reference = reference.clone();
    }
    if let Some(filter) = &args.filter {
        prefs.filter = filter.clone();
    }
    if args.reverse {
        prefs.reverse = true;
    }
    if let Some(sort) = args.sort {
        prefs.sort = sort;
    }
    if let Some(order) = &args.order {
        prefs.reverse = order == "desc";
    }
    if args.monochrome || std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
        prefs.monochrome = true;
    }
    let mut city_ids = args.city.clone();
    if let Some(Command::Detail { id }) = &args.command {
        city_ids = vec![id.clone()];
    }
    let service = Service {
        options: FetchOptions {
            offline: args.offline,
            refresh: args.refresh,
            demo: args.demo,
            no_location: args.no_location,
            years: args.years.unwrap_or(config.history_years),
            month: args.month.unwrap_or_else(|| chrono::Utc::now().month()),
            city_ids,
            past_days: args.past_days,
        },
        config,
        client,
    };
    if let Some(Command::Cities) = &args.command {
        let (cities, current, warnings) = service.cities();
        println!(
            "{}",
            serde_json::to_string_pretty(
                &serde_json::json!({"cities":cities,"current_city":current,"pinned":prefs.reference,"warnings":warnings})
            )?
        );
        return Ok(0);
    }
    if tui {
        ensure!(
            std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
            "TUI requires a terminal; use weather --format json"
        );
        return crate::tui::run(service, prefs, state_path, notice, config_path);
    }
    let mut report = service.load(&prefs, false)?;
    report.cities.retain(|r| {
        r.city
            .name
            .to_lowercase()
            .contains(&prefs.filter.to_lowercase())
    });
    let sort_time = chrono::Utc::now();
    report
        .cities
        .sort_by(|a, b| prefs.sort.compare_at(a, b, prefs.reverse, sort_time));
    let width = args.width.unwrap_or_else(|| {
        crossterm::terminal::size()
            .map(|(w, _)| w.max(32))
            .unwrap_or(100)
    });
    let output = match args.format {
        Format::Json => serde_json::to_string_pretty(&report)? + "\n",
        Format::Csv => view::csv(&report),
        Format::Table => {
            if matches!(args.command, Some(Command::Detail { .. })) {
                report
                    .cities
                    .iter()
                    .flat_map(|r| view::details(r, usize::from(width)))
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\n"
            } else {
                view::summary(&report, width)
            }
        }
    };
    if let Err(e) = std::io::stdout().lock().write_all(output.as_bytes())
        && e.kind() != std::io::ErrorKind::BrokenPipe
    {
        return Err(e.into());
    }
    for warning in &report.warnings {
        eprintln!("{warning}");
    }
    if !matches!(args.format, Format::Json) {
        for row in &report.cities {
            if let Some(error) = &row.error {
                eprintln!("{}: {error}", row.city.name);
            }
            for warning in &row.warnings {
                eprintln!("{}: {warning}", row.city.name);
            }
        }
    }
    Ok(i32::from(report.cities.iter().any(|r| {
        r.error.is_some()
            || r.comparison_base
                .as_ref()
                .is_some_and(|b| b.error.is_some())
    })))
}
