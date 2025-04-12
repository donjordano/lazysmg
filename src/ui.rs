use ratatui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Spans,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Row, Table},
    Terminal,
};
use crate::{App, AppMode};

/// Compute a centered rectangle for popups.
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

pub fn draw_app<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &App,
    mode: &AppMode,
    _spinner_chars: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    terminal.draw(|f| {
        let size = f.size();
        // Outer layout: main area and bottom legend.
        let outer_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(3)].as_ref())
            .split(size);
        // Main area: left panel (33%) and right panel (67%).
        let main_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(33), Constraint::Percentage(67)].as_ref())
            .split(outer_chunks[0]);
        // Left panel for devices.
        let left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
            .split(main_chunks[0]);

        // Left panel: Device list.
        let items: Vec<ListItem> = app
            .devices
            .iter()
            .enumerate()
            .map(|(_i, dev)| {
                let mut text = dev.name.clone();
                if dev.ejectable {
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

        // Left panel: Device details.
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

        // Right panel: Show file listing if available; otherwise a placeholder.
        let right_content = if app.devices.is_empty() {
            "No storage devices detected."
        } else if app.scanning {
            "Scanning in progress..."
        } else if let Some(ref entries) = app.file_entries {
            if entries.is_empty() {
                "No files/folders found on this device."
            } else {
                ""
            }
        } else {
            "Loading files..."
        };

        if app.file_entries.is_some() && !app.scanning && !app.file_entries.as_ref().unwrap().is_empty() {
            let rows: Vec<Row> = app.file_entries.as_ref().unwrap().iter().map(|entry| {
                let size_str = format!("{} bytes", entry.size);
                Row::new(vec![entry.name.clone(), entry.path.clone(), size_str])
            }).collect();
            let table = Table::new(rows)
                .header(
                    Row::new(vec!["Name", "Path", "File Size"])
                        .style(Style::default().fg(Color::LightBlue))
                        .bottom_margin(1),
                )
                .block(Block::default().borders(Borders::ALL).title("Files & Folders"))
                .widths(&[
                    Constraint::Percentage(30),
                    Constraint::Percentage(50),
                    Constraint::Percentage(20),
                ]);
            f.render_widget(table, main_chunks[1]);
        } else {
            let right_panel = Paragraph::new(right_content)
                .block(Block::default().borders(Borders::ALL).title("Right Panel"));
            f.render_widget(right_panel, main_chunks[1]);
        }

        let legend_text = "Keys: j/k = up/down, Ctrl-l/Ctrl-h = focus left/right, r = refresh, q = quit, e = eject, s = scan";
        let legend = Paragraph::new(legend_text)
            .block(Block::default().borders(Borders::ALL).title("Legend"));
        f.render_widget(legend, outer_chunks[1]);

        // Popup overlays.
        match mode {
            AppMode::ConfirmEject(index) => {
                if let Some(device) = app.devices.get(*index) {
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
            },
            AppMode::Ejected(msg) => {
                let popup_area = centered_rect(60, 20, size);
                let text = format!("{}\nPress any key to continue.", msg);
                let block = Block::default()
                    .borders(Borders::ALL)
                    .title("Ejection Result")
                    .style(Style::default().fg(Color::White).bg(Color::Black));
                let paragraph = Paragraph::new(text).block(block);
                f.render_widget(paragraph, popup_area);
            },
            _ => {}
        }
    })?;
    Ok(())
}
