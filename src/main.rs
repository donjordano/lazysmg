mod ui;
mod event_handler;
mod platform;
mod scanner;
mod storage; // if needed

use std::{
    error::Error,
    sync::mpsc,
    time::Duration,
};
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use ui::draw_app;
use event_handler::process_event;
use platform::macos::{detect_storage_devices, StorageDevice};
use scanner::{FileEntry, list_directory};

/// Which panel is focused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelFocus {
    Left,
    Right,
}

/// Application mode. We keep only Normal (with a possible scanning flag).
#[derive(Debug, Clone)]
pub enum AppMode {
    Normal,
    ConfirmEject(usize),
    Ejected(String),
    Scanning { device_index: usize, spinner_index: usize },
}

/// Main application state.
#[derive(Debug)]
pub struct App {
    pub devices: Vec<StorageDevice>,
    pub selected: usize,
    pub file_entries: Option<Vec<FileEntry>>, // current directory listing for the selected device
    pub scanning: bool,                        // whether a directory listing is in progress
    pub focus: PanelFocus,
}

impl App {
    pub fn new(devices: Vec<StorageDevice>) -> App {
        App {
            devices,
            selected: 0,
            file_entries: None,
            scanning: false,
            focus: PanelFocus::Left,
        }
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize terminal.
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create an mpsc channel for device updates.
    let (device_tx, device_rx) = mpsc::channel();
    event_handler::start_device_listener(device_tx);

    // Tokio mpsc channel for async directory listings.
    let (scan_tx, mut scan_rx) =
        tokio::sync::mpsc::channel::<Result<Vec<FileEntry>, Box<dyn Error + Send + 'static>>>(1);

    let devices = detect_storage_devices();
    let mut app = App::new(devices);
    let mut mode = AppMode::Normal;
    let spinner_chars = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

    // When the app starts, if there is at least one device, trigger a directory listing for it.
    let mut last_selected = app.selected;
    if !app.devices.is_empty() {
        let mount = app.devices[app.selected].mount_point.clone();
        let sender = scan_tx.clone();
        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || list_directory(&mount))
                .await
                .unwrap_or_else(|e| Err(Box::new(e) as Box<dyn Error + Send + 'static>));
            let _ = sender.send(result).await;
        });
        app.scanning = true;
        mode = AppMode::Scanning { device_index: app.selected, spinner_index: 0 };
    }

    loop {
        // Update device list from listener.
        if let Ok(new_devices) = device_rx.try_recv() {
            app.devices = new_devices;
            if app.devices.is_empty() {
                app.selected = 0;
            } else if app.selected >= app.devices.len() {
                app.selected = app.devices.len() - 1;
            }
        }

        // When in Normal mode, check if the selection changed.
        if let AppMode::Normal = mode {
            if !app.devices.is_empty() && app.selected != last_selected {
                // A new device was selected; trigger an async directory listing.
                app.scanning = true;
                let mount = app.devices[app.selected].mount_point.clone();
                let sender = scan_tx.clone();
                tokio::spawn(async move {
                    let result = tokio::task::spawn_blocking(move || list_directory(&mount))
                        .await
                        .unwrap_or_else(|e| Err(Box::new(e) as Box<dyn Error + Send + 'static>));
                    let _ = sender.send(result).await;
                });
                // Update last_selected.
                last_selected = app.selected;
                mode = AppMode::Scanning { device_index: app.selected, spinner_index: 0 };
            }
        }

        // In Scanning mode, update spinner and attempt to receive the file listing.
        if let AppMode::Scanning { ref mut spinner_index, .. } = mode {
            *spinner_index = (*spinner_index + 1) % spinner_chars.len();
            if let Ok(result) = scan_rx.try_recv() {
                match result {
                    Ok(file_entries) => {
                        app.file_entries = Some(file_entries);
                        app.scanning = false;
                        mode = AppMode::Normal;
                    }
                    Err(e) => {
                        mode = AppMode::Ejected(format!("Scan failed: {}", e));
                        app.scanning = false;
                    }
                }
            }
        }

        // Draw UI.
        draw_app(&mut terminal, &app, &mode, &spinner_chars)?;

        // Process key events.
        if process_event(&mut app, &mut mode, &scan_tx).await? {
            break;
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}
