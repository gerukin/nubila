use crate::{
    cities,
    config::{Config, Preferences},
    detail,
    model::{City, Mode, Period, Report, Weather},
    service::Service,
    sort::Sort,
    theme::Theme,
    view,
};
use anyhow::Result;
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    text::{Line, Span, Text},
    widgets::{Cell, Paragraph, Row, Table, TableState},
};
use std::{io, path::PathBuf, sync::mpsc, thread};
use unicode_width::UnicodeWidthStr;

// Temporary visual review; set false to restore loading-only animation.
pub const LOADING_PREVIEW: bool = false;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modal {
    Detail,
    Help,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Reused,
    None,
    Quit,
    Reload(bool),
    OlderHours,
    Theme,
    Copy,
    SearchCities,
    AddCity,
    RemoveCity,
}
type OrderKey = (u64, usize, Sort, bool, String, i64);
#[derive(Default)]
struct OrderCache {
    key: Option<OrderKey>,
    rows: Vec<usize>,
    positions: std::collections::HashMap<String, usize>,
    raw: std::collections::HashMap<String, usize>,
}
pub struct App {
    visible_ids: Vec<String>,
    pub period_picker: Option<tapp_ui::palette::Palette>,
    pub comparison_picker: bool,
    pub solar_sort_picker: bool,
    pub sun_header: Rect,
    revision: u64,
    order: std::cell::RefCell<OrderCache>,
    pub ui: crate::ui_shell::Shell,
    pub config_path: Option<PathBuf>,
    pub saved_search: Option<cities::State>,
    pub config: Config,
    pub city_manager: Option<cities::State>,
    help_return: Option<Modal>,
    pub prefs: Preferences,
    pub report: Option<Report>,
    alternate_report: Option<Report>,
    pub focus: Option<String>,
    pub modal: Option<Modal>,
    pub scroll: usize,
    pub column_offset: usize,
    pub column_max: usize,
    pub column_geometry: crate::viewport::Geometry,
    pub editing: bool,
    pub cursor: usize,
    pub notice: String,
    pub loading: bool,
    pub loading_indicator: tapp_ui::loading::Indicator,
    pub demo: bool,
    pub table: TableState,
    pub table_area: Rect,
    pub table_layout: tapp_ui::table::TableLayout,
    pub max_scroll: usize,
    pub detail: detail::State,
    pub pinned_area: Rect,
    pub sort_hits: Vec<(Rect, Sort)>,
    pub past_days: u32,
    pub sought_days: std::collections::HashMap<String, u32>,
    pub seeking: bool,
}
impl App {
    pub(crate) fn request_ids(&self) -> Vec<String> {
        let details = self.session_preferences().last_city.is_some();
        let mut ids: Vec<String> = if details {
            self.focus.clone().into_iter().collect()
        } else {
            self.visible_ids.clone()
        };
        if !details
            && let Some(i) = self.pinned()
            && let Some(r) = &self.report
        {
            ids.push(r.cities[i].city.id.clone());
        }
        if (!details || self.prefs.comparison_period.is_none())
            && !self.prefs.reference.is_empty()
            && !ids.contains(&self.prefs.reference)
        {
            ids.push(self.prefs.reference.clone());
        }
        if self.prefs.mode != Mode::Comparison && details {
            ids.retain(|id| Some(id) == self.focus.as_ref());
        }
        ids.sort();
        ids.dedup();
        ids
    }
    pub(crate) fn quota_status(&self) -> String {
        let ids = self.request_ids();
        self.report
            .as_ref()
            .and_then(|r| {
                r.cities
                    .iter()
                    .filter(|r| ids.contains(&r.city.id))
                    .flat_map(|c| {
                        c.retry_at.into_iter().chain(
                            c.comparison_base
                                .as_ref()
                                .filter(|_| self.prefs.mode == Mode::Comparison)
                                .and_then(|b| b.retry_at),
                        )
                    })
                    .min()
            })
            .and_then(|at| chrono::DateTime::from_timestamp(at, 0))
            .map(|t| {
                format!(
                    "Quota wait · retry {}",
                    t.with_timezone(&chrono::Local).format("%m-%d %H:%M")
                )
            })
            .unwrap_or_default()
    }
    pub fn restore_screen(&mut self) {
        let city = self
            .prefs
            .last_city
            .as_ref()
            .filter(|id| {
                self.report
                    .as_ref()
                    .is_some_and(|r| r.cities.iter().any(|c| &c.city.id == *id))
            })
            .cloned();
        self.prefs.last_city = city.clone();
        self.modal = city.as_ref().map(|_| Modal::Detail);
        self.focus = city;
    }

    pub fn session_preferences(&self) -> Preferences {
        let mut prefs = self.prefs.clone();
        let screen = if self.modal == Some(Modal::Help) {
            self.help_return
        } else {
            self.modal
        };
        prefs.last_city = (screen == Some(Modal::Detail))
            .then(|| self.focus.clone())
            .flatten();
        if self.editing {
            prefs.filter = self.ui.filter.committed().to_owned();
        }
        prefs
    }

    fn switch_view(&mut self, mode: Mode, reference: String) -> Action {
        if self.prefs.mode == mode && self.prefs.reference == reference {
            return Action::None;
        }
        let matches = |report: &Report| {
            report.source == self.prefs.source
                && report.period == self.prefs.period
                && report.cities.iter().any(|row| !row.time.is_empty())
                && (report.mode == Mode::Historical) == (mode == Mode::Historical)
        };
        let swapped = !self.report.as_ref().is_some_and(&matches)
            && self.alternate_report.as_ref().is_some_and(&matches);
        if swapped {
            std::mem::swap(&mut self.report, &mut self.alternate_report);
        }
        if let Some(report) = &mut self.report
            && matches(report)
        {
            if let Err(error) = report.present(mode, &reference) {
                if swapped {
                    std::mem::swap(&mut self.report, &mut self.alternate_report);
                }
                self.notice = error.to_string();
                self.ui.toast.push(tapp_ui::notification::Toast::plain(
                    tapp_ui::theme::ToastKind::Warning,
                    &self.notice,
                ));
                self.prefs.mode = mode;
                self.prefs.reference = reference;
                return Action::Reload(false);
            }
            self.prefs.mode = mode;
            self.prefs.reference = reference;
            self.loading = self.loading && !swapped;
            self.notice = report.warnings.join("; ");
            self.revision = self.revision.wrapping_add(1);
            self.detail.invalidate_data();
            self.normalize();
            return Action::Reused;
        }
        if self.report.is_some() && !self.loading {
            self.alternate_report = self.report.take();
        } else {
            self.report = None;
        }
        self.prefs.mode = mode;
        self.prefs.reference = reference;
        Action::Reload(false)
    }
    pub fn receive_city_search(
        &mut self,
        id: u64,
        query: &str,
        result: Result<Vec<cities::Candidate>>,
    ) {
        if let Some(m) = &mut self.city_manager
            && m.request_id == id
            && m.query() == query
            && m.removal.is_none()
        {
            m.receive(result);
            self.ui.context = None;
        }
    }
    pub fn save_city_change(&mut self, path: &std::path::Path, remove: bool) -> Result<()> {
        let manager = self
            .city_manager
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No city selected"))?;
        let city = if remove {
            manager.removal.as_ref()
        } else {
            manager.results.get(manager.selected()).map(|r| &r.city)
        }
        .ok_or_else(|| anyhow::anyhow!("Choose a city first"))?
        .clone();
        let change = |config: &mut Config| {
            if remove {
                cities::remove(config, &city.id)
            } else {
                cities::add(config, city.clone())
            }
        };
        let config = if self.demo {
            let mut config = self.config.clone();
            change(&mut config)?;
            config
        } else {
            Config::update(path, change)?
        };
        self.ui.toast.push(tapp_ui::notification::Toast::plain(
            tapp_ui::theme::ToastKind::Tip,
            if remove { "City removed" } else { "City added" },
        ));
        if remove && self.prefs.reference == city.id {
            self.prefs.reference = config.reference.clone();
        }
        self.config = config;
        self.alternate_report = None;
        self.saved_search = self.city_manager.take();
        self.focus = if remove { None } else { Some(city.id) };
        if !remove {
            self.prefs.filter.clear();
        }
        Ok(())
    }
    pub fn new(prefs: Preferences, demo: bool, notice: String) -> Self {
        Self {
            visible_ids: vec![],
            period_picker: None,
            comparison_picker: false,
            solar_sort_picker: false,
            sun_header: Rect::default(),
            revision: 0,
            order: Default::default(),
            ui: Default::default(),
            config_path: None,
            saved_search: None,
            config: toml::from_str(crate::config::EXAMPLE).expect("embedded config"),
            city_manager: None,
            help_return: None,
            prefs,
            report: None,
            alternate_report: None,
            focus: None,
            modal: None,
            scroll: 0,
            column_offset: 0,
            column_max: 0,
            column_geometry: Default::default(),
            editing: false,
            cursor: 0,
            notice,
            loading: true,
            loading_indicator: Default::default(),
            demo,
            table: TableState::default(),
            table_area: Rect::default(),
            table_layout: tapp_ui::table::TableStyle::default().layout(Rect::default(), 2),
            max_scroll: 0,
            detail: detail::State::default(),
            pinned_area: Rect::default(),
            sort_hits: vec![],
            past_days: 2,
            sought_days: Default::default(),
            seeking: false,
        }
    }
    pub fn visible(&self) -> Vec<usize> {
        let Some(report) = &self.report else {
            return vec![];
        };
        let key = (
            self.revision,
            report.cities.len(),
            self.prefs.sort,
            self.prefs.reverse,
            self.prefs.filter.clone(),
            if self.prefs.sort == Sort::LocalTime {
                chrono::Utc::now().timestamp().div_euclid(60)
            } else {
                0
            },
        );
        if self.order.borrow().key.as_ref() == Some(&key) {
            return self.order.borrow().rows.clone();
        }
        let filter = self.prefs.filter.to_lowercase();
        let mut indices: Vec<_> = report
            .cities
            .iter()
            .enumerate()
            .filter(|(_, r)| r.city.name.to_lowercase().contains(&filter))
            .map(|(i, _)| i)
            .collect();
        let sort_time = chrono::Utc::now();
        indices.sort_by(|&a, &b| {
            self.prefs.sort.compare_at(
                &report.cities[a],
                &report.cities[b],
                self.prefs.reverse,
                sort_time,
            )
        });
        *self.order.borrow_mut() = OrderCache {
            key: Some(key),
            rows: indices.clone(),
            positions: indices
                .iter()
                .enumerate()
                .map(|(position, &i)| (report.cities[i].city.id.clone(), position))
                .collect(),
            raw: report
                .cities
                .iter()
                .enumerate()
                .map(|(i, r)| (r.city.id.clone(), i))
                .collect(),
        };
        indices
    }
    pub fn pinned(&self) -> Option<usize> {
        let report = self.report.as_ref()?;
        if report.cities.len() <= 1 {
            return None;
        }
        let _ = self.visible();
        let order = self.order.borrow();
        let pinned = order.raw.get(&self.prefs.reference).copied().or_else(|| {
            report
                .current_city
                .as_ref()
                .and_then(|id| order.raw.get(id).copied())
        });
        if pinned.is_some_and(|i| self.visible() == [i]) {
            None
        } else {
            pinned
        }
    }
    fn sort_by(&mut self, sort: Sort) {
        if let Some([base, min, max]) = sort.family().filter(|f| f[0] == sort) {
            let sort = base;
            (self.prefs.sort, self.prefs.reverse) = match (self.prefs.sort, self.prefs.reverse) {
                (current, false) if current == sort => (sort, true),
                (current, true) if current == sort => (min, false),
                (current, _) if current == min => (max, true),
                _ => (sort, false),
            };
            self.normalize();
            return;
        }
        self.prefs.reverse = self.prefs.sort == sort && !self.prefs.reverse;
        self.prefs.sort = sort;
        self.normalize();
    }
    pub fn normalize(&mut self) {
        let indices = self.visible();
        if let Some(report) = &self.report {
            let selected = self
                .focus
                .as_ref()
                .and_then(|id| self.order.borrow().positions.get(id).copied());
            if self.modal != Some(Modal::Detail) {
                self.focus = selected.map(|i| report.cities[indices[i]].city.id.clone());
            }
            self.table.select(selected);
        }
    }
    pub fn receive(&mut self, result: Result<Report>) {
        self.detail.invalidate_data();
        self.revision = self.revision.wrapping_add(1);
        self.loading = false;
        match result {
            Ok(report) => {
                self.notice = report.warnings.join("; ");
                let mut report = report;
                if let Some(old) = &self.report
                    && old.period == report.period
                {
                    for row in &old.cities {
                        if !report.cities.iter().any(|c| c.city.id == row.city.id) {
                            report.cities.push(row.clone());
                        }
                    }
                }
                if self.prefs.period != Period::Now {
                    let month = if self.session_preferences().last_city.is_some() {
                        0
                    } else {
                        self.prefs.month
                    };
                    let _ = crate::climate::select_report(
                        &mut report,
                        month,
                        self.prefs.mode,
                        &self.prefs.reference,
                    );
                }
                self.report = Some(report);
                let quota = self.quota_status();
                if !quota.is_empty() {
                    self.notice = format!(
                        "{quota}. Missing data will retry automatically; completed months are saved and reused."
                    );
                }
            }
            Err(e) => {
                self.notice = format!("{e:#}");
            }
        }
        if !self.notice.is_empty() {
            self.ui.toast.push(tapp_ui::notification::Toast::plain(
                tapp_ui::theme::ToastKind::Warning,
                &self.notice,
            ));
        }
        self.normalize();
    }
    pub fn receive_partial(&mut self, row: Weather) {
        let focused = self.focus.as_deref() == Some(&row.city.id);
        let Some(report) = &mut self.report else {
            return;
        };
        // Keep one dataset, restoring absolute values before re-projecting deltas.
        let base_mode = if self.prefs.mode == Mode::Historical {
            Mode::Historical
        } else {
            Mode::Normal
        };
        if report.mode != base_mode && report.present(base_mode, &self.prefs.reference).is_err() {
            return;
        }
        if let Some(existing) = report.cities.iter_mut().find(|r| r.city.id == row.city.id) {
            *existing = row;
        } else {
            report.cities.push(row);
        }
        if self.prefs.mode == Mode::Comparison {
            // Until a usable reference arrives, leave absolute data available to
            // details; the main table blanks values that have no delta projection.
            let _ = report.present(Mode::Comparison, &self.prefs.reference);
        }
        self.revision = self.revision.wrapping_add(1);
        if focused {
            self.detail.invalidate_data();
        }
        self.normalize();
    }
    pub fn receive_hours(&mut self, id: String, result: Weather) {
        self.detail.invalidate_data();
        self.seeking = false;
        let Some(row) = self
            .report
            .as_mut()
            .and_then(|r| r.cities.iter_mut().find(|r| r.city.id == id))
        else {
            return;
        };
        if let Some(error) = result.error {
            self.notice = format!("Earlier hours unavailable: {error}");
            self.sought_days.insert(id, 92);
            return;
        }
        let anchor = row
            .hourly
            .get(self.detail.hour_offset)
            .map(|h| h.time.clone());
        let step_back = usize::from(self.detail.hour_offset == 0);
        let older = result
            .hourly
            .first()
            .is_some_and(|a| row.hourly.first().is_none_or(|b| a.time < b.time));
        if older {
            row.hourly = result.hourly;
            if self.focus.as_deref() == Some(&id)
                && let Some(time) = anchor
            {
                self.detail.hour_offset = row
                    .hourly
                    .iter()
                    .position(|h| h.time == time)
                    .unwrap_or(0)
                    .saturating_sub(step_back);
            }
            self.notice.clear();
        } else {
            self.sought_days.insert(id, 92);
            self.notice = "No earlier hours available from this source".into();
        }
    }
    fn navigate(&mut self, delta: isize) {
        if self.modal.is_some() {
            self.scroll = self
                .scroll
                .saturating_add_signed(delta)
                .min(self.max_scroll);
            return;
        }
        self.normalize();
        let indices = self.visible();
        if indices.is_empty() {
            return;
        }
        let target = match self.table.selected() {
            Some(index) if delta == 1 => (index + 1) % indices.len(),
            Some(index) if delta == -1 => (index + indices.len() - 1) % indices.len(),
            Some(index) => index.saturating_add_signed(delta).min(indices.len() - 1),
            None if delta == isize::MIN / 2 => 0,
            None if delta < 0 || delta == isize::MAX / 2 => indices.len() - 1,
            None => 0,
        };
        self.focus = self
            .report
            .as_ref()
            .map(|r| r.cities[indices[target]].city.id.clone());
        self.table.select(Some(target));
    }
    fn cycle_detail_city(&mut self, previous: bool) {
        let indices = self.visible();
        let Some(report) = &self.report else {
            return;
        };
        if indices.is_empty() {
            return;
        }
        let position = indices
            .iter()
            .position(|&i| Some(&report.cities[i].city.id) == self.focus.as_ref());
        let next = match position {
            Some(i) if previous => (i + indices.len() - 1) % indices.len(),
            Some(i) => (i + 1) % indices.len(),
            None if previous => indices.len() - 1,
            None => 0,
        };
        let target = report.cities[indices[next]].city.id.clone();
        if self.focus.as_ref() == Some(&target) {
            return;
        }
        let section = self.detail.section;
        let metric = self.detail.metric;
        let graph_requested = self.detail.graph_requested;
        let table_focus = self.detail.table_focus;
        self.focus = Some(target);
        self.detail = detail::State::default();
        self.detail.section = section;
        self.detail.metric = metric;
        self.detail.graph_requested = graph_requested;
        self.detail.table_focus = table_focus;
        self.scroll = 0;
        self.normalize();
    }
    pub fn invoke_command(&mut self, id: tapp_ui::commands::CommandId) -> Action {
        use crate::ui_shell::Command;
        self.ui.context = None;
        self.refresh_commands();
        if !self.ui.catalog.eligible(id) {
            return Action::None;
        }
        let Some(command) = Command::ALL.get(id.action as usize).copied() else {
            return Action::None;
        };
        if let Some(sort) = command.sort() {
            self.sort_by(sort);
            return Action::None;
        }
        if let Some(metric) = command.metric() {
            if let Some(key) = self.detail.metric_shortcut(metric) {
                return self.handle_local(Event::Key(KeyEvent::new(
                    KeyCode::Char(key),
                    KeyModifiers::NONE,
                )));
            }
            return Action::None;
        }
        match command {
            Command::SortSun => {
                self.choose_sun_sort();
                Action::None
            }
            Command::Period => {
                self.solar_sort_picker = false;
                self.comparison_picker = false;
                let mut picker = tapp_ui::palette::Palette::new(
                    Period::ALL
                        .iter()
                        .enumerate()
                        .map(|(id, p)| tapp_ui::palette::Command {
                            id,
                            label: p.label().into(),
                            shortcut: String::new(),
                        })
                        .collect(),
                );
                picker.select_id(self.prefs.period as usize);
                picker.focus.active = true;
                self.period_picker = Some(picker);
                Action::None
            }
            Command::PreviousMonth | Command::NextMonth => {
                self.prefs.month = (self.prefs.month
                    + if command == Command::PreviousMonth {
                        12
                    } else {
                        1
                    })
                    % 13;
                self.revision = self.revision.wrapping_add(1);
                if let Some(r) = &mut self.report {
                    let _ = crate::climate::select_report(
                        r,
                        self.prefs.month,
                        self.prefs.mode,
                        &self.prefs.reference,
                    );
                }
                Action::Reload(false)
            }
            Command::Normal
                if self.prefs.period != Period::Now || self.modal == Some(Modal::Detail) =>
            {
                self.switch_view(Mode::Normal, self.prefs.reference.clone())
            }
            Command::Compare => {
                let mut choices = vec![];
                let pinned = self
                    .config
                    .cities
                    .iter()
                    .find(|c| c.id == self.prefs.reference)
                    .map(|c| c.name.clone())
                    .or_else(|| {
                        self.report
                            .as_ref()?
                            .cities
                            .iter()
                            .find(|r| r.city.id == self.prefs.reference)
                            .map(|r| r.city.name.clone())
                    });
                if let Some(name) = pinned {
                    choices.push(tapp_ui::palette::Command {
                        id: 4,
                        label: format!("Pinned city · {name}"),
                        shortcut: String::new(),
                    });
                }
                choices.extend(
                    Period::ALL
                        .into_iter()
                        .filter(|p| *p != self.prefs.period)
                        .map(|p| tapp_ui::palette::Command {
                            id: p as usize,
                            label: p.label().into(),
                            shortcut: String::new(),
                        }),
                );
                let mut picker = tapp_ui::palette::Palette::new(choices);
                picker.select_id(self.prefs.comparison_period.map_or(4, |p| p as usize));
                picker.focus.active = true;
                self.comparison_picker = true;
                self.period_picker = Some(picker);
                Action::None
            }
            Command::Refresh | Command::Retry => {
                self.alternate_report = None;
                Action::Reload(command == Command::Refresh)
            }
            Command::List => {
                self.modal = None;
                self.detail = detail::State::default();
                self.normalize();
                Action::None
            }
            Command::Quit => Action::Quit,
            Command::Theme => Action::Theme,
            Command::Help => {
                if self.modal == Some(Modal::Help)
                    && self.ui.palette_open
                    && !self.ui.help_over_palette
                {
                    self.ui.help_over_palette = true;
                    return Action::None;
                }
                self.ui.help_over_palette = self.ui.palette_open;
                self.handle_local(Event::Key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE)))
            }
            Command::Info => {
                if self.modal == Some(Modal::Detail) {
                    self.handle_local(Event::Key(KeyEvent::new(
                        KeyCode::Char('i'),
                        KeyModifiers::NONE,
                    )))
                } else {
                    self.ui.info.set_entries(self.info_entries());
                    self.ui.info_open = true;
                    Action::None
                }
            }
            Command::Clear => {
                self.prefs.filter.clear();
                self.normalize();
                Action::None
            }
            Command::CitySearch => {
                if let Some(m) = &mut self.city_manager {
                    m.results.clear();
                    m.search.invalidate();
                    m.search.loading = false;
                }
                self.handle_local(Event::Key(KeyEvent::new(
                    KeyCode::Enter,
                    KeyModifiers::NONE,
                )))
            }
            Command::CityAccept => Action::AddCity,
            Command::CityRemove => Action::RemoveCity,
            Command::ColumnsLeft | Command::ColumnsRight => {
                self.handle_local(Event::Key(KeyEvent::new(
                    if command == Command::ColumnsLeft {
                        KeyCode::Left
                    } else {
                        KeyCode::Right
                    },
                    KeyModifiers::ALT,
                )))
            }
            other => other
                .key()
                .map(|key| self.handle_local(Event::Key(KeyEvent::new(key, KeyModifiers::NONE))))
                .unwrap_or(Action::None),
        }
    }
    pub fn handle(&mut self, event: Event) -> Action {
        if let Event::BackgroundColor(r, g, b) = event {
            tapp_ui::theme::set_terminal_background(r, g, b);
            return Action::None;
        }
        use tapp_ui::{commands::Resolution, context_menu::CommandMenuEvent, focus::Outcome};
        if let Event::Mouse(m) = &event
            && m.kind == MouseEventKind::Down(MouseButton::Left)
            && self.ui.toast.dismiss_at((m.column, m.row).into())
        {
            return Action::None;
        }
        if let Event::Key(k) = &event
            && k.kind == KeyEventKind::Release
        {
            return Action::None;
        }
        self.refresh_commands();
        if self.period_picker.is_some()
            && !self.ui.palette_open
            && self.modal != Some(Modal::Help)
            && !matches!(&event,Event::Key(k) if matches!(k.code,KeyCode::F(1)|KeyCode::F(2)) || matches!(k.code,KeyCode::Char('?'|'k')) && k.modifiers.contains(KeyModifiers::CONTROL))
        {
            let picker = self.period_picker.as_mut().unwrap();
            match picker.handle(&event) {
                Outcome::Submit => {
                    let id = picker.selected_id();
                    self.period_picker = None;
                    self.ui.context = None;
                    if self.solar_sort_picker {
                        self.solar_sort_picker = false;
                        if let Some(id) = id {
                            self.prefs.sort = [Sort::Sunrise, Sort::Sunset, Sort::Daylight][id / 2];
                            self.prefs.reverse = id % 2 == 1;
                            self.normalize();
                        }
                        return Action::None;
                    }
                    if self.comparison_picker {
                        self.comparison_picker = false;
                        if let Some(id) = id {
                            if id == 4 {
                                let changed = self.prefs.comparison_period.take().is_some();
                                if let Some(report) = &mut self.report {
                                    report.comparison_period = None;
                                }
                                if changed {
                                    self.prefs.mode = Mode::Normal;
                                }
                                return self
                                    .switch_view(Mode::Comparison, self.prefs.reference.clone());
                            }
                            self.prefs.compare_period(Period::ALL[id]);
                            self.report = None;
                            self.alternate_report = None;
                            self.detail.invalidate_data();
                            return Action::Reload(false);
                        }
                        return Action::None;
                    }
                    let period = id.map(|i| Period::ALL[i]);
                    if let Some(period) = period
                        && self.prefs.period != period
                    {
                        self.prefs.period = period;
                        if self.prefs.mode == Mode::Comparison
                            && let Some(base) = self.prefs.comparison_period
                        {
                            if period == base {
                                self.prefs.mode = Mode::Normal;
                                self.prefs.comparison_period = None;
                            } else {
                                self.prefs.compare_period(base);
                            }
                        }
                        self.report = None;
                        self.alternate_report = None;
                        let table_focus = self.detail.table_focus;
                        let (metric, section, graph) = (
                            self.detail.metric,
                            self.detail.section,
                            self.detail.graph_requested,
                        );
                        self.detail = detail::State::default();
                        self.detail.metric = metric;
                        self.detail.section = section;
                        self.detail.graph_requested = graph;
                        self.detail.table_focus = table_focus;
                        return Action::Reload(false);
                    }
                }
                Outcome::FocusReleased(_) => {
                    self.period_picker = None;
                    self.solar_sort_picker = false;
                    self.ui.context = None;
                }
                _ => {}
            }
            return Action::None;
        }
        if let Event::Key(k) = &event
            && (k.code == KeyCode::F(1)
                || (k.code == KeyCode::Char('?') && k.modifiers.contains(KeyModifiers::CONTROL)))
        {
            return self.invoke_command(tapp_ui::commands::CommandId {
                owner: 1,
                action: crate::ui_shell::Command::Help as u64,
            });
        }
        if self.modal == Some(Modal::Help)
            && (self.ui.help_over_palette && self.ui.palette_open
                || self.ui.info_open && !self.ui.palette_open)
            && !matches!(&event, Event::Key(k) if (k.code == KeyCode::Char('k') && k.modifiers == KeyModifiers::CONTROL) || k.code == KeyCode::F(2))
        {
            if matches!(self.ui.help.handle(&event), Outcome::FocusReleased(_)) {
                self.modal = self.help_return.take();
                self.ui.context = None;
            }
            return Action::None;
        }
        if let Event::Key(k) = &event
            && ((k.code == KeyCode::Char('k') && k.modifiers == KeyModifiers::CONTROL)
                || (k.code == KeyCode::F(2) && k.modifiers == KeyModifiers::NONE))
        {
            self.ui.palette_open = (self.modal == Some(Modal::Help) && self.ui.help_over_palette)
                || !self.ui.palette_open;
            self.ui.help_over_palette = false;
            self.ui.menu_open = false;
            if self.ui.palette_open {
                self.ui.context = None;
                self.refresh_commands();
                self.ui.palette.refresh(&self.ui.catalog);
            }
            return Action::None;
        }
        if self.ui.palette_open {
            self.ui.palette.palette.focus.active = true;
            match self.ui.palette.palette.handle(&event) {
                Outcome::FocusReleased(_) => self.ui.palette_open = false,
                Outcome::Submit => {
                    let id = self.ui.palette.selected(&self.ui.catalog);
                    self.ui.palette_open = false;
                    if let Some(id) = id {
                        return self.invoke_command(id);
                    }
                }
                _ => {}
            }
            return Action::None;
        }
        if self.ui.menu_open {
            match self.ui.menu.handle(&event, &self.ui.catalog) {
                CommandMenuEvent::Dismiss => self.ui.menu_open = false,
                CommandMenuEvent::Invoke(id) => {
                    self.ui.menu_open = false;
                    return self.invoke_command(id);
                }
                _ => {}
            }
            return Action::None;
        }
        let typing = self.modal != Some(Modal::Help)
            && !self.ui.info_open
            && (self.editing
                || self
                    .city_manager
                    .as_ref()
                    .is_some_and(|m| m.removal.is_none()));
        if let Event::Key(k) = &event
            && self.ui.catalog.suppressed(*k, typing)
        {
            return Action::None;
        }
        if let Event::Key(k) = &event
            && let Resolution::Command(id) = self.ui.catalog.resolve(*k, typing)
        {
            return self.invoke_command(id);
        }
        if self.ui.info_open {
            match self.ui.info.handle(&event) {
                Outcome::FocusReleased(_) => {
                    self.ui.info_open = false;
                    self.ui.context = None;
                }
                Outcome::CopyRequested(text) => {
                    self.ui.copy = Some(text);
                    return Action::Copy;
                }
                _ => {}
            }
            return Action::None;
        }
        if self.modal == Some(Modal::Help) {
            if matches!(self.ui.help.handle(&event), Outcome::FocusReleased(_)) {
                self.modal = self.help_return.take();
                self.ui.context = None;
            }
            return Action::None;
        }
        if self.editing {
            self.ui.filter.input.focus.active = true;
            if self.ui.filter.input.value() != self.prefs.filter {
                self.ui.filter.input.set(self.prefs.filter.clone());
            }
            if matches!(
                self.ui.filter.handle(&event),
                Outcome::Submit | Outcome::FocusReleased(_)
            ) {
                self.editing = false;
            }
            self.prefs.filter = self.ui.filter.input.value().to_owned();
            self.cursor = self.ui.filter.input.cursor();
            self.normalize();
            return Action::None;
        }
        if let Event::Mouse(m) = &event
            && m.kind == MouseEventKind::Down(MouseButton::Right)
            && self.city_manager.is_none()
        {
            if self.modal.is_none()
                && self.table_area.contains((m.column, m.row).into())
                && self
                    .table_layout
                    .body(self.table_area, 1)
                    .contains((m.column, m.row).into())
            {
                let Some(hit) = self.table_layout.rows.hit(
                    m.row - self.table_layout.body(self.table_area, 1).y,
                    self.table_layout.body(self.table_area, 1).height,
                ) else {
                    return Action::None;
                };
                let row = hit + self.table.offset();
                if let Some(i) = self.visible().get(row).copied() {
                    self.focus = self.report.as_ref().map(|r| r.cities[i].city.id.clone());
                }
            }
            self.ui.context = None;
            self.refresh_commands();
            self.ui.menu.menu.position = (m.column, m.row);
            self.ui
                .menu
                .refresh(self.ui.catalog.commands().map(|c| c.id), &self.ui.catalog);
            self.ui.menu_open = true;
            return Action::None;
        }
        if let Event::Key(k) = &event
            && self.city_manager.is_none()
            && k.modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
        {
            return Action::None;
        }
        self.handle_local(event)
    }
    fn handle_local(&mut self, event: Event) -> Action {
        if let Event::Key(key) = &event
            && key.kind != KeyEventKind::Release
            && (key.code == KeyCode::F(1)
                || (key.code == KeyCode::Char('?') && !self.editing && self.city_manager.is_none()))
        {
            if self.modal == Some(Modal::Help) {
                self.modal = self.help_return.take();
            } else {
                self.prepare_help();
                self.help_return = self.modal;
                self.modal = Some(Modal::Help);
                self.scroll = 0;
            }
            return Action::None;
        }
        if self.modal == Some(Modal::Help)
            && matches!(&event, Event::Key(k) if k.kind != KeyEventKind::Release && k.code == KeyCode::Esc)
        {
            self.modal = self.help_return.take();
            return Action::None;
        }
        if self.modal != Some(Modal::Help)
            && let Some(manager) = &mut self.city_manager
        {
            manager.search.focus.active = true;
            manager.search.input.focus.active = true;
            return match manager.handle(&event) {
                cities::Intent::None => Action::None,
                cities::Intent::Close => {
                    manager.request_id = 0;
                    manager.search.loading = false;
                    self.saved_search = self.city_manager.take();
                    Action::None
                }
                cities::Intent::Search => Action::SearchCities,
                cities::Intent::Add => Action::AddCity,
                cities::Intent::Remove => Action::RemoveCity,
                cities::Intent::Quit => Action::Quit,
            };
        }
        if self.modal == Some(Modal::Detail)
            && let Event::Key(key) = &event
            && key.kind != KeyEventKind::Release
            && matches!(key.code, KeyCode::Char('[' | ']'))
        {
            self.cycle_detail_city(key.code == KeyCode::Char('['));
            return Action::None;
        }
        if self.modal == Some(Modal::Detail)
            && matches!(
                self.detail.section,
                detail::Section::Hours | detail::Section::Graph
            )
            && self.detail.hour_offset == 0
            && (self.detail.section != detail::Section::Graph || !self.detail.graph_daily())
            && !self.seeking
            && !self.loading
            && !self.demo
            && self.prefs.period == Period::Now
            && (matches!(&event, Event::Key(k) if k.kind != KeyEventKind::Release && matches!(k.code, KeyCode::Up | KeyCode::PageUp | KeyCode::Char('k')))
                || matches!(&event, Event::Mouse(m) if m.kind == MouseEventKind::ScrollUp && !m.modifiers.contains(KeyModifiers::SHIFT) && (self.detail.hour_area.contains((m.column,m.row).into()) || self.detail.graph_area.contains((m.column,m.row).into()))))
            && self
                .focus
                .as_ref()
                .is_some_and(|id| self.sought_days.get(id).copied().unwrap_or(self.past_days) < 92)
        {
            return Action::OlderHours;
        }
        if self.modal == Some(Modal::Detail)
            && let Some(row) = self.report.as_ref().and_then(|r| {
                r.cities
                    .iter()
                    .find(|r| Some(r.city.id.as_str()) == self.focus.as_deref())
            })
            && self.detail.handle(&event, row)
        {
            return Action::None;
        }
        match event {
            Event::Paste(text) if self.editing && self.modal != Some(Modal::Help) => {
                let text = view::clean(&text);
                self.prefs.filter.insert_str(self.cursor, &text);
                self.cursor += text.len();
                self.normalize();
            }
            Event::Mouse(mouse) => self.mouse(mouse),
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    return Action::Quit;
                }
                if self.editing && self.modal != Some(Modal::Help) {
                    self.ui.filter.input.handle(&Event::Key(key));
                    return Action::None;
                }
                if key.code == KeyCode::Char('q') {
                    return Action::Quit;
                }
                let page = isize::try_from(
                    self.table_layout
                        .rows
                        .capacity(self.table_layout.body(self.table_area, 1).height),
                )
                .unwrap_or(1)
                .max(1);
                match key.code {
                    KeyCode::Left | KeyCode::Right if key.modifiers.contains(KeyModifiers::ALT) => {
                        self.column_offset = self.column_geometry.advance(
                            self.column_offset,
                            crate::viewport::Bindings::default()
                                .direction(&Event::Key(key))
                                .unwrap_or(0),
                        );
                    }
                    KeyCode::Esc => {
                        if self.modal.is_some() {
                            self.modal = None;
                        } else {
                            self.focus = None;
                            self.table.select(None);
                        }
                    }
                    KeyCode::Char('?') => {
                        self.modal = Some(Modal::Help);
                        self.scroll = 0;
                    }
                    KeyCode::Down | KeyCode::Char('j') => self.navigate(1),
                    KeyCode::Up | KeyCode::Char('k') => self.navigate(-1),
                    KeyCode::PageDown => self.navigate(page),
                    KeyCode::PageUp => self.navigate(-page),
                    KeyCode::Home => self.navigate(isize::MIN / 2),
                    KeyCode::End => self.navigate(isize::MAX / 2),
                    _ if self.modal.is_some() => {}
                    KeyCode::Char('a') => {
                        self.city_manager = Some(
                            self.saved_search
                                .take()
                                .filter(|s| s.removal.is_none())
                                .unwrap_or_default(),
                        )
                    }
                    KeyCode::Char('d') => {
                        if let Some(city) = self
                            .report
                            .as_ref()
                            .and_then(|r| {
                                r.cities
                                    .iter()
                                    .find(|r| Some(&r.city.id) == self.focus.as_ref())
                            })
                            .map(|r| r.city.clone())
                        {
                            self.city_manager = Some(cities::State {
                                removal: Some(city),
                                ..Default::default()
                            });
                        } else {
                            self.ui.toast.push(tapp_ui::notification::Toast::plain(
                                tapp_ui::theme::ToastKind::Note,
                                "Select a city with ↑/↓ first",
                            ));
                        }
                    }
                    KeyCode::Enter => {
                        if self.focus.is_none() {
                            let index = self.pinned().or_else(|| self.visible().first().copied());
                            self.focus = index.and_then(|i| {
                                self.report.as_ref().map(|r| r.cities[i].city.id.clone())
                            });
                        }
                        if self.focus.is_none() {
                            return Action::None;
                        }
                        self.modal = Some(Modal::Detail);
                        self.scroll = 0;
                        self.detail = detail::State::default();
                    }
                    KeyCode::Char('/') => {
                        self.editing = true;
                        self.ui.filter.begin(&self.prefs.filter);
                        self.cursor = self.prefs.filter.len();
                    }
                    KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.prefs.filter.clear();
                        self.cursor = 0;
                        self.normalize();
                    }
                    KeyCode::Char('o') => {
                        self.prefs.reverse = !self.prefs.reverse;
                        self.normalize();
                    }
                    KeyCode::Char(c)
                        if crate::ui_shell::TABLE_SORTS
                            .iter()
                            .any(|(_, _, key, _)| *key == c) =>
                    {
                        let command = crate::ui_shell::TABLE_SORTS
                            .iter()
                            .find(|(_, _, key, _)| *key == c)
                            .unwrap()
                            .0;
                        return self.invoke_command(tapp_ui::commands::CommandId {
                            owner: 1,
                            action: command as u64,
                        });
                    }
                    KeyCode::Char('n') => {
                        return self.switch_view(Mode::Normal, self.prefs.reference.clone());
                    }
                    KeyCode::Char('c') => {
                        return self.switch_view(Mode::Comparison, self.prefs.reference.clone());
                    }
                    KeyCode::Char('s') => {
                        self.prefs.source = self.prefs.source.next();
                        self.alternate_report = None;
                        return Action::Reload(false);
                    }
                    KeyCode::Char('r') => {
                        if let Some(id) = &self.focus {
                            return self.switch_view(self.prefs.mode, id.clone());
                        }
                    }
                    KeyCode::Char('u') => {
                        self.alternate_report = None;
                        return Action::Reload(true);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Action::None
    }
    fn choose_sun_sort(&mut self) {
        let mut picker = tapp_ui::palette::Palette::new(
            ["Sunrise", "Sunset", "Daylight"]
                .into_iter()
                .enumerate()
                .flat_map(|(i, label)| {
                    [false, true]
                        .into_iter()
                        .map(move |desc| tapp_ui::palette::Command {
                            id: i * 2 + usize::from(desc),
                            label: format!("{label} {}", if desc { "DESC" } else { "ASC" }),
                            shortcut: String::new(),
                        })
                })
                .collect(),
        );
        let selected = [Sort::Sunrise, Sort::Sunset, Sort::Daylight]
            .iter()
            .position(|sort| *sort == self.prefs.sort)
            .map_or(4, |i| i * 2 + usize::from(self.prefs.reverse));
        picker.select_id(selected);
        picker.focus.active = true;
        self.solar_sort_picker = true;
        self.comparison_picker = false;
        self.period_picker = Some(picker);
    }
    fn mouse(&mut self, mouse: MouseEvent) {
        if self.modal.is_none() && mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            if self.sun_header.contains((mouse.column, mouse.row).into()) {
                self.choose_sun_sort();
                return;
            }
            if let Some((_, sort)) = self
                .sort_hits
                .iter()
                .find(|(rect, _)| rect.contains((mouse.column, mouse.row).into()))
            {
                let sort = *sort;
                self.sort_by(sort);
                return;
            }
            if self.pinned_area.contains((mouse.column, mouse.row).into())
                && let Some(i) = self.pinned()
            {
                self.focus = self.report.as_ref().map(|r| r.cities[i].city.id.clone());
                self.modal = Some(Modal::Detail);
                self.detail = detail::State::default();
                return;
            }
        }
        if let Some(direction) =
            crate::viewport::Bindings::default().direction(&Event::Mouse(mouse))
        {
            if self.table_area.contains((mouse.column, mouse.row).into())
                || self.pinned_area.contains((mouse.column, mouse.row).into())
            {
                self.column_offset = self.column_geometry.advance(self.column_offset, direction);
            }
            return;
        }
        match mouse.kind {
            MouseEventKind::ScrollDown => self.navigate(3),
            MouseEventKind::ScrollUp => self.navigate(-3),
            MouseEventKind::Down(MouseButton::Left)
                if self.modal.is_none()
                    && self.table_area.contains((mouse.column, mouse.row).into())
                    && self
                        .table_layout
                        .body(self.table_area, 1)
                        .contains((mouse.column, mouse.row).into()) =>
            {
                let Some(hit) = self.table_layout.rows.hit(
                    mouse.row - self.table_layout.body(self.table_area, 1).y,
                    self.table_layout.body(self.table_area, 1).height,
                ) else {
                    return;
                };
                let row = hit + self.table.offset();
                let indices = self.visible();
                if let Some(&i) = indices.get(row)
                    && let Some(report) = &self.report
                {
                    let target = report.cities[i].city.id.clone();
                    if self.focus.as_deref() == Some(&target) {
                        self.modal = Some(Modal::Detail);
                        self.scroll = 0;
                        self.detail = detail::State::default();
                    }
                    self.focus = Some(target);
                    self.table.select(Some(row));
                }
            }
            _ => {}
        }
    }
}
fn main_row(
    row: &Weather,
    report: &Report,
    prefs: &Preferences,
    cols: &[(&str, &str)],
    city_width: usize,
    theme: &Theme,
    selected: bool,
) -> Row<'static> {
    let markers = format!(
        "{}{}{}",
        if row.city.id == prefs.reference {
            " *"
        } else {
            ""
        },
        if Some(&row.city.id) == report.current_city.as_ref() {
            " @"
        } else {
            ""
        },
        if row.flagged() { " !" } else { "" }
    );
    let name = Line::from(vec![
        Span::raw(" "),
        Span::raw(
            view::fit(
                &row.city.name,
                city_width.saturating_sub(markers.width() + 1),
            )
            .trim_end()
            .to_owned(),
        ),
        Span::styled(markers, theme.muted),
    ]);
    let clock = view::clock_text(row, true);
    let clock = if clock.width() < city_width {
        clock
    } else {
        view::clock_text(row, false)
    };
    let mut cells = vec![Cell::from(Text::from(vec![
        name,
        Line::styled(
            format!(" {}", view::fit(&clock, city_width.saturating_sub(1))),
            theme.muted,
        ),
    ]))];
    let pending = row.time.is_empty() && row.error.is_none();
    if prefs.period == Period::Now && prefs.mode != Mode::Historical {
        cells.push(if pending {
            Cell::from("")
        } else {
            Cell::from(theme.condition_line(row.condition.as_ref()))
        });
    }
    let absolute = row.absolute_values.as_ref().unwrap_or(&row.values);
    let ranges = row.absolute_ranges.as_ref().unwrap_or(&row.ranges);
    for (key, _) in cols {
        if pending || (prefs.mode == Mode::Comparison && row.absolute_values.is_none()) {
            cells.push(Cell::from(""));
            continue;
        }
        let text = view::range_text(row, key, true);
        if *key == "sun" {
            let solar = |key: &str| {
                crate::solar::text(
                    key,
                    row.values.get(key).copied().flatten(),
                    prefs.mode == Mode::Comparison,
                )
            };
            cells.push(Cell::from(Text::from(vec![
                Line::from(format!("{} / {}", solar("sunrise"), solar("sunset"))),
                Line::styled(solar("daylight"), theme.muted),
            ])));
            continue;
        }
        let range = ranges.get(*key);
        cells.push(Cell::from(Text::from(vec![
            Line::from(theme.metric_span(
                view::metric_text(
                    key,
                    row.values.get(*key).copied().flatten(),
                    prefs.mode == Mode::Comparison,
                ),
                key,
                absolute.get(*key).copied().flatten(),
                prefs.mode == Mode::Historical,
            )),
            theme.secondary_range(
                &text,
                key,
                range.and_then(|r| r.min),
                range.and_then(|r| r.max),
                false,
            ),
        ])));
    }
    Row::new(cells).height(2).style(if selected {
        theme.selected
    } else {
        Default::default()
    })
}
pub fn draw(frame: &mut Frame, app: &mut App) {
    draw_in(frame, frame.area(), app);
}
pub fn draw_in(frame: &mut Frame, area: Rect, app: &mut App) {
    draw_body(frame, area, app);
    let overlay = app.modal == Some(Modal::Help)
        || app.ui.info_open
        || app.ui.palette_open
        || app.ui.menu_open;
    if app.editing {
        app.ui.filter.input.monochrome = app.prefs.monochrome;
        app.ui.filter.input.focus.active = !overlay && app.city_manager.is_none();
        app.ui.filter.draw(
            frame,
            Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1)),
            "Filter cities · live",
        );
    }
    if let Some(manager) = &mut app.city_manager {
        manager.search.focus.active = !overlay;
        manager.draw(
            frame,
            area,
            &app.config,
            app.demo,
            &Theme::new(app.prefs.monochrome).with_palette(app.ui.theme),
        );
    }
    if let Some(picker) = &mut app.period_picker {
        picker.monochrome = app.prefs.monochrome;
        picker.selection = tapp_ui::table::RowSelection::for_theme(app.ui.theme);
        picker.focus.active = app.modal != Some(Modal::Help);
        picker.draw(
            frame,
            area,
            if app.solar_sort_picker {
                "Sort Sun"
            } else if app.comparison_picker {
                "Compare with"
            } else {
                "Period"
            },
        );
    }
    if app.ui.info_open {
        app.ui.info.monochrome = app.prefs.monochrome;
        app.ui.info.draw(frame, area, "Info");
    }
    if app.modal == Some(Modal::Help) && !app.ui.help_over_palette {
        app.ui.help.monochrome = app.prefs.monochrome;
        app.ui.help.draw(frame, area, "Help");
    }
    if app.ui.menu_open {
        app.ui
            .menu
            .draw(frame, area, "City actions", &app.ui.catalog);
    }
    if app.ui.palette_open {
        app.ui.palette.palette.selection = tapp_ui::table::RowSelection::for_theme(app.ui.theme);
        app.ui.palette.palette.monochrome = app.prefs.monochrome;
        app.ui.palette.palette.focus.active =
            app.modal != Some(Modal::Help) || !app.ui.help_over_palette;
        app.ui.palette.palette.draw(frame, area, "Commands");
    }
    if app.modal == Some(Modal::Help) && app.ui.help_over_palette {
        app.ui.help.monochrome = app.prefs.monochrome;
        app.ui.help.draw(frame, area, "Help");
    }
    {
        app.ui.toast.draw(
            frame,
            Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1)),
            app.prefs.monochrome,
        );
    }
    if !app.prefs.monochrome {
        app.ui.theme.apply(frame.buffer_mut(), area);
    }
}
fn draw_body(frame: &mut Frame, area: Rect, app: &mut App) {
    app.detail.graph_tabs.clear();
    if app.prefs.period != Period::Now {
        let month = if app.session_preferences().last_city.is_some() {
            0
        } else {
            app.prefs.month
        };
        if let Some(report) = &mut app.report
            && report
                .cities
                .iter()
                .any(|r| r.baseline.as_ref().is_some_and(|b| b.month != month))
        {
            let _ =
                crate::climate::select_report(report, month, app.prefs.mode, &app.prefs.reference);
            app.revision = app.revision.wrapping_add(1);
            app.detail.invalidate_data();
        }
    }
    let theme = Theme::new(app.prefs.monochrome).with_palette(app.ui.theme);
    if area.is_empty() {
        return;
    }
    if area.width < 32 || area.height < 9 {
        frame.render_widget(
            Paragraph::new("q quits · resize 32×9"),
            Rect::new(
                area.x,
                area.y + area.height.saturating_sub(1),
                area.width,
                1,
            ),
        );
        return;
    }
    app.normalize();
    let base_modal = if app.modal == Some(Modal::Help) {
        app.help_return
    } else {
        app.modal
    };
    if base_modal == Some(Modal::Detail)
        && let Some(row) = app.report.as_ref().and_then(|r| {
            r.cities
                .iter()
                .find(|r| Some(r.city.id.as_str()) == app.focus.as_deref())
        })
    {
        app.detail.loading = app.loading || app.seeking;
        app.detail.waiting = !app.quota_status().is_empty();
        app.detail.status = {
            match app
                .prefs
                .comparison_period
                .filter(|_| app.prefs.mode == Mode::Comparison)
            {
                Some(base) => format!("{} − {}", app.prefs.period.label(), base.label()),
                None => format!("{} · {}", app.prefs.period.label(), app.prefs.mode.label()),
            }
        };
        app.detail.pending_history = app.loading
            && row.time.is_empty()
            && row.error.is_none()
            && (app.prefs.period != Period::Now || app.prefs.mode == Mode::Historical);
        app.detail.loading_symbol = app.loading_indicator.symbol().to_owned();
        let mut shown = row.clone();
        if app.prefs.mode == Mode::Comparison {
            if let Some(base) = shown.comparison_base.take() {
                crate::climate::compare_details(&mut shown, &base);
                shown.comparison_base = Some(base);
            } else if app.prefs.comparison_period.is_none()
                && let Some(reference) = app
                    .report
                    .as_ref()
                    .and_then(|r| r.cities.iter().find(|r| r.city.id == app.prefs.reference))
            {
                crate::climate::compare_details(&mut shown, reference);
            }
            if shown.absolute_values.is_none() {
                for v in shown.values.values_mut() {
                    *v = None;
                }
                shown.ranges.clear();
                for month in &mut shown.monthly {
                    for v in month.values.values_mut() {
                        *v = None;
                    }
                    month.ranges.clear();
                }
            }
        }
        shown.monthly.retain(|m| m.month != 0);
        detail::draw(frame, area, &shown, &mut app.detail, &theme);
        return;
    }
    app.table_area = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1));
    let table_style = tapp_ui::table::TableStyle {
        theme: app.ui.theme,
        separators: tapp_ui::table::TableOptions {
            columns: tapp_ui::table::Separator::Never,
            ..Default::default()
        },
        borders: ratatui::widgets::Borders::TOP,
        header_style: app
            .prefs
            .monochrome
            .then(|| tapp_ui::theme::Role::Command.style(true)),
        rule_style: app
            .prefs
            .monochrome
            .then(|| tapp_ui::theme::Role::Muted.style(true)),
    };
    {
        let indices = app.visible();
        let cols = view::main_columns(area.width, app.prefs.mode, app.column_offset);
        let icons = app.prefs.period == Period::Now && app.prefs.mode != Mode::Historical;
        let city_width = usize::from((area.width / 4).clamp(12, 28));
        let widths = std::iter::once(Constraint::Length(city_width as u16))
            .chain(if icons {
                Some(Constraint::Length(view::ICON_WIDTH))
            } else {
                None
            })
            .chain(
                cols.iter()
                    .map(|(key, _)| Constraint::Length(view::main_column_width(key))),
            )
            .collect::<Vec<_>>();
        let frozen = 1 + usize::from(icons);
        let fixed_width = crate::viewport::frozen_width(&widths, frozen).min(area.width);
        app.column_geometry = crate::viewport::Geometry::new(&widths, area.width, frozen);
        app.column_max = app.column_geometry.max;
        app.column_offset = app.column_offset.min(app.column_max);
        let shift = app.column_offset as u16;
        let mut headers = vec!["City".to_string()];
        if icons {
            headers.push(String::new());
        }
        headers.extend(cols.iter().map(|(k, l)| {
            if *k == "sun" {
                return if [Sort::Sunrise, Sort::Sunset, Sort::Daylight].contains(&app.prefs.sort) {
                    format!(
                        "{}{}",
                        app.prefs.sort.label(),
                        if app.prefs.reverse { "↓" } else { "↑" }
                    )
                } else {
                    "Sun ▾".into()
                };
            }
            let family = app.prefs.sort.family();
            let active = *k == family.map_or(app.prefs.sort, |f| f[0]).key();
            let bound = match family.filter(|_| active) {
                Some([_, min, _]) if app.prefs.sort == min => " min",
                Some([_, _, max]) if app.prefs.sort == max => " max",
                _ => "",
            };
            format!(
                "{}{bound}{}",
                if bound.is_empty() {
                    *l
                } else {
                    match family.unwrap()[0] {
                        Sort::Humidity => "RH",
                        Sort::Pressure => "hPa",
                        base => base.label(),
                    }
                },
                if active {
                    if app.prefs.reverse { "↓" } else { "↑" }
                } else {
                    ""
                }
            )
        }));
        if app.prefs.sort == Sort::City {
            headers[0].push(if app.prefs.reverse { '↓' } else { '↑' });
        }
        let header = Row::new(headers);
        app.pinned_area = Rect::default();
        if let Some(i) = app.pinned() {
            let top = 0;
            let report = app.report.as_ref().unwrap();
            app.pinned_area = Rect::new(area.x, area.y + top, area.width, 2);
            let pinned = main_row(
                &report.cities[i],
                report,
                &app.prefs,
                &cols,
                city_width,
                &theme,
                false,
            );
            crate::viewport::draw_frozen(
                frame,
                app.pinned_area,
                Table::new([pinned], widths.clone()).column_spacing(1),
                &widths,
                app.column_offset,
                frozen,
            );
            app.table_area = Rect::new(
                area.x,
                area.y + top + 2,
                area.width,
                area.height.saturating_sub(top + 3),
            );
        }
        app.table_layout = table_style.layout(app.table_area, 2);
        let header_area = app.table_layout.header_area(app.table_area, 1);
        app.sort_hits.clear();
        app.sun_header = Rect::default();
        let mut column_gaps = Vec::with_capacity(cols.len() + 1);
        let mut x = area.x;
        app.sort_hits.push((
            Rect::new(x, header_area.y, city_width as u16, 1),
            Sort::City,
        ));
        column_gaps.push(city_width as u16);
        x += city_width as u16 + 1;
        if icons {
            app.sort_hits.push((
                Rect::new(x, header_area.y, view::ICON_WIDTH, 1),
                Sort::Weather,
            ));
            column_gaps.push(x - area.x + view::ICON_WIDTH);
            x += view::ICON_WIDTH + 1;
        }
        for (key, _) in &cols {
            let width = view::main_column_width(key);
            if *key == "sun" {
                app.sun_header = Rect::new(x, header_area.y, width, 1);
            }
            if let Some(sort) = Sort::ALL.iter().find(|s| s.key() == *key) {
                app.sort_hits
                    .push((Rect::new(x, header_area.y, width, 1), *sort));
            }
            column_gaps.push(x - area.x + width);
            x += width + 1;
        }
        column_gaps.pop(); // No trailing vertical rule.
        let metric_area = Rect::new(
            header_area.x + fixed_width,
            header_area.y,
            header_area.width.saturating_sub(fixed_width),
            header_area.height,
        );
        for (hit, _) in app.sort_hits.iter_mut().skip(frozen) {
            *hit = crate::viewport::hit(*hit, metric_area, shift);
        }
        app.sun_header = crate::viewport::hit(app.sun_header, metric_area, shift);
        let body = app.table_layout.body(app.table_area, 1);
        let visible_rows = app.table_layout.rows.capacity(body.height);
        let selected = app.table.selected();
        let window = tapp_ui::table::window(
            indices.len(),
            selected,
            app.table.offset_mut(),
            visible_rows,
        );
        app.visible_ids = app
            .report
            .as_ref()
            .map(|r| {
                indices[window.clone()]
                    .iter()
                    .map(|&i| r.cities[i].city.id.clone())
                    .collect()
            })
            .unwrap_or_default();
        let row_count = window.len();
        let rows = indices[window]
            .iter()
            .enumerate()
            .map(|(position, &i)| {
                let report = app.report.as_ref().expect("visible report");
                let row = app.table_layout.rows.row(main_row(
                    &report.cities[i],
                    report,
                    &app.prefs,
                    &cols,
                    city_width,
                    &theme,
                    app.focus.as_deref() == Some(&report.cities[i].city.id),
                ));
                if position + 1 == row_count {
                    row.bottom_margin(0)
                } else {
                    row
                }
            })
            .collect::<Vec<_>>();
        crate::viewport::draw_frozen(
            frame,
            header_area,
            Table::new([app.table_layout.header(header)], widths.clone()).column_spacing(1),
            &widths,
            app.column_offset,
            frozen,
        );
        crate::viewport::draw_frozen(
            frame,
            body,
            Table::new(rows, widths.clone()).column_spacing(1),
            &widths,
            app.column_offset,
            frozen,
        );
        app.table_layout
            .draw(frame, app.table_area, 1, row_count, &column_gaps);
        if indices.is_empty() {
            frame.render_widget(
                Paragraph::new(if app.loading {
                    ""
                } else {
                    "No matching cities. / filter; Ctrl-U clears."
                }),
                Rect::new(body.x, body.y, body.width, body.height.min(1)),
            );
        }
    }
    let right = if app.editing
        || app
            .city_manager
            .as_ref()
            .is_some_and(|m| m.removal.is_none())
    {
        "Ctrl+? help"
    } else {
        "p period · ? help"
    };
    let status = if area.width < 45 {
        let mode = match app.prefs.mode {
            Mode::Normal => "normal",
            Mode::Comparison => "compare",
            Mode::Historical => "history",
        };
        let sort = match app.prefs.sort {
            Sort::Humidity => "RH",
            Sort::Pressure => "hPa",
            Sort::Weather => "Sky",
            Sort::LocalTime => "Time",
            Sort::TemperatureMin => "Min°",
            Sort::TemperatureMax => "Max°",
            Sort::FeelsMin => "Min feels",
            Sort::FeelsMax => "Max feels",
            other => other.label(),
        };
        format!(
            "{}{mode} {sort}{}",
            if app.demo { "DEMO " } else { "" },
            if app.prefs.reverse { "↓" } else { "↑" }
        )
    } else {
        format!(
            "{}{} · {} {}",
            if app.demo { "DEMO · " } else { "" },
            app.prefs.mode.label(),
            app.prefs.sort.label(),
            if app.prefs.reverse { "DESC" } else { "ASC" }
        )
    };
    let mut status_spans = Vec::with_capacity(3);
    status_spans.push(Span::raw(status));
    if app.prefs.mode == Mode::Comparison
        && let Some(base) = app.prefs.comparison_period
    {
        status_spans.push(Span::raw(format!(" · vs {}", base.label())));
    }
    if app.prefs.period != Period::Now {
        status_spans.insert(
            0,
            Span::raw(format!(
                "{} · {} · ",
                app.prefs.period.label(),
                crate::climate::MONTHS[app.prefs.month as usize]
            )),
        );
    }
    let quota = app.quota_status();
    let bar = Rect::new(area.x, area.bottom() - 1, area.width, 1);
    tapp_ui::chrome::StatusBar {
        left: tapp_ui::loading::status_line(
            Line::from(status_spans),
            (app.loading || app.seeking || LOADING_PREVIEW).then(|| app.loading_indicator.symbol()),
            !quota.is_empty(),
            tapp_ui::theme::Role::Search.style(app.prefs.monochrome),
        ),
        right: Line::styled(
            right,
            tapp_ui::theme::Role::Command.style(app.prefs.monochrome),
        ),
        style: Default::default(),
    }
    .draw(frame, bar);
}

enum Message {
    Partial(u64, Box<Weather>),
    Cities(u64, String, Result<Vec<cities::Candidate>>),
    Input(Event),
    InputError(io::Error),
    Data(u64, Box<Result<Report>>, Vec<String>),
    Shutdown,
    Hours(u64, String, Box<Weather>),
}
struct Request {
    priority: Option<String>,
    config: Config,
    ids: Vec<String>,
    generation: u64,
    prefs: Preferences,
    refresh: bool,
    hours: Option<(City, u32)>,
}
pub fn run(
    mut service: Service,
    prefs: Preferences,
    state: PathBuf,
    notice: String,
    config_path: PathBuf,
) -> Result<i32> {
    let initial = service.initial(&prefs)?;
    // The launch flag is a one-shot command, not a policy for future reloads.
    let initial_refresh = std::mem::take(&mut service.options.refresh);
    let auto_refresh = !service.options.demo && !service.client.offline;
    let mut startup_service = service.clone();
    let (tx, rx) = mpsc::sync_channel(128);
    let input_tx = tx.clone();
    let worker_tx = tx.clone();
    let search_tx = tx.clone();
    let search_client = service.client.clone();
    let config = service.config.clone();
    let demo = service.options.demo;
    let past_days = service.options.past_days;
    let cancellation = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(1));
    let worker_cancellation = cancellation.clone();
    let requests = tapp_ui::background::Latest::spawn(move |request: Request| {
        service.client.generation = Some((worker_cancellation.clone(), request.generation));
        if service.config.cities != request.config.cities {
            service.options.city_ids.clear();
        }
        service.config = request.config;
        service.options.city_ids = request.ids.clone();
        let mut prefs = request.prefs;
        if prefs.last_city.is_some() && prefs.period != Period::Now {
            prefs.month = 0;
        }
        if request.ids.is_empty() {
            let _ = worker_tx.send(Message::Data(
                request.generation,
                Box::new(service.initial(&prefs)),
                vec![],
            ));
            return;
        }
        if let Some((city, days)) = request.hours {
            let row = service.more_hours(&city, &prefs, days);
            let _ = worker_tx.send(Message::Hours(request.generation, city.id, Box::new(row)));
        } else {
            let result = service.load_progress_prioritized(
                &prefs,
                request.refresh,
                request.priority.as_deref().or(prefs.last_city.as_deref()),
                |row| {
                    let _ =
                        worker_tx.send(Message::Partial(request.generation, Box::new(row.clone())));
                },
            );
            let _ = worker_tx.send(Message::Data(
                request.generation,
                Box::new(result),
                service.options.city_ids.clone(),
            ));
        }
    });
    let searches = tapp_ui::background::Latest::spawn(move |(id, query): (u64, String)| {
        let result = if demo {
            Ok(cities::demo_search(&query))
        } else {
            search_client.search(&query).and_then(cities::candidates)
        };
        let _ = search_tx.send(Message::Cities(id, query, result));
    });
    let state_snapshot = tapp_ui::storage::read(&state);
    let state_writable = match &state_snapshot {
        Ok(None) => true,
        Ok(Some(bytes)) => serde_json::from_slice::<Preferences>(bytes).is_ok(),
        Err(_) => false,
    };
    #[cfg(unix)]
    {
        use std::io::Read;
        // An OS pipe also works in sandboxes that restrict socket send syscalls.
        let (mut reader, writer) = std::io::pipe()?;
        signal_hook::low_level::pipe::register(signal_hook::consts::SIGTERM, writer.try_clone()?)?;
        signal_hook::low_level::pipe::register(signal_hook::consts::SIGHUP, writer.try_clone()?)?;
        signal_hook::low_level::pipe::register(signal_hook::consts::SIGINT, writer)?;
        thread::spawn(move || {
            if reader.read_exact(&mut [0_u8; 1]).is_ok() {
                let _ = tx.send(Message::Shutdown);
            }
        });
    }
    #[cfg(windows)]
    let windows_shutdown = crate::windows_shutdown::Guard::install(move || {
        let _ = tx.send(Message::Shutdown);
    })?;
    let mut session = tapp_ui::terminal::Session::start()?;
    if !prefs.monochrome {
        let _ = tapp_ui::terminal::request_selection_background();
    }
    thread::spawn(move || {
        loop {
            match event::read() {
                Ok(Event::Key(key)) if key.kind == KeyEventKind::Release => {}
                Ok(Event::Mouse(mouse))
                    if matches!(mouse.kind, MouseEventKind::Moved | MouseEventKind::Drag(_)) => {}
                Ok(event) => {
                    if input_tx.send(Message::Input(event)).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = input_tx.send(Message::InputError(e));
                    break;
                }
            }
        }
    });
    let mut app = App::new(prefs, demo, notice);
    app.config = config;
    app.report = Some(initial);
    app.restore_screen();
    app.config_path = Some(config_path.clone());
    if !app.notice.is_empty() {
        app.ui.toast.push(tapp_ui::notification::Toast::plain(
            tapp_ui::theme::ToastKind::Warning,
            &app.notice,
        ));
    }
    if !demo {
        app.ui.theme_path = tapp_ui::preferences::theme_path();
        if let Some(path) = &app.ui.theme_path {
            match tapp_ui::preferences::load_theme(path) {
                Ok(theme) => app.ui.theme = theme,
                Err(e) => app.ui.toast.push(tapp_ui::notification::Toast::plain(
                    tapp_ui::theme::ToastKind::Warning,
                    &e.to_string(),
                )),
            }
        }
    }
    app.past_days = past_days;
    let mut search_generation = 0;
    let mut generation = 1;
    session.terminal.draw(|frame| draw(frame, &mut app))?;
    requests.submit(Request {
        priority: app.focus.clone(),
        config: app.config.clone(),
        ids: app.request_ids(),
        generation,
        prefs: app.session_preferences(),
        refresh: initial_refresh,
        hours: None,
    });
    // Terminal-window closure may arrive as SIGHUP, input EOF, or a draw error.
    // All paths leave through the same persistence/terminal-cleanup tail.
    let outcome = (|| -> Result<i32> {
        let mut exit = 0;
        let mut retry_at = std::time::Instant::now();
        let mut report_failed = false;
        let scope = |app: &App| {
            (
                app.request_ids(),
                app.prefs.period,
                if app.session_preferences().last_city.is_some() {
                    0
                } else {
                    app.prefs.month
                },
                app.session_preferences().last_city.is_some(),
                app.prefs.comparison_period,
            )
        };
        let mut last_scope = scope(&app);
        loop {
            let refresh_wait = (auto_refresh && !app.loading && !app.seeking)
                .then(|| {
                    app.report.as_ref().map(|report| {
                        let mut scoped = report.clone();
                        let ids = app.request_ids();
                        scoped.cities.retain(|r| ids.contains(&r.city.id));
                        let retry_after =
                            retry_at.saturating_duration_since(std::time::Instant::now());
                        let wait = crate::service::refresh_wait(
                            &scoped,
                            chrono::Utc::now().timestamp(),
                            retry_after,
                        );
                        if report_failed {
                            wait.max(retry_after)
                        } else {
                            wait
                        }
                    })
                })
                .flatten();
            if refresh_wait.is_some_and(|wait| wait.is_zero()) {
                generation += 1;
                cancellation.store(generation, std::sync::atomic::Ordering::Relaxed);
                app.loading = true;
                requests.submit(Request {
                    priority: app.focus.clone(),
                    config: app.config.clone(),
                    ids: app.request_ids(),
                    generation,
                    prefs: app.session_preferences(),
                    refresh: false,
                    hours: None,
                });
            }
            let viewport = session.terminal.draw(|frame| draw(frame, &mut app))?.area;
            let next_scope = scope(&app);
            if next_scope != last_scope {
                last_scope = next_scope;
                generation += 1;
                cancellation.store(generation, std::sync::atomic::Ordering::Relaxed);
                app.loading = true;
                requests.submit(Request {
                    priority: app.focus.clone(),
                    config: app.config.clone(),
                    ids: app.request_ids(),
                    generation,
                    prefs: app.session_preferences(),
                    refresh: false,
                    hours: None,
                });
            }
            // Visible clocks wake at minute boundaries; animate only pending work.
            let clock_visible = !app.demo && app.report.is_some() && viewport.height > 2;
            let clock_wait = clock_visible.then(|| {
                std::time::Duration::from_millis(
                    (60_000 - chrono::Utc::now().timestamp_millis().rem_euclid(60_000)) as u64,
                )
            });
            let wait = app
                .ui
                .toast
                .next_wakeup()
                .into_iter()
                .chain(clock_wait)
                .chain(refresh_wait.filter(|_| !app.loading))
                .chain(
                    (app.loading || app.seeking || LOADING_PREVIEW)
                        .then(|| app.loading_indicator.next_wakeup()),
                )
                .min();
            let message = if let Some(wait) = wait {
                match rx.recv_timeout(wait) {
                    Ok(message) => message,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        app.ui.toast.expire();
                        continue;
                    }
                    Err(e) => return Err(e.into()),
                }
            } else {
                rx.recv()?
            };
            match message {
                Message::Partial(tag, row) => {
                    if tag == generation {
                        app.receive_partial(*row);
                    }
                }
                Message::Cities(id, query, result) => app.receive_city_search(id, &query, result),
                Message::Hours(tag, id, result) => {
                    if tag == generation {
                        app.receive_hours(id, *result);
                    }
                }
                Message::Shutdown => {
                    exit = 130;
                    break;
                }
                Message::InputError(e) => return Err(e.into()),
                Message::Data(tag, result, selected) => {
                    if tag == generation {
                        retry_at = std::time::Instant::now()
                            + std::time::Duration::from_secs(crate::service::WEATHER_TTL as u64);
                        let result = (*result).map(|mut report| {
                            if report.mode != app.prefs.mode
                                || report.reference != app.prefs.reference
                            {
                                let _ = report.present(app.prefs.mode, &app.prefs.reference);
                            }
                            if !selected.is_empty() {
                                report.cities.retain(|r| selected.contains(&r.city.id));
                            }
                            report
                        });
                        report_failed = result.is_err();
                        app.receive(result);
                    }
                }
                Message::Input(event) => {
                    let action = app.handle(event);
                    if app
                        .city_manager
                        .as_ref()
                        .is_none_or(|m| m.request_id == 0 && !m.search.loading)
                    {
                        searches.cancel_pending();
                    }
                    let action = if matches!(action, Action::AddCity | Action::RemoveCity) {
                        match app.save_city_change(&config_path, action == Action::RemoveCity) {
                            Ok(()) => Action::Reload(false),
                            Err(e) => {
                                if let Some(m) = &mut app.city_manager {
                                    m.message = format!("{e:#}");
                                }
                                Action::None
                            }
                        }
                    } else {
                        action
                    };
                    match action {
                        Action::Reused => {
                            if !app.loading {
                                generation += 1;
                                cancellation
                                    .store(generation, std::sync::atomic::Ordering::Relaxed);
                                requests.cancel_pending();
                            }
                            if app.seeking {
                                app.sought_days.clear();
                            }
                            app.seeking = false;
                        }
                        Action::Copy => {
                            if let Some(text) = app.ui.copy.take() {
                                use base64::Engine;
                                use std::io::Write;
                                let encoded =
                                    base64::engine::general_purpose::STANDARD.encode(text);
                                let result = write!(io::stdout(), "\x1b]52;c;{encoded}\x07")
                                    .and_then(|_| io::stdout().flush());
                                let message=match result{Ok(())=>"Copy sent to terminal; clipboard support depends on the terminal".into(),Err(e)=>format!("Copy failed: {e}")};
                                app.ui.toast.push(tapp_ui::notification::Toast::plain(
                                    tapp_ui::theme::ToastKind::Note,
                                    &message,
                                ));
                            }
                        }
                        Action::Theme => {
                            let next = app.ui.theme.toggled();
                            let saved = if demo {
                                Ok(())
                            } else if let Some(path) = &app.ui.theme_path {
                                tapp_ui::preferences::save_theme(path, next)
                            } else {
                                Err(io::Error::other("No shared theme path available"))
                            };
                            match saved {
                                Ok(()) => {
                                    app.ui.theme = next;
                                    if app.ui.info_open {
                                        app.ui.info.set_entries(app.info_entries());
                                    }
                                    app.ui.toast.push(tapp_ui::notification::Toast::plain(
                                        tapp_ui::theme::ToastKind::Tip,
                                        next.label(),
                                    ));
                                }
                                Err(e) => app.ui.toast.push(tapp_ui::notification::Toast::plain(
                                    tapp_ui::theme::ToastKind::Caution,
                                    &e.to_string(),
                                )),
                            }
                        }
                        Action::SearchCities => {
                            if let Some(m) = &mut app.city_manager {
                                search_generation += 1;
                                m.request_id = search_generation;
                                let id = search_generation;
                                let query = m.query().to_owned();
                                searches.submit((id, query));
                            }
                        }
                        Action::AddCity | Action::RemoveCity => unreachable!(),
                        Action::Quit => break,
                        Action::OlderHours => {
                            if let Some(row) = app.report.as_ref().and_then(|r| {
                                r.cities
                                    .iter()
                                    .find(|r| Some(&r.city.id) == app.focus.as_ref())
                            }) {
                                let days = (app
                                    .sought_days
                                    .get(&row.city.id)
                                    .copied()
                                    .unwrap_or(app.past_days)
                                    .max(1)
                                    * 2)
                                .min(92);
                                app.sought_days.insert(row.city.id.clone(), days);
                                app.seeking = true;
                                requests.submit(Request {
                                    priority: app.focus.clone(),
                                    config: app.config.clone(),
                                    ids: app.request_ids(),
                                    generation,
                                    prefs: app.session_preferences(),
                                    refresh: false,
                                    hours: Some((row.city.clone(), days)),
                                });
                            }
                        }
                        Action::Reload(refresh) => {
                            generation += 1;
                            cancellation.store(generation, std::sync::atomic::Ordering::Relaxed);
                            app.loading = true;
                            if app.report.as_ref().is_none_or(|r| {
                                r.period != app.prefs.period
                                    || r.mode != app.prefs.mode
                                    || r.source != app.prefs.source
                            }) || startup_service.config.cities != app.config.cities
                            {
                                startup_service.config = app.config.clone();
                                startup_service.options.city_ids.clear();
                                app.report = Some(startup_service.initial(&app.prefs)?);
                            }
                            app.notice.clear();
                            app.seeking = false;
                            app.sought_days.clear();
                            requests.submit(Request {
                                priority: app.focus.clone(),
                                config: app.config.clone(),
                                ids: app.request_ids(),
                                generation,
                                prefs: app.session_preferences(),
                                refresh,
                                hours: None,
                            });
                            last_scope = scope(&app);
                        }
                        Action::None => {}
                    }
                }
            }
        }
        Ok(exit)
    })();
    let saved_prefs = app.session_preferences();
    let save = if state_writable {
        tapp_ui::storage::replace_if_unchanged(
            &state,
            state_snapshot.as_ref().ok().and_then(|b| b.as_deref()),
            &serde_json::to_vec_pretty(&saved_prefs)?,
        )
    } else {
        Ok(())
    };
    drop(session);
    #[cfg(windows)]
    drop(windows_shutdown);
    save?;
    outcome
}

#[cfg(test)]
mod scope_tests {
    use super::*;
    #[test]
    fn details_and_help_request_only_city_and_optional_reference() {
        let mut app = App::new(
            Preferences {
                reference: "ref".into(),
                ..Default::default()
            },
            true,
            String::new(),
        );
        app.visible_ids = vec!["a".into(), "b".into(), "c".into()];
        assert_eq!(app.request_ids(), vec!["a", "b", "c", "ref"]);
        app.modal = Some(Modal::Detail);
        app.focus = Some("b".into());
        assert_eq!(app.request_ids(), vec!["b"]);
        app.prefs.mode = Mode::Comparison;
        assert_eq!(app.request_ids(), vec!["b", "ref"]);
        app.prefs.comparison_period = Some(Period::Baseline);
        assert_eq!(app.request_ids(), vec!["b"]);
        app.prefs.comparison_period = None;
        app.modal = Some(Modal::Help);
        app.help_return = Some(Modal::Detail);
        assert_eq!(app.request_ids(), vec!["b", "ref"]);
        app.prefs.mode = Mode::Normal;
        assert_eq!(app.request_ids(), vec!["b"]);
    }
}
