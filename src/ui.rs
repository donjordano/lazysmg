use ratatui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, Gauge, List, ListItem, ListState, Paragraph, Row, Table, Clear},
    Terminal,
};
use crate::{App, AppMode};

/// Compute a centered rectangle for popup overlays.
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
    spinner_chars: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    terminal.draw(|f| {
        let size = f.size();
        // Outer layout: main area and bottom legend.
        let outer_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(3)].as_ref())
            .split(size);
        // Main area: left panel (30%) and right panel (70%).
        let main_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
            .split(outer_chunks[0]);

        // Split right panel into top (file listing) and bottom (scan progress)
        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)].as_ref())
            .split(main_chunks[1]);
        // Left panel: split vertically into two parts.
        // Top: device list; Bottom: split further into device details (70%) and progress bar (30%).
        let left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
            .split(main_chunks[0]);
        let details_and_gauge = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(80), Constraint::Percentage(20)].as_ref())
            .split(left_chunks[1]);

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

        // Set different block style based on focus
        let devices_block_style = if app.focus == crate::PanelFocus::Left {
            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        let list = List::new(items)
            .block(Block::default()
                .borders(Borders::ALL)
                .title("Devices")
                .border_style(devices_block_style))
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
        f.render_widget(details_paragraph, details_and_gauge[0]);

        // Left panel: Progress Bar gauge.
        if !app.devices.is_empty() {
            let device = &app.devices[app.selected];
            let total = device.total_space as f64;
            let free = device.available_space as f64;
            let used = total - free;
            let percent = if total > 0.0 {
                (used / total * 100.0).round() as u16
            } else {
                0
            };
            let label = format!("Used: {}%", percent);
            let gauge = Gauge::default()
                .block(Block::default().borders(Borders::ALL).title("Usage"))
                .gauge_style(Style::default().fg(Color::Magenta).bg(Color::Black))
                .percent(percent)
                .label(Span::raw(label));
            f.render_widget(gauge, details_and_gauge[1]);
        } else {
            // If no devices, display a placeholder.
            let placeholder = Paragraph::new("No device available")
                .block(Block::default().borders(Borders::ALL).title("Usage"));
            f.render_widget(placeholder, details_and_gauge[1]);
        }

        // Right top panel - file listing
        let right_content = if app.devices.is_empty() {
            "No storage devices detected."
        } else if app.scanning {
            "Scanning in progress..."
        } else if let Some(ref entries) = app.file_entries {
            if entries.is_empty() {
                "No files/folders found on this device."
            } else {
                "" // Table view below.
            }
        } else {
            "Loading files..."
        };

        // Determine which files to display (regular listing or full scan results)
        let display_full_scan = app.full_scan_results.is_some() && !app.scan_progress.in_progress;

        // Right top panel - File listing
        if (app.file_entries.is_some() && !app.scanning && !app.file_entries.as_ref().unwrap().is_empty()) || display_full_scan {
            let entries = if display_full_scan {
                app.full_scan_results.as_ref().unwrap()
            } else {
                app.file_entries.as_ref().unwrap()
            };

            let title = if display_full_scan {
                "Files By Size (Descending)"
            } else {
                "Files & Folders"
            };

            // Apply scrolling by showing a window of entries
            let visible_entries: Vec<(usize, &crate::scanner::FileEntry)> = entries.iter()
                .enumerate()
                .skip(app.file_list_offset)
                .take(20) // Show ~20 entries at a time
                .collect();

            // Show scroll indicators and count in the title
            let mut title = title.to_string();
            title = format!("{} [{}/{}]", title, app.selected_file_index + 1, entries.len());

            // Add up/down scroll indicators with more visible characters
            if app.file_list_offset > 0 {
                title = format!("▲▲▲ {} ▲▲▲", title);
            }
            if app.file_list_offset + 20 < entries.len() {
                title = format!("{} ▼▼▼", title);
            }

            let rows: Vec<Row> = visible_entries.iter().map(|(idx, entry)| {
                // Format file size in a more readable way (KB, MB, GB)
                let size_str = if entry.size < 1024 {
                    format!("{} B", entry.size)
                } else if entry.size < 1024 * 1024 {
                    format!("{:.2} KB", entry.size as f64 / 1024.0)
                } else if entry.size < 1024 * 1024 * 1024 {
                    format!("{:.2} MB", entry.size as f64 / (1024.0 * 1024.0))
                } else {
                    format!("{:.2} GB", entry.size as f64 / (1024.0 * 1024.0 * 1024.0))
                };

                // Highlight the selected file
                let style = if *idx == app.selected_file_index && app.focus == crate::PanelFocus::Right {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                Row::new(vec![
                    Span::styled(entry.name.clone(), style),
                    Span::styled(entry.path.clone(), style),
                    Span::styled(size_str, style)
                ])
            }).collect();

            // Set different block style based on focus
            let right_block_style = if app.focus == crate::PanelFocus::Right {
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let table = Table::new(rows)
                .header(
                    Row::new(vec!["Name", "Path", "File Size"])
                        .style(Style::default().fg(Color::LightBlue))
                        .bottom_margin(1),
                )
                .block(Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(right_block_style))
                .widths(&[
                    Constraint::Percentage(30),
                    Constraint::Percentage(50),
                    Constraint::Percentage(20),
                ]);
            f.render_widget(table, right_chunks[0]);
        } else {
            // Set different block style based on focus
            let right_block_style = if app.focus == crate::PanelFocus::Right {
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let right_panel = Paragraph::new(right_content)
                .block(Block::default()
                    .borders(Borders::ALL)
                    .title("Files & Folders")
                    .border_style(right_block_style));
            f.render_widget(right_panel, right_chunks[0]);
        }

        // Right bottom panel - Only show scan progress when in scan mode
        if app.scan_progress.in_progress || matches!(mode, AppMode::FullScan { .. }) {
            // Full scan in progress - show detailed progress
            let progress_percent = if app.scan_progress.total_bytes > 0 {
                (app.scan_progress.scanned_bytes as f64 / app.scan_progress.total_bytes as f64 * 100.0) as u16
            } else {
                0
            };

            // Format sizes in a readable way
            let scanned_str = if app.scan_progress.scanned_bytes < 1024 * 1024 {
                format!("{:.2} KB", app.scan_progress.scanned_bytes as f64 / 1024.0)
            } else if app.scan_progress.scanned_bytes < 1024 * 1024 * 1024 {
                format!("{:.2} MB", app.scan_progress.scanned_bytes as f64 / (1024.0 * 1024.0))
            } else {
                format!("{:.2} GB", app.scan_progress.scanned_bytes as f64 / (1024.0 * 1024.0 * 1024.0))
            };

            let total_str = if app.scan_progress.total_bytes < 1024 * 1024 {
                format!("{:.2} KB", app.scan_progress.total_bytes as f64 / 1024.0)
            } else if app.scan_progress.total_bytes < 1024 * 1024 * 1024 {
                format!("{:.2} MB", app.scan_progress.total_bytes as f64 / (1024.0 * 1024.0))
            } else {
                format!("{:.2} GB", app.scan_progress.total_bytes as f64 / (1024.0 * 1024.0 * 1024.0))
            };

            // Progress bar
            let label = format!("Scanned: {} / {} ({}%)", scanned_str, total_str, progress_percent);
            let gauge = Gauge::default()
                .block(Block::default().borders(Borders::ALL).title("Full Scan Progress"))
                .gauge_style(Style::default().fg(Color::Cyan).bg(Color::Black))
                .percent(progress_percent)
                .label(Span::raw(label));

            let scan_stats = format!(
                "Files processed: {}\nPress 'q' to quit or 'c' to cancel scan",
                app.scan_progress.files_processed
            );

            // Create a vertical layout for the gauge and stats text
            let progress_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
                .split(right_chunks[1]);

            f.render_widget(gauge, progress_chunks[0]);

            let stats_paragraph = Paragraph::new(scan_stats)
                .block(Block::default().borders(Borders::ALL).title("Scan Statistics"));
            f.render_widget(stats_paragraph, progress_chunks[1]);
        } else if let AppMode::FullScan { spinner_index, .. } = mode {
            // Full scan is initializing
            let spinner = spinner_chars[*spinner_index];
            let text = format!("{} Preparing full scan...", spinner);
            let paragraph = Paragraph::new(text)
                .block(Block::default().borders(Borders::ALL).title("Full Scan"));
            f.render_widget(paragraph, right_chunks[1]);
        } else if app.focus == crate::PanelFocus::Right && (app.file_entries.is_some() || app.full_scan_results.is_some()) {
            // Show file operations help when files are displayed and right panel is focused
            let help_text = "\n\n- Press 'd' to delete file\n- Press 'c' to copy file\n- Press 'm' to move file\n- Press 'S' for full scan and size sorting";
            let paragraph = Paragraph::new(help_text)
                .block(Block::default().borders(Borders::ALL).title("File Operations"));
            f.render_widget(paragraph, right_chunks[1]);
        }
        // No else condition - hide panel when not needed

        let file_op_keys = if app.focus == crate::PanelFocus::Right && (app.file_entries.is_some() || app.full_scan_results.is_some()) {
            "File operations: Up/Down = navigate, d = delete, c = copy, m = move"
        } else {
            ""
        };

        let legend_text = format!(
            "Keys: j/k = up/down, Ctrl-l/Ctrl-h = switch panels, r = refresh, q = quit, e = eject, s = scan, S = full scan\n{}",
            file_op_keys
        );
        // Use smaller text for the legend
        let legend_text_spans = Spans::from(vec![
            Span::styled(legend_text, Style::default().add_modifier(Modifier::ITALIC).fg(Color::Gray))
        ]);

        let legend = Paragraph::new(legend_text_spans)
            .block(Block::default().borders(Borders::ALL).title("Legend"));
        f.render_widget(legend, outer_chunks[1]);

        match mode {
            AppMode::ConfirmEject(index) => {
                if let Some(device) = app.devices.get(*index) {
                    let popup_area = centered_rect(60, 20, size);

                    // First, render a clear background to make it fully opaque
                    f.render_widget(Clear, popup_area);

                    let text = format!(
                        "Are you sure you want to eject this device?\n(Device: {})\nPress Y to confirm, N to cancel.",
                        device.name
                    );
                    let block = Block::default()
                        .borders(Borders::ALL)
                        .title("Confirm Eject")
                        .style(Style::default().fg(Color::White).bg(Color::DarkGray));
                    let paragraph = Paragraph::new(text).block(block);
                    f.render_widget(paragraph, popup_area);
                }
            },
            AppMode::Ejected(msg) => {
                let popup_area = centered_rect(60, 20, size);

                // Clear the background first
                f.render_widget(Clear, popup_area);

                let text = format!("{}\nPress any key to continue.", msg);
                let block = Block::default()
                    .borders(Borders::ALL)
                    .title("Ejection Result")
                    .style(Style::default().fg(Color::White).bg(Color::DarkGray));
                let paragraph = Paragraph::new(text).block(block);
                f.render_widget(paragraph, popup_area);
            },
            AppMode::ConfirmFileOp { op_type, file_index, target_path } => {
                // First get the correct file based on the stored index
                let file_option = if let Some(ref entries) = app.full_scan_results {
                    if *file_index < entries.len() {
                        Some(&entries[*file_index])
                    } else {
                        None
                    }
                } else if let Some(ref entries) = app.file_entries {
                    if *file_index < entries.len() {
                        Some(&entries[*file_index])
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Some(file) = file_option {
                    let popup_area = centered_rect(70, 30, size);

                    // Clear the background first
                    f.render_widget(Clear, popup_area);

                    let (title, message) = match op_type {
                        crate::FileOperation::Copy => {
                            // Fix temporary value issue by creating a longer-lived default string
                            let default_dest = "destination".to_string();
                            let target = target_path.as_ref().unwrap_or(&default_dest);
                            (
                                "Confirm Copy",
                                format!(
                                    "Are you sure you want to copy this file?\n\nSource: {}\nDestination: {}\n\nPress Y to confirm, N to cancel.",
                                    file.path, target
                                )
                            )
                        },
                        crate::FileOperation::Move => {
                            // Fix temporary value issue by creating a longer-lived default string
                            let default_dest = "destination".to_string();
                            let target = target_path.as_ref().unwrap_or(&default_dest);
                            (
                                "Confirm Move",
                                format!(
                                    "Are you sure you want to move this file?\n\nSource: {}\nDestination: {}\n\nPress Y to confirm, N to cancel.",
                                    file.path, target
                                )
                            )
                        },
                        crate::FileOperation::Delete => (
                            "Confirm Delete",
                            format!(
                                "Are you sure you want to delete this file?\n\nFile: {}\n\nThis action cannot be undone!\n\nPress Y to confirm, N to cancel.",
                                file.path
                            )
                        ),
                    };

                    let block = Block::default()
                        .borders(Borders::ALL)
                        .title(title)
                        .style(Style::default().fg(Color::White).bg(Color::DarkGray));
                    let paragraph = Paragraph::new(message).block(block);
                    f.render_widget(paragraph, popup_area);
                }
            },
            _ => {}
        }

        // Show help popup if enabled
        if app.show_help {
            let help_area = centered_rect(70, 70, size);

            // Clear the background first
            f.render_widget(Clear, help_area);

            let help_text = "
            LAZYSMG KEYBOARD SHORTCUTS

Navigation:
-----------
j, Down       : Move down in current panel
k, Up         : Move up in current panel
Ctrl+h        : Focus left panel (devices)
Ctrl+l        : Focus right panel (files)
?             : Show/hide this help screen

Device Operations:
-----------------
r             : Refresh device list
e             : Eject selected device (if ejectable)

File Operations (when right panel is focused):
--------------------------------------------
s             : Scan current directory (non-recursive)
S             : Full device scan with progress bar
d             : Delete selected file (requires confirmation)
c             : Copy selected file (requires confirmation)
m             : Move selected file (requires confirmation)

General:
-------
q             : Quit application
            ";

            let help_paragraph = Paragraph::new(help_text)
                .block(Block::default()
                    .borders(Borders::ALL)
                    .title("Help (press ? to close)")
                    .border_style(Style::default().fg(Color::Cyan))
                    .style(Style::default().bg(Color::DarkGray)))
                .style(Style::default().fg(Color::White));

            f.render_widget(help_paragraph, help_area);
        }
    })?;
    Ok(())
}
