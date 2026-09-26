use std::process::Command;
fn command() -> Command {
    static ROOT: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    let root = ROOT.get_or_init(|| tempfile::tempdir().unwrap());
    let mut command = Command::new(env!("CARGO_BIN_EXE_nubila"));
    for kind in ["CONFIG", "CACHE", "STATE"] {
        command.env(format!("XDG_{kind}_HOME"), root.path().join(kind));
    }
    command
}
#[test]
fn cli_period_pair_orders_chronologically_and_exports_solar_data() {
    let out = command()
        .args([
            "weather",
            "--demo",
            "--no-location",
            "--period",
            "baseline",
            "--compare-period",
            "future",
            "--city",
            "paris",
            "--pinned",
            "tokyo",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["period"], "future");
    assert_eq!(value["comparison_period"], "baseline");
    assert_eq!(value["pinned"], "tokyo");
    assert_eq!(value["cities"].as_array().unwrap().len(), 1);
    assert!(value["cities"][0]["values"]["daylight"].is_number());
    assert!(value["cities"][0]["monthly"][0]["sunrise"].is_number());
}

#[test]
fn json_cli_is_lossless_and_noninteractive() {
    let out = command()
        .args([
            "weather",
            "--demo",
            "--no-location",
            "--mode",
            "comparison",
            "--reference",
            "tokyo",
            "--city",
            "paris",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["cities"].as_array().unwrap().len(), 1);
    assert_eq!(value["cities"][0]["hourly"].as_array().unwrap().len(), 48);
    assert_eq!(value["cities"][0]["daily"].as_array().unwrap().len(), 15);
    assert_eq!(value["cities"][0]["condition"]["code"], 2);
    assert!(value["cities"][0]["daily"][0]["cloud_cover"].is_number());
    assert!(
        (value["cities"][0]["values"]["temperature_2m"]
            .as_f64()
            .unwrap()
            - 1.31804)
            .abs()
            < 1e-8
    );
}
#[test]
fn config_roundtrip_and_invalid_edit_preserves_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("cities.toml");
    let out = command()
        .args(["config", "--config"])
        .arg(&path)
        .args(["--init"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let out = command()
        .args(["config", "--config"])
        .arg(&path)
        .args([
            "--set",
            "source=blend",
            "--set",
            "reference=paris",
            "--set",
            "auto_location=false",
            "--add-city",
            r#"{"id":"kyoto","name":"京都","latitude":35.01,"longitude":135.77}"#,
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let before = std::fs::read(&path).unwrap();
    let out = command()
        .args(["config", "--config"])
        .arg(&path)
        .args(["--remove-city", "paris"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
#[test]
fn offline_failure_has_per_city_error_and_nonzero_exit() {
    let tmp = tempfile::tempdir().unwrap();
    let out = command()
        .args([
            "weather",
            "--offline",
            "--no-location",
            "--city",
            "tokyo",
            "--cache-dir",
        ])
        .arg(tmp.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        v["cities"][0]["error"]
            .as_str()
            .unwrap()
            .contains("Offline")
    );
}

#[test]
fn cli_sort_order_and_past_days_are_programmatic() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config.toml");
    std::fs::write(&config, nubila::config::EXAMPLE).unwrap();
    let out = command()
        .arg("--config")
        .arg(&config)
        .args([
            "weather",
            "--demo",
            "--no-location",
            "--sort",
            "temperature",
            "--order",
            "desc",
            "--past-days",
            "7",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let rows = report["cities"].as_array().unwrap();
    assert_eq!(rows.len(), 3); // pinning does not duplicate JSON records
    assert!(
        rows.windows(2)
            .all(|pair| pair[0]["values"]["temperature_2m"].as_f64()
                >= pair[1]["values"]["temperature_2m"].as_f64())
    );
    assert!(rows[0]["hourly"][0]["time"].as_str() < rows[0]["time"].as_str());
    let out = command()
        .args(["weather", "--demo", "--no-location", "--reference", ""])
        .output()
        .unwrap();
    assert!(out.status.success());
}

#[test]
fn shared_theme_cli_roundtrip_and_unknown_value_protection() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("theme");
    let run = |args: &[&str]| {
        command()
            .env("TAPP_UI_THEME_FILE", &path)
            .args(args)
            .output()
            .unwrap()
    };
    assert!(
        run(&["theme", "--set", "tokyo-night-omarchy"])
            .status
            .success()
    );
    let json: serde_json::Value = serde_json::from_slice(&run(&["theme"]).stdout).unwrap();
    assert_eq!(json["theme"], "tokyo-night-omarchy");
    std::fs::write(&path, "future-theme\n").unwrap();
    assert!(!run(&["theme", "--set", "terminal"]).status.success());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "future-theme\n");
}

#[test]
fn cli_supports_local_time_and_temperature_range_sorting() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config.toml");
    for criterion in ["local-time", "temperature-min", "temperature-max"] {
        for reverse in [false, true] {
            let mut cmd = command();
            cmd.arg("--config").arg(&config);
            cmd.args(["weather", "--demo", "--no-location", "--sort", criterion]);
            if reverse {
                cmd.arg("--reverse");
            }
            let out = cmd.output().unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            assert_eq!(value["cities"].as_array().unwrap().len(), 3);
        }
    }
    let out = command()
        .arg("--config")
        .arg(&config)
        .args([
            "weather",
            "--demo",
            "--no-location",
            "--format",
            "table",
            "--width",
            "160",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("00:00 UTC"));
}

#[test]
fn climate_period_and_comparison_are_independent_cli_options() {
    let output = command()
        .args([
            "weather",
            "--demo",
            "--no-location",
            "--period",
            "baseline",
            "--month",
            "7",
            "--mode",
            "comparison",
            "--reference",
            "tokyo",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["period"], "baseline");
    assert_eq!(value["mode"], "comparison");
    assert_eq!(value["units"]["precipitation"], "mm/month");
    let reference = value["cities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["city"]["id"] == "tokyo")
        .unwrap();
    assert_eq!(reference["values"]["temperature_2m"], 0.0);
    assert_eq!(reference["baseline"]["start_year"], 1950);
    assert!(reference["absolute_monthly"].as_array().unwrap().len() >= 12);
}

#[test]
fn climate_details_ignore_list_month_and_keep_twelve_calendar_months() {
    let output = command()
        .args([
            "detail",
            "tokyo",
            "--demo",
            "--no-location",
            "--period",
            "recent",
            "--month",
            "7",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let row = &value["cities"][0];
    assert_eq!(row["baseline"]["month"], 0);
    assert_eq!(row["monthly"].as_array().unwrap().len(), 12);
    assert_eq!(value["units"]["precipitation"], "mm/year");
    assert!(row["annual"].is_object());
}
