//! Offline climate UI captures for review; does not read or change user files.
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use nubila::{
    config::{Config, EXAMPLE, Preferences},
    model::{Mode, Period},
    service::{Client, FetchOptions, Service},
    tui::{App, draw},
};
use ratatui::{Terminal, backend::TestBackend};
fn main() {
    let config: Config = toml::from_str(EXAMPLE).unwrap();
    for (period, mode, comparison_period, width, height) in [
        (Period::Baseline, Mode::Normal, None, 100, 40),
        (Period::Future, Mode::Comparison, None, 80, 24),
        (
            Period::Future,
            Mode::Comparison,
            Some(Period::Baseline),
            180,
            40,
        ),
        (Period::Now, Mode::Comparison, Some(Period::Recent), 80, 24),
    ] {
        let prefs = Preferences {
            reference: config.reference.clone(),
            period,
            mode,
            comparison_period,
            ..Default::default()
        };
        let service = Service {
            config: config.clone(),
            client: Client::new(std::env::temp_dir(), true),
            options: FetchOptions {
                demo: true,
                offline: true,
                refresh: false,
                no_location: true,
                years: 5,
                month: 1,
                city_ids: vec![],
                past_days: 2,
            },
        };
        let mut app = App::new(prefs.clone(), true, String::new());
        app.config = config.clone();
        app.receive(service.load(&prefs, false));
        for (name, key) in [
            ("list", None),
            ("details", Some(KeyCode::Enter)),
            ("comparison", Some(KeyCode::Char('c'))),
            ("comparison-return", Some(KeyCode::Esc)),
            ("period", Some(KeyCode::Char('p'))),
        ] {
            if let Some(key) = key {
                app.handle(Event::Key(KeyEvent::new(key, KeyModifiers::NONE)));
            }
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|f| draw(f, &mut app)).unwrap();
            println!("\n{name} {period:?} {mode:?} {width}x{height}");
            for y in 0..height {
                let line: String = (0..width)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect();
                println!("{}", line.trim_end());
            }
        }
    }
}
