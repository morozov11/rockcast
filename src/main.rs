//! RockCast — internet radio on Chromecast (native CASTV2 client).

// Release: GUI only (no console flash). Debug keeps a console for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    sync::{Arc, Mutex},
};

use env_logger::Target;
use rockcast::{
    app::RockCastApp,
    settings::{self, AppSettings},
};

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE},
    System::Threading::{AttachThreadInput, CreateMutexW, GetCurrentThreadId},
    UI::WindowsAndMessaging::{
        BringWindowToTop, FindWindowW, GetForegroundWindow, GetWindowThreadProcessId, IsIconic,
        SW_RESTORE, SetForegroundWindow, ShowWindow,
    },
};

#[cfg(windows)]
struct SingleInstance(HANDLE);

#[cfg(windows)]
impl Drop for SingleInstance {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: this handle came from CreateMutexW and is owned by this guard.
            unsafe { CloseHandle(self.0) };
        }
    }
}

/// Claims the per-desktop-session RockCast instance. If another instance owns
/// the mutex, restores its window and brings it forward before returning None.
#[cfg(windows)]
fn claim_single_instance() -> Option<SingleInstance> {
    let mutex_name: Vec<u16> = "Local\\RockCast.SingleInstance.v1\0"
        .encode_utf16()
        .collect();

    // SAFETY: both pointers are valid for this call and the name is NUL-terminated.
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, mutex_name.as_ptr()) };
    if handle.is_null() {
        // Do not make an OS resource failure prevent RockCast from starting.
        return Some(SingleInstance(handle));
    }

    // GetLastError must be read immediately after CreateMutexW.
    if unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
        return Some(SingleInstance(handle));
    }

    // This process does not own the existing mutex handle for any useful work.
    unsafe { CloseHandle(handle) };
    activate_existing_window();
    None
}

#[cfg(windows)]
fn activate_existing_window() {
    // The first process may own the mutex just before eframe creates its window.
    for _ in 0..40 {
        for title in rockcast::i18n::WINDOW_TITLES {
            let title: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
            // SAFETY: title is NUL-terminated and lives for the duration of the call.
            let hwnd = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
            if hwnd.is_null() {
                continue;
            }

            // SAFETY: hwnd identifies the existing RockCast top-level window. Temporarily
            // sharing the foreground thread's input queue gives this user-initiated launcher
            // a reliable activation context; the queues are detached before returning.
            unsafe {
                let foreground = GetForegroundWindow();
                let current_thread = GetCurrentThreadId();
                let foreground_thread = if foreground.is_null() {
                    0
                } else {
                    GetWindowThreadProcessId(foreground, std::ptr::null_mut())
                };
                let attached = foreground_thread != 0
                    && foreground_thread != current_thread
                    && AttachThreadInput(current_thread, foreground_thread, 1) != 0;

                if IsIconic(hwnd) != 0 {
                    ShowWindow(hwnd, SW_RESTORE);
                }
                BringWindowToTop(hwnd);
                SetForegroundWindow(hwnd);

                if attached {
                    AttachThreadInput(current_thread, foreground_thread, 0);
                }
            }
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// Writes log lines to a file and (in debug) also to stderr.
struct TeeLog {
    file: Arc<Mutex<File>>,
    also_stderr: bool,
}

impl Write for TeeLog {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Ok(mut f) = self.file.lock() {
            let _ = f.write_all(buf);
            let _ = f.flush();
        }
        if self.also_stderr {
            let _ = io::stderr().write_all(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Ok(mut f) = self.file.lock() {
            let _ = f.flush();
        }
        if self.also_stderr {
            let _ = io::stderr().flush();
        }
        Ok(())
    }
}

fn init_logging() -> std::path::PathBuf {
    let path = settings::log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    // Fresh log each launch so a failure session is easy to share.
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
        .unwrap_or_else(|_| File::create(&path).expect("create rockcast.log"));

    let tee = TeeLog {
        file: Arc::new(Mutex::new(file)),
        also_stderr: cfg!(debug_assertions),
    };

    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("rockcast=debug,info"),
    )
    .format_timestamp_millis()
    .target(Target::Pipe(Box::new(tee)))
    .init();

    path
}

fn main() -> eframe::Result<()> {
    #[cfg(windows)]
    let Some(_single_instance) = claim_single_instance() else {
        return Ok(());
    };

    // rustls 0.23: crypto provider required
    let _ = rustls::crypto::ring::default_provider().install_default();
    let log_path = init_logging();
    log::info!("RockCast starting; log file: {}", log_path.display());
    if rockcast::profile::enabled() {
        log::info!(
            "Playback diagnostics ON — PLAYBACK_DIAG every 2s + DIAG warnings in {}",
            log_path.display()
        );
    }
    #[cfg(debug_assertions)]
    log::debug!("Optional telemetry: set ROCKCAST_METRICS=1 or ROCKCAST_PROFILE=1 before launch");

    let settings = AppSettings::load();
    let title = settings.language.t().window_title;
    let app_icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/app-icon.png"))
        .expect("embedded RockCast app icon must be a valid PNG");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([940.0, 740.0])
            .with_min_inner_size([820.0, 640.0])
            .with_title(title)
            .with_icon(app_icon),
        ..Default::default()
    };

    eframe::run_native(
        "RockCast",
        options,
        Box::new(|cc| Ok(Box::new(RockCastApp::new(cc)))),
    )
}
