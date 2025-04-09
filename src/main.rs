use std::{error::Error, io};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Table, Row},
    Terminal,
};

mod platform;
mod storage;

fn main() -> Result<(), Box<dyn Error>> {
    // Set up terminal in raw mode and enter the alternate screen.
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    loop {
        terminal.draw(|f| {
            // Divide the screen into three parts: Title, Instructions, and the Devices Table.
            let size = f.size();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(2)
                .constraints(
                    [
                        Constraint::Length(3), // Title
                        Constraint::Length(3), // Instructions
                        Constraint::Min(5),    // Table
                    ]
                    .as_ref(),
                )
                .split(size);

            // Title
            let title = Paragraph::new("Lazy Storage Manager")
                .style(Style::default().fg(Color::Yellow))
                .alignment(Alignment::Center)
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(title, chunks[0]);

            // Instructions
            let instructions = Paragraph::new("Press 'r' to refresh, 'q' to quit.")
                .block(Block::default().borders(Borders::ALL).title("Instructions"));
            f.render_widget(instructions, chunks[1]);

            // Fetch storage devices from the platform module.
            let devices = platform::macos::detect_storage_devices();

            // Build table rows from the device list.
            let rows: Vec<Row> = devices.iter().map(|dev| {
                // Convert space from bytes to GB.
                let total_gb = dev.total_space as f64 / 1024.0_f64.powi(3);
                let free_gb = dev.available_space as f64 / 1024.0_f64.powi(3);
                Row::new(vec![
                    dev.name.clone(),
                    format!("{:.2} GB", total_gb),
                    format!("{:.2} GB", free_gb),
                ])
            }).collect();

            // Create the table widget.
            let table = Table::new(rows)
                .header(Row::new(vec!["Device", "Size", "Free"]))
                .block(Block::default().borders(Borders::ALL).title("Storage Devices"))
                .widths(&[
                    Constraint::Length(20),
                    Constraint::Length(15),
                    Constraint::Length(15),
                ]);
            f.render_widget(table, chunks[2]);
        })?;

        // Poll for and handle input events.
        if event::poll(std::time::Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => break, // Exit on 'q'.
                    KeyCode::Char('r') => {
                        // When 'r' is pressed, simply let the loop redraw – the data is reloaded every time.
                    }
                    _ => {}
                }
            }
        }
    }

    // Restore the terminal state.
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    Ok(())
}
