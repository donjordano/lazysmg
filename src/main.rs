use std::{
    error::Error,
    fs,
    io,
    sync::mpsc,
    thread,
    time::Duration,
};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Spans},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Row, Table},
    Terminal,
};

mod platform;
mod storage;

// Structure to hold file details.
#[derive(Debug, Clone)]
struct FileEntry {
    name: String,
    path: String,
    size: u64,
}

/// Recursively scans the given start path for files.
/// Returns a vector of FileEntry items sorted in descending order by file size.
///
/// In case of errors (for example, permissions issues) the error is wrapped with context.
/// The error type is marked with `Send + 'static` so that it can be sent over threads.
fn scan_files(start_path: &str) -> Result<Vec<FileEntry>, Box<dyn Error + Send + 'static>> {
    let mut files = Vec::new();

    fn visit_dirs(dir: &std::path::Path, files: &mut Vec<FileEntry>) -> Result<(), Box<dyn Error + Send + 'static>> {
        let entries = fs::read_dir(dir).map_err(|e| -> Box<dyn Error + Send + 'static> {
            Box::new(io::Error::new(
                io::ErrorKind::Other,
                format!("Failed to read directory {:?}: {}", dir, e),
            ))
        })?;
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("Failed to get directory entry in {:?}: {}", dir, e);
                    continue;
                }
            };
            let path = entry.path();
            if path.is_dir() {
                if let Err(e) = visit_dirs(&path, files) {
                    if let Some(io_err) = e.downcast_ref::<io::Error>() {
                        if io_err.kind() == io::ErrorKind::PermissionDenied {
                            eprintln!("Permission denied for directory: {:?}", path);
                            continue;
                        }
                    }
                    return Err(e);
                }
            } else if path.is_file() {
                let metadata = match fs::metadata(&path) {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("Failed to get metadata for {:?}: {}", path, e);
                        continue;
                    }
                };
                let size = metadata.len();
                files.push(FileEntry {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    path: path.to_string_lossy().into_owned(),
                    size,
                });
            }
        }
        Ok(())
    }

    visit_dirs(std::path::Path::new(start_path), &mut files)
        .map_err(|e| -> Box<dyn Error + Send + 'static> { e })?;
    files.sort_by(|a, b| b.size.cmp(&a.size));
    Ok(files)
}

// Enum to manage the current UI state.
enum AppMode {
    Normal,
    ConfirmEject(usize), // holds the index of the device to confirm eject.
    Ejected(String),     // holds the ejection result message.
    Scanning { device_index: usize, spinner_index: usize }, // scanning in progress
    FileList { file_entries: Vec<FileEntry>, selected: usize }, // scan result view
}

// Application state structure.
struct App {
    devices: Vec<platform::macos::StorageDevice>,
    selected: usize,
}

impl App {
    fn new(devices: Vec<platform::macos::StorageDevice>) -> App {
        App {
            devices,
            selected: 0,
        }
    }

    fn next(&mut self) {
        if !self.devices.is_empty() {
            self.selected = (self.selected + 1) % self.devices.len();
        }
    }

    fn previous(&mut self) {
        if !self.devices.is_empty() {
            if self.selected == 0 {
                self.selected = self.devices.len() - 1;
            } else {
                self.selected -= 1;
            }
        }
    }

    fn refresh(&mut self) {
        self.devices = platform::macos::detect_storage_devices();
        if self.devices.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.devices.len() {
            self.selected = self.devices.len() - 1;
        }
    }
}

/// Spawns a background thread that polls for new devices every second.
/// When the device list changes, sends the new list through the provided channel.
fn start_device_listener(tx: mpsc::Sender<Vec<platform::macos::StorageDevice>>) {
    thread::spawn(move || {
        let mut old_devices = platform::macos::detect_storage_devices();
        loop {
            let new_devices = platform::macos::detect_storage_devices();
            if new_devices != old_devices {
                let _ = tx.send(new_devices.clone());
                old_devices = new_devices;
            }
            thread::sleep(Duration::from_secs(1));
        }
    });
}

/// Helper to compute a centered rectangle for popup modals.
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ]
            .as_ref(),
        )
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ]
            .as_ref(),
        )
        .split(popup_layout[1])[1]
}

fn main() -> Result<(), Box<dyn Error>> {
    // Initialize terminal.
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Set up channels for device updates and scan results.
    let (device_tx, device_rx) = mpsc::channel();
    start_device_listener(device_tx);

    let mut scan_rx: Option<mpsc::Receiver<Result<Vec<FileEntry>, Box<dyn Error + Send + 'static>>>> = None;

    let devices = platform::macos::detect_storage_devices();
    let mut app = App::new(devices);
    let mut mode = AppMode::Normal;

    // Spinner characters for scanning animation.
    let spinner_chars = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

    loop {
        // Check for updated devices.
        if let Ok(new_devices) = device_rx.try_recv() {
            app.devices = new_devices;
            if app.devices.is_empty() {
                app.selected = 0;
            } else if app.selected >= app.devices.len() {
                app.selected = app.devices.len() - 1;
            }
        }

        // Update spinner and check for scan results if scanning.
        if let AppMode::Scanning { device_index: _, ref mut spinner_index } = mode {
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

        terminal.draw(|f| {
            let size = f.size();

            // Outer layout: main area and bottom legend.
            let outer_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(3)].as_ref())
                .split(size);

            // Main area split: left panel (33%) and right panel (67%).
            let main_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(33), Constraint::Percentage(67)].as_ref())
                .split(outer_chunks[0]);

            // Left panel: top for device list, bottom for device details.
            let left_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
                .split(main_chunks[0]);

            // Build device list items.
            let items: Vec<ListItem> = app
                .devices
                .iter()
                .enumerate()
                .map(|(i, dev)| {
                    let mut text = dev.name.clone();
                    if let AppMode::Scanning { device_index, spinner_index } = mode {
                        if i == device_index {
                            text = format!("{} {}", dev.name, spinner_chars[spinner_index]);
                        }
                    } else if dev.ejectable {
                        text = format!("{} ⏏", dev.name);
                    }
                    ListItem::new(Spans::from(text))
                })
                .collect();

            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title("Devices"))
                .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
                .highlight_symbol(">> ");
            let mut list_state = ListState::default();
            list_state.select(Some(app.selected));
            f.render_stateful_widget(list, left_chunks[0], &mut list_state);

            // Device details panel.
            let device_details = if !app.devices.is_empty() {
                let device = &app.devices[app.selected];
                let total_gb = device.total_space as f64 / 1024_f64.powi(3);
                let free_gb = device.available_space as f64 / 1024_f64.powi(3);
                let mut info = format!(
                    "Name: {}\nMount: {}\nTotal: {:.2} GB\nFree: {:.2} GB",
                    device.name, device.mount_point, total_gb, free_gb
                );
                if let Some(extra) = &device.vendor_info {
                    info.push_str("\nInfo:");
                    for part in extra.split(',') {
                        info.push_str(&format!("\n       - {}", part.trim()));
                    }
                }
                info
            } else {
                "No devices found.".to_string()
            };
            let details_paragraph = Paragraph::new(device_details)
                .block(Block::default().borders(Borders::ALL).title("Device Details"));
            f.render_widget(details_paragraph, left_chunks[1]);

            // Right panel: changes depending on mode.
            match mode {
                AppMode::Normal => {
                    let right_panel = Paragraph::new("Empty panel")
                        .block(Block::default().borders(Borders::ALL).title("Right Panel"));
                    f.render_widget(right_panel, main_chunks[1]);
                }
                AppMode::Scanning { .. } => {
                    let scan_panel = Paragraph::new("Scanning in progress...")
                        .block(Block::default().borders(Borders::ALL).title("Scan Status"));
                    f.render_widget(scan_panel, main_chunks[1]);
                }
                AppMode::FileList { ref file_entries, selected } => {
                    let rows: Vec<Row> = file_entries
                        .iter()
                        .map(|entry| {
                            let size_str = format!("{} bytes", entry.size);
                            Row::new(vec![entry.name.clone(), entry.path.clone(), size_str])
                        })
                        .collect();
                    let table = Table::new(rows)
                        .header(
                            Row::new(vec!["Name", "Path", "File Size"])
                                .style(Style::default().fg(Color::LightBlue))
                                .bottom_margin(1),
                        )
                        .block(Block::default().borders(Borders::ALL).title("Files"))
                        .widths(&[
                            Constraint::Percentage(30),
                            Constraint::Percentage(50),
                            Constraint::Percentage(20),
                        ]);
                    f.render_widget(table, main_chunks[1]);
                }
                _ => {
                    let right_panel = Paragraph::new("Empty panel")
                        .block(Block::default().borders(Borders::ALL).title("Right Panel"));
                    f.render_widget(right_panel, main_chunks[1]);
                }
            }

            // Bottom legend.
            let legend_text = "Keys: j = next, k = previous, e = eject, s = scan, r = refresh, q = quit, b = back (from file list)";
            let legend = Paragraph::new(legend_text)
                .block(Block::default().borders(Borders::ALL).title("Legend"));
            f.render_widget(legend, outer_chunks[1]);

            // Popup overlays.
            match mode {
                AppMode::ConfirmEject(index) => {
                    if let Some(device) = app.devices.get(index) {
                        let popup_area = centered_rect(60, 20, size);
                        let text = format!(
                            "Are you sure you want to eject this device?\n(Device: {})\nPress Y to confirm, N to cancel.",
                            device.name
                        );
                        let block = Block::default()
                            .borders(Borders::ALL)
                            .title("Confirm Eject")
                            .style(Style::default().fg(Color::White).bg(Color::Black));
                        let paragraph = Paragraph::new(text).block(block);
                        f.render_widget(paragraph, popup_area);
                    }
                }
                AppMode::Ejected(ref msg) => {
                    let popup_area = centered_rect(60, 20, size);
                    let text = format!("{}\nPress any key to continue.", msg);
                    let block = Block::default()
                        .borders(Borders::ALL)
                        .title("Ejection Result")
                        .style(Style::default().fg(Color::White).bg(Color::Black));
                    let paragraph = Paragraph::new(text).block(block);
                    f.render_widget(paragraph, popup_area);
                }
                _ => {}
            }
        })?;

        // Event handling.
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                match mode {
                    AppMode::Normal => {
                        match key.code {
                            KeyCode::Char('q') => break,
                            KeyCode::Char('j') => app.next(),
                            KeyCode::Char('k') => app.previous(),
                            KeyCode::Char('r') => app.refresh(),
                            KeyCode::Char('e') => {
                                if !app.devices.is_empty() && app.devices[app.selected].ejectable {
                                    mode = AppMode::ConfirmEject(app.selected);
                                }
                            }
                            KeyCode::Char('s') => {
                                if !app.devices.is_empty() {
                                    let device = &app.devices[app.selected];
                                    let mount = device.mount_point.clone();
                                    let (tx_scan, rx_scan) = mpsc::channel();
                                    thread::spawn(move || {
                                        let result = scan_files(&mount);
                                        let _ = tx_scan.send(result);
                                    });
                                    mode = AppMode::Scanning { device_index: app.selected, spinner_index: 0 };
                                    scan_rx = Some(rx_scan);
                                }
                            }
                            _ => {}
                        }
                    }
                    AppMode::ConfirmEject(index) => {
                        match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                if let Some(device) = app.devices.get(index) {
                                    match platform::macos::eject_device(device) {
                                        Ok(()) => {
                                            let device_name = device.name.clone();
                                            app.devices.remove(index);
                                            if app.devices.is_empty() {
                                                app.selected = 0;
                                            } else if app.selected >= app.devices.len() {
                                                app.selected = app.devices.len() - 1;
                                            }
                                            mode = AppMode::Ejected(format!("Ejected Device: {} successfully", device_name));
                                        }
                                        Err(err) => {
                                            mode = AppMode::Ejected(format!("Failed to eject {}: {}", device.name, err));
                                        }
                                    }
                                } else {
                                    mode = AppMode::Normal;
                                }
                            }
                            KeyCode::Char('n') | KeyCode::Char('N') => {
                                mode = AppMode::Normal;
                            }
                            _ => {}
                        }
                    }
                    AppMode::Ejected(_) => {
                        mode = AppMode::Normal;
                    }
                    AppMode::Scanning { .. } => {
                        // While scanning, ignore key events.
                    }
                    AppMode::FileList { ref mut selected, .. } => {
                        match key.code {
                            KeyCode::Char('j') => *selected += 1,
                            KeyCode::Char('k') => { if *selected > 0 { *selected -= 1; } },
                            KeyCode::Char('b') => { mode = AppMode::Normal; },
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    Ok(())
}
