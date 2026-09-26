//! Offline review of queued/loading feedback and directional table scrolling.
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
            no_location: true,
            years: 10,
            month: 9,
            offline: true,
            refresh: false,
            city_ids: vec![],
            past_days: 2,
        },
    };
    let mut app = App::new(prefs.clone(), true, String::new());
    let mut report = service.load(&prefs, false).unwrap();
    report.cities[0].retry_at = Some(chrono::Utc::now().timestamp() + 60);
    report.cities[0].error = Some("API minute budget; remaining data is queued".into());
    app.receive(Ok(report));
    app.ui.toast.push(tapp_ui::notification::Toast::plain(
        tapp_ui::theme::ToastKind::Tip,
        "City added",
    ));
    for stage in [
        "stacked feedback",
        "waiting",
        "main help",
        "main help return",
        "main info",
        "color guide",
        "main info return",
        "scrolled right",
        "scrolled left",
        "loading details",
        "city info",
    ] {
        if stage == "waiting" {
            app.ui.toast.clear();
        }
        if stage == "main help" {
            app.handle(Event::Key(KeyEvent::new(
                KeyCode::Char('?'),
                KeyModifiers::NONE,
            )));
        }
        if stage == "main help return" {
            app.handle(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        }
        if stage == "main info" || stage == "city info" {
            app.handle(Event::Key(KeyEvent::new(
                KeyCode::Char('i'),
                KeyModifiers::NONE,
            )));
        }
        if stage == "main info return" {
            app.handle(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        }
        if stage == "color guide" {
            app.handle(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)));
        }
        if stage == "scrolled right" {
            app.handle(Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT)));
        }
        if stage == "scrolled left" {
            app.handle(Event::Key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT)));
        }
        if stage == "loading details" {
            app.loading = true;
            app.handle(Event::Key(KeyEvent::new(
                KeyCode::Enter,
                KeyModifiers::NONE,
            )));
        }
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        println!("\n{stage}");
        let buffer = terminal.backend().buffer();
        for y in 0..24 {
            println!(
                "{}",
                (0..80).map(|x| buffer[(x, y)].symbol()).collect::<String>()
            );
        }
    }
}
