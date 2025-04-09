use std::{error::Error, io};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::Spans,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Terminal,
};

mod platform;
mod storage;

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
}

fn main() -> Result<(), Box<dyn Error>> {
    // Set up the terminal (raw mode, alternate screen, etc.)
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Load devices (using our macOS detection).
    let devices = platform::macos::detect_storage_devices();
    let mut app = App::new(devices);

    loop {
        terminal.draw(|f| {
            // Split entire area vertically into the main area and the bottom legend.
            let outer_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(0),    // Main area.
                    Constraint::Length(3), // Legend.
                ])
                .split(f.size());

            // Main area split horizontally: left panel (1/3) and right panel (2/3).
            let main_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(33), // Left column (device list and details).
                    Constraint::Percentage(67), // Right column (empty for now).
                ])
                .split(outer_chunks[0]);

            // Left column split vertically into Device List (top) and Device Details (bottom).
            let left_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(50), // Device list.
                    Constraint::Percentage(50), // Device details.
                ])
                .split(main_chunks[0]);

            // Build device list items.
            let items: Vec<ListItem> = app
                .devices
                .iter()
                .map(|dev| {
                    let content = vec![Spans::from(dev.name.clone())];
                    ListItem::new(content)
                })
                .collect();

            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL).title("Devices"))
                .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
                .highlight_symbol(">> ");

            let mut list_state = ListState::default();
            list_state.select(Some(app.selected));
            f.render_stateful_widget(list, left_chunks[0], &mut list_state);

            // Device details panel in the left column.
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

            // Right column: placeholder.
            let right_panel = Paragraph::new("Empty panel")
                .block(Block::default().borders(Borders::ALL).title("Right Panel"));
            f.render_widget(right_panel, main_chunks[1]);

            // Bottom legend.
            let legend_text = "Keys: j = next, k = previous, q = quit";
            let legend = Paragraph::new(legend_text)
                .block(Block::default().borders(Borders::ALL).title("Legend"));
            f.render_widget(legend, outer_chunks[1]);
        })?;

        // Handle input events: vim-like keys for navigation and quit.
        if event::poll(std::time::Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('j') => app.next(),
                    KeyCode::Char('k') => app.previous(),
                    _ => {}
                }
            }
        }
    }

    // Restore terminal to its original state.
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    Ok(())
}
