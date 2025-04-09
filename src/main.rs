use std::{error::Error, io, sync::mpsc, thread, time::Duration};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Terminal,
};

mod platform;
mod storage;

// Application mode for controlling popup states.
enum AppMode {
    Normal,
    ConfirmEject(usize), // holds the index of the device to eject.
    Ejected(String),     // holds the eject result message.
}

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

    // Refresh the device list (used for manual refresh).
    fn refresh(&mut self) {
        self.devices = platform::macos::detect_storage_devices();
        if self.devices.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.devices.len() {
            self.selected = self.devices.len() - 1;
        }
    }
}

/// Spawns a background thread that polls the OS every second for new devices.
/// When the device list changes compared to the previous poll, the new list is sent through the channel.
fn start_device_listener(tx: mpsc::Sender<Vec<platform::macos::StorageDevice>>) {
    thread::spawn(move || {
        let mut old_devices = platform::macos::detect_storage_devices();
        loop {
            let new_devices = platform::macos::detect_storage_devices();
            if new_devices != old_devices {
                if tx.send(new_devices.clone()).is_err() {
                    break; // main thread has dropped the receiver.
                }
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

    // Create an mpsc channel for device updates.
    let (tx, rx) = mpsc::channel();
    start_device_listener(tx);

    // Load initial devices.
    let devices = platform::macos::detect_storage_devices();
    let mut app = App::new(devices);
    let mut mode = AppMode::Normal;

    loop {
        // Check for device updates from the listener thread.
        if let Ok(new_devices) = rx.try_recv() {
            app.devices = new_devices;
            if app.devices.is_empty() {
                app.selected = 0;
            } else if app.selected >= app.devices.len() {
                app.selected = app.devices.len() - 1;
            }
        }

        terminal.draw(|f| {
            let size = f.size();
            // Outer layout splits the main area and the legend.
            let outer_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(3)].as_ref())
                .split(size);

            // Main area: left panel (33%) and right panel (67%).
            let main_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(33), Constraint::Percentage(67)].as_ref())
                .split(outer_chunks[0]);

            // Left panel split vertically: top for device list, bottom for device details.
            let left_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
                .split(main_chunks[0]);

            // Build device list items; append an eject emoji (⏏) if ejectable.
            let items: Vec<ListItem> = app.devices.iter().map(|dev| {
                let text = if dev.ejectable {
                    format!("{} ⏏", dev.name)
                } else {
                    dev.name.clone()
                };
                ListItem::new(Spans::from(text))
            }).collect();

            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title("Devices"))
                .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
                .highlight_symbol(">> ");
            let mut list_state = ListState::default();
            list_state.select(Some(app.selected));
            f.render_stateful_widget(list, left_chunks[0], &mut list_state);

            // Device details panel.
            let details = if !app.devices.is_empty() {
                let device = &app.devices[app.selected];
                let total_gb = device.total_space as f64 / 1024_f64.powi(3);
                let free_gb = device.available_space as f64 / 1024_f64.powi(3);
                format!(
                    "Name: {}\nMount: {}\nTotal: {:.2} GB\nFree: {:.2} GB",
                    device.name, device.mount_point, total_gb, free_gb
                )
            } else {
                "No devices found.".to_string()
            };
            let details_paragraph = Paragraph::new(details)
                .block(Block::default().borders(Borders::ALL).title("Device Details"));
            f.render_widget(details_paragraph, left_chunks[1]);

            // Right panel placeholder.
            let right_panel = Paragraph::new("Empty panel")
                .block(Block::default().borders(Borders::ALL).title("Right Panel"));
            f.render_widget(right_panel, main_chunks[1]);

            // Bottom legend.
            let legend_text = "Keys: j = next, k = previous, e = eject, r = refresh, q = quit";
            let legend = Paragraph::new(legend_text)
                .block(Block::default().borders(Borders::ALL).title("Legend"));
            f.render_widget(legend, outer_chunks[1]);

            // Draw popups on top of the main UI if needed.
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
                AppMode::Normal => {}
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
                            KeyCode::Char('r') => app.refresh(), // manual refresh still works
                            KeyCode::Char('e') => {
                                if !app.devices.is_empty() && app.devices[app.selected].ejectable {
                                    mode = AppMode::ConfirmEject(app.selected);
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
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    Ok(())
}
