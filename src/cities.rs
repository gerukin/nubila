//! Domain adapter for tapp-ui search and confirmation components.
use crate::{config::Config, model::City, theme::Theme, view};
use anyhow::{Result, ensure};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    text::Line,
    widgets::{Cell, Row},
};
use tapp_ui::{
    confirmation::Confirmation,
    focus::Outcome,
    search::{SearchDialog, SearchEvent},
};
#[derive(Debug, Clone)]
pub struct Candidate {
    pub city: City,
    pub region: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    None,
    Close,
    Search,
    Add,
    Remove,
    Quit,
}
pub struct State {
    pub search: SearchDialog<String>,
    pub results: Vec<Candidate>,
    pub removal: Option<City>,
    pub message: String,
    pub request_id: u64,
    pub results_area: Rect,
    pub confirmation: Confirmation,
}
impl Default for State {
    fn default() -> Self {
        let mut confirmation = Confirmation::default();
        confirmation.selected = 1;
        let mut search = SearchDialog::default();
        search.accept_on_click = false;
        search.limit = 10;
        Self {
            search,
            results: vec![],
            removal: None,
            message: String::new(),
            request_id: 0,
            results_area: Rect::default(),
            confirmation,
        }
    }
}
impl State {
    pub fn query(&self) -> &str {
        self.search.input.value()
    }
    pub fn selected(&self) -> usize {
        self.search.viewport.selected().unwrap_or(0)
    }
    pub fn receive(&mut self, result: Result<Vec<Candidate>>) {
        match result {
            Ok(rows) => {
                self.results = rows;
                self.search.complete(
                    self.search.revision(),
                    self.results.iter().map(|r| r.city.id.clone()).collect(),
                    false,
                );
                self.message = if self.results.is_empty() {
                    "No matching cities".into()
                } else {
                    "↑↓ choose · Enter saves".into()
                };
            }
            Err(e) => {
                self.message = format!("Search failed: {e:#}");
                self.search
                    .fail(self.search.revision(), self.message.clone());
            }
        }
    }
    pub fn handle(&mut self, event: &Event) -> Intent {
        if let Event::Key(k) = event
            && k.kind != KeyEventKind::Release
            && k.code == KeyCode::Char('c')
            && k.modifiers == KeyModifiers::CONTROL
        {
            return Intent::Quit;
        }
        if self.removal.is_some() {
            return match self.confirmation.handle(event) {
                Outcome::FocusReleased(_) => Intent::Close,
                Outcome::Submit if self.confirmation.selected == 1 => Intent::Remove,
                Outcome::Submit => Intent::Close,
                _ => Intent::None,
            };
        }
        if let Event::Key(k) = event
            && k.kind != KeyEventKind::Release
            && k.modifiers == KeyModifiers::NONE
            && k.code == KeyCode::Enter
            && self.results.is_empty()
        {
            if self.query().trim().chars().count() < 2 {
                self.message = "Enter at least two characters".into();
                return Intent::None;
            }
            if !self.search.loading {
                self.search.loading = true;
                self.message = "Searching…".into();
                return Intent::Search;
            }
            return Intent::None;
        }
        match self.search.handle(event) {
            SearchEvent::Dismiss => Intent::Close,
            SearchEvent::Accept(_) => Intent::Add,
            SearchEvent::QueryChanged(_) => {
                self.results.clear();
                self.message.clear();
                self.request_id = 0;
                self.search.loading = false;
                Intent::None
            }
            _ => Intent::None,
        }
    }
    pub fn draw(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        config: &Config,
        demo: bool,
        theme: &Theme,
    ) {
        if let Some(city) = &self.removal {
            let text = if config.cities.iter().any(|c| c.id == city.id) {
                format!(
                    "Remove {} from favorites?{}{}",
                    city.name,
                    if config.reference == city.id {
                        " The pinned city will change to another saved city."
                    } else {
                        ""
                    },
                    if self.message.is_empty() {
                        String::new()
                    } else {
                        format!("\n{}", self.message)
                    }
                )
            } else {
                format!("Hide {}? This disables automatic location.", city.name)
            };
            self.confirmation.draw(
                frame,
                area,
                "Remove city",
                &text,
                "Remove",
                tapp_ui::theme::Role::Search,
                theme.monochrome(),
            );
            return;
        }
        self.search.monochrome = theme.monochrome();
        self.search.viewport.selection_style = Some(theme.selected);
        let rows = &self.results;
        let footer = if self.message.is_empty() {
            if demo {
                "DEMO · memory only"
            } else {
                "Enter search/save"
            }
        } else {
            &self.message
        };
        self.search.draw_table(
            frame,
            area,
            "Add city",
            footer,
            vec![],
            vec![Constraint::Percentage(40), Constraint::Percentage(60)],
            |id, _selected| {
                let row = rows
                    .iter()
                    .find(|r| &r.city.id == id)
                    .expect("search payload");
                Row::new(vec![
                    Cell::from(row.city.name.as_str()),
                    Cell::from(Line::styled(row.region.as_str(), theme.muted)),
                ])
            },
        );
        self.results_area = self.search.viewport.area();
    }
}

pub fn candidates(value: serde_json::Value) -> Result<Vec<Candidate>> {
    let rows = value
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Invalid city search response"))?;
    let mut out = vec![];
    for r in rows {
        let (Some(id), Some(name), Some(lat), Some(lon)) = (
            r["id"].as_u64(),
            r["name"].as_str(),
            r["latitude"].as_f64(),
            r["longitude"].as_f64(),
        ) else {
            continue;
        };
        let city = City {
            id: format!("geonames-{id}"),
            name: view::clean(name),
            latitude: lat,
            longitude: lon,
        };
        if city.validate().is_err() {
            continue;
        }
        let region = ["admin1", "country"]
            .iter()
            .filter_map(|k| r[k].as_str())
            .map(view::clean)
            .collect::<Vec<_>>()
            .join(", ");
        out.push(Candidate { city, region });
    }
    Ok(out)
}
pub fn add(config: &mut Config, city: City) -> Result<()> {
    city.validate()?;
    ensure!(
        !config.cities.iter().any(|c| c.id == city.id
            || (c.name.eq_ignore_ascii_case(&city.name)
                && (c.latitude - city.latitude).abs() < 0.25
                && (c.longitude - city.longitude).abs() < 0.25)),
        "This city is already saved"
    );
    config.cities.push(city);
    config.validate()
}
pub fn remove(config: &mut Config, id: &str) -> Result<()> {
    if id == "__current__" && !config.cities.iter().any(|c| c.id == id) {
        config.auto_location = false;
        return Ok(());
    }
    ensure!(
        config.cities.iter().any(|c| c.id == id),
        "City is no longer saved"
    );
    ensure!(config.cities.len() > 1, "Keep at least one saved city");
    config.cities.retain(|c| c.id != id);
    if config.reference == id {
        config.reference = config.cities[0].id.clone();
    }
    config.validate()
}
pub fn demo_search(query: &str) -> Vec<Candidate> {
    [
        ("kyoto", "Kyoto", 35.0116, 135.7681, "Japan"),
        ("osaka", "Osaka", 34.6937, 135.5023, "Japan"),
        ("new-york", "New York", 40.7128, -74.006, "United States"),
    ]
    .into_iter()
    .filter(|(_, name, _, _, _)| name.to_lowercase().contains(&query.to_lowercase()))
    .map(|(id, name, latitude, longitude, region)| Candidate {
        city: City {
            id: id.into(),
            name: name.into(),
            latitude,
            longitude,
        },
        region: region.into(),
    })
    .collect()
}
