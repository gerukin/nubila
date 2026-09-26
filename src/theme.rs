//! Semantic styles using terminal defaults and the user's ANSI palette.
use ratatui::style::{Color, Modifier, Style};

pub struct Theme {
    pub accent: Style,
    pub selected: Style,
    pub muted: Style,
    pub border: Style,
}
impl Theme {
    pub fn with_palette(mut self, palette: tapp_ui::theme::Theme) -> Self {
        self.selected = tapp_ui::table::RowSelection::for_theme(palette).style(self.monochrome());
        self
    }
    pub fn monochrome(&self) -> bool {
        self.accent.fg.is_none()
    }
    pub fn thresholds(key: &str, daily_rain: bool) -> &'static [f64] {
        match key {
            "temperature_2m" | "apparent_temperature" => &[0.0, 10.0, 25.0, 30.0, 35.0],
            "wind_speed_10m" => &[20.0, 40.0, 60.0],
            "precipitation" | "rain" if daily_rain => &[0.0, 10.0, 30.0],
            "precipitation" | "rain" => &[0.0, 2.5, 7.5],
            "snowfall" if daily_rain => &[0.0, 5.0, 15.0],
            "snowfall" => &[0.0, 1.0, 5.0],
            _ => &[],
        }
    }
    pub fn new(monochrome: bool) -> Self {
        Self {
            accent: if monochrome {
                Style::default()
            } else {
                Style::default().fg(Color::Cyan)
            }
            .add_modifier(Modifier::BOLD),
            selected: tapp_ui::table::RowSelection {
                emphasize: true,
                ..Default::default()
            }
            .style(monochrome),
            muted: Style::default().add_modifier(Modifier::DIM),
            border: Style::default().add_modifier(Modifier::DIM),
        }
    }
}

impl Theme {
    fn color(&self, color: Color) -> Style {
        if self.accent.fg.is_none() {
            Style::default()
        } else {
            Style::default().fg(color)
        }
    }
    pub fn condition(&self, condition: Option<&crate::model::Condition>) -> Style {
        self.color(match condition.map(|c| c.code) {
            Some(0..=2) if condition.is_some_and(|c| c.is_day == Some(false)) => Color::Cyan,
            Some(0..=2) => Color::Yellow,
            Some(3 | 45 | 48) => return self.muted,
            Some(51 | 56 | 61 | 66 | 80) => Color::Cyan,
            Some(53 | 63 | 81) => Color::Blue,
            Some(55 | 57 | 65 | 67 | 82) => Color::LightBlue,
            Some(71 | 77 | 85) => Color::Blue,
            Some(73) => Color::Cyan,
            Some(75 | 86) => Color::LightCyan,
            Some(95) => Color::Magenta,
            Some(96 | 99) => Color::LightMagenta,
            _ => return self.muted,
        })
    }
    pub fn condition_line(
        &self,
        condition: Option<&crate::model::Condition>,
    ) -> ratatui::text::Line<'static> {
        ratatui::text::Line::from(ratatui::text::Span::styled(
            crate::view::weather_symbol(condition),
            self.condition(condition),
        ))
    }
    /// Visual intensity bands, not safety alerts. Daily rain uses daily thresholds.
    pub fn metric(&self, key: &str, value: Option<f64>, daily_rain: bool) -> Style {
        let Some(v) = value.filter(|v| v.is_finite()) else {
            return self.muted;
        };
        let thresholds = Self::thresholds(key, daily_rain);
        let color = match key {
            "temperature_2m" | "apparent_temperature" => {
                if v < thresholds[0] {
                    Color::Blue
                } else if v < thresholds[1] {
                    Color::Cyan
                } else if v < thresholds[2] {
                    return Style::default();
                } else if v < thresholds[3] {
                    Color::Yellow
                } else if v < thresholds[4] {
                    Color::Red
                } else {
                    Color::Magenta
                }
            }
            "precipitation" | "rain" | "snowfall" => {
                let (light, heavy) = (thresholds[1], thresholds[2]);
                if v <= 0.0 {
                    return Style::default();
                } else if v < light {
                    Color::Cyan
                } else if v < heavy {
                    Color::Blue
                } else {
                    Color::LightBlue
                }
            }
            "wind_speed_10m" => {
                if v < thresholds[0] {
                    return Style::default();
                } else if v < thresholds[1] {
                    Color::Yellow
                } else if v < thresholds[2] {
                    Color::Red
                } else {
                    Color::Magenta
                }
            }
            _ => return Style::default(),
        };
        self.color(color)
    }
    pub fn metric_span(
        &self,
        text: impl Into<String>,
        key: &str,
        value: Option<f64>,
        daily: bool,
    ) -> ratatui::text::Span<'static> {
        ratatui::text::Span::styled(text.into(), self.metric(key, value, daily))
    }
}
impl Theme {
    pub fn range(
        &self,
        text: &str,
        key: &str,
        min: Option<f64>,
        max: Option<f64>,
        daily: bool,
    ) -> ratatui::text::Line<'static> {
        use ratatui::text::{Line, Span};
        if let Some((a, b)) = text.split_once('/') {
            Line::from(vec![
                self.metric_span(a, key, min, daily),
                Span::styled("/", self.muted),
                self.metric_span(b, key, max, daily),
            ])
        } else {
            Line::styled(text.to_owned(), self.muted)
        }
    }
    pub fn secondary_range(
        &self,
        text: &str,
        key: &str,
        min: Option<f64>,
        max: Option<f64>,
        daily: bool,
    ) -> ratatui::text::Line<'static> {
        let mut line = self.range(text, key, min, max, daily);
        for span in &mut line.spans {
            span.style = span.style.add_modifier(Modifier::DIM);
        }
        line
    }
}

impl Theme {
    /// The guide uses the same thresholds and styles as cells and graphs.
    pub fn color_guide(&self) -> Vec<tapp_ui::chrome::Entry> {
        use ratatui::text::{Line, Span};
        use tapp_ui::chrome::Entry;
        let mut entries = vec![
            Entry::Heading("Color guide".into()),
            Entry::Text("Primary values: full intensity · ranges: muted".into()),
        ];
        for (key, daily, label) in [
            ("temperature_2m", false, "Temperature / feels · °C"),
            ("wind_speed_10m", false, "Wind · km/h"),
            ("rain", false, "Rain · mm · hourly / current"),
            ("rain", true, "Rain · mm · daily / climate display"),
            ("snowfall", false, "Snow · cm · hourly / current"),
            ("snowfall", true, "Snow · cm · daily / climate display"),
        ] {
            entries.push(Entry::Text(label.into()));
            let thresholds = Self::thresholds(key, daily);
            let bands: Vec<(String, f64)> = if matches!(key, "rain" | "snowfall") {
                let (a, b) = (thresholds[1], thresholds[2]);
                vec![
                    ("Dry".into(), 0.0),
                    (format!(">0–<{a}"), a / 2.0),
                    (format!("{a}–<{b}"), a),
                    (format!("≥{b}"), b),
                ]
            } else {
                (0..=thresholds.len())
                    .map(|i| {
                        if i == 0 {
                            (format!("<{}", thresholds[0]), thresholds[0] - 1.0)
                        } else if i == thresholds.len() {
                            (format!("≥{}", thresholds[i - 1]), thresholds[i - 1])
                        } else {
                            (
                                format!("{}–<{}", thresholds[i - 1], thresholds[i]),
                                thresholds[i - 1],
                            )
                        }
                    })
                    .collect()
            };
            for group in bands.chunks(3) {
                let mut spans = Vec::new();
                for (i, (range, value)) in group.iter().enumerate() {
                    if i > 0 {
                        spans.push(Span::styled(" · ", self.muted));
                    }
                    let color = match Self::new(false).metric(key, Some(*value), daily).fg {
                        Some(Color::Blue) => "blue",
                        Some(Color::Cyan) => "cyan",
                        Some(Color::Yellow) => "yellow",
                        Some(Color::Red) => "red",
                        Some(Color::Magenta) => "magenta",
                        Some(Color::LightBlue) => "bright blue",
                        _ => "neutral",
                    };
                    spans.push(self.metric_span(
                        format!("{range} {color}"),
                        key,
                        Some(*value),
                        daily,
                    ));
                }
                entries.push(Entry::Rich(Line::from(spans)));
            }
        }
        entries
    }
}
