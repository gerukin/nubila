//! Host command policy and shared chrome. Weather actions remain in App.
use crate::tui::{App, Modal};
use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers as M};
use tapp_ui::{
    chrome::{Entry, StaticDialog},
    commands::{Binding, Catalog, CommandId, CommandPalette, CommandScope, CommandSpec},
    context_menu::CommandMenu,
    notification::ToastStack,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum Command {
    Quit,
    Help,
    Info,
    Theme,
    Add,
    Remove,
    Filter,
    Clear,
    Normal,
    Compare,
    History,
    Source,
    Reference,
    Refresh,
    Reverse,
    SortCity,
    SortTemp,
    SortFeels,
    SortRain,
    SortWind,
    SortHumidity,
    SortPressure,
    SortCloud,
    SortWeather,
    Inspect,
    Next,
    Previous,
    PageDown,
    PageUp,
    First,
    Last,
    NextCity,
    PreviousCity,
    NextGraph,
    PreviousGraph,
    TempGraph,
    RainGraph,
    WindGraph,
    CloudGraph,
    Pane,
    Now,
    CitySearch,
    CityAccept,
    CityRemove,
    PreviousPane,
    SortTime,
    List,
    Retry,
    Period,
    PreviousMonth,
    NextMonth,
    SortSunrise,
    SortSunset,
    SortDaylight,
    SortSnow,
    SnowGraph,
    MinGraph,
    MaxGraph,
    DaylightGraph,
    ColumnsLeft,
    ColumnsRight,
    SortSun,
    FeelsGraph,
    FeelsMinGraph,
    FeelsMaxGraph,
}
/// Main table order is also the source of truth for sorting keys and commands.
pub const TABLE_SORTS: &[(Command, Option<crate::sort::Sort>, char, &str)] = &[
    (
        Command::SortCity,
        Some(crate::sort::Sort::City),
        '0',
        "City",
    ),
    (
        Command::SortTemp,
        Some(crate::sort::Sort::Temperature),
        '1',
        "Temperature",
    ),
    (
        Command::SortFeels,
        Some(crate::sort::Sort::Feels),
        '2',
        "Feels like",
    ),
    (
        Command::SortRain,
        Some(crate::sort::Sort::Rain),
        '3',
        "Rain",
    ),
    (
        Command::SortSnow,
        Some(crate::sort::Sort::Snow),
        '4',
        "Snow",
    ),
    (
        Command::SortWind,
        Some(crate::sort::Sort::Wind),
        '5',
        "Wind",
    ),
    (
        Command::SortHumidity,
        Some(crate::sort::Sort::Humidity),
        '6',
        "Humidity",
    ),
    (
        Command::SortCloud,
        Some(crate::sort::Sort::Cloud),
        '7',
        "Cloud",
    ),
    (Command::SortSun, None, '8', "Sun"),
    (
        Command::SortPressure,
        Some(crate::sort::Sort::Pressure),
        '9',
        "Pressure",
    ),
    (
        Command::SortTime,
        Some(crate::sort::Sort::LocalTime),
        't',
        "Local time",
    ),
    (
        Command::SortWeather,
        Some(crate::sort::Sort::Weather),
        'w',
        "Conditions",
    ),
];
pub struct Shell {
    pub copy: Option<String>,
    pub catalog: Catalog,
    pub scope: CommandScope,
    pub palette: CommandPalette,
    pub palette_open: bool,
    pub help_over_palette: bool,
    pub menu: CommandMenu,
    pub menu_open: bool,
    pub help: StaticDialog,
    pub info: StaticDialog,
    pub info_open: bool,
    pub filter: tapp_ui::input::LiveFilter,
    pub theme: tapp_ui::theme::Theme,
    pub theme_path: Option<std::path::PathBuf>,
    pub toast: ToastStack,
    pub context: Option<(u8, bool, crate::model::Period, bool)>,
}
impl Default for Shell {
    fn default() -> Self {
        let help = StaticDialog::help(vec![]);
        Self {
            copy: None,
            catalog: Catalog::default(),
            scope: CommandScope::default(),
            palette: CommandPalette::default(),
            palette_open: false,
            help_over_palette: false,
            menu: CommandMenu::default(),
            menu_open: false,
            help,
            info: StaticDialog::info(vec![]),
            info_open: false,
            filter: Default::default(),
            theme: Default::default(),
            theme_path: None,
            toast: ToastStack::default(),
            context: None,
        }
    }
}
impl App {
    pub(crate) fn prepare_help(&mut self) {
        let field = |label: &str, value: &str| Entry::Field {
            label: format!("{label:<14}"),
            value: value.into(),
        };
        let mut entries = if self.editing {
            vec![
                Entry::Heading("Filter cities".into()),
                field("Type", "Preview matching cities"),
                field("Enter", "Apply filter"),
                field("Esc", "Restore previous filter"),
                field("Ctrl+U", "Clear preview"),
            ]
        } else if self
            .city_manager
            .as_ref()
            .is_some_and(|manager| manager.removal.is_some())
        {
            vec![
                Entry::Heading("Remove city".into()),
                field("Enter", "Confirm removal"),
                field("Esc", "Cancel"),
            ]
        } else if self.city_manager.is_some() || self.period_picker.is_some() {
            vec![
                Entry::Heading("Choose".into()),
                field("Type", "Search"),
                field("↑ ↓", "Move through results"),
                field("Enter", "Search / confirm choice"),
                field("Esc", "Go back"),
            ]
        } else if self.modal == Some(Modal::Detail) {
            let mut rows = vec![
                Entry::Heading("City details".into()),
                field("↑ ↓", "Scroll table or graph"),
                field("← →", "Switch graph (or Tables in short windows)"),
                field(
                    "Alt+← →",
                    "Scroll columns (also Shift+wheel / horizontal wheel)",
                ),
                field(
                    if self.detail.graph_daily() {
                        "1–9 / 0"
                    } else {
                        "1–6"
                    },
                    if self.detail.graph_daily() {
                        "1 mean · 2 min · 3 max · 4 feels min · 5 feels max · 6 rain · 7 snow · 8 wind · 9 cloud · 0 daylight"
                    } else {
                        "1 temp · 2 feels · 3 rain · 4 snow · 5 wind · 6 cloud"
                    },
                ),
                field("[ ]", "Previous / next city"),
                field("l / Esc", "Return to city list"),
            ];
            if self.prefs.period == crate::model::Period::Now {
                rows.insert(3, field("Tab", "Switch hourly / daily table focus"));
                rows.insert(5, field("t", "Return to current hour"));
            }
            rows
        } else {
            vec![
                Entry::Heading("Cities".into()),
                field("↑ ↓ · Enter", "Select · open city"),
                field("f / Ctrl+F / /", "Filter cities"),
                field("a / d / r", "Add / remove / pin city"),
                field("Click header", "Sort column; click again to cycle"),
                field("← →", "Annual / month (climate periods)"),
                field(
                    "Alt+← →",
                    "Scroll columns (also Shift+wheel / horizontal wheel)",
                ),
            ]
        };
        if !self.editing && self.city_manager.is_none() && self.period_picker.is_none() {
            if self.modal != Some(Modal::Detail) {
                entries.push(Entry::Heading("Sort columns".into()));
                for group in TABLE_SORTS.chunks(3) {
                    let mut spans = Vec::new();
                    for (index, (_, _, key, label)) in group.iter().enumerate() {
                        if index > 0 {
                            spans.push(ratatui::text::Span::raw(" · "));
                        }
                        spans.push(ratatui::text::Span::styled(
                            key.to_string(),
                            tapp_ui::theme::Role::Command.style(self.prefs.monochrome),
                        ));
                        spans.push(ratatui::text::Span::raw(format!(" {label}")));
                    }
                    entries.push(Entry::Rich(ratatui::text::Line::from(spans)));
                }
                entries.push(Entry::Text("Repeat: value ↑ → value ↓ → min ↑ → max ↓\n8 opens Sun choices · o reverses sorting".into()));
            }
            entries.extend([
                Entry::Heading("Weather".into()),
                field("p", "Choose period"),
                field("n / c", "Normal / choose comparison"),
                field("R / u", "Retry missing data / force refresh"),
                field("i", "Data and source information"),
            ]);
        }
        entries.extend([
            Entry::Heading("More".into()),
            field("Ctrl+K / F2", "All commands and shortcuts"),
            field("Ctrl+? / F1", "Help anywhere; ? outside inputs"),
            field("Ctrl+Q", "Quit; q outside inputs"),
            Entry::Heading("About Nubila".into()),
            Entry::Text("Weather in your terminal · Rust + Ratatui".into()),
            Entry::Text("Weather: Open-Meteo · NOAA GFS · DWD ICON".into()),
            Entry::Text("Historical: ERA5 / Copernicus · Projections: Open-Meteo".into()),
            Entry::Text("Geocoding: GeoNames through Open-Meteo".into()),
            Entry::Text("Solar times: local NOAA equation estimates".into()),
        ]);
        self.ui.help.set_entries(entries);
        self.ui.help.scroll = 0;
    }

    pub fn refresh_commands(&mut self) {
        let view =
            if self.ui.info_open || self.modal == Some(Modal::Help) || self.period_picker.is_some()
            {
                3
            } else if self
                .city_manager
                .as_ref()
                .is_some_and(|m| m.removal.is_some())
            {
                5
            } else if self.city_manager.is_some() {
                4
            } else if self.editing {
                2
            } else if self.modal == Some(Modal::Detail) {
                1
            } else {
                0
            };
        let context = (
            view,
            self.focus.is_some(),
            self.prefs.period,
            self.detail.graph_daily(),
        );
        if self.ui.context == Some(context) {
            return;
        }
        self.ui.context = Some(context);
        let mut specs = Vec::new();
        let mut add = |cmd: Command, label: &str, keys: Vec<KeyEvent>, available: bool| {
            let shortcut = keys
                .iter()
                .map(|k| tapp_ui::commands::shortcut(*k))
                .collect::<Vec<_>>()
                .join(" / ");
            specs.push(CommandSpec {
                id: CommandId {
                    owner: 1,
                    action: cmd as u64,
                },
                label: label.into(),
                shortcut,
                bindings: keys.into_iter().map(Binding::new).collect(),
                available,
            });
        };
        let plain = |c| KeyEvent::new(K::Char(c), M::NONE);
        let ctrl = |c| KeyEvent::new(K::Char(c), M::CONTROL);
        let key = |k| KeyEvent::new(k, M::NONE);
        add(
            Command::Quit,
            "Quit",
            vec![ctrl('q'), ctrl('c'), plain('q')],
            true,
        );
        add(
            Command::Help,
            "Help",
            vec![
                key(K::F(1)),
                tapp_ui::focus::HELP,
                KeyEvent::new(K::Char('?'), M::CONTROL | M::SHIFT),
                plain('?'),
                KeyEvent::new(K::Char('?'), M::SHIFT),
            ],
            true,
        );
        add(
            Command::Info,
            "Info",
            vec![ctrl('i'), plain('i')],
            view <= 1,
        );
        add(Command::Theme, "Toggle theme", vec![], true);
        add(
            Command::Period,
            "Choose period",
            vec![plain('p')],
            view <= 1,
        );
        add(
            Command::Normal,
            "Normal values",
            vec![plain('n')],
            view <= 1,
        );
        add(
            Command::Compare,
            "Choose comparison",
            vec![plain('c')],
            view <= 1,
        );
        add(
            Command::PreviousMonth,
            "Previous month / annual",
            vec![key(K::Left)],
            view == 0 && self.prefs.period != crate::model::Period::Now,
        );
        add(
            Command::NextMonth,
            "Next month / annual",
            vec![key(K::Right)],
            view == 0 && self.prefs.period != crate::model::Period::Now,
        );
        for (cmd, label, c) in [
            (Command::Add, "Add city", 'a'),
            (Command::Remove, "Remove focused city", 'd'),
            (Command::Source, "Cycle weather source", 's'),
            (Command::Reference, "Pin focused city", 'r'),
            (Command::Reverse, "Reverse sort order", 'o'),
        ] {
            add(
                cmd,
                label,
                vec![plain(c)],
                view == 0
                    && (!matches!(cmd, Command::Remove | Command::Reference)
                        || self.focus.is_some()),
            );
        }
        add(
            Command::Refresh,
            "Refresh weather (bypass cache)",
            vec![plain('u')],
            view <= 1,
        );
        add(
            Command::Retry,
            "Retry missing/stale data",
            vec![plain('R')],
            view <= 1,
        );
        add(
            Command::Filter,
            "Filter cities",
            vec![ctrl('f'), plain('f'), plain('/')],
            view == 0,
        );
        add(
            Command::Clear,
            "Clear city filter",
            vec![ctrl('u')],
            view == 0,
        );
        for &(cmd, _, shortcut, label) in TABLE_SORTS {
            add(
                cmd,
                &format!("Sort by {label}"),
                vec![plain(shortcut)],
                view == 0,
            );
        }
        for (command, label) in [
            (Command::SortSunrise, "Sort by sunrise"),
            (Command::SortSunset, "Sort by sunset"),
            (Command::SortDaylight, "Sort by daylight"),
        ] {
            add(command, label, vec![], view == 0);
        }
        add(
            Command::Inspect,
            "Open city details",
            vec![key(K::Enter)],
            view == 0,
        );
        for (cmd, label, k) in [
            (Command::Next, "Next row", K::Down),
            (Command::Previous, "Previous row", K::Up),
            (Command::PageDown, "Page down", K::PageDown),
            (Command::PageUp, "Page up", K::PageUp),
            (Command::First, "First row", K::Home),
            (Command::Last, "Last row", K::End),
        ] {
            let mut keys = vec![key(k)];
            if cmd == Command::Next {
                keys.push(plain('j'));
            }
            if cmd == Command::Previous {
                keys.push(plain('k'));
            }
            add(cmd, label, keys, view <= 1);
        }
        for (cmd, label, c) in [
            (Command::NextCity, "Next city", ']'),
            (Command::PreviousCity, "Previous city", '['),
            (Command::TempGraph, "Mean temperature graph", '1'),
            (Command::MinGraph, "Minimum temperature graph", '2'),
            (Command::MaxGraph, "Maximum temperature graph", '3'),
            (Command::RainGraph, "Rain graph", '4'),
            (Command::WindGraph, "Wind graph", '6'),
            (Command::CloudGraph, "Cloud graph", '7'),
            (Command::SnowGraph, "Snow graph", '5'),
            (Command::DaylightGraph, "Daylight graph", '8'),
            (Command::FeelsGraph, "Feels-like temperature graph", '2'),
            (Command::FeelsMinGraph, "Feels-like minimum graph", '4'),
            (Command::FeelsMaxGraph, "Feels-like maximum graph", '5'),
            (Command::Now, "Return to current local hour", 't'),
        ] {
            add(
                cmd,
                if cmd == Command::TempGraph && !self.detail.graph_daily() {
                    "Temperature graph"
                } else {
                    label
                },
                cmd.metric().map_or_else(
                    || vec![plain(c)],
                    |m| {
                        self.detail
                            .metric_shortcut(m)
                            .map(|c| vec![plain(c)])
                            .unwrap_or_default()
                    },
                ),
                view == 1
                    && cmd
                        .metric()
                        .is_none_or(|m| self.detail.metrics().contains(&m)),
            );
        }
        for (cmd, label, code) in [
            (Command::ColumnsLeft, "Scroll columns left", K::Left),
            (Command::ColumnsRight, "Scroll columns right", K::Right),
        ] {
            add(cmd, label, vec![KeyEvent::new(code, M::ALT)], view <= 1);
        }
        add(
            Command::NextGraph,
            "Next graph / view tab",
            vec![key(K::Right)],
            view == 1,
        );
        add(
            Command::PreviousGraph,
            "Previous graph / view tab",
            vec![key(K::Left)],
            view == 1,
        );
        add(
            Command::Pane,
            "Next table focus",
            vec![key(K::Tab)],
            view == 1,
        );
        add(
            Command::PreviousPane,
            "Previous table focus",
            vec![key(K::BackTab), KeyEvent::new(K::BackTab, M::SHIFT)],
            view == 1,
        );
        add(Command::CitySearch, "Search cities", vec![], view == 4);
        add(
            Command::List,
            "Go to city list",
            vec![plain('l')],
            view == 1,
        );
        add(
            Command::CityAccept,
            "Add chosen city",
            vec![],
            view == 4
                && self
                    .city_manager
                    .as_ref()
                    .is_some_and(|m| !m.results.is_empty()),
        );
        add(
            Command::CityRemove,
            "Confirm city removal",
            vec![],
            view == 5,
        );
        self.ui.catalog.contribute(1, self.ui.scope.clone(), specs);
        if self.ui.palette_open {
            self.ui.palette.refresh(&self.ui.catalog);
        }
    }
    pub fn info_entries(&self) -> Vec<Entry> {
        let quota = self.quota_status();
        let mut out = vec![
            Entry::Heading("View & preferences".into()),
            Entry::Field {
                label: "Period".into(),
                value: if self.prefs.period == crate::model::Period::Now {
                    "Now".into()
                } else {
                    format!(
                        "{} · {}",
                        self.prefs.period.label(),
                        crate::climate::MONTHS[self.prefs.month as usize]
                    )
                },
            },
            Entry::Field {
                label: "Mode".into(),
                value: self.prefs.mode.label().into(),
            },
            Entry::Field {
                label: "Source".into(),
                value: self.prefs.source.label().into(),
            },
            Entry::Field {
                label: "Theme".into(),
                value: self.ui.theme.label().into(),
            },
            Entry::Field {
                label: "Pinned city".into(),
                value: self
                    .config
                    .cities
                    .iter()
                    .find(|c| c.id == self.prefs.reference)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| self.prefs.reference.clone()),
            },
            Entry::Field {
                label: "Saved cities".into(),
                value: self.config.cities.len().to_string(),
            },
        ];
        if let Some(path) = &self.config_path {
            out.push(Entry::Heading("Configuration".into()));
            out.push(Entry::Path(path.display().to_string()));
        }
        if !self.notice.is_empty() || !quota.is_empty() {
            out.push(Entry::Heading("Queue & notices".into()));
        }
        if !self.notice.is_empty() && (quota.is_empty() || !self.notice.starts_with("Quota wait")) {
            out.push(Entry::Text(self.notice.clone()));
        }
        if !quota.is_empty() {
            out.push(Entry::Field {
                label: "Retry".into(),
                value: quota
                    .strip_prefix("Quota wait · retry ")
                    .unwrap_or(&quota)
                    .to_owned(),
            });
            out.push(Entry::Text("Automatic retry · completed months reused\nQuota resets: UTC minute / hour / day\nReserved capacity: current weather".into()));
            let ids = self.request_ids();
            if let Some(report) = &self.report {
                for row in report.cities.iter().filter(|r| ids.contains(&r.city.id)) {
                    for pending in std::iter::once(row).chain(row.comparison_base.as_deref()) {
                        if pending.retry_at.is_some() {
                            let reason = pending.error.as_deref().or_else(|| {
                                pending
                                    .provenance
                                    .values()
                                    .find_map(|p| p.warning.as_deref())
                            });
                            if let Some(reason) = reason {
                                out.push(Entry::Field {
                                    label: pending.city.name.clone(),
                                    value: reason.replace("; ", "\n"),
                                });
                            }
                        }
                    }
                }
            }
        }
        out.extend(crate::theme::Theme::new(self.prefs.monochrome).color_guide());
        out.extend([
            Entry::Heading("App & sources".into()),
            Entry::Text("Nubila · free Open-Meteo hosted service\nNon-commercial use".into()),
            Entry::Field {
                label: "Display times".into(),
                value: "City-local timezone".into(),
            },
            Entry::Field {
                label: "JSON / CSV times".into(),
                value: "UTC".into(),
            },
        ]);
        out
    }
}
impl Command {
    pub const ALL: &[Self] = &[
        Self::Quit,
        Self::Help,
        Self::Info,
        Self::Theme,
        Self::Add,
        Self::Remove,
        Self::Filter,
        Self::Clear,
        Self::Normal,
        Self::Compare,
        Self::History,
        Self::Source,
        Self::Reference,
        Self::Refresh,
        Self::Reverse,
        Self::SortCity,
        Self::SortTemp,
        Self::SortFeels,
        Self::SortRain,
        Self::SortWind,
        Self::SortHumidity,
        Self::SortPressure,
        Self::SortCloud,
        Self::SortWeather,
        Self::Inspect,
        Self::Next,
        Self::Previous,
        Self::PageDown,
        Self::PageUp,
        Self::First,
        Self::Last,
        Self::NextCity,
        Self::PreviousCity,
        Self::NextGraph,
        Self::PreviousGraph,
        Self::TempGraph,
        Self::RainGraph,
        Self::WindGraph,
        Self::CloudGraph,
        Self::Pane,
        Self::Now,
        Self::CitySearch,
        Self::CityAccept,
        Self::CityRemove,
        Self::PreviousPane,
        Self::SortTime,
        Self::List,
        Self::Retry,
        Self::Period,
        Self::PreviousMonth,
        Self::NextMonth,
        Self::SortSunrise,
        Self::SortSunset,
        Self::SortDaylight,
        Self::SortSnow,
        Self::SnowGraph,
        Self::MinGraph,
        Self::MaxGraph,
        Self::DaylightGraph,
        Self::ColumnsLeft,
        Self::ColumnsRight,
        Self::SortSun,
        Self::FeelsGraph,
        Self::FeelsMinGraph,
        Self::FeelsMaxGraph,
    ];
    pub fn sort(self) -> Option<crate::sort::Sort> {
        TABLE_SORTS
            .iter()
            .find(|(cmd, _, _, _)| *cmd == self)
            .and_then(|(_, sort, _, _)| *sort)
            .or(match self {
                Self::SortSunrise => Some(crate::sort::Sort::Sunrise),
                Self::SortSunset => Some(crate::sort::Sort::Sunset),
                Self::SortDaylight => Some(crate::sort::Sort::Daylight),
                _ => None,
            })
    }
    pub fn metric(self) -> Option<crate::detail::Metric> {
        use crate::detail::Metric as M;
        Some(match self {
            Self::TempGraph => M::Temperature,
            Self::MinGraph => M::TemperatureMin,
            Self::MaxGraph => M::TemperatureMax,
            Self::FeelsGraph => M::Feels,
            Self::FeelsMinGraph => M::FeelsMin,
            Self::FeelsMaxGraph => M::FeelsMax,
            Self::RainGraph => M::Rain,
            Self::SnowGraph => M::Snow,
            Self::WindGraph => M::Wind,
            Self::CloudGraph => M::Cloud,
            Self::DaylightGraph => M::Daylight,
            _ => return None,
        })
    }
    pub fn key(self) -> Option<K> {
        if let Some((_, _, key, _)) = TABLE_SORTS.iter().find(|(cmd, _, _, _)| *cmd == self) {
            return Some(K::Char(*key));
        }
        Some(match self {
            Self::Add => K::Char('a'),
            Self::Remove => K::Char('d'),
            Self::Filter => K::Char('/'),
            Self::Normal => K::Char('n'),
            Self::Compare => K::Char('c'),
            Self::History => K::Char('h'),
            Self::Source => K::Char('s'),
            Self::Reference => K::Char('r'),
            Self::Refresh => K::Char('u'),
            Self::Reverse => K::Char('o'),
            Self::Inspect => K::Enter,
            Self::Next => K::Down,
            Self::Previous => K::Up,
            Self::PageDown => K::PageDown,
            Self::PageUp => K::PageUp,
            Self::First => K::Home,
            Self::Last => K::End,
            Self::NextCity => K::Char(']'),
            Self::PreviousCity => K::Char('['),
            Self::NextGraph => K::Right,
            Self::PreviousGraph => K::Left,
            Self::Pane => K::Tab,
            Self::PreviousPane => K::BackTab,
            Self::Now => K::Char('t'),
            _ => return None,
        })
    }
}
