//! Offline installation/update checks. Only temporary files and a Rust curl stub.
#![cfg(unix)]
use std::{fs, path::Path, process::Command};

fn run(program: &str, args: &[&str]) {
    assert!(Command::new(program).args(args).status().unwrap().success());
}

#[test]
fn install_update_and_failed_checksum_preserve_existing_binary() {
    let root = std::env::temp_dir().join(format!("nubila-installer-test-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let fixtures = root.join("fixtures");
    let bin = root.join("bin");
    let destination = root.join("installed/bin");
    fs::create_dir(&fixtures).unwrap();
    fs::create_dir(&bin).unwrap();
    let stub = root.join("curl.rs");
    fs::write(&stub, r#"
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let out = &args[args.iter().position(|a| a == "-o").unwrap()+1];
    let url = args.iter().find(|a| a.starts_with("https://")).unwrap();
    let path = std::path::Path::new(&std::env::var("FIXTURES").unwrap()).join(url.rsplit('/').next().unwrap());
    std::fs::copy(path, out).unwrap();
}
"#).unwrap();
    run(
        "rustc",
        &[
            stub.to_str().unwrap(),
            "-o",
            bin.join("curl").to_str().unwrap(),
        ],
    );
    let machine = Command::new("uname").arg("-m").output().unwrap();
    let arch = match std::str::from_utf8(&machine.stdout).unwrap().trim() {
        "aarch64" | "arm64" => "aarch64",
        "x86_64" => "x86_64",
        other => panic!("unsupported test host {other}"),
    };
    let os = if cfg!(target_os = "macos") {
        "apple-darwin"
    } else {
        "unknown-linux-gnu"
    };
    let archive = format!("nubila-0.1.0-{arch}-{os}.tar.gz");
    let stage = root.join("stage");
    fs::create_dir(&stage).unwrap();
    for notice in [
        "LICENSE",
        "LICENSE-tapp-ui",
        "THIRD_PARTY.html",
        "LICENSE-timezone-data",
        "TIMEZONE-DATA.txt",
        "RELEASE.txt",
    ] {
        fs::write(stage.join(notice), "fixture notice").unwrap();
    }
    let install = |success: bool| {
        let result = Command::new("sh")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../install.sh"))
            .env(
                "PATH",
                format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
            )
            .env("FIXTURES", &fixtures)
            .env("NUBILA_INSTALL_DIR", &destination)
            .output()
            .unwrap();
        assert_eq!(
            result.status.success(),
            success,
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    };
    for contents in ["first binary", "updated binary"] {
        fs::write(stage.join("nubila"), contents).unwrap();
        run(
            "tar",
            &[
                "-czf",
                fixtures.join(&archive).to_str().unwrap(),
                "-C",
                stage.to_str().unwrap(),
                ".",
            ],
        );
        let sum = Command::new("shasum")
            .args(["-a", "256"])
            .arg(fixtures.join(&archive))
            .output()
            .unwrap();
        assert!(sum.status.success());
        let sum = String::from_utf8(sum.stdout).unwrap();
        fs::write(
            fixtures.join("SHA256SUMS"),
            format!("{}  {archive}\n", sum.split_whitespace().next().unwrap()),
        )
        .unwrap();
        install(true);
        assert_eq!(
            fs::read_to_string(destination.join("nubila")).unwrap(),
            contents
        );
    }
    fs::write(
        fixtures.join("SHA256SUMS"),
        format!("{}  {archive}\n", "0".repeat(64)),
    )
    .unwrap();
    install(false);
    assert_eq!(
        fs::read_to_string(destination.join("nubila")).unwrap(),
        "updated binary"
    );
    assert!(root.join("installed/share/doc/nubila/LICENSE").is_file());
    fs::remove_dir_all(root).unwrap();
}
