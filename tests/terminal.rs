#![cfg(unix)]
use std::{
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::process::CommandExt,
    },
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct Pty {
    master: File,
    slave: File,
    child: Child,
    before: libc::termios,
}
impl Drop for Pty {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Pty {
    fn spawn(state: &std::path::Path, config: &std::path::Path) -> Self {
        let mut master = -1;
        let mut slave = -1;
        let size = libc::winsize {
            ws_row: 24,
            ws_col: 100,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: openpty initializes owned descriptors; all pointer lifetimes span the call.
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    &size,
                )
            },
            0
        );
        let master = unsafe { File::from_raw_fd(master) };
        let slave = unsafe { File::from_raw_fd(slave) };
        let mut before = unsafe { std::mem::zeroed::<libc::termios>() };
        assert_eq!(
            unsafe { libc::tcgetattr(slave.as_raw_fd(), &mut before) },
            0
        );
        unsafe {
            libc::fcntl(master.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK);
            // The child must not keep the terminal's master alive after closure.
            libc::fcntl(master.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC);
        }
        let mut command = Command::new(env!("CARGO_BIN_EXE_nubila"));
        command
            .args(["tui", "--demo", "--state"])
            .arg(state)
            .arg("--config")
            .arg(config)
            .env("TERM", "xterm-256color")
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave.try_clone().unwrap()));
        let fd = slave.as_raw_fd();
        // SAFETY: only async-signal-safe syscalls in the post-fork child.
        unsafe {
            command.pre_exec(move || {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::ioctl(fd, libc::TIOCSCTTY as _, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn().unwrap();
        Self {
            master,
            slave,
            child,
            before,
        }
    }
    fn drain(&mut self, duration: Duration) -> String {
        let end = Instant::now() + duration;
        let mut out = vec![];
        while Instant::now() < end {
            let mut bytes = [0; 65536];
            match self.master.read(&mut bytes) {
                Ok(0) => break,
                Ok(n) => out.extend_from_slice(&bytes[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(e) => panic!("PTY read: {e}"),
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }
    fn send(&mut self, keys: &[u8]) -> String {
        self.master.write_all(keys).unwrap();
        self.drain(Duration::from_millis(70))
    }
    fn resize(&mut self, width: u16, height: u16) {
        let size = libc::winsize {
            ws_row: height,
            ws_col: width,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        assert_eq!(
            unsafe { libc::ioctl(self.slave.as_raw_fd(), libc::TIOCSWINSZ as _, &size) },
            0
        );
        unsafe {
            libc::kill(self.child.id() as i32, libc::SIGWINCH);
        }
    }
    fn finish(&mut self, signal: bool) -> String {
        if signal {
            unsafe {
                assert_eq!(libc::kill(self.child.id() as i32, libc::SIGTERM), 0);
            }
        } else {
            self.master.write_all(b"q").unwrap();
        }
        let end = Instant::now() + Duration::from_secs(3);
        let status = loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                break status;
            }
            assert!(
                Instant::now() < end,
                "TUI did not exit promptly: {}",
                self.drain(Duration::from_millis(10))
            );
            thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(status.code(), Some(if signal { 130 } else { 0 }));
        let mut after = unsafe { std::mem::zeroed::<libc::termios>() };
        assert_eq!(
            unsafe { libc::tcgetattr(self.slave.as_raw_fd(), &mut after) },
            0
        );
        assert_eq!(after.c_lflag, self.before.c_lflag);
        assert_eq!(after.c_iflag, self.before.c_iflag);
        assert_eq!(after.c_oflag, self.before.c_oflag);
        assert_eq!(after.c_cc, self.before.c_cc);
        let output = self.drain(Duration::from_millis(40));
        assert!(
            output.contains("\x1b[?1049l"),
            "alternate screen not restored: {output:?}"
        );
        assert!(
            output.contains("\x1b[?2004l"),
            "bracketed paste not restored"
        );
        assert!(output.contains("\x1b[?1000l"), "mouse capture not restored");
        output
    }
}
#[test]
fn real_terminal_navigation_resize_idle_and_cleanup() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state.json");
    let mut pty = Pty::spawn(&state, &tmp.path().join("config.toml"));
    let mut output = pty.drain(Duration::from_millis(500));
    assert!(output.contains("DEMO"), "{output}");
    assert!(output.contains("Tokyo"));
    if nubila::tui::LOADING_PREVIEW {
        assert!(
            !pty.drain(Duration::from_millis(350)).is_empty(),
            "Loading preview did not animate"
        );
    } else {
        assert!(
            pty.drain(Duration::from_millis(150)).is_empty(),
            "Idle TUI produced output"
        );
    }
    output += &pty.send(b"j\r");
    output += &pty.send(b"\x1b");
    output += &pty.send(b"?");
    output += &pty.send(b"\x1b");
    for (w, h) in [(20, 6), (32, 9), (55, 15), (80, 24), (180, 40)] {
        pty.resize(w, h);
        output += &pty.drain(Duration::from_millis(60));
    }
    output += &pty.send(b"c\r");
    output += &pty.send(b"h");
    output += &pty.send(b"n");
    output += &pty.send(b"aKyoto\r");
    output += &pty.drain(Duration::from_millis(100));
    output += &pty.send(b"\r");
    output += &pty.drain(Duration::from_millis(100));
    assert!(output.contains("Kyoto"), "terminal output: {output:?}");
    output += &pty.send(b"d");
    output += &pty.send(b"\r");
    assert!(
        !tmp.path().join("config.toml").exists(),
        "Demo must not save config"
    );
    output += &pty.send(b"/Tokyo\r");
    output += &pty.send(&[21]);
    output += &pty.finish(false);
    assert!(!output.contains("panicked"));
    let prefs: serde_json::Value = serde_json::from_slice(&std::fs::read(state).unwrap()).unwrap();
    assert_eq!(prefs["filter"], "");
    assert_eq!(prefs["mode"], "normal");
}
#[test]
fn sigterm_restores_terminal_modes() {
    let tmp = tempfile::tempdir().unwrap();
    let mut pty = Pty::spawn(
        &tmp.path().join("state.json"),
        &tmp.path().join("config.toml"),
    );
    pty.drain(Duration::from_millis(250));
    pty.finish(true);
}

#[test]
fn malformed_and_future_preferences_are_preserved_after_quit() {
    for bytes in [
        b"{broken".as_slice(),
        br#"{"future_setting":true}"#.as_slice(),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state.json");
        std::fs::write(&state, bytes).unwrap();
        let mut pty = Pty::spawn(&state, &tmp.path().join("config.toml"));
        pty.drain(Duration::from_millis(250));
        pty.finish(false);
        assert_eq!(std::fs::read(&state).unwrap(), bytes);
    }
}

#[test]
fn measure_idle_cpu_and_shared_dialog_terminal_cleanup() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state.json");
    let mut pty = Pty::spawn(&state, &tmp.path().join("config.toml"));
    pty.drain(Duration::from_millis(350));
    #[cfg(target_os = "linux")]
    {
        let ticks = || {
            let stat = std::fs::read_to_string(format!("/proc/{}/stat", pty.child.id())).unwrap();
            let fields = stat
                .rsplit_once(')')
                .unwrap()
                .1
                .split_whitespace()
                .collect::<Vec<_>>();
            fields[11].parse::<u64>().unwrap() + fields[12].parse::<u64>().unwrap()
        };
        let before = ticks();
        std::thread::sleep(Duration::from_millis(500));
        let after = ticks();
        println!("idle CPU ticks over 500ms: {}", after - before);
        let status = std::fs::read_to_string(format!("/proc/{}/status", pty.child.id())).unwrap();
        println!(
            "{}",
            status
                .lines()
                .find(|l| l.starts_with("VmRSS:"))
                .unwrap_or("RSS unavailable")
        );
    }
    pty.send(&[11]);
    pty.send(b"add city");
    pty.send(b"\r");
    pty.send(b"Kyoto?");
    let help = pty.send(b"\x1b[63;5u");
    assert!(help.contains("Help"), "Ctrl+? did not open Help: {help:?}");
    pty.send(b"\x1b");
    pty.send(&[11]);
    pty.send(b"\x1b");
    pty.send(b"\x1b");
    pty.finish(false);
}

#[test]
fn quitting_during_filter_preview_saves_only_the_committed_query() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state.json");
    let mut pty = Pty::spawn(&state, &tmp.path().join("config.toml"));
    pty.drain(Duration::from_millis(500));
    pty.send(b"fParis\r");
    pty.send(b"f\x15Tok");
    pty.finish(true);
    let prefs: serde_json::Value = serde_json::from_slice(&std::fs::read(state).unwrap()).unwrap();
    assert_eq!(prefs["filter"], "Paris");
}

#[test]
fn last_city_screen_survives_restart_and_list_shortcut_clears_it() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state.json");
    let config = tmp.path().join("config.toml");
    let mut pty = Pty::spawn(&state, &config);
    pty.drain(Duration::from_millis(500));
    pty.send(b"\r");
    pty.finish(false);
    let prefs: serde_json::Value = serde_json::from_slice(&std::fs::read(&state).unwrap()).unwrap();
    assert_eq!(prefs["last_city"], "tokyo");
    let mut reopened = Pty::spawn(&state, &config);
    let output = reopened.drain(Duration::from_millis(500));
    assert!(output.contains("l/Esc list"), "{output}");
    reopened.send(b"l");
    reopened.finish(false);
    let prefs: serde_json::Value = serde_json::from_slice(&std::fs::read(&state).unwrap()).unwrap();
    assert!(prefs["last_city"].is_null());
}

#[test]
fn closing_terminal_window_saves_last_city() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state.json");
    let config = tmp.path().join("config.toml");
    let mut pty = Pty::spawn(&state, &config);
    pty.drain(Duration::from_millis(500));
    pty.send(b"\r");
    // Dropping the last master simulates the terminal emulator closing its window.
    drop(std::mem::replace(
        &mut pty.master,
        File::open("/dev/null").unwrap(),
    ));
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = pty.child.try_wait().unwrap() {
            assert!(status.code().is_some(), "Killed by a signal: {status}");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "App did not exit after terminal closure"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let prefs: serde_json::Value = serde_json::from_slice(&std::fs::read(&state).unwrap()).unwrap();
    assert_eq!(prefs["last_city"], "tokyo");
    let mut reopened = Pty::spawn(&state, &config);
    let output = reopened.drain(Duration::from_millis(500));
    assert!(output.contains("l/Esc list"), "{output}");
    reopened.finish(false);
}
