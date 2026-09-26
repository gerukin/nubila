//! Responsive detail dashboard with independent bounded forecast windows.
use crate::{
    model::{Values, Weather},
    theme::Theme,
    view,
};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{
        Axis, Block, BorderType, Borders, Cell, Dataset, GraphType, LineGauge, Paragraph, Row,
    },
};

pub use tapp_ui::chart::{GraphSegment, render_graph};
pub fn graph_segments(metric: Metric, values: &[Option<f64>], daily: bool) -> Vec<GraphSegment> {
    tapp_ui::chart::segments(
        if matches!(metric, Metric::Rain | Metric::Snow) {
            tapp_ui::chart::PlotKind::Bars
        } else if metric == Metric::Cloud {
            tapp_ui::chart::PlotKind::Area
        } else {
            tapp_ui::chart::PlotKind::Line
        },
        values,
        Theme::thresholds(metric.key(), daily),
    )
}
pub const WINDOW: usize = 8;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Metric {
    #[default]
    Temperature,
    Rain,
    Wind,
    Cloud,
    Snow,
    TemperatureMin,
    TemperatureMax,
    Daylight,
    Feels,
    FeelsMin,
    FeelsMax,
}
impl Metric {
    pub const ALL: [Self; 10] = [
        Self::Temperature,
        Self::TemperatureMin,
        Self::TemperatureMax,
        Self::FeelsMin,
        Self::FeelsMax,
        Self::Rain,
        Self::Snow,
        Self::Wind,
        Self::Cloud,
        Self::Daylight,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::Feels | Self::FeelsMin | Self::FeelsMax => "apparent_temperature",
            Self::Temperature | Self::TemperatureMin | Self::TemperatureMax => "temperature_2m",
            Self::Daylight => "daylight",
            Self::Rain => "rain",
            Self::Snow => "snowfall",
            Self::Wind => "wind_speed_10m",
            Self::Cloud => "cloud_cover",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Feels => "Feels like",
            Self::FeelsMin => "Feels min",
            Self::FeelsMax => "Feels max",
            Self::Temperature => "Temperature mean",
            Self::TemperatureMin => "Temperature min",
            Self::TemperatureMax => "Temperature max",
            Self::Daylight => "Daylight",
            Self::Rain => "Rain",
            Self::Snow => "Snow",
            Self::Wind => "Wind",
            Self::Cloud => "Cloud",
        }
    }
    pub fn unit(self, historical: bool) -> &'static str {
        match self {
            Self::Feels | Self::FeelsMin | Self::FeelsMax => "°C",
            Self::Temperature | Self::TemperatureMin | Self::TemperatureMax => "°C",
            Self::Daylight => "h/day",
            Self::Rain if historical => "mm/month",
            Self::Rain => "mm",
            Self::Snow if historical => "cm/month",
            Self::Snow => "cm",
            Self::Wind => "km/h",
            Self::Cloud => "%",
        }
    }
    pub fn graph_type(self) -> GraphType {
        match self {
            Self::Rain | Self::Snow | Self::Cloud => GraphType::Area,
            _ => GraphType::Line,
        }
    }
    fn temperature(self) -> bool {
        matches!(
            self,
            Self::Temperature
                | Self::TemperatureMin
                | Self::TemperatureMax
                | Self::Feels
                | Self::FeelsMin
                | Self::FeelsMax
        )
    }
    pub fn bounds(self, values: &[Option<f64>]) -> [f64; 2] {
        if self == Self::Cloud && values.iter().flatten().all(|v| *v >= 0.) {
            return [0.0, 100.0];
        }
        let high = values
            .iter()
            .flatten()
            .copied()
            .filter(|v| v.is_finite())
            .reduce(f64::max)
            .unwrap_or(0.0);
        if let Some(low) = values.iter().flatten().copied().reduce(f64::min)
            && low < 0.
            && !self.temperature()
        {
            return [low * 1.1, (high * 1.1).max(0.)];
        }
        if !self.temperature() {
            return [0.0, (high * 1.1).max(1.0)];
        }
        let low = values
            .iter()
            .flatten()
            .copied()
            .filter(|v| v.is_finite())
            .reduce(f64::min)
            .unwrap_or(0.0);
        [low - 0.5, high + 0.5]
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Section {
    #[default]
    Hours,
    Days,
    Graph,
    Months,
    Info,
}
#[derive(Debug, Default)]
pub struct State {
    pub status: String,
    pub loading: bool,
    pub waiting: bool,
    pub loading_symbol: String,
    pub pending_history: bool,
    pub section: Section,
    pub metric: Metric,
    pub graph_requested: bool,
    pub compact: bool,
    pub graph_area: Rect,
    pub tables_tab: Rect,
    pub previous_tab: Rect,
    pub next_tab: Rect,
    pub(crate) table_focus: Section,
    info_return: Section,
    pub graph_tabs: Vec<(Rect, Metric)>,
    pub tabs: tapp_ui::panel::TabStrip,
    pub chart: tapp_ui::chart::CachedChart,
    pub hour_offset: usize,
    pub day_offset: usize,
    pub month_offset: usize,
    pub hour_column: usize,
    pub day_column: usize,
    pub month_column: usize,
    pub hour_geometry: crate::viewport::Geometry,
    pub day_geometry: crate::viewport::Geometry,
    pub month_geometry: crate::viewport::Geometry,
    pub hour_column_max: usize,
    pub day_column_max: usize,
    pub month_column_max: usize,
    pub info_offset: usize,
    info_key: Option<u64>,
    info_cache: ratatui::text::Text<'static>,
    pub hour_count: usize,
    pub day_count: usize,
    pub month_count: usize,
    pub hour_area: Rect,
    pub day_area: Rect,
    pub month_area: Rect,
    info_max: usize,
    initialized: bool,
}
impl State {
    pub fn metric_shortcut(&self, metric: Metric) -> Option<char> {
        self.metrics()
            .iter()
            .position(|m| *m == metric)
            .map(|i| char::from(b'0' + ((i + 1) % 10) as u8))
    }
    pub fn graph_daily(&self) -> bool {
        matches!(self.section, Section::Days | Section::Months)
            || matches!(self.section, Section::Graph | Section::Info)
                && matches!(self.table_focus, Section::Days | Section::Months)
    }
    pub fn metrics(&self) -> &'static [Metric] {
        if self.graph_daily() {
            &Metric::ALL
        } else {
            &[
                Metric::Temperature,
                Metric::Feels,
                Metric::Rain,
                Metric::Snow,
                Metric::Wind,
                Metric::Cloud,
            ]
        }
    }
    pub fn invalidate_data(&mut self) {
        self.info_key = None;
        self.chart.invalidate();
    }
    fn normalize(&mut self, row: &Weather) {
        if !self.initialized && (!row.hourly.is_empty() || row.baseline.is_some()) {
            self.hour_offset = Self::current_hour(row);
            self.initialized = true;
        }
        if row.baseline.is_some() && matches!(self.section, Section::Hours | Section::Days) {
            self.section = Section::Months;
        }
        if row.baseline.is_none() && self.section == Section::Months {
            self.section = Section::Hours;
        }
        if matches!(
            self.section,
            Section::Hours | Section::Days | Section::Months
        ) {
            self.table_focus = self.section;
        }
        if row.baseline.is_some() {
            self.table_focus = Section::Months;
        } else if self.table_focus == Section::Months {
            self.table_focus = Section::Days;
        }
        if !self.metrics().contains(&self.metric) {
            self.metric = match self.metric {
                Metric::Feels => Metric::FeelsMin,
                Metric::FeelsMin | Metric::FeelsMax => Metric::Feels,
                _ => Metric::Temperature,
            };
        }
        self.hour_offset = self
            .hour_offset
            .min(row.hourly.len().saturating_sub(self.hour_count.max(1)));
        self.day_offset = self
            .day_offset
            .min(row.daily.len().saturating_sub(self.day_count.max(1)));
        self.month_offset = self
            .month_offset
            .min(row.monthly.len().saturating_sub(self.month_count.max(1)));
    }
    pub fn current_hour(row: &Weather) -> usize {
        let now = if row.sources.iter().any(|s| s == "demo") {
            row.time.clone()
        } else {
            chrono::Utc::now().format("%Y-%m-%dT%H:%M").to_string()
        };
        Self::current_hour_at(row, &now)
    }
    pub fn current_hour_at(row: &Weather, now_utc: &str) -> usize {
        row.hourly
            .iter()
            .rposition(|h| h.time.as_str() <= now_utc)
            .unwrap_or(0)
    }
    fn move_window(&mut self, delta: isize, row: &Weather) {
        let (offset, len, count) = match self.section {
            Section::Graph if row.baseline.is_some() => return,
            Section::Graph if self.graph_daily() => {
                (&mut self.day_offset, row.daily.len(), self.day_count)
            }
            Section::Hours | Section::Graph => {
                (&mut self.hour_offset, row.hourly.len(), self.hour_count)
            }
            Section::Days => (&mut self.day_offset, row.daily.len(), self.day_count),
            Section::Months => (&mut self.month_offset, row.monthly.len(), self.month_count),
            Section::Info => {
                self.info_offset = self
                    .info_offset
                    .saturating_add_signed(delta)
                    .min(self.info_max);
                return;
            }
        };
        *offset = offset
            .saturating_add_signed(delta)
            .min(len.saturating_sub(count.max(1)));
    }
    /// True means the event belongs to the detail dashboard, never the city table.
    pub fn handle(&mut self, event: &Event, row: &Weather) -> bool {
        self.normalize(row);
        let metrics = self.metrics();
        if matches!(
            self.section,
            Section::Hours | Section::Days | Section::Months
        ) {
            self.table_focus = self.section;
        }
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                let page = match self.section {
                    Section::Graph if self.graph_daily() => self.day_count,
                    Section::Hours | Section::Graph => self.hour_count,
                    Section::Days => self.day_count,
                    Section::Months => self.month_count,
                    Section::Info => WINDOW,
                }
                .max(1) as isize;
                match key.code {
                    _ if crate::viewport::Bindings::default()
                        .direction(event)
                        .is_some() =>
                    {
                        self.scroll_columns(
                            crate::viewport::Bindings::default()
                                .direction(event)
                                .unwrap(),
                        );
                    }
                    KeyCode::Char(c @ '0'..='9') => {
                        let Some(&metric) = metrics.get(if c == '0' {
                            9
                        } else {
                            (c as u8 - b'1') as usize
                        }) else {
                            return true;
                        };
                        if !metrics.contains(&metric) {
                            return true;
                        }
                        self.metric = metric;
                        self.graph_requested = true;
                        if self.compact {
                            self.section = Section::Graph;
                        }
                    }
                    KeyCode::Left | KeyCode::Right => {
                        if self.section != Section::Info {
                            let metric =
                                metrics.iter().position(|m| *m == self.metric).unwrap_or(0);
                            if self.compact {
                                let index = if self.section == Section::Graph {
                                    metric + 1
                                } else {
                                    0
                                };
                                let next = (index
                                    + if key.code == KeyCode::Left {
                                        metrics.len()
                                    } else {
                                        1
                                    })
                                    % (metrics.len() + 1);
                                if next == 0 {
                                    self.section = if row.baseline.is_some() {
                                        Section::Months
                                    } else {
                                        self.table_focus
                                    };
                                    self.graph_requested = false;
                                } else {
                                    self.metric = metrics[next - 1];
                                    self.section = Section::Graph;
                                    self.graph_requested = true;
                                }
                            } else {
                                self.metric = metrics[(metric
                                    + if key.code == KeyCode::Left {
                                        metrics.len() - 1
                                    } else {
                                        1
                                    })
                                    % metrics.len()];
                                self.graph_requested = true;
                            }
                        }
                    }
                    KeyCode::Tab | KeyCode::BackTab => {
                        self.section = match self.section {
                            Section::Hours => Section::Days,
                            Section::Days => Section::Hours,
                            other => other,
                        };
                    }
                    KeyCode::Char('i') => {
                        self.section = if self.section == Section::Info {
                            self.info_return
                        } else {
                            self.info_return = self.section;
                            Section::Info
                        }
                    }
                    KeyCode::Esc if self.section == Section::Info => {
                        self.section = self.info_return;
                    }
                    KeyCode::Down | KeyCode::Char('j') => self.move_window(1, row),
                    KeyCode::Up | KeyCode::Char('k') => self.move_window(-1, row),
                    KeyCode::Char('t')
                        if matches!(self.section, Section::Hours | Section::Graph) =>
                    {
                        self.hour_offset = Self::current_hour(row);
                        self.day_offset = 0;
                    }
                    KeyCode::PageDown => self.move_window(page, row),
                    KeyCode::PageUp => self.move_window(-page, row),
                    KeyCode::Home => self.move_window(isize::MIN / 2, row),
                    KeyCode::End => self.move_window(isize::MAX / 2, row),
                    _ => return false,
                }
                true
            }
            Event::Mouse(mouse) => {
                let pos = (mouse.column, mouse.row).into();
                if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                    && self.section != Section::Info
                {
                    let code = if self.previous_tab.contains(pos) {
                        Some(KeyCode::Left)
                    } else if self.next_tab.contains(pos) {
                        Some(KeyCode::Right)
                    } else {
                        None
                    };
                    if let Some(code) = code {
                        return self.handle(
                            &Event::Key(crossterm::event::KeyEvent::new(code, KeyModifiers::NONE)),
                            row,
                        );
                    }
                }
                if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                    && self.section != Section::Info
                    && self.compact
                    && self.tables_tab.contains(pos)
                {
                    self.section = if row.baseline.is_some() {
                        Section::Months
                    } else {
                        self.table_focus
                    };
                    self.graph_requested = false;
                    return true;
                }
                if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                    && self.section != Section::Info
                    && let Some((_, metric)) =
                        self.graph_tabs.iter().find(|(area, _)| area.contains(pos))
                {
                    self.metric = *metric;
                    self.graph_requested = true;
                    if self.compact {
                        self.section = Section::Graph;
                    }
                    return true;
                }
                if self.section != Section::Info {
                    if self.compact && self.graph_area.contains(pos) {
                        self.section = Section::Graph;
                    } else if self.hour_area.contains(pos) {
                        self.section = Section::Hours;
                    } else if self.day_area.contains(pos) {
                        self.section = Section::Days;
                    } else if self.month_area.contains(pos) {
                        self.section = Section::Months;
                    } else {
                        return true;
                    }
                }
                if let Some(direction) = crate::viewport::Bindings::default().direction(event) {
                    self.scroll_columns(direction);
                    return true;
                }
                match mouse.kind {
                    MouseEventKind::ScrollDown => self.move_window(1, row),
                    MouseEventKind::ScrollUp => self.move_window(-1, row),
                    MouseEventKind::Down(MouseButton::Left) => {}
                    _ => {}
                }
                true
            }
            _ => false,
        }
    }
    fn scroll_columns(&mut self, delta: isize) {
        let (offset, geometry) = match self.section {
            Section::Hours => (&mut self.hour_column, &self.hour_geometry),
            Section::Days => (&mut self.day_column, &self.day_geometry),
            Section::Months => (&mut self.month_column, &self.month_geometry),
            _ => return,
        };
        *offset = geometry.advance(*offset, delta);
    }
}
fn value(values: &Values, key: &str) -> Option<f64> {
    values.get(key).copied().flatten()
}
fn block(title: String, active: bool, theme: &Theme) -> Block<'static> {
    tapp_ui::panel::block(title, active, theme.monochrome())
}
fn window_title(label: &str, offset: usize, count: usize, total: usize) -> String {
    if total == 0 {
        format!("{label} · no data")
    } else {
        format!(
            "{label} · {}–{} / {total}",
            offset + 1,
            (offset + count).min(total)
        )
    }
}
fn shown_count(area: Rect) -> usize {
    usize::from(area.height.saturating_sub(3)).min(WINDOW)
}
fn hero_height(row: &Weather, width: u16) -> u16 {
    let delta = row.absolute_values.is_some();
    let n = |key| view::number(value(&row.values, key), delta);
    let first = format!(
        "{} °C  Feels {}°  ·  Wind {} km/h",
        n("temperature_2m"),
        n("apparent_temperature"),
        n("wind_speed_10m")
    );
    let range = row.ranges.get("temperature_2m");
    let second = format!(
        "{}/{}°  ·  Rain {} {} · Snow {} {}",
        view::number(range.and_then(|r| r.min), false),
        view::number(range.and_then(|r| r.max), false),
        n("rain"),
        if row.baseline.is_some() {
            "mm/year"
        } else {
            "mm"
        },
        n("snowfall"),
        if row.baseline.is_some() {
            "cm/year"
        } else {
            "cm"
        }
    );
    use unicode_width::UnicodeWidthStr;
    if first.width() + 5 + second.width() <= usize::from(width.saturating_sub(2)) {
        6
    } else {
        7
    }
}
fn hero(frame: &mut Frame, area: Rect, row: &Weather, theme: &Theme) {
    let historical = row.baseline.is_some();
    let values = &row.values;
    let mut title = vec![Span::raw(format!(" {} · ", view::clean(&row.city.name)))];
    if historical {
        title.push(Span::styled(row.time.clone(), theme.muted));
    } else {
        title.extend(theme.condition_line(row.condition.as_ref()).spans);
        title.push(Span::styled(
            format!(" {}", view::weather_label(row.condition.as_ref())),
            theme.condition(row.condition.as_ref()),
        ));
    }
    title.push(Span::raw(" "));
    if Line::from(title.clone()).width() + 9 > usize::from(area.width) {
        title = vec![Span::raw(format!(
            " {} ",
            view::fit(
                &view::clean(&row.city.name),
                usize::from(area.width).saturating_sub(12)
            )
            .trim_end()
        ))];
    }
    let available = usize::from(area.width).saturating_sub(Line::from(title.clone()).width() + 4);
    let mut clock = view::clock_text(row, true);
    if unicode_width::UnicodeWidthStr::width(clock.as_str()) > available {
        clock = view::clock_text(row, false);
    }
    let mut panel = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.border)
        .title(
            Line::from(title).style(
                ratatui::style::Style::default()
                    .fg(ratatui::style::Color::Reset)
                    .remove_modifier(ratatui::style::Modifier::DIM),
            ),
        );
    if unicode_width::UnicodeWidthStr::width(clock.as_str()) <= available {
        panel = panel.title(Line::styled(format!(" {clock} "), theme.muted).right_aligned());
    }
    let inner = panel.inner(area);
    frame.render_widget(panel, area);
    let delta = row.absolute_values.is_some();
    let temp = view::number(value(values, "temperature_2m"), delta);
    let feels = view::number(value(values, "apparent_temperature"), delta);
    let wind = view::number(value(values, "wind_speed_10m"), delta);
    let rain = view::number(value(values, "rain"), delta);
    let mut lines = vec![Line::from(vec![
        theme.metric_span(
            temp,
            "temperature_2m",
            value(values, "temperature_2m"),
            false,
        ),
        Span::styled(" °C  Feels ", theme.muted),
        theme.metric_span(
            feels,
            "apparent_temperature",
            value(values, "apparent_temperature"),
            false,
        ),
        Span::styled("°  ·  Wind ", theme.muted),
        theme.metric_span(
            wind,
            "wind_speed_10m",
            value(values, "wind_speed_10m"),
            false,
        ),
        Span::styled(" km/h", theme.muted),
    ])];
    let temp_range = row.ranges.get("temperature_2m");
    let range = temp_range
        .map(|r| {
            format!(
                "{}/{}",
                view::number(r.min, false),
                view::number(r.max, false)
            )
        })
        .unwrap_or_else(|| "—".into());
    let mut range_line = theme.secondary_range(
        &range,
        "temperature_2m",
        temp_range.and_then(|r| r.min),
        temp_range.and_then(|r| r.max),
        false,
    );
    range_line
        .spans
        .push(Span::styled("°  ·  Rain ", theme.muted));
    range_line
        .spans
        .push(theme.metric_span(rain, "rain", value(values, "rain"), historical));
    range_line.spans.push(Span::styled(
        if row
            .comparison_base
            .as_ref()
            .is_some_and(|b| crate::comparison::mixed(row, b))
        {
            " mm/day"
        } else if historical {
            " mm/year"
        } else {
            " mm"
        },
        theme.muted,
    ));
    range_line.spans.push(Span::styled(" · Snow ", theme.muted));
    range_line.spans.push(theme.metric_span(
        view::number(value(values, "snowfall"), delta),
        "snowfall",
        value(values, "snowfall"),
        historical,
    ));
    range_line.spans.push(Span::styled(
        if row
            .comparison_base
            .as_ref()
            .is_some_and(|b| crate::comparison::mixed(row, b))
        {
            " cm/day"
        } else if historical {
            " cm/year"
        } else {
            " cm"
        },
        theme.muted,
    ));
    if lines[0].width() + 5 + range_line.width() <= usize::from(inner.width) {
        lines[0].spans.push(Span::styled("  ·  ", theme.muted));
        lines[0].spans.extend(range_line.spans);
    } else {
        lines.push(range_line);
    }
    if row.absolute_values.is_some() {
        lines.push(
            Line::from(format!(
                "Δ vs {}",
                row.comparison_label.as_deref().unwrap_or("pinned city")
            ))
            .style(theme.muted),
        );
    } else {
        lines.push(
            Line::from(format!(
                "{} · Source: {}",
                row.range_date,
                row.sources.join(" + ")
            ))
            .style(theme.muted),
        );
    }
    lines.push(solar_line(&row.values, delta, theme));
    frame.render_widget(
        Paragraph::new(lines),
        Rect::new(
            inner.x,
            inner.y,
            inner.width,
            inner.height.saturating_sub(1),
        ),
    );
    atmosphere(
        frame,
        Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
        row,
        theme,
    );
}
fn solar_line(values: &Values, delta: bool, theme: &Theme) -> Line<'static> {
    let mut spans = vec![];
    for (key, label) in [
        ("sunrise", "↑ "),
        ("sunset", "  ↓ "),
        ("daylight", "  Day "),
    ] {
        spans.push(Span::styled(label, theme.muted));
        spans.push(Span::raw(crate::solar::text(
            key,
            value(values, key),
            delta,
        )));
    }
    Line::from(spans)
}
fn atmosphere(frame: &mut Frame, area: Rect, row: &Weather, theme: &Theme) {
    let values = &row.values;
    if area.width >= 68 {
        let parts = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Fill(1),
            Constraint::Length(12),
        ])
        .spacing(2)
        .split(area);
        for (rect, key, label) in [
            (parts[0], "cloud_cover", "Cloud"),
            (parts[1], "relative_humidity_2m", "Humidity"),
        ] {
            let val = value(values, key);
            if let Some(v) = val {
                frame.render_widget(
                    LineGauge::default()
                        .ratio((v / 100.0).clamp(0.0, 1.0))
                        .filled_style(theme.accent)
                        .unfilled_style(theme.muted)
                        .label(format!("{label} {v:.0}% ")),
                    rect,
                );
            } else {
                frame.render_widget(Paragraph::new(format!("{label} —")), rect);
            }
        }
        frame.render_widget(
            Paragraph::new(format!(
                "  {} hPa",
                view::number(value(values, "surface_pressure"), false)
            ))
            .alignment(Alignment::Right),
            parts[2],
        );
    } else {
        frame.render_widget(
            Paragraph::new(format!(
                "Cloud {}% · RH {}% · {} hPa",
                view::number(value(values, "cloud_cover"), false),
                view::number(value(values, "relative_humidity_2m"), false),
                view::number(value(values, "surface_pressure"), false)
            )),
            area,
        );
    }
}
fn scrolling_table(
    headers: Vec<&'static str>,
    widths: Vec<Constraint>,
    rows: Vec<Vec<Cell<'static>>>,
    theme: &Theme,
) -> ratatui::widgets::Table<'static> {
    ratatui::widgets::Table::new(rows.into_iter().map(Row::new), widths)
        .column_spacing(1)
        .header(Row::new(headers).style(theme.accent))
}

fn forecast_table(
    frame: &mut Frame,
    area: Rect,
    row: &Weather,
    state: &mut State,
    hours: bool,
    theme: &Theme,
) {
    let hour_label = format!("Hours · {}", row.range_timezone);
    let (offset, count, total, label, active) = if hours {
        (
            state.hour_offset,
            state.hour_count,
            row.hourly.len(),
            hour_label.as_str(),
            state.section == Section::Hours,
        )
    } else {
        (
            state.day_offset,
            state.day_count,
            row.daily.len(),
            "Next days · local",
            state.section == Section::Days,
        )
    };
    let panel = block(window_title(label, offset, count, total), active, theme);
    let pending = state.loading && row.time.is_empty();
    let panel = if pending {
        block(label.to_owned(), active, theme)
    } else {
        panel
    };
    if total == 0 && !pending {
        let inner = panel.inner(area);
        frame.render_widget(panel, area);
        frame.render_widget(
            Paragraph::new("No forecast available").style(theme.muted),
            inner,
        );
        return;
    }
    let mut headers = vec![
        if hours { "Time" } else { "Day" },
        "",
        if hours { "Temp °C" } else { "Low/high °C" },
        "Feels °C",
    ];
    let mut widths = vec![
        Constraint::Length(if hours { 11 } else { 10 }),
        Constraint::Length(view::ICON_WIDTH),
        Constraint::Length(if hours { 8 } else { 12 }),
        Constraint::Length(if hours { 8 } else { 12 }),
    ];
    {
        headers.extend([
            "Rain mm",
            "Snow cm",
            if hours { "Wind km/h" } else { "Max km/h" },
            "RH %",
            "Cloud %",
        ]);
        widths.extend([
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Length(6),
            Constraint::Length(7),
        ]);
    }
    let solar = !hours;
    if solar {
        headers.extend(["Sunrise", "Sunset", "Daylight"]);
        widths.extend([
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(9),
        ]);
    }
    let rows: Vec<Vec<Cell>> = if pending {
        (0..count.min(WINDOW))
            .map(|_| vec![Cell::from("—").style(theme.muted); headers.len()])
            .collect()
    } else if hours {
        row.hourly
            .iter()
            .skip(offset)
            .take(count)
            .map(|h| {
                let local = view::local_time(&h.time, &row.range_timezone);
                let time = local.get(5..).unwrap_or(&local).replace('T', " ");
                let mut cells = vec![
                    Cell::from(time),
                    Cell::from(theme.condition_line(h.condition.as_ref())),
                    Cell::from(theme.metric_span(
                        view::number(value(&h.values, "temperature_2m"), false),
                        "temperature_2m",
                        value(&h.values, "temperature_2m"),
                        false,
                    )),
                    Cell::from(theme.metric_span(
                        view::number(value(&h.values, "apparent_temperature"), false),
                        "apparent_temperature",
                        value(&h.values, "apparent_temperature"),
                        false,
                    )),
                ];
                {
                    cells.extend(
                        [
                            "rain",
                            "snowfall",
                            "wind_speed_10m",
                            "relative_humidity_2m",
                            "cloud_cover",
                        ]
                        .map(|key| {
                            Cell::from(theme.metric_span(
                                view::number(value(&h.values, key), false),
                                key,
                                value(&h.values, key),
                                false,
                            ))
                        }),
                    );
                }
                cells
            })
            .collect()
    } else {
        row.daily
            .iter()
            .skip(offset)
            .take(count)
            .map(|d| {
                let date = chrono::NaiveDate::parse_from_str(&d.date, "%Y-%m-%d")
                    .map(|v| v.format("%a %m-%d").to_string())
                    .unwrap_or(d.date.clone());
                let mut cells = vec![
                    Cell::from(date),
                    Cell::from(theme.condition_line(d.condition.as_ref())),
                    Cell::from(theme.range(
                        &format!(
                            "{}/{}",
                            view::number(d.temperature_min, false),
                            view::number(d.temperature_max, false)
                        ),
                        "temperature_2m",
                        d.temperature_min,
                        d.temperature_max,
                        false,
                    )),
                    Cell::from(theme.range(
                        &format!(
                            "{}/{}",
                            view::number(d.feels_min, false),
                            view::number(d.feels_max, false)
                        ),
                        "apparent_temperature",
                        d.feels_min,
                        d.feels_max,
                        false,
                    )),
                ];
                {
                    cells.extend(
                        [
                            ("rain", d.rain_sum),
                            ("snowfall", d.snowfall_sum),
                            ("wind_speed_10m", d.wind_speed_max),
                            ("relative_humidity_2m", d.humidity),
                            ("cloud_cover", d.cloud_cover),
                        ]
                        .map(|(k, v)| {
                            Cell::from(theme.metric_span(view::number(v, false), k, v, true))
                        }),
                    );
                }
                if solar {
                    cells.extend(crate::solar::KEYS.map(|key| {
                        Cell::from(crate::solar::text(
                            key,
                            value(&d.solar, key),
                            row.absolute_values.is_some(),
                        ))
                    }));
                }
                cells
            })
            .collect()
    };
    let inner = panel.inner(area);
    frame.render_widget(panel, area);
    let geometry = crate::viewport::Geometry::new(&widths, inner.width, 0);
    let max = geometry.max;
    let offset = if hours {
        state.hour_geometry = geometry;
        state.hour_column_max = max;
        &mut state.hour_column
    } else {
        state.day_geometry = geometry;
        state.day_column_max = max;
        &mut state.day_column
    };
    *offset = (*offset).min(max);
    crate::viewport::draw(
        frame,
        inner,
        scrolling_table(headers, widths.clone(), rows, theme),
        &widths,
        *offset,
    );
}
fn monthly_table(frame: &mut Frame, area: Rect, row: &Weather, state: &mut State, theme: &Theme) {
    let mut headers = vec!["Month", "Mean °C", "Low/high °C"];
    let mut widths = vec![
        Constraint::Length(6),
        Constraint::Length(9),
        Constraint::Length(13),
    ];
    {
        headers.extend([
            "Rain mm",
            "Snow cm",
            "Wind km/h",
            if row.absolute_values.is_some() {
                "RH pp"
            } else {
                "RH %"
            },
            if row.absolute_values.is_some() {
                "Cloud pp"
            } else {
                "Cloud %"
            },
        ]);
        widths.extend([
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Length(6),
            Constraint::Length(8),
        ]);
    }
    let solar = true;
    if solar {
        headers.extend(["Sunrise", "Sunset", "Daylight"]);
        widths.extend([
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(9),
        ]);
    }
    let rows = row
        .monthly
        .iter()
        .skip(state.month_offset)
        .take(state.month_count)
        .map(|m| {
            let label = [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ]
            .get(m.month.saturating_sub(1) as usize)
            .unwrap_or(&"?");
            let range = m.ranges.get("temperature_2m");
            let mut cells = vec![
                Cell::from(*label),
                Cell::from(theme.metric_span(
                    view::number(value(&m.values, "temperature_2m"), false),
                    "temperature_2m",
                    value(&m.values, "temperature_2m"),
                    false,
                )),
                Cell::from(theme.secondary_range(
                    &format!(
                        "{}/{}",
                        view::number(range.and_then(|r| r.min), false),
                        view::number(range.and_then(|r| r.max), false)
                    ),
                    "temperature_2m",
                    range.and_then(|r| r.min),
                    range.and_then(|r| r.max),
                    false,
                )),
            ];
            {
                cells.extend(
                    [
                        "rain",
                        "snowfall",
                        "wind_speed_10m",
                        "relative_humidity_2m",
                        "cloud_cover",
                    ]
                    .map(|k| {
                        Cell::from(theme.metric_span(
                            view::number(value(&m.values, k), false),
                            k,
                            value(&m.values, k),
                            true,
                        ))
                    }),
                );
            }
            if solar {
                cells.extend(crate::solar::KEYS.map(|key| {
                    Cell::from(crate::solar::text(
                        key,
                        value(&m.values, key),
                        row.absolute_values.is_some(),
                    ))
                }));
            }
            cells
        })
        .collect();
    let panel = block(
        window_title(
            "Months",
            state.month_offset,
            state.month_count,
            row.monthly.len(),
        ),
        true,
        theme,
    );
    let inner = panel.inner(area);
    frame.render_widget(panel, area);
    state.month_geometry = crate::viewport::Geometry::new(&widths, inner.width, 0);
    state.month_column_max = state.month_geometry.max;
    state.month_column = state.month_column.min(state.month_column_max);
    crate::viewport::draw(
        frame,
        inner,
        scrolling_table(headers, widths.clone(), rows, theme),
        &widths,
        state.month_column,
    );
}
pub fn info_lines(row: &Weather, width: usize) -> Vec<String> {
    view::wrap(&view::overview(row), width)
}
pub fn draw(frame: &mut Frame, area: Rect, row: &Weather, state: &mut State, theme: &Theme) {
    state.graph_tabs.clear();
    state.graph_area = Rect::default();
    state.tables_tab = Rect::default();
    let was_compact = state.compact;
    state.compact = area.height.saturating_sub(1) < 33;
    if !was_compact && state.compact && state.graph_requested && state.section != Section::Info {
        state.section = Section::Graph;
    }
    if !state.compact && state.section == Section::Graph {
        state.section = if row.baseline.is_some() {
            Section::Months
        } else {
            state.table_focus
        };
    }
    state.hour_area = Rect::default();
    state.day_area = Rect::default();
    state.month_area = Rect::default();
    state.normalize(row);
    let footer = Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1);
    let body = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1));
    tapp_ui::chrome::StatusBar {
        left: tapp_ui::loading::status_line(
            Line::from(state.status.clone()),
            state.loading.then_some(state.loading_symbol.as_str()),
            state.waiting,
            tapp_ui::theme::Role::Search.style(theme.monochrome()),
        ),
        right: Line::from(if row.baseline.is_some() {
            if area.width >= 75 {
                "↑↓ months · ←→ graph · p period · Esc list · ? help"
            } else {
                "↑↓ · ←→ · p period · Esc · ? help"
            }
        } else if state.loading && area.width < 68 {
            "↑↓ · l/Esc · ?"
        } else if state.section == Section::Graph {
            if area.width >= 60 {
                "↑↓ scroll · ←→ tabs · l/Esc list · ? help"
            } else {
                "↑↓ · ←→ tabs · Esc · ? help"
            }
        } else if area.width >= 90 {
            "↑↓ scroll · Tab focus · ←→ tabs · p period · l/Esc list · ? help"
        } else if area.width >= 45 {
            "↑↓ move · Tab · ←→ tabs · Esc · ? help"
        } else {
            "↑↓ · Tab · ←→ · Esc · ? help"
        }),
        style: theme.muted,
    }
    .draw(frame, footer);
    if state.pending_history && state.section != Section::Info {
        let top = Layout::vertical([
            Constraint::Length(hero_height(row, body.width)),
            Constraint::Min(0),
        ])
        .split(body);
        hero(frame, top[0], row, theme);
        let rows = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ]
        .into_iter()
        .take(shown_count(top[1]))
        .map(|month| Row::new(vec![month, "—", "—/—"]).style(theme.muted));
        frame.render_widget(
            tapp_ui::table::compact(
                rows,
                [
                    Constraint::Length(6),
                    Constraint::Length(9),
                    Constraint::Length(13),
                ],
                top[1].width,
            )
            .header(Row::new(["Month", "Mean °C", "Low/high °C"]).style(theme.accent))
            .block(block("Monthly averages · loading".into(), true, theme)),
            top[1],
        );
        return;
    }
    if state.section == Section::Info {
        state.section = state.info_return;
        draw(frame, area, row, state, theme);
        state.section = Section::Info;
        use std::hash::{Hash, Hasher};
        let width = tapp_ui::layout::modal_rect(body).width.saturating_sub(4);
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        row.city.id.hash(&mut hash);
        row.city.name.hash(&mut hash);
        row.range_timezone.hash(&mut hash);
        row.range_date.hash(&mut hash);
        row.retry_at.hash(&mut hash);
        row.baseline.as_ref().map(|b| &b.method).hash(&mut hash);
        for (source, meta) in &row.provenance {
            source.hash(&mut hash);
            meta.fetched_at.hash(&mut hash);
            meta.cached.hash(&mut hash);
            meta.stale.hash(&mut hash);
            meta.warning.hash(&mut hash);
            meta.retry_at.hash(&mut hash);
        }
        for (name, range) in &row.ranges {
            name.hash(&mut hash);
            range.min.map(f64::to_bits).hash(&mut hash);
            range.max.map(f64::to_bits).hash(&mut hash);
        }
        row.time.hash(&mut hash);
        view::city_now(row)
            .map(|t| t.timestamp().div_euclid(60))
            .hash(&mut hash);
        row.sources.hash(&mut hash);
        row.error.hash(&mut hash);
        row.warnings.hash(&mut hash);
        width.hash(&mut hash);
        theme.monochrome().hash(&mut hash);
        row.absolute_values.is_some().hash(&mut hash);
        for (key, value) in &row.values {
            key.hash(&mut hash);
            value.map(f64::to_bits).hash(&mut hash);
        }
        let key = hash.finish();
        if state.info_key != Some(key) {
            state.info_cache = ratatui::text::Text::from(
                info_lines(row, usize::from(width))
                    .into_iter()
                    .map(|line| {
                        if view::INFO_HEADINGS.contains(&line.as_str()) {
                            return Line::styled(
                                line,
                                tapp_ui::theme::Role::Info.style(theme.monochrome()).bold(),
                            );
                        }
                        let key = [
                            ("Temperature:", "temperature_2m"),
                            ("Feels like:", "apparent_temperature"),
                            ("Precipitation:", "precipitation"),
                            ("Rain:", "rain"),
                            ("Snowfall:", "snowfall"),
                            ("Wind:", "wind_speed_10m"),
                        ]
                        .iter()
                        .find(|(label, _)| line.starts_with(label))
                        .map(|(_, key)| *key);
                        if let Some(key) = key {
                            let style = theme.metric(
                                key,
                                value(row.absolute_values.as_ref().unwrap_or(&row.values), key),
                                row.baseline.is_some(),
                            );
                            if let Some((primary, secondary)) = line.split_once("  [") {
                                Line::from(vec![
                                    Span::styled(primary.to_owned(), style),
                                    Span::styled(
                                        format!("  [{secondary}"),
                                        style.add_modifier(ratatui::style::Modifier::DIM),
                                    ),
                                ])
                            } else {
                                Line::styled(line, style)
                            }
                        } else if line == view::condition_text(row.condition.as_ref()) {
                            let mut text = theme.condition_line(row.condition.as_ref());
                            text.spans.push(Span::raw(format!(
                                " {}",
                                view::weather_label(row.condition.as_ref())
                            )));
                            text
                        } else {
                            Line::from(line)
                        }
                    })
                    .collect::<Vec<_>>(),
            );
            state.info_key = Some(key);
        }
        let inner = tapp_ui::chrome::draw_content(
            frame,
            body,
            &format!("{} · details / sources", row.city.name),
            tapp_ui::modal::Category::Info,
            theme.monochrome(),
            &state.info_cache,
            &mut state.info_offset,
            false,
        );
        state.info_max = state
            .info_cache
            .height()
            .saturating_sub(usize::from(inner.height));
        return;
    }
    if body.height < 14 {
        let values = row.absolute_values.as_ref().unwrap_or(&row.values);
        tapp_ui::chrome::StatusBar {
            left: Line::from(vec![
                Span::raw(format!("{} · ", view::clean(&row.city.name))),
                theme.metric_span(
                    format!("{}°", view::number(value(values, "temperature_2m"), false)),
                    "temperature_2m",
                    value(values, "temperature_2m"),
                    false,
                ),
            ]),
            right: Line::styled(view::clock_text(row, body.width >= 55), theme.muted),
            style: Default::default(),
        }
        .draw(frame, Rect::new(body.x, body.y, body.width, 1));
        draw_tabs(
            frame,
            Rect::new(body.x, body.y + 1, body.width, 1),
            state,
            theme,
        );
        let table = Rect::new(
            body.x,
            body.y + 2,
            body.width,
            body.height.saturating_sub(2),
        );
        if state.section == Section::Graph {
            state.hour_count = WINDOW;
            state.day_count = WINDOW;
            state.normalize(row);
            draw_graph(frame, table, row, state, theme);
        } else if row.baseline.is_some() {
            state.month_area = table;
            state.month_count = shown_count(table);
            state.normalize(row);
            monthly_table(frame, table, row, state, theme);
        } else if state.section == Section::Days {
            state.day_area = table;
            state.day_count = shown_count(table);
            state.normalize(row);
            forecast_table(frame, table, row, state, false, theme);
        } else {
            state.hour_area = table;
            state.hour_count = shown_count(table);
            state.normalize(row);
            forecast_table(frame, table, row, state, true, theme);
        }
        return;
    }
    let hero_height = hero_height(row, body.width);
    if state.compact && state.section == Section::Graph {
        hero(
            frame,
            Rect::new(body.x, body.y, body.width, hero_height),
            row,
            theme,
        );
        state.hour_count = WINDOW;
        state.day_count = WINDOW;
        state.normalize(row);
        draw_tabs(
            frame,
            Rect::new(body.x, body.y + hero_height, body.width, 1),
            state,
            theme,
        );
        draw_graph(
            frame,
            Rect::new(
                body.x,
                body.y + hero_height + 1,
                body.width,
                body.height.saturating_sub(hero_height + 1),
            ),
            row,
            state,
            theme,
        );
        return;
    }
    let chart_height = if !state.compact {
        7.max(body.height.saturating_sub(hero_height + 22))
    } else {
        0
    };
    let top = Layout::vertical([
        Constraint::Length(hero_height),
        Constraint::Length(u16::from(state.compact)),
        Constraint::Length(chart_height),
        Constraint::Min(0),
    ])
    .split(body);
    hero(frame, top[0], row, theme);
    if state.compact {
        draw_tabs(frame, top[1], state, theme);
    }
    if chart_height > 0 {
        if row.baseline.is_none() {
            let height = top[3].height.min(22);
            state.hour_count = usize::from((height / 2).saturating_sub(3)).min(WINDOW);
            state.day_count = usize::from((height - height / 2).saturating_sub(3)).min(WINDOW);
            state.normalize(row);
        }
        draw_graph(frame, top[2], row, state, theme);
    }
    if row.baseline.is_some() {
        state.month_area = Rect::new(top[3].x, top[3].y, top[3].width, top[3].height.min(15));
        state.month_count = usize::from(state.month_area.height.saturating_sub(3)).min(12);
        state.normalize(row);
        monthly_table(frame, state.month_area, row, state, theme);
        let y = state.month_area.bottom();
        if top[3].bottom() > y + 3 {
            let cloud = row
                .monthly
                .iter()
                .map(|m| {
                    value(&m.values, "cloud_cover")
                        .map(|v| format!("{v:.0}%"))
                        .unwrap_or_else(|| "—".into())
                })
                .collect::<Vec<_>>()
                .join("  ");
            let area = Rect::new(body.x, y, body.width, 3);
            frame.render_widget(
                Paragraph::new(cloud).block(block("Cloud cover · Jan → Dec".into(), false, theme)),
                area,
            );
        }
    } else {
        let total = top[3].height.min(22);
        let a = total / 2;
        let b = total - a;
        state.hour_area = Rect::new(top[3].x, top[3].y, top[3].width, a);
        state.day_area = Rect::new(top[3].x, top[3].y + a, top[3].width, b);
        state.hour_count = shown_count(state.hour_area);
        state.day_count = shown_count(state.day_area);
        state.normalize(row);
        forecast_table(frame, state.hour_area, row, state, true, theme);
        forecast_table(frame, state.day_area, row, state, false, theme);
    }
}

fn draw_graph(frame: &mut Frame, area: Rect, row: &Weather, state: &mut State, theme: &Theme) {
    state.graph_area = area;
    let metric = state.metric;
    let graph_area = if state.compact {
        area
    } else {
        draw_tabs(
            frame,
            Rect::new(area.x, area.y, area.width, 1),
            state,
            theme,
        );
        Rect::new(
            area.x,
            area.y + 1,
            area.width,
            area.height.saturating_sub(1),
        )
    };
    let values = if row.baseline.is_some() {
        row.monthly
            .iter()
            .map(|m| match metric {
                Metric::TemperatureMin => value(&m.values, "low"),
                Metric::TemperatureMax => value(&m.values, "high"),
                Metric::FeelsMin => m.ranges.get("apparent_temperature").and_then(|r| r.min),
                Metric::FeelsMax => m.ranges.get("apparent_temperature").and_then(|r| r.max),
                Metric::Daylight => value(&m.values, "daylight").map(|v| v / 60.),
                _ => value(&m.values, metric.key()),
            })
            .collect::<Vec<_>>()
    } else if state.graph_daily() {
        row.daily
            .iter()
            .skip(state.day_offset)
            .take(state.day_count.max(1))
            .map(|d| match metric {
                Metric::Temperature => d.temperature_mean,
                Metric::Feels => d.feels_mean,
                Metric::FeelsMin => d.feels_min,
                Metric::FeelsMax => d.feels_max,
                Metric::TemperatureMin => d.temperature_min,
                Metric::TemperatureMax => d.temperature_max,
                Metric::Daylight => value(&d.solar, "daylight").map(|v| v / 60.),
                Metric::Rain => d.rain_sum,
                Metric::Snow => d.snowfall_sum,
                Metric::Wind => d.wind_speed_max,
                Metric::Cloud => d.cloud_cover,
            })
            .collect()
    } else {
        row.hourly
            .iter()
            .skip(state.hour_offset)
            .take(state.hour_count.max(1))
            .map(|h| value(&h.values, metric.key()))
            .collect()
    };
    let panel = block(
        format!(
            "{} · {} · {}{}",
            if !state.graph_daily() && metric == Metric::Temperature {
                "Temperature"
            } else if row.baseline.is_none() && state.graph_daily() && metric == Metric::Wind {
                "Wind max"
            } else {
                metric.label()
            },
            if row.absolute_values.is_some() && metric == Metric::Cloud {
                "pp"
            } else {
                metric.unit(row.baseline.is_some())
            },
            if row.baseline.is_some() {
                "Year"
            } else if state.graph_daily() {
                "visible days"
            } else {
                "visible hours"
            },
            if values.iter().all(Option::is_none) {
                if state.loading && row.time.is_empty() {
                    " · loading"
                } else {
                    " · no data"
                }
            } else {
                ""
            }
        ),
        state.section == Section::Graph,
        theme,
    );
    let bounds = metric.bounds(&values);
    let labels = if row.baseline.is_some() {
        vec![Line::from("Jan"), Line::from("Dec")]
    } else if state.graph_daily() {
        let dates: Vec<_> = row
            .daily
            .iter()
            .skip(state.day_offset)
            .take(values.len())
            .collect();
        [dates.first(), dates.last()]
            .into_iter()
            .map(|d| {
                Line::from(
                    d.map_or("—", |d| d.date.get(5..).unwrap_or(&d.date))
                        .to_owned(),
                )
            })
            .collect()
    } else {
        let count = if graph_area.width >= 90 {
            4
        } else if graph_area.width >= 55 {
            3
        } else {
            2
        };
        let first = row
            .hourly
            .get(state.hour_offset)
            .and_then(|h| chrono::NaiveDateTime::parse_from_str(&h.time, "%Y-%m-%dT%H:%M").ok());
        (0..count)
            .map(|i| {
                let span = if matches!(metric, Metric::Rain | Metric::Snow) {
                    values.len().max(1) as f64
                } else {
                    values.len().saturating_sub(1).max(1) as f64
                };
                let offset = if matches!(metric, Metric::Rain | Metric::Snow) {
                    -0.5
                } else {
                    0.0
                } + span * i as f64 / (count - 1) as f64;
                let label = first
                    .map(|t| {
                        let time = t + chrono::Duration::minutes((offset * 60.0).round() as i64);
                        let utc = time.format("%Y-%m-%dT%H:%M").to_string();
                        view::local_time(&utc, &row.range_timezone)[11..].to_owned()
                    })
                    .unwrap_or_else(|| "—".into());
                Line::from(label)
            })
            .collect()
    };
    use std::hash::{Hash, Hasher};
    let mut fingerprint = std::collections::hash_map::DefaultHasher::new();
    metric.label().hash(&mut fingerprint);
    (state.section == Section::Graph).hash(&mut fingerprint);
    row.city.id.hash(&mut fingerprint);
    state.hour_offset.hash(&mut fingerprint);
    state.day_offset.hash(&mut fingerprint);
    let daily_graph = state.graph_daily();
    daily_graph.hash(&mut fingerprint);
    theme.monochrome().hash(&mut fingerprint);
    row.baseline.is_some().hash(&mut fingerprint);
    row.range_timezone.hash(&mut fingerprint);
    for value in &values {
        value.map(f64::to_bits).hash(&mut fingerprint);
    }
    for label in &labels {
        label.to_string().hash(&mut fingerprint);
    }
    state
        .chart
        .draw(frame, graph_area, fingerprint.finish(), |raster| {
            let segments = graph_segments(metric, &values, daily_graph);
            let zero = [
                (0.0, 0.0),
                (values.len().saturating_sub(1).max(1) as f64, 0.0),
            ];
            let mut datasets = Vec::with_capacity(segments.len() + 1);
            if metric.graph_type() == GraphType::Line && bounds[0] <= 0.0 && bounds[1] >= 0.0 {
                // Draw first: render_graph merges masks, retaining curve dots at crossings.
                let style = if theme.monochrome() {
                    ratatui::style::Style::default()
                } else {
                    ratatui::style::Style::default().fg(ratatui::style::Color::DarkGray)
                };
                datasets.push(
                    Dataset::default()
                        .graph_type(GraphType::Line)
                        .marker(ratatui::symbols::Marker::Braille)
                        .style(style)
                        .data(&zero),
                );
            }
            datasets.extend(segments.iter().map(|segment| {
                Dataset::default()
                    .graph_type(metric.graph_type())
                    .marker(ratatui::symbols::Marker::Braille)
                    .style(theme.metric(metric.key(), Some(segment.value), daily_graph))
                    .data(&segment.points)
            }));
            render_graph(
                datasets,
                panel,
                Axis::default()
                    .bounds(if matches!(metric, Metric::Rain | Metric::Snow) {
                        [-0.5, values.len().max(1) as f64 - 0.5]
                    } else {
                        [0.0, values.len().saturating_sub(1).max(1) as f64]
                    })
                    .labels(labels)
                    .style(theme.muted),
                Axis::default()
                    .bounds(bounds)
                    .labels(vec![
                        Line::from(view::number(Some(bounds[0]), false)),
                        Line::from(view::number(Some(bounds[1]), false)),
                    ])
                    .style(theme.muted),
                graph_area,
                raster,
            );
        });
}

fn draw_tabs(frame: &mut Frame, area: Rect, state: &mut State, theme: &Theme) {
    use unicode_width::UnicodeWidthStr;
    let metrics = state.metrics();
    let labels: Vec<String> = metrics
        .iter()
        .map(|metric| {
            let label = match metric {
                Metric::Temperature if !state.graph_daily() => "Temp",
                Metric::Temperature => "Mean",
                Metric::TemperatureMin => "Min",
                Metric::TemperatureMax => "Max",
                other => other.label(),
            };
            if area.width >= 92 {
                format!("{} {label}", state.metric_shortcut(*metric).unwrap())
            } else {
                label.into()
            }
        })
        .collect();
    let metric = metrics.iter().position(|m| *m == state.metric).unwrap_or(0);
    // Keep the active metric visible, retaining as many neighboring tabs as fit.
    // Tables stays reachable by mouse even when the graph strip overflows.
    let budget = usize::from(area.width).saturating_sub(if state.compact { 11 } else { 2 });
    let mut start = metric;
    let mut end = metric + 1;
    let mut used = labels[metric].width() + 3;
    while start > 0 && used + labels[start - 1].width() + 3 <= budget {
        start -= 1;
        used += labels[start].width() + 3;
    }
    while end < labels.len() && used + labels[end].width() + 3 <= budget {
        used += labels[end].width() + 3;
        end += 1;
    }
    let mut titles: Vec<String> = if state.compact {
        vec!["Tables".into()]
    } else {
        vec![]
    };
    titles.extend(labels.iter().take(end).skip(start).cloned());
    state.tabs.selected = if state.compact {
        if state.section == Section::Graph {
            metric - start + 1
        } else {
            0
        }
    } else {
        metric - start
    };
    state.previous_tab = Rect::default();
    state.next_tab = Rect::default();
    if start > 0 {
        state.previous_tab = Rect::new(area.x, area.y, 1, 1);
        frame.render_widget(Paragraph::new("‹").style(theme.accent), state.previous_tab);
    }
    if end < labels.len() {
        state.next_tab = Rect::new(area.right().saturating_sub(1), area.y, 1, 1);
        frame.render_widget(Paragraph::new("›").style(theme.accent), state.next_tab);
    }
    state.tabs.draw(
        frame,
        Rect::new(area.x + 1, area.y, area.width.saturating_sub(2), 1),
        &titles,
        theme.monochrome(),
    );
    let skip = usize::from(state.compact);
    if state.compact {
        state.tables_tab = state.tabs.hits.first().copied().unwrap_or_default();
    }
    state.graph_tabs = state
        .tabs
        .hits
        .iter()
        .skip(skip)
        .copied()
        .zip(metrics.iter().copied().take(end).skip(start))
        .collect();
}
