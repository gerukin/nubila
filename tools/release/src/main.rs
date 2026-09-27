//! Local-only publishing. No runtime dependency and no implicit build hook.
use std::{env, error::Error, fs, path::Path, process::Command, time::SystemTime};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const REPO: &str = "gerukin/nubila";
const TARGETS: [&str; 6] = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc",
    "aarch64-pc-windows-msvc",
];

fn run(program: &str, args: &[&str]) -> Result<()> {
    eprintln!("+ {program} {}", args.join(" "));
    if !Command::new(program).args(args).status()?.success() {
        return Err(format!("{program} failed; nothing further was published").into());
    }
    Ok(())
}

fn capture(program: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(program).args(args).output()?;
    if !out.status.success() {
        return Err(format!("{program}: {}", String::from_utf8_lossy(&out.stderr)).into());
    }
    Ok(String::from_utf8(out.stdout)?.trim().to_owned())
}

fn verify_binary_version(binary: &Path, version: &str) -> Result<()> {
    let output = Command::new(binary).arg("--version").output()?;
    let expected = format!("nubila {version}");
    let actual = String::from_utf8(output.stdout)?.trim().to_owned();
    if !output.status.success() || actual != expected {
        return Err(format!(
            "{} reported {actual:?}, expected {expected:?}",
            binary.display()
        )
        .into());
    }
    Ok(())
}

fn refresh_local(version: &str) -> Result<()> {
    if env::consts::OS != "linux" {
        return Err("Run the local development install on Linux".into());
    }
    run(
        "cargo",
        &["build", "--release", "--locked", "--bin", "nubila"],
    )?;
    let binary = env::current_dir()?.join("target/release/nubila");
    verify_binary_version(&binary, version)?;
    let home = env::var_os("HOME").ok_or("HOME is not set")?;
    let install_dir = Path::new(&home).join(".local/bin");
    fs::create_dir_all(&install_dir)?;
    let link = install_dir.join("nubila");
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_nanos();
    let pending = install_dir.join(format!(".nubila-link-{}-{nonce}", std::process::id()));
    std::os::unix::fs::symlink(&binary, &pending)?;
    if let Err(error) = fs::rename(&pending, &link) {
        let _ = fs::remove_file(&pending);
        return Err(error.into());
    }
    if fs::read_link(&link)? != binary {
        return Err("Installed Nubila link points to the wrong binary".into());
    }
    verify_binary_version(&link, version)?;
    println!("Installed {} -> {}", link.display(), binary.display());
    Ok(())
}

fn version(manifest: &str) -> Result<&str> {
    let value = manifest
        .lines()
        .find_map(|line| line.strip_prefix("version = \"")?.strip_suffix('"'))
        .ok_or("Missing package version")?;
    let parts: Vec<_> = value.split('.').collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|c| c.is_ascii_digit()))
    {
        return Err("Use a numeric major.minor.patch release version".into());
    }
    Ok(value)
}

fn notes(changelog: &str, version: &str) -> Result<String> {
    let heading = format!("## [{version}] - ");
    let start = changelog
        .find(&heading)
        .ok_or("Date the release section in CHANGELOG.md and commit it first")?;
    let text = &changelog[start..];
    let end = text.find("\n## ").unwrap_or(text.len());
    Ok(text[..end].trim().to_owned())
}

fn archive_name(version: &str, target: &str) -> String {
    format!(
        "nubila-{version}-{target}.{}",
        if target.contains("windows") {
            "zip"
        } else {
            "tar.gz"
        }
    )
}

fn clean_commit(path: &str) -> Result<String> {
    if !capture("git", &["-C", path, "status", "--porcelain"])?.is_empty() {
        return Err(format!("Commit pending changes in {path} before publishing").into());
    }
    capture("git", &["-C", path, "rev-parse", "HEAD"])
}

fn formula(version: &str, hashes: &[String]) -> String {
    let mut text = format!(
        "class Nubila < Formula\n  desc \"Fast weather TUI and structured CLI\"\n  homepage \"https://github.com/{REPO}\"\n  version \"{version}\"\n  license \"MIT\"\n"
    );
    for (os, first) in [("linux", 0), ("macos", 2)] {
        text.push_str(&format!("\n  on_{os} do\n"));
        for (arch, index) in [("intel", first), ("arm", first + 1)] {
            text.push_str(&format!("    on_{arch} do\n      url \"https://github.com/{REPO}/releases/download/v{version}/{}\"\n      sha256 \"{}\"\n    end\n", archive_name(version, TARGETS[index]), hashes[index]));
        }
        text.push_str("  end\n");
    }
    text.push_str("\n  def install\n    bin.install \"nubila\"\n    doc.install \"LICENSE\", \"LICENSE-tapp-ui\", \"THIRD_PARTY.html\", \"RELEASE.txt\", \"LICENSE-timezone-data\", \"TIMEZONE-DATA.txt\"\n  end\n\n  test do\n    assert_match version.to_s, shell_output(\"#{bin}/nubila --version\")\n  end\nend\n");
    text
}

fn cask(version: &str, hashes: &[String]) -> String {
    format!(
        r#"cask "nubila" do
  version "{version}"
  arch arm: "aarch64", intel: "x86_64"
  sha256 arm: "{arm}", intel: "{intel}"
  url "https://github.com/{REPO}/releases/download/v#{{version}}/nubila-#{{version}}-#{{arch}}-apple-darwin.tar.gz"
  name "Nubila"
  desc "Fast weather TUI and structured CLI"
  homepage "https://github.com/{REPO}"
  depends_on macos: :big_sur
  binary "nubila"
end
"#,
        arm = hashes[3],
        intel = hashes[2]
    )
}

fn target_status(target: &str) -> &'static str {
    match target {
        "x86_64-unknown-linux-gnu" => {
            "Tested on the developer's Linux machine; other distributions untested."
        }
        _ => "UNTESTED: cross-compiled; not run on this architecture.",
    }
}

fn prepare(version: &str) -> Result<()> {
    if env::consts::OS != "linux" {
        return Err("Run the publishing tool on the Linux development machine".into());
    }
    let commit = clean_commit(".")?;
    let framework = clean_commit("../tapp-ui")?;
    let release_notes = notes(&fs::read_to_string("CHANGELOG.md")?, version)?;
    // Check all tools before any expensive builds or remote mutations.
    for (program, args) in [
        ("cargo", vec!["zigbuild", "--help"]),
        ("cargo", vec!["xwin", "--version"]),
        ("cargo", vec!["about", "--version"]),
        ("zig", vec!["version"]),
        ("tar", vec!["--version"]),
        ("zip", vec!["-v"]),
        ("sha256sum", vec!["--version"]),
    ] {
        capture(program, &args)?;
    }
    if !env::var_os("SDKROOT").is_some_and(|p| Path::new(&p).is_dir()) {
        return Err("Set SDKROOT to your legally obtained macOS SDK before publishing".into());
    }
    let dist = Path::new("dist").join(version);
    fs::create_dir_all(&dist)?;
    let license_output = Command::new("cargo")
        .args(["about", "generate", "packaging/licenses.hbs"])
        .output()?;
    if !license_output.status.success() {
        return Err(format!(
            "License bundle generation failed: {}",
            String::from_utf8_lossy(&license_output.stderr)
        )
        .into());
    }
    fs::write(dist.join("THIRD_PARTY.html"), license_output.stdout)?;
    let mut hashes = Vec::new();
    let mut checksums = String::new();
    for target in TARGETS {
        let windows = target.contains("windows");
        let build_target = if target.contains("linux") {
            format!("{target}.2.28")
        } else {
            target.to_owned()
        };
        let mut build = Command::new("cargo");
        let windows_arm = windows && target.starts_with("aarch64");
        build.arg(if windows && !windows_arm {
            "xwin"
        } else if target.contains("apple") || windows_arm {
            "build"
        } else {
            "zigbuild"
        });
        if windows && !windows_arm {
            build.arg("build");
        }
        let target_dir = if windows {
            "target/publish-windows-static"
        } else {
            "target/publish"
        };
        build.args([
            "--release",
            "--locked",
            "--bin",
            "nubila",
            "--target",
            &build_target,
            "--target-dir",
            target_dir,
        ]);
        if windows {
            let key = format!(
                "CARGO_TARGET_{}_RUSTFLAGS",
                target.replace('-', "_").to_uppercase()
            );
            let flags = env::var(&key).unwrap_or_default();
            build.env(key, format!("{flags} -C target-feature=+crt-static"));
            // ring insists on GNU-style Clang for ARM64 Windows. Ask xwin to
            // supply matching include flags, not clang-cl's /imsvc arguments.
            if target.starts_with("aarch64") {
                let setup = capture("cargo", &["xwin", "env", "--target", target])?;
                for line in setup.lines() {
                    let Some(export) = line.strip_prefix("export ") else {
                        continue;
                    };
                    let Some((name, value)) = export.split_once("=\"") else {
                        continue;
                    };
                    let value = value
                        .strip_suffix("\";")
                        .ok_or("Unexpected xwin environment format")?;
                    if name == "RUSTFLAGS" && value.is_empty() {
                        build.env_remove(name);
                        continue;
                    }
                    let value = if name.starts_with("CFLAGS_") {
                        format!(
                            "{} -fms-runtime-lib=static",
                            value.replace("/imsvc", "-isystem")
                        )
                    } else if name.starts_with("CC_") {
                        "clang".into()
                    } else if name.ends_with("_RUSTFLAGS") {
                        format!("{value} -C target-feature=+crt-static")
                    } else {
                        value.to_owned()
                    };
                    build.env(name, value);
                }
            }
        }
        if target.contains("apple") {
            build.env("MACOSX_DEPLOYMENT_TARGET", "11.0");
            build.env("CC_SHELL_ESCAPED_FLAGS", "1");
            let sdk = env::var("SDKROOT")?;
            let host = capture("rustc", &["-vV"])?;
            let host = host
                .lines()
                .find_map(|l| l.strip_prefix("host: "))
                .ok_or("Missing Rust host")?;
            let sysroot = capture("rustc", &["--print", "sysroot"])?;
            let linker = format!("{sysroot}/lib/rustlib/{host}/bin/gcc-ld/ld64.lld");
            let arch = if target.starts_with("aarch64") {
                "arm64"
            } else {
                "x86_64"
            };
            let clang_target = format!("{arch}-apple-macos11.0");
            let mut flags = env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default();
            for flag in [
                "-C".into(),
                "linker=clang".into(),
                "-C".into(),
                format!("link-arg=--target={clang_target}"),
                "-C".into(),
                "link-arg=-isysroot".into(),
                "-C".into(),
                format!("link-arg={sdk}"),
                "-C".into(),
                format!("link-arg=-fuse-ld={linker}"),
                "-C".into(),
                "link-arg=-Wl,-platform_version,macos,11.0,15.4".into(),
                "-C".into(),
                "link-arg=-Wl,-adhoc_codesign".into(),
            ] {
                if !flags.is_empty() {
                    flags.push('\x1f');
                }
                flags.push_str(&flag);
            }
            build.env("CARGO_ENCODED_RUSTFLAGS", flags);
            build.env(format!("CC_{}", target.replace('-', "_")), "clang");
            build.env(
                format!("CFLAGS_{}", target.replace('-', "_")),
                format!("--target={clang_target} -isysroot '{sdk}' -mmacosx-version-min=11.0"),
            );
        }
        eprintln!("Building {target}");
        if !build.status()?.success() {
            return Err(format!("Build failed for {target}; nothing pushed").into());
        }
        let stage = dist.join(format!("stage-{target}"));
        if stage.exists() {
            fs::remove_dir_all(&stage)?;
        }
        fs::create_dir(&stage)?;
        let binary = if windows { "nubila.exe" } else { "nubila" };
        fs::copy(
            Path::new(target_dir)
                .join(target)
                .join("release")
                .join(binary),
            stage.join(binary),
        )?;
        let native = target == format!("{}-unknown-linux-gnu", env::consts::ARCH);
        if native {
            verify_binary_version(&stage.join(binary), version)?;
        }
        fs::copy("LICENSE", stage.join("LICENSE"))?;
        fs::copy(
            dist.join("THIRD_PARTY.html"),
            stage.join("THIRD_PARTY.html"),
        )?;
        fs::copy("../tapp-ui/LICENSE", stage.join("LICENSE-tapp-ui"))?;
        fs::copy(
            "packaging/LICENSE-timezone-data",
            stage.join("LICENSE-timezone-data"),
        )?;
        fs::copy(
            "packaging/TIMEZONE-DATA.txt",
            stage.join("TIMEZONE-DATA.txt"),
        )?;
        let status = target_status(target);
        fs::write(
            stage.join("RELEASE.txt"),
            format!(
                "Nubila {version}\n{target}\n{status}\nNubila commit: {commit}\ntapp-ui commit: {framework}\nLinux GNU builds target glibc 2.28 or newer; macOS deployment target 11.0.\n"
            ),
        )?;
        let name = archive_name(version, target);
        let archive = env::current_dir()?.join(&dist).join(&name);
        if archive.exists() {
            fs::remove_file(&archive)?;
        }
        let archive_path = archive.to_str().ok_or("Non-UTF8 release path")?;
        if windows {
            if !Command::new("zip")
                .current_dir(&stage)
                .args(["-q", "-r", archive_path, "."])
                .status()?
                .success()
            {
                return Err("zip failed".into());
            }
        } else {
            run(
                "tar",
                &[
                    "-czf",
                    archive_path,
                    "-C",
                    stage.to_str().ok_or("Non-UTF8 stage path")?,
                    ".",
                ],
            )?;
        }
        if native {
            fs::remove_file(stage.join(binary))?;
            run(
                "tar",
                &[
                    "-xzf",
                    archive_path,
                    "-C",
                    stage.to_str().ok_or("Non-UTF8 stage path")?,
                ],
            )?;
            verify_binary_version(&stage.join(binary), version)?;
        }
        let digest = capture("sha256sum", &[archive_path])?;
        let hash = digest
            .split_whitespace()
            .next()
            .ok_or("Missing checksum")?
            .to_owned();
        checksums.push_str(&format!("{hash}  {name}\n"));
        hashes.push(hash);
    }
    fs::write(dist.join("SHA256SUMS"), checksums)?;
    fs::write(dist.join("nubila.rb"), formula(version, &hashes))?;
    fs::write(dist.join("nubila.cask.rb"), cask(version, &hashes))?;
    fs::write(
        dist.join("notes.md"),
        format!(
            "{release_notes}\n\nTested on Linux x86-64 only. Other targets are cross-built and untested at runtime. macOS builds are not Developer ID signed or notarized; Windows builds are unsigned. Operating-system security policies may block downloads. No signing or notarization bypass is performed by the installer.\n"
        ),
    )?;
    fs::copy("install.sh", dist.join("install.sh"))?;
    if clean_commit(".")? != commit || clean_commit("../tapp-ui")? != framework {
        return Err("Sources changed during the build; rerun before publishing".into());
    }
    println!(
        "Artifacts prepared in {}. Nothing pushed or published.",
        dist.display()
    );
    Ok(())
}

fn main() -> Result<()> {
    env::set_current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))?;
    let manifest = fs::read_to_string("Cargo.toml")?;
    let version = version(&manifest)?;
    match env::args().nth(1).as_deref() {
        None | Some("plan") => {
            println!(
                "Nubila {version}: six local optimized release builds, only on explicit prepare."
            );
            for target in TARGETS {
                println!(
                    "  {} — {}",
                    archive_name(version, target),
                    target_status(target)
                );
            }
            println!(
                "Requires cargo-zigbuild, Zig, cargo-xwin/MSVC SDK, macOS SDKROOT, cargo-about, tar, zip, sha256sum, all six rustup targets.\nNo CI or build hooks. Normal cargo build/run remain native-only.\nPrepare: cargo run --locked --manifest-path tools/release/Cargo.toml -- prepare"
            );
            Ok(())
        }
        Some("prepare") => prepare(version),
        Some("refresh-local") => refresh_local(version),
        Some(_) => Err("Usage: nubila-release [plan|prepare|refresh-local]".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_notes_exclude_other_versions_and_unreleased() {
        let text = "# Changelog\n## [Unreleased]\nfuture\n## [0.1.0] - 2026-09-23\n### Added\n- First\n## [0.0.1] - 2026-09-01\nold";
        let result = notes(text, "0.1.0").unwrap();
        assert!(result.contains("First"));
        assert!(!result.contains("future") && !result.contains("old"));
        assert!(notes(text, "1.0.0").is_err());
    }
    #[test]
    fn six_unique_archives_and_platform_specific_formula() {
        let names: std::collections::HashSet<_> =
            TARGETS.iter().map(|t| archive_name("0.1.0", t)).collect();
        assert_eq!(names.len(), 6);
        let formula = formula("0.1.0", &vec!["a".repeat(64); 6]);
        assert!(formula.contains("on_macos") && formula.contains("on_linux"));
        assert!(!formula.contains("windows"));
        assert_eq!(formula.matches("sha256").count(), 4);
        let cask = cask("0.1.0", &vec!["a".repeat(64); 6]);
        assert!(cask.contains("binary \"nubila\"") && cask.contains("depends_on macos"));
        assert!(cask.contains("sha256 arm:") && cask.contains("intel:"));
        assert!(version("[package]\nversion = \"0.1\"").is_err());
    }

    #[test]
    fn version_gate_rejects_a_mismatched_executable() {
        let binary = std::env::current_exe().unwrap();
        assert!(verify_binary_version(&binary, "0.1.0").is_err());
    }
}
