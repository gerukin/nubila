//! Offline text/style review captures; no network or user settings.
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use nubila::{
    config::{Config, EXAMPLE, Preferences},
    service::{Client, FetchOptions, Service},
    tui::{App, draw},
};
use ratatui::{Terminal, backend::TestBackend};
fn main() {
    let config: Config = toml::from_str(EXAMPLE).unwrap();
    let prefs = Preferences {
        reference: config.reference.clone(),
        ..Default::default()
    };
    let service = Service {
        config,
        client: Client::new(std::env::temp_dir(), true),
        options: FetchOptions {
            demo: true,
            offline: true,
            refresh: false,
            no_location: true,
            years: 10,
            month: 9,
            city_ids: vec![],
            past_days: 2,
        },
    };
    let mut app = App::new(prefs.clone(), true, String::new());
    app.receive(service.load(&prefs, false));
    for (name, key) in [
        ("main", None),
        ("filter", Some(KeyCode::Char('/'))),
        ("filter-return", Some(KeyCode::Esc)),
        ("focus", Some(KeyCode::Down)),
        ("remove", Some(KeyCode::Char('d'))),
        ("remove-return", Some(KeyCode::Esc)),
        ("commands", Some(KeyCode::F(2))),
        ("main-again", Some(KeyCode::Esc)),
        ("add-city", Some(KeyCode::Char('a'))),
        ("search-results", None),
        ("return", Some(KeyCode::Esc)),
        ("detail", Some(KeyCode::Enter)),
        ("rain-graph", Some(KeyCode::Char('3'))),
        ("info", Some(KeyCode::Char('i'))),
        ("info-return", Some(KeyCode::Esc)),
        ("help", Some(KeyCode::F(1))),
    ] {
        if let Some(key) = key {
            app.handle(Event::Key(KeyEvent::new(key, KeyModifiers::NONE)));
        }
        if name == "filter" {
            app.handle(Event::Paste("Tok".into()));
        }
        if name == "filter-return" {
            app.handle(Event::Key(KeyEvent::new(
                KeyCode::Char('u'),
                KeyModifiers::CONTROL,
            )));
        }
        if name == "search-results" {
            app.city_manager.as_mut().unwrap().search.set_query("Kyoto");
            app.city_manager
                .as_mut()
                .unwrap()
                .receive(Ok(nubila::cities::demo_search("")));
        }
        for (w, h) in [(32, 12), (80, 24), (160, 44)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|f| draw(f, &mut app)).unwrap();
            println!("=== {name} {w}x{h} ===");
            let b = terminal.backend().buffer();
            for y in 0..h {
                for x in 0..w {
                    print!("{}", b[(x, y)].symbol());
                }
                println!();
            }
        }
    }
}
