//! Reproducible warm UI measurements; excludes terminal transport and network.
use nubila::{
    config::{Config, EXAMPLE, Preferences},
    service::{Client, FetchOptions, Service},
    tui::{App, draw},
};
use ratatui::{Terminal, backend::TestBackend};
use std::time::Instant;
fn main() {
    for count in [3, 1000] {
        let mut config: Config = toml::from_str(EXAMPLE).unwrap();
        let template = config.cities[0].clone();
        for i in config.cities.len()..count {
            let mut c = template.clone();
            c.id = format!("city-{i}");
            c.name = format!("City {i:04}");
            config.cities.push(c);
        }
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
        let startup = Instant::now();
        app.report = Some(service.initial(&prefs).unwrap());
        println!(
            "offline identities and clocks cities={count}: {:.1} us",
            startup.elapsed().as_secs_f64() * 1e6
        );
        app.receive(service.load(&prefs, false));
        for (w, h) in [(80, 24), (180, 48)] {
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            let cold = Instant::now();
            term.draw(|f| draw(f, &mut app)).unwrap();
            println!(
                "main cities={count} {w}x{h} first frame: {:.1} us",
                cold.elapsed().as_secs_f64() * 1e6
            );
            let start = Instant::now();
            for _ in 0..200 {
                term.draw(|f| draw(f, &mut app)).unwrap();
            }
            println!(
                "main cities={count} {w}x{h}: {:.1} us/frame",
                start.elapsed().as_secs_f64() * 1e6 / 200.0
            );
        }
        app.focus = Some("tokyo".into());
        app.modal = Some(nubila::tui::Modal::Detail);
        let mut term = Terminal::new(TestBackend::new(180, 48)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();
        let start = Instant::now();
        for _ in 0..200 {
            term.draw(|f| draw(f, &mut app)).unwrap();
        }
        println!(
            "detail cities={count} 180x48: {:.1} us/frame",
            start.elapsed().as_secs_f64() * 1e6 / 200.0
        );
    }
    #[cfg(target_os = "linux")]
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status.lines().filter(|line| line.starts_with("VmHWM:")) {
            println!("whole benchmark {line}");
        }
    }
}
