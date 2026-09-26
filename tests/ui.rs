use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use nubila::{
    config::{Config, Preferences},
    model::Mode,
    service::{Client, FetchOptions, Service},
    tui::{Action, App, Modal, draw},
};
use ratatui::{Terminal, backend::TestBackend};
fn app() -> App {
    let prefs = Preferences {
        reference: "tokyo".into(),
        ..Default::default()
    };
    let service = Service {
        config: toml::from_str::<Config>(nubila::config::EXAMPLE).unwrap(),
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
        client: Client::new(std::env::temp_dir(), true),
    };
    let mut app = App::new(prefs.clone(), true, String::new());
    app.receive(service.load(&prefs, false));
    app
}
fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn compare_pinned(app: &mut App) -> Action {
    assert_eq!(app.handle(key(KeyCode::Char('c'))), Action::None);
    assert!(app.period_picker.is_some() && app.comparison_picker);
    app.period_picker.as_mut().unwrap().select_id(4);
    app.handle(key(KeyCode::Enter))
}
fn screen(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| draw(f, app)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
#[test]
fn renders_tiny_narrow_normal_wide_and_details() {
    let mut app = app();
    assert!(screen(&mut app, 20, 6).contains("q quits"));
    for (w, h) in [(32, 9), (55, 15), (80, 24), (110, 32), (180, 50)] {
        let output = screen(&mut app, w, h);
        assert!(!output.lines().next().unwrap().contains("NUBILA"));
        assert!(output.contains("Temp °C"));
        assert!(output.contains("London"));
        app.handle(key(KeyCode::Home));
        app.handle(key(KeyCode::Enter));
        let detail = screen(&mut app, w, h);
        if w == 80 && std::env::var_os("NUBILA_PRINT_PREVIEW").is_some() {
            println!("{detail}");
        }
        assert!(detail.lines().last().unwrap().contains("Esc"));
        app.handle(key(KeyCode::End));
        screen(&mut app, w, h);
        assert!(app.scroll <= app.max_scroll);
        app.handle(key(KeyCode::Esc));
    }
}
#[test]
fn sorting_preserves_stable_focus() {
    let mut app = app();
    app.handle(key(KeyCode::Down));
    let before = app.focus.clone();
    app.handle(key(KeyCode::Char('o')));
    assert_eq!(app.focus, before);
}
#[test]
fn unicode_filter_supports_paste_and_grapheme_deletion() {
    let mut app = app();
    app.handle(key(KeyCode::Char('/')));
    app.handle(Event::Paste("東京e\u{301}".into()));
    app.handle(key(KeyCode::Backspace));
    assert_eq!(app.prefs.filter, "東京");
    app.handle(key(KeyCode::Home));
    app.handle(key(KeyCode::Delete));
    assert_eq!(app.prefs.filter, "京");
    app.handle(Event::Key(KeyEvent::new(
        KeyCode::Char('u'),
        KeyModifiers::CONTROL,
    )));
    assert!(app.prefs.filter.is_empty());
    app.handle(Event::Paste("tok".into()));
    assert_eq!(app.visible().len(), 1);
    app.handle(key(KeyCode::Esc));
    assert!(app.prefs.filter.is_empty());
}
#[test]
fn modal_blocks_underlying_actions_and_mouse_rows() {
    let mut app = app();
    screen(&mut app, 80, 24);
    app.handle(key(KeyCode::Char('?')));
    let before = app.prefs.reference.clone();
    assert_eq!(app.handle(key(KeyCode::Char('r'))), Action::None);
    app.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 3,
        row: 5,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(app.prefs.reference, before);
    assert_eq!(app.modal, Some(Modal::Help));
}
#[test]
fn comparison_picker_orders_periods_preserves_month_and_keeps_background() {
    use nubila::model::Period;
    let mut app = app();
    app.prefs.month = 7;
    app.handle(key(KeyCode::Char('c')));
    let screen = screen(&mut app, 110, 40);
    assert!(screen.contains("Compare with"));
    assert!(screen.contains("Pinned city · Tokyo"));
    assert!(screen.contains("City"));
    app.handle(key(KeyCode::Esc));
    assert_eq!(app.prefs.mode, Mode::Normal);
    app.prefs.period = Period::Baseline;
    app.handle(key(KeyCode::Char('c')));
    app.period_picker
        .as_mut()
        .unwrap()
        .select_id(Period::Future as usize);
    assert_eq!(app.handle(key(KeyCode::Enter)), Action::Reload(false));
    assert_eq!(app.prefs.period, Period::Future);
    assert_eq!(app.prefs.comparison_period, Some(Period::Baseline));
    assert_eq!(app.prefs.month, 7);
    app.modal = Some(Modal::Detail);
    app.handle(key(KeyCode::Char('c')));
    app.period_picker
        .as_mut()
        .unwrap()
        .select_id(Period::Recent as usize);
    app.handle(key(KeyCode::Enter));
    assert_eq!(app.modal, Some(Modal::Detail));
    assert_eq!(app.prefs.period, Period::Future);
    assert_eq!(app.prefs.comparison_period, Some(Period::Recent));
    assert_eq!(app.prefs.month, 7);
}

#[test]
fn modes_and_reference_use_shared_actions() {
    let mut app = app();
    assert_eq!(compare_pinned(&mut app), Action::Reused);
    assert_eq!(app.prefs.mode, Mode::Comparison);
    app.handle(key(KeyCode::Down));
    assert_eq!(app.handle(key(KeyCode::Char('r'))), Action::Reused);
    assert_eq!(Some(&app.prefs.reference), app.focus.as_ref());
}
#[test]
fn preferences_exclude_transient_intent() {
    let mut app = app();
    app.handle(key(KeyCode::Down));
    app.handle(key(KeyCode::Enter));
    let json = serde_json::to_value(&app.prefs).unwrap();
    assert!(json.get("focus").is_none());
    assert!(json.get("modal").is_none());
}

#[test]
fn two_line_rows_show_ranges_and_mouse_targets_both_lines() {
    let mut app = app();
    let output = screen(&mut app, 80, 24);
    assert!(
        output
            .lines()
            .nth(app.table_layout.header_area(app.table_area, 1).y as usize)
            .unwrap()
            .starts_with("City")
    );
    assert!(!output.contains("City (*"));
    assert!(
        output
            .lines()
            .nth(app.table_layout.body(app.table_area, 1).y as usize + 1)
            .unwrap()
            .contains('/')
    );
    app.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 3,
        row: app.table_layout.body(app.table_area, 1).y + app.table_layout.rows.pitch() + 1,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(app.focus.as_deref(), Some("paris"));
    assert_eq!(app.modal, None);
    app.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 3,
        row: app.table_layout.body(app.table_area, 1).y + app.table_layout.rows.pitch(),
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(app.modal, Some(Modal::Detail));
}

#[test]
fn starts_unfocused_and_selection_uses_highlight_only() {
    use ratatui::style::Modifier;
    let mut app = app();
    assert_eq!(app.focus, None);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| draw(f, &mut app)).unwrap();
    assert!(
        !terminal
            .backend()
            .buffer()
            .content
            .iter()
            .any(|c| c.modifier.contains(Modifier::REVERSED))
    );
    app.handle(key(KeyCode::Enter));
    assert_eq!(app.modal, Some(Modal::Detail));
    assert_eq!(app.focus.as_deref(), Some("tokyo"));
    app.handle(key(KeyCode::Esc));
    app.handle(key(KeyCode::Esc));
    app.handle(key(KeyCode::Down));
    assert_eq!(app.focus.as_deref(), Some("london"));
    let output = screen(&mut app, 80, 24);
    assert!(!output.lines().any(|line| line.starts_with('>')));
    app.handle(key(KeyCode::Esc));
    assert_eq!(app.focus, None);
    app.handle(key(KeyCode::Up));
    assert_eq!(app.focus.as_deref(), Some("tokyo"));
}

#[test]
fn detail_windows_scroll_independently_and_cap_at_eight() {
    use nubila::detail::Section;
    let mut app = app();
    app.handle(key(KeyCode::Down));
    app.handle(key(KeyCode::Enter));
    let output = screen(&mut app, 110, 40);
    if std::env::var_os("NUBILA_PRINT_PREVIEW").is_some() {
        println!("{output}");
    }
    assert_eq!((app.detail.hour_count, app.detail.day_count), (8, 8));
    assert!(output.contains("Cloud"));
    assert!(output.contains("Temperature · °C · visible hours"));
    assert_eq!(app.detail.hour_offset, 24);
    app.handle(key(KeyCode::Down));
    assert_eq!(app.detail.hour_offset, 25);
    app.handle(key(KeyCode::Tab));
    assert_eq!(app.detail.section, Section::Days);
    app.handle(key(KeyCode::End));
    assert_eq!(app.detail.day_offset, 7);
    assert_eq!(app.detail.hour_offset, 25);
    app.handle(key(KeyCode::Home));
    app.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: app.detail.day_area.x + 1,
        row: app.detail.day_area.y + 1,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(app.detail.day_offset, 1);
    assert_eq!(app.detail.hour_offset, 25);
    app.handle(key(KeyCode::Tab));
    assert_eq!(app.detail.section, Section::Hours);
    app.handle(key(KeyCode::Char('i')));
    assert_eq!(app.detail.section, Section::Info);
    assert!(screen(&mut app, 80, 24).contains("sources"));
    app.handle(key(KeyCode::Esc));
    assert_eq!(app.modal, Some(Modal::Detail));
    for (w, h) in [(32, 9), (55, 15), (80, 24), (180, 50)] {
        screen(&mut app, w, h);
        assert!(app.detail.hour_count <= 8 && app.detail.day_count <= 8);
    }
    app.handle(key(KeyCode::Esc));
    assert_eq!(app.modal, None);
}

#[test]
fn historical_dashboard_has_clouds_but_no_forecast_symbols_and_inherits_theme() {
    use nubila::{detail::Section, service::fixture};
    use ratatui::style::Color;
    let mut app = app();
    for row in &mut app.report.as_mut().unwrap().cities {
        *row = fixture(&row.city, Mode::Historical, 10, 9);
    }
    app.handle(key(KeyCode::Down));
    app.handle(key(KeyCode::Enter));
    let output = screen(&mut app, 110, 40);
    assert!(output.contains("Cloud"));
    assert!(!output.contains('☀') && !output.contains('☂') && !output.contains('☁'));
    assert_eq!(app.detail.section, Section::Months);
    assert_eq!(app.detail.month_count, 12);
    app.handle(key(KeyCode::End));
    assert_eq!(app.detail.month_offset, 0);
    screen(&mut app, 110, 18);
    assert!(app.detail.month_count < 12);
    app.handle(key(KeyCode::End));
    assert_eq!(app.detail.month_offset, 12 - app.detail.month_count);
    let mut terminal = Terminal::new(TestBackend::new(110, 40)).unwrap();
    terminal.draw(|f| draw(f, &mut app)).unwrap();
    for cell in &terminal.backend().buffer().content {
        assert_eq!(cell.bg, Color::Reset);
        assert!(!matches!(cell.fg, Color::Rgb(..)));
    }
}

#[test]
fn sorting_pinning_and_header_clicks_keep_separate_reference() {
    use nubila::sort::Sort;
    let mut app = app();
    assert_eq!(app.prefs.sort, Sort::City);
    let output = screen(&mut app, 110, 24);
    if std::env::var_os("NUBILA_PRINT_PREVIEW").is_some() {
        println!("{output}");
    }
    assert_eq!(output.matches("Tokyo").count(), 2);
    assert!(output.find("Tokyo").unwrap() < output.find("London").unwrap());
    assert_eq!(
        app.visible()
            .iter()
            .map(|&i| app.report.as_ref().unwrap().cities[i].city.id.as_str())
            .collect::<Vec<_>>(),
        ["london", "paris", "tokyo"]
    );
    app.handle(key(KeyCode::Char('1')));
    assert_eq!(app.prefs.sort, Sort::Temperature);
    assert!(!app.prefs.reverse);
    app.handle(key(KeyCode::Char('1')));
    assert!(app.prefs.reverse);
    screen(&mut app, 110, 24);
    let hit = app
        .sort_hits
        .iter()
        .find(|(_, sort)| *sort == Sort::Wind)
        .unwrap()
        .0;
    app.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: hit.x,
        row: hit.y,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(app.prefs.sort, Sort::Wind);
    assert!(!app.prefs.reverse);
    let report = app.report.as_mut().unwrap();
    report.current_city = Some("paris".into());
    app.prefs.reference.clear();
    assert_eq!(
        app.report.as_ref().unwrap().cities[app.pinned().unwrap()]
            .city
            .id,
        "paris"
    );
    app.report.as_mut().unwrap().cities.truncate(1);
    assert!(app.pinned().is_none());
    assert_eq!(screen(&mut app, 110, 24).matches("Tokyo").count(), 1);
}

#[test]
fn icon_column_and_muted_markers_have_semantic_styles() {
    use ratatui::style::{Color, Modifier};
    let mut app = app();
    app.prefs.monochrome = false;
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| draw(f, &mut app)).unwrap();
    let buf = terminal.backend().buffer();
    let y = app.pinned_area.y;
    let icon_x = (0..80).find(|&x| buf[(x, y)].symbol() == "⛅︎").unwrap();
    let marker_x = (0..80).find(|&x| buf[(x, y)].symbol() == "*").unwrap();
    assert!(marker_x < icon_x);
    assert!(buf[(marker_x, y)].modifier.contains(Modifier::DIM));
    assert!(!buf[(0, y)].modifier.contains(Modifier::DIM));
    assert_eq!(buf[(icon_x, y)].fg, Color::Yellow);
    assert_eq!(
        buf[(icon_x, app.table_layout.header_area(app.table_area, 1).y)].symbol(),
        " "
    );
    assert_eq!(buf[(icon_x + 5, y)].fg, Color::Reset);
    app.handle(key(KeyCode::Down));
    app.handle(key(KeyCode::Enter));
    terminal.draw(|f| draw(f, &mut app)).unwrap();
    let buf = terminal.backend().buffer();
    let y = app.detail.hour_area.y + 2;
    let moon_x = (0..80).find(|&x| buf[(x, y)].symbol() == "☾").unwrap();
    assert_eq!(buf[(moon_x, y)].fg, Color::Cyan);
    assert_eq!(buf[(moon_x + 5, y)].fg, Color::Reset);
}

#[test]
fn hourly_time_travel_starts_now_and_seeks_only_at_the_edge() {
    let mut app = app();
    app.handle(key(KeyCode::Down));
    app.handle(key(KeyCode::Enter));
    screen(&mut app, 110, 40);
    assert_eq!(app.detail.hour_offset, 24);
    app.handle(key(KeyCode::Up));
    assert_eq!(app.detail.hour_offset, 23);
    app.handle(key(KeyCode::Char('t')));
    assert_eq!(app.detail.hour_offset, 24);
    app.handle(key(KeyCode::Home));
    app.demo = false;
    assert_eq!(app.handle(key(KeyCode::Up)), Action::OlderHours);
    app.seeking = true;
    assert_eq!(app.handle(key(KeyCode::Up)), Action::None);
    app.seeking = false;
    app.sought_days.insert(app.focus.clone().unwrap(), 92);
    assert_eq!(app.handle(key(KeyCode::Up)), Action::None);
    app.handle(key(KeyCode::End));
    assert_eq!(app.detail.hour_offset, 40);
    assert_eq!(app.handle(key(KeyCode::Down)), Action::None);
    assert_eq!(app.detail.hour_offset, 40);
}

#[test]
fn extending_hours_keeps_current_screen_anchored_and_preserves_data_on_failure() {
    let mut app = app();
    app.handle(key(KeyCode::Down));
    app.handle(key(KeyCode::Enter));
    screen(&mut app, 110, 40);
    app.handle(key(KeyCode::Home));
    let index = app
        .report
        .as_ref()
        .unwrap()
        .cities
        .iter()
        .position(|r| Some(&r.city.id) == app.focus.as_ref())
        .unwrap();
    let mut more = app.report.as_ref().unwrap().cities[index].clone();
    let id = more.city.id.clone();
    let mut past = more.hourly[0].clone();
    past.time = "2026-09-17T23:00".into();
    more.hourly.insert(0, past);
    let previous_values = more.values.clone();
    app.seeking = true;
    app.receive_hours(id.clone(), more.clone());
    assert!(!app.seeking);
    assert_eq!(app.detail.hour_offset, 0);
    let row = &app.report.as_ref().unwrap().cities[index];
    assert_eq!(row.hourly.len(), 49);
    assert_eq!(row.hourly[0].time, "2026-09-17T23:00");
    assert_eq!(row.values, previous_values);
    more.error = Some("unavailable".into());
    app.receive_hours(id.clone(), more);
    assert_eq!(app.report.as_ref().unwrap().cities[index].hourly.len(), 49);
    assert!(app.notice.contains("unavailable"));
    assert_eq!(app.sought_days[&id], 92);
}

#[test]
fn selection_fills_row_without_decorations_and_preserves_secondary_values() {
    use ratatui::style::{Color, Modifier};
    let mut app = app();
    app.prefs.monochrome = false;
    app.handle(key(KeyCode::Down));
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| draw(f, &mut app)).unwrap();
    let b = terminal.backend().buffer();
    let y = app.table_layout.body(app.table_area, 1).y;
    assert_eq!(b[(0, y)].symbol(), " ");
    assert_eq!(b[(1, y)].symbol(), "L");
    assert!(!b[(1, y)].modifier.contains(Modifier::UNDERLINED));
    assert!(b[(1, y)].modifier.contains(Modifier::BOLD));
    let icon = (0..80).find(|&x| b[(x, y)].symbol() == "⛅︎").unwrap();
    assert!(
        !b[(icon + 5, y)]
            .modifier
            .intersects(Modifier::DIM | Modifier::UNDERLINED)
    );
    assert!(b[(icon + 5, y + 1)].modifier.contains(Modifier::DIM));
    for row in y..y + 2 {
        for x in 0..80 {
            assert_eq!(
                b[(x, row)].bg,
                tapp_ui::theme::terminal_selection_background()
            );
            assert!(!b[(x, row)].modifier.contains(Modifier::REVERSED));
        }
    }
    assert_eq!(b[(1, y + 2)].bg, Color::Reset);
}

#[test]
fn hourly_table_and_chart_use_city_local_time() {
    let mut app = app();
    for row in &mut app.report.as_mut().unwrap().cities {
        row.range_timezone = "Asia/Tokyo".into();
        row.time = "2026-09-19T07:50".into();
    }
    app.handle(key(KeyCode::Down));
    app.handle(key(KeyCode::Enter));
    let output = screen(&mut app, 110, 40);
    assert!(output.contains("Hours · Asia/Tokyo"));
    assert!(output.contains("09-19 16:00"));
    assert!(output.contains("16:00"));
    let row = app
        .report
        .as_ref()
        .unwrap()
        .cities
        .iter()
        .find(|r| Some(&r.city.id) == app.focus.as_ref())
        .unwrap();
    assert_eq!(row.hourly[app.detail.hour_offset].time, "2026-09-19T07:00");
}

#[test]
fn hero_labels_are_muted_and_tab_never_enters_info() {
    use nubila::detail::Section;
    use ratatui::style::{Color, Modifier};
    let mut app = app();
    app.prefs.monochrome = false;
    for row in &mut app.report.as_mut().unwrap().cities {
        row.values.insert("wind_speed_10m".into(), Some(50.0));
        row.values.insert("apparent_temperature".into(), Some(32.0));
    }
    app.handle(key(KeyCode::Down));
    app.handle(key(KeyCode::Enter));
    let mut terminal = Terminal::new(TestBackend::new(110, 40)).unwrap();
    terminal.draw(|f| draw(f, &mut app)).unwrap();
    let b = terminal.backend().buffer();
    let line = (0..110).map(|x| b[(x, 1)].symbol()).collect::<String>();
    for label in ["Feels", "Wind", "km/h", "°C"] {
        // Positions are terminal cells, not UTF-8 byte offsets.
        let byte = line.find(label).unwrap();
        let x = unicode_width::UnicodeWidthStr::width(&line[..byte]) as u16;
        assert_eq!(b[(x, 1)].fg, Color::Reset);
        assert!(b[(x, 1)].modifier.contains(Modifier::DIM));
    }
    let byte = line.find("32.0").unwrap();
    let x = unicode_width::UnicodeWidthStr::width(&line[..byte]) as u16;
    assert_eq!(b[(x, 1)].fg, Color::Red);
    assert!(!b[(x, 1)].modifier.contains(Modifier::DIM));
    assert!(screen(&mut app, 110, 40).contains("Source: demo"));
    for keycode in [KeyCode::Tab, KeyCode::BackTab, KeyCode::Tab, KeyCode::Tab] {
        app.handle(key(keycode));
        assert!(matches!(app.detail.section, Section::Hours | Section::Days));
    }
    app.handle(key(KeyCode::Char('i')));
    assert_eq!(app.detail.section, Section::Info);
    app.handle(key(KeyCode::Tab));
    assert_eq!(app.detail.section, Section::Info);
    app.handle(key(KeyCode::Char('i')));
    assert_eq!(app.detail.section, Section::Hours);
}

#[test]
fn detail_city_cycle_wraps_respects_sort_filter_and_preserves_pane() {
    use nubila::detail::Section;
    let mut app = app();
    app.handle(key(KeyCode::Down));
    app.handle(key(KeyCode::Enter));
    screen(&mut app, 110, 40);
    app.handle(key(KeyCode::Tab));
    app.handle(key(KeyCode::End));
    app.handle(key(KeyCode::Char(']')));
    assert_eq!(app.focus.as_deref(), Some("paris"));
    assert_eq!(app.modal, Some(Modal::Detail));
    assert_eq!(app.detail.section, Section::Days);
    assert_eq!(app.detail.day_offset, 0);
    app.handle(key(KeyCode::Char(']')));
    assert_eq!(app.focus.as_deref(), Some("tokyo"));
    app.handle(key(KeyCode::Char(']')));
    assert_eq!(app.focus.as_deref(), Some("london"));
    app.handle(key(KeyCode::Char('[')));
    assert_eq!(app.focus.as_deref(), Some("tokyo"));
    app.prefs.reverse = true;
    app.handle(key(KeyCode::Char(']')));
    assert_eq!(app.focus.as_deref(), Some("paris"));
    app.prefs.filter = "tok".into();
    app.handle(key(KeyCode::Char(']')));
    assert_eq!(app.focus.as_deref(), Some("tokyo"));
    app.detail.day_offset = 3;
    app.handle(key(KeyCode::Char(']')));
    assert_eq!(app.detail.day_offset, 3); // one city: no reset or duplication
    app.handle(key(KeyCode::Char('i')));
    app.prefs.filter.clear();
    app.handle(key(KeyCode::Char(']')));
    assert_eq!(app.detail.section, Section::Info);
    assert_eq!(app.focus.as_deref(), Some("paris"));
}

#[test]
fn graph_switching_preserves_time_pane_and_uses_metric_units() {
    use nubila::detail::{Metric, Section};
    let mut app = app();
    app.handle(key(KeyCode::Down));
    app.handle(key(KeyCode::Enter));
    screen(&mut app, 110, 40);
    app.handle(key(KeyCode::Down));
    app.handle(key(KeyCode::Tab));
    let offset = app.detail.hour_offset;
    for (keycode, metric, title) in [
        ('6', Metric::Rain, "Rain · mm · visible days"),
        ('8', Metric::Wind, "Wind max · km/h · visible days"),
        ('9', Metric::Cloud, "Cloud · % · visible days"),
        (
            '1',
            Metric::Temperature,
            "Temperature mean · °C · visible days",
        ),
        (
            '2',
            Metric::TemperatureMin,
            "Temperature min · °C · visible days",
        ),
        (
            '3',
            Metric::TemperatureMax,
            "Temperature max · °C · visible days",
        ),
    ] {
        app.handle(key(KeyCode::Char(keycode)));
        let output = screen(&mut app, 110, 40);
        assert!(output.contains(title), "{output}");
        if std::env::var_os("NUBILA_PRINT_PREVIEW").is_some() {
            println!("{output}");
        }
        assert_eq!(app.detail.metric, metric);
        assert_eq!(app.detail.hour_offset, offset);
        assert_eq!(app.detail.section, Section::Days);
        assert_eq!(app.prefs.sort, nubila::sort::Sort::City);
        if metric == Metric::Cloud {
            assert!(output.contains("100.0"));
        }
    }
    app.handle(key(KeyCode::Right));
    assert_eq!(app.detail.metric, Metric::FeelsMin);
    app.handle(key(KeyCode::Right));
    assert_eq!(app.detail.metric, Metric::FeelsMax);
    app.handle(key(KeyCode::Right));
    assert_eq!(app.detail.metric, Metric::Rain);
    app.handle(key(KeyCode::Char(']')));
    assert_eq!(app.detail.metric, Metric::Rain);
    assert_eq!(app.detail.section, Section::Days);
    let output = screen(&mut app, 80, 24);
    assert!(output.contains("Rain · mm"));
    app.handle(key(KeyCode::Tab));
    assert_eq!(app.detail.section, Section::Graph);
    assert_eq!(app.detail.metric, Metric::Rain);
}

#[test]
fn historical_and_missing_graphs_remain_explicit() {
    use nubila::{detail::Section, service::fixture};
    let mut app = app();
    for row in &mut app.report.as_mut().unwrap().cities {
        *row = fixture(&row.city, Mode::Historical, 10, 9);
    }
    app.handle(key(KeyCode::Down));
    app.handle(key(KeyCode::Enter));
    app.handle(key(KeyCode::Char('6')));
    assert!(screen(&mut app, 110, 40).contains("Rain · mm/month · Year"));
    assert_eq!(app.detail.section, Section::Months);
    for row in &mut app.report.as_mut().unwrap().cities {
        for month in &mut row.monthly {
            month.values.insert("rain".into(), None);
        }
    }
    assert!(screen(&mut app, 110, 40).contains("no data"));
}

#[test]
fn native_graph_tabs_are_clickable_after_resize_and_arrows_only_cycle_metrics() {
    use nubila::detail::{Metric, Section};
    let mut app = app();
    app.handle(key(KeyCode::Down));
    app.handle(key(KeyCode::Enter));
    screen(&mut app, 110, 40);
    app.handle(key(KeyCode::Home));
    app.demo = false;
    assert_eq!(app.handle(key(KeyCode::Left)), Action::None);
    assert_eq!(app.detail.metric, Metric::Cloud);
    assert_eq!(app.detail.hour_offset, 0);
    app.handle(key(KeyCode::Right));
    assert_eq!(app.detail.metric, Metric::Temperature);
    app.handle(key(KeyCode::Char('g')));
    assert_eq!(app.detail.metric, Metric::Temperature);
    for (w, h) in [(110, 40), (80, 24), (32, 24)] {
        screen(&mut app, w, h);
        let hits = app.detail.graph_tabs.clone();
        assert!(!hits.is_empty() && hits.len() <= Metric::ALL.len());
        for (area, metric) in hits {
            assert!(area.right() <= w && area.width > 0);
            app.handle(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: area.x + 1,
                row: area.y,
                modifiers: KeyModifiers::NONE,
            }));
            assert_eq!(app.detail.metric, metric);
            assert!(
                app.detail
                    .graph_tabs
                    .iter()
                    .any(|(area, tab)| *tab == metric && area.width > 0 && area.right() <= w)
            );
            assert_eq!(
                app.detail.section,
                if h < 34 {
                    Section::Graph
                } else {
                    Section::Hours
                }
            );
            assert_eq!(app.detail.hour_offset, 0);
        }
    }
    screen(&mut app, 20, 6);
    assert!(app.detail.graph_tabs.is_empty());
}

#[test]
fn hero_city_is_full_intensity_and_rain_streaks_are_blue() {
    use ratatui::style::{Color, Modifier};
    let mut app = app();
    app.prefs.monochrome = false;
    for row in &mut app.report.as_mut().unwrap().cities {
        row.condition.as_mut().unwrap().code = 82;
    }
    app.handle(key(KeyCode::Down));
    app.handle(key(KeyCode::Enter));
    let mut terminal = Terminal::new(TestBackend::new(110, 40)).unwrap();
    terminal.draw(|f| draw(f, &mut app)).unwrap();
    let b = terminal.backend().buffer();
    let city_x = (0..110).find(|&x| b[(x, 0)].symbol() == "L").unwrap();
    assert!(!b[(city_x, 0)].modifier.contains(Modifier::DIM));
    assert_eq!(b[(city_x, 0)].fg, Color::Reset);
    let icon_x = (0..110).find(|&x| b[(x, 0)].symbol() == "☔︎").unwrap();
    assert_eq!(b[(icon_x, 0)].fg, Color::LightBlue);
    assert!(!b[(icon_x, 0)].modifier.contains(Modifier::DIM));
}

#[test]
fn city_search_editing_stale_results_and_persistence() {
    use nubila::cities;
    let mut app = app();
    app.handle(key(KeyCode::Char('a')));
    app.handle(Event::Paste("Kyoto".into()));
    assert_eq!(app.handle(key(KeyCode::Enter)), Action::SearchCities);
    app.city_manager.as_mut().unwrap().request_id = 1;
    app.receive_city_search(2, "Kyoto", Ok(cities::demo_search("Kyoto")));
    assert!(app.city_manager.as_ref().unwrap().results.is_empty());
    app.receive_city_search(1, "Kyoto", Ok(cities::demo_search("Kyoto")));
    assert!(screen(&mut app, 40, 12).contains("Japan"));
    assert_eq!(app.handle(key(KeyCode::Enter)), Action::AddCity);
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("cities.toml");
    app.config.write(&path).unwrap();
    app.demo = false;
    app.save_city_change(&path, false).unwrap();
    assert!(
        Config::read(&path)
            .unwrap()
            .cities
            .iter()
            .any(|c| c.id == "kyoto")
    );
    app.handle(key(KeyCode::Char('a')));
    app.city_manager
        .as_mut()
        .unwrap()
        .receive(Ok(cities::demo_search("Kyoto")));
    assert!(
        app.save_city_change(&path, false)
            .unwrap_err()
            .to_string()
            .contains("already saved")
    );
    app.handle(key(KeyCode::Esc));
    app.focus = Some("tokyo".into());
    app.handle(key(KeyCode::Char('d')));
    assert!(screen(&mut app, 60, 12).contains("pinned"));
    assert_eq!(app.handle(key(KeyCode::Enter)), Action::RemoveCity);
    app.save_city_change(&path, true).unwrap();
    let saved = Config::read(&path).unwrap();
    assert!(!saved.cities.iter().any(|c| c.id == "tokyo"));
    assert_eq!(app.prefs.reference, saved.reference);
    saved.validate().unwrap();
}

#[test]
fn city_management_demo_cancel_unicode_and_write_failure() {
    use nubila::cities;
    let mut app = app();
    app.handle(key(KeyCode::Char('a')));
    assert_eq!(app.handle(key(KeyCode::Char('q'))), Action::None);
    app.handle(Event::Paste("東京".into()));
    app.handle(key(KeyCode::Backspace));
    assert_eq!(app.city_manager.as_ref().unwrap().query(), "q東");
    app.handle(key(KeyCode::Esc));
    assert!(app.city_manager.is_none());
    app.handle(key(KeyCode::Char('a')));
    app.city_manager
        .as_mut()
        .unwrap()
        .receive(Ok(cities::demo_search("Kyoto")));
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("cities.toml");
    app.save_city_change(&path, false).unwrap();
    assert!(!path.exists());
    app.handle(key(KeyCode::Char('a')));
    app.city_manager
        .as_mut()
        .unwrap()
        .receive(Ok(cities::demo_search("Osaka")));
    app.demo = false;
    let before = app.config.cities.clone();
    assert!(app.save_city_change(tmp.path(), false).is_err());
    assert_eq!(app.config.cities, before);
    assert!(app.city_manager.is_some());
    let mut config = app.config.clone();
    config.cities.truncate(1);
    config.reference = config.cities[0].id.clone();
    let id = config.reference.clone();
    assert!(cities::remove(&mut config, &id).is_err());
    cities::remove(&mut config, "__current__").unwrap();
    assert!(!config.auto_location);
}

#[test]
fn graph_tabs_are_centered_and_status_is_only_in_footer() {
    let mut app = app();
    let output = screen(&mut app, 110, 40);
    assert!(output.lines().next().unwrap().contains("Tokyo"));
    let status = output.lines().last().unwrap();
    assert!(status.contains("normal") && status.contains("City ASC"));
    assert!(status.trim_end().ends_with("? help"));
    assert!(!output.lines().nth(38).unwrap().contains("Source:"));
    app.handle(key(KeyCode::Down));
    app.handle(key(KeyCode::Enter));
    screen(&mut app, 110, 40);
    let tabs = &app.detail.graph_tabs;
    let left = tabs.first().unwrap().0.x;
    let right = 110 - tabs.last().unwrap().0.right();
    assert!(left.abs_diff(right) <= 1);
}

#[test]
fn help_is_contextual_and_reopens_at_the_top() {
    let mut app = app();
    app.handle(key(KeyCode::F(1)));
    let list = screen(&mut app, 100, 40);
    assert!(list.contains("Filter cities"));
    assert!(!list.contains("Switch hourly"));
    app.handle(key(KeyCode::End));
    app.handle(key(KeyCode::Esc));
    app.handle(key(KeyCode::Enter));
    app.handle(key(KeyCode::F(1)));
    let details = screen(&mut app, 100, 40);
    assert!(details.contains("City details"));
    assert!(details.contains("Switch hourly"));
    assert!(!details.contains("Click header"));
    app.handle(key(KeyCode::Esc));
    app.handle(key(KeyCode::Esc));
    app.handle(key(KeyCode::Char('f')));
    app.handle(key(KeyCode::F(1)));
    let filter = screen(&mut app, 100, 40);
    assert!(filter.contains("Restore previous filter"));
    assert!(!filter.contains("Choose period"));
}

#[test]
fn essential_help_stays_visible_and_returns_to_the_active_screen() {
    let mut app = app();
    for width in [32, 40, 60, 110] {
        let output = screen(&mut app, width, 24);
        assert!(output.lines().last().unwrap().contains("? help"));
    }
    app.handle(key(KeyCode::Down));
    app.handle(key(KeyCode::Enter));
    let output = screen(&mut app, 32, 24);
    assert!(output.contains("? help"));
    app.handle(key(KeyCode::Char('?')));
    screen(&mut app, 32, 9);
    app.handle(key(KeyCode::End));
    let output = screen(&mut app, 32, 9);
    assert!(output.contains("Esc"));
    app.handle(key(KeyCode::Esc));
    assert_eq!(app.modal, Some(Modal::Detail));
    app.handle(key(KeyCode::Esc));
    app.handle(key(KeyCode::Char('a')));
    app.handle(Event::Paste("Kyoto".into()));
    assert!(
        screen(&mut app, 32, 9)
            .lines()
            .last()
            .unwrap()
            .contains("Ctrl+? help")
    );
    app.handle(key(KeyCode::F(1)));
    assert!(screen(&mut app, 32, 9).contains("Help"));
    app.handle(key(KeyCode::Esc));
    assert_eq!(app.city_manager.as_ref().unwrap().query(), "Kyoto");
    app.handle(key(KeyCode::Esc));
    app.handle(key(KeyCode::Char('/')));
    app.handle(key(KeyCode::F(1)));
    screen(&mut app, 32, 9);
    app.handle(key(KeyCode::End));
    assert!(app.ui.help.scroll > 0);
    app.handle(key(KeyCode::Esc));
    assert!(app.editing);
}

#[test]
fn city_results_use_single_table_rows_and_atmosphere_is_inside_hero() {
    let mut app = app();
    app.handle(key(KeyCode::Char('a')));
    let m = app.city_manager.as_mut().unwrap();
    m.receive(Ok(nubila::cities::demo_search("")));
    let output = screen(&mut app, 80, 24);
    assert!(!output.contains("Region / country"));
    assert!(!output.contains("35.012"));
    let area = app.city_manager.as_ref().unwrap().results_area;
    app.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: area.x + 2,
        row: area.y + 1,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(app.city_manager.as_ref().unwrap().selected(), 1);
    app.handle(key(KeyCode::Esc));
    app.handle(key(KeyCode::Down));
    app.handle(key(KeyCode::Enter));
    let output = screen(&mut app, 110, 48);
    let lines = output.lines().collect::<Vec<_>>();
    let cloud = lines.iter().position(|l| l.contains("Humidity")).unwrap();
    assert!(lines[cloud].starts_with('│') && lines[cloud].ends_with('│'));
    assert!(lines[cloud + 1].starts_with('╰'));
    assert_eq!(app.detail.hour_count, 8);
    assert_eq!(app.detail.day_count, 8);
    assert_eq!(app.detail.day_area.bottom(), 47);
}

#[test]
fn framework_palette_policy_typing_and_focus_round_trip() {
    use nubila::ui_shell::Command;
    use tapp_ui::commands::CommandId;
    let mut app = app();
    app.ui.scope.set_command_enabled(Command::Add as u64, false);
    app.handle(key(KeyCode::Char('a')));
    assert!(app.city_manager.is_none());
    assert_eq!(
        app.invoke_command(CommandId {
            owner: 1,
            action: Command::Add as u64
        }),
        Action::None
    );
    app.ui.scope.set_command_enabled(Command::Add as u64, true);
    app.handle(Event::Key(KeyEvent::new(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    )));
    assert!(app.city_manager.is_none());
    app.handle(key(KeyCode::Char('a')));
    app.handle(Event::Paste("Kyoto?".into()));
    assert_eq!(app.city_manager.as_ref().unwrap().query(), "Kyoto?");
    app.handle(Event::Key(KeyEvent::new(
        KeyCode::Char('k'),
        KeyModifiers::CONTROL,
    )));
    assert!(app.ui.palette_open);
    let output = screen(&mut app, 80, 24);
    assert!(output.contains("Search cities"));
    assert!(!output.contains("Sort by temperature"));
    app.handle(key(KeyCode::Esc));
    assert!(!app.ui.palette_open);
    assert_eq!(app.city_manager.as_ref().unwrap().query(), "Kyoto?");
    app.handle(key(KeyCode::Esc));
    assert!(app.city_manager.is_none());
    app.handle(key(KeyCode::Char('a')));
    assert_eq!(app.city_manager.as_ref().unwrap().query(), "Kyoto?");
}

#[test]
fn all_views_stay_in_offset_rects_and_caret_is_only_for_inputs() {
    use ratatui::{layout::Rect, widgets::Paragraph};
    let mut app = app();
    let mut term = Terminal::new(TestBackend::new(100, 44)).unwrap();
    let area = Rect::new(5, 3, 80, 36);
    for event in [
        None,
        Some(key(KeyCode::Enter)),
        Some(key(KeyCode::F(1))),
        Some(key(KeyCode::Esc)),
        Some(key(KeyCode::Esc)),
        Some(key(KeyCode::Char('a'))),
        Some(key(KeyCode::F(2))),
    ] {
        if let Some(event) = event {
            app.handle(event);
        }
        term.draw(|f| {
            f.render_widget(Paragraph::new("outside"), Rect::new(0, 0, 20, 1));
            nubila::tui::draw_in(f, area, &mut app);
        })
        .unwrap();
        assert_eq!(
            term.backend().cursor_visible(),
            app.city_manager.is_some() || app.ui.palette_open || app.editing,
            "only the active input owns the hardware caret"
        );
        let b = term.backend().buffer();
        assert_eq!(b[(0, 0)].symbol(), "o");
        for y in 1..44 {
            for x in 0..100 {
                if !area.contains((x, y).into()) {
                    assert_eq!(b[(x, y)].symbol(), " ", "outside {x},{y}");
                }
            }
        }
    }
    let cursor = term.get_cursor_position().unwrap();
    assert!(area.contains(cursor));
}

#[test]
fn city_search_highlights_row_and_keeps_region_muted() {
    use ratatui::style::Modifier;
    let mut app = app();
    app.prefs.monochrome = false;
    app.handle(key(KeyCode::Char('a')));
    app.city_manager
        .as_mut()
        .unwrap()
        .receive(Ok(nubila::cities::demo_search("Kyoto")));
    let mut term = Terminal::new(TestBackend::new(90, 24)).unwrap();
    term.draw(|f| draw(f, &mut app)).unwrap();
    let b = term.backend().buffer();
    let area = app.city_manager.as_ref().unwrap().results_area;
    let y = area.y;
    let start = (area.x..area.right())
        .find(|&x| b[(x, y)].symbol() == "K")
        .unwrap();
    assert!(!b[(start, y)].modifier.contains(Modifier::UNDERLINED));
    for x in area.x..area.right() {
        assert_eq!(
            b[(x, y)].bg,
            tapp_ui::theme::terminal_selection_background()
        );
    }
    assert!(!b[(start + 5, y)].modifier.contains(Modifier::UNDERLINED));
    let region = (start + 5..area.right())
        .find(|&x| b[(x, y)].symbol() == "J")
        .unwrap();
    assert!(!b[(region, y)].modifier.contains(Modifier::UNDERLINED));
    assert!(b[(region, y)].modifier.contains(Modifier::DIM));
    assert_eq!(b[(area.x, y - 1)].symbol(), "─");
}

#[test]
fn closed_search_rejects_late_results_and_palette_search_requeries() {
    use nubila::ui_shell::Command;
    use tapp_ui::commands::CommandId;
    let mut app = app();
    app.handle(key(KeyCode::Char('a')));
    app.handle(Event::Paste("Kyoto".into()));
    assert_eq!(app.handle(key(KeyCode::Enter)), Action::SearchCities);
    app.city_manager.as_mut().unwrap().request_id = 17;
    app.handle(key(KeyCode::Esc));
    app.handle(key(KeyCode::Char('a')));
    app.receive_city_search(17, "Kyoto", Ok(nubila::cities::demo_search("Kyoto")));
    assert!(app.city_manager.as_ref().unwrap().results.is_empty());
    assert!(!app.city_manager.as_ref().unwrap().search.loading);
    let manager = app.city_manager.as_mut().unwrap();
    manager.request_id = 18;
    app.receive_city_search(18, "Kyoto", Ok(nubila::cities::demo_search("Kyoto")));
    assert!(!app.city_manager.as_ref().unwrap().results.is_empty());
    assert_eq!(
        app.invoke_command(CommandId {
            owner: 1,
            action: Command::CitySearch as u64
        }),
        Action::SearchCities
    );
    assert!(app.city_manager.as_ref().unwrap().results.is_empty());
}

#[test]
fn control_help_preserves_background_input_and_caret_ownership() {
    let mut app = app();
    let mut term = Terminal::new(TestBackend::new(110, 40)).unwrap();
    term.draw(|f| draw(f, &mut app)).unwrap();
    let corner = term.backend().buffer()[(1, 0)].clone();
    app.handle(key(KeyCode::Char('a')));
    app.handle(Event::Paste("Kyoto".into()));
    term.draw(|f| draw(f, &mut app)).unwrap();
    assert_eq!(term.backend().buffer()[(1, 0)], corner);
    assert!(term.backend().cursor_visible());
    app.handle(key(KeyCode::Char('?')));
    assert_eq!(app.city_manager.as_ref().unwrap().query(), "Kyoto?");
    for modifiers in [
        KeyModifiers::CONTROL,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ] {
        app.handle(Event::Key(KeyEvent::new(KeyCode::Char('?'), modifiers)));
        assert_eq!(app.modal, Some(Modal::Help));
        term.draw(|f| draw(f, &mut app)).unwrap();
        assert_eq!(term.backend().buffer()[(1, 0)], corner);
        assert!(!term.backend().cursor_visible());
        app.handle(key(KeyCode::Esc));
        term.draw(|f| draw(f, &mut app)).unwrap();
        assert!(term.backend().cursor_visible());
        assert_eq!(app.city_manager.as_ref().unwrap().query(), "Kyoto?");
    }
    app.handle(key(KeyCode::Esc));
    app.handle(key(KeyCode::Char('i')));
    term.draw(|f| draw(f, &mut app)).unwrap();
    assert!(app.ui.info_open);
    assert_eq!(term.backend().buffer()[(1, 0)], corner);
    assert!(!term.backend().cursor_visible());
    app.handle(key(KeyCode::Esc));
    app.handle(key(KeyCode::F(2)));
    app.handle(Event::Key(KeyEvent::new(
        KeyCode::Char('?'),
        KeyModifiers::CONTROL,
    )));
    term.draw(|f| draw(f, &mut app)).unwrap();
    assert!(!term.backend().cursor_visible());
    app.handle(key(KeyCode::Esc));
    // Fast queued input after dismissal must work before the next redraw.
    app.handle(Event::Paste("theme".into()));
    assert_eq!(app.ui.palette.palette.query(), "theme");
    app.handle(key(KeyCode::Esc));
    app.handle(key(KeyCode::Char('?')));
    assert_eq!(app.modal, Some(Modal::Help));
    app.handle(key(KeyCode::F(2)));
    term.draw(|f| draw(f, &mut app)).unwrap();
    assert!(term.backend().cursor_visible());
    app.handle(key(KeyCode::Esc));
    assert_eq!(app.modal, Some(Modal::Help));
    term.draw(|f| draw(f, &mut app)).unwrap();
    assert!(!term.backend().cursor_visible());
}

#[test]
fn short_detail_cycle_reaches_all_graphs_and_scrolls_time() {
    use nubila::detail::{Metric, Section};
    let mut app = app();
    app.handle(key(KeyCode::Enter));
    for (width, height) in [(32, 9), (32, 24), (80, 15), (80, 24), (110, 32)] {
        app.detail = Default::default();
        let output = screen(&mut app, width, height);
        assert!(output.contains("Tables"));
        assert_eq!(app.detail.section, Section::Hours);
        app.handle(key(KeyCode::Tab));
        assert_eq!(app.detail.section, Section::Days);
        app.handle(key(KeyCode::BackTab));
        assert_eq!(app.detail.section, Section::Hours);
        app.handle(key(KeyCode::Tab));
        let start = app.detail.hour_offset;
        for metric in Metric::ALL {
            app.handle(key(KeyCode::Right));
            let output = screen(&mut app, width, height);
            assert!(output.contains("Tables"));
            assert_eq!(app.detail.section, Section::Graph);
            assert_eq!(app.detail.metric, metric);
            app.handle(key(KeyCode::Tab));
            app.handle(key(KeyCode::BackTab));
            assert_eq!(app.detail.section, Section::Graph);
            assert_eq!(app.detail.metric, metric);
            assert!(!app.detail.graph_area.is_empty());
            assert!(
                app.detail
                    .graph_tabs
                    .iter()
                    .any(|(area, tab)| *tab == metric && area.width > 0 && area.right() <= width)
            );
        }
        app.handle(key(KeyCode::Down));
        assert_eq!(app.detail.day_offset, 1);
        assert_eq!(app.detail.hour_offset, start);
        app.handle(key(KeyCode::Up));
        assert_eq!(app.detail.day_offset, 0);
        assert_eq!(app.detail.hour_offset, start);
        app.handle(key(KeyCode::Right));
        assert_eq!(
            app.detail.section,
            Section::Days,
            "Tables restores its focused list"
        );
        for metric in Metric::ALL.into_iter().rev() {
            app.handle(key(KeyCode::Left));
            assert_eq!(app.detail.section, Section::Graph);
            assert_eq!(app.detail.metric, metric);
        }
        app.handle(key(KeyCode::Left));
        assert_eq!(app.detail.section, Section::Days);
        app.handle(key(KeyCode::Right));
        screen(&mut app, width, height);
        let hit = app.detail.tables_tab;
        assert!(hit.width > 0);
        app.handle(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: hit.x,
            row: hit.y,
            modifiers: KeyModifiers::NONE,
        }));
        assert_eq!(app.detail.section, Section::Days);
        let output = screen(&mut app, width, height);
        let bar = output.lines().last().unwrap();
        assert!(bar.trim_end().ends_with("? help"));
        assert!(bar.find("↑↓").unwrap() < bar.find("Tab").unwrap());
        assert!(bar.find("Tab").unwrap() < bar.find("←→").unwrap());
        assert!(bar.find("←→").unwrap() < bar.find("Esc").unwrap());
    }
}

#[test]
fn medium_hero_fits_metrics_on_one_line_and_narrow_keeps_two() {
    let mut app = app();
    app.handle(key(KeyCode::Enter));
    let wide = screen(&mut app, 100, 24);
    let metrics = wide.lines().nth(1).unwrap();
    for text in ["Feels", "Wind", "Rain", "/"] {
        assert!(metrics.contains(text), "{metrics}");
    }
    let narrow = screen(&mut app, 55, 24);
    assert!(narrow.lines().nth(1).unwrap().contains("Feels"));
    assert!(!narrow.lines().nth(1).unwrap().contains("Rain"));
    assert!(narrow.lines().nth(2).unwrap().contains("Rain"));
    app.handle(key(KeyCode::Char('i')));
    let info = screen(&mut app, 100, 24);
    assert!(info.lines().next().unwrap().contains("Tokyo"));
    app.handle(key(KeyCode::Esc));
    app.handle(key(KeyCode::Char('?')));
    let help = screen(&mut app, 100, 24);
    assert!(help.lines().next().unwrap().contains("Tokyo"));
    app.handle(key(KeyCode::Esc));
    app.handle(key(KeyCode::Char('2')));
    screen(&mut app, 100, 24);
    app.handle(key(KeyCode::Char('i')));
    screen(&mut app, 100, 24);
    let hit = app.detail.graph_tabs[0].0;
    app.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: hit.x,
        row: hit.y,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(app.detail.section, nubila::detail::Section::Info);
}

#[test]
fn live_filter_modal_keeps_background_and_status_visible() {
    let mut app = app();
    for (width, height) in [(32, 9), (80, 24)] {
        app.prefs.filter.clear();
        app.editing = false;
        app.handle(key(KeyCode::Char('f')));
        app.handle(Event::Paste("Tok".into()));
        let mut term = Terminal::new(TestBackend::new(width, height)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();
        let buffer = term.backend().buffer();
        let row = |y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        };
        let output = (0..height).map(row).collect::<Vec<_>>().join("\n");
        assert!(output.contains("Filter cities · live"));
        assert!(output.contains("Tok"));
        assert!(!output.contains("Filter: "));
        assert!(row(app.table_area.y).contains('─'));
        if height > 9 {
            assert!(row(app.table_layout.header_area(app.table_area, 1).y).contains("City"));
        }
        assert!(row(height - 1).contains("normal"));
        assert!(row(height - 1).contains("help"));
        assert!(!row(height - 1).contains("Tok"));
        assert!(term.backend().cursor_position().y < height - 2);
        assert_eq!(app.table_area.bottom(), height - 1);
        app.handle(key(KeyCode::Enter));
        screen(&mut app, width, height);
        assert_eq!(app.table_area.bottom(), height - 1);
        assert_eq!(app.prefs.filter, "Tok");
    }
}

#[test]
fn mode_and_reference_projection_reuses_forecasts_without_drift() {
    let mut app = app();
    let original = serde_json::to_value(app.report.as_ref().unwrap()).unwrap();
    let hourly_ptr = app.report.as_ref().unwrap().cities[0].hourly.as_ptr();
    for _ in 0..40 {
        assert_eq!(compare_pinned(&mut app), Action::Reused);
        let report = app.report.as_ref().unwrap();
        assert_eq!(report.units["cloud_cover"], "percentage points");
        assert_eq!(report.cities[0].hourly.as_ptr(), hourly_ptr);
        let reference = report
            .cities
            .iter()
            .find(|r| r.city.id == report.reference)
            .unwrap();
        assert_eq!(reference.values["temperature_2m"], Some(0.0));
        assert_eq!(app.handle(key(KeyCode::Char('n'))), Action::Reused);
        assert_eq!(
            serde_json::to_value(app.report.as_ref().unwrap()).unwrap(),
            original
        );
    }
    assert_eq!(app.handle(key(KeyCode::Char('n'))), Action::None);
    app.focus = Some("paris".into());
    assert_eq!(app.handle(key(KeyCode::Char('r'))), Action::Reused);
    assert_eq!(compare_pinned(&mut app), Action::Reused);
    let report = app.report.as_ref().unwrap();
    assert_eq!(
        report
            .cities
            .iter()
            .find(|r| r.city.id == "paris")
            .unwrap()
            .values["temperature_2m"],
        Some(0.0)
    );
    assert!(!app.loading);
}

#[test]
fn period_picker_preserves_session_month_and_cancels_without_changes() {
    use nubila::model::Period;
    let mut app = app();
    app.prefs.month = 7;
    app.handle(key(KeyCode::Char('p')));
    assert!(screen(&mut app, 100, 30).contains("Period"));
    app.handle(key(KeyCode::Down));
    assert_eq!(app.handle(key(KeyCode::Enter)), Action::Reload(false));
    assert_eq!(app.prefs.period, Period::Baseline);
    assert_eq!(app.prefs.month, 7);
    app.handle(key(KeyCode::Char('p')));
    screen(&mut app, 100, 30);
    app.handle(key(KeyCode::Down));
    app.handle(key(KeyCode::Esc));
    assert_eq!(app.prefs.period, Period::Baseline);
    let saved = serde_json::to_vec(&app.session_preferences()).unwrap();
    let restored: Preferences = serde_json::from_slice(&saved).unwrap();
    assert_eq!(restored.month, 0);
    assert_eq!(restored.period, Period::Baseline);
}

#[test]
fn unavailable_reference_requests_data_and_enters_comparison() {
    let mut app = app();
    let id = app.prefs.reference.clone();
    app.report
        .as_mut()
        .unwrap()
        .cities
        .iter_mut()
        .find(|r| r.city.id == id)
        .unwrap()
        .error = Some("unavailable".into());
    assert_eq!(compare_pinned(&mut app), Action::Reload(false));
    assert_eq!(app.prefs.mode, Mode::Comparison);
    assert!(app.notice.contains("Pinned city weather unavailable"));
    assert!(
        app.report
            .as_ref()
            .unwrap()
            .cities
            .iter()
            .all(|r| r.absolute_values.is_none())
    );
}

#[test]
fn city_clock_is_local_responsive_and_sortable_with_temperature_extrema() {
    use nubila::{sort::Sort, ui_shell::Command};
    let mut app = app();
    for row in &mut app.report.as_mut().unwrap().cities {
        row.range_timezone = match row.city.id.as_str() {
            "tokyo" => "Asia/Tokyo",
            "paris" => "Europe/Paris",
            _ => "Europe/London",
        }
        .into();
        row.time = "2026-07-01T07:00".into();
    }
    let wide = screen(&mut app, 160, 40);
    assert!(wide.contains("16:00 JST"));
    assert!(wide.contains("09:00 CEST"));
    assert!(screen(&mut app, 110, 24).contains("16:00 JST"));
    screen(&mut app, 160, 40);
    app.handle(key(KeyCode::Char('t')));
    assert_eq!(app.prefs.sort, Sort::LocalTime);
    assert_eq!(
        app.report.as_ref().unwrap().cities[app.visible()[0]]
            .city
            .id,
        "london"
    );
    for (command, base) in [
        (Command::SortTemp, Sort::Temperature),
        (Command::SortFeels, Sort::Feels),
        (Command::SortRain, Sort::Rain),
        (Command::SortSnow, Sort::Snow),
        (Command::SortWind, Sort::Wind),
        (Command::SortHumidity, Sort::Humidity),
        (Command::SortPressure, Sort::Pressure),
        (Command::SortCloud, Sort::Cloud),
    ] {
        let [_, min, max] = base.family().unwrap();
        for expected in [
            (base, false),
            (base, true),
            (min, false),
            (max, true),
            (base, false),
        ] {
            app.invoke_command(tapp_ui::commands::CommandId {
                owner: 1,
                action: command as u64,
            });
            assert_eq!((app.prefs.sort, app.prefs.reverse), expected);
        }
        app.handle(key(KeyCode::Char('0')));
        for expected in [
            (base, false),
            (base, true),
            (min, false),
            (max, true),
            (base, false),
        ] {
            app.handle(key(command.key().unwrap()));
            assert_eq!((app.prefs.sort, app.prefs.reverse), expected);
        }
        screen(&mut app, 160, 40);
        let rect = app
            .sort_hits
            .iter()
            .find(|(_, sort)| *sort == base)
            .unwrap()
            .0;
        for expected in [(base, true), (min, false), (max, true), (base, false)] {
            app.handle(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: rect.x,
                row: rect.y,
                modifiers: KeyModifiers::NONE,
            }));
            assert_eq!((app.prefs.sort, app.prefs.reverse), expected);
        }
    }
    app.focus = Some("tokyo".into());
    app.handle(key(KeyCode::Enter));
    for (width, height) in [(32, 9), (55, 24), (110, 40)] {
        let output = screen(&mut app, width, height);
        let title = output.lines().next().unwrap();
        assert!(title.contains("16:00"), "{title}");
        if width >= 55 {
            assert!(title.contains("JST"));
        }
    }
}

#[test]
fn pending_weather_keeps_cities_clocks_and_blanks_measurements() {
    let mut app = app();
    for row in &mut app.report.as_mut().unwrap().cities {
        let city = row.city.clone();
        *row = nubila::model::Weather::empty(city);
        row.range_timezone = "Asia/Tokyo".into();
    }
    app.loading = true;
    let text = screen(&mut app, 110, 24);
    assert!(text.contains("Tokyo") && text.contains("Paris") && text.contains("London"));
    assert!(text.contains("JST") && text.chars().any(|c| "⠛⠹⢸⣰⣤⣆⡇⠏".contains(c)));
    assert!(!text.contains("—"));
    app.receive(Err(anyhow::anyhow!("Unavailable")));
    let text = screen(&mut app, 110, 24);
    assert!(text.contains("Tokyo"));
    assert!(!text.contains("Loading"));
    assert_eq!(compare_pinned(&mut app), Action::Reload(false));
}

#[test]
fn main_row_separators_disable_on_short_resize_and_ignore_clicks() {
    let mut app = app();
    screen(&mut app, 80, 30);
    assert!(app.table_layout.rows.separator.is_some());
    app.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: app.table_area.x,
        row: app.table_layout.header_area(app.table_area, 1).y + 1,
        modifiers: KeyModifiers::NONE,
    }));
    assert!(app.focus.is_none());
    app.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: app.table_area.x,
        row: app.table_layout.body(app.table_area, 1).y + 2,
        modifiers: KeyModifiers::NONE,
    }));
    assert!(app.focus.is_none());
    screen(&mut app, 80, 18);
    assert!(app.table_layout.rows.separator.is_none());
    app.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: app.table_area.x,
        row: app.table_layout.body(app.table_area, 1).y + 2,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(app.focus.as_deref(), Some("paris"));
}

#[test]
fn main_selection_wraps_in_both_directions_and_respects_filter() {
    let mut app = app();
    app.handle(key(KeyCode::Up));
    assert_eq!(app.focus.as_deref(), Some("tokyo"));
    app.handle(key(KeyCode::Down));
    assert_eq!(app.focus.as_deref(), Some("london"));
    app.handle(key(KeyCode::Up));
    assert_eq!(app.focus.as_deref(), Some("tokyo"));
    app.handle(key(KeyCode::Home));
    app.handle(key(KeyCode::Char('k')));
    assert_eq!(app.focus.as_deref(), Some("tokyo"));
    app.handle(key(KeyCode::Char('j')));
    assert_eq!(app.focus.as_deref(), Some("london"));
    app.prefs.filter = "Paris".into();
    app.normalize();
    for code in [KeyCode::Down, KeyCode::Down, KeyCode::Up] {
        app.handle(key(code));
        assert_eq!(app.focus.as_deref(), Some("paris"));
    }
    app.prefs.filter = "no such city".into();
    app.normalize();
    app.handle(key(KeyCode::Up));
    app.handle(key(KeyCode::Down));
    assert!(app.focus.is_none());
}

#[test]
fn filter_alias_is_text_inside_input_and_cancels_live_query() {
    let mut app = app();
    for event in [
        key(KeyCode::Char('f')),
        key(KeyCode::Char('/')),
        Event::Key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL)),
    ] {
        app.handle(event);
        assert!(app.editing);
        app.handle(key(KeyCode::Char('f')));
        assert!(app.prefs.filter.ends_with('f'));
        app.handle(Event::Key(KeyEvent::new(
            KeyCode::Char('u'),
            KeyModifiers::CONTROL,
        )));
        app.handle(Event::Paste("Tok".into()));
        assert_eq!(app.visible().len(), 1);
        let focus = app.focus.clone();
        app.handle(key(KeyCode::Down));
        assert_eq!(app.focus, focus);
        app.handle(Event::Key(KeyEvent::new(
            KeyCode::Char('?'),
            KeyModifiers::CONTROL,
        )));
        assert_eq!(app.modal, Some(Modal::Help));
        app.handle(key(KeyCode::Esc));
        assert!(app.editing);
        app.handle(key(KeyCode::Esc));
        assert!(!app.editing);
        assert!(app.prefs.filter.is_empty());
    }
}

#[test]
fn filter_preview_commit_and_cancel_restore_the_last_applied_query() {
    let mut app = app();
    app.handle(key(KeyCode::Char('f')));
    app.handle(Event::Paste("Paris".into()));
    assert_eq!(app.ui.filter.committed(), "");
    app.handle(key(KeyCode::Enter));
    assert_eq!(app.ui.filter.committed(), "Paris");
    assert_eq!(app.visible().len(), 1);
    app.handle(key(KeyCode::Char('f')));
    app.handle(Event::Key(KeyEvent::new(
        KeyCode::Char('u'),
        KeyModifiers::CONTROL,
    )));
    assert_eq!(app.visible().len(), 3);
    assert_eq!(app.ui.filter.committed(), "Paris");
    app.handle(key(KeyCode::Esc));
    assert_eq!(app.prefs.filter, "Paris");
    assert_eq!(app.visible().len(), 1);
    app.handle(key(KeyCode::Char('f')));
    app.handle(Event::Key(KeyEvent::new(
        KeyCode::Char('u'),
        KeyModifiers::CONTROL,
    )));
    app.handle(key(KeyCode::Enter));
    assert_eq!(app.prefs.filter, "");
    assert_eq!(app.ui.filter.committed(), "");
    assert_eq!(app.visible().len(), 3);
}

#[test]
fn streaming_rows_keep_loading_and_reproject_when_reference_arrives() {
    let mut app = app();
    let ready = app.report.as_ref().unwrap().cities.clone();
    for row in &mut app.report.as_mut().unwrap().cities {
        *row = nubila::model::Weather::empty(row.city.clone());
    }
    app.loading = true;
    let paris = ready.iter().find(|r| r.city.id == "paris").unwrap().clone();
    let tokyo = ready.iter().find(|r| r.city.id == "tokyo").unwrap().clone();
    let expected =
        paris.values["temperature_2m"].unwrap() - tokyo.values["temperature_2m"].unwrap();
    app.receive_partial(paris);
    assert!(app.loading);
    assert!(
        screen(&mut app, 110, 24)
            .chars()
            .any(|c| "⠛⠹⢸⣰⣤⣆⡇⠏".contains(c))
    );
    assert!(
        app.report
            .as_ref()
            .unwrap()
            .cities
            .iter()
            .find(|r| r.city.id == "london")
            .unwrap()
            .time
            .is_empty()
    );
    assert_eq!(compare_pinned(&mut app), Action::Reused);
    assert!(
        app.loading,
        "Presentation changes must not cancel the unfinished batch"
    );
    app.receive_partial(tokyo);
    let row = app
        .report
        .as_ref()
        .unwrap()
        .cities
        .iter()
        .find(|r| r.city.id == "paris")
        .unwrap();
    assert_eq!(row.values["temperature_2m"], Some(expected));
    assert!(row.absolute_values.is_some());
    let output = screen(&mut app, 110, 24);
    assert_eq!(app.table_area.y, app.pinned_area.bottom());
    assert!(
        output
            .lines()
            .nth(app.table_area.y as usize)
            .unwrap()
            .contains('─')
    );
    assert!(
        output
            .lines()
            .nth(app.table_layout.header_area(app.table_area, 1).y as usize + 1)
            .unwrap()
            .contains('─')
    );
}

#[test]
fn pending_detail_starts_at_current_hour_when_its_data_arrives() {
    let mut app = app();
    let ready = app
        .report
        .as_ref()
        .unwrap()
        .cities
        .iter()
        .find(|r| r.city.id == "tokyo")
        .unwrap()
        .clone();
    let expected = nubila::detail::State::current_hour(&ready);
    let row = app
        .report
        .as_mut()
        .unwrap()
        .cities
        .iter_mut()
        .find(|r| r.city.id == "tokyo")
        .unwrap();
    *row = nubila::model::Weather::empty(row.city.clone());
    app.loading = true;
    app.focus = Some("tokyo".into());
    app.handle(key(KeyCode::Enter));
    screen(&mut app, 110, 40);
    app.receive_partial(ready);
    screen(&mut app, 110, 40);
    assert_eq!(app.detail.hour_offset, expected);
}

#[test]
fn main_table_uses_shared_cyan_header_top_border_and_no_column_rules() {
    use ratatui::{
        style::{Color, Modifier},
        widgets::Borders,
    };
    let mut app = app();
    for theme in [
        tapp_ui::theme::Theme::Terminal,
        tapp_ui::theme::Theme::TokyoNightOmarchy,
    ] {
        app.ui.theme = theme;
        for width in [32, 80, 160] {
            let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
            terminal.draw(|f| draw(f, &mut app)).unwrap();
            let buffer = terminal.backend().buffer();
            let header = app.table_layout.header_area(app.table_area, 1);
            assert_eq!(app.table_layout.borders, Borders::TOP);
            assert!(!app.table_layout.columns);
            assert_eq!(header.y, app.table_area.y + 1);
            assert_eq!(
                buffer[(header.x, header.y)].fg,
                theme.color(Color::Cyan, false)
            );
            for x in app.table_area.x..app.table_area.right() {
                assert_eq!(buffer[(x, app.table_area.y)].symbol(), "─");
                for y in header.y..app.table_area.bottom() {
                    assert_ne!(buffer[(x, y)].symbol(), "│");
                }
            }
            assert_eq!(app.sort_hits[0].0.y, header.y);
        }
    }
    app.prefs.monochrome = true;
    let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
    terminal.draw(|f| draw(f, &mut app)).unwrap();
    let header = app.table_layout.header_area(app.table_area, 1);
    let cell = &terminal.backend().buffer()[(header.x, header.y)];
    assert_eq!(cell.fg, Color::Reset);
    assert!(cell.modifier.contains(Modifier::BOLD));
}

#[test]
fn restores_details_even_when_saved_filter_hides_city_and_falls_back_if_removed() {
    let mut app = app();
    app.prefs.last_city = Some("tokyo".into());
    app.prefs.filter = "Paris".into();
    app.restore_screen();
    assert_eq!(app.modal, Some(Modal::Detail));
    assert!(screen(&mut app, 100, 30).contains("Tokyo"));
    assert_eq!(
        app.session_preferences().last_city.as_deref(),
        Some("tokyo")
    );
    app.handle(key(KeyCode::Char('?')));
    assert_eq!(
        app.session_preferences().last_city.as_deref(),
        Some("tokyo")
    );
    app.handle(key(KeyCode::Esc));
    app.handle(key(KeyCode::Char('l')));
    assert_eq!(app.modal, None);
    assert_eq!(app.session_preferences().last_city, None);
    app.prefs.last_city = Some("removed-city".into());
    app.restore_screen();
    assert_eq!(app.modal, None);
    assert_eq!(app.focus, None);
    assert_eq!(app.prefs.last_city, None);
}

#[test]
fn restored_details_show_placeholders_then_data_without_losing_focus() {
    for mode in [Mode::Normal, Mode::Historical] {
        let mut app = app();
        let row = app
            .report
            .as_ref()
            .unwrap()
            .cities
            .iter()
            .find(|r| r.city.id == "tokyo")
            .unwrap()
            .clone();
        let mut pending = nubila::model::Weather::empty(row.city.clone());
        pending.range_timezone = "Asia/Tokyo".into();
        app.report.as_mut().unwrap().cities = vec![pending];
        app.prefs.mode = mode;
        app.prefs.last_city = Some("tokyo".into());
        app.report.as_mut().unwrap().mode = mode;
        app.loading = true;
        app.restore_screen();
        for (w, h) in [(32, 9), (55, 15), (100, 30), (180, 50)] {
            let output = screen(&mut app, w, h);
            assert!(output.contains("Tokyo"), "{output}");
            assert!(output.chars().any(|c| "⠛⠹⢸⣰⣤⣆⡇⠏".contains(c)), "{output}");
            assert!(output.contains('—'), "{output}");
            assert!(!output.contains("No forecast available"));
        }
        let loaded = nubila::service::fixture(&row.city, mode, 10, 9);
        app.receive_partial(loaded);
        let output = screen(&mut app, 100, 30);
        assert!(output.contains("Tokyo"));
        assert!(!output.contains("· loading"));
        assert_eq!(app.modal, Some(Modal::Detail));
        app.loading = false;
        assert!(!screen(&mut app, 100, 30).contains("Loading"));
    }
}

#[test]
fn list_command_is_available_only_in_details() {
    let mut app = app();
    let command = tapp_ui::commands::CommandId {
        owner: 1,
        action: nubila::ui_shell::Command::List as u64,
    };
    app.handle(key(KeyCode::Enter));
    app.refresh_commands();
    assert!(app.ui.catalog.eligible(command));
    app.invoke_command(command);
    assert_eq!(app.modal, None);
    app.refresh_commands();
    assert!(!app.ui.catalog.eligible(command));
}

#[test]
fn retry_and_force_refresh_are_available_on_list_and_details() {
    let mut app = app();
    for details in [false, true] {
        if details {
            app.handle(key(KeyCode::Enter));
        }
        assert!(matches!(
            app.handle(key(KeyCode::Char('R'))),
            Action::Reload(false)
        ));
        assert!(matches!(
            app.handle(key(KeyCode::Char('u'))),
            Action::Reload(true)
        ));
        assert_eq!(app.modal == Some(Modal::Detail), details);
    }
}

#[test]
fn climate_details_show_twelve_months_and_comparison_preserves_list_month() {
    use nubila::model::Period;
    let mut app = app();
    let svc = Service {
        config: app.config.clone(),
        options: FetchOptions {
            offline: true,
            refresh: false,
            demo: true,
            no_location: true,
            years: 5,
            month: 1,
            city_ids: vec![],
            past_days: 2,
        },
        client: Client::new(std::env::temp_dir().join("nubila-ui-unused"), true),
    };
    app.prefs.period = Period::Baseline;
    app.prefs.month = 7;
    let report = svc.load(&app.prefs, false).unwrap();
    app.receive(Ok(report));
    assert!(screen(&mut app, 110, 40).contains("1950~1969 · Jul"));
    app.handle(key(KeyCode::Enter));
    let details = screen(&mut app, 110, 40);
    assert!(details.contains("/ 12"));
    assert!(!details.contains("/ 13"));
    app.handle(key(KeyCode::Right));
    assert_eq!(
        app.prefs.month, 7,
        "detail arrows change graph, not list month"
    );
    app.handle(key(KeyCode::Char(']')));
    assert_eq!(app.prefs.month, 7);
    compare_pinned(&mut app);
    assert_eq!(app.prefs.mode, Mode::Comparison);
    let row = app
        .report
        .as_ref()
        .unwrap()
        .cities
        .iter()
        .find(|r| r.city.id == app.prefs.reference)
        .unwrap();
    assert!(
        row.monthly
            .iter()
            .all(|m| m.values["temperature_2m"] == Some(0.))
    );
    for (w, h) in [(32, 9), (55, 18), (100, 35), (160, 45)] {
        let text = screen(&mut app, w, h);
        assert!(text.contains("help") || text.contains('?'));
    }
    app.handle(key(KeyCode::Char('l')));
    assert_eq!(app.prefs.month, 7);
}

#[test]
fn quota_wait_is_visible_in_list_and_details_with_retry_toast() {
    let mut app = app();
    let mut report = app.report.clone().unwrap();
    let row = report
        .cities
        .iter_mut()
        .find(|r| r.city.id == app.prefs.reference)
        .unwrap();
    row.retry_at = Some(chrono::Utc::now().timestamp() + 120);
    row.error = Some("API quota reached; retry scheduled".into());
    app.receive(Ok(report));
    assert!(app.notice.contains("retry automatically"));
    assert!(!app.ui.toast.is_empty());
    assert!(screen(&mut app, 110, 40).contains("Quota wait"));
    app.ui.toast.clear();
    app.handle(key(KeyCode::Enter));
    let output = screen(&mut app, 110, 40);
    assert!(output.contains(tapp_ui::loading::WAITING_SYMBOL));
    assert!(output.contains("Next days"));
    assert!(!output.contains("details / sources"));
    assert_ne!(app.detail.section, nubila::detail::Section::Info);
    app.handle(key(KeyCode::Char('i')));
    let output = screen(&mut app, 110, 40);
    assert!(output.contains("details / sources"));
    assert!(
        output.contains("Next days"),
        "Info must retain the dashboard behind it"
    );
    app.handle(key(KeyCode::Esc));
    assert_eq!(app.modal, Some(Modal::Detail));
    assert_ne!(app.detail.section, nubila::detail::Section::Info);
}

#[test]
fn horizontal_columns_preserve_navigation_and_reveal_hidden_data() {
    use nubila::detail::{Metric, Section};
    let mut app = app();
    app.handle(key(KeyCode::Down));
    let focus = app.focus.clone();
    let month = app.prefs.month;
    let sideways = |code| Event::Key(KeyEvent::new(code, KeyModifiers::ALT));
    screen(&mut app, 32, 24);
    for _ in 0..40 {
        app.handle(sideways(KeyCode::Right));
    }
    assert!(screen(&mut app, 32, 24).contains("hPa"));
    assert_eq!(app.focus, focus);
    assert_eq!(app.prefs.month, month);
    for _ in 0..40 {
        app.handle(sideways(KeyCode::Left));
    }
    assert!(screen(&mut app, 32, 24).contains("Temp °C"));
    app.handle(key(KeyCode::Enter));
    screen(&mut app, 32, 24);
    let hour = app.detail.hour_offset;
    for _ in 0..3 {
        app.handle(sideways(KeyCode::Right));
    }
    assert!(screen(&mut app, 32, 24).contains("Snow cm"));
    assert_eq!(app.detail.hour_offset, hour);
    assert_eq!(app.detail.metric, Metric::Temperature);
    assert_eq!(app.detail.section, Section::Hours);
    app.handle(key(KeyCode::Tab));
    for _ in 0..9 {
        app.handle(sideways(KeyCode::Right));
    }
    assert!(screen(&mut app, 32, 24).contains("Daylight"));
    assert_eq!(app.detail.section, Section::Days);
    app.handle(key(KeyCode::Right));
    assert_eq!(app.detail.section, Section::Graph);
    app.handle(sideways(KeyCode::Right));
    assert_eq!(app.detail.metric, Metric::Temperature);
}

#[test]
fn climate_columns_and_daylight_graph_remain_available_in_narrow_windows() {
    use nubila::detail::{Metric, Section};
    let mut app = app();
    for row in &mut app.report.as_mut().unwrap().cities {
        *row = nubila::service::fixture(&row.city, Mode::Historical, 10, 9);
        nubila::solar::enrich(row);
    }
    app.handle(key(KeyCode::Enter));
    screen(&mut app, 32, 24);
    for _ in 0..9 {
        app.handle(Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT)));
    }
    assert!(screen(&mut app, 32, 24).contains("Daylight"));
    assert_eq!(app.detail.section, Section::Months);
    app.handle(key(KeyCode::Char('0')));
    let output = screen(&mut app, 80, 24);
    assert!(output.contains("Daylight · h/day · Year"));
    assert!(!output.contains("no data"));
    assert_eq!(app.detail.metric, Metric::Daylight);
}

#[test]
fn narrow_tab_arrows_allow_mouse_navigation_to_every_graph() {
    let mut app = app();
    app.handle(key(KeyCode::Enter));
    app.handle(key(KeyCode::Char('1')));
    let metrics = app.detail.metrics().to_vec();
    for expected in metrics.iter().copied().skip(1) {
        screen(&mut app, 32, 24);
        let area = if app.detail.next_tab.is_empty() {
            app.detail
                .graph_tabs
                .iter()
                .find(|(_, metric)| *metric == expected)
                .unwrap()
                .0
        } else {
            app.detail.next_tab
        };
        app.handle(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x,
            row: area.y,
            modifiers: KeyModifiers::NONE,
        }));
        assert_eq!(app.detail.metric, expected);
    }
    for expected in metrics.into_iter().rev().skip(1) {
        screen(&mut app, 32, 24);
        let area = if app.detail.previous_tab.is_empty() {
            app.detail
                .graph_tabs
                .iter()
                .find(|(_, metric)| *metric == expected)
                .unwrap()
                .0
        } else {
            app.detail.previous_tab
        };
        app.handle(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x,
            row: area.y,
            modifiers: KeyModifiers::NONE,
        }));
        assert_eq!(app.detail.metric, expected);
    }
}

#[test]
fn sun_column_keeps_independent_sorts_at_every_width() {
    use nubila::sort::Sort;
    let mut app = app();
    let output = screen(&mut app, 160, 30);
    let header = output
        .lines()
        .find(|l| l.contains("Temp °C") && l.contains("Rain mm"))
        .unwrap();
    let names = [
        "Temp °C",
        "Feels °C",
        "Rain mm",
        "Snow cm",
        "Wind km/h",
        "RH %",
        "Cloud %",
        "Sun",
        "hPa",
    ];
    for pair in names.windows(2) {
        assert!(
            header.find(pair[0]).unwrap() < header.find(pair[1]).unwrap(),
            "{header}"
        );
    }
    assert!(!output.contains('↓'));
    let hit = app.sun_header;
    assert!(!hit.is_empty());
    app.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: hit.x,
        row: hit.y,
        modifiers: KeyModifiers::NONE,
    }));
    assert!(app.solar_sort_picker);
    assert!(screen(&mut app, 160, 30).contains("Sort Sun"));
    app.period_picker.as_mut().unwrap().select_id(3);
    assert_eq!(app.handle(key(KeyCode::Enter)), Action::None);
    assert_eq!(app.prefs.sort, Sort::Sunset);
    assert!(app.prefs.reverse);
    assert!(!app.solar_sort_picker);
    screen(&mut app, 32, 24);
    let mut saw_sun = false;
    for _ in 0..40 {
        app.handle(Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT)));
        saw_sun |= screen(&mut app, 32, 24).contains("Sunset");
    }
    assert!(saw_sun);
    assert_eq!(
        nubila::view::main_columns(32, Mode::Normal, 0),
        nubila::view::main_columns(160, Mode::Normal, 0)
    );
}

#[test]
fn main_mouse_horizontal_scrolling_preserves_city_and_period() {
    let mut app = app();
    screen(&mut app, 80, 24);
    app.handle(key(KeyCode::Down));
    let focus = app.focus.clone();
    let period = app.prefs.period;
    let month = app.prefs.month;
    for (kind, modifiers, direction) in [
        (MouseEventKind::ScrollRight, KeyModifiers::NONE, 1),
        (MouseEventKind::ScrollDown, KeyModifiers::SHIFT, 1),
        (MouseEventKind::ScrollLeft, KeyModifiers::NONE, -1),
        (MouseEventKind::ScrollUp, KeyModifiers::SHIFT, -1),
    ] {
        let expected = app.column_geometry.advance(app.column_offset, direction);
        app.handle(Event::Mouse(MouseEvent {
            kind,
            column: 10,
            row: 5,
            modifiers,
        }));
        assert_eq!(app.column_offset, expected);
        assert_eq!(app.focus, focus);
        assert_eq!(app.prefs.period, period);
        assert_eq!(app.prefs.month, month);
    }
}

#[test]
fn horizontal_viewports_stop_at_content_edge_and_reset_when_widened() {
    let mut app = app();
    let right = || Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT));
    screen(&mut app, 80, 24);
    for _ in 0..40 {
        app.handle(right());
    }
    assert_eq!(app.column_offset, app.column_max);
    let end = screen(&mut app, 80, 24);
    app.handle(right());
    assert_eq!(screen(&mut app, 80, 24), end);
    screen(&mut app, 200, 40);
    assert_eq!(app.column_offset, 0);
    assert_eq!(app.column_max, 0);
    app.handle(key(KeyCode::Enter));
    screen(&mut app, 80, 24);
    for _ in 0..40 {
        app.handle(right());
    }
    assert_eq!(app.detail.hour_column, app.detail.hour_column_max);
    let end = screen(&mut app, 80, 24);
    app.handle(right());
    assert_eq!(screen(&mut app, 80, 24), end);
    app.handle(key(KeyCode::Tab));
    screen(&mut app, 80, 24);
    for _ in 0..40 {
        app.handle(right());
    }
    assert_eq!(app.detail.day_column, app.detail.day_column_max);
    let end = screen(&mut app, 80, 24);
    app.handle(right());
    assert_eq!(screen(&mut app, 80, 24), end);
    screen(&mut app, 200, 40);
    assert_eq!(app.detail.hour_column, 0);
    assert_eq!(app.detail.day_column, 0);
}

#[test]
fn graphs_follow_table_focus_and_keep_it_across_city_changes() {
    use nubila::detail::{Metric, Section};
    let mut app = app();
    app.handle(key(KeyCode::Enter));
    let hourly = screen(&mut app, 100, 40);
    assert!(hourly.contains("Temperature · °C · visible hours"));
    assert!(!app.detail.metrics().contains(&Metric::Daylight));
    assert!(!app.detail.metrics().contains(&Metric::TemperatureMin));
    app.handle(key(KeyCode::Char('3')));
    assert!(screen(&mut app, 100, 40).contains("Rain · mm · visible hours"));
    app.handle(key(KeyCode::Tab));
    assert!(screen(&mut app, 100, 40).contains("Rain · mm · visible days"));
    app.handle(key(KeyCode::Char('0')));
    assert!(screen(&mut app, 80, 24).contains("Daylight · h/day · visible days"));
    assert_eq!(app.detail.section, Section::Graph);
    app.handle(key(KeyCode::Char(']')));
    assert!(screen(&mut app, 80, 24).contains("Daylight · h/day · visible days"));
    screen(&mut app, 100, 40);
    app.handle(key(KeyCode::Tab));
    assert!(screen(&mut app, 100, 40).contains("Temperature · °C · visible hours"));
    assert_eq!(app.detail.metric, Metric::Temperature);
}

#[test]
fn scrolled_sun_header_hit_and_monthly_right_edge_stay_aligned() {
    let mut app = app();
    let right = || Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT));
    screen(&mut app, 100, 24);
    for _ in 0..30 {
        app.handle(right());
    }
    screen(&mut app, 100, 24);
    let hit = app.sun_header;
    assert!(!hit.is_empty() && hit.right() <= 100);
    app.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: hit.x,
        row: hit.y,
        modifiers: KeyModifiers::NONE,
    }));
    assert!(app.solar_sort_picker);
    app.handle(key(KeyCode::Esc));
    for row in &mut app.report.as_mut().unwrap().cities {
        *row = nubila::service::fixture(&row.city, Mode::Historical, 10, 9);
    }
    app.handle(key(KeyCode::Enter));
    screen(&mut app, 80, 24);
    for _ in 0..30 {
        app.handle(right());
    }
    assert_eq!(app.detail.month_column, app.detail.month_column_max);
    let end = screen(&mut app, 80, 24);
    app.handle(right());
    assert_eq!(end, screen(&mut app, 80, 24));
    screen(&mut app, 200, 40);
    assert_eq!(app.detail.month_column, 0);
}

#[test]
fn city_time_and_weather_columns_stay_fixed_at_every_horizontal_step() {
    for mode in [Mode::Normal, Mode::Historical] {
        let mut app = app();
        app.prefs.mode = mode;
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let before = terminal.backend().buffer().clone();
        let fixed = app
            .sort_hits
            .iter()
            .find(|(_, sort)| *sort == nubila::sort::Sort::Temperature)
            .map(|(r, _)| r.x);
        let fixed = fixed.unwrap();
        let max = app.column_max;
        assert!(max > 0);
        for step in 1..=max + 2 {
            app.handle(Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT)));
            terminal.draw(|f| draw(f, &mut app)).unwrap();
            assert!(app.column_offset <= max);
            for y in 0..23 {
                for x in 0..fixed {
                    assert_eq!(
                        terminal.backend().buffer()[(x, y)],
                        before[(x, y)],
                        "mode {mode:?}, step {step}, cell {x},{y}"
                    );
                }
            }
            assert!(
                app.sort_hits
                    .iter()
                    .skip(if mode == Mode::Normal { 2 } else { 1 })
                    .all(|(r, _)| r.width == 0 || r.x >= fixed)
            );
        }
    }
}

#[test]
fn activity_is_always_the_leftmost_cell_even_before_climate_period() {
    for period in [nubila::model::Period::Now, nubila::model::Period::Baseline] {
        let mut app = app();
        app.prefs.period = period;
        app.loading = true;
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        assert!("⠛⠹⢸⣰⣤⣆⡇⠏".contains(terminal.backend().buffer()[(0, 29)].symbol()));
        app.loading = false;
        for row in &mut app.report.as_mut().unwrap().cities {
            row.retry_at = Some(chrono::Utc::now().timestamp() + 120);
        }
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        assert_eq!(
            terminal.backend().buffer()[(0, 29)].symbol(),
            tapp_ui::loading::WAITING_SYMBOL
        );
    }
}

#[test]
fn city_info_has_sections_and_retains_sources_values_and_errors() {
    let mut app = app();
    let row = &mut app.report.as_mut().unwrap().cities[0];
    row.error = Some("Partial data · waiting for quota".into());
    let info = nubila::view::overview(row).join("\n");
    for text in [
        "Location & period",
        "Queue & notices",
        "Daylight",
        "Sources & freshness",
        "Measurements",
        "Temperature:",
        "Partial data · waiting for quota",
    ] {
        assert!(info.contains(text), "missing {text}");
    }
    assert!(row.sources.iter().all(|source| info.contains(source)));
}

#[test]
fn main_sort_shortcuts_follow_columns_and_sun_chooser() {
    use nubila::sort::Sort;
    let mut app = app();
    for (shortcut, expected) in [
        ('1', Sort::Temperature),
        ('2', Sort::Feels),
        ('3', Sort::Rain),
        ('4', Sort::Snow),
        ('5', Sort::Wind),
        ('6', Sort::Humidity),
        ('7', Sort::Cloud),
        ('9', Sort::Pressure),
        ('t', Sort::LocalTime),
        ('w', Sort::Weather),
        ('0', Sort::City),
    ] {
        assert_eq!(app.handle(key(KeyCode::Char(shortcut))), Action::None);
        assert_eq!(app.prefs.sort, expected);
        assert!(!app.prefs.reverse);
    }
    app.handle(key(KeyCode::Char('8')));
    assert!(app.solar_sort_picker);
    assert!(screen(&mut app, 80, 24).contains("Daylight"));
    app.handle(key(KeyCode::Enter));
    assert_eq!(app.prefs.sort, Sort::Daylight);
    app.handle(key(KeyCode::Char('f')));
    app.handle(key(KeyCode::Char('4')));
    assert_eq!(
        app.prefs.sort,
        Sort::Daylight,
        "input must not trigger sorting"
    );
    app.handle(key(KeyCode::Esc));
    assert_eq!(app.prefs.sort, Sort::Daylight);
}

#[test]
fn color_guide_preserves_semantic_colors_and_monochrome() {
    use ratatui::style::Color;
    use tapp_ui::chrome::Entry;
    let entries = nubila::theme::Theme::new(false).color_guide();
    let colors: Vec<_> = entries
        .iter()
        .filter_map(|entry| match entry {
            Entry::Rich(line) => Some(line.spans.iter().filter_map(|s| s.style.fg)),
            _ => None,
        })
        .flatten()
        .collect();
    for color in [
        Color::Blue,
        Color::Cyan,
        Color::Yellow,
        Color::Red,
        Color::Magenta,
    ] {
        assert!(colors.contains(&color));
    }
    for entry in nubila::theme::Theme::new(true).color_guide() {
        if let Entry::Rich(line) = entry {
            assert!(line.spans.iter().all(|s| s.style.fg.is_none()));
        }
    }
}

#[test]
fn graph_commands_refresh_when_table_focus_changes() {
    use nubila::{detail::Section, ui_shell::Command};
    use tapp_ui::commands::CommandId;
    let mut app = app();
    app.handle(key(KeyCode::Enter));
    let daylight = CommandId {
        owner: 1,
        action: Command::DaylightGraph as u64,
    };
    app.detail.section = Section::Hours;
    app.refresh_commands();
    assert!(!app.ui.catalog.eligible(daylight));
    app.detail.section = Section::Days;
    app.refresh_commands();
    assert!(app.ui.catalog.eligible(daylight));
    app.detail.section = Section::Hours;
    app.refresh_commands();
    assert!(!app.ui.catalog.eligible(daylight));
    for (index, command) in Command::ALL.iter().enumerate() {
        assert_eq!(
            *command as usize, index,
            "command IDs must dispatch the advertised action"
        );
    }
}

#[test]
fn feels_like_tables_and_graph_follow_hourly_and_daily_focus() {
    use nubila::detail::{Metric, Section};
    let mut app = app();
    app.handle(key(KeyCode::Enter));
    let output = screen(&mut app, 180, 48);
    assert!(output.matches("Feels °C").count() >= 2);
    app.handle(key(KeyCode::Char('2')));
    assert_eq!(app.detail.metric, Metric::Feels);
    assert!(screen(&mut app, 180, 48).contains("Feels like · °C"));
    app.detail.section = Section::Days;
    assert!(screen(&mut app, 180, 48).contains("Feels min · °C"));
    app.detail.section = Section::Hours;
    app.detail.metric = Metric::Temperature;
    app.handle(key(KeyCode::Right));
    assert_eq!(app.detail.metric, Metric::Feels);
    for (width, height) in [(32, 12), (80, 24), (180, 48)] {
        let output = screen(&mut app, width, height);
        assert!(output.contains("Feels"));
    }
}

#[test]
fn graph_numbers_match_display_order_and_command_palette_in_each_focus() {
    use nubila::{detail::Section, ui_shell::Command};
    let mut app = app();
    app.handle(key(KeyCode::Enter));
    for section in [Section::Hours, Section::Days] {
        app.detail.section = section;
        screen(&mut app, 180, 48);
        for (index, metric) in app.detail.metrics().to_vec().into_iter().enumerate() {
            let shortcut = char::from(b'0' + ((index + 1) % 10) as u8);
            app.handle(key(KeyCode::Char(shortcut)));
            assert_eq!(app.detail.metric, metric);
            let command = Command::ALL
                .iter()
                .find(|c| c.metric() == Some(metric))
                .unwrap();
            app.invoke_command(tapp_ui::commands::CommandId {
                owner: 1,
                action: *command as u64,
            });
            assert_eq!(app.detail.metric, metric);
            assert_eq!(app.detail.metric_shortcut(metric), Some(shortcut));
        }
    }
}
