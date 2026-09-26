//! Windows console close events run on a system-created thread, not a Unix
//! signal handler. Keep that thread alive until normal state saving completes:
//! returning from CTRL_CLOSE_EVENT authorizes Windows to terminate the process.
use std::{
    io,
    sync::{Arc, Condvar, Mutex, mpsc},
    thread,
    time::Duration,
};
use windows_sys::Win32::System::Console::{
    CTRL_BREAK_EVENT, CTRL_C_EVENT, CTRL_CLOSE_EVENT, SetConsoleCtrlHandler,
};

struct State {
    notify: mpsc::Sender<()>,
    complete: Mutex<bool>,
    completed: Condvar,
}

static ACTIVE: Mutex<Option<Arc<State>>> = Mutex::new(None);

pub struct Guard(Arc<State>);

impl Guard {
    pub fn install(shutdown: impl FnOnce() + Send + 'static) -> io::Result<Self> {
        let mut active = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
        if active.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Console shutdown handler already installed",
            ));
        }
        let (notify, received) = mpsc::channel();
        let state = Arc::new(State {
            notify,
            complete: Mutex::new(false),
            completed: Condvar::new(),
        });
        thread::Builder::new()
            .name("console-close".into())
            .spawn(move || {
                if received.recv().is_ok() {
                    shutdown();
                }
            })?;
        *active = Some(Arc::clone(&state));
        // SAFETY: callback has the required ABI and remains valid for the process lifetime.
        if unsafe { SetConsoleCtrlHandler(Some(handle), 1) } == 0 {
            *active = None;
            return Err(io::Error::last_os_error());
        }
        Ok(Self(state))
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        // The host drops this guard after saving preferences and restoring the terminal.
        *self.0.complete.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.0.completed.notify_all();
        // SAFETY: unregister exactly the callback registered by install.
        unsafe {
            SetConsoleCtrlHandler(Some(handle), 0);
        }
        *ACTIVE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

unsafe extern "system" fn handle(event: u32) -> windows_sys::core::BOOL {
    if !matches!(event, CTRL_CLOSE_EVENT | CTRL_C_EVENT | CTRL_BREAK_EVENT) {
        return 0;
    }
    let state = ACTIVE.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let Some(state) = state else {
        return 0;
    };
    // Unbounded notification avoids blocking on the TUI's bounded event queue.
    let _ = state.notify.send(());
    if event == CTRL_CLOSE_EVENT {
        let complete = state.complete.lock().unwrap_or_else(|e| e.into_inner());
        // Windows normally allows five seconds. Leave a margin and never wait
        // indefinitely if the main thread is stuck or its storage is unavailable.
        drop(
            state
                .completed
                .wait_timeout_while(complete, Duration::from_secs(4), |done| !*done),
        );
    }
    1
}
