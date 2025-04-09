mod ui;
mod event_handler;
mod platform;
mod storage;
mod scanner;

use std::{
    error::Error,
    sync::mpsc,
    thread,
    time::Duration,
};

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};
use ui::draw_app;
use event_handler::process_event;
use platform::macos::{detect_storage_devices, StorageDevice};
use scanner::FileEntry;

/// Enum to represent the current application UI mode.
#[derive(Debug, Clone)]
pub enum AppMode {
    Normal,
    ConfirmEject(usize), // holds device index for eject confirmation.
    Ejected(String),     // holds the result message for an ejection attempt.
    Scanning { device_index: usize, spinner_index: usize }, // scanning in progress.
    FileList { file_entries: Vec<FileEntry>, selected: usize }, // scan result view.
}

/// Main application state.
#[derive(Debug)]
pub struct App {
    pub devices: Vec<StorageDevice>,
    pub selected: usize,
}

impl App {
    pub fn new(devices: Vec<StorageDevice>) -> App {
        App { devices, selected: 0 }
    }

    pub fn next(&mut self) {
        if !self.devices.is_empty() {
            self.selected = (self.selected + 1) % self.devices.len();
        }
    }

    pub fn previous(&mut self) {
        if !self.devices.is_empty() {
            if self.selected == 0 {
                self.selected = self.devices.len() - 1;
            } else {
                self.selected -= 1;
            }
        }
    }

    pub fn refresh(&mut self) {
        self.devices = detect_storage_devices();
        if self.devices.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.devices.len() {
            self.selected = self.devices.len() - 1;
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Set up a channel for device updates.
    let (device_tx, device_rx) = mpsc::channel();
    event_handler::start_device_listener(device_tx);

    let mut scan_rx: Option<mpsc::Receiver<Result<Vec<FileEntry>, Box<dyn Error + Send + 'static>>>> = None;

    let devices = detect_storage_devices();
    let mut app = App::new(devices);
    let mut mode = AppMode::Normal;
    let spinner_chars = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

    loop {
        if let Ok(new_devices) = device_rx.try_recv() {
            app.devices = new_devices;
            if app.devices.is_empty() {
                app.selected = 0;
            } else if app.selected >= app.devices.len() {
                app.selected = app.devices.len() - 1;
            }
        }

        if let AppMode::Scanning { ref mut spinner_index, .. } = mode {
            *spinner_index = (*spinner_index + 1) % spinner_chars.len();
            if let Some(ref rx) = scan_rx {
                if let Ok(result) = rx.try_recv() {
                    match result {
                        Ok(file_entries) => {
                            mode = AppMode::FileList { file_entries, selected: 0 };
                            scan_rx = None;
                        }
                        Err(e) => {
                            mode = AppMode::Ejected(format!("Scan failed: {}", e));
                            scan_rx = None;
                        }
                    }
                }
            }
        }

        draw_app(&mut terminal, &app, &mode, &spinner_chars)?;

        if process_event(&mut app, &mut mode, &mut scan_rx)? {
            break;
        }

        thread::sleep(Duration::from_millis(200));
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}
